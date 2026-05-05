use rand::{Rng, RngExt};
use serde_json::{Map, Value};
use sha2::digest::consts::False;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Debug;
use std::fs;
use std::io::{self, Write};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use crate::api::auth::AuthManager;
use crate::api::forum::{
    ForumActionHandler, ForumDataFetcher, ForumReportReasonId, ItemType, PostReportReasonId,
};
use crate::api::shop::{WorkShopReportReasonId, WorkshopActionHandler, WorkshopDataFetcher};
use crate::api::whale::{
    CommentReportFilterType, CommentSourceType, ReportHandler, ReportStatus, Resolution,
    WhaleReportFetcher, WorkReportFilterType, WorkSourceType,
};
use crate::api::work::{BaseWorkOperations, CommentOperations, WorkDataFetcher};
use crate::utils::acquire::{
    self, BaseKey, ClientFactory, CodeMaoClient, FileUploader, HttpMethod, Identity,
};
use crate::utils::data::{DataManager, PathConfig, SettingManager};

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
impl From<acquire::Error> for ProcessorError {
    fn from(e: acquire::Error) -> Self {
        ProcessorError::External(Box::new(e))
    }
}
impl From<Box<dyn std::error::Error>> for ProcessorError {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        ProcessorError::External(e)
    }
}

// ==================== 交互工具 ====================
fn prompt_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

fn get_valid_input(prompt: &str, valid_options: &HashSet<String>) -> String {
    loop {
        let input = prompt_input(prompt);
        if valid_options.contains(&input.to_uppercase()) {
            return input.to_uppercase();
        }
        println!("无效输入，请重试");
    }
}

// ==================== 辅助函数：从Value中提取字符串或数字 ====================
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn value_to_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

// ==================== 策略模式：评论违规检测 ====================
pub trait CommentProcessStrategy: Send + Sync {
    fn process(
        &self,
        comments: &[Value],
        item_id: i64,
        title: &str,
        params: &HashMap<String, Value>,
        target_lists: &mut HashMap<String, Vec<String>>,
        source_type: &str,
    );
}

struct AdsStrategy;
impl CommentProcessStrategy for AdsStrategy {
    fn process(
        &self,
        comments: &[Value],
        item_id: i64,
        title: &str,
        params: &HashMap<String, Value>,
        target_lists: &mut HashMap<String, Vec<String>>,
        source_type: &str,
    ) {
        let ad_keywords: Vec<String> = params
            .get("ads")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_lowercase()))
                    .collect()
            })
            .unwrap_or_default();

        if ad_keywords.is_empty() {
            return;
        }

        for comment in comments {
            if comment
                .get("is_top")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            process_single_comment(
                comment,
                item_id,
                title,
                &ad_keywords.iter().cloned().collect::<HashSet<_>>(),
                target_lists,
                source_type,
                false,
                "ads",
                |content, keywords| {
                    keywords
                        .iter()
                        .any(|kw| content.as_str().map_or(false, |c| c.contains(kw)))
                },
                |data, log_type, source_type, title, _parent_info| {
                    let content = data.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    let content_preview = content.chars().take(50).collect::<String>();
                    let title_preview = if title.is_empty() {
                        String::new()
                    } else {
                        format!("[{}]", &title[..title.len().min(10)])
                    };
                    format!(
                        "广告 {} [{}]{} : {}",
                        log_type,
                        source_type.to_uppercase(),
                        title_preview,
                        content_preview
                    )
                },
            );
            if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
                for reply in replies {
                    process_single_comment(
                        reply,
                        item_id,
                        title,
                        &ad_keywords.iter().cloned().collect::<HashSet<_>>(),
                        target_lists,
                        source_type,
                        true,
                        "ads",
                        |content, keywords| {
                            keywords
                                .iter()
                                .any(|kw| content.as_str().map_or(false, |c| c.contains(kw)))
                        },
                        |data, log_type, source_type, title, _parent_info| {
                            let content =
                                data.get("content").and_then(|c| c.as_str()).unwrap_or("");
                            let content_preview = content.chars().take(50).collect::<String>();
                            let title_preview = if title.is_empty() {
                                String::new()
                            } else {
                                format!("[{}]", &title[..title.len().min(10)])
                            };
                            format!(
                                "广告 {} [{}]{} : {}",
                                log_type,
                                source_type.to_uppercase(),
                                title_preview,
                                content_preview
                            )
                        },
                    );
                }
            }
        }
    }
}

struct BlacklistStrategy;
impl CommentProcessStrategy for BlacklistStrategy {
    fn process(
        &self,
        comments: &[Value],
        item_id: i64,
        title: &str,
        params: &HashMap<String, Value>,
        target_lists: &mut HashMap<String, Vec<String>>,
        source_type: &str,
    ) {
        let blacklist: HashSet<String> = params
            .get("blacklist")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(|v| value_to_string(v)).collect())
            .unwrap_or_default();

        if blacklist.is_empty() {
            return;
        }

