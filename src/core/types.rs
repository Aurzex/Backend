use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::future::Future;
use std::io::{self, Write};
use std::num::ParseIntError;
use std::pin::Pin;
use std::time::{Duration, UNIX_EPOCH};

use crate::api::whale::{CommentSourceType, ReportStatus, WhaleReportFetcher, WorkSourceType};
use crate::utils::acquire::{self};

use serde_json::Value;

// ==================== 自定义错误类型 ====================
#[derive(Debug)]
pub enum ProcessorError {
    Processing(String),
    Io(io::Error),
    Json(serde_json::Error),
    ParseInt(ParseIntError),
    External(Box<dyn std::error::Error>),
}

impl std::fmt::Display for ProcessorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessorError::Processing(s) => write!(f, "Processing error: {}", s),
            ProcessorError::Io(e) => write!(f, "I/O error: {}", e),
            ProcessorError::Json(e) => write!(f, "JSON error: {}", e),
            ProcessorError::ParseInt(e) => write!(f, "Parse int error: {}", e),
            ProcessorError::External(e) => write!(f, "External error: {}", e),
        }
    }
}

impl std::error::Error for ProcessorError {}

impl From<io::Error> for ProcessorError {
    fn from(e: io::Error) -> Self {
        ProcessorError::Io(e)
    }
}
impl From<serde_json::Error> for ProcessorError {
    fn from(e: serde_json::Error) -> Self {
        ProcessorError::Json(e)
    }
}
impl From<ParseIntError> for ProcessorError {
    fn from(e: ParseIntError) -> Self {
        ProcessorError::ParseInt(e)
    }
}
impl From<acquire::MewError> for ProcessorError {
    fn from(e: acquire::MewError) -> Self {
        ProcessorError::External(Box::new(e))
    }
}
impl From<Box<dyn std::error::Error>> for ProcessorError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        ProcessorError::External(e)
    }
}

pub trait CommentConfig {
    fn get_comments(&self, item_id: i64) -> Option<Vec<Value>>;
}

// ==================== 交互工具 ====================
pub fn prompt_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

pub fn get_valid_input(prompt: &str, valid_options: &HashSet<String>) -> String {
    loop {
        let input = prompt_input(prompt);
        if valid_options.contains(&input.to_uppercase()) {
            return input.to_uppercase();
        }
        println!("无效输入，请重试");
    }
}

