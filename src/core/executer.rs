use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::RngExt;
use serde_json::{Map, Value};

use crate::api::forum::{
    ForumActionHandler, ForumDataFetcher, ForumReportReasonId, ItemType, PostReportReasonId,
};
use crate::api::shop::{WorkShopReportReasonId, WorkshopActionHandler, WorkshopDataFetcher};
use crate::api::whale::{ReportHandler, ReportStatus, Resolution};
use crate::api::work::{BaseWorkOperations, CommentOperations, WorkDataFetcher};
use crate::core::types::CommentConfig;
use crate::utils::acquire::{BaseKey, Catsona, FileUploader, HttpMethod, KittyFactory};
use crate::utils::data::{DataManager, PathConfig, SettingManager};

use super::types::{
    ProcessorError, ReportFetcher, ReportTypeRegistry, SourceConfig, bytes_to_human,
    get_valid_input, html_to_text, prompt_input, timestamp_to_string, value_to_i64,
    value_to_string,
};

// ==================== 公共工具函数 ====================

/// 生成标题预览字符串，最多显示前10个字符。
fn title_preview_str(title: &str) -> String {
    if title.is_empty() {
        String::new()
    } else {
        format!("[{}]", &title[..title.len().min(10)])
    }
}

/// 按字符截断字符串，确保不截断多字节字符。
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect::<String>() + "..."
    }
}

/// 生成统一的违规标识符: source_type:item_id:type:parent_id:content_id
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

