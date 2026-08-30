//! 反编译(compiler)真机集成测试
//!
//! 设计原则:
//! - **配置与代码分离**:作品 ID / 账号等全部来自 `data/test-config.json`
//!   (复制 `tests/fixtures/test-config.example.json` 后填写)。`data/` 已被
//!   `.gitignore` 忽略,账号密码不会入库。也支持 `BACKEND_TEST_CONFIG` 环境
//!   变量覆盖配置文件路径。
//! - **无凭据硬编码**:反编译走公开接口(`source/public` 等),默认无需登录;
//!   配置 `accounts` 数组可提供多个账号,逐个登录后按序轮询分配给作品
//!   (用于私有作品或规避单账号限流)。
//! - **NEMO 费时**:NEMO 作品在 `works` 中按 `kind == "NEMO"` 识别,反编译
//!   约 5 分钟,单独标记 `#[ignore]` 默认跳过,需显式
//!   `cargo test --test compile_live -- --ignored` 运行。
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
    /// 作品类型(KITTEN4 / NEKO / NEMO / ...),`NEMO` 走费时测试
    kind: String,
}

/// 账号配置(密码仅存于本地 gitignored 配置文件)
#[derive(Debug, Deserialize)]
struct AccountEntry {
    account: String,
    password: String,
}

/// 测试配置:作品列表(含 NEMO)+ 可选多账号
#[derive(Debug, Deserialize)]
struct TestConfig {
    works: Vec<WorkEntry>,
    #[serde(default)]
    accounts: Vec<AccountEntry>,
}

impl TestConfig {
    fn nemo_works(&self) -> Vec<&WorkEntry> {
        self.works
            .iter()
            .filter(|w| w.kind.eq_ignore_ascii_case("NEMO"))
            .collect()
    }

    fn non_nemo_works(&self) -> Vec<&WorkEntry> {
        self.works
            .iter()
            .filter(|w| !w.kind.eq_ignore_ascii_case("NEMO"))
            .collect()
    }
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

/// 逐个登录所有配置账号,返回可用账号数
/// 全局身份槽只保留最后登录的 token;账号用于私有作品或限流轮询。
fn login_all(cfg: &TestConfig) -> usize {
    if cfg.accounts.is_empty() {
        eprintln!("[compile_live] 未配置账号,以匿名身份请求(公开接口无需登录)");
        return 0;
    }
    let mut ok = 0;
    for entry in &cfg.accounts {
        match LoginBuilder::new()
            .identity(&entry.account)
            .password(&entry.password)
            .execute()
        {
            Ok(result) if result.success => {
                ok += 1;
                eprintln!("[compile_live] 账号 {} 登录成功", entry.account);
            }
            Ok(result) => eprintln!(
                "[compile_live] 账号 {} 登录失败: {}",
                entry.account, result.message
            ),
            Err(e) => eprintln!("[compile_live] 账号 {} 登录请求失败: {e}", entry.account),
        }
    }
    ok
}

/// 反编译单个作品,断言返回的路径存在
/// 注意:Kitten/NEKO 返回 JSON 文件路径;NEMO 返回资源目录路径(且忽略 output_dir)。
/// 此处仅断言成功与路径存在,不校验内容格式(各类型产物形态不同)。
fn decompile_ok(work: &WorkEntry, work_dir: &Path) -> PathBuf {
    let options = DecompileOptions::new()
        .output_dir(work_dir.to_path_buf())
        .save_raw(false);
    let saved = decompile_work_with(work.id.into(), options)
        .unwrap_or_else(|e| panic!("作品 {} ({}) 反编译失败: {e}", work.id, work.kind));
    assert!(
        saved.exists(),
        "作品 {} 反编译产物不存在: {}",
        work.id,
        saved.display()
    );
    saved
}

/// 全部非 NEMO 作品(费时类型不在此列)
#[test]
fn decompile_non_nemo_works() {
    let Some(cfg) = load_config() else {
        return;
    };
    login_all(&cfg);
    let works = cfg.non_nemo_works();
    assert!(
        !works.is_empty(),
        "配置中非 NEMO 作品列表为空(works 字段无非 NEMO 作品)"
    );

    // 独立临时目录,避免作品间相互覆盖
    let base = std::env::temp_dir().join(format!("backend-compile-test-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("创建临时目录失败");
    let _ = std::fs::remove_dir_all(&base);

    for work in works {
        let work_dir = base.join(work.id.to_string());
        std::fs::create_dir_all(&work_dir).expect("创建作品目录失败");
        decompile_ok(work, &work_dir);
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// NEMO 作品费时(约 5 分钟),默认 `#[ignore]` 跳过
/// works 中 kind=NEMO 的作品逐个反编译(配置中通常仅保留一个)
#[test]
#[ignore = "NEMO 反编译约 5 分钟,费时;显式 cargo test --test compile_live -- --ignored 运行"]
fn decompile_nemo_works() {
    let Some(cfg) = load_config() else {
        return;
    };
    login_all(&cfg);
    let nemo_works = cfg.nemo_works();
    if nemo_works.is_empty() {
        eprintln!("[compile_live] 配置 works 中无 kind=NEMO 作品,跳过 NEMO 测试");
        return;
    }

    let base =
        std::env::temp_dir().join(format!("backend-compile-nemo-test-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("创建临时目录失败");
    let _ = std::fs::remove_dir_all(&base);

    for work in nemo_works {
        let work_dir = base.join(work.id.to_string());
        std::fs::create_dir_all(&work_dir).expect("创建作品目录失败");
        let saved = decompile_ok(work, &work_dir);
        eprintln!(
            "[compile_live] NEMO 作品 {} 反编译产物(下载目录): {}",
            work.id,
            saved.display()
        );
        // NEMO 的 save_result 忽略 output_dir,产物写入默认 download/compile/,
        // 测试后清理避免污染仓库目录
        let _ = std::fs::remove_file(&saved);
    }
    let _ = std::fs::remove_dir_all(&base);
}
