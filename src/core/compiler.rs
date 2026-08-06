use crate::api::auth::CloudAuthenticator;
use crate::utils::acquire::{CodeMaoClient, HttpMethod, KittyFactory};
use crate::utils::data::PathConfig;
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
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use thiserror::Error;

// 错误定义
#[derive(Error, Debug)]
pub enum DecompilerError {
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP错误: {0}")]
    Http(String),
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

pub type Result<T> = std::result::Result<T, DecompilerError>;

// 错误上下文扩展
pub trait ResultExt<T> {
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
pub trait ValueExt {
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
            .and_then(|v| v.as_i64())
            .ok_or_else(|| DecompilerError::MissingField {
                field: key.to_string(),
            })
    }

    fn get_bool(&self, key: &str) -> Result<bool> {
        self.get(key)
            .and_then(|v| v.as_bool())
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
        self.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
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
pub struct ShadowTemplate {
    pub editable: bool,
    pub visible: String,
    pub extra_fields: Vec<(String, String)>,
    pub default_text: Option<String>,
    pub use_custom_name: bool,
    pub main_field: Option<String>, // 新增,指明主字段名(在 shadow_fields 中的键)
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
pub struct DecompilerConfig {
    pub base_url: String,
    pub creation_base_url: String,
    pub client_secret: String,
    pub crypto_salt: Vec<u8>,
    pub default_output_dir: PathBuf,
    pub toolbox_categories: Vec<String>,
    pub shadow_types: Arc<HashSet<String>>,
    pub shadow_fields: Arc<HashMap<String, HashMap<String, String>>>,
    pub file_extensions: Arc<HashMap<String, String>>,
    pub shadow_templates: Arc<HashMap<String, ShadowTemplate>>,
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
        text.insert("text".to_string(), "".to_string());
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
        get_current_costume.insert("text".to_string(), "".to_string());
        shadow_fields.insert("get_current_costume".to_string(), get_current_costume);

        let mut default_value = HashMap::new();
        default_value.insert("name".to_string(), "TEXT".to_string());
        default_value.insert("text".to_string(), "0".to_string());
        default_value.insert("has_been_edited".to_string(), "false".to_string());
        shadow_fields.insert("default_value".to_string(), default_value);

        let mut get_current_scene = HashMap::new();
        get_current_scene.insert("name".to_string(), "scene".to_string());
        get_current_scene.insert("text".to_string(), "".to_string());
        shadow_fields.insert("get_current_scene".to_string(), get_current_scene);

        let mut get_sensing_current_scene = HashMap::new();
        get_sensing_current_scene.insert("name".to_string(), "scene".to_string());
        get_sensing_current_scene.insert("text".to_string(), "".to_string());
        shadow_fields.insert(
            "get_sensing_current_scene".to_string(),
            get_sensing_current_scene,
        );

        let mut shadow_text = HashMap::new();
        shadow_text.insert("name".to_string(), "TEXT".to_string());
        shadow_text.insert("text".to_string(), "".to_string());
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
        file_extensions.insert("NEMO".to_string(), "".to_string());
        file_extensions.insert("WOOD".to_string(), "".to_string());

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
                default_text: Some("".to_string()),
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
                default_text: Some("".to_string()),
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
                default_text: Some("".to_string()),
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
                default_text: Some("".to_string()),
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
                default_text: Some("".to_string()),
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
pub enum WorkType {
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
    pub fn as_str(&self) -> &'static str {
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

    pub fn is_kitten(&self) -> bool {
        matches!(
            self,
            WorkType::Kitten2 | WorkType::Kitten3 | WorkType::Kitten4
        )
    }
    pub fn is_nemo(&self) -> bool {
        matches!(self, WorkType::Nemo)
    }
    pub fn is_neko(&self) -> bool {
        matches!(self, WorkType::Neko)
    }
    pub fn is_coco(&self) -> bool {
        matches!(self, WorkType::Coco)
    }
    pub fn is_wood(&self) -> bool {
        matches!(self, WorkType::Wood)
    }
    pub fn use_xml_shadow(&self) -> bool {
        // Kitten2/3/4 编辑版(.bcm/.bcm4)的 shadows 均为 XML 字符串
        matches!(
            self,
            WorkType::Kitten2 | WorkType::Kitten3 | WorkType::Kitten4
        )
    }
}

// 作品信息
#[derive(Debug, Clone)]
pub struct WorkInfo {
    pub id: i64,
    pub name: String,
    pub work_type: WorkType,
    pub version: String,
    pub user_id: i64,
    pub preview_url: String,
    pub application_version: String,
}

impl WorkInfo {
    pub fn from_api_response(data: &Value) -> Result<Self> {
        let work_type_str = data.get_str_or("type", "NEMO");
        let work_type = work_type_str.parse::<WorkType>().unwrap_or(WorkType::Nemo);
        let name = data
            .get("work_name")
            .or_else(|| data.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知作品")
            .to_string();
        Ok(Self {
            id: data.get_i64_or_default("id", 0),
            name,
            work_type,
            version: data.get_string_or("bcm_version", "0.16.2"),
            user_id: data.get_i64_or_default("user_id", 0),
            preview_url: data.get_string_or("preview", ""),
            application_version: data.get_string_or("application_version", "0.0.0"),
        })
    }

    pub fn file_extension(&self, config: &Arc<DecompilerConfig>) -> String {
        config
            .file_extensions
            .get(self.work_type.as_str())
            .cloned()
            .unwrap_or_else(|| ".json".to_string())
    }
}

// 文件服务
#[derive(Clone)]
pub struct FileService {
    config: Arc<DecompilerConfig>,
}

impl FileService {
    pub fn new(config: Arc<DecompilerConfig>) -> Self {
        Self { config }
    }

    pub fn safe_filename(name: &str, work_id: i64, extension: &str) -> String {
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

    pub fn ensure_dir(path: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(path)?;
        Ok(path.to_path_buf())
    }

    pub fn write_json(path: &Path, data: &Value) -> Result<()> {
        let json_str = to_string(data)?;
        std::fs::write(path, json_str)?;
        Ok(())
    }

    pub fn write_binary(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }
}

// 新 ID 生成器(方案一风格)
#[derive(Clone)]
pub struct IdGenerator {
    chars: Vec<char>,
}

impl IdGenerator {
    pub fn new() -> Self {
        let chars = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .collect();
        Self { chars }
    }

    pub fn generate(&self, length: usize) -> String {
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
pub struct CryptoService {
    salt: Vec<u8>,
}

const NONCE_SIZE: usize = 12;

impl CryptoService {
    pub fn new(salt: &[u8]) -> Self {
        Self {
            salt: salt.to_vec(),
        }
    }

    pub fn sha256(data: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let result = hasher.finalize();
        result.iter().map(|b| format!("{:02x}", b)).collect()
    }

    pub fn base64_to_bytes(&self, data: &str) -> Result<Vec<u8>> {
        general_purpose::STANDARD
            .decode(data)
            .map_err(|e| DecompilerError::Crypto(format!("Base64解码失败: {}", e)))
    }

    pub fn reverse_string(&self, data: &str) -> String {
        data.chars().rev().collect()
    }

    pub fn generate_aes_key(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.salt);
        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash);
        key
    }

    pub fn decrypt_aes_gcm(&self, ciphertext: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
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

    pub fn decrypt_bcmkn(&self, encrypted_content: &str) -> Result<Vec<u8>> {
        let reversed = self.reverse_string(encrypted_content);
        let decoded = self.base64_to_bytes(&reversed)?;
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

pub struct BCMKNDecryptor {
    crypto_service: CryptoService,
}

impl BCMKNDecryptor {
    pub fn new(crypto_service: CryptoService) -> Self {
        Self { crypto_service }
    }

    pub fn decrypt(&self, encrypted_content: &str) -> Result<Value> {
        let decrypted_bytes = self.crypto_service.decrypt_bcmkn(encrypted_content)?;
        let decrypted_str = String::from_utf8(decrypted_bytes)
            .map_err(|e| DecompilerError::Crypto(format!("UTF-8转换失败: {}", e)))?;
        let json_value: Value = serde_json::from_str(&decrypted_str)?;
        Ok(json_value)
    }
}

// 阴影构建器
#[derive(Clone)]
pub struct ShadowBuilder {
    pub(crate) config: Arc<DecompilerConfig>,
    pub(crate) id_generator: IdGenerator,
    work_type: WorkType,
}

impl ShadowBuilder {
    pub fn new(
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

    pub fn create(&self, shadow_type: &str, block_id: Option<String>, text: Option<&str>) -> Value {
        if self.work_type.use_xml_shadow() {
            let xml = self.create_xml(shadow_type, block_id, text);
            Value::String(xml)
        } else {
            self.create_json(shadow_type, block_id, text)
        }
    }

    pub fn create_json(
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

        if template.is_none() {
            warn!("未找到影子类型 {} 的模板,回退为 logic_empty", shadow_type);
            return format!(
                r#"<shadow type="logic_empty" id="{}" visible="visible" editable="false"></shadow>"#,
                block_id
            );
        }

        let tmpl = template.unwrap();
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
            xml.push_str(&format!(r#"<field name="{}">{}</field>"#, name, value));
        }
        xml.push_str("</shadow>");
        xml
    }
}

// 积木行为
pub trait BlockDecompilerBehavior: Send + Sync {
    fn get_child_input_name(&self, index: usize, conditions_count: usize) -> String;
}

#[derive(Clone)]
pub enum BlockBehavior {
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
pub struct BlockContext {
    pub actor_data: Value,
    pub functions: Arc<HashMap<String, Value>>,
    pub variable_map: Arc<HashMap<String, String>>, // UUID -> 变量名
    pub shadow_builder: ShadowBuilder,
    pub blocks: HashMap<String, Value>,
    pub connections: HashMap<String, HashMap<String, Value>>,
    // 布局游标:编译版无 location 时按树形自动排列积木,避免恢复产物全部重叠在 [0,0]
    pub layout_col: f64,
    pub layout_row: f64,
}

impl BlockContext {
    pub fn new(
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

    pub fn with_capacity(
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

    pub fn insert_connection(&mut self, source_id: &str, target_id: &str, connection_info: Value) {
        self.connections
            .entry(source_id.to_string())
            .or_default()
            .insert(target_id.to_string(), connection_info);
    }
}

// 积木反编译核心

pub struct BlockDecompilerCore<'a> {
    compiled: &'a Value,
    behavior: BlockBehavior,
}

impl<'a> BlockDecompilerCore<'a> {
    pub fn new(compiled: &'a Value, behavior: BlockBehavior) -> Self {
        Self { compiled, behavior }
    }

    pub fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let config = &context.shadow_builder.config;
        let id = self.compiled.get_str_or("id", "");
        let block_type = self.compiled.get_str_or("type", "");
        let is_shadow = config.shadow_types.contains(block_type);
        // 编辑版 is_output 与编译版 output_type 严格对应(0→false,2→true)
        let output_type = self.compiled.get_i64_or_default("output_type", 0);
        let is_output = is_shadow || output_type > 0;

        let location = self
            .compiled
            .get_array_opt("location")
            .map(|arr| Value::Array(arr.clone()))
            .unwrap_or_else(|| {
                // 编译版无 location:按树形自动布局,避免全部重叠在 [0,0]
                let loc = json!([context.layout_col, context.layout_row]);
                context.layout_row += 70.0;
                loc
            });

        let mut block_value = json!({
            "id": id,
            "type": block_type,
            "location": location,
            "is_shadow": is_shadow,
            "is_output": is_output,
            "collapsed": false,
            "disabled": false,
            "parent_id": null,
            "deletable": true,
            "movable": true,
            "editable": true,
            "visible": "visible",
            "fields": {},
            "field_constraints": {},
            "field_extra_attr": {},
            "comment": self.compiled.get("comment").cloned().unwrap_or(Value::Null),
            "mutation": "",
        });

        let mut shadows: HashMap<String, Value> = HashMap::new();

        self.process_next(context, &mut block_value)?;
        self.process_children(context, &mut shadows, &mut block_value)?;
        self.process_conditions(context, &mut shadows, &mut block_value)?;
        self.process_params(context, &mut shadows, &mut block_value)?;

        if let Some(obj) = block_value.as_object_mut() {
            let shadows_map: serde_json::Map<String, Value> = shadows.into_iter().collect();
            obj.insert("shadows".to_string(), Value::Object(shadows_map));
        }

        context.blocks.insert(id.to_string(), block_value.clone());
        Ok(block_value)
    }

    fn process_next(&self, context: &mut BlockContext, block_value: &mut Value) -> Result<()> {
        if let Some(next_compiled) = self.compiled.get("next_block")
            && !next_compiled.is_null()
        {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            // 树内块也经专用分派,保证 callnoreturn/controls_if 等专用反编译器生效
            let mut decompiler = create_block_decompiler(next_compiled);
            // 下一层链块向右缩进一个层级
            context.layout_col += 220.0;
            let next_block = decompiler.decompile(context)?;
            context.layout_col -= 220.0;
            let next_id = next_block
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("next_block缺少id".to_string()))?
                .to_string();
            context.blocks.insert(next_id.clone(), next_block);
            if let Some(b) = context.blocks.get_mut(&next_id)
                && let Some(o) = b.as_object_mut()
            {
                o.insert("parent_id".to_string(), json!(parent_id));
            }
            context.insert_connection(&parent_id, &next_id, json!({"type": "next"}));
        }
        Ok(())
    }

    fn process_children(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(children) = self.compiled.get("child_block").and_then(|v| v.as_array()) {
            let conditions_count = self
                .compiled
                .get("conditions")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);

            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (i, child) in children.iter().enumerate() {
                if !child.is_null() {
                    let mut decompiler = create_block_decompiler(child);
                    context.layout_col += 220.0;
                    let child_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let child_id = child_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("child_block缺少id".to_string())
                        })?
                        .to_string();
                    let input_name = self.behavior.get_child_input_name(i, conditions_count);
                    context.blocks.insert(child_id.clone(), child_block);
                    if let Some(b) = context.blocks.get_mut(&child_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &child_id,
                        json!({
                            "type": "input",
                            "input_type": "statement",
                            "input_name": input_name
                        }),
                    );
                    if let std::collections::hash_map::Entry::Vacant(e) = shadows.entry(input_name)
                    {
                        let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                        e.insert(shadow_value);
                    }
                }
            }
        }
        Ok(())
    }

    fn process_conditions(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(conditions) = self.compiled.get("conditions").and_then(|v| v.as_array()) {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (i, condition) in conditions.iter().enumerate() {
                let input_name = format!("IF{}", i);
                if condition.is_null() {
                    let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                    shadows.insert(input_name, shadow_value);
                } else {
                    let mut decompiler = create_block_decompiler(condition);
                    context.layout_col += 220.0;
                    let condition_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let cond_id = condition_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("condition_block缺少id".to_string())
                        })?
                        .to_string();
                    context.blocks.insert(cond_id.clone(), condition_block);
                    if let Some(b) = context.blocks.get_mut(&cond_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &cond_id,
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": input_name
                        }),
                    );
                    let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                    shadows.insert(input_name, shadow_value);
                }
            }
        }
        Ok(())
    }

    fn infer_shadow_type(&self, param_name: &str, value: &Value) -> &'static str {
        match param_name {
            "condition" | "BOOL" => "logic_empty",
            "message" | "MESSAGE" => "broadcast_input",
            "sound_id" | "SOUND" => "get_audios",
            "whole_sound" | "all_sounds" => "get_whole_audios",
            "style_id" | "costume" | "COSTUME" => "get_current_costume",
            "scene" | "SCENE" | "scene_id" => "get_current_scene",
            "list" | "LIST" => "lists_get",
            _ => match value {
                Value::String(_) => "text",
                Value::Bool(_) => "logic_boolean",
                _ => "math_number",
            },
        }
    }

    fn process_params(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        let block_type = self.compiled.get_str_or("type", "");
        // 过程定义/调用块的 params(参数名→参数块)由 FunctionDef/FunctionCallDecompiler
        // 单独处理,此处跳过避免双连接与 fields 污染
        if block_type == "procedures_2_defnoreturn"
            || block_type == "procedures_2_callnoreturn"
            || block_type == "procedures_2_callreturn"
        {
            return Ok(());
        }
        if let Some(params) = self.compiled.get_object_opt("params") {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (name, value) in params {
                if value.is_object() {
                    let mut decompiler = create_block_decompiler(value);
                    context.layout_col += 220.0;
                    let param_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let param_id = param_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("param_block缺少id".to_string())
                        })?
                        .to_string();
                    context.blocks.insert(param_id.clone(), param_block.clone());
                    if let Some(b) = context.blocks.get_mut(&param_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &param_id,
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": name
                        }),
                    );

                    let param_type = param_block
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if context
                        .shadow_builder
                        .config
                        .shadow_types
                        .contains(param_type)
                    {
                        // 编辑版 shadow 模板显示的是类型默认值(如 math_number 的 0),
                        // 与参数块实际值无关,因此不传 text
                        let shadow_value =
                            context
                                .shadow_builder
                                .create(param_type, Some(param_id.clone()), None);
                        shadows.insert(name.clone(), shadow_value);
                    } else {
                        let shadow_type = self.infer_shadow_type(name, &Value::Null);
                        let shadow_value = context.shadow_builder.create(shadow_type, None, None);
                        shadows.insert(name.clone(), shadow_value);
                    }
                } else {
                    // 处理基本类型参数(如变量 UUID 引用)
                    // 布尔开关参数(如 bump 的 warp)在编辑版中不呈现
                    // (无 shadow,无 fields),跳过以对齐编辑版格式
                    if value.is_boolean() {
                        continue;
                    }
                    if name == "VAR" {
                        // 编辑版格式:变量引用以 UUID 存入 fields(variables_set/get 均如此),
                        // 且不生成 shadow(编辑版变量块的 shadows 中无 VAR 键)
                        if let Some(fields) = block_value
                            .as_object_mut()
                            .and_then(|v| v.get_mut("fields").and_then(|v| v.as_object_mut()))
                        {
                            fields.insert(name.clone(), value.clone());
                        }
                        continue;
                    }
                    let shadow_type = self.infer_shadow_type(name, value);
                    let num_str;
                    let shadow_text = match value {
                        Value::String(s) => Some(s.as_str()),
                        Value::Number(n) => {
                            num_str = n.to_string();
                            Some(num_str.as_str())
                        }
                        _ => None,
                    };
                    let shadow_value =
                        context
                            .shadow_builder
                            .create(shadow_type, None, shadow_text);
                    shadows.insert(name.clone(), shadow_value);

                    if let Some(fields) = block_value
                        .as_object_mut()
                        .and_then(|v| v.get_mut("fields").and_then(|v| v.as_object_mut()))
                    {
                        fields.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        Ok(())
    }
}

// 反编译器上下文
pub struct DecompilerContext {
    pub work_info: WorkInfo,
    pub http_client: Box<dyn HttpClient>,
    pub file_service: FileService,
    pub id_generator: IdGenerator,
    pub config: Arc<DecompilerConfig>,
}

// Context Builder
pub struct DecompilerContextBuilder {
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
    pub fn new() -> Self {
        Self {
            work_info: None,
            http_client: None,
            config: None,
            file_service: None,
            id_generator: None,
        }
    }

    pub fn work_info(mut self, info: WorkInfo) -> Self {
        self.work_info = Some(info);
        self
    }

    pub fn http_client(mut self, client: Box<dyn HttpClient>) -> Self {
        self.http_client = Some(client);
        self
    }

    pub fn config(mut self, config: Arc<DecompilerConfig>) -> Self {
        self.config = Some(config);
        self
    }

    pub fn file_service(mut self, service: FileService) -> Self {
        self.file_service = Some(service);
        self
    }

    pub fn id_generator(mut self, generator: IdGenerator) -> Self {
        self.id_generator = Some(generator);
        self
    }

    pub fn build(self) -> Result<DecompilerContext> {
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
pub enum DecompileResult {
    Json(Value),
    Path(String),
}

pub enum RawWorkData {
    Kitten(Arc<Value>),
    NekoEncrypted(String),
    Nemo(Arc<Value>, Arc<Value>),
    Wood(Arc<Value>),
    Coco(Arc<Value>),
}

pub trait WorkFetcher: Send + Sync {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData>;
}

pub trait WorkDecompiler: Send + Sync {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult>;
    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<String>;
}

// NEKO
pub struct NekoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NekoFetcher {
    pub fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
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

pub struct NekoDecompiler {
    crypto_service: CryptoService,
}

impl NekoDecompiler {
    pub fn new(salt: &[u8]) -> Self {
        Self {
            crypto_service: CryptoService::new(salt),
        }
    }
}

impl WorkDecompiler for NekoDecompiler {
    fn decompile(&self, raw: RawWorkData, _context: &DecompilerContext) -> Result<DecompileResult> {
        match raw {
            RawWorkData::NekoEncrypted(encrypted) => {
                let decryptor = BCMKNDecryptor::new(self.crypto_service.clone());
                let decrypted_json = decryptor.decrypt(&encrypted)?;
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
    ) -> Result<String> {
        match result {
            DecompileResult::Json(json) => {
                let output_path = output_dir.unwrap_or(&context.config.default_output_dir);
                FileService::ensure_dir(output_path)?;
                let filename = FileService::safe_filename(
                    &context.work_info.name,
                    context.work_info.id,
                    "bcmkn",
                );
                let filepath = output_path.join(filename);
                FileService::write_json(&filepath, json)?;
                Ok(filepath.to_string_lossy().to_string())
            }
            _ => Err(DecompilerError::Decompile(
                "NekoDecompiler 应返回 JSON".into(),
            )),
        }
    }
}

// KITTEN
pub struct KittenFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl KittenFetcher {
    pub fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for KittenFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let url = format!(
            "{}/kitten/r2/work/player/load/{}",
            self.config.creation_base_url, work_info.id
        );
        let data = self.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;
        let compiled = self.http_client.get_json(compiled_url, None)?;
        Ok(RawWorkData::Kitten(Arc::new(compiled)))
    }
}

pub struct KittenDecompiler;

impl KittenDecompiler {
    fn get_actor_info(work: &Value, actor_id: &str) -> Value {
        if let Some(theatre) = work.get("theatre").and_then(|v| v.as_object()) {
            if let Some(actors) = theatre.get("actors").and_then(|v| v.as_object())
                && let Some(actor) = actors.get(actor_id)
            {
                return actor.clone();
            }
            if let Some(scenes) = theatre.get("scenes").and_then(|v| v.as_object())
                && let Some(scene) = scenes.get(actor_id)
            {
                return scene.clone();
            }
        }
        let short_id = if actor_id.len() > 8 {
            &actor_id[..8]
        } else {
            actor_id
        };
        json!({
            "direction": 90,
            "draggable": false,
            "id": actor_id,
            "name": format!("未知角色_{}", short_id),
            "rotation_style": "all around",
            "size": 100,
            "type": "sprite",
            "visible": true,
            "x": 0,
            "y": 0,
        })
    }

    fn decompile_actor_blocks(
        config: &Arc<DecompilerConfig>,
        id_generator: &IdGenerator,
        actor_compiled: &Value,
        functions: &Arc<HashMap<String, Value>>,
        actor_info: Value,
        variable_map: Arc<HashMap<String, String>>,
        work_type: WorkType,
    ) -> Result<Value> {
        let shadow_builder = ShadowBuilder::new(config.clone(), id_generator.clone(), work_type);
        let compiled_blocks = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object());
        let estimated_blocks = compiled_blocks.map(|m| m.len() * 10 + 100).unwrap_or(256);
        let functions_arc = Arc::clone(functions);
        // 移动前先提取 actor_info 中已有的注释,供注释回退使用
        let actor_existing_comments = actor_info
            .get("block_data_json")
            .and_then(|b| b.get("comments"))
            .cloned();
        let mut context = BlockContext::with_capacity(
            actor_info,
            functions_arc,
            shadow_builder,
            variable_map,
            estimated_blocks,
            estimated_blocks * 2,
        );

        let factory = BlockDecompilerFactory::new(config.as_ref(), id_generator);

        if let Some(blocks) = compiled_blocks {
            let mut referenced_ids: HashSet<String> = HashSet::new();
            for (_, block) in blocks {
                if let Some(next) = block.get("next_block") {
                    if let Some(id) = next.as_str() {
                        referenced_ids.insert(id.to_string());
                    } else if let Some(obj) = next.as_object()
                        && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                    {
                        referenced_ids.insert(id.to_string());
                    }
                }
                if let Some(children) = block.get("child_block").and_then(|v| v.as_array()) {
                    for child in children {
                        if let Some(id) = child.as_str() {
                            referenced_ids.insert(id.to_string());
                        } else if let Some(obj) = child.as_object()
                            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                        {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(conditions) = block.get("conditions").and_then(|v| v.as_array()) {
                    for cond in conditions {
                        if let Some(id) = cond.as_str() {
                            referenced_ids.insert(id.to_string());
                        } else if let Some(obj) = cond.as_object()
                            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                        {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(params) = block.get("params").and_then(|v| v.as_object()) {
                    for (_, param_value) in params {
                        if let Some(obj) = param_value.as_object()
                            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                        {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
            }

            for (id, block_data) in blocks {
                if !referenced_ids.contains(id) {
                    // 根块之间增加垂直间距,避免自动布局后挤在一起
                    context.layout_row += 50.0;
                    let mut decompiler = factory.create(block_data);
                    // 重新插入补充后的块(If/FunctionDef 等会修改 block_value)
                    let block_value = decompiler.decompile(&mut context)?;
                    if let Some(bid) = block_value.get("id").and_then(|v| v.as_str()) {
                        context.blocks.insert(bid.to_string(), block_value);
                    }
                }
            }
        }

        // 生成函数定义块(procedures_2_defnoreturn),否则调用块会因找不到
        // 定义而被 FunctionCallDecompiler 置为 disabled,函数功能丢失
        // 独立于 compiled_block_map,避免其缺失时连带丢失函数定义
        if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
            for (_, func_data) in procedures {
                context.layout_row += 50.0;
                let mut decompiler = factory.create(func_data);
                // 重新插入:FunctionDefDecompiler 补充的 shadows/mutation/NAME 需覆盖 core 版本
                let block_value = decompiler.decompile(&mut context)?;
                if let Some(bid) = block_value.get("id").and_then(|v| v.as_str()) {
                    context.blocks.insert(bid.to_string(), block_value);
                }
            }
        }

        // 优先使用 compile_result 中的注释;若数据源未提供,则保留 actor_info
        // 中已有的注释,避免反编译覆盖掉输入中已有的注释数据
        let mut comments = actor_compiled
            .get("comments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if comments.as_object().map(|o| o.is_empty()).unwrap_or(true)
            && let Some(existing) = actor_existing_comments
        {
            comments = existing;
        }

        let mut actor_data = context.actor_data;
        if let Some(obj) = actor_data.as_object_mut() {
            obj.insert(
                "block_data_json".to_string(),
                json!({
                    "blocks": context.blocks,
                    "connections": context.connections,
                    "comments": comments,
                }),
            );
        }
        Ok(actor_data)
    }

    fn decompile_scene_blocks(
        config: &Arc<DecompilerConfig>,
        id_generator: &IdGenerator,
        actor_compiled: &Value,
        scene_info: &Value,
        work_type: WorkType,
        functions: &Arc<HashMap<String, Value>>,
    ) -> Result<Value> {
        let shadow_builder = ShadowBuilder::new(config.clone(), id_generator.clone(), work_type);
        let compiled_blocks = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object());
        let estimated_blocks = compiled_blocks.map(|m| m.len() * 10 + 100).unwrap_or(256);
        // 场景同样使用全局函数表:函数可定义在某个场景(屏幕角色)中,
        // 被其它场景/角色调用(如"总移动设置4"定义在背景(3),调用在背景(1)),
        // 否则场景中的调用块会因找不到定义而被禁用
        let mut context = BlockContext::with_capacity(
            json!({}),
            Arc::clone(functions),
            shadow_builder,
            Arc::new(HashMap::new()), // 场景没有变量映射
            estimated_blocks,
            estimated_blocks * 2,
        );

        let factory = BlockDecompilerFactory::new(config.as_ref(), id_generator);

        if let Some(blocks) = compiled_blocks {
            let mut referenced_ids: HashSet<String> = HashSet::new();
            for (_, block) in blocks {
                if let Some(next) = block.get("next_block") {
                    if let Some(id) = next.as_str() {
                        referenced_ids.insert(id.to_string());
                    } else if let Some(obj) = next.as_object()
                        && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                    {
                        referenced_ids.insert(id.to_string());
                    }
                }
                if let Some(children) = block.get("child_block").and_then(|v| v.as_array()) {
                    for child in children {
                        if let Some(id) = child.as_str() {
                            referenced_ids.insert(id.to_string());
                        } else if let Some(obj) = child.as_object()
                            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                        {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(conditions) = block.get("conditions").and_then(|v| v.as_array()) {
                    for cond in conditions {
                        if let Some(id) = cond.as_str() {
                            referenced_ids.insert(id.to_string());
                        } else if let Some(obj) = cond.as_object()
                            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                        {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(params) = block.get("params").and_then(|v| v.as_object()) {
                    for (_, param_value) in params {
                        if let Some(obj) = param_value.as_object()
                            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                        {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
            }

            for (id, block_data) in blocks {
                if !referenced_ids.contains(id) {
                    // 根块之间增加垂直间距,避免自动布局后挤在一起
                    context.layout_row += 50.0;
                    let mut decompiler = factory.create(block_data);
                    let block_value = decompiler.decompile(&mut context)?;
                    if let Some(bid) = block_value.get("id").and_then(|v| v.as_str()) {
                        context.blocks.insert(bid.to_string(), block_value);
                    }
                }
            }
        }

        // 生成函数定义块(procedures_2_defnoreturn).函数可能定义在场景
        // (屏幕角色)中(如"总移动设置4"定义在背景(3)),与角色分支一致,
        // 否则场景中定义的函数缺失,调用块会被 FunctionCallDecompiler 禁用
        if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
            for (_, func_data) in procedures {
                context.layout_row += 50.0;
                let mut decompiler = factory.create(func_data);
                // 重新插入:FunctionDefDecompiler 补充的 shadows/mutation/NAME 需覆盖 core 版本
                let block_value = decompiler.decompile(&mut context)?;
                if let Some(bid) = block_value.get("id").and_then(|v| v.as_str()) {
                    context.blocks.insert(bid.to_string(), block_value);
                }
            }
        }

        // 优先使用 compile_result 中的注释;若数据源未提供,则保留 scene_info
        // 中已有的注释,避免反编译覆盖掉输入中已有的注释数据
        let mut comments = actor_compiled
            .get("comments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if comments.as_object().map(|o| o.is_empty()).unwrap_or(true)
            && let Some(existing) = scene_info
                .get("block_data_json")
                .and_then(|b| b.get("comments"))
        {
            comments = existing.clone();
        }

        let mut scene = scene_info.clone();
        if let Some(obj) = scene.as_object_mut() {
            obj.insert(
                "block_data_json".to_string(),
                json!({
                    "blocks": context.blocks,
                    "connections": context.connections,
                    "comments": comments,
                }),
            );
        }
        Ok(scene)
    }

    fn update_work_info(
        work: &mut Value,
        work_info: &WorkInfo,
        config: &DecompilerConfig,
    ) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;

        let feature_keys = [
            "physics2",
            "cloud_variable",
            "cloud_list",
            "ai_lab",
            "camera",
            "video",
            "midimusic",
        ];
        let mut original_features = serde_json::Map::new();
        for key in &feature_keys {
            if let Some(val) = work_obj.get(*key) {
                original_features.insert((*key).to_string(), val.clone());
            }
        }

        work_obj.insert(
            "hidden_toolbox".to_string(),
            json!({
                "toolbox": [],
                "blocks": [],
            }),
        );
        // Kitten3 编辑版(如春风得意)work_source_label 为 6,且无 sample_id/设备/最后工具箱等字段
        let is_k3 = matches!(work_info.work_type, WorkType::Kitten2 | WorkType::Kitten3);
        work_obj.insert(
            "work_source_label".to_string(),
            json!(if is_k3 { 6 } else { 1 }),
        );
        if is_k3 {
            // Kitten3 编辑版(如春风得意)顶层含 work_business 字段
            work_obj.insert("work_business".to_string(), json!(0));
        }
        if !is_k3 {
            work_obj.insert("sample_id".to_string(), json!(""));
            work_obj.insert("codemao_value".to_string(), json!(work_info.id.to_string()));
            work_obj.insert("device_widget_type".to_string(), Value::Null);
        }
        work_obj.insert("project_name".to_string(), json!(work_info.name));
        work_obj.insert(
            "toolbox_order".to_string(),
            json!(config.toolbox_categories),
        );
        if !is_k3 {
            work_obj.insert(
                "last_toolbox_order".to_string(),
                json!(config.toolbox_categories),
            );
        }

        for (k, v) in original_features {
            work_obj.insert(k, v);
        }
        Ok(())
    }

    fn clean_work_data(work: &mut Value, work_type: WorkType) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;
        let keys_to_remove = ["compile_result", "preview", "author_nickname"];
        for key in &keys_to_remove {
            work_obj.remove(*key);
        }
        // 清理编译版 theatre 的运行时字段:Kitten4 编辑版无这些键,
        // 但 Kitten3 编辑版(如春风得意)保留 current_entity/current_scene/style_collections
        if !matches!(work_type, WorkType::Kitten2 | WorkType::Kitten3)
            && let Some(theatre) = work.get_mut("theatre").and_then(|t| t.as_object_mut())
        {
            for key in ["current_entity", "current_scene", "style_collections"] {
                theatre.remove(key);
            }
        }
        Ok(())
    }

    fn restore_global_fields(
        work: &mut Value,
        restore_fields: &HashMap<&'static str, Value>,
        restore_groups: Option<&Value>,
    ) {
        for (key, val) in restore_fields {
            work[key] = val.clone();
        }

        if let Some(groups) = restore_groups
            && let Some(theatre) = work.get_mut("theatre")
            && let Some(obj) = theatre.as_object_mut()
        {
            obj.insert("groups".to_string(), groups.clone());
        }
    }
}

// Kitten2/3 blocksXML 序列化器
/// Kitten2/3 编辑版(如春风得意)以 Blockly XML 字符串(blocksXML)存储积木
/// 与 Kitten4 的 block_data_json(blocks/connections)不同,本组件负责把编译块树
/// 序列化为 Blockly XML,独立成组件便于单独测试与复用
pub struct XmlBlockWriter<'a> {
    config: &'a DecompilerConfig,
}

impl<'a> XmlBlockWriter<'a> {
    pub fn new(config: &'a DecompilerConfig) -> Self {
        Self { config }
    }

    /// 生成 actor/场景的 blocksXML(`<variables></variables>` + 各根块)
    pub fn write_blocks(&self, actor_compiled: &Value) -> Result<String> {
        let mut xml = String::from("<variables></variables>");
        let compiled_blocks = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object());
        if let Some(blocks) = compiled_blocks {
            // 收集被引用的块 id,只将顶层根块作为独立 XML 块输出
            let mut referenced_ids: HashSet<String> = HashSet::new();
            for (_, block) in blocks {
                if let Some(next) = block.get("next_block")
                    && let Some(id) = next.get("id").and_then(|v| v.as_str())
                {
                    referenced_ids.insert(id.to_string());
                }
                if let Some(children) = block.get("child_block").and_then(|v| v.as_array()) {
                    for child in children {
                        if let Some(id) = child.get("id").and_then(|v| v.as_str()) {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(conds) = block.get("conditions").and_then(|v| v.as_array()) {
                    for c in conds {
                        if let Some(id) = c.get("id").and_then(|v| v.as_str()) {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(params) = block.get("params").and_then(|v| v.as_object()) {
                    for (_, pv) in params {
                        if let Some(id) = pv.get("id").and_then(|v| v.as_str()) {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
            }
            let mut y = 0.0;
            for (id, block) in blocks {
                if !referenced_ids.contains(id) {
                    xml.push_str(&self.block_xml(block, true, y));
                    y += 220.0;
                }
            }
        }
        Ok(xml)
    }

    /// 将编译块树的单个块渲染为 Blockly XML
    fn block_xml(&self, compiled: &Value, is_root: bool, y: f64) -> String {
        let bt = compiled.get_str_or("type", "");
        let bid = compiled.get_str_or("id", "");
        let mut s = if is_root {
            format!(
                r#"<block type="{}" id="{}" inline="true" visible="visible" x="0" y="{}">"#,
                bt, bid, y
            )
        } else {
            format!(
                r#"<block type="{}" id="{}" inline="true" visible="visible">"#,
                bt, bid
            )
        };

        // fields:params 标量
        let mut field_xml = String::new();
        let mut value_xml = String::new();
        if let Some(params) = compiled.get_object_opt("params") {
            for (k, v) in params {
                if !v.is_object() && !v.is_array() {
                    field_xml.push_str(&format!(
                        r#"<field name="{}">{}</field>"#,
                        k,
                        Self::escape_text(&Self::value_to_text(v))
                    ));
                }
            }
            // value 插槽:params 对象
            for (k, v) in params {
                if v.is_object() {
                    value_xml.push_str(&format!(r#"<value name="{}">"#, k));
                    value_xml.push_str(&self.value_xml(v));
                    value_xml.push_str("</value>");
                }
            }
        }
        s.push_str(&field_xml);
        // value 插槽先于 statement(编辑版如 self_listen 为 <value>...<statement>)
        s.push_str(&value_xml);

        // conditions → <value name="IF{i}">
        let conditions = compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for (i, c) in conditions.iter().enumerate() {
            if c.is_object() {
                s.push_str(&format!(r#"<value name="IF{}">"#, i));
                s.push_str(&self.value_xml(c));
                s.push_str("</value>");
            }
        }

        // child_block → <statement name="...">
        if let Some(children) = compiled.get("child_block").and_then(|v| v.as_array()) {
            for (i, c) in children.iter().enumerate() {
                if !c.is_object() {
                    continue;
                }
                let name = match bt {
                    "controls_if" | "controls_if_no_else" => {
                        if i < conditions.len() {
                            format!("DO{}", i)
                        } else {
                            "ELSE".to_string()
                        }
                    }
                    "procedures_2_defnoreturn" => "STACK".to_string(),
                    _ => "DO".to_string(),
                };
                s.push_str(&format!(r#"<statement name="{}">"#, name));
                s.push_str(&self.block_xml(c, false, 0.0));
                s.push_str("</statement>");
            }
        }

        // next 链
        if let Some(nb) = compiled.get("next_block")
            && nb.is_object()
        {
            s.push_str("<next>");
            s.push_str(&self.block_xml(nb, false, 0.0));
            s.push_str("</next>");
        }

        s.push_str("</block>");
        s
    }

    /// value 插槽内容:shadow 类型渲染为 `<shadow>`,否则递归为 `<block>`
    fn value_xml(&self, v: &Value) -> String {
        let vt = v.get_str_or("type", "");
        let vid = v.get_str_or("id", "");
        if self.config.shadow_types.contains(vt) {
            let mut s = format!(r#"<shadow type="{}" id="{}" visible="visible">"#, vt, vid);
            if let Some(params) = v.get_object_opt("params") {
                for (k, fv) in params {
                    if !fv.is_object() && !fv.is_array() {
                        s.push_str(&format!(
                            r#"<field name="{}">{}</field>"#,
                            k,
                            Self::escape_text(&Self::value_to_text(fv))
                        ));
                    }
                }
            }
            s.push_str("</shadow>");
            s
        } else {
            self.block_xml(v, false, 0.0)
        }
    }

    /// XML 转义
    fn escape_text(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    fn value_to_text(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        }
    }
}

impl WorkDecompiler for KittenDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let work_arc = match raw {
            RawWorkData::Kitten(data) => data,
            _ => {
                return Err(DecompilerError::Decompile(
                    "KittenDecompiler 只能处理 Kitten 数据".into(),
                ));
            }
        };

        // 提取需要恢复的全局字段,仅克隆这 13 个字段而非整份作品 JSON
        // 反编译会重写 work 的 theatre 与各角色积木,但 variables/lists/broadcasts 等
        // 顶层全局字段必须保留原值,故先在 try_unwrap 之前借 work_arc 读出
        // (try_unwrap 会消耗 work_arc,若先解包则无法再借用原始数据)
        // 作品 JSON 的主体是各角色的 blocks/block_data_json,恢复逻辑用不到,
        // 只克隆这些字段可避免数十 MB 的整份深拷贝
        let mut restore_fields: HashMap<&'static str, Value> = HashMap::new();
        let mut restore_groups: Option<Value> = None;
        {
            let original = work_arc.as_ref();
            for key in [
                "variables",
                "lists",
                "broadcasts",
                "audio",
                "matrix",
                "models",
                "physics2",
                "cloud_variable",
                "cloud_list",
                "ai_lab",
                "camera",
                "video",
                "midimusic",
            ] {
                if let Some(val) = original.get(key) {
                    restore_fields.insert(key, val.clone());
                }
            }
            restore_groups = original
                .get("theatre")
                .and_then(|t| t.get("groups"))
                .cloned();
        }
        let mut work = Arc::try_unwrap(work_arc).unwrap_or_else(|arc| (*arc).clone());

        // 编译产物数组是反编译的输入,但最终会被 clean_work_data 从输出中删除,
        // 因此直接 take 移出获得所有权(零拷贝),既作为只读输入又避免整数组深拷贝
        let compile_result = work
            .get_mut("compile_result")
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .ok_or_else(|| DecompilerError::InvalidResponse("compile_result不存在".to_string()))?;

        // 从全局 variables 构建 UUID -> 变量名映射
        let mut global_variable_map = HashMap::new();
        if let Some(vars) = work.get("variables").and_then(|v| v.as_object()) {
            for (uuid, var_info) in vars {
                if let Some(name) = var_info.get("name").and_then(|v| v.as_str()) {
                    global_variable_map.insert(uuid.clone(), name.to_string());
                }
            }
        }
        // 所有角色/场景共享同一份映射,避免每角色深拷贝
        let global_variable_map = Arc::new(global_variable_map);

        let work_type = context.work_info.work_type;
        // Kitten2/3 编辑版用 blocksXML(Blockly XML 字符串),Kitten4 用 block_data_json
        let use_blocks_xml = matches!(work_type, WorkType::Kitten2 | WorkType::Kitten3);

        // 全局函数表:过程可在一个角色(如 Function)中定义,被其它角色调用,
        // 因此合并所有 compile_result 的 procedures,否则跨角色调用会被禁用
        let mut global_functions: HashMap<String, Value> = HashMap::new();
        for actor_compiled in &compile_result {
            if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
                for (name, func_data) in procedures {
                    global_functions.insert(name.clone(), func_data.clone());
                }
            }
        }
        // 所有角色/场景共享同一份函数表,避免每角色深拷贝
        let global_functions = Arc::new(global_functions);

        // 将 scenes 整表移出 work,处理完再写回
        // 场景反编译需要同时持有 scene 数据(只读)与 theatre 引用(写回),
        // 直接借用会造成 work 的不可变/可变借用冲突,逐场景克隆则浪费整份深拷贝
        // 移出后 scenes 与 work 相互独立,读写无冲突,且 is_scene 判断也复用同一份表
        let had_scenes = work
            .get("theatre")
            .and_then(|t| t.get("scenes"))
            .and_then(|s| s.as_object())
            .is_some();
        let mut scenes = work
            .get_mut("theatre")
            .and_then(|t| t.get_mut("scenes"))
            .and_then(|s| s.as_object_mut())
            .map(std::mem::take)
            .unwrap_or_default();

        for actor_compiled in &compile_result {
            let actor_id = actor_compiled.get_str_or("id", "");

            let is_scene = scenes.contains_key(actor_id);

            if is_scene {
                if use_blocks_xml {
                    // Kitten2/3:生成 blocksXML 字符串
                    let xml = XmlBlockWriter::new(context.config.as_ref())
                        .write_blocks(actor_compiled)
                        .with_context(|| format!("反编译场景 {} 失败", actor_id))?;
                    if let Some(scene) = scenes.get_mut(actor_id).and_then(|v| v.as_object_mut()) {
                        scene.insert("blocksXML".to_string(), Value::String(xml));
                    }
                } else {
                    let scene_info = &scenes[actor_id];
                    let updated_scene = Self::decompile_scene_blocks(
                        &context.config,
                        &context.id_generator,
                        actor_compiled,
                        scene_info,
                        work_type,
                        &global_functions,
                    )
                    .with_context(|| format!("反编译场景 {} 失败", actor_id))?;
                    scenes.insert(actor_id.to_string(), updated_scene);
                }
            } else {
                if use_blocks_xml {
                    // Kitten2/3:生成 blocksXML 字符串
                    let xml = XmlBlockWriter::new(context.config.as_ref())
                        .write_blocks(actor_compiled)
                        .with_context(|| format!("反编译角色 {} 失败", actor_id))?;
                    if let Some(actors) = work
                        .get_mut("theatre")
                        .and_then(|t| t.get_mut("actors"))
                        .and_then(|a| a.as_object_mut())
                        && let Some(actor) =
                            actors.get_mut(actor_id).and_then(|v| v.as_object_mut())
                    {
                        actor.insert("blocksXML".to_string(), Value::String(xml));
                    }
                } else {
                    let actor_info = Self::get_actor_info(&work, actor_id);
                    // 角色也使用全局变量映射
                    let updated_actor = Self::decompile_actor_blocks(
                        &context.config,
                        &context.id_generator,
                        actor_compiled,
                        &global_functions,
                        actor_info,
                        Arc::clone(&global_variable_map),
                        work_type,
                    )
                    .with_context(|| format!("反编译角色 {} 失败", actor_id))?;

                    if let Some(actors) = work
                        .get_mut("theatre")
                        .and_then(|t| t.get_mut("actors"))
                        .and_then(|a| a.as_object_mut())
                    {
                        actors.insert(actor_id.to_string(), updated_actor);
                    }
                }
            }
        }

        // 写回处理后的 scenes
        if had_scenes
            && let Some(theatre) = work.get_mut("theatre").and_then(|t| t.as_object_mut())
        {
            theatre.insert("scenes".to_string(), Value::Object(scenes));
        }

        Self::update_work_info(&mut work, &context.work_info, context.config.as_ref())?;
        Self::clean_work_data(&mut work, work_type)?;
        Self::restore_global_fields(&mut work, &restore_fields, restore_groups.as_ref());

        Ok(DecompileResult::Json(work))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<String> {
        match result {
            DecompileResult::Json(json) => {
                let output_path = output_dir.unwrap_or(&context.config.default_output_dir);
                FileService::ensure_dir(output_path)?;
                let filename = FileService::safe_filename(
                    &context.work_info.name,
                    context.work_info.id,
                    context
                        .work_info
                        .file_extension(&context.config)
                        .trim_start_matches('.'),
                );
                let filepath = output_path.join(filename);
                FileService::write_json(&filepath, json)?;
                Ok(filepath.to_string_lossy().to_string())
            }
            _ => Err(DecompilerError::Decompile(
                "KITTEN反编译器应返回JSON".to_string(),
            )),
        }
    }
}

// NEMO
pub struct NemoResourceConfig<'a> {
    pub http_client: &'a dyn HttpClient,
    pub file_service: &'a FileService,
    pub work_id: i64,
}

pub struct NemoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NemoFetcher {
    pub fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
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

pub struct NemoDecompiler;

impl NemoDecompiler {
    fn decompile_inner(
        context: &DecompilerContext,
        bcm_data: Arc<Value>,
        source_info: Arc<Value>,
    ) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id, "");
        let base_dir = &context.config.default_output_dir;
        let work_dir = base_dir.join(folder_name);

        let resource_config = NemoResourceConfig {
            http_client: &*context.http_client,
            file_service: &context.file_service,
            work_id,
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
    ) -> Result<String> {
        match result {
            DecompileResult::Path(path) => Ok(path.clone()),
            _ => Err(DecompilerError::Decompile(
                "NEMO反编译器应返回路径".to_string(),
            )),
        }
    }
}

pub struct NemoResourceManager<'a> {
    config: NemoResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
    sha_cache: RefCell<HashMap<String, String>>,
}

impl<'a> NemoResourceManager<'a> {
    pub fn new(config: NemoResourceConfig<'a>, work_dir: PathBuf) -> Self {
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

    pub fn create_directories(&mut self) -> Result<&HashMap<String, PathBuf>> {
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

    pub fn save_core_files(&self, bcm_data: &Value, source_info: &Value) -> Result<()> {
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
                "work_id": self.config.work_id,
                "have_uploaded": 2,
            },
        }))
    }

    pub fn download_resources(&self, bcm_data: &Value) -> Result<()> {
        let material_dir = self
            .dirs
            .get("material")
            .ok_or_else(|| DecompilerError::Other {
                msg: "material目录不存在".to_string(),
                source: None,
            })?;

        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for style_data in styles.values() {
                if let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) {
                    match self.config.http_client.get_binary(image_url) {
                        Ok(image_data) => {
                            let sha_hash = self.get_sha(image_url);
                            let image_path = material_dir.join(format!("{}.webp", sha_hash));
                            FileService::write_binary(&image_path, &image_data)?;
                        }
                        Err(e) => warn!("资源下载失败 {}: {}", image_url, e),
                    }
                }
            }
        }
        Ok(())
    }
}

// WOOD
pub struct WoodResourceConfig<'a> {
    pub http_client: &'a dyn HttpClient,
    pub file_service: &'a FileService,
    pub work_id: i64,
}

pub struct WoodFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl WoodFetcher {
    pub fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
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

pub struct WoodDecompiler;

impl WoodDecompiler {
    fn decompile_inner(context: &DecompilerContext, work_data: Arc<Value>) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id, "");
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
    ) -> Result<String> {
        match result {
            DecompileResult::Path(path) => Ok(path.clone()),
            _ => Err(DecompilerError::Decompile(
                "WOOD反编译器应返回路径".to_string(),
            )),
        }
    }
}

pub struct WoodResourceManager<'a> {
    config: WoodResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
}

impl<'a> WoodResourceManager<'a> {
    pub fn new(config: WoodResourceConfig<'a>, work_dir: PathBuf) -> Self {
        Self {
            config,
            work_dir,
            dirs: HashMap::new(),
        }
    }

    pub fn create_directories(&mut self) -> Result<&HashMap<String, PathBuf>> {
        self.dirs
            .insert("root".to_string(), FileService::ensure_dir(&self.work_dir)?);
        self.dirs.insert(
            "images".to_string(),
            FileService::ensure_dir(&self.work_dir.join("images"))?,
        );
        Ok(&self.dirs)
    }

    pub fn save_work_files(&self, work_data: &Value) -> Result<()> {
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
            "code_visible": work_data.get("code_visible").and_then(|v| v.as_bool()).unwrap_or(true),
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

// COCO
pub struct CocoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl CocoFetcher {
    pub fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
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

pub struct CocoDecompiler;

impl CocoDecompiler {
    fn reorganize(work: &mut Value, context: &DecompilerContext) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;

        let mut widget_map = work_obj
            .remove("widgetMap")
            .filter(|v| v.is_object())
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let screen_list = work_obj
            .remove("screenList")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();

        work_obj.insert("authorId".to_string(), json!(context.work_info.user_id));
        work_obj.insert("title".to_string(), json!(context.work_info.name));
        work_obj.insert("screens".to_string(), json!({}));
        work_obj.insert("screenIds".to_string(), json!([]));

        let mut screens = serde_json::Map::new();
        let mut screen_ids = Vec::with_capacity(screen_list.len());

        for mut screen in screen_list {
            let screen_obj = screen
                .as_object_mut()
                .ok_or_else(|| DecompilerError::Decompile("screen不是对象".to_string()))?;
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
            screens.insert(screen_id.clone(), Value::Object(screen_obj.clone()));
            screen_ids.push(Value::String(screen_id));
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
    ) -> Result<String> {
        match result {
            DecompileResult::Json(json) => {
                let output_path = output_dir.unwrap_or(&context.config.default_output_dir);
                FileService::ensure_dir(output_path)?;
                let filename = FileService::safe_filename(
                    &context.work_info.name,
                    context.work_info.id,
                    context
                        .work_info
                        .file_extension(&context.config)
                        .trim_start_matches('.'),
                );
                let filepath = output_path.join(filename);
                FileService::write_json(&filepath, json)?;
                Ok(filepath.to_string_lossy().to_string())
            }
            _ => Err(DecompilerError::Decompile(
                "COCO反编译器应返回JSON".to_string(),
            )),
        }
    }
}

// 积木反编译器 trait 与具体实现
pub trait BlockDecompiler<'a>: Send + Sync {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value>;
}

pub struct DefaultBlockDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
}

impl<'a> DefaultBlockDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
        }
    }
}

impl<'a> BlockDecompiler<'a> for DefaultBlockDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        self.core.decompile(context)
    }
}

pub struct IfBlockDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> IfBlockDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        let conditions_count = compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);
        let behavior = BlockBehavior::If { conditions_count };
        let core = BlockDecompilerCore::new(compiled, behavior);
        Self { core, compiled }
    }
}

impl<'a> BlockDecompiler<'a> for IfBlockDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let children = self
            .compiled
            .get("child_block")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DecompilerError::Decompile("child_block不存在".to_string()))?;
        let conditions_len = self
            .compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);

        // 根据方案8.1 修正 else 属性的判断
        let has_else = children.len() > conditions_len
            && !children.last().map(|v| v.is_null()).unwrap_or(true);

        if let Some(obj) = block_value.as_object_mut() {
            let mut shadows_mut = obj.get_mut("shadows").and_then(|s| s.as_object_mut());
            if let Some(shadows) = shadows_mut.as_mut() {
                if !has_else {
                    shadows.insert("EXTRA_ADD_ELSE".to_string(), json!(""));
                } else {
                    // 编辑版:有 else 时 shadows 同时含 ELSE_TEXT 与 ELSE
                    shadows.insert("ELSE_TEXT".to_string(), json!(""));
                    shadows.insert("ELSE".to_string(), json!(""));
                }
            }
            // 编辑版:有 else 时 mutation 标记 else="1",无 else 时为空字符串
            if has_else {
                let mutation =
                    r#"<mutation xmlns="http://www.w3.org/1999/xhtml" else="1"></mutation>"#
                        .to_string();
                obj.insert("mutation".to_string(), Value::String(mutation));
            }
        }
        Ok(block_value)
    }
}

pub struct TextJoinDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> TextJoinDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for TextJoinDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let param_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map(|obj| obj.len())
            .unwrap_or(0);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, param_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub struct AskAndChooseDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> AskAndChooseDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for AskAndChooseDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let item_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map(|obj| obj.len())
            .unwrap_or(0);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, item_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub struct SetEntityShowHideDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> SetEntityShowHideDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for SetEntityShowHideDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let need_text = self
            .compiled
            .get("need_text")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let time_block_id = self
            .compiled
            .get("time")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mutation = format!(
            r#"<mutation need_text="{}" time="{}"></mutation>"#,
            need_text, time_block_id
        );
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub struct TextSelectChangeableDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> TextSelectChangeableDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for TextSelectChangeableDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let item_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map(|obj| obj.len())
            .unwrap_or(0);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, item_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub struct FunctionDefDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> FunctionDefDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            // 函数体 child_block 使用 STACK 插槽
            core: BlockDecompilerCore::new(compiled, BlockBehavior::FunctionBody),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for FunctionDefDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let procedure_name = self.compiled.get_str_or("procedure_name", "");
        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let block = block_value
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("block_value不是对象".to_string()))?;

        if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
            // 编辑版 defnoreturn shadows 键集合:DEFINE / PARAMS0..n / MUTATOR / STACK
            shadows.insert("PROCEDURES_2_DEFNORETURN_DEFINE".to_string(), json!(""));
            shadows.insert("PROCEDURES_2_DEFNORETURN_MUTATOR".to_string(), json!(""));
            shadows.insert("STACK".to_string(), json!(""));
            for i in 0..params.len() {
                // 每个参数插槽配一个 math_number 占位 shadow(编辑版同款)
                let shadow_value = context.shadow_builder.create("math_number", None, None);
                shadows.insert(format!("PARAMS{}", i), shadow_value);
            }
        }

        let mut mutation_args = String::with_capacity(params.len() * 32);
        let parent_id = block
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
            .to_string();

        for (i, (param_name, _)) in params.iter().enumerate() {
            // 编辑版插槽名为 PARAMS0/PARAMS1/...(无空格)
            let input_name = format!("PARAMS{}", i);
            mutation_args.push_str(&format!(r#"<arg name="{}"></arg>"#, input_name));

            // 生成稳定的参数块(编辑版 is_shadow=false,可编辑)
            let param_block_id = context.shadow_builder.id_generator.generate(20);
            let param_block = json!({
                "id": param_block_id,
                "type": "procedures_2_stable_parameter",
                "is_shadow": false,
                "is_output": true,
                "fields": {
                    "param_name": param_name,
                    "param_default_value": ""
                },
                "location": [0, 0],
                "collapsed": false,
                "disabled": false,
                "parent_id": parent_id,
                "deletable": true,
                "movable": true,
                "editable": true,
                "visible": "visible",
                "comment": null,
                "mutation": "",
                "shadows": {},
                "field_constraints": {},
                "field_extra_attr": {}
            });
            context.blocks.insert(param_block_id.clone(), param_block);
            context.insert_connection(
                &parent_id,
                &param_block_id,
                json!({
                    "type": "input",
                    "input_type": "value",
                    "input_name": input_name
                }),
            );
        }

        // 编辑版 mutation:<mutation xmlns="..."><arg name="PARAMS0"></arg>...</mutation>
        let mutation = format!(
            r#"<mutation xmlns="http://www.w3.org/1999/xhtml">{}</mutation>"#,
            mutation_args
        );
        block.insert("mutation".to_string(), Value::String(mutation));

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| DecompilerError::Decompile("fields对象不存在".to_string()))?;
        fields.insert(
            "NAME".to_string(),
            Value::String(procedure_name.to_string()),
        );
        Ok(block_value)
    }
}

pub struct FunctionCallDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> FunctionCallDecompiler<'a> {
    pub fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for FunctionCallDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let procedure_name = self.compiled.get_str_or("procedure_name", "");

        let (def_id, disabled) = match context.functions.get(procedure_name) {
            Some(func) => {
                let id = func.get("id").and_then(|v| v.as_str()).unwrap_or("");
                (id.to_string(), false)
            }
            None => {
                error!("调用未定义的函数: {},将禁用该积木", procedure_name);
                (String::new(), true)
            }
        };

        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let block = block_value
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("block_value不是对象".to_string()))?;

        block.insert("disabled".to_string(), Value::Bool(disabled));

        let mut mutation = String::from(r#"<mutation xmlns="http://www.w3.org/1999/xhtml""#);
        mutation.push_str(&format!(r#" name="{}""#, procedure_name));
        mutation.push_str(&format!(r#" def_id="{}""#, def_id));
        mutation.push('>');
        for (param_name, _) in params.iter() {
            mutation.push_str(&format!(
                r#"<procedures_2_parameter_shadow name="{}" value="0"></procedures_2_parameter_shadow>"#,
                param_name
            ));
        }
        mutation.push_str("</mutation>");
        block.insert("mutation".to_string(), Value::String(mutation));

        if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
            shadows.insert("NAME".to_string(), json!(""));
        }

        let parent_id = block
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
            .to_string();

        for (param_index, (_param_name, param_value)) in params.iter().enumerate() {
            // 编辑版插槽名为 ARG0/ARG1/...(无空格)
            let input_name = format!("ARG{}", param_index);
            if param_value.is_object() {
                let mut param_decompiler =
                    BlockDecompilerCore::new(param_value, BlockBehavior::Default);
                let param_block = param_decompiler.decompile(context)?;
                let param_id = param_block
                    .get("id")
                    .ok_or_else(|| {
                        DecompilerError::InvalidResponse("param_block缺少id".to_string())
                    })?
                    .as_str()
                    .ok_or_else(|| {
                        DecompilerError::InvalidResponse("param_block id不是字符串".to_string())
                    })?
                    .to_string();
                context.blocks.insert(param_id.clone(), param_block);
                if let Some(b) = context.blocks.get_mut(&param_id)
                    && let Some(o) = b.as_object_mut()
                {
                    o.insert("parent_id".to_string(), json!(parent_id));
                }
                context.insert_connection(
                    &parent_id,
                    &param_id,
                    json!({
                        "type": "input",
                        "input_type": "value",
                        "input_name": input_name
                    }),
                );
                if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
                    let shadow_value =
                        context
                            .shadow_builder
                            .create("default_value", Some(param_id), None);
                    shadows.insert(input_name, shadow_value);
                }
            } else {
                if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
                    let shadow_value = context.shadow_builder.create("default_value", None, None);
                    shadows.insert(input_name, shadow_value);
                }
            }
        }

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| DecompilerError::Decompile("fields对象不存在".to_string()))?;
        fields.insert(
            "NAME".to_string(),
            Value::String(procedure_name.to_string()),
        );
        Ok(block_value)
    }
}

pub struct MutationDecompiler<'a> {
    inner: DefaultBlockDecompiler<'a>,
    mutation: String,
}

impl<'a> MutationDecompiler<'a> {
    pub fn new(compiled: &'a Value, mutation: String) -> Self {
        Self {
            inner: DefaultBlockDecompiler::new(compiled),
            mutation,
        }
    }
}

impl<'a> BlockDecompiler<'a> for MutationDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.inner.decompile(context)?;
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(self.mutation.clone()));
        }
        Ok(block_value)
    }
}

// 积木反编译器工厂
/// 按块类型分派专用反编译器
/// 树内递归(process_next/children/conditions/params)也使用本函数,
/// 否则嵌套的 procedures_2_callnoreturn / controls_if 等不会走专用反编译器,
/// 导致 NAME/mutation/ARG 参数块/if-else 结构缺失
/// 独立于 BlockDecompilerFactory,避免其 lifetime 绑定 BlockContext
fn create_block_decompiler<'a>(compiled: &'a Value) -> Box<dyn BlockDecompiler<'a> + 'a> {
    let block_type = compiled.get_str_or("type", "");
    match block_type {
        "controls_if" | "controls_if_no_else" => Box::new(IfBlockDecompiler::new(compiled)),
        "text_join" => Box::new(TextJoinDecompiler::new(compiled)),
        "ask_and_choose" => Box::new(AskAndChooseDecompiler::new(compiled)),
        "set_entity_show_hide" => Box::new(SetEntityShowHideDecompiler::new(compiled)),
        "text_select_changeable" => Box::new(TextSelectChangeableDecompiler::new(compiled)),
        "procedures_2_defnoreturn" => Box::new(FunctionDefDecompiler::new(compiled)),
        "procedures_2_callnoreturn" | "procedures_2_callreturn" => {
            Box::new(FunctionCallDecompiler::new(compiled))
        }
        "procedures_2_return_value" => {
            let item_count = compiled
                .get("params")
                .and_then(|v| v.as_object())
                .map(|obj| obj.len())
                .unwrap_or(0);
            let mutation = format!("<mutation items=\"{}\"></mutation>", item_count);
            Box::new(MutationDecompiler::new(compiled, mutation))
        }
        "procedures_2_stable_parameter" | "procedures_2_parameter" => {
            Box::new(DefaultBlockDecompiler::new(compiled))
        }
        _ => Box::new(DefaultBlockDecompiler::new(compiled)),
    }
}

pub struct BlockDecompilerFactory<'a> {
    config: &'a DecompilerConfig,
    id_generator: &'a IdGenerator,
}

impl<'a> BlockDecompilerFactory<'a> {
    pub fn new(config: &'a DecompilerConfig, id_generator: &'a IdGenerator) -> Self {
        Self {
            config,
            id_generator,
        }
    }

