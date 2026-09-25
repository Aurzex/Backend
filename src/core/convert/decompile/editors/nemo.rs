use crate::core::convert::decompile::{
    DecompileResult, DecompilerContext, ResourceTask, WorkDecompiler, download_resources_parallel,
    save_path_result,
};
use crate::core::convert::shared::{
    CryptoService, DecompilerConfig, DecompilerError, FileService, HttpClient, RawWorkData, Result,
    ValueExt, WorkFetcher, WorkId, WorkInfo,
};
use log::{info, warn};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// NEMO
pub(crate) struct NemoResourceConfig<'a> {
    pub(crate) http_client: &'a dyn HttpClient,
    pub(crate) file_service: &'a FileService,
    pub(crate) work_id: WorkId,
    /// 资源下载并发数(见 [`DecompileOptions::resource_concurrency`])
    pub(crate) resource_concurrency: usize,
    /// 是否下载资源文件(见 [`DecompileOptions::skip_resources`])
    pub(crate) download_resources: bool,
}

pub(crate) struct NemoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NemoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for NemoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let source_url = format!(
            "{}/creation-tools/v1/works/{}/source/public",
            self.config.base_url, work_info.id
        );
        let source_info = self.http_client.get_json(&source_url, None)?;

        let bcm_url = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取work_urls".to_string()))?;

        let bcm_data = self.http_client.get_json(bcm_url, None)?;
        Ok(RawWorkData::Nemo(Arc::new(bcm_data), Arc::new(source_info)))
    }
}

pub(crate) struct NemoDecompiler;

impl NemoDecompiler {
    fn decompile_inner(
        context: &DecompilerContext,
        bcm_data: Arc<Value>,
        source_info: Arc<Value>,
    ) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id.get(), "");
        // 与 `save_result` 同一落点:调用方指定了就写它,否则回退默认目录
        let base_dir = context
            .output_dir
            .as_deref()
            .unwrap_or(&context.config.default_output_dir);
        let work_dir = base_dir.join(folder_name);

        let resource_config = NemoResourceConfig {
            http_client: &*context.http_client,
            file_service: &context.file_service,
            work_id,
            resource_concurrency: context.resource_concurrency,
            download_resources: context.download_resources,
        };
        let mut resource_manager = NemoResourceManager::new(resource_config, work_dir.clone());

        resource_manager.create_directories()?;
        resource_manager.save_core_files(&bcm_data, &source_info)?;
        resource_manager.download_resources(&bcm_data)?;

        info!("NEMO作品解密成功!");
        info!("将反编译的文件复制到: /data/data/com.codemao.nemo/files/nemo_users_db");

        Ok(work_dir.to_string_lossy().to_string())
    }
}

impl WorkDecompiler for NemoDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let (bcm, src) = match raw {
            RawWorkData::Nemo(b, s) => (b, s),
            _ => {
                return Err(DecompilerError::Decompile(
                    "NemoDecompiler 需要 Nemo 数据".into(),
                ));
            }
        };
        let path = Self::decompile_inner(context, bcm, src)?;
        Ok(DecompileResult::Path(path))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        _output_dir: Option<&Path>,
        _context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_path_result(result, "NEMO")
    }
}

pub(crate) struct NemoResourceManager<'a> {
    config: NemoResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
    sha_cache: RefCell<HashMap<String, String>>,
}

impl<'a> NemoResourceManager<'a> {
    pub(crate) fn new(config: NemoResourceConfig<'a>, work_dir: PathBuf) -> Self {
        Self {
            config,
            work_dir,
            dirs: HashMap::new(),
            sha_cache: RefCell::new(HashMap::new()),
        }
    }

    fn get_sha(&self, url: &str) -> String {
        let mut cache = self.sha_cache.borrow_mut();
        cache
            .entry(url.to_owned())
            .or_insert_with(|| CryptoService::sha256(url))
            .clone()
    }

    pub(crate) fn create_directories(&mut self) -> Result<&HashMap<String, PathBuf>> {
        self.dirs.insert(
            "material".to_string(),
            FileService::ensure_dir(&self.work_dir.join("user_material"))?,
        );
        self.dirs.insert(
            "works".to_string(),
            FileService::ensure_dir(
                &self
                    .work_dir
                    .join("user_works")
                    .join(self.config.work_id.to_string()),
            )?,
        );
        self.dirs.insert(
            "record".to_string(),
            FileService::ensure_dir(
                &self
                    .work_dir
                    .join("user_works")
                    .join(self.config.work_id.to_string())
                    .join("record"),
            )?,
        );
        Ok(&self.dirs)
    }

