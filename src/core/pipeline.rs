use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use fastrand;
use log::{error, info, warn};
use serde_json::Value;

use super::types::{
    ActionConfig, CommentConfig, ProcessorError, ReportTypeRegistry, SourceConfig, get_valid_input,
    html_to_text, prompt_input, timestamp_to_string, value_to_i64, value_to_string,
};
use crate::api::forum::{
    ForumActionHandler, ForumDataFetcher, ForumReportReasonId, ItemType, PostReportReasonId,
};
use crate::api::shop::{WorkShopReportReasonId, WorkshopActionHandler};
use crate::api::whale::{ReportHandler, Resolution};
use crate::api::work::{BaseWorkOperations, CommentOperations};
use crate::core::retrieve::{CommentSource, DataQuery, JsonObject};
use crate::utils::acquire::{Catsona, KittyFactory};
use crate::utils::data::PathConfig;

// ==================== 配置结构体（依赖注入） ====================
#[derive(Clone)]
pub struct CheckConfig {
    pub official_ids: &'static [i64],
    pub ad_keywords: &'static [&'static str],
    pub spam_threshold: usize,
    pub comment_fetch_default_limit: usize,
    pub max_reports_per_account: usize,
    pub batch_item_id_threshold: usize, // 新增
    pub batch_content_threshold: usize, // 新增
}

impl Default for CheckConfig {
    fn default() -> Self {
        CheckConfig {
            official_ids: &[
                128963, 629055, 203577, 859722, 148883, 2191000, 7492052, 387963, 3649031,
            ],
            ad_keywords: &[
                "codemao.cn/work",
                "cpdd",
                "scp",
                "不喜可删",
                "互关",
                "互赞",
                "交友",
                "光头强",
                "关注",
                "再创作",
                "冲传说",
                "冲大佬",
                "冲高手",
                "协作项目",
                "基金会",
                "处cp",
                "家族招人",
                "我的作品",
                "戴雨默",
                "所有作品",
                "扫厕所",
                "找徒弟",
                "找闺",
                "招人",
                "有赞必回",
                "点个",
                "爬虫",
                "看一下我的",
                "看我的",
                "看看我的",
                "粘贴到别人作品",
                "赞我",
                "转发",
            ],
            spam_threshold: 3,
            comment_fetch_default_limit: 100,
            max_reports_per_account: 25,
            batch_item_id_threshold: 5, // 新增：item_id 组阈值
            batch_content_threshold: 3, // 新增：content 组阈值
        }
    }
}

// ==================== 静态映射与注册表 ====================
/// 来源类型映射
static SOURCE_TYPE_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
fn get_source_type_map() -> &'static HashMap<&'static str, &'static str> {
    SOURCE_TYPE_MAP.get_or_init(|| {
        HashMap::from([
            ("shop_comment", "shop"),
            ("forum_post", "forum"),
            ("forum_discussion", "forum"),
        ])
    })
}

// ==================== 公共工具函数 ====================
fn title_preview_str(title: &str) -> String {
    if title.is_empty() {
        String::new()
    } else {
        format!("[{}]", &title[..title.len().min(10)])
    }
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        s.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn build_identifier(source_type: &str, item_id: i64, data: &Value, is_reply: bool) -> String {
    let content_id = data.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let parent_id = if is_reply {
        data.get("parent_id").and_then(|v| v.as_i64()).unwrap_or(0)
    } else {
        0
    };
    format!(
        "{}:{}:{}:{}:{}",
        source_type,
        item_id,
        if is_reply { "reply" } else { "comment" },
        parent_id,
        content_id
    )
}

fn for_each_comment_reply(comments: &[Value], mut handler: impl FnMut(&Value, bool)) {
    for comment in comments {
        if comment
            .get("is_top")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        handler(comment, false);
        if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
            for reply in replies {
                handler(reply, true);
            }
        }
    }
}

pub(crate) fn parse_resolution(resolution: &str) -> Result<Resolution, ProcessorError> {
    match resolution {
        "DELETE" => Ok(Resolution::Delete),
        "MUTE_SEVEN_DAYS" => Ok(Resolution::MuteSevenDays),
        "MUTE_THREE_MONTHS" => Ok(Resolution::MuteThreeMonths),
        "PASS" => Ok(Resolution::Pass),
        "UNLOAD" => Ok(Resolution::Unload),
        _ => Err(ProcessorError::Processing(format!(
            "未知状态: {}",
            resolution
        ))),
    }
}

pub trait ReportIdExt {
    fn get_report_id(&self, item: &Value) -> Result<i32, ProcessorError>;
}

impl ReportIdExt for SourceConfig {
    fn get_report_id(&self, item: &Value) -> Result<i32, ProcessorError> {
        item.get(&self.report_id_field)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| {
                ProcessorError::Processing(format!(
                    "无法解析 report_id 字段: {}",
                    self.report_id_field
                ))
            })
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
        let ad_keywords: HashSet<String> = params
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

