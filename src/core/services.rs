use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use super::pipeline::{
    BatchActionManager, ProcessingContext, ProcessingPipeline, apply_action_by_method,
};
use super::types::{
    ProcessorError, ReportFetcher, ReportTypeRegistry, bytes_to_human, get_valid_input,
    value_to_string,
};
use crate::api::whale::{ReportHandler, ReportStatus, Resolution};
use crate::core::pipeline::BatchGroup;
use crate::core::pipeline::ReportIdExt;
use crate::utils::acquire::{FileUploader, KittyFactory};

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
            if entry.file_type().is_ok_and(|ft| ft.is_file()) {
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
            if let Some(report_type) = self.infer_report_type(item)
                && let Some(config) = self.fetcher.registry.get_config(&report_type)
            {
                let record_id = item
                    .get(&config.report_id_field)
                    .map(value_to_string)
                    .unwrap_or_else(|| "0".to_string());

                let item_id = item
                    .get(&config.item_id_field)
                    .map(value_to_string)
                    .unwrap_or_default();

                item_id_groups
                    .entry(item_id.clone())
                    .or_default()
                    .push(record_id.clone());

                let content_key = format!(
                    "{}:{}:{}",
                    item.get(&config.content_field)
                        .map(value_to_string)
                        .unwrap_or_default(),
                    report_type,
                    item.get(&config.source_id_field)
                        .map(value_to_string)
                        .unwrap_or_default()
                );
                content_groups
                    .entry(content_key)
                    .or_default()
                    .push(record_id);
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
                        && let Some(report_type) = self.infer_report_type(item)
                        && self
                            .fetcher
                            .registry
                            .is_action_available(&report_type, &action)
                    {
                        let _ = self.apply_simple_action(item, &report_type, &action, admin_id);
                        self.batch_manager
                            .lock()
                            .unwrap()
                            .mark_record_processed(record_id);
                    }
                }
            } else {
                if let Some(first_record_id) = group.record_ids.first()
                    && let Some(first_item) = chunk
                        .iter()
                        .find(|v| self.record_id_matches(v, first_record_id))
                    && let Some(report_type) = self.infer_report_type(first_item)
                {
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
                                && let Some(rt) = self.infer_report_type(item)
                            {
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

        Ok(())
    }

    fn record_id_matches(&self, item: &Value, target_id: &str) -> bool {
        if let Some(rt) = self.infer_report_type(item)
            && let Some(cfg) = self.fetcher.registry.get_config(&rt)
        {
            return item
                .get(&cfg.report_id_field)
                .map(value_to_string)
                .map(|s| s == target_id)
                .unwrap_or(false);
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
            let report_type = self.infer_report_type(item);
            let config = report_type
                .as_ref()
                .and_then(|rt| self.fetcher.registry.get_config(rt));

            let record_id = config
                .map(|c| {
                    item.get(&c.report_id_field)
                        .map(value_to_string)
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
        if let Some(t) = item.get("_report_type").and_then(|v| v.as_str()) {
            return Some(t.to_string());
        }
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
