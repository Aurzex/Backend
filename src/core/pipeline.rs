use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use log::{error, info, warn};
use serde_json::Value;

use super::registry::{
    ProcessorError, SourceConfig, html_to_text, timestamp_to_string, value_to_string,
};
use crate::api::forum::{
    ForumActionHandler, ForumDataFetcher, ForumReportReasonId, ItemType, PostReportReasonId,
};
use crate::api::shop::{ReportCommentArgs, WorkShopReportReasonId, WorkshopActionHandler};
use crate::api::whale::{ReportHandler, Resolution};
use crate::api::work::{BaseWorkOperations, CommentOperations};
use crate::core::retrieve::{CommentSource, DataQuery, JsonObject};
use crate::utils::acquire::{Catsona, KittyFactory};
use crate::utils::data::PathConfig;

// 配置结构体(依赖注入)
#[derive(Clone)]
pub struct CheckConfig {
    pub(crate) official_ids: &'static [i64],
    pub(crate) ad_keywords: &'static [&'static str],
    pub(crate) spam_threshold: usize,
    pub(crate) comment_fetch_default_limit: usize,
    pub(crate) max_reports_per_account: usize,
    pub(crate) batch_item_id_threshold: usize,
    pub(crate) batch_content_threshold: usize,
}

impl Default for CheckConfig {
    fn default() -> Self {
        CheckConfig {
            official_ids: &[
                128_963, 629_055, 203_577, 859_722, 148_883, 2_191_000, 7_492_052, 387_963,
                3_649_031,
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
            batch_item_id_threshold: 5,
            batch_content_threshold: 3,
        }
    }
}

// 静态映射与注册表
/// 来源类型映射
static SOURCE_TYPE_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("shop_comment", "shop"),
        ("forum_post", "forum"),
        ("forum_discussion", "forum"),
    ])
});
pub(crate) fn get_source_type_map() -> &'static HashMap<&'static str, &'static str> {
    &SOURCE_TYPE_MAP
}

// 公共工具函数
pub(crate) fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        s.chars().take(max_chars).collect::<String>() + "..."
    }
}

pub(crate) trait ReportIdExt {
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

// 批量组与管理器
#[derive(Debug, Clone)]
pub struct BatchGroup {
    pub group_type: String,
    pub group_key: String,
    /// 组内完整举报记录,保证跨 chunk 的组在阈值达成时仍能处理早期记录
    pub items: Vec<Value>,
}

impl BatchGroup {
    pub(crate) fn new(group_type: &str, group_key: &str, items: Vec<Value>) -> Self {
        BatchGroup {
            group_type: group_type.to_string(),
            group_key: group_key.to_string(),
            items,
        }
    }
}

#[derive(Default)]
pub(crate) struct BatchActionManager {
    batch_actions: HashMap<(String, String), String>,
    processed_records: HashSet<String>,
}

/// 批量动作记忆与已决策记录的上限;超过后整体清空
/// (两者都只是会话内的优化缓存,清空无副作用,仅失去记忆)
const MAX_BATCH_ACTIONS: usize = 512;
const MAX_PROCESSED_RECORDS: usize = 4096;

impl BatchActionManager {
    pub(crate) fn new() -> Self {
        BatchActionManager::default()
    }

    pub(crate) fn save_batch_action(&mut self, group_type: &str, group_key: &str, action: &str) {
        if self.batch_actions.len() >= MAX_BATCH_ACTIONS {
            self.batch_actions.clear();
        }
        self.batch_actions.insert(
            (group_type.to_string(), group_key.to_string()),
            action.to_string(),
        );
    }

    pub(crate) fn get_batch_action(&self, group_type: &str, group_key: &str) -> Option<String> {
        self.batch_actions
            .get(&(group_type.to_string(), group_key.to_string()))
            .cloned()
    }

    pub(crate) fn mark_record_processed(&mut self, record_id: &str) {
        if self.processed_records.len() >= MAX_PROCESSED_RECORDS {
            self.processed_records.clear();
        }
        self.processed_records.insert(record_id.to_string());
    }

    pub(crate) fn is_record_processed(&self, record_id: &str) -> bool {
        self.processed_records.contains(record_id)
    }

