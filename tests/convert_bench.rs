//! convert 域性能基准(方案 23 §5「基准方法(避免自欺)」)。
//!
//! **只在 release 下有意义**:debug 构建会把 `serde_json` 与字符串操作拖到不成比例,
//! 得出的结论会指向错误的优化方向。此外发布档是 `opt-level = "z"`(为体积),
//! 不适合量测热路径,故用专用档 `bench_perf`(见 `Cargo.toml`,不改动发布档):
//!
//! ```bash
//! cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture
//! ```
//!
//! 每个样本(真作品产物,位于 gitignored 的 `download/`)打印:
//! `read`(读盘)/ `parse`(JSON→Value)/ `core`(转化内核,取自 `report.elapsed_ms`)/
//! `ser`(Value→JSON)/ `e2e`(translate_file 全程,含自身读写与序列化)/
//! 产物字节数 / 积木数 / 告警数 / 产物 SHA256。
//!
//! 断言(三条,**产物不变**是核心):
//! - **产物不变**:与 `tests/fixtures/translate/convert_bench_baseline.json` 逐项 SHA256 相同;
//!   另外 `#meta`(源文件 SHA256 / 产物字节 / 块数 / 告警数)也参与断言 —— "字节没变但报告退化
//!   (块变少、告警变多)"与"输入被换掉"都必须被抓住。
//! - **并发不改产物(第二职责)**:每个样本再用 `entity_concurrency = [`PARALLEL_FACTOR`]`
//!   跑一遍,与并发 1 的 SHA256 必须相同 —— 这是实体级并行(方案 25 S3a)的核心门;
//!   同时它的 `core`/`e2e` 列就是加速比证据(绑核、5 轮取最小)。
//!
//! 性能可以变,产物不能变 —— 所以基准的第一职责是守住 SHA256。
//!
//! **两个环境开关**:
//! - `BACKEND_REQUIRE_BENCH=1`:**严格模式** —— debug 构建、样本缺失**直接失败**
//!   (默认这两条是"打印后 return 显示 pass",CI/干净检出上等于没有这条门);
//! - `BACKEND_BENCH_REFRESH=1`:**有据刷新基线** —— 写出新的基线文件,并**逐键打印变化**
//!   (产物/元信息),让"为什么变"在提交信息里可审计。
//!
//! **基线缺失(W3a)**:默认模式下也**直接失败** —— 曾经的"NotFound ⇒ 返回空 map + 首次运行
//! 自动写盘"让门在「基线被删 / 全新检出」这两种它必须挡住的情形下静默消失。建基线的唯一通道
//! 是显式 `BACKEND_BENCH_REFRESH=1`(四条路径见 [`load_baseline`])。
//!
//! **重刷不许丢键(W3e)**:`REFRESH` 写出的 `fresh` 只由**存在的**样本构成 ⇒ 样本缺失时重刷会
//! 把缺样本的基线键**永久删掉**(而默认模式下样本缺失只是打印一行警告)⇒ 门的覆盖面会静默缩水。
//! 所以样本缺失时**拒绝写盘**并逐键列出"将被删掉的键"。
//!
//! **NEMO 样本(W6)**:`SAMPLES` 里的 `nemo-3.4MB` 是 NEMO 真作品(源编辑器按**内容**判定),
//! 目标是 KN;NEMO 的版本迁移只由 `TranslateOptions::source_version` 驱动,而 `translate_file`
//! **不会**自动带上它(那是域门面 `translate_work` 的行为)⇒ 该参数经 [`Sample::source_version`]
//! 逐样本透传,并作为 `#meta.source_version` 记进基线。不传就把"未迁移"的产物锁成基线:
//! 门是绿的,证的东西却是错的。
//!
//! 测量纪律:这台机器(笔记本/CPU 调频)上**绝对毫秒会漂**(同一二进制两次跑
//! `core` 差 20–40% 是常事),所以:
//! - 每个样本取 `RUNS` 次的**最小值**(≈最少干扰);
//! - 判断"新旧谁快"要在**同一轮内并排比**(见 `convert_facade_flow_bench`),
//!   跨轮比绝对值只能看量级,不能当结论。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use backend::core::convert::EditorType;
use backend::core::convert::translate::{TargetEditor, TranslateOptions, translate_file};
use sha2::{Digest, Sha256};

