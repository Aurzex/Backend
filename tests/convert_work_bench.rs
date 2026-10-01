//! `translate_work`(作品 id → 抓取 → 反编译 → 转化 → 落盘)的端到端基准。
//!
//! 补这条的原因(rounds/37 §0.5 ③):此前只有 `translate_file`(从**本地文件**开始)有基准,
//! 而 P1(关掉 `save_raw`)这类改动只作用在 `translate_work` 这条路上 ⇒ 收益无处可量。
//!
//! 用法:
//! ```text
//! WORK_ID=273988379 cargo test --profile bench_perf --test convert_work_bench -- --ignored --nocapture
//! ```
//!
//! 判读纪律与 `convert_bench` 相同:绝对毫秒会漂(20–40%),**只能同一轮内并排比**
//! (本文件打印每轮的耗时与产物 SHA256,配合 `git worktree` 做"改动前/后交替跑")。
//!
//! 需要网络 + `data/test-config.json`(账号);缺配置时**跳过**
//! (`BACKEND_REQUIRE_FIXTURES=1` 或 `BACKEND_REQUIRE_BENCH=1` 下改为失败)。

use std::path::PathBuf;
use std::time::Instant;

use backend::core::convert::translate::{TargetEditor, TranslateOptions, translate_file};
use backend::core::convert::{WorkId, translate_work};
use sha2::{Digest, Sha256};

/// 每档重复次数(取最小值:CPU 基准里最小值≈最少干扰)
const RUNS: usize = 3;

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

/// 缺夹具/配置时**失败**而不是跳过:`BACKEND_REQUIRE_FIXTURES=1`(统一开关 —— 与
/// `tests/convert_bench.rs`、`tests/convert_facade_bench.rs`、
/// `src/core/convert/translate/reverse_tests.rs` 同一口径,见 `docs/knowledge/repo-conventions.md` §3ter)
/// 或本文件历史上用的 `BACKEND_REQUIRE_BENCH=1`(保留,别让已有环境悄悄变松)。
fn strict_mode() -> bool {
    fn env_flag(name: &str) -> bool {
        std::env::var(name).is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
    }
    env_flag("BACKEND_REQUIRE_FIXTURES") || env_flag("BACKEND_REQUIRE_BENCH")
}

/// 登录(与其它真机测试同一份配置;失败时按严格模式决定跳过还是炸)
fn login_or_skip() -> bool {
    let cfg_path = std::env::var("BACKEND_TEST_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/test-config.json")
        });
    let Ok(text) = std::fs::read_to_string(&cfg_path) else {
        if strict_mode() {
            panic!(
                "严格模式:缺测试配置 {}(无法跑 translate_work 基准)",
                cfg_path.display()
            );
        }
        eprintln!("[work-bench] 跳过:缺 {}", cfg_path.display());
        return false;
    };
    let cfg: serde_json::Value = serde_json::from_str(&text).expect("配置 JSON");
    let account = cfg["accounts"][0]["account"]
        .as_str()
        .expect("account")
        .to_string();
    let password = cfg["accounts"][0]["password"]
        .as_str()
        .expect("password")
        .to_string();
    let login = backend::api::auth::LoginBuilder::new()
        .identity(&account)
        .password(&password)
        .execute()
        .expect("登录调用失败");
    assert!(login.success, "登录未成功:{:?}", login.message);
    true
}