        for_each_comment_reply(comments, |data, is_reply| {
            let content = data
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if ad_keywords.iter().any(|kw| content.contains(kw)) {
                let identifier = build_identifier(source_type, item_id, data, is_reply);
                let log_type = if is_reply { "回复" } else { "评论" };
                let content_preview = truncate_chars(&content, 50);
                let title_part = title_preview_str(title);
                info!(
                    "广告 {} [{}]{} : {}",
                    log_type,
                    source_type.to_uppercase(),
                    title_part,
                    content_preview
                );
                target_lists
                    .entry("ads".to_string())
                    .or_default()
                    .push(identifier);
            }
        });
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

        for_each_comment_reply(comments, |data, is_reply| {
            let user_id = data.get("user_id").map(value_to_string).unwrap_or_default();
            let content = data
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if user_id.is_empty() || content.is_empty() {
                return;
            }
            let identifier = build_identifier(source_type, item_id, data, is_reply);
            content_map
                .entry((user_id, content))
                .or_default()
                .push(identifier);
        });

        for ((user_id, content), identifiers) in content_map {
            if identifiers.len() >= threshold {
                info!(
                    "用户 {} 刷屏评论: {}... - 出现 {} 次",
                    user_id,
                    truncate_chars(&content, 50),
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
        factory.register("duplicates", Box::new(DuplicatesStrategy));
        factory
    }

    pub fn register(&mut self, name: &str, strategy: Box<dyn CommentProcessStrategy>) {
        self.strategies.insert(name.to_string(), strategy);
    }

    pub fn get(&self, name: &str) -> Option<&dyn CommentProcessStrategy> {
        self.strategies.get(name).map(|b| b.as_ref())
    }

    pub fn get_all_strategy_types(&self) -> Vec<String> {
        self.strategies.keys().cloned().collect()
    }
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
        if let Some(strategy) = self.factory.get(action_type)
            && let Some(comments) = config.get_comments(item_id)
        {
            strategy.process(comments, item_id, title, params, target_lists, source_type);
        }
    }

    pub fn register_strategy(
        &mut self,
        action_type: &str,
        strategy: Box<dyn CommentProcessStrategy>,
    ) {
        self.factory.register(action_type, strategy);
    }

    pub fn get_all_strategy_types(&self) -> Vec<String> {
        self.factory.get_all_strategy_types()
    }
}

// ==================== 批量组与管理器 ====================
#[derive(Debug, Clone)]
pub struct BatchGroup {
    pub group_type: String,
    pub group_key: String,
    pub record_ids: Vec<String>,
}

impl BatchGroup {
    pub fn new(group_type: &str, group_key: &str, record_ids: Vec<String>) -> Self {
        BatchGroup {
            group_type: group_type.to_string(),
            group_key: group_key.to_string(),
            record_ids,
        }
    }
}

#[derive(Default)]
pub struct BatchActionManager {
    batch_actions: HashMap<(String, String), String>,
    processed_records: HashSet<String>,
}

impl BatchActionManager {
    pub fn new() -> Self {
        BatchActionManager::default()
    }

    pub fn save_batch_action(&mut self, group_type: &str, group_key: &str, action: &str) {
        self.batch_actions.insert(
            (group_type.to_string(), group_key.to_string()),
            action.to_string(),
        );
    }

    pub fn get_batch_action(&self, group_type: &str, group_key: &str) -> Option<String> {
        self.batch_actions
            .get(&(group_type.to_string(), group_key.to_string()))
            .cloned()
    }

    pub fn mark_record_processed(&mut self, record_id: &str) {
        self.processed_records.insert(record_id.to_string());
    }

    pub fn is_record_processed(&self, record_id: &str) -> bool {
        self.processed_records.contains(record_id)
    }

