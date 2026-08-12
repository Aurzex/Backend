//! 反编译(compiler)真机集成测试
//!
//! 设计原则:
//! - **配置与代码分离**:作品 ID / 账号等全部来自 `data/test-config.json`
//!   (复制 `tests/fixtures/test-config.example.json` 后填写)。`data/` 已被
//!   `.gitignore` 忽略,账号密码不会入库。也支持 `BACKEND_TEST_CONFIG` 环境
//!   变量覆盖配置文件路径。
//! - **无凭据硬编码**:反编译走公开接口(`source/public` 等),默认无需登录;
//!   仅当配置提供了 account/password 时才执行登录(供私有作品等场景)。
//! - **NEMO 费时**:非 NEMO 作品为常规测试(各 3~6s);NEMO 反编译约 5 分钟,
//!   标记 `#[ignore]` 默认跳过,需显式 `cargo test --test compile_live -- --ignored`
//!   运行(配置中仅保留一个 NEMO 作品)。
//! - **配置缺失即跳过**:无配置时打印提示并直接返回,不导致 `cargo test` 失败。
//! - **不污染仓库**:输出写入系统临时目录。
//!
//! 运行:`cargo test --test compile_live`(常规)/ `cargo test --test compile_live -- --ignored`(含 NEMO)

use std::path::{Path, PathBuf};

use backend::api::auth::LoginBuilder;
use backend::core::compiler::{DecompileOptions, decompile_work_with};
use serde::Deserialize;

/// 单个作品的测试配置
#[derive(Debug, Deserialize)]
struct WorkEntry {
    id: i64,
    /// 展示用(如 KITTEN4 / NEKO / NEMO),不参与逻辑
    kind: String,
}

/// 测试配置:作品列表 + 可选账号
#[derive(Debug, Deserialize)]
struct TestConfig {
    works: Vec<WorkEntry>,
    nemo_work: Option<WorkEntry>,
    #[serde(default)]
    account: String,
    #[serde(default)]
    password: String,
}

/// 加载测试配置;缺失时返回 None(测试跳过)
fn load_config() -> Option<TestConfig> {
    let path = std::env::var("BACKEND_TEST_CONFIG")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/test-config.json"));
    if !path.exists() {
        eprintln!(
            "[compile_live] 未找到测试配置 {path:?},跳过(可复制 \
             tests/fixtures/test-config.example.json 为 data/test-config.json 后填写)"
        );
        return None;
    }
    let text = std::fs::read_to_string(&path).expect("读取测试配置失败");
    match serde_json::from_str(&text) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("[compile_live] 解析测试配置失败: {e}");
            None
        }
    }
}

/// 可选登录:配置提供账号密码时执行,写全局身份槽(后续请求自动携带)
fn login_if_configured(cfg: &TestConfig) {
    if cfg.account.is_empty() || cfg.password.is_empty() {
        eprintln!("[compile_live] 未配置账号,以匿名身份请求(公开接口无需登录)");
        return;
    }
    let mut session = LoginBuilder::new()
        .identity(&cfg.account)
        .password(&cfg.password)
        .build();
    match session.execute() {
        Ok(result) if result.success => {
            eprintln!("[compile_live] 已登录: {}", result.message);
        }
        Ok(result) => eprintln!("[compile_live] 登录失败: {}", result.message),
        Err(e) => eprintln!("[compile_live] 登录请求失败: {e}"),
    }
}

/// 反编译单个作品,断言返回的路径存在
/// 注意:Kitten/NEKO 返回 JSON 文件路径;NEMO 返回资源目录路径(且忽略 output_dir)。
/// 此处仅断言成功与路径存在,不校验内容格式(各类型产物形态不同)。
fn decompile_ok(work: &WorkEntry, work_dir: &Path) -> String {
    let options = DecompileOptions::new()
        .output_dir(work_dir.to_path_buf())
        .save_raw(false);
    let saved = decompile_work_with(work.id, options)
        .unwrap_or_else(|e| panic!("作品 {} ({}) 反编译失败: {e}", work.id, work.kind));
    assert!(
        Path::new(&saved).exists(),
        "作品 {} 反编译产物不存在: {}",
        work.id,
        saved
    );
    saved
}

/// 全部非 NEMO 作品(费时类型不在此列)
#[test]
fn decompile_non_nemo_works() {
    let Some(cfg) = load_config() else {
        return;
    };
    login_if_configured(&cfg);
    assert!(
        !cfg.works.is_empty(),
        "配置中非 NEMO 作品列表为空(works 字段)"
    );

    // 独立临时目录,避免作品间相互覆盖
    let base = std::env::temp_dir().join(format!("backend-compile-test-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("创建临时目录失败");
    let _ = std::fs::remove_dir_all(&base);

    for work in &cfg.works {
        let work_dir = base.join(work.id.to_string());
        std::fs::create_dir_all(&work_dir).expect("创建作品目录失败");
        decompile_ok(work, &work_dir);
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// NEMO 作品费时(约 5 分钟),默认 `#[ignore]` 跳过,配置中仅保留一个
#[test]
#[ignore = "NEMO 反编译约 5 分钟,费时;显式 cargo test --test compile_live -- --ignored 运行"]
fn decompile_nemo_work() {
    let Some(cfg) = load_config() else {
        return;
    };
    login_if_configured(&cfg);
    let Some(nemo) = &cfg.nemo_work else {
        eprintln!("[compile_live] 配置未提供 nemo_work,跳过 NEMO 测试");
        return;
    };

    let base =
        std::env::temp_dir().join(format!("backend-compile-nemo-test-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("创建临时目录失败");
    let _ = std::fs::remove_dir_all(&base);
    let work_dir = base.join(nemo.id.to_string());
    std::fs::create_dir_all(&work_dir).expect("创建作品目录失败");

    let saved = decompile_ok(nemo, &work_dir);
    eprintln!(
        "[compile_live] NEMO 作品 {} 反编译产物(下载目录): {}",
        nemo.id, saved
    );
    let _ = std::fs::remove_dir_all(&base);
    // NEMO 的 save_result 忽略 output_dir,产物写入默认 download/compile/,
    // 测试后清理避免污染仓库目录
    let _ = std::fs::remove_file(&saved);
}
