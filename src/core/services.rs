use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::pipeline::{
    BatchActionManager, BatchGroup, CheckConfig, ProcessingContext, ProcessingPipeline,
    ReportIdExt, apply_action_by_method, global_action_registry,
};
use super::types::{
    ProcessorError, ReportFetcher, ReportTypeRegistry, SourceConfig, bytes_to_human,
    get_valid_input, value_to_string,
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
    pub fetcher: ReportFetcher,
    pub pipeline_factory: Arc<ReportTypeRegistry>,
    pub batch_manager: Arc<Mutex<BatchActionManager>>,
    pending_groups: Mutex<HashMap<GroupKey, Vec<String>>>,
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
        let registry = Arc::new(fetcher.registry.clone());
        let batch_manager = Arc::new(Mutex::new(BatchActionManager::new()));
        ReportProcessor {
            fetcher,
            pipeline_factory: registry,
            batch_manager,
            pending_groups: Mutex::new(HashMap::new()),
            config,
        }
    }

    pub fn process_all_reports(&self, admin_id: i32) -> Result<i64, ProcessorError> {
        println!("=== 开始处理所有举报 ===");
        self.reset_batch_state();

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

            // 跨 chunk 批量组识别与处理
            self.update_and_handle_pending_groups(&chunk, admin_id)?;

            // 单独处理未被批量组包含的项
            let processed_in_chunk = self.process_non_group_items(&chunk, admin_id)?;
            total_processed += processed_in_chunk;

            println!(
                "第 {} 块处理完成, 累计处理 {} 条举报",
                chunk_count + 1,
                total_processed
            );
        }

        // 流结束:处理所有剩余未达阈值的组(记录警告,将在下一个周期处理)
        let remaining = self
            .pending_groups
            .lock()
            .unwrap()
            .drain()
            .collect::<Vec<_>>();
        for ((group_type, group_key), record_ids) in remaining {
            eprintln!(
                "警告: 跨 chunk 组 ({}, {}) 未达到处理阈值,包含 {} 个记录,将在下一个周期处理",
                group_type,
                group_key,
                record_ids.len()
            );
        }

        println!("所有举报处理完成, 共处理 {} 条举报", total_processed);
        Ok(total_processed)
    }

    /// 分块更新并处理已达阈值的组
    fn update_and_handle_pending_groups(
        &self,
        chunk: &[Value],
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        let mut pending = self.pending_groups.lock().unwrap();
        let mut ready_groups = Vec::new();

        for item in chunk {
            if let Some((key, record_id)) = self.extract_group_key(item) {
                let entry = pending.entry(key.clone()).or_default();
                entry.push(record_id);
                // 使用配置中的阈值
                let threshold = if key.0 == "item_id" {
                    self.config.batch_item_id_threshold
                } else {
                    self.config.batch_content_threshold
                };
                if entry.len() >= threshold {
                    let ids = pending.remove(&key).unwrap();
                    ready_groups.push(BatchGroup::new(&key.0, &key.1, ids));
                }
            }
        }
        drop(pending); // 释放锁

        // 处理已达阈值的组
        for group in ready_groups {
            self.handle_single_batch_group(&group, chunk, admin_id)?;
        }
        Ok(())
    }

    fn extract_group_key(&self, item: &Value) -> Option<(GroupKey, String)> {
        let rt = self.infer_report_type(item)?;
        let config = self.fetcher.registry.get_config(rt)?;
        let record_id = item.get(&config.report_id_field).map(value_to_string)?;
        let item_id = item
            .get(&config.item_id_field)
            .map(value_to_string)
            .unwrap_or_default();
        if !item_id.is_empty() {
            Some((("item_id".into(), item_id), record_id))
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
            Some((("content".into(), content_key), record_id))
        }
    }

    fn handle_single_batch_group(
        &self,
        group: &BatchGroup,
        chunk: &[Value],
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        println!(
            "处理批量组 [{}] {} (共 {} 条举报)",
            group.group_type,
            group.group_key,
            group.record_ids.len()
        );

        let saved_action = self
            .batch_manager
            .lock()
            .unwrap()
            .get_batch_action(&group.group_type, &group.group_key);

        if let Some(action) = saved_action {
            // 已有保存的批量动作:应用到组内全部记录
            println!("应用保存的批量动作: {}", action);
            self.apply_action_to_records(chunk, &group.record_ids, &action, admin_id)?;
        } else if let Some(action) = self.ask_first_record_action(group, chunk, admin_id)? {
            // 无保存动作:询问第一条后,将动作应用到剩余记录
            self.apply_action_to_records(chunk, &group.record_ids[1..], &action, admin_id)?;
            // 第一条已由管道处理,标记避免在 process_non_group_items 中被重复处理
            if let Some(first_record_id) = group.record_ids.first() {
                self.batch_manager
                    .lock()
                    .unwrap()
                    .mark_record_processed(first_record_id);
            }
        }
        Ok(())
    }

    /// 无保存动作时,通过管道交互询问组内第一条记录的处理动作,并保存该动作
    fn ask_first_record_action(
        &self,
        group: &BatchGroup,
        chunk: &[Value],
        admin_id: i32,
    ) -> Result<Option<String>, ProcessorError> {
        let Some(first_record_id) = group.record_ids.first() else {
            return Ok(None);
        };
        let Some(first_item) = chunk
            .iter()
            .find(|v| self.record_id_matches(v, first_record_id))
        else {
            return Ok(None);
        };
        let Some(report_type) = self.infer_report_type(first_item) else {
            return Ok(None);
        };

        let config = self.fetcher.registry.get_config(report_type).cloned();
        let mut context = ProcessingContext::new(
            first_record_id.to_string(),
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

    /// 将批量动作应用到一批记录:仅对动作可用的记录执行,成功才标记已处理,失败仅记录日志
    fn apply_action_to_records(
        &self,
        chunk: &[Value],
        record_ids: &[String],
        action: &str,
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        for record_id in record_ids {
            let Some(item) = chunk.iter().find(|v| self.record_id_matches(v, record_id)) else {
                continue;
            };
            let Some(report_type) = self.infer_report_type(item) else {
                continue;
            };
            if !self
                .fetcher
                .registry
                .is_action_available(report_type, action)
            {
                continue;
            }
            match self.apply_simple_action(item, report_type, action, admin_id) {
                Ok(()) => {
                    self.batch_manager
                        .lock()
                        .unwrap()
                        .mark_record_processed(record_id);
                }
                Err(e) => {
                    eprintln!("批量应用失败 (id={}): {}", record_id, e);
                }
            }
        }
        Ok(())
    }

    fn process_non_group_items(
        &self,
        chunk: &[Value],
        admin_id: i32,
    ) -> Result<i64, ProcessorError> {
        let mut processed = 0i64;

        for item in chunk {
            let Some(report_type) = self.infer_report_type(item) else {
                continue;
            };
            let Some(config) = self.fetcher.registry.get_config(report_type) else {
                continue;
            };
            let record_id = self.extract_record_id(item, config);

            if self.is_record_processed(&record_id) {
                continue;
            }

            let mut context = ProcessingContext::new(
                record_id.clone(),
                report_type.to_string(),
                item.clone(),
                admin_id,
            );
            context.record.config = Some(config.clone());

            let pipeline = self.create_pipeline();
            if let Err(e) = pipeline.execute(&mut context) {
                eprintln!("处理记录 {} 失败: {},跳过", record_id, e);
                continue;
            }

            if context.state.processed {
                processed += 1;
                self.batch_manager
                    .lock()
                    .unwrap()
                    .mark_record_processed(&record_id);
            }
        }

        Ok(processed)
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

    fn record_id_matches(&self, item: &Value, target_id: &str) -> bool {
        if let Some(rt) = self.infer_report_type(item)
            && let Some(cfg) = self.fetcher.registry.get_config(rt)
        {
            return item
                .get(&cfg.report_id_field)
                .map(value_to_string)
                .map(|s| s == target_id)
                .unwrap_or(false);
        }
        false
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