    pub fn clear_processed_records(&mut self) {
        self.processed_records.clear();
    }
}

// ==================== 处理上下文（不可变记录 + 可变状态） ====================
#[derive(Debug, Clone)]
pub struct ReportRecord {
    pub record_id: String,
    pub report_type: String,
    pub item: Value,
    pub admin_id: i32,
    pub is_batch_mode: bool,
    pub is_reprocess_mode: bool,
    pub config: Option<SourceConfig>,
    pub user_id: Option<i64>,
}

#[derive(Debug, Default)]
pub struct ProcessingState {
    pub processed: bool,
    pub action: Option<String>,
    pub skip_reason: Option<String>,
    pub messages: Vec<String>,
}

pub struct ProcessingContext {
    pub record: ReportRecord,
    pub state: ProcessingState,
}

impl ProcessingContext {
    pub fn new(record_id: String, report_type: String, item: Value, admin_id: i32) -> Self {
        ProcessingContext {
            record: ReportRecord {
                record_id,
                report_type,
                item,
                admin_id,
                is_batch_mode: false,
                is_reprocess_mode: false,
                config: None,
                user_id: None,
            },
            state: ProcessingState::default(),
        }
    }
}

// ==================== 处理器接口 ====================
pub trait Processor: Send + Sync {
    fn process(
        &self,
        record: &ReportRecord,
        state: &mut ProcessingState,
    ) -> Result<(), ProcessorError>;
}

// ==================== 动作注册表（静态函数表） ====================
type ActionFn = fn(i32, i32, Resolution) -> Result<bool, Box<dyn std::error::Error>>;

pub struct ActionRegistry {
    handlers: HashMap<&'static str, ActionFn>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        let mut handlers: HashMap<&'static str, ActionFn> = HashMap::new();
        handlers.insert(
            "execute_process_comment_report",
            |report_id: i32,
             admin_id: i32,
             resolution: Resolution|
             -> Result<bool, Box<dyn std::error::Error>> {
                ReportHandler::new()
                    .execute_process_comment_report(report_id, admin_id, resolution)
                    .map_err(|e| e.into())
            },
        );
        handlers.insert(
            "execute_process_work_report",
            |report_id: i32,
             admin_id: i32,
             resolution: Resolution|
             -> Result<bool, Box<dyn std::error::Error>> {
                ReportHandler::new()
                    .execute_process_work_report(report_id, admin_id, resolution)
                    .map_err(|e| e.into())
            },
        );
        handlers.insert(
            "execute_process_post_report",
            |report_id: i32,
             admin_id: i32,
             resolution: Resolution|
             -> Result<bool, Box<dyn std::error::Error>> {
                ReportHandler::new()
                    .execute_process_post_report(report_id, admin_id, resolution)
                    .map_err(|e| e.into())
            },
        );
        handlers.insert(
            "execute_process_discussion_report",
            |report_id: i32,
             admin_id: i32,
             resolution: Resolution|
             -> Result<bool, Box<dyn std::error::Error>> {
                ReportHandler::new()
                    .execute_process_discussion_report(report_id, admin_id, resolution)
                    .map_err(|e| e.into())
            },
        );
        ActionRegistry { handlers }
    }

    pub fn apply(
        &self,
        method: &str,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> Result<bool, ProcessorError> {
        self.handlers
            .get(method)
            .ok_or_else(|| ProcessorError::Processing(format!("未知处理方法: {}", method)))?(
            report_id, admin_id, resolution,
        )
        .map_err(ProcessorError::External)
    }
}

static ACTION_REGISTRY: OnceLock<ActionRegistry> = OnceLock::new();
pub fn global_action_registry() -> &'static ActionRegistry {
    ACTION_REGISTRY.get_or_init(ActionRegistry::new)
}

// 应用动作的便捷函数
pub(crate) fn apply_action_by_method(
    method: &str,
    report_id: i32,
    admin_id: i32,
    resolution: &str,
) -> Result<bool, ProcessorError> {
    let resolution_enum = parse_resolution(resolution)?;
    global_action_registry().apply(method, report_id, admin_id, resolution_enum)
}

// ==================== 详情展示 trait 与注册 ====================
pub trait ReportDisplay: Send + Sync {
    fn display(&self, item: &Value, config: &SourceConfig);
}

struct WorkWorkDisplay;
impl ReportDisplay for WorkWorkDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) {
        info!("=== {} 详情 ===", config.name);
        macro_rules! print_if {
            ($label:expr, $field:expr, $transform:expr) => {
                if let Some(val) = item.get($field) {
                    info!("{}: {}", $label, $transform(val));
                }
            };
            ($label:expr, $field:expr) => {
                if let Some(val) = item.get($field).and_then(|v| v.as_str()) {
                    info!("{}: {}", $label, val);
                }
            };
        }
        print_if!("作者昵称", &config.user_nickname_field);
        print_if!("作者链接", &config.user_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/user/{}",
            value_to_string(v)
        ));
        print_if!("作品链接", &config.source_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/work/{}",
            value_to_string(v)
        ));
        if let Some(type_field) = &config.work_type_field {
            print_if!("作品类型", type_field);
        }
        print_if!("举报原因", &config.reason_field);
        print_if!("举报线索", &config.description_field);
        print_if!("举报时间", &config.created_at_field, |v: &Value| {
            timestamp_to_string(v)
        });
    }
}

struct ShopCommentDisplay;
impl ReportDisplay for ShopCommentDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) {
        info!("=== {} 详情 ===", config.name);
        macro_rules! print_if {
            ($label:expr, $field:expr, $transform:expr) => {
                if let Some(val) = item.get($field) {
                    info!("{}: {}", $label, $transform(val));
                }
            };
            ($label:expr, $field:expr) => {
                if let Some(val) = item.get($field).and_then(|v| v.as_str()) {
                    info!("{}: {}", $label, val);
                }
            };
        }
        print_if!("举报内容", &config.content_field, |v: &Value| {
            html_to_text(v.as_str().unwrap_or(""))
        });
        print_if!("被举报人昵称", &config.user_nickname_field);
        print_if!("被举报人链接", &config.user_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/user/{}",
            value_to_string(v)
        ));
        print_if!("工作室名称", &config.source_name_field);
        print_if!("工作室链接", &config.source_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/work_shop/{}",
            value_to_string(v)
        ));
        print_if!("举报原因", &config.reason_field);
        print_if!("举报时间", &config.created_at_field, |v: &Value| {
            timestamp_to_string(v)
        });
    }
}

