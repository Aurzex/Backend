use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use log::{error as log_error, info as log_info};

use serde_json::Value;

use super::pipeline::{
    BatchActionManager, BatchGroup, CheckConfig, ReportIdExt, ViolationChecker,
    apply_action_by_key, get_display_registry, get_source_type_map, global_action_registry,
};
use super::types::{
    ProcessorError, ReportFetcher, ReportTypeRegistry, SourceConfig, bytes_to_human, html_to_text,
    resolution_display_name, value_to_i64, value_to_string,
};
use crate::api::whale::{ReportStatus, Resolution};
use crate::utils::acquire::{FileUploader, KittyFactory};

/// 批量分组的键:(分组类型, 分组键)
type GroupKey = (String, String);

// 文件处理器
pub struct FileProcessor;

impl FileProcessor {
    /// 上传单个文件,返回上传后的 URL
    pub fn handle_file_upload(
        file_path: &Path,
        save_path: &str,
        method: &str,
        max_size_bytes: u64,
    ) -> Result<String, ProcessorError> {
        let metadata = fs::metadata(file_path)?;
        let file_size = metadata.len();

        if file_size > max_size_bytes {
            let size_mb = file_size as f64 / 1024.0 / 1024.0;
            log_error!(
                "警告: 文件 {} 大小 {:.2} MB 超过 {} MB 限制, 跳过上传",
                file_path.display(),
                size_mb,
                max_size_bytes as f64 / 1024.0 / 1024.0
            );
            return Err(ProcessorError::Processing(format!(
                "文件过大: {} ({} bytes)",
                file_path.display(),
                file_size
            )));
        }

        let client = KittyFactory::global_client().clone();
        let uploader = FileUploader::new(client);
        let url = uploader.upload(file_path, method, save_path)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let size_human = bytes_to_human(file_size);
        log_info!(
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
        max_size_bytes: u64,
    ) -> Result<HashMap<PathBuf, String>, ProcessorError> {
        let mut results = HashMap::new();
        let mut cb = |entry: fs::DirEntry| {
            if entry.file_type().is_ok_and(|ft| ft.is_file()) {
                let path = entry.path();
                match Self::handle_file_upload(path.as_path(), save_path, method, max_size_bytes) {
                    Ok(url) => {
                        results.insert(path.to_path_buf(), url);
                    }
                    Err(e) => {
                        log_error!("上传失败 {}: {}", path.display(), e);
                    }
                }
            }
            Ok(())
        };
        visit_dir(dir_path, &mut cb)?;
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

/// 单条举报的展示与决策信息(供外部 ui 层使用)
pub struct ReportItemView {
    pub record_id: String,
    pub report_type: String,
    pub type_name: String,
    /// 是否官方账号(应自动通过)
    pub is_official: bool,
    /// 详情展示行
    pub details: Vec<String>,
    /// 可用动作 (键, 名称)
    pub actions: Vec<(String, String)>,
    /// 回车默认动作键(通常为 P 通过)
    pub default_action: Option<String>,
}

/// 单次"处理待办"会话:持有分块流与跨 chunk 分组状态
pub struct PendingSession<'a> {
    processor: &'a ReportProcessor,
    chunks: Box<dyn Iterator<Item = Vec<Value>> + 'a>,
    pending_groups: HashMap<GroupKey, Vec<Value>>,
}

impl<'a> PendingSession<'a> {
    /// 拉取下一块,返回(已达标批量组, 非组内项);流结束时返回 None
    pub fn next_chunk(&mut self) -> Option<(Vec<BatchGroup>, Vec<Value>)> {
        let chunk = self.chunks.next()?;
        Some(self.processor.split_chunk(&chunk, &mut self.pending_groups))
    }

    /// 流结束后的遗留组(未达阈值的组,交由调用方决定处理)
    pub fn leftover_groups(&mut self) -> Vec<BatchGroup> {
        self.pending_groups
            .drain()
            .map(|((group_type, group_key), items)| BatchGroup::new(&group_type, &group_key, items))
            .collect()
    }
}

// 主举报处理器(纯引擎:不交互,仅暴露原语操作;流程由外部 ui 层驱动)
pub struct ReportProcessor {
    pub(crate) fetcher: ReportFetcher,
    batch_manager: Arc<Mutex<BatchActionManager>>,
    violation_checker: ViolationChecker,
    config: CheckConfig,
}

impl Default for ReportProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportProcessor {
    /// 使用默认配置构造
    pub fn new() -> Self {
        Self::new_with_config(CheckConfig::default())
    }

    /// 使用自定义配置构造
    pub fn new_with_config(config: CheckConfig) -> Self {
        let fetcher = ReportFetcher::new();
        let batch_manager = Arc::new(Mutex::new(BatchActionManager::new()));
        let violation_checker = ViolationChecker::new(config.clone());
        ReportProcessor {
            fetcher,
            batch_manager,
            violation_checker,
            config,
        }
    }

    // ---- 查询 ----

    /// 待处理举报总数
    pub fn pending_total(&self) -> i64 {
        self.fetcher.get_total_reports(ReportStatus::ToBeDone)
    }

    /// 已处理举报总数
    pub fn done_total(&self) -> i64 {
        self.fetcher.get_total_reports(ReportStatus::Done)
    }

    /// 各举报类型的待处理数量 (类型名, 数量)
    pub fn backlog(&self) -> Vec<(String, i64)> {
        let mut items = Vec::new();
        for report_type in self.fetcher.registry.get_all_types() {
            if let Some(cfg) = self.fetcher.registry.get_config(&report_type) {
                match (cfg.fetch_total)(ReportStatus::ToBeDone) {
                    Ok(v) => items.push((cfg.name.clone(), v.as_i64().unwrap_or(0))),
                    Err(e) => log_error!("获取 {} 总数失败: {}", cfg.name, e),
                }
            }
        }
        items
    }

    /// 举报类型选项 (类型键, 类型名),按名称排序
    pub fn report_type_options(&self) -> Vec<(String, String)> {
        let mut options: Vec<(String, String)> = self
            .fetcher
            .registry
            .get_all_types()
            .into_iter()
            .map(|rt| {
                let name = self
                    .fetcher
                    .registry
                    .get_config(&rt)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| rt.clone());
                (rt, name)
            })
            .collect();
        options.sort_by(|a, b| a.1.cmp(&b.1));
        options
    }

    // ---- 待处理流 ----

    /// 创建"处理待办"会话(分块流 + 跨 chunk 分组状态)
    pub fn pending_session(&self) -> PendingSession<'_> {
        PendingSession {
            processor: self,
            chunks: Box::new(self.fetcher.fetch_reports_chunked(ReportStatus::ToBeDone)),
            pending_groups: HashMap::new(),
        }
    }