    pub fn create(&self, compiled: &'a Value) -> Box<dyn BlockDecompiler<'a> + 'a> {
        create_block_decompiler(compiled)
    }
}

// HTTP 客户端
pub trait HttpClient: Send + Sync {
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
pub struct CodeMaoHttpClient {
    client: Arc<CodeMaoClient>,
}

impl CodeMaoHttpClient {
    pub fn new(client: Arc<CodeMaoClient>) -> Self {
        Self { client }
    }
}

impl HttpClient for CodeMaoHttpClient {
    fn get_json(&self, url: &str, headers: Option<Vec<(String, String)>>) -> Result<Value> {
        let mut request_builder = self.client.build_request(HttpMethod::Get, url, None);
        if let Some(headers_map) = headers {
            request_builder = request_builder.with_headers(headers_map);
        }
        let response = request_builder
            .send()
            .map_err(|e| DecompilerError::Http(format!("请求失败: {}", e)))?;
        self.client
            .response_to_json(response)
            .map_err(|e| DecompilerError::Http(format!("JSON解析失败: {}", e)))
    }

    fn get_binary(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .send()
            .map_err(|e| DecompilerError::Http(format!("请求失败: {}", e)))?;
        self.client
            .response_to_binary(response)
            .map_err(|e| DecompilerError::Http(format!("Binary解析失败: {}", e)))
    }

    fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .send()
            .map_err(|e| DecompilerError::Http(format!("请求失败: {}", e)))?;
        self.client
            .response_to_string(response)
            .map_err(|e| DecompilerError::Http(format!("String解析失败: {}", e)))
    }

    fn box_clone(&self) -> Box<dyn HttpClient> {
        Box::new(self.clone())
    }
}

// 反编译选项(构建器)
/// 反编译调用配置,供外部通过链式方法定制(门面模式的参数对象)
#[derive(Debug, Clone)]
pub struct DecompileOptions {
    /// 输出目录;`None` 时使用 `DecompilerConfig::default_output_dir`
    output_dir: Option<PathBuf>,
    /// 是否保存原始(未反编译)数据到 `output_dir/raw/`,默认 true
    save_raw: bool,
    /// 批处理并发数(≥1),默认 1
    batch_concurrency: usize,
}

impl Default for DecompileOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl DecompileOptions {
    pub fn new() -> Self {
        Self {
            output_dir: None,
            save_raw: true,
            batch_concurrency: 1,
        }
    }

    /// 指定输出目录
    pub fn output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.output_dir = Some(dir.into());
        self
    }

    /// 是否保存原始数据(默认 true)
    pub fn save_raw(mut self, on: bool) -> Self {
        self.save_raw = on;
        self
    }

