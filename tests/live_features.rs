//! 真机集成测试:登录 + AI 对话 + 作品云变量 + 作品反编译
//!
//! 设计原则:
//! - **配置与代码分离**:作品 ID / 账号等全部来自 `data/test-config.json`
//!   (复制 `tests/fixtures/test-config.example.json` 后填写)。`data/` 已被
//!   `.gitignore` 忽略,账号密码不会入库。也支持 `BACKEND_TEST_CONFIG` 环境
//!   变量覆盖配置文件路径。
//! - **无凭据硬编码**:无账号也可跑(公开接口匿名);配置 `accounts` 数组
//!   提供多账号时,逐个登录并用于 AI 对话/云变量(各自携带 token)。
//! - **各测试独立取 token**:每个测试自行登录拿 token,不依赖全局身份槽,
//!   与其它测试并行安全(cargo test 默认多线程)。
//! - **NEMO 反编译费时,已放弃**:作品云变量与反编译均不含 NEMO 专用用例。
//! - **配置缺失即跳过**:无配置时打印提示并直接返回,不导致 `cargo test` 失败。
//! - **不污染仓库**:反编译输出写入系统临时目录。
//!
//! 运行:`cargo test --test live_features`

use std::path::{Path, PathBuf};

use backend::api::auth::LoginBuilder;
use backend::core::cloudvar::CloudBuilder;
use backend::core::compiler::{DecompileOptions, decompile_work_with};
use backend::core::converse::{ChatBuilder, ChatEventType, HistoryMode};
use serde::Deserialize;

/// 单个作品的测试配置(反编译用)
#[derive(Debug, Deserialize)]
struct WorkEntry {
    id: i64,
    /// 作品类型展示用(KITTEN4 / NEKO / ...),不参与逻辑
    kind: String,
}

/// 账号配置(密码仅存于本地 gitignored 配置文件)
#[derive(Debug, Deserialize)]
struct AccountEntry {
    account: String,
    password: String,
}

/// 测试配置:作品列表 + 云变量作品 + 可选多账号
#[derive(Debug, Deserialize)]
struct TestConfig {
    /// 反编译作品列表
    works: Vec<WorkEntry>,
    /// 云变量测试作品 ID 列表
    #[serde(default)]
    cloud_works: Vec<i64>,
    /// AI 对话提问(默认"你好")
    #[serde(default = "default_prompt")]
    chat_prompt: String,
    #[serde(default)]
    accounts: Vec<AccountEntry>,
}

fn default_prompt() -> String {
    "你好,请用一句话介绍你自己".to_string()
}

/// 加载测试配置;缺失时返回 None(测试跳过)
fn load_config() -> Option<TestConfig> {
    let path = std::env::var("BACKEND_TEST_CONFIG")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/test-config.json"));
    if !path.exists() {
        eprintln!(
            "[live_features] 未找到测试配置 {path:?},跳过(可复制 \
             tests/fixtures/test-config.example.json 为 data/test-config.json 后填写)"
        );
        return None;
    }
    let text = std::fs::read_to_string(&path).expect("读取测试配置失败");
    match serde_json::from_str(&text) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            eprintln!("[live_features] 解析测试配置失败: {e}");
            None
        }
    }
}

/// 登录单个账号,成功返回 token;失败返回 None(不 panic,便于多账号逐个报告)
fn login(account: &str, password: &str) -> Option<String> {
    let mut session = LoginBuilder::new()
        .identity(account)
        .password(password)
        .build();
    match session.execute() {
        Ok(result) if result.success => Some(result.token),
        Ok(result) => {
            eprintln!(
                "[live_features] 账号 {account} 登录失败: {}",
                result.message
            );
            None
        }
        Err(e) => {
            eprintln!("[live_features] 账号 {account} 登录请求失败: {e}");
            None
        }
    }
}

