use crate::core::convert::decompile::{
    context::DecompilerContext,
    contract::{DecompileResult, WorkDecompiler, save_path_result},
};
use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, FileService, HttpClient, RawWorkData, Result, ValueExt,
    WorkFetcher, WorkId, WorkInfo,
};
use log::warn;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// WOOD
pub(crate) struct WoodResourceConfig<'a> {
    pub(crate) http_client: &'a dyn HttpClient,
    pub(crate) file_service: &'a FileService,
    pub(crate) work_id: WorkId,
}

pub(crate) struct WoodFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl WoodFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for WoodFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let publish_url = format!(
            "{}/wood/work/{}/publish?channel_type=0",
            self.config.creation_base_url, work_info.id
        );
        let data = self.http_client.get_json(&publish_url, None)?;
        Ok(RawWorkData::Wood(Arc::new(data)))
    }
}

pub(crate) struct WoodDecompiler;

impl WoodDecompiler {
    fn decompile_inner(context: &DecompilerContext, work_data: Arc<Value>) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id.get(), "");
        let base_dir = &context.config.default_output_dir;
        let work_dir = base_dir.join(folder_name);

        let resource_config = WoodResourceConfig {
            http_client: &*context.http_client,
            file_service: &context.file_service,
            work_id,
        };
        let mut resource_manager = WoodResourceManager::new(resource_config, work_dir.clone());

        resource_manager.create_directories()?;
        resource_manager.save_work_files(&work_data)?;
        Ok(work_dir.to_string_lossy().to_string())
    }
}

impl WorkDecompiler for WoodDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let data = match raw {
            RawWorkData::Wood(d) => d,
            _ => {
                return Err(DecompilerError::Decompile(
                    "WoodDecompiler 需要 Wood 数据".into(),
                ));
            }
        };
        let path = Self::decompile_inner(context, data)?;
        Ok(DecompileResult::Path(path))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        _output_dir: Option<&Path>,
        _context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_path_result(result, "WOOD")
    }
}

pub(crate) struct WoodResourceManager<'a> {
    config: WoodResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
}

impl<'a> WoodResourceManager<'a> {
    pub(crate) fn new(config: WoodResourceConfig<'a>, work_dir: PathBuf) -> Self {
        Self {
            config,
            work_dir,
            dirs: HashMap::new(),
        }
    }

    pub(crate) fn create_directories(&mut self) -> Result<&HashMap<String, PathBuf>> {
        self.dirs
            .insert("root".to_string(), FileService::ensure_dir(&self.work_dir)?);
        self.dirs.insert(
            "images".to_string(),
            FileService::ensure_dir(&self.work_dir.join("images"))?,
        );
        Ok(&self.dirs)
    }

    pub(crate) fn save_work_files(&self, work_data: &Value) -> Result<()> {
        self.save_work_info(work_data)?;
        self.save_code_files(work_data)?;
        self.download_images(work_data)?;
        Ok(())
    }

    fn save_work_info(&self, work_data: &Value) -> Result<()> {
        let root_dir = self
            .dirs
            .get("root")
            .ok_or_else(|| DecompilerError::Other {
                msg: "root目录不存在".to_string(),
                source: None,
            })?;
        let info = json!({
            "id": work_data.get_i64_or_default("work_id", 0),
            "name": work_data.get_str_or("work_name", ""),
            "type": "WOOD",
            "language_type": work_data.get_i64_or_default("language_type", 3),
            "run_mode": work_data.get_i64_or_default("run_mode", 0),
            "code_visible": work_data.get("code_visible").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "addition": work_data.get("addition").cloned().unwrap_or(json!({})),
        });
        FileService::write_json(&root_dir.join("work_info.json"), &info)
    }

    fn save_code_files(&self, work_data: &Value) -> Result<()> {
        let root_dir = self
            .dirs
            .get("root")
            .ok_or_else(|| DecompilerError::Other {
                msg: "root目录不存在".to_string(),
                source: None,
            })?;
        if let Some(content) = work_data.get("content").and_then(|v| v.as_array()) {
            for file_info in content {
                if file_info.get_i64_or_default("file_type", 0) == 2 {
                    let file_name = file_info.get_str_or("file_name", "");
                    if file_name.ends_with(".py")
                        && let Some(source) = file_info.get("source").and_then(|v| v.as_str())
                    {
                        std::fs::write(root_dir.join(file_name), source)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn extract_filename_from_url(&self, url: &str) -> String {
        if let Some(last_slash) = url.rfind('/') {
            let part = &url[last_slash + 1..];
            if let Some(q) = part.find('?') {
                return part[..q].to_string();
            }
            if let Some(h) = part.find('#') {
                return part[..h].to_string();
            }
            return part.to_string();
        }
        String::new()
    }

    fn download_images(&self, work_data: &Value) -> Result<()> {
        let images_dir = self
            .dirs
            .get("images")
            .ok_or_else(|| DecompilerError::Other {
                msg: "images目录不存在".to_string(),
                source: None,
            })?;
        if let Some(content) = work_data.get("content").and_then(|v| v.as_array()) {
            for file_info in content {
                if file_info.get_i64_or_default("file_type", 0) == 3
                    && let Some(image_url) = file_info.get("url").and_then(|v| v.as_str())
                {
                    match self.config.http_client.get_binary(image_url) {
                        Ok(data) => {
                            let name = file_info.get_str_or("file_name", "");
                            let name = if name.is_empty() {
                                self.extract_filename_from_url(image_url)
                            } else {
                                name.to_string()
                            };
                            let name = if name.is_empty() {
                                "image.png".to_string()
                            } else {
                                name
                            };
                            FileService::write_binary(&images_dir.join(name), &data)?;
                        }
                        Err(e) => warn!("图片下载失败 {}: {}", image_url, e),
                    }
                }
            }
        }
        Ok(())
    }
}
