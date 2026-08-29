use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::api::whale::{
    CommentSourceType, ReportStatus, Resolution, WhaleReportFetcher, WorkSourceType,
};
use crate::utils::requests::{self, CodeMaoClient};
use log::error;

use serde_json::{Value, json};

// 自定义错误类型
#[derive(Debug, thiserror::Error)]
pub enum ProcessorError {
    #[error("Processing error: {0}")]
    Processing(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("External error: {0}")]
    Mew(#[from] requests::MewError),
    /// 用户在交互中主动中止(如按 Q 退出处理会话)
    #[error("Aborted by user")]
    Aborted,
}

// 辅助函数
pub(crate) fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// 将时间戳转换为字符串表示
pub(crate) fn timestamp_to_string(ts: &serde_json::Value) -> String {
    if let Some(secs) = ts.as_i64()
        && secs > 0
    {
        // 原实现先做 UNIX_EPOCH+Duration 再换算回秒数,结果恒等于 secs,属无意义换算
        return format!("{}", secs);
    }
    ts.to_string()
}

pub(crate) fn html_to_text(html: &str) -> String {
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

pub(crate) fn bytes_to_human(size_bytes: u64) -> String {
    if size_bytes >= 1024 * 1024 {
        format!("{:.2} MB", size_bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} KB", size_bytes as f64 / 1024.0)
    }
}

// 举报类型配置
#[derive(Debug, Clone)]
pub(crate) struct ActionConfig {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// 动作对应的处理决议;非执行类动作(检查违规/跳过)为 None
    pub(crate) resolution: Option<Resolution>,
    pub(crate) enabled: bool,
}

/// 举报处理动作(动作键语义与 `action_name` 一致)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportAction {
    Delete,         // "D"
    Mute7d,         // "S"
    Mute3m,         // "T"
    Unpublish,      // "U"
    Pass,           // "P"
    CheckViolation, // "F"
    Skip,           // "J"
}

impl ReportAction {
    pub(crate) fn key(self) -> &'static str {
        match self {
            ReportAction::Delete => "D",
            ReportAction::Mute7d => "S",
            ReportAction::Mute3m => "T",
            ReportAction::Unpublish => "U",
            ReportAction::Pass => "P",
            ReportAction::CheckViolation => "F",
            ReportAction::Skip => "J",
        }
    }

    pub fn from_key(s: &str) -> Option<ReportAction> {
        match s {
            "D" => Some(ReportAction::Delete),
            "S" => Some(ReportAction::Mute7d),
            "T" => Some(ReportAction::Mute3m),
            "U" => Some(ReportAction::Unpublish),
            "P" => Some(ReportAction::Pass),
            "F" => Some(ReportAction::CheckViolation),
            "J" => Some(ReportAction::Skip),
            _ => None,
        }
    }
}

/// 动作键对应的中文名称
pub(crate) fn action_name(key: &str) -> &'static str {
    match key {
        "D" => "删除",
        "S" => "禁言7天",
        "T" => "禁言3月",
        "U" => "取消发布",
        "P" => "通过",
        "F" => "检查违规",
        "J" => "跳过",
        _ => "未知动作",
    }
}

/// 决议/状态字符串对应的中文名称,用于展示已处理记录
pub(crate) fn resolution_display_name(status: &str) -> String {
    match status {
        "PASS" => "通过".into(),
        "DELETE" => "删除".into(),
        "MUTE_SEVEN_DAYS" => "禁言7天".into(),
        "MUTE_THREE_MONTHS" => "禁言3月".into(),
        "UNLOAD" => "取消发布".into(),
        "TOBEDONE" => "待处理".into(),
        "DONE" => "已处理".into(),
        s => s.to_string(),
    }
}

impl ActionConfig {
    /// 由动作键构造配置(名称与决议取自内置映射)
    fn simple(key: &str) -> Self {
        let resolution = match key {
            "D" => Some(Resolution::Delete),
            "S" => Some(Resolution::MuteSevenDays),
            "T" => Some(Resolution::MuteThreeMonths),
            "U" => Some(Resolution::Unload),
            "P" => Some(Resolution::Pass),
            _ => None,
        };
        ActionConfig {
            key: key.into(),
            name: action_name(key).into(),
            description: String::new(),
            resolution,
            enabled: true,
        }
    }
}