    pub(crate) fn clear_processed_records(&mut self) {
        self.processed_records.clear();
    }
}

// 动作注册表(静态函数表)
type ActionFn = fn(i32, i32, Resolution) -> Result<bool, ProcessorError>;

pub(crate) struct ActionRegistry {
    handlers: HashMap<&'static str, ActionFn>,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    pub(crate) fn new() -> Self {
        macro_rules! register_report_handler {
            ($handlers:ident, $method:literal, $handler:ident) => {
                $handlers.insert(
                    $method,
                    |report_id: i32,
                     admin_id: i32,
                     resolution: Resolution|
                     -> Result<bool, ProcessorError> {
                        ReportHandler::new()
                            .$handler(report_id, admin_id, resolution)
                            .map_err(ProcessorError::from)
                    },
                );
            };
        }
        let mut handlers: HashMap<&'static str, ActionFn> = HashMap::new();
        register_report_handler!(
            handlers,
            "execute_process_comment_report",
            execute_process_comment_report
        );
        register_report_handler!(
            handlers,
            "execute_process_work_report",
            execute_process_work_report
        );
        register_report_handler!(
            handlers,
            "execute_process_post_report",
            execute_process_post_report
        );
        register_report_handler!(
            handlers,
            "execute_process_discussion_report",
            execute_process_discussion_report
        );
        ActionRegistry { handlers }
    }

    pub(crate) fn apply(
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
    }
}

static ACTION_REGISTRY: LazyLock<ActionRegistry> = LazyLock::new(ActionRegistry::new);
pub(crate) fn global_action_registry() -> &'static ActionRegistry {
    &ACTION_REGISTRY
}

// 应用动作的便捷函数
pub(crate) fn apply_action_by_method(
    method: &str,
    report_id: i32,
    admin_id: i32,
    resolution: Resolution,
) -> Result<bool, ProcessorError> {
    global_action_registry().apply(method, report_id, admin_id, resolution)
}

/// 依据动作键查配置中的决议并执行处理动作;
/// 未知键或非执行类动作(检查违规/跳过)直接报错,避免调用方误把记录静默标记为已处理
pub(crate) fn apply_action_by_key(
    config: &SourceConfig,
    report_id: i32,
    admin_id: i32,
    action_key: &str,
) -> Result<(), ProcessorError> {
    let action = config
        .available_actions
        .iter()
        .find(|a| a.key == action_key)
        .ok_or_else(|| ProcessorError::Processing(format!("动作 {} 对该类型不可用", action_key)))?;
    let resolution = action
        .resolution
        .ok_or_else(|| ProcessorError::Processing(format!("动作 {} 为非执行类动作", action_key)))?;
    apply_action_by_method(&config.handle_method, report_id, admin_id, resolution)?;
    Ok(())
}

// 详情展示(字段表驱动)
pub(crate) trait ReportDisplay: Send + Sync {
    /// 生成详情展示行(由外部 ui 层输出)
    fn display(&self, item: &Value, config: &SourceConfig) -> Vec<String>;
}

/// 单个展示字段:标签,数据字段与可选格式化函数
struct DisplayField<'a> {
    label: &'static str,
    field: &'a str,
    format: Option<fn(&Value) -> String>,
}

impl<'a> DisplayField<'a> {
    /// 原样输出字符串字段
    fn raw(label: &'static str, field: &'a str) -> Self {
        DisplayField {
            label,
            field,
            format: None,
        }
    }

    /// 原样输出可选字段,为 None 时自动跳过
    fn optional_raw(label: &'static str, field: Option<&'a str>) -> Self {
        DisplayField {
            label,
            field: field.unwrap_or(""),
            format: None,
        }
    }

    /// 格式化输出字段
    fn formatted(label: &'static str, field: &'a str, format: fn(&Value) -> String) -> Self {
        DisplayField {
            label,
            field,
            format: Some(format),
        }
    }
}

fn user_link(v: &Value) -> String {
    format!("https://shequ.codemao.cn/user/{}", value_to_string(v))
}

fn timestamp_str(v: &Value) -> String {
    timestamp_to_string(v)
}