    /// 批处理并发数(默认 1)
    pub fn batch_concurrency(mut self, n: usize) -> Self {
        self.batch_concurrency = n.max(1);
        self
    }
}

// 作品处理器注册表(注册表模式)
/// fetcher 构造器:按作品类型创建对应的 `WorkFetcher`
pub type FetcherFactory =
    Box<dyn Fn(Box<dyn HttpClient>, Arc<DecompilerConfig>) -> Box<dyn WorkFetcher> + Send + Sync>;
/// decompiler 构造器:按作品类型创建对应的 `WorkDecompiler`
pub type DecompilerFactory =
    Box<dyn Fn(&Arc<DecompilerConfig>) -> Box<dyn WorkDecompiler> + Send + Sync>;

/// 作品类型 → 处理器(fetcher/decompiler)的注册表
/// 新增作品类型时只需 `register`,无需修改门面代码(开闭原则)
pub struct WorkProcessorRegistry {
    fetchers: HashMap<WorkType, FetcherFactory>,
    decompilers: HashMap<WorkType, DecompilerFactory>,
}

impl WorkProcessorRegistry {
    pub fn new() -> Self {
        Self {
            fetchers: HashMap::new(),
            decompilers: HashMap::new(),
        }
    }

    /// 注册某一作品类型的 fetcher 与 decompiler 构造器
    pub fn register(
        &mut self,
        work_type: WorkType,
        fetcher: FetcherFactory,
        decompiler: DecompilerFactory,
    ) {
        self.fetchers.insert(work_type, fetcher);
        self.decompilers.insert(work_type, decompiler);
    }

