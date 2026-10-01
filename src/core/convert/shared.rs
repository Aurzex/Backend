//! 作品文件转换域共享地基:错误、JSON 访问、配置(含影子模板大表)、领域模型、文件/ID、
//! 加密、HTTP、抓取契约。上传到账号的编排**不在**这里,而在域级工具 `upload.rs`
//! (与门面同级 —— 它要读 `translate` 的生成常量;见 `docs/rounds/39` §W2a)。
//!
//! `decompile`(读)与 `translate`(写)两个子域共用,不对外暴露;对外只需要
//! `convert/mod.rs` 里那一条 `pub use`(`DecompilerError` / `EditorType` / `WorkId`)。
//!
//! 本文件由 `shared/{mod,error,model,config,infra}.rs` 合并而成(见
//! `docs/rounds/31-convert-layout-consolidation-plan.md`),分节注释保留原文件的模块说明。

// ===== 外部依赖 =====
use crate::utils::filedata::PathConfig;
use crate::utils::requests::MewError;
use crate::utils::requests::{CodeMaoClient, HttpMethod};
use aes_gcm::aead::array::Array;
use aes_gcm::aead::array::typenum::{U12, U32};
use base64::{Engine as _, engine::general_purpose};
use log::warn;
use serde_json::Value;
use serde_json::json;
use serde_json::to_string;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
// 注意:`thiserror::Error` 是**派生宏**(宏命名空间),与上面 std 的 trait 同名但可共存
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use thiserror::Error;

// ---------------------------------------------------------------------------
// 来自 shared/error.rs

// 错误定义
#[derive(Error, Debug)]
pub enum DecompilerError {
    #[error("外部错误: {0}")]
    Mew(#[from] MewError),
    #[error("加密错误: {0}")]
    Crypto(String),
    #[error("作品解析失败: {0}")]
    Decompile(String),
    #[error("不支持的作品类型: {0}")]
    UnsupportedType(String),
    #[error("无效的响应数据: {0}")]
    InvalidResponse(String),
    #[error("缺少字段: {field}")]
    MissingField { field: String },
    #[error("类型不匹配: 期望 {expected}, 实际 {actual}")]
    TypeMismatch { expected: String, actual: String },
    #[error("{msg}")]
    Other {
        msg: String,
        #[source]
        source: Option<Box<dyn Error + Send + Sync>>,
    },
}

impl From<std::io::Error> for DecompilerError {
    fn from(e: std::io::Error) -> Self {
        DecompilerError::Mew(e.into())
    }
}

impl From<serde_json::Error> for DecompilerError {
    fn from(e: serde_json::Error) -> Self {
        DecompilerError::Mew(e.into())
    }
}

pub(crate) type Result<T> = std::result::Result<T, DecompilerError>;

// 错误上下文扩展
pub(crate) trait ResultExt<T> {
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|e| DecompilerError::Other {
            msg: f(),
            source: Some(Box::new(e)),
        })
    }
}

// ---------------------------------------------------------------------------
// 来自 shared/model.rs
// 领域模型:编辑器判别与扩展名表、作品信息、抓取契约、id 生成器。

// 作品类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorType {
    Kitten2,
    Kitten3,
    Kitten4,
    Coco,
    Neko,
    Nemo,
    Wood,
}

impl std::str::FromStr for EditorType {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "KITTEN2" => Ok(EditorType::Kitten2),
            "KITTEN3" => Ok(EditorType::Kitten3),
            // 无后缀的 KITTEN 作品(如 geometry 对战)编辑版使用 XML shadow,对应 Kitten3 格式
            "KITTEN" => Ok(EditorType::Kitten3),
            "KITTEN4" => Ok(EditorType::Kitten4),
            "COCO" => Ok(EditorType::Coco),
            "NEKO" => Ok(EditorType::Neko),
            "NEMO" => Ok(EditorType::Nemo),
            "WOOD" => Ok(EditorType::Wood),
            _ => Err(()),
        }
    }
}

impl EditorType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            EditorType::Kitten2 => "KITTEN2",
            EditorType::Kitten3 => "KITTEN3",
            EditorType::Kitten4 => "KITTEN4",
            EditorType::Coco => "COCO",
            EditorType::Neko => "NEKO",
            EditorType::Nemo => "NEMO",
            EditorType::Wood => "WOOD",
        }
    }

    pub(crate) fn use_xml_shadow(&self) -> bool {
        // Kitten2/3/4 编辑版(.bcm/.bcm4)的 shadows 均为 XML 字符串
        matches!(
            self,
            EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4
        )
    }
}