/// 每个样本重复次数(取**最小值**:CPU 基准里最小值≈最少干扰,比中位数稳)
const RUNS: usize = 5;

/// 实体级并发的对照值(方案 25 S3a):同一输入在 1 与 8 下产物必须逐字节相同。
/// 实际线程数还会被工作项数与可用核数夹取(见 `remint::workers`)
const PARALLEL_FACTOR: usize = 8;

/// 样本 = 仓库里的真作品产物(gitignored,缺失即整测试跳过)
struct Sample {
    label: &'static str,
    path: &'static str,
    /// 源编辑器(`detect_editor` 按**内容**判定;用于"正向 Kitten4→KN 才有实体级并行"的守卫)
    source_editor: EditorType,
    target: TargetEditor,
    /// 目标编辑器标识(用于产物扩展名/基线键)
    slug: &'static str,
    /// 源作品的 `bcm_version`(NEMO 版本迁移的**唯一**驱动;`None` = 不迁移)
    ///
    /// NEMO 的编辑版文档里没有 `bcm_version`(`translate_file` 也不会自动带上 —— 那是域门面
    /// `translate_work` 的行为)⇒ 不给这一列,门锁住的就是"未迁移"口径的产物:门是绿的,
    /// 但证错了东西。值来自作品元信息(`<work_id>.meta` 的 `bcm_version`)。
    source_version: Option<&'static str>,
}

const SAMPLES: &[Sample] = &[
    Sample {
        label: "kitten4-10.8MB",
        path: "download/compile/原气骑士 且听风吟_136021231.bcm4",
        source_editor: EditorType::Kitten4,
        target: TargetEditor::KittenN,
        slug: "kn",
        source_version: None,
    },
    Sample {
        label: "kitten4-0.3MB",
        path: "download/compile/几何对战-联机_215246857.bcm4",
        source_editor: EditorType::Kitten4,
        target: TargetEditor::KittenN,
        slug: "kn",
        source_version: None,
    },
    Sample {
        label: "kn-9.4MB",
        path: "download/convert/Phigros 自制谱模拟器_195038626.kn.bcmkn",
        source_editor: EditorType::Neko,
        target: TargetEditor::Kitten4,
        slug: "kitten4",
        source_version: None,
    },
    Sample {
        label: "kn-3.7MB",
        path: "download/compile/HEX Editor_317683843.bcmkn",
        source_editor: EditorType::Neko,
        target: TargetEditor::Kitten4,
        slug: "kitten4",
        source_version: None,
    },
    Sample {
        // W6:NEMO 侧此前**没有任何字节基线**(SAMPLES 只有 4 个 Kitten 样本)
        label: "nemo-3.4MB",
        path: "download/compile/蛋仔派对2-奥姆返场新盲盒生存赛重做_194684070/user_works/194684070/194684070.bcm",
        source_editor: EditorType::Nemo,
        target: TargetEditor::KittenN,
        slug: "kn",
        // 作品元信息(`…/194684070.meta`)的 `bcm_version`;与 `nemo_tests::REAL_SAMPLES` 同值。
        // 它**等于**迁移目标版本(`nemo_mapping::NEMO_BCM_VERSION = "0.16.2"`)且 ≥ YC 段边界
        // 0.15.0 ⇒ 官方与这里的迁移标志都是 (false, false):产物是"不需要迁移"的官方口径。
        // 这条仍由本字段**显式固定**(而不是靠"没传"的默认);下一件老版本 NEMO 作品才会让
        // 这个参数在字节上显形(本轮已用临时改成 0.14.0 的 A/B 证明它真的驱动管线)。
        source_version: Some("0.16.2"),
    },
];

const BASELINE: &str = "tests/fixtures/translate/convert_bench_baseline.json";

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    // sha2 0.11 的 digest 类型不实现 LowerHex,自行拼十六进制
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, byte| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{byte:02x}");
            acc
        })
}

/// 取最小值(见 RUNS 注释)
fn best(samples: Vec<f64>) -> f64 {
    samples.into_iter().fold(f64::INFINITY, f64::min)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 每个基准测试**独立**的临时目录(cargo 默认并行跑测试函数,共用一个目录会互相删)
fn bench_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backend-convert-bench-{tag}-{}-{:08x}",
        std::process::id(),
        fastrand::u32(..)
    ));
    std::fs::create_dir_all(&dir).expect("创建基准临时目录失败");
    dir
}