        for comment in comments {
            if comment
                .get("is_top")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            process_single_comment(
                comment,
                item_id,
                title,
                &blacklist,
                target_lists,
                source_type,
                false,
                "blacklist",
                |data, blist| {
                    let uid = data
                        .get("user_id")
                        .map(|v| value_to_string(v))
                        .unwrap_or_default();
                    blist.contains(&uid)
                },
                |data, log_type, source_type, title, _parent_info| {
                    let nickname = data
                        .get("nickname")
                        .and_then(|v| v.as_str())
                        .unwrap_or("未知用户");
                    let title_preview = if title.is_empty() {
                        String::new()
                    } else {
                        format!("[{}]", &title[..title.len().min(10)])
                    };
                    format!(
                        "黑名单 {} [{}]{} : {}",
                        log_type,
                        source_type.to_uppercase(),
                        title_preview,
                        nickname
                    )
                },
            );
            if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
                for reply in replies {
                    process_single_comment(
                        reply,
                        item_id,
                        title,
                        &blacklist,
                        target_lists,
                        source_type,
                        true,
                        "blacklist",
                        |data, blist| {
                            let uid = data
                                .get("user_id")
                                .map(|v| value_to_string(v))
                                .unwrap_or_default();
                            blist.contains(&uid)
                        },
                        |data, log_type, source_type, title, _parent_info| {
                            let nickname = data
                                .get("nickname")
                                .and_then(|v| v.as_str())
                                .unwrap_or("未知用户");
                            let title_preview = if title.is_empty() {
                                String::new()
                            } else {
                                format!("[{}]", &title[..title.len().min(10)])
                            };
                            format!(
                                "黑名单 {} [{}]{} : {}",
                                log_type,
                                source_type.to_uppercase(),
                                title_preview,
                                nickname
                            )
                        },
                    );
                }
            }
        }
    }
}

struct DuplicatesStrategy;
impl CommentProcessStrategy for DuplicatesStrategy {
    fn process(
        &self,
        comments: &[Value],
        item_id: i64,
        _title: &str,
        params: &HashMap<String, Value>,
        target_lists: &mut HashMap<String, Vec<String>>,
        source_type: &str,
    ) {
        let threshold = params
            .get("duplicates")
            .and_then(|v| v.as_i64())
            .unwrap_or(3) as usize;

        let mut content_map: HashMap<(String, String), Vec<String>> = HashMap::new();

        fn track(
            data: &Value,
            item_id: i64,
            map: &mut HashMap<(String, String), Vec<String>>,
            source_type: &str,
            is_reply: bool,
        ) {
            let user_id = data
                .get("user_id")
                .map(|v| value_to_string(v))
                .unwrap_or_default();
            let content = data
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if user_id.is_empty() || content.is_empty() {
                return;
            }
            let id = data.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let parent_id = if is_reply {
                data.get("parent_id").and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            let identifier = if is_reply {
                format!("{}:{}:reply:{}:{}", source_type, item_id, parent_id, id)
            } else {
                format!("{}:{}:comment:0:{}", source_type, item_id, id)
            };
            map.entry((user_id, content)).or_default().push(identifier);
        }

        for comment in comments {
            track(comment, item_id, &mut content_map, source_type, false);
            if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
                for reply in replies {
                    track(reply, item_id, &mut content_map, source_type, true);
                }
            }
        }

        for ((user_id, content), identifiers) in content_map {
            if identifiers.len() >= threshold {
                println!(
                    "用户 {} 刷屏评论: {}... - 出现 {} 次",
                    user_id,
                    &content[..content.len().min(50)],
                    identifiers.len()
                );
                target_lists
                    .entry("duplicates".to_string())
                    .or_default()
                    .extend(identifiers);
            }
        }
    }
}

/// 辅助函数，用于处理单条评论/回复并构建标识符和日志
fn process_single_comment<T: std::hash::Hash + Eq>(
    data: &Value,
    item_id: i64,
    title: &str,
    condition_set: &HashSet<T>,
    target_lists: &mut HashMap<String, Vec<String>>,
    source_type: &str,
    is_reply: bool,
    action_key: &str,
    checker: fn(&Value, &HashSet<T>) -> bool,
    log_formatter: fn(&Value, &str, &str, &str, &str) -> String,
) {
    if !checker(data, condition_set) {
        return;
    }
    let id = data.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let parent_id = if is_reply {
        data.get("parent_id").and_then(|v| v.as_i64()).unwrap_or(0)
    } else {
        0
    };
    let identifier = if is_reply {
        format!("{}:{}:reply:{}:{}", source_type, item_id, parent_id, id)
    } else {
        format!("{}:{}:comment:0:{}", source_type, item_id, id)
    };
    let log_type = if is_reply { "回复" } else { "评论" };
    let parent_info = if is_reply {
        let parent_content = data
            .get("parent_content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if parent_content.is_empty() {
            String::new()
        } else {
            format!(
                "(父内容: {}...)",
                &parent_content[..parent_content.len().min(20)]
            )
        }
    } else {
        String::new()
    };
    let log_msg = log_formatter(data, log_type, source_type, title, &parent_info);
    println!("{}", log_msg);
    target_lists
        .entry(action_key.to_string())
        .or_default()
        .push(identifier);
}

// ==================== 策略工厂 ====================
pub struct StrategyFactory {
    strategies: HashMap<String, Box<dyn CommentProcessStrategy>>,
}

impl StrategyFactory {
    pub fn new() -> Self {
        let mut factory = StrategyFactory {
            strategies: HashMap::new(),
        };
        factory.register("ads", Box::new(AdsStrategy));
        factory.register("blacklist", Box::new(BlacklistStrategy));
        factory.register("duplicates", Box::new(DuplicatesStrategy));
        factory
    }

    pub fn register(&mut self, name: &str, strategy: Box<dyn CommentProcessStrategy>) {
        self.strategies.insert(name.to_string(), strategy);
    }

    pub fn get(&self, name: &str) -> Option<&dyn CommentProcessStrategy> {
        self.strategies.get(name).map(|b| b.as_ref())
    }
}

