use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use fastrand;
use serde_json::Value;

use crate::api::forum::{
    ForumActionHandler, ForumDataFetcher, ForumReportReasonId, ItemType, PostReportReasonId,
};
use crate::api::shop::{WorkShopReportReasonId, WorkshopActionHandler, WorkshopDataFetcher};
use crate::api::whale::{ReportHandler, Resolution};
use crate::api::work::{BaseWorkOperations, CommentOperations, WorkDataFetcher};
use crate::core::types::CommentConfig;
use crate::utils::acquire::{BaseKey, Catsona, HttpMethod, KittyFactory, PaginatedIter};
use crate::utils::data::PathConfig;

use super::types::{
    ProcessorError, ReportTypeRegistry, SourceConfig, get_valid_input, html_to_text, prompt_input,
    timestamp_to_string, value_to_i64, value_to_string,
};

// ==================== 硬编码配置数据 ====================

/// 官方账号 ID 列表
const OFFICIAL_IDS: [i64; 9] = [
    128963, 629055, 203577, 859722, 148883, 2191000, 7492052, 387963, 3649031,
];

/// 广告关键词列表（默认值）
const DEFAULT_ADS_KEYWORDS: &[&str] = &[
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
];

/// 刷屏阈值
const DEFAULT_SPAM_THRESHOLD: i64 = 3;

/// 违规检查时请求评论的默认数量
const DEFAULT_COMMENT_FETCH_LIMIT: usize = 100;

/// 学生账号单次登录最大举报次数
const MAX_REPORTS_PER_ACCOUNT: usize = 25;

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
        s.to_string()
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
            .unwrap_or(DEFAULT_SPAM_THRESHOLD) as usize;

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
            .and_then(value_to_i64);

        if let Some(uid) = user_id {
            context.user_id = Some(uid);
            if OFFICIAL_IDS.contains(&uid) {
                context.messages.push("官方内容，自动通过".into());
                context.action = Some("P".into());
                context.processed = true;

                // 复用注册表的状态映射（这里无法获取注册表，直接使用局部常量）
                let status_map = HashMap::from([
                    ("D".to_string(), "DELETE".to_string()),
                    ("S".to_string(), "MUTE_SEVEN_DAYS".to_string()),
                    ("T".to_string(), "MUTE_THREE_MONTHS".to_string()),
                    ("P".to_string(), "PASS".to_string()),
                ]);

                if let Some(resolution) = status_map.get("P") {
                    let report_id = config.get_report_id(&context.item)?;
                    apply_action_by_method(
                        &config.handle_method,
                        report_id,
                        context.admin_id,
                        resolution,
                    )?;
                    context.messages.push("已自动通过官方内容".into());
                    println!("自动通过官方举报ID: {}", context.record_id);
                }
            }
        }

        Ok(())
    }
}

// ==================== 详情显示处理器 ====================
pub struct DetailDisplayProcessor;