struct ForumPostDisplay;
impl ReportDisplay for ForumPostDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) {
        info!("=== {} 详情 ===", config.name);
        macro_rules! print_if {
            ($label:expr, $field:expr, $transform:expr) => {
                if let Some(val) = item.get($field) {
                    info!("{}: {}", $label, $transform(val));
                }
            };
            ($label:expr, $field:expr) => {
                if let Some(val) = item.get($field).and_then(|v| v.as_str()) {
                    info!("{}: {}", $label, val);
                }
            };
        }
        print_if!("帖子作者", &config.user_nickname_field);
        print_if!("作者链接", &config.user_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/user/{}",
            value_to_string(v)
        ));
        if let Ok(post_id) = item
            .get(&config.source_id_field)
            .map(value_to_string)
            .unwrap_or_default()
            .parse::<i32>()
        {
            match ForumDataFetcher::new().fetch_single_post_details(post_id) {
                Ok(details) => {
                    if let Some(content) = details.get("content").and_then(|v| v.as_str()) {
                        info!("内容: {}", truncate_chars(&html_to_text(content), 200));
                    }
                }
                Err(e) => warn!("获取帖子详情失败: {}", e),
            }
        }
        if let Some(title_field) = &config.title_field {
            print_if!("标题", title_field);
        }
        print_if!("举报原因", &config.reason_field);
        print_if!("举报线索", &config.description_field);
        print_if!("举报时间", &config.created_at_field, |v: &Value| {
            timestamp_to_string(v)
        });
    }
}

struct ForumDiscussionDisplay;
impl ReportDisplay for ForumDiscussionDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) {
        info!("=== {} 详情 ===", config.name);
        macro_rules! print_if {
            ($label:expr, $field:expr, $transform:expr) => {
                if let Some(val) = item.get($field) {
                    info!("{}: {}", $label, $transform(val));
                }
            };
            ($label:expr, $field:expr) => {
                if let Some(val) = item.get($field).and_then(|v| v.as_str()) {
                    info!("{}: {}", $label, val);
                }
            };
        }
        print_if!("被举报内容", &config.content_field, |v: &Value| {
            html_to_text(v.as_str().unwrap_or(""))
        });
        print_if!("被举报人昵称", &config.user_nickname_field);
        print_if!("被举报人链接", &config.user_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/user/{}",
            value_to_string(v)
        ));
        print_if!("帖子链接", &config.source_id_field, |v: &Value| format!(
            "https://shequ.codemao.cn/community/{}",
            value_to_string(v)
        ));
        if let Some(title_field) = &config.title_field {
            print_if!("帖子标题", title_field);
        }
        if let Some(board_field) = &config.board_name_field {
            print_if!("分区", board_field);
        }
        print_if!("举报原因", &config.reason_field);
        print_if!("举报时间", &config.created_at_field, |v: &Value| {
            timestamp_to_string(v)
        });
    }
}

// 详情展示注册表
static DISPLAY_REGISTRY: OnceLock<HashMap<&'static str, Box<dyn ReportDisplay>>> = OnceLock::new();
fn get_display_registry() -> &'static HashMap<&'static str, Box<dyn ReportDisplay>> {
    DISPLAY_REGISTRY.get_or_init(|| {
        let mut m: HashMap<&'static str, Box<dyn ReportDisplay>> = HashMap::new();
        m.insert("work_work", Box::new(WorkWorkDisplay));
        m.insert("shop_comment", Box::new(ShopCommentDisplay));
        m.insert("forum_post", Box::new(ForumPostDisplay));
        m.insert("forum_discussion", Box::new(ForumDiscussionDisplay));
        m
    })
}

// ==================== 详情显示处理器 ====================
pub struct DetailDisplayProcessor;

impl Processor for DetailDisplayProcessor {
    fn process(
        &self,
        record: &ReportRecord,
        _state: &mut ProcessingState,
    ) -> Result<(), ProcessorError> {
        if let Some(config) = &record.config {
            if let Some(display) = get_display_registry().get(record.report_type.as_str()) {
                display.display(&record.item, config);
            } else {
                // 回退通用展示
                info!("=== {} 详情 ===", config.name);
                macro_rules! print_if {
                    ($label:expr, $field:expr, $transform:expr) => {
                        if let Some(val) = record.item.get($field) {
                            info!("{}: {}", $label, $transform(val));
                        }
                    };
                    ($label:expr, $field:expr) => {
                        if let Some(val) = record.item.get($field).and_then(|v| v.as_str()) {
                            info!("{}: {}", $label, val);
                        }
                    };
                }
                print_if!("内容", &config.content_field);
                print_if!("举报原因", &config.reason_field);
                print_if!("举报描述", &config.description_field);
                print_if!("用户昵称", &config.user_nickname_field);
                print_if!("举报时间", &config.created_at_field, |v: &Value| {
                    timestamp_to_string(v)
                });
            }
        }
        Ok(())
    }
}

