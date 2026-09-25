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

fn measure(sample: &Sample, dir: &Path) -> Measured {
    // 预热一轮(不计入):首个样本会吃冷缓存/调频爬升,不预热的话它最不可信
    warmup(sample, dir);

    let mut read = Vec::new();
    let mut parse = Vec::new();
    let mut core = Vec::new();
    let mut ser = Vec::new();
    let mut e2e = Vec::new();
    let mut last: Option<(std::path::PathBuf, String, usize, usize, usize, u64)> = None;

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
                .keep_source(false),
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
        ));
    }

    let (_, sha256, blocks_total, blocks_converted, warnings, out_bytes) = last.unwrap();
    Measured {
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
fn warmup(sample: &Sample, dir: &Path) {
    let _ = translate_file(
        Path::new(sample.path),
        sample.target,
        TranslateOptions::new()
            .output_dir(dir)
            .deterministic_ids(true)
            .keep_source(false),
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

    let baseline = load_baseline();
    let mut fresh = serde_json::Map::new();
    let mut mismatched = Vec::new();

    for sample in SAMPLES {
        if !Path::new(sample.path).exists() {
            println!("| {} | (缺样本,跳过) | | | | | | | | | |", sample.label);
            continue;
        }
        let src_bytes = std::fs::metadata(sample.path).expect("stat 失败").len();
        let m = measure(sample, &dir);
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

        let key = format!("{}-{}", sample.label, sample.slug);
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

    if mismatched.is_empty() && !fresh.is_empty() {
        if baseline.is_empty() {
            std::fs::write(BASELINE, serde_json::to_string_pretty(&fresh).unwrap())
                .expect("写基线失败");
            println!("\n[convert_bench] 首次运行:基线已写入 {BASELINE}");
        } else {
            println!("\n[convert_bench] 产物 SHA256 与基线一致 ✅");
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