// ==================== 评论配置接口 (依赖注入) ====================
pub trait CommentConfig {
    fn get_comments(&self, item_id: i64) -> Option<Vec<Value>>;
}

// ==================== 评论处理器 ====================
pub struct CommentProcessor {
    factory: StrategyFactory,
}

impl CommentProcessor {
    pub fn new() -> Self {
        CommentProcessor {
            factory: StrategyFactory::new(),
        }
    }

    pub fn process_item(
        &self,
        item_id: i64,
        title: &str,
        config: &dyn CommentConfig,
        action_type: &str,
        params: &HashMap<String, Value>,
        target_lists: &mut HashMap<String, Vec<String>>,
        source_type: &str,
    ) {
        if let Some(strategy) = self.factory.get(action_type) {
            if let Some(comments) = config.get_comments(item_id) {
                strategy.process(&comments, item_id, title, params, target_lists, source_type);
            }
        }
    }
}

// ==================== 举报类型注册表 ====================
#[derive(Debug, Clone)]
pub struct ActionConfig {
    pub key: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct SourceConfig {
    pub name: String,
    pub fetch_total: fn(status: ReportStatus) -> Result<Value, ProcessorError>,
    pub fetch_generator:
        fn(status: ReportStatus) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>>>,
    pub handle_method: String,
    pub item_id_field: String,
    pub report_id_field: String,
    pub reason_field: String,
    pub description_field: String,
    pub status_field: String,
    pub admin_id_field: String,
    pub admin_username_field: String,
    pub content_field: String,
    pub user_id_field: String,
    pub user_nickname_field: String,
    pub source_id_field: String,
    pub source_name_field: String,
    pub available_actions: Vec<ActionConfig>,
}

pub struct ReportTypeRegistry {
    registry: HashMap<String, SourceConfig>,
    default_actions: Vec<ActionConfig>,
}

impl ReportTypeRegistry {
    pub fn new() -> Self {
        let default_actions = vec![
            ActionConfig {
                key: "D".into(),
                name: "删除".into(),
                description: "删除内容".into(),
                status: "DELETE".into(),
                enabled: true,
            },
            ActionConfig {
                key: "S".into(),
                name: "禁言7天".into(),
                description: "禁言用户7天".into(),
                status: "MUTE_SEVEN_DAYS".into(),
                enabled: true,
            },
            ActionConfig {
                key: "T".into(),
                name: "禁言3月".into(),
                description: "禁言用户3个月".into(),
                status: "MUTE_THREE_MONTHS".into(),
                enabled: true,
            },
            ActionConfig {
                key: "U".into(),
                name: "取消发布".into(),
                description: "取消作品发布".into(),
                status: "UNLOAD".into(),
                enabled: true,
            },
            ActionConfig {
                key: "P".into(),
                name: "通过".into(),
                description: "通过举报".into(),
                status: "PASS".into(),
                enabled: true,
            },
            ActionConfig {
                key: "F".into(),
                name: "检查违规".into(),
                description: "检查其他违规内容".into(),
                status: "CHECK_VIOLATION".into(),
                enabled: true,
            },
            ActionConfig {
                key: "J".into(),
                name: "跳过".into(),
                description: "跳过当前举报".into(),
                status: "SKIP".into(),
                enabled: true,
            },
            ActionConfig {
                key: "C".into(),
                name: "查看详情".into(),
                description: "查看详细内容".into(),
                status: "VIEW".into(),
                enabled: false,
            }, // 默认不显示
        ];
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

    pub fn get_available_actions(&self, report_type: &str) -> Vec<&ActionConfig> {
        if let Some(config) = self.get_config(report_type) {
            config
                .available_actions
                .iter()
                .filter(|a| a.enabled && a.key != "C")
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
        for action in &self.default_actions {
            if matches!(action.key.as_str(), "D" | "S" | "T" | "P") {
                map.insert(action.key.clone(), action.status.clone());
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
    pub fn new() -> Self {
        let mut registry = ReportTypeRegistry::new();
        // ===== 注册商店评论举报 =====
        registry.register(
            "shop_comment",
            SourceConfig {
                name: "工作室评论举报".into(),
                fetch_total: |status| {
                    WhaleReportFetcher::new()
                        .fetch_comment_reports_total(CommentSourceType::All, status, None, None)
                        .map_err(|e| ProcessorError::External(e.into()))
                },
                fetch_generator: |status| {
                    let iter = WhaleReportFetcher::new().fetch_comment_reports_gen(
                        CommentSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
                handle_method: "execute_process_comment_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_user_name".into(),
                content_field: "comment_content".into(),
                user_id_field: "comment_user_id".into(),
                user_nickname_field: "comment_user_nickname".into(),
                source_id_field: "comment_source_object_id".into(),
                source_name_field: "comment_source_object_name".into(),
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

        // ===== 注册作品举报 =====
        registry.register(
            "work_work",
            SourceConfig {
                name: "作品举报".into(),
                fetch_total: |status| {
                    WhaleReportFetcher::new()
                        .fetch_work_reports_total(WorkSourceType::All, status, None, None)
                        .map_err(|e| ProcessorError::External(e.into()))
                },
                fetch_generator: |status| {
                    let iter = WhaleReportFetcher::new().fetch_work_reports_gen(
                        WorkSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
                handle_method: "execute_process_work_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_username".into(),
                content_field: "work_name".into(),
                user_id_field: "work_user_id".into(),
                user_nickname_field: "work_user_nickname".into(),
                source_id_field: "work_id".into(),
                source_name_field: "work_name".into(),
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

        // ===== 注册论坛帖子举报 =====
        registry.register(
            "forum_post",
            SourceConfig {
                name: "帖子举报".into(),
                fetch_total: |status| {
                    WhaleReportFetcher::new()
                        .fetch_post_reports_total(status, None, None, None)
                        .map_err(|e| ProcessorError::External(e.into()))
                },
                fetch_generator: |status| {
                    let iter = WhaleReportFetcher::new().fetch_post_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
                handle_method: "execute_process_post_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_username".into(),
                content_field: "post_title".into(),
                user_id_field: "post_user_id".into(),
                user_nickname_field: "post_user_nick_name".into(),
                source_id_field: "post_id".into(),
                source_name_field: "board_name".into(),
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

        // ===== 注册论坛讨论举报 =====
        registry.register(
            "forum_discussion",
            SourceConfig {
                name: "讨论举报".into(),
                fetch_total: |status| {
                    WhaleReportFetcher::new()
                        .fetch_discussion_reports_total(status, None, None, None)
                        .map_err(|e| ProcessorError::External(e.into()))
                },
                fetch_generator: |status| {
                    let iter = WhaleReportFetcher::new().fetch_discussion_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    );
                    Box::new(iter.map(|r| r.map_err(|e| ProcessorError::External(e.into()))))
                },
                handle_method: "execute_process_discussion_report".into(),
                item_id_field: "id".into(),
                report_id_field: "id".into(),
                reason_field: "reason_content".into(),
                description_field: "description".into(),
                status_field: "status".into(),
                admin_id_field: "admin_id".into(),
                admin_username_field: "admin_username".into(),
                content_field: "discussion_content".into(),
                user_id_field: "discussion_user_id".into(),
                user_nickname_field: "discussion_user_nickname".into(),
                source_id_field: "post_id".into(),
                source_name_field: "post_title".into(),
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

    /// 分块获取举报，跨类型轮询
    pub fn fetch_chunked(&self, status: ReportStatus) -> impl Iterator<Item = Vec<Value>> {
        let report_types = self.registry.registry.keys().cloned().collect::<Vec<_>>();
        let total_types = report_types.len();
        let mut type_index = 0;
        let mut carry_over = Vec::<Value>::new();
        std::iter::from_fn(move || {
            let mut chunk = Vec::new();
            std::mem::swap(&mut chunk, &mut carry_over);

            while type_index < total_types {
                let report_type = &report_types[type_index];
                let config = match self.registry.get_config(report_type) {
                    Some(cfg) => cfg,
                    None => {
                        type_index += 1;
                        continue;
                    }
                };
                let generator = (config.fetch_generator)(status.clone());
                let mut type_items = Vec::new();
                for result in generator {
                    match result {
                        Ok(item) => {
                            if status == ReportStatus::ToBeDone {
                                if let Some(state) =
                                    item.get(&config.status_field).and_then(|v| v.as_str())
                                {
                                    if state != "TOBEDONE" {
                                        continue;
                                    }
                                }
                            }
                            type_items.push(item);
                            if type_items.len() >= 100 {
                                carry_over = type_items.clone();
                                break;
                            }
                        }
                        Err(error) => {
                            eprintln!("Error fetching report data: {}", error);
                            break;
                        }
                    }
                }
                chunk.extend(type_items);
                if chunk.len() < 100 {
                    type_index += 1;
                } else {
                    break;
                }
            }
            if chunk.is_empty() && type_index >= total_types {
                None
            } else if !chunk.is_empty() {
                Some(chunk)
            } else {
                None
            }
        })
    }

    /// 获取所有举报类型的总数
    pub fn get_total_reports(&self, status: ReportStatus) -> i64 {
        let mut total = 0i64;
        for rtype in self.registry.registry.keys() {
            if let Some(config) = self.registry.get_config(rtype) {
                if let Ok(result) = (config.fetch_total)(status) {
                    if let Some(t) = result.get("total").and_then(|v| v.as_i64()) {
                        total += t;
                    }
                }
            }
        }
        total
    }
}

// ==================== 处理管道构件 ====================
#[derive(Debug, Clone)]
pub struct ProcessingContext {
    pub record_id: String,
    pub report_type: String,
    pub item: Value,
    pub admin_id: i32,
    pub processed: bool,
    pub action: Option<String>,
    pub skip_reason: Option<String>,
    pub messages: Vec<String>,
}

pub trait Processor: Send + Sync {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError>;
}

/// 官方账号检查处理器
pub struct OfficialCheckProcessor;
impl Processor for OfficialCheckProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        let official_ids: HashSet<i64> = vec![
            128963, 629055, 203577, 859722, 148883, 2191000, 7492052, 387963, 3649031,
        ]
        .into_iter()
        .collect();
        if let Some(user_id) = context
            .item
            .get("user_id")
            .or_else(|| context.item.get("comment_user_id"))
            .or_else(|| context.item.get("work_user_id"))
            .or_else(|| context.item.get("post_user_id"))
            .or_else(|| context.item.get("discussion_user_id"))
            .and_then(|v| value_to_i64(v))
        {
            if official_ids.contains(&user_id) {
                context.messages.push("官方内容，自动通过".into());
                context.action = Some("P".into());
                context.processed = true;
                let status_map = HashMap::from([
                    ("D".to_string(), "DELETE".to_string()),
                    ("S".to_string(), "MUTE_SEVEN_DAYS".to_string()),
                    ("T".to_string(), "MUTE_THREE_MONTHS".to_string()),
                    ("P".to_string(), "PASS".to_string()),
                ]);
                if let Some(resolution) = status_map.get("P") {
                    println!("自动通过官方举报ID: {}", context.record_id);
                }
            }
        }
        Ok(())
    }
}

/// 详情显示处理器
pub struct DetailDisplayProcessor;
impl Processor for DetailDisplayProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        println!("=== {} 举报详情 ===", context.report_type);
        if let Some(reason) = context.item.get("reason_content").and_then(|v| v.as_str()) {
            println!("举报原因: {}", reason);
        }
        if let Some(desc) = context.item.get("description").and_then(|v| v.as_str()) {
            println!("举报描述: {}", desc);
        }
        Ok(())
    }
}

/// 动作选择处理器
pub struct ActionSelectionProcessor {
    pub registry: Arc<ReportTypeRegistry>,
}
impl Processor for ActionSelectionProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        let actions = self.registry.get_available_actions(&context.report_type);
        let valid_keys: HashSet<String> = actions.iter().map(|a| a.key.clone()).collect();
        let prompt = self.registry.get_action_prompt(&context.report_type);
        loop {
            let choice = get_valid_input(&prompt, &valid_keys);
            match choice.as_str() {
                "D" | "S" | "T" | "P" => {
                    context.action = Some(choice.clone());
                    if let Some(config) = self.registry.get_config(&context.report_type) {
                        let status_map = self.registry.get_status_mapping();
                        if let Some(resolution) = status_map.get(&choice) {
                            let resolution_enum = match resolution.as_str() {
                                "DELETE" => Resolution::Delete,
                                "MUTE_SEVEN_DAYS" => Resolution::MuteSevenDays,
                                "MUTE_THREE_MONTHS" => Resolution::MuteThreeMonths,
                                "PASS" => Resolution::Pass,
                                _ => {
                                    return Err(ProcessorError::Processing(format!(
                                        "未知状态: {}",
                                        resolution
                                    )));
                                }
                            };

                            let report_id: i32 = context.item["id"].as_i64().unwrap_or(0) as i32;
                            match config.handle_method.as_str() {
                                "execute_process_comment_report" => {
                                    ReportHandler::new()
                                        .execute_process_comment_report(
                                            report_id,
                                            context.admin_id,
                                            resolution_enum,
                                        )
                                        .map_err(|e| ProcessorError::External(e.into()))?;
                                }
                                "execute_process_work_report" => {
                                    ReportHandler::new()
                                        .execute_process_work_report(
                                            report_id,
                                            context.admin_id,
                                            resolution_enum,
                                        )
                                        .map_err(|e| ProcessorError::External(e.into()))?;
                                }
                                "execute_process_post_report" => {
                                    ReportHandler::new()
                                        .execute_process_post_report(
                                            report_id,
                                            context.admin_id,
                                            resolution_enum,
                                        )
                                        .map_err(|e| ProcessorError::External(e.into()))?;
                                }
                                "execute_process_discussion_report" => {
                                    ReportHandler::new()
                                        .execute_process_discussion_report(
                                            report_id,
                                            context.admin_id,
                                            resolution_enum,
                                        )
                                        .map_err(|e| ProcessorError::External(e.into()))?;
                                }
                                _ => {
                                    return Err(ProcessorError::Processing(format!(
                                        "未知处理方法: {}",
                                        config.handle_method
                                    )));
                                }
                            }
                            println!("已应用操作: {} -> {}", choice, resolution);
                        }
                    }
                    context.processed = true;
                    break;
                }
                "F" => {
                    println!("执行违规检查...");
                    continue;
                }
                "J" => {
                    context.skip_reason = Some("用户选择跳过".into());
                    context.processed = true;
                    break;
                }
                _ => unreachable!(),
            }
        }
        Ok(())
    }
}

// ==================== 处理管道 ====================
pub struct ProcessingPipeline {
    processors: Vec<Box<dyn Processor>>,
}

impl ProcessingPipeline {
    pub fn new(processors: Vec<Box<dyn Processor>>) -> Self {
        ProcessingPipeline { processors }
    }

    pub fn execute(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        for processor in &self.processors {
            if context.processed || context.skip_reason.is_some() {
                break;
            }
            processor.process(context)?;
        }
        Ok(())
    }

    pub fn create_default(registry: Arc<ReportTypeRegistry>) -> Self {
        ProcessingPipeline::new(vec![
            Box::new(OfficialCheckProcessor),
            Box::new(DetailDisplayProcessor),
            Box::new(ActionSelectionProcessor { registry }),
        ])
    }
}

// ==================== 违规检查器 (自动举报相关) ====================
pub struct ViolationChecker {
    pub comment_processor: CommentProcessor,
}

impl ViolationChecker {
    pub fn new() -> Self {
        ViolationChecker {
            comment_processor: CommentProcessor::new(),
        }
    }

    pub fn check_violation(
        &self,
        source_id: i64,
        source_type: &str,
        board_name: &str,
        user_id: Option<i64>,
    ) -> Result<(), ProcessorError> {
        println!(
            "检查违规: source_id={}, type={}, board={}, user={:?}",
            source_id, source_type, board_name, user_id
        );

        let total = self.get_comment_total(source_id, source_type)?;
        println!("该内容共有 {} 条评论", total);
        let limit_str = prompt_input("输入要获取的评论数: ");
        let limit: usize = limit_str.parse().unwrap_or(100);

        let comments = self.fetch_comments(source_id, source_type, limit)?;

        let data = DataManager::global()
            .data()
            .map_err(|e| ProcessorError::External(e.into()))?;
        let ads: Vec<Value> = data
            .user_data
            .ads
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect();
        let blacklist: Vec<Value> = data
            .user_data
            .black_room
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect();
        let setting = SettingManager::global()
            .data()
            .map_err(|e| ProcessorError::External(e.into()))?;
        let spam_max = setting.parameter.spam_del_max;

        let mut params = HashMap::new();
        params.insert("ads".to_string(), Value::Array(ads));
        params.insert("blacklist".to_string(), Value::Array(blacklist));
        params.insert(
            "duplicates".to_string(),
            Value::Number(serde_json::Number::from(spam_max)),
        );

        let mut target_lists: HashMap<String, Vec<String>> = HashMap::new();

        struct SimpleCommentConfig {
            comments: Vec<Value>,
        }
        impl CommentConfig for SimpleCommentConfig {
            fn get_comments(&self, _item_id: i64) -> Option<Vec<Value>> {
                Some(self.comments.clone())
            }
        }
        let config = SimpleCommentConfig {
            comments: comments.clone(),
        };

        for check_type in &["ads", "blacklist", "duplicates"] {
            self.comment_processor.process_item(
                source_id,
                board_name,
                &config,
                check_type,
                &params,
                &mut target_lists,
                source_type,
            );
        }

        let mut violations: Vec<String> = Vec::new();
        if let Some(ads) = target_lists.get("ads") {
            violations.extend(ads.clone());
        }
        if let Some(bl) = target_lists.get("blacklist") {
            violations.extend(bl.clone());
        }
        if let Some(dup) = target_lists.get("duplicates") {
            violations.extend(dup.clone());
        }
        let violations: HashSet<String> = violations.into_iter().collect();
        if violations.is_empty() {
            println!("未检测到违规内容");
            return Ok(());
        }

        println!("检测到 {} 条违规内容", violations.len());
        self.process_auto_report(violations)
    }

    fn get_comment_total(&self, source_id: i64, source_type: &str) -> Result<i64, ProcessorError> {
        match source_type {
            "work" => {
                let resp = ClientFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "1")
                    .send()?;
                let json = ClientFactory::global_client().response_to_json(resp)?;
                Ok(json.get("total").and_then(|v| v.as_i64()).unwrap_or(0))
            }
            "shop" => {
                let resp = ClientFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/discussions/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("source", "WORK_SHOP")
                    .with_param("limit", "1")
                    .send()?;
                let json = ClientFactory::global_client().response_to_json(resp)?;
                let total = json.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
                let total_reply = json.get("totalReply").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(total + total_reply)
            }
            "forum" => {
                let resp = ClientFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/forums/posts/{}/details", source_id),
                        Some(BaseKey::Default),
                    )
                    .send()?;
                let json = ClientFactory::global_client().response_to_json(resp)?;
                let n_replies = json.get("n_replies").and_then(|v| v.as_i64()).unwrap_or(0);
                let n_comments = json.get("n_comments").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(n_replies + n_comments)
            }
            _ => Err(ProcessorError::Processing(format!(
                "不支持的来源类型: {}",
                source_type
            ))),
        }
    }

    fn fetch_comments(
        &self,
        source_id: i64,
        source_type: &str,
        limit: usize,
    ) -> Result<Vec<Value>, ProcessorError> {
        match source_type {
            "work" => {
                let iter =
                    WorkDataFetcher::new().fetch_work_comments_gen(source_id as i32, Some(limit));
                let comments: Result<Vec<Value>, _> = iter.collect();
                comments.map_err(|e| ProcessorError::External(e.into()))
            }
            "forum" => {
                let iter = ForumDataFetcher::new().fetch_post_replies_gen(
                    source_id as i32,
                    None,
                    Some(limit),
                );
                let mut comments = Vec::new();
                for item in iter {
                    match item {
                        Ok(v) => comments.push(v),
                        Err(e) => break,
                    }
                }
                Ok(comments)
            }
            "shop" => {
                let iter = WorkshopDataFetcher::new().fetch_workshop_discussions_gen(
                    source_id as i32,
                    None,
                    None,
                    Some(limit),
                );
                let mut comments = Vec::new();
                for item in iter {
                    match item {
                        Ok(v) => comments.push(v),
                        Err(e) => break,
                    }
                }
                Ok(comments)
            }
            _ => Err(ProcessorError::Processing(format!(
                "不支持的来源类型: {}",
                source_type
            ))),
        }
    }

    fn process_auto_report(&self, violations: HashSet<String>) -> Result<(), ProcessorError> {
        let mut multi_account = MultiAccount::new(Identity::Edu);
        let password_path = PathConfig::password_file_path();
        if password_path.exists() {
            multi_account.load_from_file(&password_path)?;
        } else {
            println!("未找到学生账号文件，跳过自动举报");
            return Ok(());
        }

        let choice = get_valid_input(
            "是否自动举报违规评论? (Y/N)",
            &["Y".into(), "N".into()].into_iter().collect(),
        );
        if choice != "Y" {
            println!("自动举报取消");
            return Ok(());
        }

        let mut accounts = multi_account.accounts.clone();
        if accounts.is_empty() {
            println!("没有可用账号");
            return Ok(());
        }

        let reason_content = "违规内容";

        let mut success = 0;
        let mut account_usage = HashMap::new();
        let mut account_index = 0;

        for (idx, violation) in violations.iter().enumerate() {
            if account_usage.get(&account_index).copied().unwrap_or(0) >= 25 {
                account_index = (account_index + 1) % accounts.len();
            }
            if account_usage.get(&account_index).copied().unwrap_or(0) == 0 {
                let (user, pass) = &accounts[account_index];
                if let Err(e) = self.login_student(user, pass) {
                    println!("账号 {} 登录失败: {}", user, e);
                    accounts.remove(account_index);
                    if accounts.is_empty() {
                        break;
                    }
                    account_index = 0;
                    continue;
                }
            }

            match self.execute_single_report(violation, &reason_content) {
                Ok(_) => {
                    success += 1;
                    let usage = account_usage.entry(account_index).or_insert(0);
                    *usage += 1;
                    println!("[{}/{}] 举报成功: {}", idx + 1, violations.len(), violation);
                }
                Err(e) => {
                    println!(
                        "[{}/{}] 举报失败: {} - {}",
                        idx + 1,
                        violations.len(),
                        violation,
                        e
                    );
                }
            }
        }

        ClientFactory::global_client()
            .switch_identity(Identity::Judgement)
            .ok();
        println!("自动举报完成，成功 {}/{}", success, violations.len());
        Ok(())
    }

    fn login_student(&self, username: &str, password: &str) -> Result<(), ProcessorError> {
        crate::auth::LoginBuilder::new()
            .identity(username)
            .password(password)
            .status(crate::api::auth::AccountStatus::Edu)
            .execute()
            .map_err(|e| ProcessorError::External(e.into()))?;
        Ok(())
    }

    fn execute_single_report(
        &self,
        violation: &str,
        reason_content: &str,
    ) -> Result<(), ProcessorError> {
        let parts: Vec<&str> = violation.split(':').collect();
        if parts.len() != 5 {
            return Err(ProcessorError::Processing("违规标识符格式错误".into()));
        }
        let source = parts[0];
        let source_id: i64 = parts[1].parse()?;
        let vtype = parts[2];
        let _parent_id: i32 = parts[3].parse()?;
        let content_id: i32 = parts[4].parse()?;

        match vtype {
            "post" => {
                if source != "forum" {
                    return Err(ProcessorError::Processing("不能在非论坛举报帖子".into()));
                }
                ForumActionHandler::new()
                    .report_post(
                        content_id,
                        PostReportReasonId::Reason7,
                        reason_content,
                        false,
                    )
                    .map_err(|e| ProcessorError::External(e.into()))?;
            }
            "work" => {
                BaseWorkOperations::new()
                    .execute_report_work(content_id, reason_content, reason_content)
                    .map_err(|e| ProcessorError::External(e.into()))?;
            }
            "comment" | "reply" => {
                let is_reply = vtype == "reply";
                match source {
                    "work" => {
                        CommentOperations::new()
                            .execute_report_comment(
                                source_id as i32,
                                content_id as i32,
                                reason_content,
                            )
                            .map_err(|e| ProcessorError::External(e.into()))?;
                    }
                    "forum" => {
                        let item_type = if is_reply {
                            ItemType::Reply
                        } else {
                            ItemType::Comment
                        };
                        ForumActionHandler::new()
                            .report_item(
                                content_id,
                                ForumReportReasonId::Reason7,
                                "",
                                item_type,
                                false,
                            )
                            .map_err(|e| ProcessorError::External(e.into()))?;
                    }
                    "shop" => {
                        let reporter_id = rand::rng().random_range(10000..=199999999);
                        if is_reply {
                            WorkshopActionHandler::new()
                                .execute_report_comment(
                                    content_id,
                                    reason_content,
                                    WorkShopReportReasonId::Reason7,
                                    reporter_id,
                                    None,
                                    Some(_parent_id),
                                    Some(""),
                                )
                                .map_err(|e| ProcessorError::External(e.into()))?;
                        } else {
                            WorkshopActionHandler::new()
                                .execute_report_comment(
                                    content_id,
                                    reason_content,
                                    WorkShopReportReasonId::Reason7,
                                    reporter_id,
                                    None,
                                    None,
                                    Some(""),
                                )
                                .map_err(|e| ProcessorError::External(e.into()))?;
                        }
                    }
                    _ => return Err(ProcessorError::Processing("不支持的来源".into())),
                }
            }
            _ => {
                return Err(ProcessorError::Processing(format!(
                    "未知违规类型: {}",
                    vtype
                )));
            }
        }
        Ok(())
    }
}

// ==================== 多账号管理器 ====================
pub struct MultiAccount {
    pub accounts: Vec<(String, String)>,
    identity_type: Identity,
}

impl MultiAccount {
    pub fn new(identity_type: Identity) -> Self {
        MultiAccount {
            accounts: Vec::new(),
            identity_type,
        }
    }

    pub fn load_from_file(&mut self, path: &Path) -> Result<(), ProcessorError> {
        let content = fs::read_to_string(path)?;
        self.accounts.clear();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((user, pass)) = line.split_once(':') {
                self.accounts
                    .push((user.trim().to_string(), pass.trim().to_string()));
            }
        }
        Ok(())
    }

    pub fn execute_with_accounts<F>(&self, func: F, limit: Option<usize>, delay_secs: u64)
    where
        F: Fn(),
    {
        let accs = if let Some(lim) = limit {
            &self.accounts[..lim.min(self.accounts.len())]
        } else {
            &self.accounts[..]
        };
        for (i, _) in accs.iter().enumerate() {
            func();
            if i < accs.len() - 1 && delay_secs > 0 {
                thread::sleep(Duration::from_secs(delay_secs));
            }
        }
    }
}

// ==================== 文件处理器 ====================
pub struct FileProcessor;

impl FileProcessor {
    pub fn handle_file_upload(
        file_path: &Path,
        save_path: &str,
        method: &str,
    ) -> Result<String, ProcessorError> {
        let client = ClientFactory::global_client().clone();
        let uploader = FileUploader::new(client);
        let url = uploader
            .upload(file_path, method, save_path)
            .map_err(|e| ProcessorError::External(e.into()))?;
        Ok(url)
    }

    pub fn handle_directory_upload(
        dir_path: &Path,
        save_path: &str,
        method: &str,
    ) -> Result<HashMap<PathBuf, String>, ProcessorError> {
        let mut results = HashMap::new();
        visit_dir(dir_path, &mut |entry| {
            if entry.file_type().map_or(false, |ft| ft.is_file()) {
                let path = entry.path();
                let url = Self::handle_file_upload(path.as_path(), save_path, method)?;
                results.insert(path.to_path_buf(), url);
            }
            Ok(())
        })?;
        Ok(results)
    }
}

fn visit_dir(
    dir: &Path,
    cb: &mut dyn FnMut(fs::DirEntry) -> Result<(), ProcessorError>,
) -> io::Result<()> {
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dir(&path, cb)?;
            } else {
                cb(entry).map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            }
        }
    }
    Ok(())
}

// ==================== 主举报处理器 (整合管道) ====================
pub struct ReportProcessor {
    pub fetcher: ReportFetcher,
    pub pipeline_factory: Arc<ReportTypeRegistry>,
}

impl ReportProcessor {
    pub fn new() -> Self {
        let fetcher = ReportFetcher::new();
        let registry = Arc::new(ReportTypeRegistry::new());
        ReportProcessor {
            fetcher,
            pipeline_factory: registry,
        }
    }

    pub fn process_all_reports(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        println!("=== 开始处理所有举报 ===");
        let total = self.fetcher.get_total_reports(ReportStatus::ToBeDone);
        println!("当前待处理举报总数: {}", total);
        if total == 0 {
            return Ok(0);
        }

        let choice = get_valid_input(
            "是否一键全部通过? (Y/N)",
            &["Y".into(), "N".into()].into_iter().collect(),
        );
        if choice == "Y" {
            return self.pass_all_pending(admin_id);
        }

        let mut processed = 0i64;
        for chunk in self.fetcher.fetch_chunked(ReportStatus::ToBeDone) {
            println!("正在处理一批 {} 条举报", chunk.len());
            for report_item in chunk {
                if let Some(report_type) = self.infer_report_type(&report_item) {
                    let config = match self.fetcher.registry.get_config(&report_type) {
                        Some(c) => c,
                        None => continue,
                    };
                    let item_id = report_item[&config.item_id_field]
                        .as_i64()
                        .unwrap_or(0)
                        .to_string();
                    let mut context = ProcessingContext {
                        record_id: item_id.clone(),
                        report_type: report_type.clone(),
                        item: report_item.clone(),
                        admin_id,
                        processed: false,
                        action: None,
                        skip_reason: None,
                        messages: Vec::new(),
                    };
                    let pipeline =
                        ProcessingPipeline::create_default(self.pipeline_factory.clone());
                    pipeline.execute(&mut context)?;
                    if context.processed {
                        processed += 1;
                    }
                }
            }
        }
        println!("处理完成，共处理 {} 条举报", processed);
        Ok(processed)
    }

    fn infer_report_type(&self, item: &Value) -> Option<String> {
        if item.get("comment_content").is_some() || item.get("comment_id").is_some() {
            Some("shop_comment".into())
        } else if item.get("work_name").is_some() {
            Some("work_work".into())
        } else if item.get("post_title").is_some() && item.get("board_name").is_some() {
            Some("forum_post".into())
        } else if item.get("discussion_content").is_some() {
            Some("forum_discussion".into())
        } else {
            None
        }
    }

    fn pass_all_pending(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        let mut count = 0i64;
        for chunk in self.fetcher.fetch_chunked(ReportStatus::ToBeDone) {
            for report_item in chunk {
                if let Some(report_type) = self.infer_report_type(&report_item) {
                    let config = match self.fetcher.registry.get_config(&report_type) {
                        Some(c) => c,
                        None => continue,
                    };
                    let report_id = report_item["id"].as_i64().unwrap_or(0) as i32;

                    match config.handle_method.as_str() {
                        "execute_process_comment_report" => {
                            ReportHandler::new()
                                .execute_process_comment_report(
                                    report_id,
                                    admin_id,
                                    Resolution::Pass,
                                )
                                .ok();
                        }
                        "execute_process_work_report" => {
                            ReportHandler::new()
                                .execute_process_work_report(report_id, admin_id, Resolution::Pass)
                                .ok();
                        }
                        "execute_process_post_report" => {
                            ReportHandler::new()
                                .execute_process_post_report(report_id, admin_id, Resolution::Pass)
                                .ok();
                        }
                        "execute_process_discussion_report" => {
                            ReportHandler::new()
                                .execute_process_discussion_report(
                                    report_id,
                                    admin_id,
                                    Resolution::Pass,
                                )
                                .ok();
                        }
                        _ => {}
                    }
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}

// ==================== 全局单例 ====================
static COMMENT_PROCESSOR: OnceLock<CommentProcessor> = OnceLock::new();
static VIOLATION_CHECKER: OnceLock<ViolationChecker> = OnceLock::new();
fn comment_processor() -> &'static CommentProcessor {
    COMMENT_PROCESSOR.get_or_init(CommentProcessor::new)
}

fn violation_checker() -> &'static ViolationChecker {
    VIOLATION_CHECKER.get_or_init(ViolationChecker::new)
}