// ==================== 官方账号检查处理器 ====================
pub struct OfficialCheckProcessor {
    config: CheckConfig,
}

impl OfficialCheckProcessor {
    pub fn new(config: CheckConfig) -> Self {
        OfficialCheckProcessor { config }
    }
}

impl Processor for OfficialCheckProcessor {
    fn process(
        &self,
        record: &ReportRecord,
        state: &mut ProcessingState,
    ) -> Result<(), ProcessorError> {
        let config = match &record.config {
            Some(c) => c,
            None => return Ok(()),
        };

        let user_id = record
            .item
            .get(&config.user_id_field)
            .and_then(value_to_i64);

        if let Some(uid) = user_id
            && self.config.official_ids.contains(&uid)
        {
            state.messages.push("官方内容，自动通过".into());
            state.action = Some("P".into());
            state.processed = true;

            let status_map = record.config.as_ref().map(|_| record).map(|_| {
                // 使用全局静态状态映射
                let map: HashMap<&str, &str> = HashMap::from([
                    ("D", "DELETE"),
                    ("S", "MUTE_SEVEN_DAYS"),
                    ("T", "MUTE_THREE_MONTHS"),
                    ("P", "PASS"),
                ]);
                map
            });

            if let Some(ref status_map) = status_map
                && let Some(resolution) = status_map.get("P")
            {
                let report_id = config.get_report_id(&record.item)?;
                apply_action_by_method(
                    &config.handle_method,
                    report_id,
                    record.admin_id,
                    resolution,
                )?;
                state.messages.push("已自动通过官方内容".into());
                info!("自动通过官方举报ID: {}", record.record_id);
            }
        }

        Ok(())
    }
}

// ==================== 违规信息枚举 ====================
#[derive(Debug)]
enum ViolationKind {
    Ad {
        identifier: String,
    },
    Duplicate {
        user_content: (String, String),
        sample_identifier: String,
    },
}

// ==================== 违规检查器 ====================
pub struct ViolationChecker {
    pub comment_processor: CommentProcessor,
    config: CheckConfig,
    ad_keywords_cache: Arc<HashSet<String>>,
}

impl ViolationChecker {
    pub fn new(config: CheckConfig) -> Self {
        let ad_keywords_cache = Arc::new(
            config
                .ad_keywords
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
        );
        ViolationChecker {
            comment_processor: CommentProcessor::new(),
            config,
            ad_keywords_cache,
        }
    }

    /// 处理单条评论，返回违规信息（仅在违规时构建标识符）
    fn process_single_comment(
        item: &JsonObject,
        source_type: &str,
        source_id: i64,
        parent_comment_id: i64,
        is_reply: bool,
        ad_keywords: &HashSet<String>,
    ) -> Option<ViolationKind> {
        let content = item
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        let user_id_str = item
            .get("user_id")
            .and_then(|v| v.as_i64())
            .map(|id| id.to_string())
            .unwrap_or_default();
        let item_id = item.get("id").and_then(|v| v.as_i64()).unwrap_or(0);

        if !content.is_empty() && ad_keywords.iter().any(|kw| content.contains(kw)) {
            let identifier = format!(
                "{}:{}:{}:{}:{}",
                source_type,
                source_id,
                if is_reply { "reply" } else { "comment" },
                parent_comment_id,
                item_id
            );
            return Some(ViolationKind::Ad { identifier });
        }

        if !user_id_str.is_empty() && !content.is_empty() {
            let sample_identifier = format!(
                "{}:{}:{}:{}:{}",
                source_type,
                source_id,
                if is_reply { "reply" } else { "comment" },
                parent_comment_id,
                item_id
            );
            return Some(ViolationKind::Duplicate {
                user_content: (user_id_str, content),
                sample_identifier,
            });
        }

        None
    }

