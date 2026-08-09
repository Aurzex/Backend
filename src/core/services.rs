use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::pipeline::{
    BatchActionManager, BatchGroup, CheckConfig, DetailDisplayProcessor, ProcessingContext,
    ProcessingPipeline, ProcessingState, Processor, ReportIdExt, ReportRecord,
    apply_action_by_method, global_action_registry,
};
use super::types::{
    ProcessorError, ReportFetcher, ReportTypeRegistry, SourceConfig, action_name, bytes_to_human,
    get_valid_input, html_to_text, prompt_input, resolution_display_name, value_to_string,
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
            println!(
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
                        eprintln!("上传失败 {}: {}", path.display(), e);
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

// 主举报处理器
pub struct ReportProcessor {
    pub(crate) fetcher: ReportFetcher,
    pub(crate) pipeline_factory: Arc<ReportTypeRegistry>,
    pub(crate) batch_manager: Arc<Mutex<BatchActionManager>>,
    pending_groups: Mutex<HashMap<GroupKey, Vec<Value>>>,
    config: CheckConfig, // 注入配置,包含批量识别阈值等
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
        // fetcher.registry 已是 Arc,此处仅共享引用计数,避免深拷贝整个注册表
        let registry = fetcher.registry.clone();
        let batch_manager = Arc::new(Mutex::new(BatchActionManager::new()));
        ReportProcessor {
            fetcher,
            pipeline_factory: registry,
            batch_manager,
            pending_groups: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// 交互式举报处理控制台:处理待办,浏览已处理记录,查看分布
    pub fn run_interactive(&self, admin_id: i32) -> Result<(), ProcessorError> {
        loop {
            let todo = self.fetcher.get_total_reports(ReportStatus::ToBeDone);
            let done = self.fetcher.get_total_reports(ReportStatus::Done);
            println!("\n=== 举报处理控制台 ===");
            println!("待处理: {} 条 | 已处理: {} 条", todo, done);
            println!("1. 处理待处理举报");
            println!("2. 查看已处理记录");
            println!("3. 待处理分布(按类型)");
            println!("0. 退出");
            let input = prompt_input("> ");
            match input.trim() {
                "1" => {
                    let processed = self.process_all_reports(admin_id)?;
                    println!("本次共处理 {} 条举报", processed);
                }
                "2" => self.view_processed_reports()?,
                "3" => self.show_backlog()?,
                "0" | "q" | "Q" | "" => {
                    println!("退出举报处理控制台");
                    return Ok(());
                }
                _ => println!("无效输入,请重试"),
            }
        }
    }

    /// 展示各举报类型的待处理数量
    fn show_backlog(&self) -> Result<(), ProcessorError> {
        println!("=== 待处理举报分布 ===");
        let mut total = 0i64;
        for report_type in self.fetcher.registry.get_all_types() {
            if let Some(cfg) = self.fetcher.registry.get_config(&report_type) {
                match (cfg.fetch_total)(ReportStatus::ToBeDone) {
                    Ok(v) => {
                        let n = v.as_i64().unwrap_or(0);
                        total += n;
                        println!("  {}: {}", cfg.name, n);
                    }
                    Err(e) => println!("  {}: 获取失败 ({})", cfg.name, e),
                }
            }
        }
        println!("  合计: {}", total);
        Ok(())
    }

    pub fn process_all_reports(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        println!("=== 开始处理所有举报 ===");
        self.reset_batch_state();

        let total = self.fetcher.get_total_reports(ReportStatus::ToBeDone);
        println!("当前待处理举报总数: {}", total);
        if total == 0 {
            println!("没有待处理的举报");
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

        for chunk in self.fetcher.fetch_reports_chunked(ReportStatus::ToBeDone) {
            println!(
                "--- 本块 {} 条, 进度 {}/{} ---",
                chunk.len(),
                processed + 1,
                total
            );
            match self.process_chunk(&chunk, admin_id, &mut processed, total) {
                Err(ProcessorError::Aborted) => {
                    println!("已中止处理, 累计处理 {} 条", processed);
                    return Ok(processed);
                }
                Err(e) => return Err(e),
                Ok(()) => {}
            }
        }

        // 流结束:处理未达阈值的遗留组,不再留到下一周期
        let leftover = match self.process_leftover_groups(admin_id) {
            Err(ProcessorError::Aborted) => {
                println!("已中止处理遗留组, 累计处理 {} 条", processed);
                return Ok(processed);
            }
            Err(e) => return Err(e),
            Ok(n) => n,
        };
        processed += leftover;

        println!("所有举报处理完成, 共处理 {} 条举报", processed);
        Ok(processed)
    }

    /// 处理单个 chunk:先处理达标批量组,再单独处理非组内项
    fn process_chunk(
        &self,
        chunk: &[Value],
        admin_id: i32,
        processed: &mut i64,
        total: i64,
    ) -> Result<(), ProcessorError> {
        let (ready_groups, non_group) = self.split_chunk_items(chunk);

        for group in ready_groups {
            let n = self.handle_single_batch_group(&group, admin_id)?;
            *processed += n;
        }

        for item in &non_group {
            let done = match self.process_single_item(item, admin_id, *processed + 1, total) {
                Err(ProcessorError::Aborted) => return Err(ProcessorError::Aborted),
                Err(e) => {
                    eprintln!("处理记录失败: {}, 跳过", e);
                    false
                }
                Ok(done) => done,
            };
            if done {
                *processed += 1;
            }
        }
        Ok(())
    }

    /// 将 chunk 划分为"已达标批量组"与"非组内项"。
    /// 组内成员先缓存到跨 chunk 的待分组表,未达标前不单独处理,
    /// 避免组员在组完成前被当作个体处理导致动作不一致。
    fn split_chunk_items<'a>(&self, chunk: &'a [Value]) -> (Vec<BatchGroup>, Vec<&'a Value>) {
        let mut pending = self.pending_groups.lock().unwrap();
        let mut ready = Vec::new();
        let mut non_group = Vec::new();

        for item in chunk {
            let Some(key) = self.extract_group_key(item) else {
                // 非组内项仅借用 chunk,后续处理时再克隆进上下文
                non_group.push(item);
                continue;
            };
            let entry = pending.entry(key.clone()).or_default();
            // 组内成员需跨 chunk 保留,必须克隆
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
            Some(("item_id".into(), item_id))
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

    /// 处理一个批量组:有保存动作则直接应用,否则询问首条后应用到其余
    fn handle_single_batch_group(
        &self,
        group: &BatchGroup,
        admin_id: i32,
    ) -> Result<i64, ProcessorError> {
        println!(
            "处理批量组 [{}] {} (共 {} 条举报)",
            group.group_type,
            group.group_key,
            group.items.len()
        );

        let saved_action = self
            .batch_manager
            .lock()
            .unwrap()
            .get_batch_action(&group.group_type, &group.group_key);

        if let Some(action) = saved_action {
            println!("应用保存的批量动作: {}", action_name(&action));
            return self.apply_action_to_items(&group.items, &action, admin_id);
        }

        let Some(action) = self.ask_first_record_action(group, admin_id)? else {
            return Ok(0);
        };
        println!(
            "批量组动作: {}, 应用到剩余 {} 条记录",
            action_name(&action),
            group.items.len().saturating_sub(1)
        );
        let rest = self.apply_action_to_items(&group.items[1..], &action, admin_id)?;
        Ok(rest + 1)
    }

    /// 无保存动作时,通过管道交互询问组内第一条记录的处理动作,并保存该动作
    fn ask_first_record_action(
        &self,
        group: &BatchGroup,
        admin_id: i32,
    ) -> Result<Option<String>, ProcessorError> {
        let Some(first_item) = group.items.first() else {
            return Ok(None);
        };
        let Some(report_type) = self.infer_report_type(first_item) else {
            return Ok(None);
        };
        let config = self.fetcher.registry.get_config_arc(report_type);
        let Some(config_ref) = config.as_ref() else {
            return Ok(None);
        };
        let record_id = self.extract_record_id(first_item, config_ref);

        println!("--- 批量组首条记录 (举报ID: {}) ---", record_id);
        let mut context = ProcessingContext::new(
            record_id,
            report_type.to_string(),
            first_item.clone(),
            admin_id,
        );
        context.record.is_batch_mode = false;
        context.record.config = config;

        let pipeline = self.create_pipeline();
        pipeline.execute(&mut context)?;

        let action = context.state.action.clone();
        if let Some(action) = &action {
            self.batch_manager.lock().unwrap().save_batch_action(
                &group.group_type,
                &group.group_key,
                action,
            );
        }
        Ok(action)
    }

    /// 将批量动作应用到一组记录:仅对动作可用的记录执行,成功才标记已处理,失败仅记录日志
    fn apply_action_to_items(
        &self,
        items: &[Value],
        action: &str,
        admin_id: i32,
    ) -> Result<i64, ProcessorError> {
        let mut applied = 0i64;
        for item in items {
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
            match self.apply_simple_action(item, report_type, action, admin_id) {
                Ok(()) => {
                    applied += 1;
                    self.batch_manager
                        .lock()
                        .unwrap()
                        .mark_record_processed(&record_id);
                }
                Err(e) => {
                    eprintln!("批量应用失败 (id={}): {}", record_id, e);
                }
            }
        }
        Ok(applied)
    }

    /// 处理单条非组内举报,返回是否已处理
    fn process_single_item(
        &self,
        item: &Value,
        admin_id: i32,
        index: i64,
        total: i64,
    ) -> Result<bool, ProcessorError> {
        let Some(report_type) = self.infer_report_type(item) else {
            return Ok(false);
        };
        let Some(config) = self.fetcher.registry.get_config(report_type) else {
            return Ok(false);
        };
        let record_id = self.extract_record_id(item, config);

        if self.is_record_processed(&record_id) {
            return Ok(false);
        }

        println!(
            "--- [{}/{}] {} (举报ID: {}) ---",
            index, total, config.name, record_id
        );

        let mut context = ProcessingContext::new(
            record_id.clone(),
            report_type.to_string(),
            item.clone(),
            admin_id,
        );
        context.record.config = self.fetcher.registry.get_config_arc(report_type);

        let pipeline = self.create_pipeline();
        if let Err(e) = pipeline.execute(&mut context) {
            if matches!(e, ProcessorError::Aborted) {
                return Err(e);
            }
            eprintln!("处理记录 {} 失败: {}, 跳过", record_id, e);
            return Ok(false);
        }

        if context.state.processed {
            self.batch_manager
                .lock()
                .unwrap()
                .mark_record_processed(&record_id);
            if let Some(action) = &context.state.action {
                println!("  => 已处理: {}", action_name(action));
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// 流结束:处理所有未达阈值的遗留组
    fn process_leftover_groups(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        let remaining = self
            .pending_groups
            .lock()
            .unwrap()
            .drain()
            .collect::<Vec<_>>();
        let mut processed = 0i64;
        for ((group_type, group_key), items) in remaining {
            let group = BatchGroup::new(&group_type, &group_key, items);
            processed += self.handle_single_batch_group(&group, admin_id)?;
        }
        Ok(processed)
    }

    /// 分页浏览已处理记录,支持按举报类型过滤切换(如仅看工作室评论举报)
    /// 数据按需分块拉取:仅当翻页到未加载区域时才请求下一块,避免一次性拉全量造成卡顿
    pub fn view_processed_reports(&self) -> Result<(), ProcessorError> {
        println!("=== 已处理记录 ===");
        let mut type_options: Vec<(String, String)> = self
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
        type_options.sort_by(|a, b| a.1.cmp(&b.1));

        let mut current_type: Option<String> = None; // None = 全部类型
        let mut chunks = self.fetcher.fetch_reports_chunked(ReportStatus::Done);
        let mut raw_items: Vec<Value> = Vec::new();
        let mut visible_count = 0usize; // 已缓存原始数据中匹配当前过滤的数量
        let mut exhausted = false;

        const PAGE_SIZE: usize = 15;
        let mut page = 0usize;

        loop {
            // 按需拉取:确保当前页所需可见数据已加载
            let needed = (page + 1) * PAGE_SIZE;
            while visible_count < needed && !exhausted {
                match chunks.next() {
                    Some(chunk) => {
                        visible_count += chunk
                            .iter()
                            .filter(|v| self.type_filter_matches(v, &current_type))
                            .count();
                        raw_items.extend(chunk);
                    }
                    None => exhausted = true,
                }
            }

            let filter_name = current_type
                .as_ref()
                .and_then(|t| {
                    type_options
                        .iter()
                        .find(|(rt, _)| rt == t)
                        .map(|(_, name)| name.as_str())
                })
                .unwrap_or("全部");
            let page_count = visible_count.div_ceil(PAGE_SIZE).max(1);
            println!(
                "\n=== 已处理记录 (类型: {}, 第 {}/{} 页, 共 {} 条{}) ===",
                filter_name,
                page + 1,
                page_count,
                visible_count,
                if exhausted {
                    ""
                } else {
                    ", 按需加载更多"
                }
            );

            if visible_count > 0 {
                let start = page * PAGE_SIZE;
                for (i, item) in raw_items
                    .iter()
                    .filter(|v| self.type_filter_matches(v, &current_type))
                    .skip(start)
                    .take(PAGE_SIZE)
                    .enumerate()
                {
                    println!("{}", self.format_done_row(start + i + 1, item));
                }
            } else {
                println!("(该类型暂无已处理记录)");
            }

            println!("[序号] 查看详情 | n 下一页 | p 上一页 | t 切换类型 | q 返回");
            let input = prompt_input("> ");
            let trimmed = input.trim();
            if let Ok(idx) = trimmed.parse::<usize>() {
                if visible_count == 0 {
                    println!("当前无记录");
                } else if idx >= 1 && idx <= visible_count {
                    if let Some(item) = raw_items
                        .iter()
                        .filter(|v| self.type_filter_matches(v, &current_type))
                        .nth(idx - 1)
                    {
                        self.display_done_item(item)?;
                    }
                } else {
                    println!("序号超出范围");
                }
            } else {
                match trimmed.to_lowercase().as_str() {
                    "n" => {
                        if exhausted && page + 1 >= page_count {
                            println!("已经是最后一页");
                        } else {
                            page += 1;
                        }
                    }
                    "p" => {
                        if page > 0 {
                            page -= 1;
                        } else {
                            println!("已经是第一页");
                        }
                    }
                    "t" => {
                        if let Some(new_type) = self.pick_report_type(&type_options, &current_type)
                        {
                            current_type = new_type;
                            visible_count = raw_items
                                .iter()
                                .filter(|v| self.type_filter_matches(v, &current_type))
                                .count();
                            page = 0;
                        }
                    }
                    "q" | "" => break,
                    _ => println!("无效输入,请重试"),
                }
            }
        }
        Ok(())
    }

    /// 判断记录是否匹配当前类型过滤(None 表示不过滤)
    fn type_filter_matches(&self, item: &Value, current: &Option<String>) -> bool {
        match current {
            Some(t) => self.infer_report_type(item) == Some(t.as_str()),
            None => true,
        }
    }

    /// 交互选择要查看的举报类型,返回 None 表示取消,Some(None) 表示全部
    fn pick_report_type(
        &self,
        type_options: &[(String, String)],
        current: &Option<String>,
    ) -> Option<Option<String>> {
        println!("\n选择要查看的举报类型:");
        println!("  0. 全部");
        for (i, (rt, name)) in type_options.iter().enumerate() {
            let mark = if current.as_ref() == Some(rt) {
                " (当前)"
            } else {
                ""
            };
            println!("  {}. {}{}", i + 1, name, mark);
        }
        println!("  回车返回");
        let input = prompt_input("> ");
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed == "0" {
            return Some(None);
        }
        if let Ok(n) = trimmed.parse::<usize>() {
            if n >= 1 && n <= type_options.len() {
                return Some(Some(type_options[n - 1].0.clone()));
            }
        }
        println!("无效输入");
        None
    }

    /// 生成已处理记录列表行
    fn format_done_row(&self, index: usize, item: &Value) -> String {
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

    /// 展示单条已处理记录的完整详情
    fn display_done_item(&self, item: &Value) -> Result<(), ProcessorError> {
        let Some(report_type) = self.infer_report_type(item) else {
            println!("无法识别举报类型");
            return Ok(());
        };
        let Some(config) = self.fetcher.registry.get_config(report_type) else {
            println!("未知举报类型: {}", report_type);
            return Ok(());
        };
        let record_id = item
            .get(&config.report_id_field)
            .map(value_to_string)
            .unwrap_or_default();

        println!(
            "\n=== 已处理记录详情: {} (举报ID: {}) ===",
            config.name, record_id
        );
        if let Some(status) = item.get(&config.status_field).and_then(|v| v.as_str()) {
            println!("处理结果: {}", resolution_display_name(status));
        }
        if let Some(admin) = item
            .get(&config.admin_username_field)
            .map(value_to_string)
            .filter(|s| !s.is_empty())
        {
            println!("处理管理员: {}", admin);
        } else if let Some(admin_id) = item
            .get(&config.admin_id_field)
            .map(value_to_string)
            .filter(|s| !s.is_empty())
        {
            println!("处理管理员ID: {}", admin_id);
        }
        if let Some(time) = item
            .get(&config.created_at_field)
            .map(format_timestamp)
            .filter(|s| !s.is_empty())
        {
            println!("举报时间: {}", time);
        }
        if let Some(time) = item
            .get("updated_at")
            .map(format_timestamp)
            .filter(|s| !s.is_empty())
        {
            println!("处理时间: {}", time);
        }
        println!("----------------------------------------");

        let record = ReportRecord {
            record_id,
            report_type: report_type.to_string(),
            item: item.clone(),
            admin_id: 0,
            is_batch_mode: false,
            is_reprocess_mode: false,
            config: self.fetcher.registry.get_config_arc(report_type),
            user_id: None,
        };
        let mut state = ProcessingState::default();
        DetailDisplayProcessor.process(&record, &mut state)?;
        Ok(())
    }

    /// 创建默认处理管道,复用注入的注册表,批量管理器与配置
    fn create_pipeline(&self) -> ProcessingPipeline {
        ProcessingPipeline::create_default(
            self.pipeline_factory.clone(),
            self.batch_manager.clone(),
            self.config.clone(),
        )
    }

    /// 重置批量处理状态,清空已处理记录与跨 chunk 待分组
    fn reset_batch_state(&self) {
        self.batch_manager.lock().unwrap().clear_processed_records();
        self.pending_groups.lock().unwrap().clear();
    }

    /// 从举报记录中提取 report_id 字符串
    fn extract_record_id(&self, item: &Value, config: &SourceConfig) -> String {
        item.get(&config.report_id_field)
            .map(value_to_string)
            .unwrap_or_else(|| "0".to_string())
    }

    fn is_record_processed(&self, record_id: &str) -> bool {
        self.batch_manager
            .lock()
            .unwrap()
            .is_record_processed(record_id)
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
                let report_id = config.get_report_id(item)?;
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
                if let Some(report_type) = self.infer_report_type(&item)
                    && let Some(cfg) = self.fetcher.registry.get_config(report_type)
                {
                    let report_id = match cfg.get_report_id(&item) {
                        Ok(id) => id,
                        Err(e) => {
                            eprintln!("解析 report_id 失败: {}", e);
                            continue;
                        }
                    };
                    // 使用全局动作注册表代替字符串匹配
                    let registry = global_action_registry();
                    let result =
                        registry.apply(&cfg.handle_method, report_id, admin_id, Resolution::Pass);
                    match result {
                        Ok(true) => count += 1,
                        Ok(false) => eprintln!("一键通过返回 false (id={})", report_id),
                        Err(e) => eprintln!("一键通过失败 (id={}): {}", report_id, e),
                    }
                }
            }
        }

        println!("一键通过完成, 共通过 {} 条举报", count);
        Ok(count)
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