/// 作品 ID 新类型:与 user_id/admin_id 等裸 i64 区分,编译期防混用
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkId(i64);

impl WorkId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl From<i64> for WorkId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

impl From<WorkId> for i64 {
    fn from(id: WorkId) -> i64 {
        id.0
    }
}

impl std::fmt::Display for WorkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 作品信息
#[derive(Debug, Clone)]
pub(crate) struct WorkInfo {
    pub(crate) id: WorkId,
    pub(crate) name: String,
    pub(crate) work_type: EditorType,
    pub(crate) user_id: i64,
    /// 源作品的 `bcm_version`(作品详情接口给的元信息;建作品时要原样带上,空值由调用方兜底)
    pub(crate) bcm_version: String,
    /// 源作品的封面 URL(详情接口的 `preview`)。
    ///
    /// **建作品要用**:平台对 `preview` 做合法性校验,空串会被拒(`参数preview封面非法`),
    /// 所以"上传到账号"这条路上要么给源作品封面、要么给平台认的封面地址。
    pub(crate) preview: Option<String>,
}

impl WorkInfo {
    pub(crate) fn from_api_response(data: &Value) -> Result<Self> {
        let work_type_str = data.get_str_or("type", "NEMO");
        let work_type = work_type_str
            .parse::<EditorType>()
            .unwrap_or(EditorType::Nemo);
        let name = data
            .get("work_name")
            .or_else(|| data.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知作品")
            .to_string();
        Ok(Self {
            id: WorkId::new(data.get_i64_or_default("id", 0)),
            name,
            work_type,
            user_id: data.get_i64_or_default("user_id", 0),
            bcm_version: data.get_str_or("bcm_version", "").to_string(),
            preview: data
                .get("preview")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        })
    }

    pub(crate) fn file_extension(&self, config: &Arc<DecompilerConfig>) -> String {
        config
            .file_extensions
            .get(self.work_type.as_str())
            .cloned()
            .unwrap_or(".json".to_string())
    }
}

// ===== 抓取契约(原 fetch.rs)=====

pub(crate) enum RawWorkData {
    Kitten(Arc<Value>),
    NekoEncrypted(String),
    Nemo(Arc<Value>, Arc<Value>),
    Wood(Arc<Value>),
    Coco(Arc<Value>),
}

pub(crate) trait WorkFetcher: Send + Sync {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData>;
}

// ===== id 生成器(原 files.rs 的 IdGenerator)=====

// 新 ID 生成器(方案一风格)
#[derive(Clone)]
pub(crate) struct IdGenerator {
    chars: Vec<char>,
}

impl IdGenerator {
    pub(crate) fn new() -> Self {
        let chars = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .collect();
        Self { chars }
    }