/// 登录 + AI 对话:每个账号一轮,断言收到非空回复
#[test]
fn login_and_ai_chat() {
    let Some(cfg) = load_config() else {
        return;
    };
    if cfg.accounts.is_empty() {
        eprintln!("[live_features] 未配置 accounts,跳过 AI 对话测试");
        return;
    }
    for entry in &cfg.accounts {
        let Some(token) = login(&entry.account, &entry.password) else {
            continue;
        };
        let chat = ChatBuilder::new(&token)
            .connect_timeout(std::time::Duration::from_secs(10))
            .sync_timeout(std::time::Duration::from_secs(60))
            .start_timeout(std::time::Duration::from_secs(15))
            .build();
        chat.connect().expect("AI 连接失败");
        assert!(chat.is_connected(), "账号 {} AI 连接未建立", entry.account);

        let chunks = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let chunks_capture = std::sync::Arc::clone(&chunks);
        chat.on_stream(move |text, ev| {
            if ev == ChatEventType::Text {
                chunks_capture.lock().unwrap().push_str(text);
            }
        });
        let reply = chat
            .send_and_wait(&cfg.chat_prompt, HistoryMode::Exclude)
            .expect("AI 回复超时或失败");
        chat.close();

        assert!(
            !reply.trim().is_empty(),
            "账号 {} AI 回复为空",
            entry.account
        );
        eprintln!(
            "[live_features] 账号 {} AI 回复 {} 字(流式 {} 字): {}",
            entry.account,
            reply.chars().count(),
            chunks.lock().unwrap().chars().count(),
            truncate(&reply, 60)
        );
    }
}

/// 作品云变量:每个作品连接并读取数据,断言数据就绪
#[test]
fn cloud_variables() {
    let Some(cfg) = load_config() else {
        return;
    };
    if cfg.cloud_works.is_empty() {
        eprintln!("[live_features] 配置未提供 cloud_works,跳过云变量测试");
        return;
    }
    // 用第一个账号的 token(可选;匿名也可连公开作品)
    let token = cfg
        .accounts
        .first()
        .and_then(|a| login(&a.account, &a.password));

    for work_id in &cfg.cloud_works {
        let mut builder = CloudBuilder::new(*work_id)
            .connect_timeout(std::time::Duration::from_secs(10))
            .sync_timeout(std::time::Duration::from_secs(15));
        if let Some(t) = &token {
            builder = builder.authorization_token(t.clone());
        }
        let conn = builder.build();
        match conn.connect_and_wait() {
            Ok(true) => {
                let private_vars = conn.get_all_private_variables();
                let public_vars = conn.get_all_public_variables();
                let lists = conn.get_all_lists();
                eprintln!(
                    "[live_features] 作品 {work_id} 连接成功: 私有 {} 公有 {} 列表 {}",
                    private_vars.len(),
                    public_vars.len(),
                    lists.len()
                );
                conn.close();
            }
            Ok(false) => panic!("作品 {work_id} 云连接超时(连接/数据未就绪)"),
            Err(e) => panic!("作品 {work_id} 云连接失败: {e}"),
        }
    }
}

/// 反编译单个作品,断言返回的路径存在
fn decompile_ok(work: &WorkEntry, work_dir: &Path) {
    let options = DecompileOptions::new()
        .output_dir(work_dir.to_path_buf())
        .save_raw(false);
    let saved = decompile_work_with(work.id, options)
        .unwrap_or_else(|e| panic!("作品 {} ({}) 反编译失败: {e}", work.id, work.kind));
    assert!(
        saved.exists(),
        "作品 {} 反编译产物不存在: {}",
        work.id,
        saved.display()
    );
}

/// 作品反编译:works 全部(不含 NEMO;NEMO 费时已放弃)
#[test]
fn decompile_works() {
    let Some(cfg) = load_config() else {
        return;
    };
    assert!(!cfg.works.is_empty(), "配置中作品列表为空(works 字段)");

    let base = std::env::temp_dir().join(format!("backend-decompile-test-{}", std::process::id()));
    std::fs::create_dir_all(&base).expect("创建临时目录失败");
    let _ = std::fs::remove_dir_all(&base);

    for work in &cfg.works {
        let work_dir = base.join(work.id.to_string());
        std::fs::create_dir_all(&work_dir).expect("创建作品目录失败");
        decompile_ok(work, &work_dir);
    }
    let _ = std::fs::remove_dir_all(&base);
}

/// 日志截断
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let head: String = text.chars().take(max).collect();
        format!("{head}...")
    }
}
