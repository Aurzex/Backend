use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io::{self, Write};
use std::num::ParseIntError;
use std::sync::OnceLock;
use std::time::{Duration, UNIX_EPOCH};

use crate::api::whale::{CommentSourceType, ReportStatus, WhaleReportFetcher, WorkSourceType};
use crate::utils::acquire;

use serde_json::{Value, json};

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

// ==================== 评论配置 trait ====================
pub trait CommentConfig {
    fn get_comments(&self, item_id: i64) -> Option<&[Value]>;
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

/// 将时间戳转换为字符串表示。
pub fn timestamp_to_string(ts: &serde_json::Value) -> String {
    if let Some(secs) = ts.as_i64()
        && secs > 0
    {
        let t = UNIX_EPOCH + Duration::from_secs(secs as u64);
        // 简化格式化（实际可用 chrono，此处保留原有方式）
        let timestamp = t.duration_since(UNIX_EPOCH).unwrap().as_secs();
        return format!("{}", timestamp);
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

impl ActionConfig {
    /// 由动作键构造配置（名称与状态取自内置映射）。
    fn simple(key: &str) -> Self {
        let (name, status) = match key {
            "D" => ("删除", "DELETE"),
            "S" => ("禁言7天", "MUTE_SEVEN_DAYS"),
            "T" => ("禁言3月", "MUTE_THREE_MONTHS"),
            "U" => ("取消发布", "UNLOAD"),
            "P" => ("通过", "PASS"),
            "F" => ("检查违规", "CHECK_VIOLATION"),
            "J" => ("跳过", "SKIP"),
            _ => (key, key),
        };
        ActionConfig {
            key: key.into(),
            name: name.into(),
            description: String::new(),
            status: status.into(),
            enabled: true,
        }
    }
}

/// 按给定动作键列表批量构造动作配置。
fn actions(keys: &[&str]) -> Vec<ActionConfig> {
    keys.iter().map(|k| ActionConfig::simple(k)).collect()
}

pub type FetchGenerator =
    fn(ReportStatus) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>>>;
pub type FetchTotal = fn(ReportStatus) -> Result<Value, ProcessorError>;

#[derive(Clone, Debug)]
pub struct SourceConfig {
    pub admin_id_field: String,
    pub admin_username_field: String,
    pub available_actions: Vec<ActionConfig>,
    pub board_id_field: Option<String>,
    pub board_name_field: Option<String>,
    pub chunk_size: usize,
    pub content_field: String,
    pub content_id_field: String,
    pub content_type_field: String,
    pub created_at_field: String,
    pub description_field: String,
    pub fetch_generator: FetchGenerator,
    pub fetch_total: FetchTotal,
    pub handle_method: String,
    pub item_id_field: String,
    pub name: String,
    pub parent_id_field: String,
    pub reason_field: String,
    pub reason_id_field: String,
    pub report_id_field: String,
    pub source_id_field: String,
    pub source_name_field: String,
    pub source_object_id_field: String,
    pub source_object_name_field: String,
    pub source_type_field: String,
    pub special_check: Option<fn(&Value) -> bool>,
    pub status_field: String,
    pub title_field: Option<String>,
    pub user_id_field: String,
    pub user_nickname_field: String,
    pub user_parent_id_field: String,
    pub user_parent_nickname_field: String,
    pub work_type_field: Option<String>,
}

impl SourceConfig {
    /// 构造带公共默认字段名的配置；差异字段由调用方覆盖后再注册。
    ///
    /// 大部分举报类型的字段名高度一致（如 `report_id_field` 均为 "id"），
    /// 通过"公共默认值 + 覆盖差异"大幅减少重复。
    fn base(
        name: &str,
        handle_method: &str,
        fetch_total: FetchTotal,
        fetch_generator: FetchGenerator,
    ) -> Self {
        SourceConfig {
            name: name.into(),
            handle_method: handle_method.into(),
            fetch_total,
            fetch_generator,
            admin_id_field: "admin_id".into(),
            admin_username_field: String::new(),
            available_actions: Vec::new(),
            board_id_field: None,
            board_name_field: None,
            chunk_size: 100,
            content_field: String::new(),
            content_id_field: String::new(),
            content_type_field: String::new(),
            created_at_field: "created_at".into(),
            description_field: "description".into(),
            item_id_field: "id".into(),
            parent_id_field: String::new(),
            reason_field: "reason_content".into(),
            reason_id_field: "reason_id".into(),
            report_id_field: "id".into(),
            source_id_field: String::new(),
            source_name_field: String::new(),
            source_object_id_field: String::new(),
            source_object_name_field: String::new(),
            source_type_field: String::new(),
            special_check: None,
            status_field: "status".into(),
            title_field: None,
            user_id_field: String::new(),
            user_nickname_field: String::new(),
            user_parent_id_field: String::new(),
            user_parent_nickname_field: String::new(),
            work_type_field: None,
        }
    }
}

// ==================== 举报类型注册表 ====================
#[derive(Clone)]
pub struct ReportTypeRegistry {
    registry: HashMap<String, SourceConfig>,
    default_actions: Vec<ActionConfig>, // 保留用于构建默认动作，也可直接为静态
}

// 静态状态映射（避免每次构建）
static STATUS_MAPPING: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

pub(crate) fn status_mapping() -> &'static HashMap<&'static str, &'static str> {
    STATUS_MAPPING.get_or_init(|| {
        HashMap::from([
            ("D", "DELETE"),
            ("S", "MUTE_SEVEN_DAYS"),
            ("T", "MUTE_THREE_MONTHS"),
            ("P", "PASS"),
            ("U", "UNLOAD"),
        ])
    })
}

impl Default for ReportTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportTypeRegistry {
    pub fn new() -> Self {
        let default_actions = actions(&["D", "S", "T", "U", "P", "F", "J"]);

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

    /// 返回可用动作的引用，避免克隆整个 ActionConfig。
    pub fn get_available_actions(&self, report_type: &str) -> Vec<&ActionConfig> {
        self.get_config(report_type)
            .map(|config| {
                config
                    .available_actions
                    .iter()
                    .filter(|a| a.enabled && a.key != "C")
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn get_action_prompt(&self, report_type: &str) -> String {
        let actions = self.get_available_actions(report_type);
        let parts: Vec<String> = actions
            .iter()
            .map(|a| format!("{}({})", a.key, a.name))
            .collect();
        format!("选择操作:{}", parts.join(","))
    }

    /// 返回全局静态的状态映射引用。
    pub fn get_status_mapping(&self) -> &'static HashMap<&'static str, &'static str> {
        status_mapping()
    }

    pub fn is_action_available(&self, report_type: &str, action_key: &str) -> bool {
        self.get_available_actions(report_type)
            .iter()
            .any(|a| a.key == action_key)
    }
}

// ==================== 举报获取器 ====================
/// 举报状态字段中表示“待处理”的值。
const TO_BE_DONE_STATUS: &str = "TOBEDONE";

pub struct ReportFetcher {
    pub registry: ReportTypeRegistry,
}

impl Default for ReportFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportFetcher {
    pub fn new() -> Self {
        let mut registry = ReportTypeRegistry::new();

        // ==================== shop_comment ====================
        {
            let mut cfg = SourceConfig::base(
                "工作室评论举报",
                "execute_process_comment_report",
                |status| {
                    let mut paginated = WhaleReportFetcher::new().fetch_comment_reports_gen(
                        CommentSourceType::All,
                        status,
                        None,
                        None,
                        None,
                    );
                    paginated
                        .fetch_metadata()
                        .map_err(|e| ProcessorError::External(e.into()))?;
                    Ok(json!(paginated.total_items().unwrap_or(0) as i32))
                },
                |status| {
                    let iter = WhaleReportFetcher::new().fetch_comment_reports_gen(
                        CommentSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
            );
            cfg.admin_username_field = "admin_user_name".into();
            cfg.available_actions = actions(&["D", "S", "T", "P", "F", "J"]);
            cfg.content_field = "comment_content".into();
            cfg.content_type_field = "comment_source".into();
            cfg.content_id_field = "comment_id".into();
            cfg.user_id_field = "comment_user_id".into();
            cfg.user_nickname_field = "comment_user_nickname".into();
            cfg.user_parent_id_field = "comment_parent_user_id".into();
            cfg.user_parent_nickname_field = "comment_parent_user_nickname".into();
            cfg.source_id_field = "comment_source_object_id".into();
            cfg.source_name_field = "comment_source_object_name".into();
            cfg.source_type_field = "comment_source".into();
            cfg.source_object_id_field = "comment_source_object_id".into();
            cfg.source_object_name_field = "comment_source_object_name".into();
            cfg.parent_id_field = "comment_parent_id".into();
            cfg.special_check = Some(|item| {
                item.get("comment_source")
                    .and_then(|v| v.as_str())
                    .map(|s| s == "WORK_SHOP")
                    .unwrap_or(false)
            });
            registry.register("shop_comment", cfg);
        }

        // ==================== work_work ====================
        {
            let mut cfg = SourceConfig::base(
                "作品举报",
                "execute_process_work_report",
                |status| {
                    let mut paginated = WhaleReportFetcher::new().fetch_work_reports_gen(
                        WorkSourceType::All,
                        status,
                        None,
                        None,
                        None,
                    );
                    paginated
                        .fetch_metadata()
                        .map_err(|e| ProcessorError::External(e.into()))?;
                    Ok(json!(paginated.total_items().unwrap_or(0) as i32))
                },
                |status| {
                    let iter = WhaleReportFetcher::new().fetch_work_reports_gen(
                        WorkSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
            );
            cfg.admin_username_field = "admin_username".into();
            cfg.available_actions = actions(&["D", "P", "U", "J"]);
            cfg.content_field = "work_name".into();
            cfg.content_type_field = "work_type".into();
            cfg.content_id_field = "work_id".into();
            cfg.user_id_field = "work_user_id".into();
            cfg.user_nickname_field = "work_user_nickname".into();
            cfg.source_id_field = "work_id".into();
            cfg.source_name_field = "work_name".into();
            cfg.source_type_field = "work_type".into();
            cfg.source_object_id_field = "work_id".into();
            cfg.source_object_name_field = "work_name".into();
            cfg.work_type_field = Some("work_type".into());
            cfg.title_field = Some("work_name".into());
            registry.register("work_work", cfg);
        }

        // ==================== forum_post ====================
        {
            let mut cfg = SourceConfig::base(
                "帖子举报",
                "execute_process_post_report",
                |status| {
                    let mut paginated = WhaleReportFetcher::new()
                        .fetch_post_reports_gen(status, None, None, None, None);
                    paginated
                        .fetch_metadata()
                        .map_err(|e| ProcessorError::External(e.into()))?;
                    Ok(json!(paginated.total_items().unwrap_or(0) as i32))
                },
                |status| {
                    let iter = WhaleReportFetcher::new().fetch_post_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
            );
            cfg.admin_username_field = "admin_username".into();
            cfg.available_actions = actions(&["D", "S", "T", "P", "F", "J"]);
            cfg.content_field = "post_title".into();
            cfg.content_type_field = "board_name".into();
            cfg.content_id_field = "post_id".into();
            cfg.user_id_field = "post_user_id".into();
            cfg.user_nickname_field = "post_user_nick_name".into();
            cfg.source_id_field = "post_id".into();
            cfg.source_name_field = "board_name".into();
            cfg.source_type_field = "board_name".into();
            cfg.source_object_id_field = "post_id".into();
            cfg.source_object_name_field = "board_name".into();
            cfg.title_field = Some("post_title".into());
            cfg.board_name_field = Some("board_name".into());
            cfg.board_id_field = Some("board_id".into());
            registry.register("forum_post", cfg);
        }

        // ==================== forum_discussion ====================
        {
            let mut cfg = SourceConfig::base(
                "讨论举报",
                "execute_process_discussion_report",
                |status| {
                    let mut paginated = WhaleReportFetcher::new()
                        .fetch_discussion_reports_gen(status, None, None, None, None);
                    paginated
                        .fetch_metadata()
                        .map_err(|e| ProcessorError::External(e.into()))?;
                    Ok(json!(paginated.total_items().unwrap_or(0) as i32))
                },
                |status| {
                    let iter = WhaleReportFetcher::new().fetch_discussion_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
            );
            cfg.admin_username_field = "admin_username".into();
            cfg.available_actions = actions(&["D", "S", "T", "P", "F", "J"]);
            cfg.content_field = "discussion_content".into();
            cfg.content_type_field = "discussion_source".into();
            cfg.content_id_field = "discussion_id".into();
            cfg.user_id_field = "discussion_user_id".into();
            cfg.user_nickname_field = "discussion_user_nickname".into();
            cfg.source_id_field = "post_id".into();
            cfg.source_name_field = "post_title".into();
            cfg.source_type_field = "discussion_source".into();
            cfg.source_object_id_field = "post_id".into();
            cfg.source_object_name_field = "post_title".into();
            cfg.title_field = Some("post_title".into());
            cfg.board_name_field = Some("board_name".into());
            cfg.board_id_field = Some("board_id".into());
            registry.register("forum_discussion", cfg);
        }

        ReportFetcher { registry }
    }

    pub fn fetch_chunked(&self, status: ReportStatus) -> impl Iterator<Item = Vec<Value>> {
        let report_types = self.registry.get_all_types();
        let total_types = report_types.len();
        let mut type_index = 0;
        let mut pending_items: Vec<Value> = Vec::new();

        std::iter::from_fn(move || {
            let mut chunk = Vec::new();
            std::mem::swap(&mut chunk, &mut pending_items);

            while type_index < total_types {
                let report_type = &report_types[type_index];
                let config = match self.registry.get_config(report_type) {
                    Some(cfg) => cfg,
                    None => {
                        type_index += 1;
                        continue;
                    }
                };

                let generator = (config.fetch_generator)(status);

                for result in generator {
                    let mut item = match result {
                        Ok(item) => item,
                        Err(e) => {
                            eprintln!("Error fetching report data: {}", e);
                            break;
                        }
                    };

                    if status == ReportStatus::ToBeDone
                        && let Some(state) = item.get(&config.status_field).and_then(|v| v.as_str())
                        && state != TO_BE_DONE_STATUS
                    {
                        continue;
                    }

                    if let Value::Object(ref mut map) = item {
                        map.insert("_report_type".into(), Value::String(report_type.clone()));
                    }

                    chunk.push(item);
                    if chunk.len() >= config.chunk_size {
                        let remaining = chunk.split_off(config.chunk_size);
                        pending_items = remaining;
                        return Some(chunk);
                    }
                }
                type_index += 1;
            }

            if chunk.is_empty() { None } else { Some(chunk) }
        })
    }

    pub fn fetch_reports_chunked(&self, status: ReportStatus) -> impl Iterator<Item = Vec<Value>> {
        self.fetch_chunked(status)
    }

    pub fn get_total_reports(&self, status: ReportStatus) -> i64 {
        let mut total = 0i64;
        for rtype in self.registry.get_all_types() {
            if let Some(config) = self.registry.get_config(&rtype)
                && let Ok(result) = (config.fetch_total)(status)
                && let Some(t) = result.as_i64()
            {
                total += t;
            }
        }
        total
    }
}
