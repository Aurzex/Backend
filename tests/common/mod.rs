//! 集成测试共用件(2026-11-07,rounds/58)。
//!
//! 只收**口径无差异**的工具函数;口径不同的局部实现(例如各测试自己的 `fn walk`)不进这里。
#![allow(dead_code)] // 各测试目标按需取用,未用到的函数不是死代码

use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

/// SHA-256 十六进制摘要(sha2 0.11 的 digest 不实现 `LowerHex`,自行拼)。
pub fn sha256_hex(bytes: &[u8]) -> String {
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

/// `BACKEND_REQUIRE_FIXTURES` 的严格模式:缺夹具时失败而不是跳过。
pub fn require_fixtures() -> bool {
    std::env::var("BACKEND_REQUIRE_FIXTURES")
        .is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// 取最小值:CPU 基准里最小值≈最少干扰。
pub fn best(samples: Vec<f64>) -> f64 {
    samples.into_iter().fold(f64::INFINITY, f64::min)
}

/// 毫秒。
pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 每个基准测试**独立**的临时目录(cargo 并行跑测试函数,共用一个目录会互相删)。
///
/// 调用方跑完自行 `remove_dir_all`;失败/panic 时保留现场。不清理会让 `/tmp` 积压
/// (2026-10-05 实测 46 份约 1.5 GB 打满 tmpfs,见 `docs/rounds/40` §9 的临时目录清理项)。
pub fn bench_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "backend-convert-bench-{tag}-{}-{:08x}",
        std::process::id(),
        fastrand::u32(..)
    ));
    std::fs::create_dir_all(&dir).expect("创建基准临时目录失败");
    dir
}