    pub fn check_violation(
        &self,
        source_id: i64,
        source_type: &str,
        board_name: &str,
        user_id: Option<i64>,
        title: &str,
        _config: &SourceConfig,
    ) -> Result<(), ProcessorError> {
        info!(
            "检查违规: source_id={}, type={}, board={}, user={:?}",
            source_id, source_type, board_name, user_id
        );

        let comment_source: CommentSource = source_type
            .parse()
            .map_err(|_| ProcessorError::Processing(format!("未知来源类型: {}", source_type)))?;

        let total = DataQuery::new()
            .count_comments(comment_source, source_id as i32)
            .unwrap_or(0);
        info!("该内容共有 {} 条评论", total);

        let limit_str = prompt_input("输入要获取的评论数: ");
        let limit: usize = limit_str
            .parse()
            .unwrap_or(self.config.comment_fetch_default_limit);

        let comment_stream = DataQuery::new()
            .stream_detailed_comments(comment_source, source_id as i32, Some(limit))
            .map_err(|e| ProcessorError::Processing(format!("获取评论流失败: {}", e)))?;

        let mut detailed_comments = Vec::new();
        for comment_result in comment_stream {
            match comment_result {
                Ok(comment) => detailed_comments.push(comment),
                Err(e) => {
                    error!("获取评论失败: {}，跳过", e);
                }
            }
        }

        let mut pending_violations: Vec<ViolationKind> = Vec::new();

        for comment in &detailed_comments {
            if comment
                .get("is_top")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }

            if let Some(v) = Self::process_single_comment(
                comment,
                source_type,
                source_id,
                0,
                false,
                &self.ad_keywords_cache,
            ) {
                pending_violations.push(v);
            }

            if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
                let parent_comment_id = comment.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                for reply_value in replies {
                    if let Some(reply) = reply_value.as_object()
                        && let Some(v) = Self::process_single_comment(
                            reply,
                            source_type,
                            source_id,
                            parent_comment_id,
                            true,
                            &self.ad_keywords_cache,
                        )
                    {
                        pending_violations.push(v);
                    }
                }
            }
        }

        // 分离广告违规与刷屏候选
        let mut ads_identifiers: Vec<String> = Vec::new();
        let mut duplicate_counts: HashMap<(String, String), (usize, Vec<String>)> = HashMap::new();

        for v in pending_violations {
            match v {
                ViolationKind::Ad { identifier } => {
                    ads_identifiers.push(identifier);
                }
                ViolationKind::Duplicate {
                    user_content,
                    sample_identifier,
                } => {
                    let entry = duplicate_counts
                        .entry(user_content)
                        .or_insert((0, Vec::new()));
                    entry.0 += 1;
                    entry.1.push(sample_identifier);
                }
            }
        }

        let mut duplicates_identifiers = Vec::new();
        for (count, identifiers) in duplicate_counts.values() {
            if *count >= self.config.spam_threshold {
                duplicates_identifiers.extend(identifiers.iter().cloned());
            }
        }

        let mut violations: Vec<String> = Vec::new();
        violations.extend(ads_identifiers);
        violations.extend(duplicates_identifiers);

        if source_type == "forum"
            && let Some(uid) = user_id
        {
            let spam_violations = self.check_spam_posts(uid, title)?;
            violations.extend(spam_violations);
        }

        let violations: HashSet<String> = violations.into_iter().collect();
        if violations.is_empty() {
            info!("未检测到违规内容");
            return Ok(());
        }

        info!("检测到 {} 条违规内容", violations.len());
        self.process_auto_report(violations)
    }

    fn check_spam_posts(&self, user_id: i64, title: &str) -> Result<Vec<String>, ProcessorError> {
        let fetcher = ForumDataFetcher::new();
        let mut posts = Vec::new();
        for result in fetcher.search_posts_gen(title, None) {
            match result {
                Ok(post) => posts.push(post),
                Err(e) => {
                    error!("搜索帖子失败: {}", e);
                    break;
                }
            }
        }
        let user_posts: Vec<&Value> = posts
            .iter()
            .filter(|p| {
                p.get("user")
                    .and_then(|u| u.get("id"))
                    .and_then(|v| v.as_i64())
                    == Some(user_id)
            })
            .collect();
        let threshold = self.config.spam_threshold;
        if user_posts.len() >= threshold {
            warn!(
                "警告: 用户 {} 已连续发布标题为【{}】的帖子 {} 次 (疑似刷屏)",
                user_id,
                title,
                user_posts.len()
            );
            let mut violations = Vec::new();
            for post in user_posts {
                if let Some(post_id) = post.get("id").and_then(|v| v.as_i64()) {
                    violations.push(format!("forum:{}:post:0:{}", post_id, post_id));
                }
            }
            return Ok(violations);
        }
        Ok(Vec::new())
    }

    fn process_auto_report(&self, violations: HashSet<String>) -> Result<(), ProcessorError> {
        let mut multi_account = MultiAccount::new();
        let password_path = PathConfig::global().password_file_path();
        if password_path.exists() {
            multi_account.load_from_file(&password_path)?;
        } else {
            info!("未找到学生账号文件，跳过自动举报");
            return Ok(());
        }
        if multi_account.accounts.is_empty() {
            info!("未加载学生账号, 无法进行自动举报");
            return Ok(());
        }
        let choice = get_valid_input(
            "是否自动举报违规评论? (Y/N)",
            &["Y".into(), "N".into()].into_iter().collect(),
        );
        if choice != "Y" {
            info!("自动举报操作已取消");
            return Ok(());
        }
        let reason_content = "违规内容";
        let mut accounts = multi_account.accounts.clone();
        if accounts.is_empty() {
            info!("没有可用账号");
            return Ok(());
        }
        let mut success = 0;
        let mut account_usage: HashMap<usize, usize> = HashMap::new();
        let violations_vec: Vec<_> = violations.into_iter().collect();
        let mut current_idx = 0usize;
        for (idx, violation) in violations_vec.iter().enumerate() {
            let chosen_idx = loop {
                if accounts.is_empty() {
                    info!("所有账号已失效或达到上限，停止举报");
                    break None;
                }
                current_idx %= accounts.len();
                let usage = account_usage.get(&current_idx).copied().unwrap_or(0);
                if usage < self.config.max_reports_per_account {
                    break Some(current_idx);
                }
                current_idx = (current_idx + 1) % accounts.len();
                if current_idx == 0
                    && accounts.iter().enumerate().all(|(i, _)| {
                        account_usage.get(&i).copied().unwrap_or(0)
                            >= self.config.max_reports_per_account
                    })
                {
                    break None;
                }
            };
            let chosen_idx = match chosen_idx {
                Some(i) => i,
                None => {
                    info!("所有账号均已达到举报上限，停止");
                    break;
                }
            };
            let (user, pass) = &accounts[chosen_idx];
            let usage = account_usage.get(&chosen_idx).copied().unwrap_or(0);
            if usage == 0 {
                match self.login_student(user, pass) {
                    Ok(()) => {}
                    Err(e) => {
                        warn!("账号 {} 登录失败: {}，移除", user, e);
                        accounts.remove(chosen_idx);
                        account_usage.remove(&chosen_idx);
                        if chosen_idx < current_idx && current_idx > 0 {
                            current_idx -= 1;
                        }
                        current_idx %= accounts.len().max(1);
                        continue;
                    }
                }
            }
            match self.execute_single_report(violation, reason_content) {
                Ok(_) => {
                    success += 1;
                    let entry = account_usage.entry(chosen_idx).or_insert(0);
                    *entry += 1;
                    info!(
                        "[{}/{}] 举报成功: {}",
                        idx + 1,
                        violations_vec.len(),
                        violation
                    );
                }
                Err(e) => {
                    error!(
                        "[{}/{}] 举报失败: {} - {}",
                        idx + 1,
                        violations_vec.len(),
                        violation,
                        e
                    );
                }
            }
            current_idx = (chosen_idx + 1) % accounts.len();
        }
        if let Err(e) = KittyFactory::global_client().switch_identity(Catsona::Judge) {
            warn!("切换回管理员身份失败: {}", e);
        }
        info!("自动举报完成，成功 {}/{}", success, violations_vec.len());
        Ok(())
    }

    fn login_student(&self, username: &str, password: &str) -> Result<(), ProcessorError> {
        crate::api::auth::LoginBuilder::new()
            .identity(username)
            .password(password)
            .status(crate::api::auth::AccountStatus::Edu)
            .execute()
            .map_err(|e| ProcessorError::External(e.into()))?;
        Ok(())
    }

    fn parse_violation(&self, violation: &str) -> Option<(String, i64, String, i32, i32)> {
        let parts: Vec<&str> = violation.split(':').collect();
        if parts.len() != 5 {
            return None;
        }
        let source = parts[0].to_string();
        let source_id: i64 = parts[1].parse().ok()?;
        let violation_type = parts[2].to_string();
        let parent_id: i32 = parts[3].parse().ok()?;
        let content_id: i32 = parts[4].parse().ok()?;
        Some((source, source_id, violation_type, parent_id, content_id))
    }

    fn execute_single_report(
        &self,
        violation: &str,
        reason_content: &str,
    ) -> Result<(), ProcessorError> {
        let parsed = self
            .parse_violation(violation)
            .ok_or_else(|| ProcessorError::Processing("违规标识符格式错误".into()))?;
        let (source, source_id, violation_type, parent_id, content_id) = parsed;
        match violation_type.as_str() {
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
                let is_reply = violation_type == "reply";
                match source.as_str() {
                    "work" => {
                        CommentOperations::new()
                            .execute_report_comment(source_id as i32, content_id, reason_content)
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
                        let reporter_id = fastrand::i32(10000..=199999999);
                        if is_reply {
                            WorkshopActionHandler::new()
                                .execute_report_comment(
                                    content_id,
                                    reason_content,
                                    WorkShopReportReasonId::Reason7,
                                    reporter_id,
                                    None,
                                    Some(parent_id),
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
                    violation_type
                )));
            }
        }
        Ok(())
    }
}