    /// 按作品类型创建 fetcher
    pub fn fetcher_for(
        &self,
        work_type: &WorkType,
        client: Box<dyn HttpClient>,
        config: Arc<DecompilerConfig>,
    ) -> Result<Box<dyn WorkFetcher>> {
        self.fetchers
            .get(work_type)
            .ok_or_else(|| DecompilerError::UnsupportedType(format!("{:?}", work_type)))
            .map(|factory| factory(client, config))
    }

    /// 按作品类型创建 decompiler
    pub fn decompiler_for(
        &self,
        work_type: &WorkType,
        config: &Arc<DecompilerConfig>,
    ) -> Result<Box<dyn WorkDecompiler>> {
        self.decompilers
            .get(work_type)
            .ok_or_else(|| DecompilerError::UnsupportedType(format!("{:?}", work_type)))
            .map(|factory| factory(config))
    }

    /// 内置全部作品类型的默认注册
    fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Kitten2/3/4 共用 KittenFetcher / KittenDecompiler
        for wt in [WorkType::Kitten2, WorkType::Kitten3, WorkType::Kitten4] {
            registry.register(
                wt,
                Box::new(|client, config| Box::new(KittenFetcher::new(client, config))),
                Box::new(|_| Box::new(KittenDecompiler)),
            );
        }
        registry.register(
            WorkType::Neko,
            Box::new(|client, config| Box::new(NekoFetcher::new(client, config))),
            Box::new(|config| Box::new(NekoDecompiler::new(config.crypto_salt.as_slice()))),
        );
        registry.register(
            WorkType::Nemo,
            Box::new(|client, config| Box::new(NemoFetcher::new(client, config))),
            Box::new(|_| Box::new(NemoDecompiler)),
        );
        registry.register(
            WorkType::Wood,
            Box::new(|client, config| Box::new(WoodFetcher::new(client, config))),
            Box::new(|_| Box::new(WoodDecompiler)),
        );
        registry.register(
            WorkType::Coco,
            Box::new(|client, config| Box::new(CocoFetcher::new(client, config))),
            Box::new(|_| Box::new(CocoDecompiler)),
        );
        registry
    }
}

