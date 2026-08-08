use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, OnceLock};

use crate::api::whale::{CommentSourceType, ReportStatus, WhaleReportFetcher, WorkSourceType};
use crate::utils::acquire;

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
    Mew(#[from] acquire::MewError),
}

// 评论配置 trait
pub trait CommentConfig {
    fn get_comments(&self, item_id: i64) -> Option<&[Value]>;
}

// 交互工具
pub(crate) fn prompt_input(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

pub(crate) fn get_valid_input(prompt: &str, valid_options: &HashSet<String>) -> String {
    loop {
        let input = prompt_input(prompt);
        if valid_options.contains(&input.to_uppercase()) {
            return input.to_uppercase();
        }
        println!("无效输入,请重试");
    }
}

// 辅助函数
pub(crate) fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn value_to_i64(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
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
    pub(crate) status: String,
    pub(crate) enabled: bool,
}

impl ActionConfig {
    /// 由动作键构造配置(名称与状态取自内置映射)
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

/// 按给定动作键列表批量构造动作配置
fn actions(keys: &[&str]) -> Vec<ActionConfig> {
    keys.iter().map(|k| ActionConfig::simple(k)).collect()
}

pub(crate) type FetchGenerator =
    fn(ReportStatus) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>>>;
pub(crate) type FetchTotal = fn(ReportStatus) -> Result<Value, ProcessorError>;

#[derive(Clone, Debug)]
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

// 举报类型注册表
/// 按举报类型预计算的动作提示与合法键集合(注册表运行期不变)
pub(crate) struct ActionOptions {
    pub(crate) prompt: String,
    pub(crate) valid_keys: HashSet<String>,
}

pub(crate) struct ReportTypeRegistry {
    /// Arc 包装:每条举报记录的处理都需持有配置,避免逐记录深克隆整个 SourceConfig(~30 个 String)
    registry: HashMap<String, Arc<SourceConfig>>,
    default_actions: Vec<ActionConfig>, // 保留用于构建默认动作,也可直接为静态
    action_cache: Mutex<HashMap<String, Arc<ActionOptions>>>,
}

impl Clone for ReportTypeRegistry {
    /// 深拷贝注册表但清空动作缓存(缓存按需重建,无正确性影响)
    fn clone(&self) -> Self {
        ReportTypeRegistry {
            registry: self.registry.clone(),
            default_actions: self.default_actions.clone(),
            action_cache: Mutex::new(HashMap::new()),
        }
    }
}

// 静态状态映射(避免每次构建)
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
        self.registry.get(report_type).map(|a| a.as_ref())
    }

    /// 返回配置的 Arc 句柄,供处理上下文持有(零拷贝引用计数)
    pub(crate) fn get_config_arc(&self, report_type: &str) -> Option<Arc<SourceConfig>> {
        self.registry.get(report_type).cloned()
    }

    pub(crate) fn get_all_types(&self) -> Vec<String> {
        self.registry.keys().cloned().collect()
    }

    /// 返回可用动作的引用,避免克隆整个 ActionConfig
    pub(crate) fn get_available_actions(&self, report_type: &str) -> Vec<&ActionConfig> {
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

    /// 获取(或按举报类型缓存)动作提示与合法键集合,避免交互处理时每记录重建
    pub(crate) fn action_options(&self, report_type: &str) -> Arc<ActionOptions> {
        let mut cache = self.action_cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cached) = cache.get(report_type) {
            return Arc::clone(cached);
        }
        let actions = self.get_available_actions(report_type);
        let parts: Vec<String> = actions
            .iter()
            .map(|a| format!("{}({})", a.key, a.name))
            .collect();
        let options = Arc::new(ActionOptions {
            prompt: format!("选择操作:{}", parts.join(",")),
            valid_keys: actions.iter().map(|a| a.key.clone()).collect(),
        });
        cache.insert(report_type.to_string(), Arc::clone(&options));
        options
    }

    pub(crate) fn get_action_prompt(&self, report_type: &str) -> String {
        self.action_options(report_type).prompt.clone()
    }

    /// 返回全局静态的状态映射引用
    pub(crate) fn get_status_mapping(&self) -> &'static HashMap<&'static str, &'static str> {
        status_mapping()
    }

    pub(crate) fn is_action_available(&self, report_type: &str, action_key: &str) -> bool {
        self.action_options(report_type)
            .valid_keys
            .contains(action_key)
    }
}

// 举报获取器

/// 注册辅助:包装分页迭代器为"总数"闭包
fn total_from(mut paginated: acquire::PaginatedIter) -> Result<Value, ProcessorError> {
    paginated.fetch_metadata()?;
    Ok(json!(paginated.total_items().unwrap_or(0) as i32))
}

/// 注册辅助:包装分页迭代器为"生成器"闭包
fn gen_from(
    paginated: acquire::PaginatedIter,
) -> Box<dyn Iterator<Item = Result<Value, ProcessorError>>> {
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

pub(crate) struct ReportFetcher {
    pub(crate) registry: ReportTypeRegistry,
}

impl Default for ReportFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportFetcher {
    pub(crate) fn new() -> Self {
        let mut registry = ReportTypeRegistry::new();

        // shop_comment
        {
            let mut cfg = SourceConfig::base(
                "工作室评论举报",
                "execute_process_comment_report",
                |status| {
                    total_from(WhaleReportFetcher::new().fetch_comment_reports_gen(
                        CommentSourceType::All,
                        status,
                        None,
                        None,
                        None,
                    ))
                },
                |status| {
                    gen_from(WhaleReportFetcher::new().fetch_comment_reports_gen(
                        CommentSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    ))
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
                    .map(|s| s == "WORK_SHOP")
                    .unwrap_or(false)
            });
            registry.register("shop_comment", cfg);
        }

        // work_work
        {
            let mut cfg = SourceConfig::base(
                "作品举报",
                "execute_process_work_report",
                |status| {
                    total_from(WhaleReportFetcher::new().fetch_work_reports_gen(
                        WorkSourceType::All,
                        status,
                        None,
                        None,
                        None,
                    ))
                },
                |status| {
                    gen_from(WhaleReportFetcher::new().fetch_work_reports_gen(
                        WorkSourceType::All,
                        status,
                        None,
                        None,
                        Some(100),
                    ))
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
                |status| {
                    total_from(
                        WhaleReportFetcher::new()
                            .fetch_post_reports_gen(status, None, None, None, None),
                    )
                },
                |status| {
                    gen_from(WhaleReportFetcher::new().fetch_post_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    ))
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
            registry.register("forum_post", cfg);
        }

        // forum_discussion
        {
            let mut cfg = SourceConfig::base(
                "讨论举报",
                "execute_process_discussion_report",
                |status| {
                    total_from(
                        WhaleReportFetcher::new()
                            .fetch_discussion_reports_gen(status, None, None, None, None),
                    )
                },
                |status| {
                    gen_from(WhaleReportFetcher::new().fetch_discussion_reports_gen(
                        status,
                        None,
                        None,
                        None,
                        Some(100),
                    ))
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
            registry.register("forum_discussion", cfg);
        }

        ReportFetcher { registry }
    }

    pub(crate) fn fetch_chunked(&self, status: ReportStatus) -> impl Iterator<Item = Vec<Value>> {
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
                        && state != ReportStatus::ToBeDone.as_str()
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

    pub(crate) fn fetch_reports_chunked(
        &self,
        status: ReportStatus,
    ) -> impl Iterator<Item = Vec<Value>> {
        self.fetch_chunked(status)
    }

    pub(crate) fn get_total_reports(&self, status: ReportStatus) -> i64 {
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