    pub(crate) fn generate(&self, length: usize) -> String {
        (0..length)
            .map(|_| {
                let idx = fastrand::usize(0..self.chars.len());
                self.chars[idx]
            })
            .collect()
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 来自 shared/config.rs
// 配置(含影子模板大表)与影子构建器。

// 阴影模板
#[derive(Debug, Clone)]
pub(crate) struct ShadowTemplate {
    pub(crate) editable: bool,
    pub(crate) visible: String,
    pub(crate) extra_fields: Vec<(String, String)>,
    pub(crate) default_text: Option<String>,
    pub(crate) use_custom_name: bool,
    pub(crate) main_field: Option<String>, // 新增,指明主字段名(在 shadow_fields 中的键)
}

impl Default for ShadowTemplate {
    fn default() -> Self {
        Self {
            editable: true,
            visible: "visible".to_string(),
            extra_fields: vec![],
            default_text: None,
            use_custom_name: false,
            main_field: None,
        }
    }
}

// 配置
#[derive(Debug, Clone)]
pub(crate) struct DecompilerConfig {
    pub(crate) base_url: String,
    pub(crate) creation_base_url: String,
    pub(crate) crypto_salt: Vec<u8>,
    pub(crate) default_output_dir: PathBuf,
    pub(crate) toolbox_categories: Vec<String>,
    pub(crate) shadow_types: Arc<HashSet<String>>,
    pub(crate) shadow_fields: Arc<HashMap<String, HashMap<String, String>>>,
    pub(crate) file_extensions: Arc<HashMap<String, String>>,
    pub(crate) shadow_templates: Arc<HashMap<String, ShadowTemplate>>,
}

impl Default for DecompilerConfig {
    fn default() -> Self {
        let mut shadow_types = HashSet::new();
        for st in [
            "broadcast_input",
            "controller_shadow",
            "default_value",
            "get_audios",
            "get_current_costume",
            "get_current_scene",
            "get_sensing_current_scene",
            "get_whole_audios",
            "lists_get",
            "logic_empty",
            "logic_boolean",
            "math_number",
            "text",
            "shadow_text",
            "shadow_number",
        ] {
            shadow_types.insert(st.to_string());
        }

        let mut shadow_fields = HashMap::new();
        let mut math_number = HashMap::new();
        math_number.insert("name".to_string(), "NUM".to_string());
        math_number.insert("text".to_string(), "0".to_string());
        math_number.insert(
            "constraints".to_string(),
            "-Infinity,Infinity,0,".to_string(),
        );
        math_number.insert("allow_text".to_string(), "true".to_string());
        shadow_fields.insert("math_number".to_string(), math_number);

        let mut controller_shadow = HashMap::new();
        controller_shadow.insert("name".to_string(), "NUM".to_string());
        controller_shadow.insert("text".to_string(), "0".to_string());
        controller_shadow.insert(
            "constraints".to_string(),
            "-Infinity,Infinity,0,false".to_string(),
        );
        shadow_fields.insert("controller_shadow".to_string(), controller_shadow);

        let mut text = HashMap::new();
        text.insert("name".to_string(), "TEXT".to_string());
        text.insert("text".to_string(), String::new());
        shadow_fields.insert("text".to_string(), text);

        let mut lists_get = HashMap::new();
        lists_get.insert("name".to_string(), "VAR".to_string());
        lists_get.insert("text".to_string(), "?".to_string());
        shadow_fields.insert("lists_get".to_string(), lists_get);

        let mut broadcast_input = HashMap::new();
        broadcast_input.insert("name".to_string(), "MESSAGE".to_string());
        broadcast_input.insert("text".to_string(), "Hi".to_string());
        shadow_fields.insert("broadcast_input".to_string(), broadcast_input);

        let mut get_audios = HashMap::new();
        get_audios.insert("name".to_string(), "sound_id".to_string());
        get_audios.insert("text".to_string(), "?".to_string());
        shadow_fields.insert("get_audios".to_string(), get_audios);

        let mut get_whole_audios = HashMap::new();
        get_whole_audios.insert("name".to_string(), "sound_id".to_string());
        get_whole_audios.insert("text".to_string(), "all".to_string());
        shadow_fields.insert("get_whole_audios".to_string(), get_whole_audios);

        let mut get_current_costume = HashMap::new();
        get_current_costume.insert("name".to_string(), "style_id".to_string());
        get_current_costume.insert("text".to_string(), String::new());
        shadow_fields.insert("get_current_costume".to_string(), get_current_costume);

        let mut default_value = HashMap::new();
        default_value.insert("name".to_string(), "TEXT".to_string());
        default_value.insert("text".to_string(), "0".to_string());
        default_value.insert("has_been_edited".to_string(), "false".to_string());
        shadow_fields.insert("default_value".to_string(), default_value);

        let mut get_current_scene = HashMap::new();
        get_current_scene.insert("name".to_string(), "scene".to_string());
        get_current_scene.insert("text".to_string(), String::new());
        shadow_fields.insert("get_current_scene".to_string(), get_current_scene);

        let mut get_sensing_current_scene = HashMap::new();
        get_sensing_current_scene.insert("name".to_string(), "scene".to_string());
        get_sensing_current_scene.insert("text".to_string(), String::new());
        shadow_fields.insert(
            "get_sensing_current_scene".to_string(),
            get_sensing_current_scene,
        );

        let mut shadow_text = HashMap::new();
        shadow_text.insert("name".to_string(), "TEXT".to_string());
        shadow_text.insert("text".to_string(), String::new());
        shadow_fields.insert("shadow_text".to_string(), shadow_text);

        let mut shadow_number = HashMap::new();
        shadow_number.insert("name".to_string(), "NUM".to_string());
        shadow_number.insert("text".to_string(), "0".to_string());
        shadow_number.insert(
            "constraints".to_string(),
            "-Infinity,Infinity,0,".to_string(),
        );
        shadow_fields.insert("shadow_number".to_string(), shadow_number);

        // 新增 variables_get 字段定义
        let mut variables_get = HashMap::new();
        variables_get.insert("name".to_string(), "VAR".to_string());
        variables_get.insert("text".to_string(), "?".to_string());
        shadow_fields.insert("variables_get".to_string(), variables_get);

        let mut file_extensions = HashMap::new();
        file_extensions.insert("KITTEN2".to_string(), ".bcm".to_string());
        file_extensions.insert("KITTEN3".to_string(), ".bcm".to_string());
        file_extensions.insert("KITTEN4".to_string(), ".bcm4".to_string());
        file_extensions.insert("COCO".to_string(), ".json".to_string());
        file_extensions.insert("NEKO".to_string(), ".bcmkn".to_string());
        file_extensions.insert("NEMO".to_string(), String::new());
        file_extensions.insert("WOOD".to_string(), String::new());

        let mut shadow_templates = HashMap::new();
        shadow_templates.insert(
            "logic_empty".to_string(),
            ShadowTemplate {
                editable: false,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: None,
                use_custom_name: false,
                main_field: None,
            },
        );
        shadow_templates.insert(
            "logic_boolean".to_string(),
            ShadowTemplate {
                editable: false,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: None,
                use_custom_name: false,
                main_field: None,
            },
        );
        shadow_templates.insert(
            "math_number".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![
                    (
                        "constraints".to_string(),
                        "-Infinity,Infinity,0,".to_string(),
                    ),
                    ("allow_text".to_string(), "true".to_string()),
                ],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "math_angle".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![("constraints".to_string(), "0,360,0,".to_string())],
                default_text: Some("90".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "text".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "broadcast_input".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("Hi".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "lists_get".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("?".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "default_value".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![("has_been_edited".to_string(), "false".to_string())],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_audios".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("?".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_whole_audios".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("all".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_current_costume".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_current_scene".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_sensing_current_scene".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "controller_shadow".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![(
                    "constraints".to_string(),
                    "-Infinity,Infinity,0,false".to_string(),
                )],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "shadow_text".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "shadow_number".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![(
                    "constraints".to_string(),
                    "-Infinity,Infinity,0,".to_string(),
                )],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        // 新增 variables_get 模板
        shadow_templates.insert(
            "variables_get".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("?".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );

        Self {
            base_url: "https://api.codemao.cn".to_string(),
            creation_base_url: "https://api-creation.codemao.cn".to_string(),
            crypto_salt: (0..31).collect(),
            default_output_dir: PathConfig::global().compile_file_path(),
            toolbox_categories: vec![
                "action",
                "advanced",
                "ai",
                "ai_game",
                "ai_lab",
                "appearance",
                "arduino",
                "audio",
                "camera",
                "cloud_list",
                "cloud_variable",
                "cognitive",
                "control",
                "data",
                "event",
                "micro_bit",
                "midi_music",
                "mobile_control",
                "operator",
                "pen",
                "physic",
                "physics2",
                "procedure",
                "sensing",
                "video",
                "wee_make",
                "wood",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            shadow_types: Arc::new(shadow_types),
            shadow_fields: Arc::new(shadow_fields),
            file_extensions: Arc::new(file_extensions),
            shadow_templates: Arc::new(shadow_templates),
        }
    }
}

// ===== 影子构建器(原 decompile/shadow.rs)=====

// 阴影构建器
#[derive(Clone)]
pub(crate) struct ShadowBuilder {
    pub(crate) config: Arc<DecompilerConfig>,
    pub(crate) id_generator: IdGenerator,
    work_type: EditorType,
}

impl ShadowBuilder {
    pub(crate) fn new(
        config: Arc<DecompilerConfig>,
        id_generator: IdGenerator,
        work_type: EditorType,
    ) -> Self {
        Self {
            config,
            id_generator,
            work_type,
        }
    }

    pub(crate) fn create(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> Value {
        if self.work_type.use_xml_shadow() {
            let xml = self.create_xml(shadow_type, block_id, text);
            Value::String(xml)
        } else {
            self.create_json(shadow_type, block_id, text)
        }
    }

    pub(crate) fn create_json(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> Value {
        let template = self.config.shadow_templates.get(shadow_type);
        let block_id = block_id.unwrap_or_else(|| self.id_generator.generate(20));

        if let Some(tmpl) = template {
            let display_text = text.or(tmpl.default_text.as_deref()).unwrap_or("");

            let mut map = serde_json::Map::new();
            map.insert("type".to_string(), Value::String(shadow_type.to_string()));
            map.insert("id".to_string(), Value::String(block_id));
            map.insert("visible".to_string(), Value::String(tmpl.visible.clone()));
            map.insert("editable".to_string(), Value::Bool(tmpl.editable));

            if tmpl.use_custom_name {
                let mut fields = serde_json::Map::new();
                if let Some(field_map) = self.config.shadow_fields.get(shadow_type) {
                    if let Some(main_field) = &tmpl.main_field {
                        for (fname, default_val) in field_map {
                            if fname == main_field {
                                fields
                                    .insert(fname.clone(), Value::String(display_text.to_string()));
                            } else {
                                fields.insert(fname.clone(), Value::String(default_val.clone()));
                            }
                        }
                    } else {
                        for (fname, val) in field_map {
                            fields.insert(fname.clone(), Value::String(val.clone()));
                        }
                    }
                }
                for (key, value) in &tmpl.extra_fields {
                    fields.insert(key.clone(), Value::String(value.clone()));
                }
                map.insert("fields".to_string(), Value::Object(fields));
            }
            Value::Object(map)
        } else {
            warn!("未找到影子类型 {} 的模板,使用默认回退", shadow_type);
            json!({
                "type": "logic_empty",
                "id": block_id,
                "visible": "visible",
                "editable": false,
            })
        }
    }

    fn create_xml(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> String {
        let template = self.config.shadow_templates.get(shadow_type);
        let block_id = block_id.unwrap_or_else(|| self.id_generator.generate(20));

        let Some(tmpl) = template else {
            warn!("未找到影子类型 {} 的模板,回退为 logic_empty", shadow_type);
            return format!(
                r#"<shadow type="logic_empty" id="{}" visible="visible" editable="false"></shadow>"#,
                block_id
            );
        };
        let display_text = text.or(tmpl.default_text.as_deref()).unwrap_or("");

        if !tmpl.use_custom_name {
            return format!(
                r#"<shadow type="{}" id="{}" visible="{}" editable="{}"></shadow>"#,
                shadow_type, block_id, tmpl.visible, tmpl.editable
            );
        }

        let mut fields: Vec<(String, String)> = Vec::new();
        if let Some(field_map) = self.config.shadow_fields.get(shadow_type) {
            if let Some(main_field) = &tmpl.main_field {
                for (fname, default_val) in field_map {
                    if fname == main_field {
                        fields.push((fname.clone(), display_text.to_string()));
                    } else {
                        fields.push((fname.clone(), default_val.clone()));
                    }
                }
            } else {
                for (fname, val) in field_map {
                    fields.push((fname.clone(), val.clone()));
                }
            }
        }
        for (k, v) in &tmpl.extra_fields {
            fields.push((k.clone(), v.clone()));
        }

        let mut xml = format!(
            r#"<shadow type="{}" id="{}" visible="{}" editable="{}">"#,
            shadow_type, block_id, tmpl.visible, tmpl.editable
        );
        for (name, value) in fields {
            let _ = write!(xml, r#"<field name="{}">{}</field>"#, name, value);
        }
        xml.push_str("</shadow>");
        xml
    }
}

// ---------------------------------------------------------------------------
// 来自 shared/infra.rs
// 反编译引擎的基础设施:加密(BCMKN/AES-GCM)、HTTP 客户端、文件落盘、JSON 取值扩展。
// 这些是**无状态工具**,只被 `decompile` / `translate` 的动作层调用;
// 与「配置大表」(`config.rs`)、「领域模型」(`model.rs`)刻意分开。

use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit},
};

// 加密服务
#[derive(Clone)]
pub(crate) struct CryptoService {
    salt: Arc<[u8]>,
}

const NONCE_SIZE: usize = 12;

impl CryptoService {
    pub(crate) fn new(salt: &[u8]) -> Self {
        Self {
            salt: Arc::from(salt),
        }
    }

    pub(crate) fn sha256(data: &str) -> String {
        use std::fmt::Write as _;
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let result = hasher.finalize();
        let mut out = String::with_capacity(result.len() * 2);
        for b in result {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    pub(crate) fn base64_to_bytes(data: &str) -> Result<Vec<u8>> {
        general_purpose::STANDARD
            .decode(data)
            .map_err(|e| DecompilerError::Crypto(format!("Base64解码失败: {}", e)))
    }

    pub(crate) fn reverse_string(data: &str) -> String {
        data.chars().rev().collect()
    }

    pub(crate) fn generate_aes_key(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.salt);
        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash);
        key
    }

    pub(crate) fn decrypt_aes_gcm(&self, ciphertext: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
        type AesKey = Array<u8, U32>;
        type Nonce = Array<u8, U12>;

        let key = self.generate_aes_key();
        let key_array = AesKey::try_from(key.as_slice())
            .map_err(|e| DecompilerError::Crypto(format!("Invalid AES key: {}", e)))?;
        let cipher = Aes256Gcm::new(&key_array);
        let nonce = Nonce::try_from(iv)
            .map_err(|e| DecompilerError::Crypto(format!("Invalid nonce: {}", e)))?;

        cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| DecompilerError::Crypto(format!("AES解密失败: {}", e)))
    }

    pub(crate) fn decrypt_bcmkn(&self, encrypted_content: &str) -> Result<Vec<u8>> {
        let reversed = Self::reverse_string(encrypted_content);
        let decoded = Self::base64_to_bytes(&reversed)?;
        if decoded.len() <= NONCE_SIZE {
            return Err(DecompilerError::Crypto(format!(
                "数据长度 {} 不足,至少需要 {} 字节",
                decoded.len(),
                NONCE_SIZE + 1
            )));
        }
        let (iv, ciphertext) = decoded
            .split_at_checked(NONCE_SIZE)
            .ok_or_else(|| DecompilerError::Crypto("IV 长度不足".into()))?;
        self.decrypt_aes_gcm(ciphertext, iv)
    }

    /// NEKO 播放器下发的密文 → KN 文档 JSON:
    /// `base64(reverse(content))` → AES-GCM(前 12 字节为 IV)→ UTF-8 → JSON
    pub(crate) fn decrypt_bcmkn_json(&self, encrypted_content: &str) -> Result<Value> {
        let decrypted_bytes = self.decrypt_bcmkn(encrypted_content)?;
        let decrypted_str = String::from_utf8(decrypted_bytes)
            .map_err(|e| DecompilerError::Crypto(format!("UTF-8转换失败: {}", e)))?;
        Ok(serde_json::from_str(&decrypted_str)?)
    }
}

// ===== HTTP 客户端(原 http.rs)=====

// HTTP 客户端
pub(crate) trait HttpClient: Send + Sync {
    fn get_json(&self, url: &str, headers: Option<Vec<(String, String)>>) -> Result<Value>;
    fn get_binary(&self, url: &str) -> Result<Vec<u8>>;
    fn get_text(&self, url: &str) -> Result<String>;
    fn box_clone(&self) -> Box<dyn HttpClient>;
}

impl Clone for Box<dyn HttpClient> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

#[derive(Clone)]
pub(crate) struct CodeMaoHttpClient {
    client: Arc<CodeMaoClient>,
}

impl CodeMaoHttpClient {
    pub(crate) fn new(client: Arc<CodeMaoClient>) -> Self {
        Self { client }
    }
}

impl HttpClient for CodeMaoHttpClient {
    fn get_json(&self, url: &str, headers: Option<Vec<(String, String)>>) -> Result<Value> {
        let mut request_builder = self.client.build_request(HttpMethod::Get, url, None);
        if let Some(headers_map) = headers {
            request_builder = request_builder.with_headers(headers_map);
        }
        let response = request_builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }

    fn get_binary(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .send()?;
        Ok(self.client.response_to_binary(response)?)
    }

    fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .send()?;
        Ok(self.client.response_to_string(response)?)
    }

    fn box_clone(&self) -> Box<dyn HttpClient> {
        Box::new(self.clone())
    }
}

// ===== JSON 取值扩展(原 json.rs)=====

// Value 扩展
pub(crate) trait ValueExt {
    fn get_i64_or_default(&self, key: &str, default: i64) -> i64;
    fn get_str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str;
    fn get_string_or(&self, key: &str, default: &str) -> String;
    fn get_array_opt(&self, key: &str) -> Option<&Vec<Value>>;
    fn get_object_opt(&self, key: &str) -> Option<&serde_json::Map<String, Value>>;
}

impl ValueExt for Value {
    fn get_i64_or_default(&self, key: &str, default: i64) -> i64 {
        self.get(key)
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(default)
    }

    fn get_str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).and_then(|v| v.as_str()).unwrap_or(default)
    }

    fn get_string_or(&self, key: &str, default: &str) -> String {
        self.get_str_or(key, default).to_string()
    }

    fn get_array_opt(&self, key: &str) -> Option<&Vec<Value>> {
        self.get(key).and_then(|v| v.as_array())
    }

    fn get_object_opt(&self, key: &str) -> Option<&serde_json::Map<String, Value>> {
        self.get(key).and_then(|v| v.as_object())
    }
}

// ===== 文件服务(原 files.rs 的 FileService)=====

// 文件服务
/// 文件读写工具(命名空间式:方法全是关联函数,**不持有配置**)
///
/// 曾经带一个 `config: Arc<DecompilerConfig>` 字段,但全仓**零读取点** ⇒ 已删
/// (rounds/37 M5:它只是在 DecompilerContext / *ResourceConfig 之间白传一个 Arc)。
#[derive(Clone)]
pub(crate) struct FileService;

impl FileService {
    pub(crate) fn safe_filename(name: &str, work_id: i64, extension: &str) -> String {
        let safe_name: String = name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
            .collect();
        let safe_name = safe_name.trim();
        let name_part = if safe_name.is_empty() {
            format!("work_{}", work_id)
        } else {
            safe_name.to_string()
        };
        let ext = if !extension.is_empty() && !extension.starts_with('.') {
            format!(".{}", extension)
        } else {
            extension.to_string()
        };
        format!("{}_{}{}", name_part, work_id, ext)
    }

    pub(crate) fn ensure_dir(path: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(path)?;
        Ok(path.to_path_buf())
    }

    /// 写 JSON:**流式**(`to_writer` + `BufWriter`),不产生整份中间 `String`。
    ///
    /// 与 `to_string` 逐字节相同(同一个序列化器),只省掉"文档大小 ×1 的中间串"
    /// 以及一次整块拷贝、相应峰值内存(方案 23 P0-1)。10 MB 级作品实测占
    /// `serialize` 的 15–25%。
    pub(crate) fn write_json(path: &Path, data: &Value) -> Result<()> {
        use std::io::Write as _;
        let file = std::fs::File::create(path)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, data)?;
        writer.flush()?;
        Ok(())
    }

    pub(crate) fn write_binary(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 常量

/// 影子 / 变异 XML 的命名空间(两处装配与 NEMO 映射原先各有一份逐字节相同的副本,
/// 见 `docs/rounds/31` §3.6 D3)
pub(crate) const XHTML: &str = "http://www.w3.org/1999/xhtml";

// ---------------------------------------------------------------------------
// 批量执行(反编译 / 转化两个子域共用)

/// 分块并发执行:块内 `thread::scope`、块间按原顺序收集 —— 不引入锁,结果顺序与输入一致。
///
/// 两个批量入口(`CodemaoDecompiler::decompile_batch_outcomes` 与
/// `convert::translate_works`)的并发要求完全一致,原来各写了一份一模一样的
/// chunk + `thread::scope` + 保序收集(见 `docs/rounds/31` §3 A6)。子线程 panic
/// 由 `on_panic` 折成调用方的错误类型。
///
/// `concurrency <= 1` 或只有一个工作项时直接串行执行(调用方需在此前完成并发预算折算)。
pub(crate) fn batch_map<T, R, E, F, P>(
    items: &[T],
    concurrency: usize,
    work: F,
    on_panic: P,
) -> Vec<std::result::Result<R, E>>
where
    T: Sync,
    R: Send,
    E: Send,
    F: Fn(&T) -> std::result::Result<R, E> + Sync,
    P: Fn() -> E + Sync,
{
    if concurrency <= 1 || items.len() <= 1 {
        return items.iter().map(&work).collect();
    }
    let mut results = Vec::with_capacity(items.len());
    for chunk in items.chunks(concurrency) {
        let chunk_results: Vec<std::result::Result<R, E>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|item| scope.spawn(|| work(item)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or_else(|_| Err(on_panic())))
                .collect()
        });
        results.extend(chunk_results);
    }
    results
}