/// 按给定动作键列表批量构造动作配置
fn actions(keys: &[&str]) -> Vec<ActionConfig> {
    keys.iter().map(|k| ActionConfig::simple(k)).collect()
}

pub(crate) type FetchGenerator = Box<
    dyn Fn(ReportStatus) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>> + Send>
        + Send
        + Sync,
>;
pub(crate) type FetchTotal =
    Arc<dyn Fn(ReportStatus) -> Result<Value, ProcessorError> + Send + Sync>;

pub(crate) struct SourceConfig {
    pub(crate) admin_id_field: String,
    pub(crate) admin_username_field: String,
    pub(crate) available_actions: Vec<ActionConfig>,
    pub(crate) board_id_field: Option<String>,
    pub(crate) board_name_field: Option<String>,
    pub(crate) chunk_size: usize,
    pub(crate) content_field: String,
    pub(crate) content_id_field: String,
    pub(crate) content_type_field: String,
    pub(crate) created_at_field: String,
    pub(crate) description_field: String,
    pub(crate) fetch_generator: FetchGenerator,
    pub(crate) fetch_total: FetchTotal,
    pub(crate) handle_method: String,
    pub(crate) item_id_field: String,
    pub(crate) name: String,
    pub(crate) parent_id_field: String,
    pub(crate) reason_field: String,
    pub(crate) reason_id_field: String,
    pub(crate) report_id_field: String,
    pub(crate) source_id_field: String,
    pub(crate) source_name_field: String,
    pub(crate) source_object_id_field: String,
    pub(crate) source_object_name_field: String,
    pub(crate) source_type_field: String,
    pub(crate) special_check: Option<fn(&Value) -> bool>,
    pub(crate) status_field: String,
    pub(crate) title_field: Option<String>,
    pub(crate) user_id_field: String,
    pub(crate) user_nickname_field: String,
    pub(crate) user_parent_id_field: String,
    pub(crate) user_parent_nickname_field: String,
    pub(crate) work_type_field: Option<String>,
}