fn html_content(v: &Value) -> String {
    html_to_text(v.as_str().unwrap_or(""))
}

/// 渲染单个字段,无格式化函数时仅输出字符串字段;缺失时返回 None
fn print_field(
    item: &Value,
    label: &str,
    field: &str,
    format: Option<fn(&Value) -> String>,
) -> Option<String> {
    let val = item.get(field)?;
    let text = match format {
        Some(f) => f(val),
        None => val.as_str()?.to_string(),
    };
    Some(format!("{}: {}", label, text))
}

/// 渲染详情行:统一遍历字段表,避免各 Display 重复定义
fn render_details(item: &Value, config: &SourceConfig, fields: &[DisplayField<'_>]) -> Vec<String> {
    let mut lines = vec![format!("=== {} 详情 ===", config.name)];
    for f in fields {
        if let Some(line) = print_field(item, f.label, f.field, f.format) {
            lines.push(line);
        }
    }
    lines
}

/// 未注册类型时的通用详情行
pub(crate) fn generic_details(item: &Value, config: &SourceConfig) -> Vec<String> {
    render_details(
        item,
        config,
        &[
            DisplayField::raw("内容", &config.content_field),
            DisplayField::raw("举报原因", &config.reason_field),
            DisplayField::raw("举报描述", &config.description_field),
            DisplayField::raw("用户昵称", &config.user_nickname_field),
            DisplayField::formatted("举报时间", &config.created_at_field, timestamp_str),
        ],
    )
}

struct WorkWorkDisplay;
impl ReportDisplay for WorkWorkDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) -> Vec<String> {
        render_details(
            item,
            config,
            &[
                DisplayField::raw("作者昵称", &config.user_nickname_field),
                DisplayField::formatted("作者链接", &config.user_id_field, user_link),
                DisplayField::formatted("作品链接", &config.source_id_field, |v| {
                    format!("https://shequ.codemao.cn/work/{}", value_to_string(v))
                }),
                DisplayField::optional_raw("作品类型", config.work_type_field.as_deref()),
                DisplayField::raw("举报原因", &config.reason_field),
                DisplayField::raw("举报线索", &config.description_field),
                DisplayField::formatted("举报时间", &config.created_at_field, timestamp_str),
            ],
        )
    }
}

struct ShopCommentDisplay;
impl ReportDisplay for ShopCommentDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) -> Vec<String> {
        render_details(
            item,
            config,
            &[
                DisplayField::formatted("举报内容", &config.content_field, html_content),
                DisplayField::raw("被举报人昵称", &config.user_nickname_field),
                DisplayField::formatted("被举报人链接", &config.user_id_field, user_link),
                DisplayField::raw("工作室名称", &config.source_name_field),
                DisplayField::formatted("工作室链接", &config.source_id_field, |v| {
                    format!("https://shequ.codemao.cn/work_shop/{}", value_to_string(v))
                }),
                DisplayField::raw("举报原因", &config.reason_field),
                DisplayField::formatted("举报时间", &config.created_at_field, timestamp_str),
            ],
        )
    }
}

/// 拉取帖子正文,论坛帖子举报需要额外请求;返回内容行
fn forum_post_content_line(item: &Value, config: &SourceConfig) -> Option<String> {
    let post_id = item
        .get(&config.source_id_field)
        .map(value_to_string)
        .unwrap_or_default()
        .parse::<i32>()
        .ok()?;
    match ForumDataFetcher::new().fetch_single_post_details(post_id) {
        Ok(details) => details
            .get("content")
            .and_then(|v| v.as_str())
            .map(|content| format!("内容: {}", truncate_chars(&html_to_text(content), 200))),
        Err(e) => {
            warn!("获取帖子详情失败: {}", e);
            None
        }
    }
}