/// 单个样本的一次量测结果
struct Measured {
    /// 本次转换实际使用的实体级工作线程数(反向恒 1)
    entity_workers: usize,
    read_ms: f64,
    parse_ms: f64,
    core_ms: f64,
    ser_ms: f64,
    e2e_ms: f64,
    out_bytes: u64,
    blocks_total: usize,
    blocks_converted: usize,
    warnings: usize,
    sha256: String,
}

fn measure(sample: &Sample, dir: &Path, entity_concurrency: usize) -> Measured {
    // 预热一轮(不计入):首个样本会吃冷缓存/调频爬升,不预热的话它最不可信
    warmup(sample, dir, entity_concurrency);

    let mut read = Vec::new();
    let mut parse = Vec::new();
    let mut core = Vec::new();
    let mut ser = Vec::new();
    let mut e2e = Vec::new();
    let mut last: Option<(std::path::PathBuf, String, usize, usize, usize, u64, usize)> = None;

    for _ in 0..RUNS {
        let t = Instant::now();
        let text = std::fs::read_to_string(sample.path).expect("读样本失败");
        read.push(ms(t.elapsed()));

        let t = Instant::now();
        let value: serde_json::Value = serde_json::from_str(&text).expect("解析样本失败");
        parse.push(ms(t.elapsed()));
        drop(value);

        let t = Instant::now();
        let outcome = translate_file(
            Path::new(sample.path),
            sample.target,
            sample_options(sample, dir, entity_concurrency),
        )
        .expect("转化失败");
        // 声明的 `source_editor` 由内容判定(`detect_editor`)复核:样本被换成别的格式/编辑器时,
        // 下面那条"正向 Kitten4 样本的并发必须真的开起来"的空门守卫就不再成立(它按本字段判)
        assert_eq!(
            outcome.report.from, sample.source_editor,
            "{}:声明的 source_editor 与 `detect_editor` 的判定不一致(样本被换了?)",
            sample.label
        );
        e2e.push(ms(t.elapsed()));
        core.push(outcome.report.elapsed_ms as f64);

        let output = std::fs::read_to_string(&outcome.output).expect("读产物失败");
        let t = Instant::now();
        let doc: serde_json::Value = serde_json::from_str(&output).expect("产物不是 JSON");
        ser.push(ms(t.elapsed()));
        drop(doc);

        last = Some((
            outcome.output.clone(),
            sha256_hex(output.as_bytes()),
            outcome.report.blocks_total,
            outcome.report.blocks_converted,
            outcome.report.warnings().len(),
            output.len() as u64,
            outcome.report.entity_workers,
        ));
    }

    let (_, sha256, blocks_total, blocks_converted, warnings, out_bytes, entity_workers) =
        last.unwrap();
    Measured {
        entity_workers,
        read_ms: best(read),
        parse_ms: best(parse),
        core_ms: best(core),
        ser_ms: best(ser),
        e2e_ms: best(e2e),
        out_bytes,
        blocks_total,
        blocks_converted,
        warnings,
        sha256,
    }
}

/// 不计入统计的一轮(见 `measure` 注释)
fn warmup(sample: &Sample, dir: &Path, entity_concurrency: usize) {
    let _ = translate_file(
        Path::new(sample.path),
        sample.target,
        sample_options(sample, dir, entity_concurrency),
    );
}

/// 样本的转换选项:`source_version` 是 NEMO 版本迁移的**唯一**驱动,必须逐样本透传
///
/// 不传就会把"未迁移"的产物锁进基线(门是绿的,证的东西错了,见 [`Sample::source_version`])。
fn sample_options(sample: &Sample, dir: &Path, entity_concurrency: usize) -> TranslateOptions {
    let options = TranslateOptions::new()
        .output_dir(dir)
        .deterministic_ids(true)
        .keep_source(false)
        .entity_concurrency(entity_concurrency);
    match sample.source_version {
        Some(version) => options.source_version(version),
        None => options,
    }
}