impl Default for WorkProcessorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// 主入口
pub struct CodemaoDecompiler {
    config: Arc<DecompilerConfig>,
    client: Arc<CodeMaoClient>,
    id_generator: IdGenerator,
    registry: Arc<WorkProcessorRegistry>,
}

impl CodemaoDecompiler {
    pub fn new(config: Option<DecompilerConfig>, client: Arc<CodeMaoClient>) -> Self {
        let config = Arc::new(config.unwrap_or_default());
        Self {
            config,
            client,
            id_generator: IdGenerator::new(),
            registry: Arc::new(WorkProcessorRegistry::default()),
        }
    }

    /// 全局单例门面:复用全局 HTTP 客户端与默认注册表
    /// 多次反编译不重复创建客户端(性能优化)
    pub fn global() -> &'static Self {
        static GLOBAL: OnceLock<CodemaoDecompiler> = OnceLock::new();
        GLOBAL.get_or_init(|| {
            let client = Arc::new(KittyFactory::global_client().clone());
            Self::new(None, client)
        })
    }
    /// 反编译单个作品(默认选项,向后兼容)
    pub fn decompile(&self, work_id: i64, output_dir: Option<&Path>) -> Result<String> {
        let mut options = DecompileOptions::new();
        if let Some(dir) = output_dir {
            options = options.output_dir(dir.to_path_buf());
        }
        self.decompile_with_options(work_id, options)
    }

    /// 使用自定义选项反编译单个作品
    pub fn decompile_with_options(
        &self,
        work_id: i64,
        options: DecompileOptions,
    ) -> Result<String> {
        self.decompile_inner(work_id, &options)
    }

    /// 批处理反编译多个作品,返回与输入顺序一致的 `Vec<Result>`
    pub fn decompile_batch(
        &self,
        work_ids: &[i64],
        options: DecompileOptions,
    ) -> Vec<Result<String>> {
        let concurrency = options.batch_concurrency.max(1);
        if concurrency == 1 || work_ids.len() <= 1 {
            return work_ids
                .iter()
                .map(|&id| self.decompile_inner(id, &options))
                .collect();
        }
        // 按并发数分块,块内并发执行(thread::scope),块间顺序收集保持结果顺序
        let options_ref = &options;
        let mut results = Vec::with_capacity(work_ids.len());
        for chunk in work_ids.chunks(concurrency) {
            let chunk_results: Vec<Result<String>> = std::thread::scope(|scope| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|&id| scope.spawn(move || self.decompile_inner(id, options_ref)))
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle.join().unwrap_or_else(|_| {
                            Err(DecompilerError::Other {
                                msg: "反编译线程异常".to_string(),
                                source: None,
                            })
                        })
                    })
                    .collect()
            });
            results.extend(chunk_results);
        }
        results
    }

    /// 反编译主流程(模板方法)
    /// 流程为:获取信息 → 创建处理器 → 取原始数据 → (可选)保存原始数据 → 反编译 → 保存结果
    fn decompile_inner(&self, work_id: i64, options: &DecompileOptions) -> Result<String> {
        info!("开始反编译作品 [work_id={}]", work_id);
        let http_client = Box::new(CodeMaoHttpClient::new(self.client.clone()));
        let work_info = self
            .fetch_work_info(&*http_client, work_id)
            .with_context(|| format!("获取作品 {} 信息失败", work_id))?;

        let fetcher = self
            .registry
            .fetcher_for(
                &work_info.work_type,
                http_client.clone(),
                self.config.clone(),
            )
            .with_context(|| format!("不支持的{}作品类型", work_id))?;
        let decompiler = self
            .registry
            .decompiler_for(&work_info.work_type, &self.config)
            .with_context(|| format!("不支持的{}作品类型", work_id))?;
        let raw = fetcher
            .fetch(&work_info)
            .with_context(|| format!("获取作品 {} 原始数据失败", work_id))?;

        // 确定输出目录(用户指定或默认)
        let output_path = options
            .output_dir
            .as_deref()
            .unwrap_or(&self.config.default_output_dir);

        // 可选:保存原始数据到 raw/ 子目录
        if options.save_raw {
            self.save_raw_data(&work_info, &raw, output_path)
                .with_context(|| format!("保存作品 {} 原始数据失败", work_id))?;
        }

        let context = DecompilerContextBuilder::new()
            .work_info(work_info)
            .http_client(http_client)
            .config(self.config.clone())
            .id_generator(self.id_generator.clone())
            .build()?;

        let result = decompiler.decompile(raw, &context)?;
        let saved = decompiler.save_result(&result, Some(output_path), &context)?;
        info!("作品 [work_id={}] 反编译完成,保存至: {}", work_id, saved);
        Ok(saved)
    }
    /// 将获取到的未编译原始数据保存到 `output_dir/raw/` 目录下
    /// 文件名格式为 `raw-{作品名称}.{扩展名}`,其中名称经过安全过滤
    fn save_raw_data(
        &self,
        work_info: &WorkInfo,
        raw: &RawWorkData,
        output_dir: &Path,
    ) -> Result<PathBuf> {
        let raw_dir = output_dir.join("raw");
        std::fs::create_dir_all(&raw_dir)?;
        // 安全的基础文件名,不含扩展名
        let base_name = format!(
            "raw-{}",
            FileService::safe_filename(&work_info.name, work_info.id, "")
        );
        match raw {
            RawWorkData::Kitten(data) | RawWorkData::Coco(data) | RawWorkData::Wood(data) => {
                let filename = format!("{}.json", base_name);
                let path = raw_dir.join(filename);
                FileService::write_json(&path, data)?;
                Ok(path)
            }
            RawWorkData::NekoEncrypted(s) => {
                let filename = format!("{}.txt", base_name);
                let path = raw_dir.join(filename);
                std::fs::write(&path, s)?;
                Ok(path)
            }
            RawWorkData::Nemo(bcm, src) => {
                let bcm_filename = format!("{}.bcm.json", base_name);
                let src_filename = format!("{}.src.json", base_name);
                let bcm_path = raw_dir.join(bcm_filename);
                let src_path = raw_dir.join(src_filename);
                FileService::write_json(&bcm_path, bcm)?;
                FileService::write_json(&src_path, src)?;
                // 返回主文件(bcm)的路径
                Ok(bcm_path)
            }
        }
    }
    fn fetch_work_info(&self, http_client: &dyn HttpClient, work_id: i64) -> Result<WorkInfo> {
        let url = format!(
            "{}/creation-tools/v1/works/{}",
            self.config.base_url, work_id
        );
        let data = http_client.get_json(&url, None)?;
        WorkInfo::from_api_response(&data)
    }
}

/// 便捷反编译函数:使用全局单例门面(复用 HTTP 客户端),功能与之前一致
pub fn decompile_work(work_id: i64, output_dir: Option<&Path>) -> Result<String> {
    CodemaoDecompiler::global().decompile(work_id, output_dir)
}

/// 便捷反编译函数:使用自定义选项
pub fn decompile_work_with(work_id: i64, options: DecompileOptions) -> Result<String> {
    CodemaoDecompiler::global().decompile_with_options(work_id, options)
}

/// 便捷批量反编译函数:返回与输入顺序一致的 `Vec<Result<String>>`
pub fn decompile_works(work_ids: &[i64], options: DecompileOptions) -> Vec<Result<String>> {
    CodemaoDecompiler::global().decompile_batch(work_ids, options)
}