    pub(crate) fn save_core_files(&self, bcm_data: &Value, source_info: &Value) -> Result<()> {
        let works_dir = self
            .dirs
            .get("works")
            .ok_or_else(|| DecompilerError::Other {
                msg: "works目录不存在".to_string(),
                source: None,
            })?;

        let bcm_path = works_dir.join(format!("{}.bcm", self.config.work_id));
        FileService::write_json(&bcm_path, bcm_data)?;

        let user_images = self.build_user_images(bcm_data)?;
        let userimg_path = works_dir.join(format!("{}.userimg", self.config.work_id));
        FileService::write_json(&userimg_path, &user_images)?;

        let meta_data = self.build_metadata(source_info)?;
        let meta_path = works_dir.join(format!("{}.meta", self.config.work_id));
        FileService::write_json(&meta_path, &meta_data)?;

        if let Some(preview) = source_info.get("preview").and_then(|v| v.as_str())
            && !preview.is_empty()
        {
            match self.config.http_client.get_binary(preview) {
                Ok(cover_data) => {
                    let cover_path = works_dir.join(format!("{}.cover", self.config.work_id));
                    FileService::write_binary(&cover_path, &cover_data)?;
                }
                Err(e) => warn!("封面下载失败: {}", e),
            }
        }

        Ok(())
    }

    fn build_user_images(&self, bcm_data: &Value) -> Result<Value> {
        let mut user_images = serde_json::Map::new();
        let mut img_dict = serde_json::Map::new();

        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for (style_id, style_data) in styles {
                if let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) {
                    let sha_hash = self.get_sha(image_url);
                    let mut style_info = serde_json::Map::new();
                    style_info.insert("id".to_string(), Value::String(style_id.clone()));
                    style_info.insert(
                        "path".to_string(),
                        Value::String(format!("user_material/{}.webp", sha_hash)),
                    );
                    img_dict.insert(style_id.clone(), Value::Object(style_info));
                }
            }
        }

        user_images.insert("user_img_dict".to_string(), Value::Object(img_dict));
        Ok(Value::Object(user_images))
    }

    fn build_metadata(&self, source_info: &Value) -> Result<Value> {
        let work_name = source_info.get_str_or("name", "");
        let work_urls = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let bcm_version = source_info.get_str_or("bcm_version", "");
        let preview = source_info.get_str_or("preview", "");

        Ok(json!({
            "bcm_count": {
                "block_cnt_without_invisible": 0.0,
                "block_cnt": 0.0,
                "entity_cnt": 1.0,
            },
            "bcm_name": work_name,
            "bcm_url": work_urls,
            "bcm_version": bcm_version,
            "download_fail": false,
            "extra_data": {},
            "have_published_status": false,
            "have_remote_resources": false,
            "is_landscape": false,
            "is_micro_bit": false,
            "is_valid": false,
            "mcloud_variable": [],
            "publish_preview": preview,
            "publish_status": 0,
            "review_state": 0,
            "template_id": 0,
            "term_id": 0,
            "type": 0,
            "upload_status": {
                "work_id": self.config.work_id.get(),
                "have_uploaded": 2,
            },
        }))
    }

    pub(crate) fn download_resources(&self, bcm_data: &Value) -> Result<()> {
        if !self.config.download_resources {
            info!("已按 skip_resources 跳过 NEMO 资源下载");
            return Ok(());
        }
        let material_dir = self
            .dirs
            .get("material")
            .ok_or_else(|| DecompilerError::Other {
                msg: "material目录不存在".to_string(),
                source: None,
            })?;

        // 收集任务:同一 url 只下一次(内容寻址文件名 = sha256(url),与 `.userimg` 里的 path 对齐)
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut tasks = Vec::new();
        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for style_data in styles.values() {
                let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) else {
                    continue;
                };
                if !seen.insert(image_url) {
                    continue;
                }
                tasks.push(ResourceTask {
                    url: image_url.to_string(),
                    dest: material_dir.join(format!("{}.webp", self.get_sha(image_url))),
                });
            }
        }
        for failure in download_resources_parallel(
            self.config.http_client,
            tasks,
            self.config.resource_concurrency,
        ) {
            warn!("资源下载失败 {failure}");
        }
        Ok(())
    }
}