/// 对每条非置顶评论及其所有回复应用闭包处理。
fn for_each_comment_reply(
    comments: &[Value],
    mut handler: impl FnMut(&Value, bool), // is_reply: bool
) {
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

/// 将字符串解析为 Resolution 枚举，错误映射为 ProcessorError。
fn parse_resolution(resolution: &str) -> Result<Resolution, ProcessorError> {
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

/// 官方账号 ID 列表，编译期常量。
const OFFICIAL_IDS: [i64; 9] = [
    128963, 629055, 203577, 859722, 148883, 2191000, 7492052, 387963, 3649031,
];

/// 提取 SourceConfig 的 report_id 辅助 trait。
trait ReportIdExt {
    fn get_report_id(&self, item: &Value) -> i32;
}

impl ReportIdExt for SourceConfig {
    fn get_report_id(&self, item: &Value) -> i32 {
        item.get(&self.report_id_field)
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
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
                println!(
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

        for_each_comment_reply(comments, |data, is_reply| {
            let user_id = data
                .get("user_id")
                .map(|v| value_to_string(v))
                .unwrap_or_default();
            if blacklist.contains(&user_id) {
                let identifier = build_identifier(source_type, item_id, data, is_reply);
                let log_type = if is_reply { "回复" } else { "评论" };
                let nickname = data
                    .get("nickname")
                    .and_then(|v| v.as_str())
                    .unwrap_or("未知用户");
                let title_part = title_preview_str(title);
                println!(
                    "黑名单 {} [{}]{} : {}",
                    log_type,
                    source_type.to_uppercase(),
                    title_part,
                    nickname
                );
                target_lists
                    .entry("blacklist".to_string())
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
            let identifier = build_identifier(source_type, item_id, data, is_reply);
            content_map
                .entry((user_id, content))
                .or_default()
                .push(identifier);
        });

        for ((user_id, content), identifiers) in content_map {
            if identifiers.len() >= threshold {
                println!(
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
        if let Some(strategy) = self.factory.get(action_type) {
            if let Some(comments) = config.get_comments(item_id) {
                strategy.process(&comments, item_id, title, params, target_lists, source_type);
            }
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
        BatchActionManager {
            batch_actions: HashMap::new(),
            processed_records: HashSet::new(),
        }
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

// ==================== 处理上下文 ====================
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
    pub is_batch_mode: bool,
    pub is_reprocess_mode: bool,
    pub config: Option<SourceConfig>,
    pub user_id: Option<i64>,
}

impl ProcessingContext {
    pub fn new(record_id: String, report_type: String, item: Value, admin_id: i32) -> Self {
        ProcessingContext {
            record_id,
            report_type,
            item,
            admin_id,
            processed: false,
            action: None,
            skip_reason: None,
            messages: Vec::new(),
            is_batch_mode: false,
            is_reprocess_mode: false,
            config: None,
            user_id: None,
        }
    }
}

// ==================== 处理器接口 ====================
pub trait Processor: Send + Sync {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError>;
}

// ==================== 官方账号检查处理器 ====================
pub struct OfficialCheckProcessor;

impl Processor for OfficialCheckProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        let config = match &context.config {
            Some(c) => c,
            None => return Ok(()),
        };

        let user_id = context
            .item
            .get(&config.user_id_field)
            .and_then(|v| value_to_i64(v));

        if let Some(uid) = user_id {
            context.user_id = Some(uid);
            if OFFICIAL_IDS.contains(&uid) {
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
                    let report_id = config.get_report_id(&context.item);
                    let _ = apply_action_by_method(
                        &config.handle_method,
                        report_id,
                        context.admin_id,
                        resolution,
                    );
                    context.messages.push("已自动通过官方内容".into());
                    println!("自动通过官方举报ID: {}", context.record_id);
                }
            }
        }

        Ok(())
    }
}

// ==================== 详情显示处理器（按类型定制） ====================
pub struct DetailDisplayProcessor;

impl DetailDisplayProcessor {
    fn display_work_report(item: &Value, config: &SourceConfig) {
        println!("=== 作品举报详情 ===");
        let base_url = "https://shequ.codemao.cn";

        let author_nickname = item
            .get(&config.user_nickname_field)
            .and_then(|v| v.as_str())
            .unwrap_or("未知");
        let author_id = item
            .get(&config.user_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("作者昵称: {}", author_nickname);
        println!("作者链接: {}/user/{}", base_url, author_id);

        let work_id = item
            .get(&config.source_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("作品链接: {}/work/{}", base_url, work_id);

        if let Some(type_field) = &config.work_type_field {
            if let Some(work_type) = item.get(type_field).and_then(|v| v.as_str()) {
                println!("作品类型: {}", work_type);
            }
        }

        let reason = item
            .get(&config.reason_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("举报原因: {}", reason);

        let description = item
            .get(&config.description_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("举报线索: {}", description);

        let created_at = item
            .get(&config.created_at_field)
            .map(|v| timestamp_to_string(v))
            .unwrap_or_default();
        println!("举报时间: {}", created_at);
    }

    fn display_comment_report(item: &Value, config: &SourceConfig) {
        println!("=== 评论举报详情 ===");
        let base_url = "https://shequ.codemao.cn";

        let content = item
            .get(&config.content_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content_text = html_to_text(content);
        println!("举报内容: {}", content_text);

        let user_nickname = item
            .get(&config.user_nickname_field)
            .and_then(|v| v.as_str())
            .unwrap_or("未知");
        let user_id = item
            .get(&config.user_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("被举报人昵称: {}", user_nickname);
        println!("被举报人链接: {}/user/{}", base_url, user_id);

        let studio_name = item
            .get(&config.source_name_field)
            .and_then(|v| v.as_str())
            .unwrap_or("未知");
        let studio_id = item
            .get(&config.source_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("工作室名称: {}", studio_name);
        println!("工作室链接: {}/work_shop/{}", base_url, studio_id);

        let reason = item
            .get(&config.reason_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("举报原因: {}", reason);

        let created_at = item
            .get(&config.created_at_field)
            .map(|v| timestamp_to_string(v))
            .unwrap_or_default();
        println!("举报时间: {}", created_at);
    }

    fn display_forum_report(item: &Value, config: &SourceConfig) {
        println!("=== 帖子举报详情 ===");
        let base_url = "https://shequ.codemao.cn";

        let author_nickname = item
            .get(&config.user_nickname_field)
            .and_then(|v| v.as_str())
            .unwrap_or("未知");
        let author_id = item
            .get(&config.user_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("帖子作者: {}", author_nickname);
        println!("作者链接: {}/user/{}", base_url, author_id);

        let post_id_value = item
            .get(&config.source_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("帖子链接: {}/community/{}", base_url, post_id_value);

        if let Ok(post_id) = post_id_value.parse::<i32>() {
            if let Ok(details) = ForumDataFetcher::new().fetch_single_post_details(post_id) {
                if let Some(content) = details.get("content").and_then(|v| v.as_str()) {
                    let content_text = html_to_text(content);
                    println!("内容: {}", truncate_chars(&content_text, 200));
                }
            }
        }

        if let Some(title_field) = &config.title_field {
            if let Some(title) = item.get(title_field).and_then(|v| v.as_str()) {
                println!("标题: {}", title);
            }
        }

        let reason = item
            .get(&config.reason_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("举报原因: {}", reason);

        let description = item
            .get(&config.description_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("举报线索: {}", description);

        let created_at = item
            .get(&config.created_at_field)
            .map(|v| timestamp_to_string(v))
            .unwrap_or_default();
        println!("举报时间: {}", created_at);
    }

    fn display_discussion_report(item: &Value, config: &SourceConfig) {
        println!("=== 讨论举报详情 ===");
        let base_url = "https://shequ.codemao.cn";

        let content = item
            .get(&config.content_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content_text = html_to_text(content);
        println!("被举报内容: {}", content_text);

        let user_nickname = item
            .get(&config.user_nickname_field)
            .and_then(|v| v.as_str())
            .unwrap_or("未知");
        let user_id = item
            .get(&config.user_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("被举报人昵称: {}", user_nickname);
        println!("被举报人链接: {}/user/{}", base_url, user_id);

        let post_id = item
            .get(&config.source_id_field)
            .map(|v| value_to_string(v))
            .unwrap_or_default();
        println!("帖子链接: {}/community/{}", base_url, post_id);

        if let Some(title_field) = &config.title_field {
            if let Some(title) = item.get(title_field).and_then(|v| v.as_str()) {
                println!("帖子标题: {}", title);
            }
        }

        if let Some(board_field) = &config.board_name_field {
            if let Some(board) = item.get(board_field).and_then(|v| v.as_str()) {
                println!("分区: {}", board);
            }
        }

        let reason = item
            .get(&config.reason_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        println!("举报原因: {}", reason);

        let created_at = item
            .get(&config.created_at_field)
            .map(|v| timestamp_to_string(v))
            .unwrap_or_default();
        println!("举报时间: {}", created_at);
    }

    fn display_generic_report(item: &Value, config: &SourceConfig) {
        println!("=== {} 详情 ===", config.name);

        if let Some(content) = item.get(&config.content_field).and_then(|v| v.as_str()) {
            println!("内容: {}", content);
        }

        if let Some(reason) = item.get(&config.reason_field).and_then(|v| v.as_str()) {
            println!("举报原因: {}", reason);
        }

        if let Some(desc) = item.get(&config.description_field).and_then(|v| v.as_str()) {
            println!("举报描述: {}", desc);
        }

        if let Some(user_nickname) = item
            .get(&config.user_nickname_field)
            .and_then(|v| v.as_str())
        {
            println!("用户昵称: {}", user_nickname);
        }

        let created_at = item
            .get(&config.created_at_field)
            .map(|v| timestamp_to_string(v))
            .unwrap_or_default();
        println!("举报时间: {}", created_at);
    }
}

impl Processor for DetailDisplayProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        let config = match &context.config {
            Some(c) => c,
            None => return Ok(()),
        };

        let item = &context.item;

        match context.report_type.as_str() {
            "work_work" => Self::display_work_report(item, config),
            "shop_comment" => Self::display_comment_report(item, config),
            "forum_post" => Self::display_forum_report(item, config),
            "forum_discussion" => Self::display_discussion_report(item, config),
            _ => Self::display_generic_report(item, config),
        }

        Ok(())
    }
}

// ==================== 动作选择处理器 ====================
pub struct ActionSelectionProcessor {
    pub registry: Arc<ReportTypeRegistry>,
    pub batch_manager: Arc<Mutex<BatchActionManager>>,
}

impl ActionSelectionProcessor {
    fn check_violation(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        println!("=== 开始检查违规 ===");

        let config = match &context.config {
            Some(c) => c.clone(),
            None => return Ok(()),
        };

        let source_id = context
            .item
            .get(&config.source_id_field)
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let board_name = config
            .board_name_field
            .as_ref()
            .and_then(|field| context.item.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let source_type_map: HashMap<&str, &str> = HashMap::from([
            ("shop_comment", "shop"),
            ("forum_post", "forum"),
            ("forum_discussion", "forum"),
        ]);

        let source_type = source_type_map
            .get(context.report_type.as_str())
            .copied()
            .unwrap_or("work");

        let user_id = context
            .item
            .get(&config.user_id_field)
            .and_then(|v| value_to_i64(v));

        let checker = ViolationChecker::new();
        checker.check_violation(source_id, source_type, board_name, user_id)?;

        println!("=== 检查结束 ===");
        Ok(())
    }
}

impl Processor for ActionSelectionProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        let actions = self.registry.get_available_actions(&context.report_type);
        let valid_keys: HashSet<String> = actions.iter().map(|a| a.key.clone()).collect();
        let prompt = self.registry.get_action_prompt(&context.report_type);

        loop {
            if context.is_batch_mode {
                break;
            }

            let choice = get_valid_input(&prompt, &valid_keys);

            match choice.as_str() {
                "D" | "S" | "T" | "P" | "U" => {
                    context.action = Some(choice.clone());
                    if let Some(config) = &context.config {
                        let status_map = self.registry.get_status_mapping();
                        if let Some(resolution) = status_map.get(&choice) {
                            let report_id = config.get_report_id(&context.item);
                            apply_action_by_method(
                                &config.handle_method,
                                report_id,
                                context.admin_id,
                                resolution,
                            )?;
                            println!("已应用操作: {} -> {}", choice, resolution);
                        }
                    }
                    context.processed = true;
                    break;
                }
                "F" => {
                    if let Some(config) = &context.config {
                        if let Some(special_check) = config.special_check {
                            if special_check(&context.item) {
                                self.check_violation(context)?;
                                println!("违规检查完成, 请选择处理动作");
                                continue;
                            }
                        }
                    }
                    println!("该类型不支持检查违规操作");
                    continue;
                }
                "J" => {
                    context.skip_reason = Some("用户选择跳过".into());
                    context.processed = true;
                    println!("已跳过该举报");
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

    pub fn create_default(
        registry: Arc<ReportTypeRegistry>,
        batch_manager: Arc<Mutex<BatchActionManager>>,
    ) -> Self {
        ProcessingPipeline::new(vec![
            Box::new(OfficialCheckProcessor),
            Box::new(DetailDisplayProcessor),
            Box::new(ActionSelectionProcessor {
                registry,
                batch_manager,
            }),
        ])
    }
}

// ==================== 动作执行辅助函数 ====================
fn apply_action_by_method(
    method: &str,
    report_id: i32,
    admin_id: i32,
    resolution: &str,
) -> Result<bool, ProcessorError> {
    let resolution_enum = parse_resolution(resolution)?;

    match method {
        "execute_process_comment_report" => ReportHandler::new()
            .execute_process_comment_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        "execute_process_work_report" => ReportHandler::new()
            .execute_process_work_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        "execute_process_post_report" => ReportHandler::new()
            .execute_process_post_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        "execute_process_discussion_report" => ReportHandler::new()
            .execute_process_discussion_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        _ => Err(ProcessorError::Processing(format!(
            "未知处理方法: {}",
            method
        ))),
    }
}

// ==================== 违规检查器 ====================
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
        let setting = SettingManager::global()
            .data()
            .map_err(|e| ProcessorError::External(e.into()))?;
        let spam_max = setting.parameter.spam_del_max;

        let mut params: HashMap<String, Value> = HashMap::new();
        params.insert(
            "ads".into(),
            Value::Array(
                data.user_data
                    .ads
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        params.insert(
            "blacklist".into(),
            Value::Array(
                data.user_data
                    .black_room
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
        params.insert(
            "duplicates".into(),
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

        // 刷屏帖子检测
        if source_type == "forum" {
            if let Some(uid) = user_id {
                let spam_violations = self.check_spam_posts(uid, board_name)?;
                violations.extend(spam_violations);
            }
        }

        let violations: HashSet<String> = violations.into_iter().collect();

        if violations.is_empty() {
            println!("未检测到违规内容");
            return Ok(());
        }

        println!("检测到 {} 条违规内容", violations.len());
        self.process_auto_report(violations)
    }

    fn check_spam_posts(&self, user_id: i64, title: &str) -> Result<Vec<String>, ProcessorError> {
        let fetcher = ForumDataFetcher::new();
        let mut posts = Vec::new();

        for result in fetcher.search_posts_gen(title, None) {
            match result {
                Ok(post) => posts.push(post),
                Err(e) => {
                    eprintln!("搜索帖子失败: {}", e);
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

        let setting = SettingManager::global()
            .data()
            .map_err(|e| ProcessorError::External(e.into()))?;
        let threshold = setting.parameter.spam_del_max as usize;

        if user_posts.len() >= threshold {
            println!(
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

    fn get_comment_total(&self, source_id: i64, source_type: &str) -> Result<i64, ProcessorError> {
        match source_type {
            "work" => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "1")
                    .send()?;
                let json = KittyFactory::global_client().response_to_json(resp)?;
                Ok(json.get("total").and_then(|v| v.as_i64()).unwrap_or(0))
            }
            "shop" => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/discussions/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("source", "WORK_SHOP")
                    .with_param("limit", "1")
                    .send()?;
                let json = KittyFactory::global_client().response_to_json(resp)?;
                let total = json.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
                let total_reply = json.get("totalReply").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(total + total_reply)
            }
            "forum" => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/forums/posts/{}/details", source_id),
                        Some(BaseKey::Default),
                    )
                    .send()?;
                let json = KittyFactory::global_client().response_to_json(resp)?;
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
                        Err(e) => {
                            eprintln!("获取评论出错: {}", e);
                            break;
                        }
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
                        Err(e) => {
                            eprintln!("获取评论出错: {}", e);
                            break;
                        }
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
        let mut multi_account = MultiAccount::new(Catsona::Scholar);
        let password_path = PathConfig::password_file_path();
        if password_path.exists() {
            multi_account.load_from_file(&password_path)?;
        } else {
            println!("未找到学生账号文件，跳过自动举报");
            return Ok(());
        }

        if multi_account.accounts.is_empty() {
            println!("未加载学生账号, 无法进行自动举报");
            return Ok(());
        }

        let choice = get_valid_input(
            "是否自动举报违规评论? (Y/N)",
            &["Y".into(), "N".into()].into_iter().collect(),
        );
        if choice != "Y" {
            println!("自动举报操作已取消");
            return Ok(());
        }

        let reason_content = "违规内容";

        let mut accounts = multi_account.accounts.clone();
        if accounts.is_empty() {
            println!("没有可用账号");
            return Ok(());
        }

        let mut success = 0;
        let mut account_usage: HashMap<usize, usize> = HashMap::new();
        let mut account_index = 0;
        let violations_vec: Vec<String> = violations.into_iter().collect();

        for (idx, violation) in violations_vec.iter().enumerate() {
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

            match self.execute_single_report(violation, reason_content) {
                Ok(_) => {
                    success += 1;
                    let usage = account_usage.entry(account_index).or_insert(0);
                    *usage += 1;
                    println!(
                        "[{}/{}] 举报成功: {}",
                        idx + 1,
                        violations_vec.len(),
                        violation
                    );
                }
                Err(e) => {
                    println!(
                        "[{}/{}] 举报失败: {} - {}",
                        idx + 1,
                        violations_vec.len(),
                        violation,
                        e
                    );
                }
            }
        }

        KittyFactory::global_client()
            .switch_identity(Catsona::Judge)
            .ok();
        println!("自动举报完成，成功 {}/{}", success, violations_vec.len());
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
                    _ => {
                        return Err(ProcessorError::Processing("不支持的来源".into()));
                    }
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

// ==================== 自动回复处理器 ====================
pub struct ReplyProcessor;

impl ReplyProcessor {
    pub fn parse_content_field(reply: &Value) -> Option<Map<String, Value>> {
        match reply.get("content") {
            Some(Value::String(s)) => serde_json::from_str(s).ok(),
            Some(Value::Object(obj)) => Some(obj.clone()),
            _ => None,
        }
    }

    pub fn extract_comment_text(reply_type: &str, message_info: &Map<String, Value>) -> String {
        if reply_type.ends_with("_COMMENT") {
            message_info
                .get("comment")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            message_info
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        }
    }

    pub fn extract_target_and_parent_ids(
        reply_type: &str,
        reply: &Value,
        message_info: &Map<String, Value>,
        _business_id: i64,
        _source_type: &str,
    ) -> (i32, i32) {
        let is_comment = reply_type.ends_with("_COMMENT");
        let target_id = if is_comment {
            reply
                .get("reference_id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32
        } else {
            message_info
                .get("reply_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0)
        };

        let parent_id = if !is_comment {
            reply
                .get("reference_id")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32
        } else {
            0
        };

        (target_id, parent_id)
    }

    pub fn match_keyword(
        comment_text: &str,
        formatted_answers: &HashMap<String, Value>,
        formatted_replies: &[String],
    ) -> (String, Option<String>) {
        for (keyword, resp) in formatted_answers {
            if comment_text.contains(keyword) {
                let chosen = match resp {
                    Value::Array(arr) => {
                        if arr.is_empty() {
                            String::new()
                        } else {
                            let idx = rand::rng().random_range(0..arr.len());
                            arr.get(idx)
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        }
                    }
                    Value::String(s) => s.clone(),
                    _ => String::new(),
                };
                return (chosen, Some(keyword.clone()));
            }
        }

        let chosen = if formatted_replies.is_empty() {
            String::new()
        } else {
            let idx = rand::rng().random_range(0..formatted_replies.len());
            formatted_replies.get(idx).cloned().unwrap_or_default()
        };

        (chosen, None)
    }

    pub fn protect_cdn_link(link: &str) -> String {
        let mut protected = String::new();
        for ch in link.chars() {
            protected.push(ch);
            protected.push('\u{200b}');
            protected.push('\u{200d}');
        }
        protected
    }

    pub fn log_reply_info(
        reply_id: i64,
        reply_type: &str,
        source_type: &str,
        sender_nickname: &str,
        sender_id: i64,
        business_name: &str,
        comment_text: &str,
        matched_keyword: Option<&str>,
        chosen: &str,
    ) {
        println!("\n{}", "=".repeat(40));
        println!("处理新通知 [ID: {}]", reply_id);
        println!(
            "类型: {} ({})",
            reply_type,
            if source_type == "work" {
                "作品"
            } else {
                "帖子"
            }
        );
        println!("发送者: {} (ID: {})", sender_nickname, sender_id);
        println!("来源: {}", business_name);
        println!("内容: {}", comment_text);
        if let Some(kw) = matched_keyword {
            println!("匹配到关键词: 「{}」", kw);
        } else {
            println!("未匹配关键词, 使用随机回复");
        }
        println!("选择回复: 【{}】", chosen);
    }
}

// ==================== 多账号管理器 ====================
pub struct MultiAccount {
    pub accounts: Vec<(String, String)>,
    identity_type: Catsona,
}

impl MultiAccount {
    pub fn new(identity_type: Catsona) -> Self {
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
        println!("加载 {} 个账号", self.accounts.len());
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
const MAX_SIZE_BYTES: u64 = 15 * 1024 * 1024;

pub struct FileProcessor;

impl FileProcessor {
    pub fn handle_file_upload(
        file_path: &Path,
        save_path: &str,
        method: &str,
    ) -> Result<String, ProcessorError> {
        let metadata = fs::metadata(file_path)?;
        let file_size = metadata.len();

        if file_size > MAX_SIZE_BYTES {
            let size_mb = file_size as f64 / 1024.0 / 1024.0;
            println!(
                "警告: 文件 {} 大小 {:.2} MB 超过 15MB 限制, 跳过上传",
                file_path.display(),
                size_mb
            );
            return Err(ProcessorError::Processing(format!(
                "文件过大: {} ({} bytes)",
                file_path.display(),
                file_size
            )));
        }

        let client = KittyFactory::global_client().clone();
        let uploader = FileUploader::new(client);
        let url = uploader
            .upload(file_path, method, save_path)
            .map_err(|e| ProcessorError::External(e.into()))?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let size_human = bytes_to_human(file_size);
        println!(
            "上传成功: {} (文件: {}, 大小: {}, 时间戳: {})",
            url,
            file_path.display(),
            size_human,
            timestamp
        );

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
                match Self::handle_file_upload(path.as_path(), save_path, method) {
                    Ok(url) => {
                        results.insert(path.to_path_buf(), url);
                    }
                    Err(e) => {
                        eprintln!("上传失败 {}: {}", path.display(), e);
                    }
                }
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
                cb(entry).map_err(|e| io::Error::other(e.to_string()))?;
            }
        }
    }
    Ok(())
}

// ==================== 主举报处理器 ====================
pub struct ReportProcessor {
    pub fetcher: ReportFetcher,
    pub pipeline_factory: Arc<ReportTypeRegistry>,
    pub batch_manager: Arc<Mutex<BatchActionManager>>,
}

impl ReportProcessor {
    pub fn new() -> Self {
        let fetcher = ReportFetcher::new();
        let registry = Arc::new(fetcher.registry.clone());
        let batch_manager = Arc::new(Mutex::new(BatchActionManager::new()));
        ReportProcessor {
            fetcher,
            pipeline_factory: registry,
            batch_manager,
        }
    }

    pub fn process_all_reports(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        println!("=== 开始处理所有举报 ===");
        self.batch_manager.lock().unwrap().clear_processed_records();

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

        let mut total_processed = 0i64;

        for (chunk_count, chunk) in self
            .fetcher
            .fetch_reports_chunked(ReportStatus::ToBeDone)
            .enumerate()
        {
            println!(
                "处理第 {} 块数据, 共 {} 条举报",
                chunk_count + 1,
                chunk.len()
            );

            if total >= 15 {
                let batch_groups = self.identify_batch_groups(&chunk);
                if let Err(e) = self.handle_batch_groups(&batch_groups, &chunk, admin_id) {
                    eprintln!("批量组处理出错: {}，继续处理剩余记录", e);
                }
            }

            // 错误不再中断整个循环
            match self.process_chunk_with_pipeline(&chunk, admin_id) {
                Ok(processed) => total_processed += processed,
                Err(e) => eprintln!("块处理出错: {}，跳过该块", e),
            }

            println!(
                "第 {} 块处理完成, 处理了 {} 条举报",
                chunk_count + 1,
                total_processed
            );
        }

        println!("所有举报处理完成, 共处理 {} 条举报", total_processed);
        Ok(total_processed)
    }

    fn identify_batch_groups(&self, chunk: &[Value]) -> Vec<BatchGroup> {
        let mut item_id_groups: HashMap<String, Vec<String>> = HashMap::new();
        let mut content_groups: HashMap<String, Vec<String>> = HashMap::new();

        for item in chunk {
            if let Some(report_type) = self.infer_report_type(item) {
                if let Some(config) = self.fetcher.registry.get_config(&report_type) {
                    let record_id = item
                        .get(&config.report_id_field)
                        .map(|v| value_to_string(v))
                        .unwrap_or_else(|| "0".to_string());

                    let item_id = item
                        .get(&config.item_id_field)
                        .map(|v| value_to_string(v))
                        .unwrap_or_default();

                    item_id_groups
                        .entry(item_id.clone())
                        .or_default()
                        .push(record_id.clone());

                    let content_key = format!(
                        "{}:{}:{}",
                        item.get(&config.content_field)
                            .map(|v| value_to_string(v))
                            .unwrap_or_default(),
                        report_type,
                        item.get(&config.source_id_field)
                            .map(|v| value_to_string(v))
                            .unwrap_or_default()
                    );
                    content_groups
                        .entry(content_key)
                        .or_default()
                        .push(record_id);
                }
            }
        }

        let mut batch_groups = Vec::new();
        let mut processed_ids = HashSet::new();

        for (item_id, record_ids) in &item_id_groups {
            if record_ids.len() >= 5 {
                batch_groups.push(BatchGroup::new("item_id", item_id, record_ids.clone()));
                processed_ids.extend(record_ids.clone());
            }
        }

        for (content_key, record_ids) in &content_groups {
            let filtered: Vec<String> = record_ids
                .iter()
                .filter(|id| !processed_ids.contains(*id))
                .cloned()
                .collect();
            if filtered.len() >= 3 {
                batch_groups.push(BatchGroup::new("content", content_key, filtered));
            }
        }

        batch_groups
    }

    fn handle_batch_groups(
        &self,
        batch_groups: &[BatchGroup],
        chunk: &[Value],
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        for group in batch_groups {
            println!(
                "处理批量组 [{}] {} (共 {} 条举报)",
                group.group_type,
                group.group_key,
                group.record_ids.len()
            );

            let saved_action = {
                let bm = self.batch_manager.lock().unwrap();
                bm.get_batch_action(&group.group_type, &group.group_key)
            };

            if let Some(action) = saved_action {
                println!("应用保存的批量动作: {}", action);
                for record_id in &group.record_ids {
                    if let Some(item) = chunk.iter().find(|v| self.record_id_matches(v, record_id))
                    {
                        if let Some(report_type) = self.infer_report_type(item) {
                            if self
                                .fetcher
                                .registry
                                .is_action_available(&report_type, &action)
                            {
                                let _ =
                                    self.apply_simple_action(item, &report_type, &action, admin_id);
                                self.batch_manager
                                    .lock()
                                    .unwrap()
                                    .mark_record_processed(record_id);
                            }
                        }
                    }
                }
            } else {
                if let Some(first_record_id) = group.record_ids.first() {
                    if let Some(first_item) = chunk
                        .iter()
                        .find(|v| self.record_id_matches(v, first_record_id))
                    {
                        if let Some(report_type) = self.infer_report_type(first_item) {
                            let config = self.fetcher.registry.get_config(&report_type).cloned();
                            let mut context = ProcessingContext::new(
                                first_record_id.clone(),
                                report_type.clone(),
                                first_item.clone(),
                                admin_id,
                            );
                            context.is_batch_mode = false;
                            context.config = config;

                            let pipeline = ProcessingPipeline::create_default(
                                self.pipeline_factory.clone(),
                                self.batch_manager.clone(),
                            );
                            pipeline.execute(&mut context)?;

                            if let Some(action) = context.action {
                                self.batch_manager.lock().unwrap().save_batch_action(
                                    &group.group_type,
                                    &group.group_key,
                                    &action,
                                );

                                for record_id in &group.record_ids[1..] {
                                    if let Some(item) =
                                        chunk.iter().find(|v| self.record_id_matches(v, record_id))
                                    {
                                        if let Some(rt) = self.infer_report_type(item) {
                                            self.apply_simple_action(item, &rt, &action, admin_id)?;
                                            self.batch_manager
                                                .lock()
                                                .unwrap()
                                                .mark_record_processed(record_id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn record_id_matches(&self, item: &Value, target_id: &str) -> bool {
        if let Some(rt) = self.infer_report_type(item) {
            if let Some(cfg) = self.fetcher.registry.get_config(&rt) {
                return item
                    .get(&cfg.report_id_field)
                    .map(|v| value_to_string(&v))
                    .map(|s| s == target_id)
                    .unwrap_or(false);
            }
        }
        false
    }

    fn process_chunk_with_pipeline(
        &self,
        chunk: &[Value],
        admin_id: i32,
    ) -> Result<i64, ProcessorError> {
        let mut processed = 0i64;

        for item in chunk {
            // 提前计算 report_type 和 config，避免重复 infer
            let report_type = self.infer_report_type(item);
            let config = report_type
                .as_ref()
                .and_then(|rt| self.fetcher.registry.get_config(rt));

            let record_id = config
                .map(|c| {
                    item.get(&c.report_id_field)
                        .map(|v| value_to_string(v))
                        .unwrap_or_else(|| "0".to_string())
                })
                .unwrap_or_else(|| "0".to_string());

            if self
                .batch_manager
                .lock()
                .unwrap()
                .is_record_processed(&record_id)
            {
                continue;
            }

            if let (Some(rt), Some(cfg)) = (report_type, config) {
                let mut context =
                    ProcessingContext::new(record_id.clone(), rt.clone(), item.clone(), admin_id);
                context.config = Some(cfg.clone());

                let pipeline = ProcessingPipeline::create_default(
                    self.pipeline_factory.clone(),
                    self.batch_manager.clone(),
                );
                // 捕获单条错误，防止块中断
                if let Err(e) = pipeline.execute(&mut context) {
                    eprintln!("处理记录 {} 失败: {}，跳过", record_id, e);
                    continue;
                }

                if context.processed {
                    processed += 1;
                    self.batch_manager
                        .lock()
                        .unwrap()
                        .mark_record_processed(&record_id);
                }
            }
        }

        Ok(processed)
    }

    fn apply_simple_action(
        &self,
        item: &Value,
        report_type: &str,
        action: &str,
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        if let Some(config) = self.fetcher.registry.get_config(report_type) {
            let status_map = self.fetcher.registry.get_status_mapping();
            if let Some(resolution) = status_map.get(action) {
                let report_id = config.get_report_id(item);
                apply_action_by_method(&config.handle_method, report_id, admin_id, resolution)?;
            }
        }
        Ok(())
    }

    fn pass_all_pending(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        println!("=== 开始一键通过所有待处理举报 ===");
        let mut count = 0i64;

        for chunk in self.fetcher.fetch_reports_chunked(ReportStatus::ToBeDone) {
            for item in chunk {
                if let Some(report_type) = self.infer_report_type(&item) {
                    let config = self.fetcher.registry.get_config(&report_type);
                    let report_id = config.map(|cfg| cfg.get_report_id(&item)).unwrap_or(0);
                    if let Some(cfg) = config {
                        match cfg.handle_method.as_str() {
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
                                    .execute_process_work_report(
                                        report_id,
                                        admin_id,
                                        Resolution::Pass,
                                    )
                                    .ok();
                            }
                            "execute_process_post_report" => {
                                ReportHandler::new()
                                    .execute_process_post_report(
                                        report_id,
                                        admin_id,
                                        Resolution::Pass,
                                    )
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
        }

        println!("一键通过完成, 共通过 {} 条举报", count);
        Ok(count)
    }

    fn infer_report_type(&self, item: &Value) -> Option<String> {
        // 优先使用数据中注入的 _report_type
        if let Some(t) = item.get("_report_type").and_then(|v| v.as_str()) {
            return Some(t.to_string());
        }
        // 向后兼容：如果没有标记，走原来的字段推断
        if item.get("comment_content").is_some() || item.get("comment_id").is_some() {
            Some("shop_comment".into())
        } else if item.get("work_name").is_some() {
            Some("work_work".into())
        } else if item.get("discussion_content").is_some() || item.get("discussion_id").is_some() {
            Some("forum_discussion".into())
        } else if item.get("post_title").is_some() && item.get("board_name").is_some() {
            Some("forum_post".into())
        } else {
            None
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
    VIOLATION_CHECKER.get_or_init(ViolationChecker::new)
}