#[test]
#[ignore = "性能基准:需 --profile bench_perf 且本机有 download/ 真作品样本"]
fn convert_bench() {
    if cfg!(debug_assertions) {
        if strict_mode() {
            panic!("严格模式:convert_bench 必须在 --profile bench_perf(release)下跑");
        }
        eprintln!("[convert_bench] 这是 debug 构建,数字会误导;请用 --profile bench_perf 运行");
        return;
    }
    let missing: Vec<&str> = SAMPLES
        .iter()
        .filter(|s| !Path::new(s.path).exists())
        .map(|s| s.path)
        .collect();
    if missing.len() == SAMPLES.len() {
        if strict_mode() {
            panic!("严格模式:样本全缺(需先反编译作品到 download/):{missing:#?}");
        }
        eprintln!("[convert_bench] 缺样本(需先反编译作品到 download/),跳过。缺:{missing:#?}");
        return;
    }
    if !missing.is_empty() {
        eprintln!("[convert_bench] 警告:部分样本缺失,这些键不参与断言:{missing:#?}");
    }
    let dir = bench_dir("flow");

    println!(
        "\n| 样本 | 源 MB | read ms | parse ms | core ms | ser ms | e2e ms | 产物 MB | 块(源→产物) | 告警 | SHA256 |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");

    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    println!(
        "\n实体级并发对照(方案 25 S3a):可用核数 {available},请求并发 {PARALLEL_FACTOR}         (实际线程数还会被工作项数与可用核数夹取;反向不支持该选项,应当零差异)"
    );

    let baseline = load_baseline();
    // **不许静默丢键(W3e)**:重刷写出的 `fresh` 只由**存在的**样本构成 ⇒ 样本缺失时重刷会把
    // 缺样本的基线键**永久删掉**,事后从基线文件本身看不出来(而默认模式下样本缺失只是打印一行
    // 警告)⇒ 门的覆盖面静默缩水。所以在量测**之前**就拒绝(连同下面列出的键,别白跑一遍)。
    if refresh_mode() && !missing.is_empty() {
        let mut would_drop: Vec<String> = Vec::new();
        for sample in SAMPLES {
            if Path::new(sample.path).exists() {
                continue;
            }
            let key = format!("{}-{}", sample.label, sample.slug);
            for candidate in [key.clone(), format!("{key}#meta")] {
                if baseline.contains_key(&candidate) {
                    would_drop.push(candidate);
                }
            }
        }
        panic!(
            "BACKEND_BENCH_REFRESH=1 被拒绝:样本缺失 ⇒ 重刷写出的基线只含跑过的样本,\
             会把下列 {} 个基线键**永久删掉**(覆盖面静默缩水,事后看不出来)。\
             先让样本回到磁盘(反编译/转换);若该样本确实要移除,请在同一个提交里手工删掉\
             它的键并写明原因。\n  缺样本:{missing:#?}\n  将被删:{would_drop:#?}",
            would_drop.len()
        );
    }
    let mut fresh = serde_json::Map::new();
    let mut mismatched = Vec::new();
    let mut parallel_mismatched = Vec::new();
    let mut meta_mismatched = Vec::new();

    for sample in SAMPLES {
        if !Path::new(sample.path).exists() {
            println!("| {} | (缺样本,跳过) | | | | | | | | | |", sample.label);
            continue;
        }
        let src_bytes = std::fs::metadata(sample.path).expect("stat 失败").len();
        let source_sha = sha256_hex(&std::fs::read(sample.path).expect("读源文件失败"));
        let m = measure(sample, &dir, 1);
        println!(
            "| {} | {:.1} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1} | {}→{} | {} | {} |",
            sample.label,
            src_bytes as f64 / 1e6,
            m.read_ms,
            m.parse_ms,
            m.core_ms,
            m.ser_ms,
            m.e2e_ms,
            m.out_bytes as f64 / 1e6,
            m.blocks_total,
            m.blocks_converted,
            m.warnings,
            &m.sha256[..16],
        );

        // ── 第二职责:同一输入在并发 1 与 N 下产物必须逐字节相同(实体级并行的核心门)
        let p = measure(sample, &dir, PARALLEL_FACTOR);
        println!(
            "| {} [实体并发={PARALLEL_FACTOR}] | {:.1} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.1} | {}→{} | {} | {} |",
            sample.label,
            src_bytes as f64 / 1e6,
            p.read_ms,
            p.parse_ms,
            p.core_ms,
            p.ser_ms,
            p.e2e_ms,
            p.out_bytes as f64 / 1e6,
            p.blocks_total,
            p.blocks_converted,
            p.warnings,
            &p.sha256[..16],
        );
        println!(
            "└ {} 加速比(并发 1 → {PARALLEL_FACTOR},实际线程 {} → {}):core {:.0} → {:.0} ms({:.2}×),e2e {:.0} → {:.0} ms({:.2}×),产物 SHA256 {}",
            sample.label,
            m.entity_workers,
            p.entity_workers,
            m.core_ms,
            p.core_ms,
            m.core_ms / p.core_ms.max(f64::MIN_POSITIVE),
            m.e2e_ms,
            p.e2e_ms,
            m.e2e_ms / p.e2e_ms.max(f64::MIN_POSITIVE),
            if p.sha256 == m.sha256 {
                "相同 ✅"
            } else {
                "不同 ❌"
            },
        );

        // 空门守卫:若本机可用核数 ≥ 2,则**正向 Kitten4→KN** 样本的"实体并发=8"必须真的开起
        // 多线程,否则这一行只是"串行 vs 串行",SHA 相同毫无意义(`taskset -c 2` 就会这样:
        // `available_parallelism` 按亲和掩码算,单核下会被折成 1)。
        // 只对 Kitten4 源:实体级并行是正向 Kitten4 路径的实现(NEMO→KN 的管线恒串行,恒 1;
        // 反向 KN→Kitten4 同样恒 1),按目标编辑器判会把这两类错算成"空门"。
        if available >= 2 && sample.source_editor == EditorType::Kitten4 && p.entity_workers <= 1 {
            panic!(
                "{}:实体并发={PARALLEL_FACTOR} 实际只开了 {} 个线程(正向 Kitten4→KN 样本应并行)——  并发对照成了空门",
                sample.label, p.entity_workers
            );
        }

        let key = format!("{}-{}", sample.label, sample.slug);
        if p.sha256 != m.sha256 {
            parallel_mismatched.push((key.clone(), m.sha256.clone(), p.sha256.clone()));
        }
        if let Some(old) = baseline.get(&key).and_then(|v| v.as_str())
            && old != m.sha256
        {
            mismatched.push((key.clone(), old.to_string(), m.sha256.clone()));
        }
        fresh.insert(key.clone(), serde_json::Value::String(m.sha256.clone()));
        let mut meta = serde_json::json!({
            "source_sha256": source_sha,
            "source_bytes": src_bytes,
            "output_bytes": m.out_bytes,
            "blocks_total": m.blocks_total,
            "blocks_converted": m.blocks_converted,
            "warnings": m.warnings,
        });
        // NEMO 的口径进基线:该样本是靠 `source_version` 驱动迁移的,不记下来就分不清
        // "这份 SHA 是迁移后的产物"还是"参数没接上、被锁成未迁移"(只给有版本的样本加键 ⇒
        // 既有 Kitten 样本的 `#meta` 一字不变)。
        if let Some(version) = sample.source_version {
            meta["source_version"] = serde_json::Value::String(version.to_string());
        }
        // 元信息也参与断言:字节没变但块数/告警退化、或输入被换掉,都要抓
        if let Some(old) = baseline.get(&format!("{key}#meta"))
            && old != &meta
        {
            meta_mismatched.push((key.clone(), old.clone(), meta.clone()));
        }
        fresh.insert(format!("{key}#meta"), meta);
    }

    if !parallel_mismatched.is_empty() {
        eprintln!(
            "\n[convert_bench] 实体级并发改变了产物(并发 1 与 {PARALLEL_FACTOR} 的 SHA256 不一致 = 失败):"
        );
        for (key, serial, parallel) in &parallel_mismatched {
            eprintln!("  {key}\n    并发 1 {serial}\n    并发 {PARALLEL_FACTOR} {parallel}");
        }
        panic!("convert 基准:实体级并发改变了产物");
    }

    // 有据重刷:显式开关才写,并逐键打印变化(让"为什么变"可审计)。
    // (样本缺失时根本到不了这里 —— 上面 W3e 的守卫已经拒绝写盘并列出会被删的键)
    if refresh_mode() {
        eprintln!("\n[convert_bench] BACKEND_BENCH_REFRESH=1:**重刷基线** —— 变化逐键列出:");
        for key in fresh.keys() {
            let old = baseline.get(key);
            let new = fresh.get(key);
            if old != new {
                eprintln!("  {key}\n    旧 {old:?}\n    新 {new:?}");
            }
        }
        std::fs::write(BASELINE, serde_json::to_string_pretty(&fresh).unwrap())
            .expect("写基线失败");
        println!("[convert_bench] 基线已写入 {BASELINE}(记得在提交信息里写清原因)");
        return;
    }

    if !meta_mismatched.is_empty() {
        eprintln!("\n[convert_bench] 元信息与基线不一致(字节可能没变,但**报告退化或输入被换**):");
        for (key, old, new) in &meta_mismatched {
            eprintln!("  {key}\n    基线 {old}\n    现在 {new}");
        }
        panic!("convert 基准:元信息(source SHA/块数/告警/字节)与基线不一致");
    }

    if !mismatched.is_empty() {
        eprintln!("\n[convert_bench] 产物与基线不一致(性能改了但产物变了 = 失败):");
        for (key, old, new) in &mismatched {
            eprintln!("  {key}\n    基线 {old}\n    现在 {new}");
        }
        panic!("convert 基准:产物 SHA256 与基线不一致");
    }

    // 走到这里基线必然非空且对得上:缺失在 `load_baseline` 就炸了(W3a),`REFRESH` 已在上面的分支
    // 写完并 return。原先这里的"首次运行 ⇒ 自动写基线"分支已删 —— 那是默认模式下的静默重建。
    println!(
        "\n[convert_bench] 产物 SHA256 与元信息都与基线一致 ✅;实体级并发 1 vs {PARALLEL_FACTOR} 同 SHA256 ✅"
    );
}