struct ForumPostDisplay;
impl ReportDisplay for ForumPostDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) -> Vec<String> {
        let mut lines = vec![format!("=== {} 详情 ===", config.name)];
        if let Some(line) = print_field(item, "帖子作者", &config.user_nickname_field, None) {
            lines.push(line);
        }
        if let Some(line) = print_field(item, "作者链接", &config.user_id_field, Some(user_link))
        {
            lines.push(line);
        }
        if let Some(line) = forum_post_content_line(item, config) {
            lines.push(line);
        }
        if let Some(line) = print_field(
            item,
            "标题",
            config.title_field.as_deref().unwrap_or(""),
            None,
        ) {
            lines.push(line);
        }
        for (label, field) in [
            ("举报原因", config.reason_field.as_str()),
            ("举报线索", config.description_field.as_str()),
        ] {
            if let Some(line) = print_field(item, label, field, None) {
                lines.push(line);
            }
        }
        if let Some(line) = print_field(
            item,
            "举报时间",
            &config.created_at_field,
            Some(timestamp_str),
        ) {
            lines.push(line);
        }
        lines
    }
}

struct ForumDiscussionDisplay;
impl ReportDisplay for ForumDiscussionDisplay {
    fn display(&self, item: &Value, config: &SourceConfig) -> Vec<String> {
        render_details(
            item,
            config,
            &[
                DisplayField::formatted("被举报内容", &config.content_field, html_content),
                DisplayField::raw("被举报人昵称", &config.user_nickname_field),
                DisplayField::formatted("被举报人链接", &config.user_id_field, user_link),
                DisplayField::formatted("帖子链接", &config.source_id_field, |v| {
                    format!("https://shequ.codemao.cn/community/{}", value_to_string(v))
                }),
                DisplayField::optional_raw("帖子标题", config.title_field.as_deref()),
                DisplayField::optional_raw("分区", config.board_name_field.as_deref()),
                DisplayField::raw("举报原因", &config.reason_field),
                DisplayField::formatted("举报时间", &config.created_at_field, timestamp_str),
            ],
        )
    }
}

// 详情展示注册表
static DISPLAY_REGISTRY: LazyLock<HashMap<&'static str, Box<dyn ReportDisplay>>> =
    LazyLock::new(|| {
        let mut m: HashMap<&'static str, Box<dyn ReportDisplay>> = HashMap::new();
        m.insert("work_work", Box::new(WorkWorkDisplay));
        m.insert("shop_comment", Box::new(ShopCommentDisplay));
        m.insert("forum_post", Box::new(ForumPostDisplay));
        m.insert("forum_discussion", Box::new(ForumDiscussionDisplay));
        m
    });
pub(crate) fn get_display_registry() -> &'static HashMap<&'static str, Box<dyn ReportDisplay>> {
    &DISPLAY_REGISTRY
}

// 违规信息枚举
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

// 违规检查器
pub(crate) struct ViolationChecker {
    config: CheckConfig,
    ad_keywords_cache: Arc<HashSet<String>>,
    /// 与处理会话的预取线程共享:自动举报会切换全局身份,必须与在途请求互斥
    network_lock: Arc<Mutex<()>>,
}

impl ViolationChecker {
    pub(crate) fn new(config: CheckConfig, network_lock: Arc<Mutex<()>>) -> Self {
        let ad_keywords_cache = Arc::new(
            config
                .ad_keywords
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
        );
        ViolationChecker {
            config,
            ad_keywords_cache,
            network_lock,
        }
    }