// ==================== 动作选择处理器 ====================
pub struct ActionSelectionProcessor {
    pub registry: Arc<ReportTypeRegistry>,
    pub batch_manager: Arc<Mutex<BatchActionManager>>,
    violation_checker: ViolationChecker,
}

impl ActionSelectionProcessor {
    pub fn new(
        registry: Arc<ReportTypeRegistry>,
        batch_manager: Arc<Mutex<BatchActionManager>>,
        check_config: CheckConfig,
    ) -> Self {
        ActionSelectionProcessor {
            registry,
            batch_manager,
            violation_checker: ViolationChecker::new(check_config),
        }
    }

    fn check_violation(&self, record: &ReportRecord) -> Result<(), ProcessorError> {
        info!("=== 开始检查违规 ===");
        let config = match &record.config {
            Some(c) => c.clone(),
            None => return Ok(()),
        };
        let source_id = record
            .item
            .get(&config.source_id_field)
            .and_then(value_to_i64)
            .unwrap_or(0);
        let board_name = config
            .board_name_field
            .as_ref()
            .and_then(|field| record.item.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let source_type = get_source_type_map()
            .get(record.report_type.as_str())
            .copied()
            .unwrap_or("work");
        let user_id = record
            .item
            .get(&config.user_id_field)
            .and_then(value_to_i64);
        let title = config
            .title_field
            .as_ref()
            .and_then(|field| record.item.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        self.violation_checker.check_violation(
            source_id,
            source_type,
            board_name,
            user_id,
            title,
            &config,
        )?;
        info!("=== 检查结束 ===");
        Ok(())
    }
}

impl Processor for ActionSelectionProcessor {
    fn process(
        &self,
        record: &ReportRecord,
        state: &mut ProcessingState,
    ) -> Result<(), ProcessorError> {
        if record.is_batch_mode {
            let config = record
                .config
                .as_ref()
                .ok_or_else(|| ProcessorError::Processing("批量模式缺少配置".into()))?;
            let group_type = &record.report_type;
            let group_key = &record.record_id;
            let batch_action = self
                .batch_manager
                .lock()
                .expect("批量管理器 Mutex 被污染")
                .get_batch_action(group_type, group_key);

            if let Some(action) = batch_action {
                state.action = Some(action.clone());
                let status_map = self.registry.get_status_mapping();
                if let Some(resolution) = status_map.get(action.as_str()) {
                    let report_id = config.get_report_id(&record.item)?;
                    apply_action_by_method(
                        &config.handle_method,
                        report_id,
                        record.admin_id,
                        resolution,
                    )?;
                    info!("批量应用操作: {} -> {}", action, resolution);
                }
                state.processed = true;
            } else {
                state.skip_reason = Some("批量模式未找到预设动作".into());
                state.processed = true;
            }
            return Ok(());
        }

        let actions = self.registry.get_available_actions(&record.report_type);
        let valid_keys: HashSet<String> = actions.iter().map(|a| a.key.clone()).collect();
        let prompt = self.registry.get_action_prompt(&record.report_type);

        loop {
            let choice = get_valid_input(&prompt, &valid_keys);
            match choice.as_str() {
                "D" | "S" | "T" | "P" | "U" => {
                    state.action = Some(choice.clone());
                    if let Some(config) = &record.config {
                        let status_map = self.registry.get_status_mapping();
                        if let Some(resolution) = status_map.get(choice.as_str()) {
                            let report_id = config.get_report_id(&record.item)?;
                            apply_action_by_method(
                                &config.handle_method,
                                report_id,
                                record.admin_id,
                                resolution,
                            )?;
                            info!("已应用操作: {} -> {}", choice, resolution);
                        }
                    }
                    state.processed = true;
                    break;
                }
                "F" => {
                    if let Some(config) = &record.config
                        && let Some(ref special_check) = config.special_check
                        && special_check(&record.item)
                    {
                        self.check_violation(record)?;
                        info!("违规检查完成, 请选择处理动作");
                        continue;
                    }
                    info!("该类型不支持检查违规操作");
                    continue;
                }
                "J" => {
                    state.skip_reason = Some("用户选择跳过".into());
                    state.processed = true;
                    info!("已跳过该举报");
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
            if context.state.processed || context.state.skip_reason.is_some() {
                break;
            }
            processor.process(&context.record, &mut context.state)?;
        }
        Ok(())
    }

    pub fn create_default(
        registry: Arc<ReportTypeRegistry>,
        batch_manager: Arc<Mutex<BatchActionManager>>,
        check_config: CheckConfig,
    ) -> Self {
        ProcessingPipeline::new(vec![
            Box::new(OfficialCheckProcessor::new(check_config.clone())),
            Box::new(DetailDisplayProcessor),
            Box::new(ActionSelectionProcessor::new(
                registry,
                batch_manager,
                check_config,
            )),
        ])
    }
}

// ==================== 多账号管理器 ====================
pub struct MultiAccount {
    pub accounts: Vec<(String, String)>,
}

impl MultiAccount {
    pub fn new() -> Self {
        MultiAccount {
            accounts: Vec::new(),
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
        info!("加载 {} 个账号", self.accounts.len());
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

// ==================== 全局单例 ====================
static COMMENT_PROCESSOR: OnceLock<CommentProcessor> = OnceLock::new();
static VIOLATION_CHECKER: OnceLock<ViolationChecker> = OnceLock::new();

pub fn comment_processor() -> &'static CommentProcessor {
    COMMENT_PROCESSOR.get_or_init(CommentProcessor::new)
}

pub fn violation_checker() -> &'static ViolationChecker {
    VIOLATION_CHECKER.get_or_init(|| ViolationChecker::new(CheckConfig::default()))
}

// ==================== 为 Vec<Value> 实现 CommentConfig ====================
impl CommentConfig for Vec<Value> {
    fn get_comments(&self, _item_id: i64) -> Option<&[Value]> {
        Some(self.as_slice())
    }
}
