use crate::api::auth::CloudAuthenticator;
use crate::utils::filedata::PathConfig;
use crate::utils::requests::{CodeMaoClient, HttpMethod, MewError};
use aes_gcm::aead::array::Array;
use aes_gcm::aead::array::typenum::{U12, U32};
use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose};
use log::{error, info, warn};
use serde_json::{Value, json, to_string};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use thiserror::Error;

// 错误定义
#[derive(Error, Debug)]
pub enum DecompilerError {
    #[error("外部错误: {0}")]
    Mew(#[from] MewError),
    #[error("加密错误: {0}")]
    Crypto(String),
    #[error("反编译错误: {0}")]
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

// Value 扩展
pub(crate) trait ValueExt {
    fn get_str(&self, key: &str) -> Result<&str>;
    fn get_i64(&self, key: &str) -> Result<i64>;
    fn get_bool(&self, key: &str) -> Result<bool>;
    fn get_object(&self, key: &str) -> Result<&serde_json::Map<String, Value>>;
    fn get_array(&self, key: &str) -> Result<&Vec<Value>>;
    fn get_i64_or_default(&self, key: &str, default: i64) -> i64;
    fn get_str_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str;
    fn get_string_or(&self, key: &str, default: &str) -> String;
    fn get_array_opt(&self, key: &str) -> Option<&Vec<Value>>;
    fn get_object_opt(&self, key: &str) -> Option<&serde_json::Map<String, Value>>;
    fn get_str_opt(&self, key: &str) -> Option<&str>;
}

impl ValueExt for Value {
    fn get_str(&self, key: &str) -> Result<&str> {
        self.get(key)
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::MissingField {
                field: key.to_string(),
            })
    }

    fn get_i64(&self, key: &str) -> Result<i64> {
        self.get(key)
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| DecompilerError::MissingField {
                field: key.to_string(),
            })
    }

    fn get_bool(&self, key: &str) -> Result<bool> {
        self.get(key)
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| DecompilerError::MissingField {
                field: key.to_string(),
            })
    }

    fn get_object(&self, key: &str) -> Result<&serde_json::Map<String, Value>> {
        self.get(key)
            .and_then(|v| v.as_object())
            .ok_or_else(|| DecompilerError::MissingField {
                field: key.to_string(),
            })
    }

    fn get_array(&self, key: &str) -> Result<&Vec<Value>> {
        self.get(key)
            .and_then(|v| v.as_array())
            .ok_or_else(|| DecompilerError::MissingField {
                field: key.to_string(),
            })
    }

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

    fn get_str_opt(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(|v| v.as_str())
    }
}

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
    pub(crate) client_secret: String,
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
            client_secret: "pBlYqXbJDu".to_string(),
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

// 作品类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum WorkType {
    Kitten2,
    Kitten3,
    Kitten4,
    Coco,
    Neko,
    Nemo,
    Wood,
}

impl std::str::FromStr for WorkType {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "KITTEN2" => Ok(WorkType::Kitten2),
            "KITTEN3" => Ok(WorkType::Kitten3),
            // 无后缀的 KITTEN 作品(如 geometry 对战)编辑版使用 XML shadow,对应 Kitten3 格式
            "KITTEN" => Ok(WorkType::Kitten3),
            "KITTEN4" => Ok(WorkType::Kitten4),
            "COCO" => Ok(WorkType::Coco),
            "NEKO" => Ok(WorkType::Neko),
            "NEMO" => Ok(WorkType::Nemo),
            "WOOD" => Ok(WorkType::Wood),
            _ => Err(()),
        }
    }
}