#[test]
#[ignore = "需要网络:抓线上作品做端到端转换基准"]
fn translate_work_end_to_end_bench() {
    let id: i64 = std::env::var("WORK_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(273988379); // 默认 `now`(NEKO):走反向 KN → Kitten4

    if !login_or_skip() {
        return;
    }

    let dir =
        std::env::temp_dir().join(format!("backend-convert-work-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录");

    let mut best_ms = f64::INFINITY;
    let mut hashes: Vec<String> = Vec::new();
    for run in 0..RUNS {
        let started = Instant::now();
        let outcome = translate_work(
            WorkId::new(id),
            TargetEditor::Kitten4,
            // 确定性 id:多次运行才能逐字节比(判据见下方 assert)
            TranslateOptions::new()
                .deterministic_ids(true)
                .output_dir(dir.clone()),
        )
        .expect("translate_work 失败");
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        best_ms = best_ms.min(ms);
        let text = std::fs::read_to_string(&outcome.output).expect("读产物");
        let sha = sha256_hex(text.as_bytes());
        println!(
            "[work-bench] 第 {} 轮:{:.0} ms,产物 {} 字节,SHA256 {}",
            run + 1,
            ms,
            text.len(),
            &sha[..16]
        );
        hashes.push(sha);
    }
    assert!(
        hashes.iter().all(|h| h == &hashes[0]),
        "同一输入多次 translate_work 的产物必须逐字节一致(确定性):{hashes:?}"
    );
    println!(
        "[work-bench] work_id={id} 最小值 **{best_ms:.0} ms**,产物 SHA256 {}",
        &hashes[0][..16]
    );

    // 附带:把同一件作品"从本地重新转一遍"的耗时也打出来(不含抓取/反编译,便于对照)
    if let Ok(local) = std::env::var("LOCAL_FILE") {
        let path = PathBuf::from(local);
        if path.exists() {
            let started = Instant::now();
            let outcome = translate_file(
                &path,
                TargetEditor::Kitten4,
                TranslateOptions::new()
                    .deterministic_ids(true)
                    .output_dir(dir.clone()),
            )
            .expect("translate_file 失败");
            println!(
                "[work-bench] 对照 translate_file(本地文件):{:.0} ms,产物 {}",
                started.elapsed().as_secs_f64() * 1000.0,
                outcome.output.display()
            );
        }
    }
}

/// **P1 的量**:`save_raw` 消掉的是"把整份源作品 JSON 序列化一次 + 写盘一次(+ 收尾 unlink)"。
///
/// 为什么单开这条:上面那条端到端基准**被网络主导**(抓取 3~8 s,抖动比本地开销大一个量级),
/// 量不出 P1 的本地收益(实测两轮 6.9 s / 6.5 s,但第二轮后侧 15.8 s = 纯网络噪声)。
/// 这条用**本地真作品文件**(`RAW_SAMPLE`,默认取 `download/compile` 下最大的 `.bcmkn`)
/// 复现同一件事,给出"每次 translate_work 省掉多少"的量级。
#[test]
#[ignore = "性能探针:量 save_raw 消掉的那次 serialize + 写盘"]
fn raw_write_cost_probe() {
    let sample = std::env::var("RAW_SAMPLE").unwrap_or_else(|_| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("download/compile");
        let mut best = (0u64, PathBuf::new());
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("bcmkn") {
                    let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if len > best.0 {
                        best = (len, path);
                    }
                }
            }
        }
        best.1.to_string_lossy().to_string()
    });
    let text = std::fs::read_to_string(&sample).expect("读样本");
    let value: serde_json::Value = serde_json::from_str(&text).expect("解析样本");
    let dir = std::env::temp_dir().join(format!("raw-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let mut best_ms = f64::INFINITY;
    for run in 0..RUNS {
        let started = Instant::now();
        let rendered = serde_json::to_string(&value).expect("序列化");
        std::fs::write(dir.join("raw-probe.json"), &rendered).expect("写盘");
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        best_ms = best_ms.min(ms);
        println!(
            "[raw-probe] 第 {} 轮:{:.0} ms / {:.1} MB",
            run + 1,
            ms,
            rendered.len() as f64 / 1e6
        );
    }
    println!(
        "[raw-probe] 样本 {} ({:.1} MB):一次 serialize+写盘 最小值 **{best_ms:.0} ms**",
        sample,
        text.len() as f64 / 1e6
    );
    let _ = std::fs::remove_dir_all(&dir);
}