// ==================== 辅助函数 ====================
pub fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub fn value_to_i64(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub fn timestamp_to_string(ts: &serde_json::Value) -> String {
    if let Some(secs) = ts.as_i64() {
        if secs > 0 {
            let secs_u64 = secs as u64;
            let t = UNIX_EPOCH + Duration::from_secs(secs_u64);
            match t.elapsed() {
                Ok(_) => {
                    let timestamp = t.duration_since(UNIX_EPOCH).unwrap().as_secs();
                    return format!("{}", timestamp);
                }
                Err(_) => {}
            }
        }
    }
    ts.to_string()
}

pub fn html_to_text(html: &str) -> String {
    html.replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<p>", "")
        .replace("</p>", "\n")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn bytes_to_human(size_bytes: u64) -> String {
    if size_bytes >= 1024 * 1024 {
        format!("{:.2} MB", size_bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} KB", size_bytes as f64 / 1024.0)
    }
}

// ==================== 举报类型配置 ====================
#[derive(Debug, Clone)]
pub struct ActionConfig {
    pub key: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub enabled: bool,
}

#[derive(Clone, Debug)]
pub struct SourceConfig {
    pub name: String,
    // 异步获取总数
    pub fetch_total:
        fn(ReportStatus) -> Pin<Box<dyn Future<Output = Result<Value, ProcessorError>> + Send>>,
    // 同步收集所有报告（内部使用 tokio 阻塞等待）
    pub fetch_generator: fn(ReportStatus) -> Result<Vec<Value>, ProcessorError>,
    pub handle_method: String,
    pub item_id_field: String,
    pub report_id_field: String,
    pub reason_field: String,
    pub reason_id_field: String,
    pub description_field: String,
    pub status_field: String,
    pub admin_id_field: String,
    pub admin_username_field: String,
    pub content_field: String,
    pub content_type_field: String,
    pub content_id_field: String,
    pub user_id_field: String,
    pub user_nickname_field: String,
    pub user_parent_id_field: String,
    pub user_parent_nickname_field: String,
    pub source_id_field: String,
    pub source_name_field: String,
    pub source_type_field: String,
    pub source_object_id_field: String,
    pub source_object_name_field: String,
    pub parent_id_field: String,
    pub work_type_field: Option<String>,
    pub title_field: Option<String>,
    pub board_name_field: Option<String>,
    pub board_id_field: Option<String>,
    pub created_at_field: String,
    pub chunk_size: usize,
    pub special_check: Option<fn(&Value) -> bool>,
    pub available_actions: Vec<ActionConfig>,
}

// ==================== 举报类型注册表 ====================
#[derive(Clone)]
pub struct ReportTypeRegistry {
    pub(crate) registry: HashMap<String, SourceConfig>,
    pub(crate) default_actions: HashMap<String, ActionConfig>,
}

impl ReportTypeRegistry {
    pub fn new() -> Self {
        let mut default_actions = HashMap::new();
        default_actions.insert(
            "D".to_string(),
            ActionConfig {
                key: "D".into(),
                name: "删除".into(),
                description: "删除内容".into(),
                status: "DELETE".into(),
                enabled: true,
            },
        );
        default_actions.insert(
            "S".to_string(),
            ActionConfig {
                key: "S".into(),
                name: "禁言7天".into(),
                description: "禁言用户7天".into(),
                status: "MUTE_SEVEN_DAYS".into(),
                enabled: true,
            },
        );
        default_actions.insert(
            "T".to_string(),
            ActionConfig {
                key: "T".into(),
                name: "禁言3月".into(),
                description: "禁言用户3个月".into(),
                status: "MUTE_THREE_MONTHS".into(),
                enabled: true,
            },
        );
        default_actions.insert(
            "U".to_string(),
            ActionConfig {
                key: "U".into(),
                name: "取消发布".into(),
                description: "取消作品发布".into(),
                status: "UNLOAD".into(),
                enabled: true,
            },
        );
        default_actions.insert(
            "P".to_string(),
            ActionConfig {
                key: "P".into(),
                name: "通过".into(),
                description: "通过举报".into(),
                status: "PASS".into(),
                enabled: true,
            },
        );
        default_actions.insert(
            "F".to_string(),
            ActionConfig {
                key: "F".into(),
                name: "检查违规".into(),
                description: "检查其他违规内容".into(),
                status: "CHECK_VIOLATION".into(),
                enabled: true,
            },
        );
        default_actions.insert(
            "J".to_string(),
            ActionConfig {
                key: "J".into(),
                name: "跳过".into(),
                description: "跳过当前举报".into(),
                status: "SKIP".into(),
                enabled: true,
            },
        );

        ReportTypeRegistry {
            registry: HashMap::new(),
            default_actions,
        }
    }

    pub fn register(&mut self, report_type: &str, config: SourceConfig) {
        self.registry.insert(report_type.to_string(), config);
    }

    pub fn get_config(&self, report_type: &str) -> Option<&SourceConfig> {
        self.registry.get(report_type)
    }

    pub fn get_all_types(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }

    pub fn get_available_actions(&self, report_type: &str) -> Vec<ActionConfig> {
        if let Some(config) = self.get_config(report_type) {
            config
                .available_actions
                .iter()
                .filter(|a| a.enabled && a.key != "C")
                .cloned()
                .collect()
        } else {
            vec![]
        }
    }

    pub fn get_action_prompt(&self, report_type: &str) -> String {
        let actions = self.get_available_actions(report_type);
        let parts: Vec<String> = actions
            .iter()
            .map(|a| format!("{}({})", a.key, a.name))
            .collect();
        format!("选择操作:{}", parts.join(","))
    }

    pub fn get_status_mapping(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for (key, action) in &self.default_actions {
            if matches!(key.as_str(), "D" | "S" | "T" | "P" | "U") {
                map.insert(key.clone(), action.status.clone());
            }
        }
        map
    }

    pub fn is_action_available(&self, report_type: &str, action_key: &str) -> bool {
        self.get_available_actions(report_type)
            .iter()
            .any(|a| a.key == action_key)
    }
}

// ==================== 举报获取器 ====================
pub struct ReportFetcher {
    pub registry: ReportTypeRegistry,
}

impl ReportFetcher {
    pub async fn new() -> Self {
        let mut registry = ReportTypeRegistry::new();

        // 注册工作室评论举报
        registry.register(
            "shop_comment",
            SourceConfig {
                name: "工作室评论举报".into(),
                fetch_total: |status| {
                    Box::pin(async move {
                        WhaleReportFetcher::new()
                            .fetch_comment_reports_total(CommentSourceType::All, status, None, None)
                            .await
                            .map_err(|e| ProcessorError::External(e.into()))
                    })
                },
                fetch_generator: |status| {
                    // 使用 Handle::block_on 在当前 Tokio 运行时中阻塞执行异步操作
                    let handle = tokio::runtime::Handle::current();
                    let mut iter = WhaleReportFetcher::new().fetch_comment_reports_gen(
                        CommentSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    );
                    let mut all = Vec::new();
                    let result: Result<(), ProcessorError> = handle.block_on(async {
                        while let Some(item) = iter.next_item().await {
                            match item {
                                Ok(v) => all.push(v),
                                Err(e) => return Err(ProcessorError::External(e.into())),
                            }
                        }
                        Ok(())
                    });
                    // 显式标注闭包返回类型
                    result.map(|_| -> Vec<Value> { all })
                },
                handle_method: "execute_process_comment_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                reason_id_field: "reason_id".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_user_name".into(),
                content_field: "comment_content".into(),
                content_type_field: "comment_source".into(),
                content_id_field: "comment_id".into(),
                user_id_field: "comment_user_id".into(),
                user_nickname_field: "comment_user_nickname".into(),
                user_parent_id_field: "comment_parent_user_id".into(),
                user_parent_nickname_field: "comment_parent_user_nickname".into(),
                source_id_field: "comment_source_object_id".into(),
                source_name_field: "comment_source_object_name".into(),
                source_type_field: "comment_source".into(),
                source_object_id_field: "comment_source_object_id".into(),
                source_object_name_field: "comment_source_object_name".into(),
                parent_id_field: "comment_parent_id".into(),
                work_type_field: None,
                title_field: None,
                board_name_field: None,
                board_id_field: None,
                created_at_field: "created_at".into(),
                chunk_size: 100,
                special_check: Some(|item: &Value| -> bool {
                    item.get("comment_source")
                        .and_then(|v| v.as_str())
                        .map(|s| s == "WORK_SHOP")
                        .unwrap_or(false)
                }),
                available_actions: vec![
                    ActionConfig {
                        key: "D".into(),
                        name: "删除".into(),
                        description: "".into(),
                        status: "DELETE".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "S".into(),
                        name: "禁言7天".into(),
                        description: "".into(),
                        status: "MUTE_SEVEN_DAYS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "T".into(),
                        name: "禁言3月".into(),
                        description: "".into(),
                        status: "MUTE_THREE_MONTHS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "P".into(),
                        name: "通过".into(),
                        description: "".into(),
                        status: "PASS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "F".into(),
                        name: "检查违规".into(),
                        description: "".into(),
                        status: "CHECK_VIOLATION".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "J".into(),
                        name: "跳过".into(),
                        description: "".into(),
                        status: "SKIP".into(),
                        enabled: true,
                    },
                ],
            },
        );

        // 注册作品举报
        registry.register(
            "work_work",
            SourceConfig {
                name: "作品举报".into(),
                fetch_total: |status| {
                    Box::pin(async move {
                        WhaleReportFetcher::new()
                            .fetch_work_reports_total(WorkSourceType::All, status, None, None)
                            .await
                            .map_err(|e| ProcessorError::External(e.into()))
                    })
                },
                fetch_generator: |status| {
                    let handle = tokio::runtime::Handle::current();
                    let mut iter = WhaleReportFetcher::new().fetch_work_reports_gen(
                        WorkSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    );
                    let mut all = Vec::new();
                    let result: Result<(), ProcessorError> = handle.block_on(async {
                        while let Some(item) = iter.next_item().await {
                            match item {
                                Ok(v) => all.push(v),
                                Err(e) => return Err(ProcessorError::External(e.into())),
                            }
                        }
                        Ok(())
                    });
                    result.map(|_| -> Vec<Value> { all })
                },
                handle_method: "execute_process_work_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                reason_id_field: "reason_id".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_username".into(),
                content_field: "work_name".into(),
                content_type_field: "work_type".into(),
                content_id_field: "work_id".into(),
                user_id_field: "work_user_id".into(),
                user_nickname_field: "work_user_nickname".into(),
                user_parent_id_field: String::new(),
                user_parent_nickname_field: String::new(),
                source_id_field: "work_id".into(),
                source_name_field: "work_name".into(),
                source_type_field: "work_type".into(),
                source_object_id_field: "work_id".into(),
                source_object_name_field: "work_name".into(),
                parent_id_field: String::new(),
                work_type_field: Some("work_type".into()),
                title_field: Some("work_name".into()),
                board_name_field: None,
                board_id_field: None,
                created_at_field: "created_at".into(),
                chunk_size: 100,
                special_check: None,
                available_actions: vec![
                    ActionConfig {
                        key: "D".into(),
                        name: "删除".into(),
                        description: "".into(),
                        status: "DELETE".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "P".into(),
                        name: "通过".into(),
                        description: "".into(),
                        status: "PASS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "U".into(),
                        name: "取消发布".into(),
                        description: "".into(),
                        status: "UNLOAD".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "J".into(),
                        name: "跳过".into(),
                        description: "".into(),
                        status: "SKIP".into(),
                        enabled: true,
                    },
                ],
            },
        );

        // 注册帖子举报
        registry.register(
            "forum_post",
            SourceConfig {
                name: "帖子举报".into(),
                fetch_total: |status| {
                    Box::pin(async move {
                        WhaleReportFetcher::new()
                            .fetch_post_reports_total(status, None, None, None)
                            .await
                            .map_err(|e| ProcessorError::External(e.into()))
                    })
                },
                fetch_generator: |status| {
                    let handle = tokio::runtime::Handle::current();
                    let mut iter = WhaleReportFetcher::new().fetch_post_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    );
                    let mut all = Vec::new();
                    let result: Result<(), ProcessorError> = handle.block_on(async {
                        while let Some(item) = iter.next_item().await {
                            match item {
                                Ok(v) => all.push(v),
                                Err(e) => return Err(ProcessorError::External(e.into())),
                            }
                        }
                        Ok(())
                    });
                    result.map(|_| -> Vec<Value> { all })
                },
                handle_method: "execute_process_post_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                reason_id_field: "reason_id".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_username".into(),
                content_field: "post_title".into(),
                content_type_field: "board_name".into(),
                content_id_field: "post_id".into(),
                user_id_field: "post_user_id".into(),
                user_nickname_field: "post_user_nick_name".into(),
                user_parent_id_field: String::new(),
                user_parent_nickname_field: String::new(),
                source_id_field: "post_id".into(),
                source_name_field: "board_name".into(),
                source_type_field: "board_name".into(),
                source_object_id_field: "post_id".into(),
                source_object_name_field: "board_name".into(),
                parent_id_field: String::new(),
                work_type_field: None,
                title_field: Some("post_title".into()),
                board_name_field: Some("board_name".into()),
                board_id_field: Some("board_id".into()),
                created_at_field: "created_at".into(),
                chunk_size: 100,
                special_check: None,
                available_actions: vec![
                    ActionConfig {
                        key: "D".into(),
                        name: "删除".into(),
                        description: "".into(),
                        status: "DELETE".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "S".into(),
                        name: "禁言7天".into(),
                        description: "".into(),
                        status: "MUTE_SEVEN_DAYS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "T".into(),
                        name: "禁言3月".into(),
                        description: "".into(),
                        status: "MUTE_THREE_MONTHS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "P".into(),
                        name: "通过".into(),
                        description: "".into(),
                        status: "PASS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "F".into(),
                        name: "检查违规".into(),
                        description: "".into(),
                        status: "CHECK_VIOLATION".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "J".into(),
                        name: "跳过".into(),
                        description: "".into(),
                        status: "SKIP".into(),
                        enabled: true,
                    },
                ],
            },
        );

        // 注册讨论举报
        registry.register(
            "forum_discussion",
            SourceConfig {
                name: "讨论举报".into(),
                fetch_total: |status| {
                    Box::pin(async move {
                        WhaleReportFetcher::new()
                            .fetch_discussion_reports_total(status, None, None, None)
                            .await
                            .map_err(|e| ProcessorError::External(e.into()))
                    })
                },
                fetch_generator: |status| {
                    let handle = tokio::runtime::Handle::current();
                    let mut iter = WhaleReportFetcher::new().fetch_discussion_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    );
                    let mut all = Vec::new();
                    let result: Result<(), ProcessorError> = handle.block_on(async {
                        while let Some(item) = iter.next_item().await {
                            match item {
                                Ok(v) => all.push(v),
                                Err(e) => return Err(ProcessorError::External(e.into())),
                            }
                        }
                        Ok(())
                    });
                    result.map(|_| -> Vec<Value> { all })
                },
                handle_method: "execute_process_discussion_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                reason_id_field: "reason_id".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_username".into(),
                content_field: "discussion_content".into(),
                content_type_field: "discussion_source".into(),
                content_id_field: "discussion_id".into(),
                user_id_field: "discussion_user_id".into(),
                user_nickname_field: "discussion_user_nickname".into(),
                user_parent_id_field: String::new(),
                user_parent_nickname_field: String::new(),
                source_id_field: "post_id".into(),
                source_name_field: "post_title".into(),
                source_type_field: "discussion_source".into(),
                source_object_id_field: "post_id".into(),
                source_object_name_field: "post_title".into(),
                parent_id_field: String::new(),
                work_type_field: None,
                title_field: Some("post_title".into()),
                board_name_field: Some("board_name".into()),
                board_id_field: Some("board_id".into()),
                created_at_field: "created_at".into(),
                chunk_size: 100,
                special_check: None,
                available_actions: vec![
                    ActionConfig {
                        key: "D".into(),
                        name: "删除".into(),
                        description: "".into(),
                        status: "DELETE".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "S".into(),
                        name: "禁言7天".into(),
                        description: "".into(),
                        status: "MUTE_SEVEN_DAYS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "T".into(),
                        name: "禁言3月".into(),
                        description: "".into(),
                        status: "MUTE_THREE_MONTHS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "P".into(),
                        name: "通过".into(),
                        description: "".into(),
                        status: "PASS".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "F".into(),
                        name: "检查违规".into(),
                        description: "".into(),
                        status: "CHECK_VIOLATION".into(),
                        enabled: true,
                    },
                    ActionConfig {
                        key: "J".into(),
                        name: "跳过".into(),
                        description: "".into(),
                        status: "SKIP".into(),
                        enabled: true,
                    },
                ],
            },
        );

        ReportFetcher { registry }
    }

    pub fn fetch_chunked(&self, status: ReportStatus) -> impl Iterator<Item = Vec<Value>> {
        let report_types = self.registry.get_all_types();
        let total_types = report_types.len();
        let mut type_index = 0;
        let mut carry_over: Vec<Value> = Vec::new();

        std::iter::from_fn(move || {
            // 如果上一轮有剩余数据，优先返回
            if !carry_over.is_empty() {
                let mut chunk = Vec::new();
                std::mem::swap(&mut chunk, &mut carry_over);
                if chunk.len() <= 100 {
                    return Some(chunk);
                } else {
                    let remaining = chunk.split_off(100);
                    carry_over = remaining;
                    return Some(chunk);
                }
            }

            // 没有剩余数据，继续从下一类型拉取
            while type_index < total_types {
                let report_type = &report_types[type_index];
                let config = match self.registry.get_config(report_type) {
                    Some(cfg) => cfg,
                    None => {
                        type_index += 1;
                        continue;
                    }
                };

                // 同步获取该类型所有报告
                let mut all_items = match (config.fetch_generator)(status.clone()) {
                    Ok(items) => items,
                    Err(e) => {
                        eprintln!("Error fetching report data: {}", e);
                        type_index += 1;
                        continue;
                    }
                };

                // 按状态过滤（仅对 ToBeDone）
                if status == ReportStatus::ToBeDone {
                    all_items.retain(|item| {
                        item.get(&config.status_field)
                            .and_then(|v| v.as_str())
                            .map(|s| s == "TOBEDONE")
                            .unwrap_or(false)
                    });
                }

                // 注入类型标记（使用 as_object_mut 避免借用冲突）
                for item in &mut all_items {
                    if let Some(map) = item.as_object_mut() {
                        map.insert("_report_type".into(), Value::String(report_type.clone()));
                    }
                }

                if all_items.is_empty() {
                    type_index += 1;
                    continue;
                }

                // 分块处理
                if all_items.len() <= 100 {
                    type_index += 1;
                    return Some(all_items);
                } else {
                    let mut chunk = all_items;
                    let remaining = chunk.split_off(100);
                    carry_over = remaining;
                    return Some(chunk);
                }
            }

            // 所有类型都遍历完了
            None
        })
    }

    pub fn fetch_reports_chunked(&self, status: ReportStatus) -> impl Iterator<Item = Vec<Value>> {
        self.fetch_chunked(status)
    }

    pub async fn get_total_reports(&self, status: ReportStatus) -> i64 {
        let mut total = 0i64;
        for rtype in self.registry.get_all_types() {
            if let Some(config) = self.registry.get_config(&rtype) {
                if let Ok(result) = (config.fetch_total)(status).await {
                    if let Some(t) = result.get("total").and_then(|v| v.as_i64()) {
                        total += t;
                    }
                }
            }
        }
        total
    }
}
