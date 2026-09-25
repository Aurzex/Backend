//! 域门面路径对照基准(方案 23 P0-2)
//!
//! `translate_work` 原本是"反编译落盘 → `translate_file` 读回 → 上传源文件 →
//! `set_source_reference` 再把产物读回-改写-写回"。本基准把两条流程放到**同一轮**
//! 里并排量测(跨轮比绝对值不可信,见 `tests/convert_bench.rs` 的测量纪律),
//! 并断言**产物 SHA256 相同**:性能变了,产物没变。
//!
//! ```bash
//! cargo test --profile bench_perf --test convert_facade_bench -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use backend::core::convert::translate::{
    TargetEditor, TranslateOptions, set_source_reference, set_source_reference_in, translate_file,
    translate_value,
};
use sha2::{Digest, Sha256};

const RUNS: usize = 5;

struct Sample {
    label: &'static str,
    path: &'static str,
    target: TargetEditor,
}

const SAMPLES: &[Sample] = &[
    Sample {
        label: "kitten4-10.8MB",
        path: "download/compile/原气骑士 且听风吟_136021231.bcm4",
        target: TargetEditor::KittenN,
    },
    Sample {
        label: "kitten4-0.3MB",
        path: "download/compile/几何对战-联机_215246857.bcm4",
        target: TargetEditor::KittenN,
    },
    Sample {
        label: "kn-9.4MB",
        path: "download/convert/Phigros 自制谱模拟器_195038626.kn.bcmkn",
        target: TargetEditor::Kitten4,
    },
    Sample {
        label: "kn-3.7MB",
        path: "download/compile/HEX Editor_317683843.bcmkn",
        target: TargetEditor::Kitten4,
    },
];

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut acc, byte| {
            use std::fmt::Write as _;
            let _ = write!(acc, "{byte:02x}");
            acc
        })
}

fn best(samples: Vec<f64>) -> f64 {
    samples.into_iter().fold(f64::INFINITY, f64::min)
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn bench_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backend-convert-bench-{tag}-{}-{:08x}",
        std::process::id(),
        fastrand::u32(..)
    ));
    std::fs::create_dir_all(&dir).expect("创建基准临时目录失败");
    dir
}

const FAKE_SOURCE_URL: &str = "https://creation.codemao.cn/neko/bcm/bench-source.bcm4";

fn product_name(source: &str, target: TargetEditor) -> String {
    format!(
        "{}.{}.{}",
        Path::new(source)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("out"),
        if target == TargetEditor::KittenN {
            "kn"
        } else {
            "kitten4"
        },
        if target == TargetEditor::KittenN {
            "bcmkn"
        } else {
            "bcm4"
        }
    )
}

/// 旧流程:文件进、文件出,再"读回-改写-写回"补源引用
fn legacy_flow(source: &str, target: TargetEditor, dir: &Path) -> (f64, String) {
    let t = Instant::now();
    let outcome = translate_file(
        Path::new(source),
        target,
        TranslateOptions::new()
            .output_dir(dir)
            .deterministic_ids(true)
            .keep_source(false),
    )
    .expect("旧流程转化失败");
    set_source_reference(&outcome.output, FAKE_SOURCE_URL).expect("旧流程写源引用失败");
    let bytes = std::fs::read(&outcome.output).expect("读产物失败");
    (ms(t.elapsed()), sha256_hex(&bytes))
}

/// 新流程(`translate_work` 实际走的路径):读源 → 内存转化 → 内存写引用 → 一次落盘
fn memory_flow(source: &str, target: TargetEditor, dir: &Path) -> (f64, String) {
    let t = Instant::now();
    let text = std::fs::read_to_string(source).expect("读源失败");
    let document: serde_json::Value = serde_json::from_str(&text).expect("解析源失败");
    let options = TranslateOptions::new()
        .output_dir(dir)
        .deterministic_ids(true)
        .keep_source(false);
    let mut converted = translate_value(document, target, &options).expect("内存转化失败");
    set_source_reference_in(&mut converted.document, FAKE_SOURCE_URL).expect("写源引用失败");
    let output = dir.join(product_name(source, target));
    let file = std::fs::File::create(&output).expect("建产物失败");
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &converted.document).expect("写产物失败");
    use std::io::Write as _;
    writer.flush().expect("flush 失败");
    let bytes = std::fs::read(&output).expect("读产物失败");
    (ms(t.elapsed()), sha256_hex(&bytes))
}

#[test]
#[ignore = "性能基准:域门面路径对照,需 --profile bench_perf"]
fn convert_facade_flow_bench() {
    if cfg!(debug_assertions) {
        eprintln!("[facade_flow] debug 构建,跳过;请用 --profile bench_perf");
        return;
    }
    let dir = bench_dir("facade");
    println!("
| 样本 | 旧流程 ms(盘→盘 + 读回改写) | 内存直通 ms | 加速 | 产物 SHA256 |");
    println!("| --- | --- | --- | --- | --- |");
    for sample in SAMPLES {
        if !Path::new(sample.path).exists() {
            continue;
        }
        let mut old_runs = Vec::new();
        let mut new_runs = Vec::new();
        let mut old_hash = String::new();
        let mut new_hash = String::new();
        for _ in 0..RUNS {
            let (t, h) = legacy_flow(sample.path, sample.target, &dir);
            old_runs.push(t);
            old_hash = h;
            let (t, h) = memory_flow(sample.path, sample.target, &dir);
            new_runs.push(t);
            new_hash = h;
        }
        let (old_ms, new_ms) = (best(old_runs), best(new_runs));
        assert_eq!(
            old_hash, new_hash,
            "{}:两条流程产物必须逐字节相同",
            sample.label
        );
        println!(
            "| {} | {:.0} | {:.0} | {:.2}× | {} ✅ |",
            sample.label,
            old_ms,
            new_ms,
            old_ms / new_ms,
            &old_hash[..16],
        );
    }
}
