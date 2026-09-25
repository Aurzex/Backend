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
//! 断言:
//! - **产物不变(第一职责)**:与 `tests/fixtures/translate/convert_bench_baseline.json`
//!   逐项 SHA256 相同;该文件不存在时**写入它**并提示(首次跑即建立基线)。
//!   `deterministic_ids(true)` 下重复跑同一份输入,只要产物有半点不确定就会撞上这条。
//! - **并发不改产物(第二职责)**:每个样本再用 `entity_concurrency = [`PARALLEL_FACTOR`]`
//!   跑一遍,与并发 1 的 SHA256 必须相同 —— 这是实体级并行(方案 25 S3a)的核心门;
//!   同时它的 `core`/`e2e` 列就是加速比证据(绑核、5 轮取最小)。
//!
//! 性能可以变,产物不能变 —— 所以基准的第一职责是守住 SHA256。
//!
//! 测量纪律:这台机器(笔记本/CPU 调频)上**绝对毫秒会漂**(同一二进制两次跑
//! `core` 差 20–40% 是常事),所以:
//! - 每个样本取 `RUNS` 次的**最小值**(≈最少干扰);
//! - 判断"新旧谁快"要在**同一轮内并排比**(见 `convert_facade_flow_bench`),
//!   跨轮比绝对值只能看量级,不能当结论。

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    target: TargetEditor,
    /// 目标编辑器标识(用于产物扩展名/基线键)
    slug: &'static str,
}

const SAMPLES: &[Sample] = &[
    Sample {
        label: "kitten4-10.8MB",
        path: "download/compile/原气骑士 且听风吟_136021231.bcm4",
        target: TargetEditor::KittenN,
        slug: "kn",
    },
    Sample {
        label: "kitten4-0.3MB",
        path: "download/compile/几何对战-联机_215246857.bcm4",
        target: TargetEditor::KittenN,
        slug: "kn",
    },
    Sample {
        label: "kn-9.4MB",
        path: "download/convert/Phigros 自制谱模拟器_195038626.kn.bcmkn",
        target: TargetEditor::Kitten4,
        slug: "kitten4",
    },
    Sample {
        label: "kn-3.7MB",
        path: "download/compile/HEX Editor_317683843.bcmkn",
        target: TargetEditor::Kitten4,
        slug: "kitten4",
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
            TranslateOptions::new()
                .output_dir(dir)
                .deterministic_ids(true)
                .keep_source(false)
                .entity_concurrency(entity_concurrency),
        )
        .expect("转化失败");
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
        TranslateOptions::new()
            .output_dir(dir)
            .deterministic_ids(true)
            .keep_source(false)
            .entity_concurrency(entity_concurrency),
    );
}

#[test]
#[ignore = "性能基准:需 --profile bench_perf 且本机有 download/ 真作品样本"]
fn convert_bench() {
    if cfg!(debug_assertions) {
        eprintln!("[convert_bench] 这是 debug 构建,数字会误导;请用 --profile bench_perf 运行");
        return;
    }
    let missing: Vec<&str> = SAMPLES
        .iter()
        .filter(|s| !Path::new(s.path).exists())
        .map(|s| s.path)
        .collect();
    if missing.len() == SAMPLES.len() {
        eprintln!("[convert_bench] 缺样本(需先反编译作品到 download/),跳过。缺:{missing:#?}");
        return;
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
    let mut fresh = serde_json::Map::new();
    let mut mismatched = Vec::new();
    let mut parallel_mismatched = Vec::new();

    for sample in SAMPLES {
        if !Path::new(sample.path).exists() {
            println!("| {} | (缺样本,跳过) | | | | | | | | | |", sample.label);
            continue;
        }
        let src_bytes = std::fs::metadata(sample.path).expect("stat 失败").len();
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
            if p.sha256 == m.sha256 { "相同 ✅" } else { "不同 ❌" },
        );

        // 空门守卫:若本机可用核数 ≥ 2,则正向样本的"实体并发=8"必须真的开起多线程,
        // 否则这一行只是"串行 vs 串行",SHA 相同毫无意义(`taskset -c 2` 就会这样:
        // `available_parallelism` 按亲和掩码算,单核下会被折成 1)。
        if available >= 2 && sample.target == TargetEditor::KittenN && p.entity_workers <= 1 {
            panic!(
                "{}:实体并发={PARALLEL_FACTOR} 实际只开了 {} 个线程(正向样本应并行)——  并发对照成了空门",
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
        fresh.insert(
            key.clone(),
            serde_json::Value::String(m.sha256.clone()),
        );
        fresh.insert(
            format!("{key}#meta"),
            serde_json::json!({
                "source_bytes": src_bytes,
                "output_bytes": m.out_bytes,
                "blocks_total": m.blocks_total,
                "blocks_converted": m.blocks_converted,
                "warnings": m.warnings,
            }),
        );
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

    if mismatched.is_empty() && !fresh.is_empty() {
        if baseline.is_empty() {
            std::fs::write(BASELINE, serde_json::to_string_pretty(&fresh).unwrap())
                .expect("写基线失败");
            println!("\n[convert_bench] 首次运行:基线已写入 {BASELINE}");
        } else {
            println!(
                "\n[convert_bench] 产物 SHA256 与基线一致 ✅;实体级并发 1 vs {PARALLEL_FACTOR} 同 SHA256 ✅"
            );
        }
        return;
    }

    if !mismatched.is_empty() {
        eprintln!("\n[convert_bench] 产物与基线不一致(性能改了但产物变了 = 失败):");
        for (key, old, new) in &mismatched {
            eprintln!("  {key}\n    基线 {old}\n    现在 {new}");
        }
        panic!("convert 基准:产物 SHA256 与基线不一致");
    }
}

fn load_baseline() -> serde_json::Map<String, serde_json::Value> {
    let Ok(text) = std::fs::read_to_string(BASELINE) else {
        return serde_json::Map::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}