/// 读基线。**四条路径(W3a)**:
/// 1. 存在且能解析 ⇒ 正常返回(门照常比);
/// 2. 存在但解析失败 ⇒ **一律炸**(不因 `REFRESH` 放宽:被手改坏的基线不该被"顺手重刷"掩盖;
///    真要从头重建就**先删文件**再 `BACKEND_BENCH_REFRESH=1`,让"重建"在 diff 里看得见);
/// 3. `NotFound` + `BACKEND_BENCH_REFRESH=1` ⇒ 返回空 map —— "从零建基线"的**唯一合法通道**
///    (必须放在这里判:`load_baseline` 在 REFRESH 分支之前调用,无条件炸会把首跑自己堵死);
/// 4. `NotFound` + 其它(默认 / 严格模式)⇒ **炸**。原先这里是"返回空 map,再由后半段
///    `baseline.is_empty()` 分支静默写盘" —— 那等于把门在「基线被删 / 全新检出」时关掉。
fn load_baseline() -> serde_json::Map<String, serde_json::Value> {
    let text = match std::fs::read_to_string(BASELINE) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if refresh_mode() {
                return serde_json::Map::new();
            }
            panic!(
                "基线 {BASELINE} 缺失(严格模式 = {})—— 建基线请显式跑一次 BACKEND_BENCH_REFRESH=1;\
                 默认模式不再静默重建(那会让门在「基线被删 / 全新检出」时消失)",
                strict_mode()
            );
        }
        Err(e) => panic!("读基线 {BASELINE} 失败:{e}"),
    };
    // 基线损坏必须炸:静默 `unwrap_or_default()` 会把"基线被删/弄坏"变成"悄悄重建基线",
    // 第一职责门就白设了。注意这条**不因 `BACKEND_BENCH_REFRESH=1` 放宽** —— 要重建就先删文件
    // 再重刷,让"重建"这件事在 diff 里看得见。
    match serde_json::from_str(&text) {
        Ok(map) => map,
        Err(e) => panic!(
            "基线 {BASELINE} 解析失败(不要手改;确要重建请先删掉该文件再用 BACKEND_BENCH_REFRESH=1 跑一次):{e}"
        ),
    }
}

/// `BACKEND_REQUIRE_BENCH=1`:把"静默跳过"变成失败(见模块文档)
fn strict_mode() -> bool {
    std::env::var("BACKEND_REQUIRE_BENCH").is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// `BACKEND_BENCH_REFRESH=1`:有据重刷基线(打印逐键变化;不写就永远不写)
fn refresh_mode() -> bool {
    std::env::var("BACKEND_BENCH_REFRESH").is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}