impl DetailDisplayProcessor {
    fn display_report(item: &Value, config: &SourceConfig, report_type: &str) {
        let base_url = "https://shequ.codemao.cn";
        println!("=== {} 详情 ===", config.name);

        macro_rules! print_if {
            ($label:expr, $field:expr, $transform:expr) => {
                if let Some(val) = item.get($field) {
                    println!("{}: {}", $label, $transform(val));
                }
            };
            ($label:expr, $field:expr) => {
                if let Some(val) = item.get($field).and_then(|v| v.as_str()) {
                    println!("{}: {}", $label, val);
                }
            };
        }

        match report_type {
            "work_work" => {
                print_if!("作者昵称", &config.user_nickname_field);
                print_if!("作者链接", &config.user_id_field, |v: &Value| format!(
                    "{}/user/{}",
                    base_url,
                    value_to_string(v)
                ));
                print_if!("作品链接", &config.source_id_field, |v: &Value| format!(
                    "{}/work/{}",
                    base_url,
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
            "shop_comment" => {
                print_if!("举报内容", &config.content_field, |v: &Value| {
                    html_to_text(v.as_str().unwrap_or(""))
                });
                print_if!("被举报人昵称", &config.user_nickname_field);
                print_if!("被举报人链接", &config.user_id_field, |v: &Value| format!(
                    "{}/user/{}",
                    base_url,
                    value_to_string(v)
                ));
                print_if!("工作室名称", &config.source_name_field);
                print_if!("工作室链接", &config.source_id_field, |v: &Value| format!(
                    "{}/work_shop/{}",
                    base_url,
                    value_to_string(v)
                ));
                print_if!("举报原因", &config.reason_field);
                print_if!("举报时间", &config.created_at_field, |v: &Value| {
                    timestamp_to_string(v)
                });
            }
            "forum_post" => {
                print_if!("帖子作者", &config.user_nickname_field);
                print_if!("作者链接", &config.user_id_field, |v: &Value| format!(
                    "{}/user/{}",
                    base_url,
                    value_to_string(v)
                ));
                if let Ok(post_id) = item
                    .get(&config.source_id_field)
                    .map(value_to_string)
                    .unwrap_or_default()
                    .parse::<i32>()
                    && let Ok(details) = ForumDataFetcher::new().fetch_single_post_details(post_id)
                    && let Some(content) = details.get("content").and_then(|v| v.as_str())
                {
                    println!("内容: {}", truncate_chars(&html_to_text(content), 200));
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
            "forum_discussion" => {
                print_if!("被举报内容", &config.content_field, |v: &Value| {
                    html_to_text(v.as_str().unwrap_or(""))
                });
                print_if!("被举报人昵称", &config.user_nickname_field);
                print_if!("被举报人链接", &config.user_id_field, |v: &Value| format!(
                    "{}/user/{}",
                    base_url,
                    value_to_string(v)
                ));
                print_if!("帖子链接", &config.source_id_field, |v: &Value| format!(
                    "{}/community/{}",
                    base_url,
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
            _ => {
                print_if!("内容", &config.content_field);
                print_if!("举报原因", &config.reason_field);
                print_if!("举报描述", &config.description_field);
                print_if!("用户昵称", &config.user_nickname_field);
                print_if!("举报时间", &config.created_at_field, |v: &Value| {
                    timestamp_to_string(v)
                });
            }
        }
    }
}

impl Processor for DetailDisplayProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        let config = match &context.config {
            Some(c) => c,
            None => return Ok(()),
        };
        Self::display_report(&context.item, config, &context.report_type);
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
            .and_then(value_to_i64)
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
            .and_then(value_to_i64);
        let title = config
            .title_field
            .as_ref()
            .and_then(|field| context.item.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let checker = violation_checker();
        // 传入 config
        checker.check_violation(source_id, source_type, board_name, user_id, title, &config)?;

        println!("=== 检查结束 ===");
        Ok(())
    }
}

impl Processor for ActionSelectionProcessor {
    fn process(&self, context: &mut ProcessingContext) -> Result<(), ProcessorError> {
        // 批量模式：从 batch_manager 获取预先设定的动作
        if context.is_batch_mode {
            let config = context
                .config
                .as_ref()
                .ok_or_else(|| ProcessorError::Processing("批量模式缺少配置".into()))?;
            let group_type = &context.report_type;
            let group_key = &context.record_id; // 简化示例，实际可根据需要调整
            if let Some(action) = self
                .batch_manager
                .lock()
                .unwrap()
                .get_batch_action(group_type, group_key)
            {
                context.action = Some(action.clone());
                // 执行动作
                let status_map = self.registry.get_status_mapping();
                if let Some(resolution) = status_map.get(&action) {
                    let report_id = config.get_report_id(&context.item)?;
                    apply_action_by_method(
                        &config.handle_method,
                        report_id,
                        context.admin_id,
                        resolution,
                    )?;
                    println!("批量应用操作: {} -> {}", action, resolution);
                }
                context.processed = true;
            } else {
                context.skip_reason = Some("批量模式未找到预设动作".into());
                context.processed = true;
            }
            return Ok(());
        }

        // 交互式模式
        let actions = self.registry.get_available_actions(&context.report_type);
        let valid_keys: HashSet<String> = actions.iter().map(|a| a.key.clone()).collect();
        let prompt = self.registry.get_action_prompt(&context.report_type);

        loop {
            let choice = get_valid_input(&prompt, &valid_keys);

            match choice.as_str() {
                "D" | "S" | "T" | "P" | "U" => {
                    context.action = Some(choice.clone());
                    if let Some(config) = &context.config {
                        let status_map = self.registry.get_status_mapping();
                        if let Some(resolution) = status_map.get(&choice) {
                            let report_id = config.get_report_id(&context.item)?;
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
                    if let Some(config) = &context.config
                        && let Some(special_check) = config.special_check
                        && special_check(&context.item)
                    {
                        self.check_violation(context)?;
                        println!("违规检查完成, 请选择处理动作");
                        continue;
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
pub(crate) fn apply_action_by_method(
    method: &str,
    report_id: i32,
    admin_id: i32,
    resolution: &str,
) -> Result<bool, ProcessorError> {
    let resolution_enum = parse_resolution(resolution)?;
    let handler = ReportHandler::new();

    match method {
        "execute_process_comment_report" => handler
            .execute_process_comment_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        "execute_process_work_report" => handler
            .execute_process_work_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        "execute_process_post_report" => handler
            .execute_process_post_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        "execute_process_discussion_report" => handler
            .execute_process_discussion_report(report_id, admin_id, resolution_enum)
            .map_err(|e| ProcessorError::External(e.into())),
        _ => Err(ProcessorError::Processing(format!(
            "未知处理方法: {}",
            method
        ))),
    }
}

// ==================== 违规检查器（优化：避免一次性全量加载，利用迭代器） ====================
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
        title: &str,
        config: &SourceConfig, // 新增参数
    ) -> Result<(), ProcessorError> {
        println!(
            "检查违规: source_id={}, type={}, board={}, user={:?}",
            source_id, source_type, board_name, user_id
        );

        let total = self.get_comment_total(source_id, source_type)?;
        println!("该内容共有 {} 条评论", total);

        let limit_str = prompt_input("输入要获取的评论数: ");
        let limit: usize = limit_str.parse().unwrap_or(DEFAULT_COMMENT_FETCH_LIMIT);

        let mut iter = self.fetch_comments(source_id, source_type, limit);

        let ad_keywords: HashSet<String> = DEFAULT_ADS_KEYWORDS
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let spam_threshold = DEFAULT_SPAM_THRESHOLD as usize;

        // 从配置获取正确的字段名
        let content_field = &config.content_field;
        let user_id_field = &config.user_id_field;
        let content_id_field = &config.content_id_field;
        let parent_id_field = &config.parent_id_field;

        let mut ads_violations: Vec<String> = Vec::new();
        let mut duplicates_counter: HashMap<(String, String), (usize, Vec<String>)> =
            HashMap::new();
        let mut comment_count = 0;

        while let Some(item_result) = iter.next_item() {
            let value = match item_result {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("获取评论出错: {}", e);
                    break;
                }
            };
            if comment_count >= limit {
                break;
            }
            comment_count += 1;

            // 使用配置字段获取内容/用户ID/ID
            let content = value
                .get(content_field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let user_id_str = value
                .get(user_id_field)
                .map(value_to_string)
                .unwrap_or_default();
            let is_reply = value
                .get(parent_id_field)
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
                > 0;

            // 构建唯一标识符
            let content_id = value
                .get(content_id_field)
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            let parent_id = if is_reply {
                value
                    .get(parent_id_field)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
            } else {
                0
            };
            let identifier = format!(
                "{}:{}:{}:{}:{}",
                source_type,
                source_id,
                if is_reply { "reply" } else { "comment" },
                parent_id,
                content_id
            );

            // 广告检测
            if !content.is_empty() && ad_keywords.iter().any(|kw| content.contains(kw)) {
                let log_type = if is_reply { "回复" } else { "评论" };
                println!(
                    "广告 {} [{}]{} : {}",
                    log_type,
                    source_type.to_uppercase(),
                    title_preview_str(title),
                    truncate_chars(&content, 50)
                );
                ads_violations.push(identifier.clone());
            }

            // 刷屏统计
            if !user_id_str.is_empty() && !content.is_empty() {
                let entry = duplicates_counter
                    .entry((user_id_str.clone(), content.clone()))
                    .or_insert((0, Vec::new()));
                entry.0 += 1;
                entry.1.push(identifier);
            }
        }

        // 刷屏判定（不变）
        let mut duplicates_violations = Vec::new();
        for ((uid, content), (count, identifiers)) in &duplicates_counter {
            if *count >= spam_threshold {
                println!(
                    "用户 {} 刷屏评论: {}... - 出现 {} 次",
                    uid,
                    truncate_chars(content, 50),
                    count
                );
                duplicates_violations.extend(identifiers.iter().cloned());
            }
        }

        let mut violations: Vec<String> = Vec::new();
        violations.extend(ads_violations);
        violations.extend(duplicates_violations);

        // 论坛刷帖检测（不变，但也可考虑使用 config.title_field）
        if source_type == "forum"
            && let Some(uid) = user_id
        {
            let spam_violations = self.check_spam_posts(uid, title)?;
            violations.extend(spam_violations);
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

        // 注意：如果 API 支持按 user_id 过滤，应优先使用
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

        let threshold = DEFAULT_SPAM_THRESHOLD as usize;

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
                        HttpMethod::Get,
                        &format!("/creation-tools/v1/works/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "15")
                    .send()?;
                let json = KittyFactory::global_client().response_to_json(resp)?;
                Ok(json.get("total").and_then(|v| v.as_i64()).unwrap_or(0))
            }
            "shop" => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::Get,
                        &format!("/web/discussions/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("source", "WORK_SHOP")
                    .with_param("limit", "15")
                    .send()?;
                let json = KittyFactory::global_client().response_to_json(resp)?;
                let total = json.get("total").and_then(|v| v.as_i64()).unwrap_or(0);
                let total_reply = json.get("totalReply").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(total + total_reply)
            }
            "forum" => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::Get,
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

    /// 获取评论，利用迭代器特性控制数量
    fn fetch_comments(&self, source_id: i64, source_type: &str, limit: usize) -> PaginatedIter {
        match source_type {
            "work" => WorkDataFetcher::new().fetch_work_comments_gen(source_id as i32, Some(limit)),
            "forum" => {
                ForumDataFetcher::new().fetch_post_replies_gen(source_id as i32, None, Some(limit))
            }
            "shop" => WorkshopDataFetcher::new().fetch_workshop_discussions_gen(
                source_id as i32,
                None,
                None,
                Some(limit),
            ),
            _ => panic!("不支持的来源类型: {}", source_type), // 调用方应保证 source_type 合法
        }
    }

    fn process_auto_report(&self, violations: HashSet<String>) -> Result<(), ProcessorError> {
        let mut multi_account = MultiAccount::new();
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
        let violations_vec: Vec<_> = violations.into_iter().collect();
        let mut current_idx = 0usize;

        for (idx, violation) in violations_vec.iter().enumerate() {
            // 1. 寻找一个可用的账号（未达上限）
            let chosen_idx = loop {
                if accounts.is_empty() {
                    println!("所有账号已失效或达到上限，停止举报");
                    break None;
                }
                // 确保索引在合法范围内
                current_idx %= accounts.len();
                let usage = account_usage.get(&current_idx).copied().unwrap_or(0);
                if usage < MAX_REPORTS_PER_ACCOUNT {
                    break Some(current_idx);
                }
                // 当前账号已满，尝试下一个
                current_idx = (current_idx + 1) % accounts.len();
                // 若轮完一圈仍无可用账号，则终止
                if current_idx == 0 {
                    // 检查是否所有账号都满了
                    if accounts.iter().enumerate().all(|(i, _)| {
                        account_usage.get(&i).copied().unwrap_or(0) >= MAX_REPORTS_PER_ACCOUNT
                    }) {
                        break None;
                    }
                }
            };

            let chosen_idx = match chosen_idx {
                Some(i) => i,
                None => {
                    println!("所有账号均已达到举报上限，停止");
                    break;
                }
            };

            let (user, pass) = &accounts[chosen_idx];

            // 2. 登录（仅当首次使用该账号时）
            let usage = account_usage.get(&chosen_idx).copied().unwrap_or(0);
            if usage == 0 {
                match self.login_student(user, pass) {
                    Ok(()) => { /* 登录成功 */ }
                    Err(e) => {
                        println!("账号 {} 登录失败: {}，移除", user, e);
                        accounts.remove(chosen_idx);
                        // 清理对应的计数记录
                        account_usage.remove(&chosen_idx);
                        // 调整 current_idx，防止越界
                        if chosen_idx < current_idx && current_idx > 0 {
                            current_idx -= 1;
                        }
                        current_idx %= accounts.len().max(1);
                        continue; // 跳过当前违规，重新选择账号
                    }
                }
            }

            // 3. 执行举报
            match self.execute_single_report(violation, reason_content) {
                Ok(_) => {
                    success += 1;
                    // 安全地增加使用计数
                    let entry = account_usage.entry(chosen_idx).or_insert(0);
                    *entry += 1;
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

            // 4. 移动到下一个账号（轮转）
            current_idx = (chosen_idx + 1) % accounts.len();
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

// ==================== 多账号管理器（移除未使用的字段） ====================
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

// ==================== 全局单例 ====================
static COMMENT_PROCESSOR: OnceLock<CommentProcessor> = OnceLock::new();
static VIOLATION_CHECKER: OnceLock<ViolationChecker> = OnceLock::new();

pub fn comment_processor() -> &'static CommentProcessor {
    COMMENT_PROCESSOR.get_or_init(CommentProcessor::new)
}

pub fn violation_checker() -> &'static ViolationChecker {
    VIOLATION_CHECKER.get_or_init(ViolationChecker::new)
}

// ==================== 为 Vec<Value> 实现 CommentConfig，以便传递引用 ====================
impl CommentConfig for Vec<Value> {
    fn get_comments(&self, _item_id: i64) -> Option<&[Value]> {
        Some(self.as_slice())
    }
}