    // ---- 单条记录 ----

    /// 生成单条举报的展示与决策信息
    pub fn item_view(&self, item: &Value) -> Option<ReportItemView> {
        let report_type = self.infer_report_type(item)?;
        let config = self.fetcher.registry.get_config(report_type)?;
        let record_id = self.extract_record_id(item, config);
        let is_official = item
            .get(&config.user_id_field)
            .and_then(value_to_i64)
            .is_some_and(|uid| self.config.official_ids.contains(&uid));
        let details = get_display_registry()
            .get(report_type)
            .map(|d| d.display(item, config))
            .unwrap_or_else(|| super::pipeline::generic_details(item, config));
        let options = self.fetcher.registry.action_options(report_type);
        let actions: Vec<(String, String)> = options
            .actions
            .iter()
            .map(|a| (a.key.clone(), a.name.clone()))
            .collect();
        let default_action = options
            .actions
            .iter()
            .find(|a| a.key == "P")
            .map(|a| a.key.clone());
        Some(ReportItemView {
            record_id,
            report_type: report_type.to_string(),
            type_name: config.name.clone(),
            is_official,
            details,
            actions,
            default_action,
        })
    }

    /// 记录的举报类型键
    pub fn item_report_type<'a>(&self, item: &'a Value) -> Option<&'a str> {
        self.infer_report_type(item)
    }

    /// 该类型是否支持违规检查
    pub fn supports_violation_check(&self, item: &Value) -> bool {
        self.infer_report_type(item)
            .and_then(|rt| self.fetcher.registry.get_config(rt))
            .and_then(|config| config.special_check)
            .is_some_and(|check| check(item))
    }

    /// 评论获取数量的默认上限(供 ui 层询问时回退)
    pub fn default_comment_limit(&self) -> usize {
        self.config.comment_fetch_default_limit
    }

    /// 对单条记录应用动作
    pub fn apply_action(
        &self,
        item: &Value,
        action: &str,
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        let Some(report_type) = self.infer_report_type(item) else {
            return Err(ProcessorError::Processing("无法识别举报类型".into()));
        };
        let Some(config) = self.fetcher.registry.get_config(report_type) else {
            return Err(ProcessorError::Processing(format!(
                "未知举报类型: {}",
                report_type
            )));
        };
        if !self
            .fetcher
            .registry
            .is_action_available(report_type, action)
        {
            return Err(ProcessorError::Processing(format!(
                "动作 {} 对该类型不可用",
                action
            )));
        }
        let report_id = config.get_report_id(item)?;
        apply_action_by_key(config, report_id, admin_id, action)
    }

    /// 检查违规,返回违规标识符列表(纯函数,不交互)
    pub fn check_violations(
        &self,
        item: &Value,
        comment_limit: usize,
    ) -> Result<Vec<String>, ProcessorError> {
        let Some(report_type) = self.infer_report_type(item) else {
            return Err(ProcessorError::Processing("无法识别举报类型".into()));
        };
        let Some(config) = self.fetcher.registry.get_config(report_type) else {
            return Err(ProcessorError::Processing(format!(
                "未知举报类型: {}",
                report_type
            )));
        };
        let source_id = item
            .get(&config.source_id_field)
            .and_then(value_to_i64)
            .unwrap_or(0);
        let board_name = config
            .board_name_field
            .as_ref()
            .and_then(|field| item.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let source_type = get_source_type_map()
            .get(report_type)
            .copied()
            .unwrap_or("work");
        let user_id = item.get(&config.user_id_field).and_then(value_to_i64);
        let title = config
            .title_field
            .as_ref()
            .and_then(|field| item.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        self.violation_checker.check_violations(
            source_id,
            source_type,
            board_name,
            user_id,
            title,
            comment_limit,
        )
    }

    /// 用学生账号自动举报违规内容,返回成功数(纯函数,不交互)
    pub fn auto_report(&self, violations: &[String]) -> Result<usize, ProcessorError> {
        self.violation_checker.auto_report(violations)
    }

    /// 一键通过所有待处理举报
    pub fn pass_all(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        log_info!("=== 开始一键通过所有待处理举报 ===");
        let mut count = 0i64;

        for chunk in self.fetcher.fetch_reports_chunked(ReportStatus::ToBeDone) {
            for item in chunk {
                if let Some(report_type) = self.infer_report_type(&item)
                    && let Some(cfg) = self.fetcher.registry.get_config(report_type)
                {
                    let report_id = match cfg.get_report_id(&item) {
                        Ok(id) => id,
                        Err(e) => {
                            log_error!("解析 report_id 失败: {}", e);
                            continue;
                        }
                    };
                    let registry = global_action_registry();
                    let result =
                        registry.apply(&cfg.handle_method, report_id, admin_id, Resolution::Pass);
                    match result {
                        Ok(true) => count += 1,
                        Ok(false) => log_error!("一键通过返回 false (id={})", report_id),
                        Err(e) => log_error!("一键通过失败 (id={}): {}", report_id, e),
                    }
                }
            }
        }

        log_info!("一键通过完成, 共通过 {} 条举报", count);
        Ok(count)
    }

    // ---- 批量组 ----

    /// 批量组已保存的动作
    pub fn group_saved_action(&self, group: &BatchGroup) -> Option<String> {
        self.batch_manager
            .lock()
            .unwrap()
            .get_batch_action(&group.group_type, &group.group_key)
    }

    /// 保存批量组动作(同内容后续遇到时自动应用)
    pub fn save_group_action(&self, group: &BatchGroup, action: &str) {
        self.batch_manager.lock().unwrap().save_batch_action(
            &group.group_type,
            &group.group_key,
            action,
        );
    }

    /// 将动作应用到批量组全部记录,返回成功数
    pub fn apply_group(
        &self,
        group: &BatchGroup,
        action: &str,
        admin_id: i32,
    ) -> Result<i64, ProcessorError> {
        let mut applied = 0i64;
        for item in &group.items {
            let Some(report_type) = self.infer_report_type(item) else {
                continue;
            };
            let Some(config) = self.fetcher.registry.get_config(report_type) else {
                continue;
            };
            let record_id = self.extract_record_id(item, config);
            if !self
                .fetcher
                .registry
                .is_action_available(report_type, action)
            {
                continue;
            }
            match self.apply_action(item, action, admin_id) {
                Ok(()) => {
                    applied += 1;
                    self.batch_manager
                        .lock()
                        .unwrap()
                        .mark_record_processed(&record_id);
                }
                Err(e) => log_error!("批量应用失败 (id={}): {}", record_id, e),
            }
        }
        Ok(applied)
    }

    // ---- 已处理记录 ----

    /// 已处理记录的分块流(惰性)
    pub fn done_chunks(&self) -> Box<dyn Iterator<Item = Vec<Value>> + '_> {
        Box::new(self.fetcher.fetch_reports_chunked(ReportStatus::Done))
    }

    /// 生成已处理记录列表行
    pub fn done_row(&self, index: usize, item: &Value) -> String {
        let Some(report_type) = self.infer_report_type(item) else {
            return format!("{:>3}. [未知类型]", index);
        };
        let Some(config) = self.fetcher.registry.get_config(report_type) else {
            return format!("{:>3}. [{}]", index, report_type);
        };
        let record_id = item
            .get(&config.report_id_field)
            .map(value_to_string)
            .unwrap_or_default();
        let status = item
            .get(&config.status_field)
            .and_then(|v| v.as_str())
            .map(resolution_display_name)
            .unwrap_or_else(|| "未知".into());
        let admin = item
            .get(&config.admin_username_field)
            .map(value_to_string)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                item.get(&config.admin_id_field)
                    .map(value_to_string)
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_default();
        let time = item
            .get(&config.created_at_field)
            .map(format_timestamp)
            .unwrap_or_default();
        let preview = item
            .get(&config.content_field)
            .map(value_to_string)
            .map(|s| html_to_text(&s))
            .map(|s| truncate_chars_local(&s, 24))
            .unwrap_or_default();
        format!(
            "{:>3}. [{}] ID={} 状态={} 管理员={} 时间={} {}",
            index, config.name, record_id, status, admin, time, preview
        )
    }

    /// 已处理记录的详情展示行(处理结果/管理员/时间 + 标准详情)
    pub fn done_item_details(&self, item: &Value) -> Vec<String> {
        let Some(report_type) = self.infer_report_type(item) else {
            return vec!["无法识别举报类型".into()];
        };
        let Some(config) = self.fetcher.registry.get_config(report_type) else {
            return vec![format!("未知举报类型: {}", report_type)];
        };
        let record_id = item
            .get(&config.report_id_field)
            .map(value_to_string)
            .unwrap_or_default();

        let mut lines = vec![format!(
            "\n=== 已处理记录详情: {} (举报ID: {}) ===",
            config.name, record_id
        )];
        if let Some(status) = item.get(&config.status_field).and_then(|v| v.as_str()) {
            lines.push(format!("处理结果: {}", resolution_display_name(status)));
        }
        if let Some(admin) = item
            .get(&config.admin_username_field)
            .map(value_to_string)
            .filter(|s| !s.is_empty())
        {
            lines.push(format!("处理管理员: {}", admin));
        } else if let Some(admin_id) = item
            .get(&config.admin_id_field)
            .map(value_to_string)
            .filter(|s| !s.is_empty())
        {
            lines.push(format!("处理管理员ID: {}", admin_id));
        }
        if let Some(time) = item
            .get(&config.created_at_field)
            .map(format_timestamp)
            .filter(|s| !s.is_empty())
        {
            lines.push(format!("举报时间: {}", time));
        }
        if let Some(time) = item
            .get("updated_at")
            .map(format_timestamp)
            .filter(|s| !s.is_empty())
        {
            lines.push(format!("处理时间: {}", time));
        }
        lines.push("----------------------------------------".into());
        lines.extend(
            get_display_registry()
                .get(report_type)
                .map(|d| d.display(item, config))
                .unwrap_or_else(|| super::pipeline::generic_details(item, config)),
        );
        lines
    }

    // ---- 内部辅助 ----

    /// 将 chunk 划分为"已达标批量组"与"非组内项"。
    /// 组内成员先缓存到跨 chunk 的待分组表,未达标前不单独处理,
    /// 避免组员在组完成前被当作个体处理导致动作不一致。
    fn split_chunk(
        &self,
        chunk: &[Value],
        pending: &mut HashMap<GroupKey, Vec<Value>>,
    ) -> (Vec<BatchGroup>, Vec<Value>) {
        let mut ready = Vec::new();
        let mut non_group = Vec::new();

        for item in chunk {
            let Some(key) = self.extract_group_key(item) else {
                non_group.push(item.clone());
                continue;
            };
            let entry = pending.entry(key.clone()).or_default();
            entry.push(item.clone());
            let threshold = if key.0 == "item_id" {
                self.config.batch_item_id_threshold
            } else {
                self.config.batch_content_threshold
            };
            if entry.len() >= threshold {
                let items = pending.remove(&key).unwrap();
                ready.push(BatchGroup::new(&key.0, &key.1, items));
            }
        }
        (ready, non_group)
    }

    fn extract_group_key(&self, item: &Value) -> Option<GroupKey> {
        let rt = self.infer_report_type(item)?;
        let config = self.fetcher.registry.get_config(rt)?;
        let record_id = item.get(&config.report_id_field).map(value_to_string)?;
        if record_id.is_empty() {
            return None;
        }
        let item_id = item
            .get(&config.item_id_field)
            .map(value_to_string)
            .unwrap_or_default();
        if !item_id.is_empty() {
            // 分组键带上类型:不同举报类型(评论/作品/帖子)的 id 各自独立编号,可能撞号
            Some(("item_id".into(), format!("{}:{}", rt, item_id)))
        } else {
            let content_key = format!(
                "{}:{}:{}",
                item.get(&config.content_field)
                    .map(value_to_string)
                    .unwrap_or_default(),
                rt,
                item.get(&config.source_id_field)
                    .map(value_to_string)
                    .unwrap_or_default()
            );
            Some(("content".into(), content_key))
        }
    }

    /// 从举报记录中提取 report_id 字符串
    fn extract_record_id(&self, item: &Value, config: &SourceConfig) -> String {
        item.get(&config.report_id_field)
            .map(value_to_string)
            .unwrap_or_else(|| "0".to_string())
    }

    /// 推断举报类型,返回字符串引用以减少分配
    fn infer_report_type<'a>(&self, item: &'a Value) -> Option<&'a str> {
        if let Some(t) = item.get("_report_type").and_then(|v| v.as_str()) {
            return Some(t);
        }
        if item.get("comment_content").is_some() || item.get("comment_id").is_some() {
            Some("shop_comment")
        } else if item.get("work_name").is_some() {
            Some("work_work")
        } else if item.get("discussion_content").is_some() || item.get("discussion_id").is_some() {
            Some("forum_discussion")
        } else if item.get("post_title").is_some() && item.get("board_name").is_some() {
            Some("forum_post")
        } else {
            None
        }
    }
}

/// 截断字符串到指定字符数(按字符而非字节,避免切断多字节字符)
fn truncate_chars_local(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_owned()
    } else {
        s.chars().take(max_chars).collect::<String>() + "..."
    }
}

/// 将 Unix 时间戳(秒或毫秒,按数值自动识别)格式化为 UTC "YYYY-MM-DD HH:MM"
fn format_timestamp(v: &Value) -> String {
    let Some(n) = v.as_i64() else {
        return String::new();
    };
    let secs = if n > 100_000_000_000 { n / 1000 } else { n };
    if secs <= 0 {
        return String::new();
    }
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    // Howard Hinnant civil_from_days 算法:天数换算公历年月日
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        year, month, day, hour, minute
    )
}