    /// 构建违规标识符 "source:source_id:type:parent_id:content_id"
    fn violation_identifier(
        source_type: &str,
        source_id: i64,
        is_reply: bool,
        parent_comment_id: i64,
        item_id: i64,
    ) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            source_type,
            source_id,
            if is_reply { "reply" } else { "comment" },
            parent_comment_id,
            item_id
        )
    }

    /// 处理单条评论,返回违规信息,仅在违规时构建标识符
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
        // 延迟到命中 Duplicate 分支才构造字符串,避免广告等高频场景白算
        let user_id = item.get("user_id").and_then(serde_json::Value::as_i64);
        let item_id = item
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        if !content.is_empty() && ad_keywords.iter().any(|kw| content.contains(kw)) {
            return Some(ViolationKind::Ad {
                identifier: Self::violation_identifier(
                    source_type,
                    source_id,
                    is_reply,
                    parent_comment_id,
                    item_id,
                ),
            });
        }

        if let Some(user_id) = user_id
            && !content.is_empty()
        {
            return Some(ViolationKind::Duplicate {
                user_content: (user_id.to_string(), content),
                sample_identifier: Self::violation_identifier(
                    source_type,
                    source_id,
                    is_reply,
                    parent_comment_id,
                    item_id,
                ),
            });
        }

        None
    }

    /// 遍历评论与回复,收集违规候选
    fn collect_pending_violations(
        &self,
        comments: &[JsonObject],
        source_type: &str,
        source_id: i64,
    ) -> Vec<ViolationKind> {
        let mut pending = Vec::new();
        for comment in comments {
            if comment
                .get("is_top")
                .and_then(serde_json::Value::as_bool)
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
                pending.push(v);
            }
            let parent_comment_id = comment
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            if let Some(replies) = comment.get("replies").and_then(|v| v.as_array()) {
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
                        pending.push(v);
                    }
                }
            }
        }
        pending
    }

    /// 将违规候选分类为广告标识符与达到阈值的刷屏标识符
    fn classify_violations(pending: Vec<ViolationKind>, spam_threshold: usize) -> Vec<String> {
        let mut ads = Vec::new();
        let mut duplicate_counts: HashMap<(String, String), (usize, Vec<String>)> = HashMap::new();
        for v in pending {
            match v {
                ViolationKind::Ad { identifier } => ads.push(identifier),
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
        for (count, identifiers) in duplicate_counts.values() {
            if *count >= spam_threshold {
                ads.extend(identifiers.iter().cloned());
            }
        }
        ads
    }

    /// 流式获取详细评论,单条失败仅记录日志
    fn fetch_detailed_comments(
        source: CommentSource,
        source_id: i32,
        limit: usize,
    ) -> Result<Vec<JsonObject>, ProcessorError> {
        let stream = DataQuery::new()
            .stream_detailed_comments(source, source_id, Some(limit))
            .map_err(|e| ProcessorError::Processing(format!("获取评论流失败: {}", e)))?;
        let mut comments = Vec::new();
        for result in stream {
            match result {
                Ok(comment) => comments.push(comment),
                Err(e) => error!("获取评论失败: {},跳过", e),
            }
        }
        Ok(comments)
    }

    /// 检查违规,返回违规标识符列表(纯函数,不交互)
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_violations(
        &self,
        source_id: i64,
        source_type: &str,
        report_type: &str,
        board_name: &str,
        user_id: Option<i64>,
        title: &str,
        comment_limit: usize,
    ) -> Result<Vec<String>, ProcessorError> {
        info!(
            "检查违规: source_id={}, type={}, board={}, user={:?}",
            source_id, source_type, board_name, user_id
        );

        let comment_source: CommentSource = source_type
            .parse()
            .map_err(|_| ProcessorError::Processing(format!("未知来源类型: {}", source_type)))?;

        let source_id32 = i32::try_from(source_id).map_err(|_| {
            ProcessorError::Processing(format!("source_id 超出 i32 范围: {}", source_id))
        })?;

        let total = DataQuery::new()
            .count_comments(comment_source, source_id32)
            .map_err(|e| ProcessorError::Processing(format!("获取评论总数失败: {}", e)))?;
        info!("该内容共有 {} 条评论", total);

        let detailed_comments =
            Self::fetch_detailed_comments(comment_source, source_id32, comment_limit)?;

        let pending = self.collect_pending_violations(&detailed_comments, source_type, source_id);
        let mut violations = Self::classify_violations(pending, self.config.spam_threshold);

        // 刷帖检查只针对帖子举报;讨论举报仅做评论广告/刷屏
        if report_type == "forum_post"
            && let Some(uid) = user_id
        {
            violations.extend(self.check_spam_posts(uid, title));
        }

        let violations: HashSet<String> = violations.into_iter().collect();
        if violations.is_empty() {
            info!("未检测到违规内容");
        } else {
            info!("检测到 {} 条违规内容", violations.len());
        }
        Ok(violations.into_iter().collect())
    }

    fn check_spam_posts(&self, user_id: i64, title: &str) -> Vec<String> {
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
                    .and_then(serde_json::Value::as_i64)
                    == Some(user_id)
            })
            .collect();
        let threshold = self.config.spam_threshold;
        if user_posts.len() >= threshold {
            warn!(
                "警告: 用户 {} 已连续发布标题为[{}]的帖子 {} 次 (疑似刷屏)",
                user_id,
                title,
                user_posts.len()
            );
            let mut violations = Vec::new();
            for post in user_posts {
                if let Some(post_id) = post.get("id").and_then(serde_json::Value::as_i64) {
                    violations.push(format!("forum:{}:post:0:{}", post_id, post_id));
                }
            }
            return violations;
        }
        Vec::new()
    }

    /// 用学生账号自动举报违规内容,返回成功数(纯函数,不交互;账号缺失时返回错误)
    pub(crate) fn auto_report(&self, violations: &[String]) -> Result<usize, ProcessorError> {
        // 全程持网络锁:登录与举报会切换全局身份,禁止与预取线程的在途请求交错
        let _guard = self
            .network_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut multi_account = MultiAccount::new();
        let password_path = PathConfig::global().password_file_path();
        if !password_path.exists() {
            return Err(ProcessorError::Processing(
                "未找到学生账号文件,无法自动举报".into(),
            ));
        }
        multi_account.load_from_file(&password_path)?;
        if multi_account.accounts.is_empty() {
            return Err(ProcessorError::Processing(
                "未加载学生账号,无法自动举报".into(),
            ));
        }

        let violations: HashSet<String> = violations.iter().cloned().collect();
        let mut accounts = multi_account.accounts.clone();
        let success = self.report_violations(&mut accounts, &violations);
        if let Err(e) = KittyFactory::global_client().switch_identity(Catsona::Judge) {
            warn!("切换回管理员身份失败: {}", e);
        }
        info!("自动举报完成,成功 {}/{}", success, violations.len());
        Ok(success)
    }

    /// 轮询选择一个未达举报上限的账号索引,无可用账号时返回 None
    fn select_report_account(
        &self,
        accounts: &[(String, String)],
        account_usage: &HashMap<usize, usize>,
        current_idx: &mut usize,
    ) -> Option<usize> {
        if accounts.is_empty() {
            return None;
        }
        let start = *current_idx % accounts.len();
        for offset in 0..accounts.len() {
            let idx = (start + offset) % accounts.len();
            let usage = account_usage.get(&idx).copied().unwrap_or(0);
            if usage < self.config.max_reports_per_account {
                *current_idx = idx;
                return Some(idx);
            }
        }
        None
    }

    /// 确保账号已登录,登录失败则移除该账号并返回 false
    fn ensure_account_login(
        &self,
        accounts: &mut Vec<(String, String)>,
        account_usage: &mut HashMap<usize, usize>,
        idx: usize,
        current_idx: &mut usize,
    ) -> bool {
        if account_usage.get(&idx).copied().unwrap_or(0) > 0 {
            return true; // 本周期已登录过
        }
        let (user, pass) = accounts[idx].clone();
        match Self::login_student(&user, &pass) {
            Ok(()) => true,
            Err(e) => {
                warn!("账号 {} 登录失败: {},移除", user, e);
                accounts.remove(idx);
                account_usage.remove(&idx);
                if idx < *current_idx && *current_idx > 0 {
                    *current_idx -= 1;
                }
                *current_idx %= accounts.len().max(1);
                false
            }
        }
    }

    /// 用多账号轮流举报违规内容,返回成功数(逐条错误仅记录日志)
    fn report_violations(
        &self,
        accounts: &mut Vec<(String, String)>,
        violations: &HashSet<String>,
    ) -> usize {
        const REASON_CONTENT: &str = "违规内容";
        if accounts.is_empty() {
            info!("没有可用账号");
            return 0;
        }
        let violations_vec: Vec<_> = violations.iter().collect();
        let mut success = 0usize;
        let mut account_usage: HashMap<usize, usize> = HashMap::new();
        let mut current_idx = 0usize;

        for (idx, violation) in violations_vec.iter().enumerate() {
            let Some(chosen_idx) =
                self.select_report_account(accounts, &account_usage, &mut current_idx)
            else {
                info!("所有账号均已达到举报上限,停止");
                break;
            };
            if !self.ensure_account_login(
                accounts,
                &mut account_usage,
                chosen_idx,
                &mut current_idx,
            ) {
                continue;
            }
            if let Err(e) = self.execute_single_report(violation, REASON_CONTENT) {
                error!(
                    "[{}/{}] 举报失败: {} - {}",
                    idx + 1,
                    violations_vec.len(),
                    violation,
                    e
                );
            } else {
                success += 1;
                *account_usage.entry(chosen_idx).or_insert(0) += 1;
                info!(
                    "[{}/{}] 举报成功: {}",
                    idx + 1,
                    violations_vec.len(),
                    violation
                );
            }
            current_idx = (chosen_idx + 1) % accounts.len();
        }
        success
    }

    fn login_student(username: &str, password: &str) -> Result<(), ProcessorError> {
        crate::api::auth::LoginBuilder::new()
            .identity(username)
            .password(password)
            .status(crate::api::auth::AccountStatus::Edu)
            .execute()?;
        Ok(())
    }

    fn parse_violation(violation: &str) -> Result<(String, i64, String, i32, i32), ProcessorError> {
        let parts: Vec<&str> = violation.split(':').collect();
        if parts.len() != 5 {
            return Err(ProcessorError::Processing(format!(
                "违规标识符格式错误: 期望 5 段,实际 {} 段",
                parts.len()
            )));
        }
        let source = parts[0].to_string();
        let source_id: i64 = parts[1].parse().map_err(|_| {
            ProcessorError::Processing(format!("违规标识符 source_id 解析失败: {}", parts[1]))
        })?;
        let violation_type = parts[2].to_string();
        let parent_id: i32 = parts[3].parse().map_err(|_| {
            ProcessorError::Processing(format!("违规标识符 parent_id 解析失败: {}", parts[3]))
        })?;
        let content_id: i32 = parts[4].parse().map_err(|_| {
            ProcessorError::Processing(format!("违规标识符 content_id 解析失败: {}", parts[4]))
        })?;
        Ok((source, source_id, violation_type, parent_id, content_id))
    }

    fn execute_single_report(
        &self,
        violation: &str,
        reason_content: &str,
    ) -> Result<(), ProcessorError> {
        let parsed = Self::parse_violation(violation)?;
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
                    .map_err(ProcessorError::from)?;
            }
            "work" => {
                BaseWorkOperations::new()
                    .execute_report_work(content_id, reason_content, reason_content)
                    .map_err(ProcessorError::from)?;
            }
            "comment" | "reply" => {
                let is_reply = violation_type == "reply";
                match source.as_str() {
                    "work" => {
                        let work_id = i32::try_from(source_id).map_err(|_| {
                            ProcessorError::Processing(format!(
                                "作品 ID 超出 i32 范围: {}",
                                source_id
                            ))
                        })?;
                        CommentOperations::new()
                            .execute_report_comment(work_id, content_id, reason_content)
                            .map_err(ProcessorError::from)?;
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
                            .map_err(ProcessorError::from)?;
                    }
                    "shop" => {
                        let reporter_id = fastrand::i32(10000..=199_999_999);
                        WorkshopActionHandler::new()
                            .execute_report_comment(ReportCommentArgs {
                                comment_id: content_id,
                                reason_content,
                                reason_id: WorkShopReportReasonId::Reason7,
                                reporter_id,
                                comment_source: None,
                                comment_parent_id: if is_reply { Some(parent_id) } else { None },
                                description: Some(""),
                            })
                            .map_err(ProcessorError::from)?;
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

// 多账号管理器
pub(crate) struct MultiAccount {
    pub(crate) accounts: Vec<(String, String)>,
}

impl Default for MultiAccount {
    fn default() -> Self {
        Self::new()
    }
}

impl MultiAccount {
    pub(crate) fn new() -> Self {
        MultiAccount {
            accounts: Vec::new(),
        }
    }

    pub(crate) fn load_from_file(&mut self, path: &Path) -> Result<(), ProcessorError> {
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
}