impl SourceConfig {
    /// 构造带公共默认字段名的配置;差异字段由调用方覆盖后再注册
    /// 大部分举报类型的字段名高度一致(如 `report_id_field` 均为 "id"),
    /// 通过"公共默认值 + 覆盖差异"大幅减少重复
    fn base<F, G>(name: &str, handle_method: &str, fetch_total: F, fetch_generator: G) -> Self
    where
        F: Fn(ReportStatus) -> Result<Value, ProcessorError> + Send + Sync + 'static,
        G: Fn(ReportStatus) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>> + Send>
            + Send
            + Sync
            + 'static,
    {
        SourceConfig {
            name: name.into(),
            handle_method: handle_method.into(),
            fetch_total: Arc::new(fetch_total),
            fetch_generator: Box::new(fetch_generator),
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

// 举报类型注册表
/// 按举报类型预计算的动作提示与合法键集合(注册表运行期不变)
pub(crate) struct ActionOptions {
    pub(crate) prompt: String,
    pub(crate) valid_keys: HashSet<String>,
    /// 有序可用动作列表(已过滤禁用项),供菜单渲染零分配
    pub(crate) actions: Vec<ActionConfig>,
}

pub(crate) struct ReportTypeRegistry {
    /// Arc 包装:每条举报记录的处理都需持有配置,避免逐记录深克隆整个 SourceConfig(~30 个 String)
    registry: HashMap<String, Arc<SourceConfig>>,
    default_actions: Vec<ActionConfig>, // 保留用于构建默认动作,也可直接为静态
    action_cache: Mutex<HashMap<String, Arc<ActionOptions>>>,
}

impl Default for ReportTypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportTypeRegistry {
    pub(crate) fn new() -> Self {
        let default_actions = actions(&["D", "S", "T", "U", "P", "F", "J"]);

        ReportTypeRegistry {
            registry: HashMap::new(),
            default_actions,
            action_cache: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn register(&mut self, report_type: &str, config: SourceConfig) {
        self.registry
            .insert(report_type.to_string(), Arc::new(config));
    }

    pub(crate) fn get_config(&self, report_type: &str) -> Option<&SourceConfig> {
        self.registry.get(report_type).map(Arc::as_ref)
    }

    /// 返回配置的 Arc 句柄,供处理上下文持有(零拷贝引用计数)
    pub(crate) fn get_config_arc(&self, report_type: &str) -> Option<Arc<SourceConfig>> {
        self.registry.get(report_type).cloned()
    }

    pub(crate) fn get_all_types(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }

    /// 获取(或按举报类型缓存)动作选项:提示,合法键与有序动作列表
    /// 缓存避免交互处理时每条记录重建提示与动作集合
    pub(crate) fn action_options(&self, report_type: &str) -> Arc<ActionOptions> {
        let mut cache = self.action_cache.lock().unwrap();

        if let Some(cached) = cache.get(report_type) {
            return Arc::clone(cached);
        }
        let actions: Vec<ActionConfig> = self
            .get_config(report_type)
            .map(|config| {
                config
                    .available_actions
                    .iter()
                    .filter(|a| a.enabled && a.key != "C")
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let parts: Vec<String> = actions
            .iter()
            .map(|a| format!("{}({})", a.key, a.name))
            .collect();
        let options = Arc::new(ActionOptions {
            prompt: format!("选择操作:{}", parts.join(",")),
            valid_keys: actions.iter().map(|a| a.key.clone()).collect(),
            actions,
        });
        cache.insert(report_type.to_string(), Arc::clone(&options));
        options
    }

    pub(crate) fn is_action_available(&self, report_type: &str, action_key: &str) -> bool {
        self.action_options(report_type)
            .valid_keys
            .contains(action_key)
    }
}

// 举报获取器

/// 注册辅助:包装分页迭代器为"总数"闭包
fn total_from(mut paginated: requests::PaginatedIter) -> Result<Value, ProcessorError> {
    paginated.fetch_metadata()?;
    let total = paginated
        .total_items()
        .ok_or_else(|| ProcessorError::Processing("分页元数据缺少总数".into()))?;
    let total = i64::try_from(total)
        .map_err(|_| ProcessorError::Processing(format!("总数超出 i64 范围: {}", total)))?;
    Ok(json!(total))
}

/// 注册辅助:包装分页迭代器为"生成器"闭包
fn gen_from(
    paginated: requests::PaginatedIter,
) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>> + Send> {
    Box::new(paginated.map(|r| r.map_err(ProcessorError::from)))
}

/// 批量设置 `SourceConfig` 的字符串/动作列表字段,减少逐行赋值样板
macro_rules! set_config_fields {
    ($cfg:expr, $( $field:ident = $value:expr ),* $(,)?) => {
        $(
            $cfg.$field = $value.into();
        )*
    };
}

/// `fetch_chunked` 中当前正在产出的举报来源(类型 + 持久生成器)
struct ActiveSource {
    report_type: String,
    config: Arc<SourceConfig>,
    generator: Box<dyn Iterator<Item = Result<Value, ProcessorError>> + Send>,
}

pub(crate) struct ReportFetcher {
    client: CodeMaoClient,
    /// Arc 共享:ReportProcessor 的管道工厂与 fetcher 复用同一注册表,避免深拷贝
    pub(crate) registry: Arc<ReportTypeRegistry>,
}

impl Default for ReportFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportFetcher {
    pub(crate) fn new() -> Self {
        Self::new_with_client(CodeMaoClient::global().clone())
    }

    pub(crate) fn new_with_client(client: CodeMaoClient) -> Self {
        let mut registry = ReportTypeRegistry::new();

        // shop_comment
        {
            let mut cfg = SourceConfig::base(
                "工作室评论举报",
                "execute_process_comment_report",
                {
                    let c = client.clone();
                    move |status| {
                        total_from(
                            WhaleReportFetcher::new_with_client(c.clone())
                                .fetch_comment_reports_iter(
                                    CommentSourceType::All,
                                    status,
                                    None,
                                    None,
                                    None,
                                ),
                        )
                    }
                },
                {
                    let c = client.clone();
                    move |status| {
                        gen_from(
                            WhaleReportFetcher::new_with_client(c.clone())
                                .fetch_comment_reports_iter(
                                    CommentSourceType::All,
                                    status,
                                    None,
                                    None,
                                    None,
                                ),
                        )
                    }
                },
            );
            set_config_fields!(
                cfg,
                admin_username_field = "admin_user_name",
                available_actions = actions(&["D", "S", "T", "P", "F", "J"]),
                content_field = "comment_content",
                content_type_field = "comment_source",
                content_id_field = "comment_id",
                user_id_field = "comment_user_id",
                user_nickname_field = "comment_user_nickname",
                user_parent_id_field = "comment_parent_user_id",
                user_parent_nickname_field = "comment_parent_user_nickname",
                source_id_field = "comment_source_object_id",
                source_name_field = "comment_source_object_name",
                source_type_field = "comment_source",
                source_object_id_field = "comment_source_object_id",
                source_object_name_field = "comment_source_object_name",
                parent_id_field = "comment_parent_id",
            );
            cfg.special_check = Some(|item| {
                item.get("comment_source")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s == "WORK_SHOP")
            });
            registry.register("shop_comment", cfg);
        }

        // work_work
        {
            let mut cfg = SourceConfig::base(
                "作品举报",
                "execute_process_work_report",
                {
                    let c = client.clone();
                    move |status| {
                        total_from(
                            WhaleReportFetcher::new_with_client(c.clone()).fetch_work_reports_iter(
                                WorkSourceType::All,
                                status,
                                None,
                                None,
                                None,
                            ),
                        )
                    }
                },
                {
                    let c = client.clone();
                    move |status| {
                        gen_from(
                            WhaleReportFetcher::new_with_client(c.clone()).fetch_work_reports_iter(
                                WorkSourceType::All,
                                status,
                                None,
                                None,
                                None,
                            ),
                        )
                    }
                },
            );
            set_config_fields!(
                cfg,
                admin_username_field = "admin_username",
                available_actions = actions(&["D", "P", "U", "J"]),
                content_field = "work_name",
                content_type_field = "work_type",
                content_id_field = "work_id",
                user_id_field = "work_user_id",
                user_nickname_field = "work_user_nickname",
                source_id_field = "work_id",
                source_name_field = "work_name",
                source_type_field = "work_type",
                source_object_id_field = "work_id",
                source_object_name_field = "work_name",
            );
            cfg.work_type_field = Some("work_type".into());
            cfg.title_field = Some("work_name".into());
            registry.register("work_work", cfg);
        }

        // forum_post
        {
            let mut cfg = SourceConfig::base(
                "帖子举报",
                "execute_process_post_report",
                {
                    let c = client.clone();
                    move |status| {
                        total_from(
                            WhaleReportFetcher::new_with_client(c.clone())
                                .fetch_post_reports_iter(status, None, None, None, None),
                        )
                    }
                },
                {
                    let c = client.clone();
                    move |status| {
                        gen_from(
                            WhaleReportFetcher::new_with_client(c.clone())
                                .fetch_post_reports_iter(status, None, None, None, None),
                        )
                    }
                },
            );
            set_config_fields!(
                cfg,
                admin_username_field = "admin_username",
                available_actions = actions(&["D", "S", "T", "P", "F", "J"]),
                content_field = "post_title",
                content_type_field = "board_name",
                content_id_field = "post_id",
                user_id_field = "post_user_id",
                user_nickname_field = "post_user_nick_name",
                source_id_field = "post_id",
                source_name_field = "board_name",
                source_type_field = "board_name",
                source_object_id_field = "post_id",
                source_object_name_field = "board_name",
            );
            cfg.title_field = Some("post_title".into());
            cfg.board_name_field = Some("board_name".into());
            cfg.board_id_field = Some("board_id".into());
            // 支持检查违规:帖子评论广告/刷屏 + 作者刷帖
            cfg.special_check = Some(|_| true);
            registry.register("forum_post", cfg);
        }

        // forum_discussion
        {
            let mut cfg = SourceConfig::base(
                "讨论举报",
                "execute_process_discussion_report",
                {
                    let c = client.clone();
                    move |status| {
                        total_from(
                            WhaleReportFetcher::new_with_client(c.clone())
                                .fetch_discussion_reports_iter(status, None, None, None, None),
                        )
                    }
                },
                {
                    let c = client.clone();
                    move |status| {
                        gen_from(
                            WhaleReportFetcher::new_with_client(c.clone())
                                .fetch_discussion_reports_iter(status, None, None, None, None),
                        )
                    }
                },
            );
            set_config_fields!(
                cfg,
                admin_username_field = "admin_username",
                available_actions = actions(&["D", "S", "T", "P", "F", "J"]),
                content_field = "discussion_content",
                content_type_field = "discussion_source",
                content_id_field = "discussion_id",
                user_id_field = "discussion_user_id",
                user_nickname_field = "discussion_user_nickname",
                source_id_field = "post_id",
                source_name_field = "post_title",
                source_type_field = "discussion_source",
                source_object_id_field = "post_id",
                source_object_name_field = "post_title",
            );
            cfg.title_field = Some("post_title".into());
            cfg.board_name_field = Some("board_name".into());
            cfg.board_id_field = Some("board_id".into());
            // 支持检查违规:讨论评论/回复的广告关键词与刷屏
            cfg.special_check = Some(|_| true);
            registry.register("forum_discussion", cfg);
        }

        ReportFetcher {
            client,
            registry: Arc::new(registry),
        }
    }

    pub(crate) fn fetch_chunked(
        &self,
        status: ReportStatus,
    ) -> Box<dyn Iterator<Item = Vec<Value>> + Send> {
        // 预取全部类型的配置 Arc,迭代器不再借用 self,可跨线程移动(供处理会话后台预取)
        let sources: Vec<(String, Arc<SourceConfig>)> = self
            .registry
            .get_all_types()
            .into_iter()
            .filter_map(|rt| self.registry.get_config_arc(&rt).map(|cfg| (rt, cfg)))
            .collect();
        let total_types = sources.len();
        let mut type_index = 0;
        let mut pending_items: Vec<Value> = Vec::new();
        // 当前类型的生成器在闭包调用间保持,避免每个 chunk 都从头重建
        // (原实现每次重建会重复产出已有数据,类型数据超过 chunk_size 时陷入死循环)
        let mut active: Option<ActiveSource> = None;

        Box::new(std::iter::from_fn(move || {
            let mut chunk = Vec::new();
            std::mem::swap(&mut chunk, &mut pending_items);

            loop {
                // 惰性进入下一类型,为每个类型创建一次生成器
                if active.is_none() {
                    if type_index >= total_types {
                        break;
                    }
                    let (report_type, config) = sources[type_index].clone();
                    type_index += 1;
                    active = Some(ActiveSource {
                        report_type,
                        config: config.clone(),
                        generator: (config.fetch_generator)(status),
                    });
                }

                let Some(source) = active.as_mut() else { break };
                let chunk_size = source.config.chunk_size;

                // 单页瞬时错误:PaginatedIter 已保证错误后 next() 重试同一页,这里做有界重试
                const FETCH_RETRY: usize = 3;
                loop {
                    let result = source.generator.next();
                    let mut item = match result {
                        Some(Ok(item)) => item,
                        None => break,
                        Some(Err(e)) => {
                            let mut recovered = None;
                            for attempt in 1..=FETCH_RETRY {
                                thread::sleep(Duration::from_millis(300 * attempt as u64));
                                match source.generator.next() {
                                    Some(Ok(v)) => {
                                        recovered = Some(v);
                                        break;
                                    }
                                    Some(Err(e2)) => {
                                        log::warn!(
                                            "获取举报数据重试 {}/{} 失败: {}",
                                            attempt,
                                            FETCH_RETRY,
                                            e2
                                        );
                                    }
                                    None => break,
                                }
                            }
                            match recovered {
                                Some(v) => v,
                                None => {
                                    error!("获取举报数据失败: {}, 跳过该类型余下数据", e);
                                    break;
                                }
                            }
                        }
                    };

                    if status == ReportStatus::ToBeDone
                        && let Some(state) = item
                            .get(&source.config.status_field)
                            .and_then(|v| v.as_str())
                        && state != ReportStatus::ToBeDone.as_str()
                    {
                        continue;
                    }

                    if let Value::Object(map) = &mut item {
                        map.insert(
                            "_report_type".into(),
                            Value::String(source.report_type.clone()),
                        );
                    }

                    chunk.push(item);
                    if chunk.len() >= chunk_size {
                        let remaining = chunk.split_off(chunk_size);
                        pending_items = remaining;
                        return Some(chunk);
                    }
                }

                // 当前类型耗尽或出错,推进到下一类型
                active = None;
            }

            if chunk.is_empty() { None } else { Some(chunk) }
        }))
    }

    pub(crate) fn fetch_reports_chunked(
        &self,
        status: ReportStatus,
    ) -> Box<dyn Iterator<Item = Vec<Value>> + Send> {
        self.fetch_chunked(status)
    }

    /// 一次并行批次同时取两个状态的总数(菜单渲染只需 1 个 RTT)
    pub(crate) fn get_totals_pair(&self, a: ReportStatus, b: ReportStatus) -> (i64, i64) {
        let ta = &AtomicI64::new(0);
        let tb = &AtomicI64::new(0);
        thread::scope(|s| {
            for rtype in self.registry.get_all_types() {
                let Some(config) = self.registry.get_config(&rtype) else {
                    continue;
                };
                let fetch_total_a = config.fetch_total.clone();
                let fetch_total_b = config.fetch_total.clone();
                let rtype_a = rtype.clone();
                let rtype_b = rtype.clone();
                s.spawn(move || match fetch_total_a(a) {
                    Ok(result) => {
                        ta.fetch_add(result.as_i64().unwrap_or(0), Ordering::Relaxed);
                    }
                    Err(e) => error!("获取 {} 总数失败: {}", rtype_a, e),
                });
                s.spawn(move || match fetch_total_b(b) {
                    Ok(result) => {
                        tb.fetch_add(result.as_i64().unwrap_or(0), Ordering::Relaxed);
                    }
                    Err(e) => error!("获取 {} 总数失败: {}", rtype_b, e),
                });
            }
        });
        (ta.load(Ordering::Relaxed), tb.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 回归测试:单类型数据量超过 chunk_size 时,`fetch_chunked` 必须终止且不重复
    /// (旧实现每次闭包调用都重建生成器,超过 chunk_size 后无限重复产出)
    #[test]
    fn fetch_chunked_terminates_without_duplicates() {
        const TOTAL: i32 = 350; // 3.5 个 chunk,> chunk_size=100
        let mut registry = ReportTypeRegistry::new();
        let mut cfg = SourceConfig::base(
            "测试类型",
            "execute_process_comment_report",
            |_| Ok(Value::from(TOTAL)),
            |_| {
                Box::new((0..TOTAL).map(|i| -> Result<Value, ProcessorError> {
                    Ok(json!({
                        "id": i.to_string(),
                        "status": "TOBEDONE",
                        "content": format!("item {}", i),
                    }))
                }))
            },
        );
        cfg.chunk_size = 100;
        registry.register("test_type", cfg);

        let fetcher = ReportFetcher {
            client: CodeMaoClient::global().clone(),
            registry: Arc::new(registry),
        };
        let items: Vec<Value> = fetcher
            .fetch_chunked(ReportStatus::ToBeDone)
            .flatten()
            .collect();

        assert_eq!(items.len(), TOTAL as usize);

        // 无重复:每个 id 恰好出现一次
        let mut ids: Vec<String> = items
            .iter()
            .filter_map(|v| v.get("id").and_then(|s| s.as_str()).map(String::from))
            .collect();
        ids.sort();
        let unique: HashSet<String> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len());

        // 所有条目都带正确的 _report_type 标注
        assert!(
            items
                .iter()
                .all(|v| { v.get("_report_type").and_then(|s| s.as_str()) == Some("test_type") })
        );
    }
}
