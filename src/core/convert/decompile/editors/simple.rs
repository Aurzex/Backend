//! 轻量编辑器的抓取与反编译:COCO、NEKO、WOOD。
//!
//! 三者都是「单文件抓取 + 解密/重建 + 落盘」,没有 Kitten 那种 blocksXML/积木树重建
//! 的复杂度,故合在一处(原 `coco.rs` / `neko.rs` / `wood.rs`);
//! NEMO 因为要重组资源目录、另有素材管理器,单独成文件。

use crate::api::auth::CloudAuthenticator;
use crate::core::convert::decompile::{
    DecompileResult, DecompilerContext, WorkDecompiler, save_json_result, save_path_result,
};
use crate::core::convert::shared::{
    CryptoService, DecompilerConfig, DecompilerError, FileService, HttpClient, RawWorkData, Result,
    ValueExt, WorkFetcher, WorkId, WorkInfo,
};
use log::warn;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// COCO
pub(crate) struct CocoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl CocoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for CocoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let url = format!(
            "{}/coconut/web/work/{}/load",
            self.config.creation_base_url, work_info.id
        );
        let data = self.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("data")
            .and_then(|v| v.get("bcmc_url"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取bcmc_url".to_string()))?;
        let compiled = self.http_client.get_json(compiled_url, None)?;
        Ok(RawWorkData::Coco(Arc::new(compiled)))
    }
}

pub(crate) struct CocoDecompiler;

impl CocoDecompiler {
    fn reorganize(work: &mut Value, context: &DecompilerContext) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;

        let mut widget_map = work_obj
            .remove("widgetMap")
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let screen_list = work_obj
            .remove("screenList")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();

        work_obj.insert("authorId".to_string(), json!(context.work_info.user_id));
        work_obj.insert("title".to_string(), json!(context.work_info.name));
        // screens/screenIds 在下方由真实数据插入,无需先放空占位

        let mut screens = serde_json::Map::new();
        let mut screen_ids = Vec::with_capacity(screen_list.len());

        for screen in screen_list {
            // 直接解构出 Map 所有权,循环末尾整体移入 screens,避免整屏深克隆
            let mut screen_obj = match screen {
                Value::Object(map) => map,
                _ => {
                    return Err(DecompilerError::Decompile("screen不是对象".to_string()));
                }
            };
            let screen_id = screen_obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("screen缺少id".to_string()))?
                .to_string();
            screen_obj.insert("snapshot".to_string(), json!(""));
            screen_obj.insert("primitiveVariables".to_string(), json!([]));
            screen_obj.insert("arrayVariables".to_string(), json!([]));
            screen_obj.insert("objectVariables".to_string(), json!([]));
            screen_obj.insert("broadcasts".to_string(), json!(["Hi"]));
            screen_obj.insert("widgets".to_string(), json!({}));

            let widget_ids = screen_obj
                .get("widgetIds")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let invisible_widget_ids = screen_obj
                .get("invisibleWidgetIds")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut screen_widgets = serde_json::Map::new();
            let mut missing_ids = Vec::new();
            for wid in widget_ids.iter().chain(invisible_widget_ids.iter()) {
                if let Some(id) = wid.as_str() {
                    if let Some(widget) = widget_map.as_object_mut().and_then(|map| map.remove(id))
                    {
                        screen_widgets.insert(id.to_string(), widget);
                    } else {
                        warn!(
                            "屏幕 {} 中引用的部件 {} 在 widgetMap 中缺失,已保留在全局池",
                            screen_id, id
                        );
                        missing_ids.push(Value::String(id.to_string()));
                    }
                }
            }
            if !missing_ids.is_empty() {
                screen_obj.insert("missing_widget_ids".to_string(), Value::Array(missing_ids));
            }
            screen_obj.insert("widgets".to_string(), Value::Object(screen_widgets));
            screen_ids.push(Value::String(screen_id.clone()));
            screens.insert(screen_id, Value::Object(screen_obj));
        }

        work_obj.insert("screens".to_string(), Value::Object(screens));
        work_obj.insert("screenIds".to_string(), Value::Array(screen_ids));
        work_obj.insert("widgetMap".to_string(), widget_map);

        if let Some(block_json_map) = work_obj.get("blockJsonMap").and_then(|v| v.as_object()) {
            let mut blockly = serde_json::Map::new();
            for (screen_id, blocks) in block_json_map {
                blockly.insert(
                    screen_id.clone(),
                    json!({
                        "screenId": screen_id,
                        "workspaceJson": blocks,
                        "workspaceOffset": {"x": 0, "y": 0}
                    }),
                );
            }
            work_obj.insert("blockly".to_string(), Value::Object(blockly));
        }

        for (map_name, list_name) in &[
            ("imageFileMap", "imageFileList"),
            ("soundFileMap", "soundFileList"),
            ("iconFileMap", "iconFileList"),
            ("fontFileMap", "fontFileList"),
        ] {
            if let Some(map) = work_obj.get(*map_name).and_then(|v| v.as_object()) {
                let values: Vec<Value> = map.values().cloned().collect();
                work_obj.insert(list_name.to_string(), Value::Array(values));
            }
        }

        if let Some(variable_map) = work_obj.get("variableMap").and_then(|v| v.as_object()) {
            let mut var_list = Vec::new();
            let mut list_list = Vec::new();
            let mut dict_list = Vec::new();
            for (var_id, value) in variable_map {
                if value.is_array() {
                    list_list.push(json!({"id": var_id, "name": format!("列表{}", list_list.len()+1), "defaultValue": value, "value": value}));
                } else if value.is_object() {
                    dict_list.push(json!({"id": var_id, "name": format!("字典{}", dict_list.len()+1), "defaultValue": value, "value": value}));
                } else {
                    var_list.push(json!({"id": var_id, "name": format!("变量{}", var_list.len()+1), "defaultValue": value, "value": value}));
                }
            }
            work_obj.insert("globalVariableList".to_string(), json!(var_list));
            work_obj.insert("globalArrayList".to_string(), json!(list_list));
            work_obj.insert("globalObjectList".to_string(), json!(dict_list));
        }

        if let Some(widget_map) = work_obj.get("widgetMap").cloned() {
            work_obj.insert("globalWidgets".to_string(), widget_map);
        } else {
            work_obj.insert("globalWidgets".to_string(), json!({}));
        }
        if let Some(widget_map) = work_obj.get("widgetMap").and_then(|v| v.as_object()) {
            let widget_ids: Vec<String> = widget_map.keys().cloned().collect();
            work_obj.insert("globalWidgetIds".to_string(), json!(widget_ids));
        } else {
            work_obj.insert("globalWidgetIds".to_string(), json!([]));
        }
        work_obj.insert("sourceTag".to_string(), json!(1));
        work_obj.insert("sourceId".to_string(), json!(""));

        for key in &[
            "apiToken",
            "blockCode",
            "blockJsonMap",
            "fontFileMap",
            "gridMap",
            "iconFileMap",
            "id",
            "imageFileMap",
            "initialScreenId",
            "screenList",
            "soundFileMap",
            "variableMap",
            "widgetMap",
        ] {
            work_obj.remove(*key);
        }
        Ok(())
    }
}