impl WorkType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            WorkType::Kitten2 => "KITTEN2",
            WorkType::Kitten3 => "KITTEN3",
            WorkType::Kitten4 => "KITTEN4",
            WorkType::Coco => "COCO",
            WorkType::Neko => "NEKO",
            WorkType::Nemo => "NEMO",
            WorkType::Wood => "WOOD",
        }
    }

    pub(crate) fn is_kitten(&self) -> bool {
        matches!(
            self,
            WorkType::Kitten2 | WorkType::Kitten3 | WorkType::Kitten4
        )
    }
    pub(crate) fn is_nemo(&self) -> bool {
        matches!(self, WorkType::Nemo)
    }
    pub(crate) fn is_neko(&self) -> bool {
        matches!(self, WorkType::Neko)
    }
    pub(crate) fn is_coco(&self) -> bool {
        matches!(self, WorkType::Coco)
    }
    pub(crate) fn is_wood(&self) -> bool {
        matches!(self, WorkType::Wood)
    }
    pub(crate) fn use_xml_shadow(&self) -> bool {
        // Kitten2/3/4 编辑版(.bcm/.bcm4)的 shadows 均为 XML 字符串
        matches!(
            self,
            WorkType::Kitten2 | WorkType::Kitten3 | WorkType::Kitten4
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
    pub(crate) work_type: WorkType,
    pub(crate) version: String,
    pub(crate) user_id: i64,
    pub(crate) preview_url: String,
    pub(crate) application_version: String,
}

impl WorkInfo {
    pub(crate) fn from_api_response(data: &Value) -> Result<Self> {
        let work_type_str = data.get_str_or("type", "NEMO");
        let work_type = work_type_str.parse::<WorkType>().unwrap_or(WorkType::Nemo);
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
            version: data.get_string_or("bcm_version", "0.16.2"),
            user_id: data.get_i64_or_default("user_id", 0),
            preview_url: data.get_string_or("preview", ""),
            application_version: data.get_string_or("application_version", "0.0.0"),
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

// 文件服务
#[derive(Clone)]
pub(crate) struct FileService {
    config: Arc<DecompilerConfig>,
}

impl FileService {
    pub(crate) fn new(config: Arc<DecompilerConfig>) -> Self {
        Self { config }
    }

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

    pub(crate) fn write_json(path: &Path, data: &Value) -> Result<()> {
        let json_str = to_string(data)?;
        std::fs::write(path, json_str)?;
        Ok(())
    }

    pub(crate) fn write_binary(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }
}

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

// 加密服务
#[derive(Clone)]
pub(crate) struct CryptoService {
    salt: Vec<u8>,
}

const NONCE_SIZE: usize = 12;

impl CryptoService {
    pub(crate) fn new(salt: &[u8]) -> Self {
        Self {
            salt: salt.to_vec(),
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
}

pub(crate) struct BCMKNDecryptor {
    crypto_service: CryptoService,
}

impl BCMKNDecryptor {
    pub(crate) fn new(crypto_service: CryptoService) -> Self {
        Self { crypto_service }
    }

    pub(crate) fn decrypt(&self, encrypted_content: &str) -> Result<Value> {
        let decrypted_bytes = self.crypto_service.decrypt_bcmkn(encrypted_content)?;
        let decrypted_str = String::from_utf8(decrypted_bytes)
            .map_err(|e| DecompilerError::Crypto(format!("UTF-8转换失败: {}", e)))?;
        let json_value: Value = serde_json::from_str(&decrypted_str)?;
        Ok(json_value)
    }
}

// 阴影构建器
#[derive(Clone)]
pub(crate) struct ShadowBuilder {
    pub(crate) config: Arc<DecompilerConfig>,
    pub(crate) id_generator: IdGenerator,
    work_type: WorkType,
}

impl ShadowBuilder {
    pub(crate) fn new(
        config: Arc<DecompilerConfig>,
        id_generator: IdGenerator,
        work_type: WorkType,
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

// 积木行为
pub(crate) trait BlockDecompilerBehavior: Send + Sync {
    fn get_child_input_name(&self, index: usize, conditions_count: usize) -> String;
}

#[derive(Clone)]
pub(crate) enum BlockBehavior {
    Default,
    If { conditions_count: usize },
    FunctionBody,
}

impl BlockDecompilerBehavior for BlockBehavior {
    fn get_child_input_name(&self, index: usize, _conditions_count: usize) -> String {
        match self {
            BlockBehavior::Default => "DO".to_string(),
            BlockBehavior::If { conditions_count } => {
                if index < *conditions_count {
                    // 编辑版插槽名为 DO0/DO1/...(无空格)
                    format!("DO{}", index)
                } else {
                    // 编辑版 else 分支插槽名为 ELSE(无编号)
                    "ELSE".to_string()
                }
            }
            // 函数定义块的函数体插槽名为 STACK
            BlockBehavior::FunctionBody => "STACK".to_string(),
        }
    }
}

// 积木上下文
#[derive(Clone)]
pub(crate) struct BlockContext {
    pub(crate) actor_data: Value,
    pub(crate) functions: Arc<HashMap<String, Value>>,
    pub(crate) variable_map: Arc<HashMap<String, String>>, // UUID -> 变量名
    pub(crate) shadow_builder: ShadowBuilder,
    pub(crate) blocks: HashMap<String, Value>,
    pub(crate) connections: HashMap<String, HashMap<String, Value>>,
    // 布局游标:编译版无 location 时按树形自动排列积木,避免恢复产物全部重叠在 [0,0]
    pub(crate) layout_col: f64,
    pub(crate) layout_row: f64,
}

impl BlockContext {
    pub(crate) fn new(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
        variable_map: Arc<HashMap<String, String>>,
    ) -> Self {
        Self {
            actor_data,
            functions,
            variable_map,
            shadow_builder,
            blocks: HashMap::new(),
            connections: HashMap::new(),
            layout_col: 0.0,
            layout_row: 0.0,
        }
    }

    pub(crate) fn with_capacity(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
        variable_map: Arc<HashMap<String, String>>,
        blocks_cap: usize,
        connections_cap: usize,
    ) -> Self {
        Self {
            actor_data,
            functions,
            variable_map,
            shadow_builder,
            blocks: HashMap::with_capacity(blocks_cap),
            connections: HashMap::with_capacity(connections_cap),
            layout_col: 0.0,
            layout_row: 0.0,
        }
    }

    pub(crate) fn insert_connection(
        &mut self,
        source_id: &str,
        target_id: &str,
        connection_info: Value,
    ) {
        self.connections
            .entry(source_id.to_string())
            .or_default()
            .insert(target_id.to_string(), connection_info);
    }
}

pub(crate) struct DecompilerContext {
    pub(crate) work_info: WorkInfo,
    pub(crate) http_client: Box<dyn HttpClient>,
    pub(crate) file_service: FileService,
    pub(crate) id_generator: IdGenerator,
    pub(crate) config: Arc<DecompilerConfig>,
}

// Context Builder
pub(crate) struct DecompilerContextBuilder {
    work_info: Option<WorkInfo>,
    http_client: Option<Box<dyn HttpClient>>,
    config: Option<Arc<DecompilerConfig>>,
    file_service: Option<FileService>,
    id_generator: Option<IdGenerator>,
}

impl Default for DecompilerContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DecompilerContextBuilder {
    pub(crate) fn new() -> Self {
        Self {
            work_info: None,
            http_client: None,
            config: None,
            file_service: None,
            id_generator: None,
        }
    }

    pub(crate) fn work_info(mut self, info: WorkInfo) -> Self {
        self.work_info = Some(info);
        self
    }

    pub(crate) fn http_client(mut self, client: Box<dyn HttpClient>) -> Self {
        self.http_client = Some(client);
        self
    }

    pub(crate) fn config(mut self, config: Arc<DecompilerConfig>) -> Self {
        self.config = Some(config);
        self
    }

    pub(crate) fn file_service(mut self, service: FileService) -> Self {
        self.file_service = Some(service);
        self
    }

    pub(crate) fn id_generator(mut self, generator: IdGenerator) -> Self {
        self.id_generator = Some(generator);
        self
    }

    pub(crate) fn build(self) -> Result<DecompilerContext> {
        let config = self.config.unwrap_or_default();
        Ok(DecompilerContext {
            work_info: self.work_info.ok_or_else(|| DecompilerError::Other {
                msg: "缺少work_info".into(),
                source: None,
            })?,
            http_client: self.http_client.ok_or_else(|| DecompilerError::Other {
                msg: "缺少http_client".into(),
                source: None,
            })?,
            file_service: self
                .file_service
                .unwrap_or_else(|| FileService::new(config.clone())),
            id_generator: self.id_generator.unwrap_or_default(),
            config,
        })
    }
}

// 结果类型与 Trait
#[derive(Debug)]
pub(crate) enum DecompileResult {
    Json(Value),
    Path(String),
}

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

pub(crate) trait WorkDecompiler: Send + Sync {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult>;
    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf>;
}

/// 将 JSON 反编译结果写入输出目录,返回文件路径(供各反编译器共用)
pub(crate) fn save_json_result(
    result: &DecompileResult,
    output_dir: Option<&Path>,
    context: &DecompilerContext,
    extension: &str,
    decompiler_name: &str,
) -> Result<PathBuf> {
    match result {
        DecompileResult::Json(json) => {
            let output_path = output_dir.unwrap_or(&context.config.default_output_dir);
            FileService::ensure_dir(output_path)?;
            let filename = FileService::safe_filename(
                &context.work_info.name,
                context.work_info.id.get(),
                extension,
            );
            let filepath = output_path.join(filename);
            FileService::write_json(&filepath, json)?;
            Ok(filepath)
        }
        _ => Err(DecompilerError::Decompile(format!(
            "{}反编译器应返回JSON",
            decompiler_name
        ))),
    }
}

/// 返回路径型反编译结果(供返回路径的反编译器共用)
pub(crate) fn save_path_result(result: &DecompileResult, decompiler_name: &str) -> Result<PathBuf> {
    match result {
        DecompileResult::Path(path) => Ok(PathBuf::from(path)),
        _ => Err(DecompilerError::Decompile(format!(
            "{}反编译器应返回路径",
            decompiler_name
        ))),
    }
}

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