impl WorkDecompiler for CocoDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let mut work = match raw {
            RawWorkData::Coco(data) => (*data).clone(),
            _ => {
                return Err(DecompilerError::Decompile(
                    "CocoDecompiler 需要 Coco 数据".into(),
                ));
            }
        };
        Self::reorganize(&mut work, context)?;
        Ok(DecompileResult::Json(work))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        let extension = context
            .work_info
            .file_extension(&context.config)
            .trim_start_matches('.')
            .to_owned();
        save_json_result(result, output_dir, context, &extension, "COCO")
    }
}

// ===========================================================================
// NEKO(原 neko.rs)
// ===========================================================================

// NEKO
pub(crate) struct NekoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NekoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for NekoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let detail_url = format!(
            "{}/neko/community/player/published-work-detail/{}",
            self.config.creation_base_url, work_info.id
        );

        let mut auth = CloudAuthenticator::new(None);
        let device_auth = auth
            .generate_x_device_auth()
            .map_err(|e| DecompilerError::Other {
                msg: format!("生成设备认证失败: {}", e),
                source: Some(Box::new(DecompilerError::Other {
                    msg: e.to_string(),
                    source: None,
                })),
            })?;

        // 修复:generate_x_device_auth 已返回 JSON 字符串({"sign":...,"timestamp":...,"client_id":...}),
        // 直接作为 header 值.此前二次 serde_json::to_string 会再包一层引号转义,
        // 服务器解析 device-auth 失败返回 500 "Not a JSON Object",导致 NEKO 作品无法获取原始数据
        let headers: Vec<(String, String)> =
            vec![("x-creation-tools-device-auth".to_string(), device_auth)];

        let detail = self.http_client.get_json(&detail_url, Some(headers))?;

        let encrypted_url = detail
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;

        let encrypted_content = self.http_client.get_text(encrypted_url)?;
        Ok(RawWorkData::NekoEncrypted(encrypted_content))
    }
}

pub(crate) struct NekoDecompiler {
    crypto_service: CryptoService,
}

impl NekoDecompiler {
    pub(crate) fn new(salt: &[u8]) -> Self {
        Self {
            crypto_service: CryptoService::new(salt),
        }
    }
}

impl WorkDecompiler for NekoDecompiler {
    fn decompile(&self, raw: RawWorkData, _context: &DecompilerContext) -> Result<DecompileResult> {
        match raw {
            RawWorkData::NekoEncrypted(encrypted) => {
                // `CryptoService` 内部只有 `Arc<[u8]>` salt,直接借用即可,无需克隆
                let decrypted_json = self.crypto_service.decrypt_bcmkn_json(&encrypted)?;
                Ok(DecompileResult::Json(decrypted_json))
            }
            _ => Err(DecompilerError::Decompile(
                "NekoDecompiler 需要 NekoEncrypted 数据".into(),
            )),
        }
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        // 扩展名与其它编辑器同一来源(与 coco/kitten 一致,不再硬编码)
        let extension = context
            .work_info
            .file_extension(&context.config)
            .trim_start_matches('.')
            .to_owned();
        save_json_result(result, output_dir, context, &extension, "NEKO")
    }
}

// ===========================================================================
// WOOD(原 wood.rs)
// ===========================================================================

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
        // 与 `save_result` 同一落点:调用方指定了就写它,否则回退默认目录
        let base_dir = context
            .output_dir
            .as_deref()
            .unwrap_or(&context.config.default_output_dir);
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
