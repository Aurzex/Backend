use crate::api::auth::CloudAuthenticator;
use crate::utils::acquire::{ClientFactory, CodeMaoClient, HttpMethod};
use crate::utils::data::PathConfig;
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose};
use rand::{Rng, RngExt};
use serde_json::{Value, from_str, json, to_string_pretty};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use thiserror::Error;

// ============ 错误定义 ============
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
    #[error("其他错误: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DecompilerError>;

// ============ 配置管理 ============
#[derive(Debug, Clone)]
pub struct DecompilerConfig {
    // API配置
    pub base_url: String,
    pub creation_base_url: String,
    pub client_secret: String,
    pub crypto_salt: Vec<u8>,

    // 输出配置
    pub default_output_dir: PathBuf,

    // 工具箱分类顺序
    pub toolbox_categories: Vec<String>,

    // 阴影积木类型 (共享)
    pub shadow_types: Arc<HashSet<String>>,

    // 阴影积木字段配置 (共享)
    pub shadow_fields: Arc<HashMap<String, HashMap<String, String>>>,

    // 作品类型映射 (共享)
    pub file_extensions: Arc<HashMap<String, String>>,
}

static SHADOW_TYPES: OnceLock<Arc<HashSet<String>>> = OnceLock::new();
static SHADOW_FIELDS: OnceLock<Arc<HashMap<String, HashMap<String, String>>>> = OnceLock::new();
static FILE_EXTENSIONS: OnceLock<Arc<HashMap<String, String>>> = OnceLock::new();

impl Default for DecompilerConfig {
    fn default() -> Self {
        // 初始化静态数据（仅首次执行）
        let shadow_types = SHADOW_TYPES
            .get_or_init(|| {
                let mut set = HashSet::new();
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
                    "math_number",
                    "text",
                ] {
                    set.insert(st.to_string());
                }
                Arc::new(set)
            })
            .clone();

        let shadow_fields = SHADOW_FIELDS
            .get_or_init(|| {
                let mut fields = HashMap::new();

                let mut math_number = HashMap::new();
                math_number.insert("name".to_string(), "NUM".to_string());
                math_number.insert("text".to_string(), "0".to_string());
                math_number.insert(
                    "constraints".to_string(),
                    "-Infinity,Infinity,0,".to_string(),
                );
                math_number.insert("allow_text".to_string(), "true".to_string());
                fields.insert("math_number".to_string(), math_number);

                let mut controller_shadow = HashMap::new();
                controller_shadow.insert("name".to_string(), "NUM".to_string());
                controller_shadow.insert("text".to_string(), "0".to_string());
                controller_shadow.insert(
                    "constraints".to_string(),
                    "-Infinity,Infinity,0,false".to_string(),
                );
                fields.insert("controller_shadow".to_string(), controller_shadow);

                let mut text = HashMap::new();
                text.insert("name".to_string(), "TEXT".to_string());
                text.insert("text".to_string(), "".to_string());
                fields.insert("text".to_string(), text);

                let mut lists_get = HashMap::new();
                lists_get.insert("name".to_string(), "VAR".to_string());
                lists_get.insert("text".to_string(), "?".to_string());
                fields.insert("lists_get".to_string(), lists_get);

                let mut broadcast_input = HashMap::new();
                broadcast_input.insert("name".to_string(), "MESSAGE".to_string());
                broadcast_input.insert("text".to_string(), "Hi".to_string());
                fields.insert("broadcast_input".to_string(), broadcast_input);

                let mut get_audios = HashMap::new();
                get_audios.insert("name".to_string(), "sound_id".to_string());
                get_audios.insert("text".to_string(), "?".to_string());
                fields.insert("get_audios".to_string(), get_audios);

                let mut get_whole_audios = HashMap::new();
                get_whole_audios.insert("name".to_string(), "sound_id".to_string());
                get_whole_audios.insert("text".to_string(), "all".to_string());
                fields.insert("get_whole_audios".to_string(), get_whole_audios);

                let mut get_current_costume = HashMap::new();
                get_current_costume.insert("name".to_string(), "style_id".to_string());
                get_current_costume.insert("text".to_string(), "".to_string());
                fields.insert("get_current_costume".to_string(), get_current_costume);

                let mut default_value = HashMap::new();
                default_value.insert("name".to_string(), "TEXT".to_string());
                default_value.insert("text".to_string(), "0".to_string());
                default_value.insert("has_been_edited".to_string(), "false".to_string());
                fields.insert("default_value".to_string(), default_value);

                let mut get_current_scene = HashMap::new();
                get_current_scene.insert("name".to_string(), "scene".to_string());
                get_current_scene.insert("text".to_string(), "".to_string());
                fields.insert("get_current_scene".to_string(), get_current_scene);

                let mut get_sensing_current_scene = HashMap::new();
                get_sensing_current_scene.insert("name".to_string(), "scene".to_string());
                get_sensing_current_scene.insert("text".to_string(), "".to_string());
                fields.insert(
                    "get_sensing_current_scene".to_string(),
                    get_sensing_current_scene,
                );

                Arc::new(fields)
            })
            .clone();

        let file_extensions = FILE_EXTENSIONS
            .get_or_init(|| {
                let mut map = HashMap::new();
                map.insert("KITTEN2".to_string(), ".bcm".to_string());
                map.insert("KITTEN3".to_string(), ".bcm".to_string());
                map.insert("KITTEN4".to_string(), ".bcm4".to_string());
                map.insert("COCO".to_string(), ".json".to_string());
                map.insert("NEKO".to_string(), ".bcmkn".to_string());
                map.insert("NEMO".to_string(), "".to_string());
                map.insert("WOOD".to_string(), "".to_string());
                Arc::new(map)
            })
            .clone();

        Self {
            base_url: "https://api.codemao.cn".to_string(),
            creation_base_url: "https://api-creation.codemao.cn".to_string(),
            client_secret: "pBlYqXbJDu".to_string(),
            crypto_salt: (0..31).collect(),
            default_output_dir: PathConfig::compile_file_path(),
            toolbox_categories: vec![
                "action".to_string(),
                "advanced".to_string(),
                "ai".to_string(),
                "ai_game".to_string(),
                "ai_lab".to_string(),
                "appearance".to_string(),
                "arduino".to_string(),
                "audio".to_string(),
                "camera".to_string(),
                "cloud_list".to_string(),
                "cloud_variable".to_string(),
                "cognitive".to_string(),
                "control".to_string(),
                "data".to_string(),
                "event".to_string(),
                "micro_bit".to_string(),
                "midi_music".to_string(),
                "mobile_control".to_string(),
                "operator".to_string(),
                "pen".to_string(),
                "physic".to_string(),
                "physics2".to_string(),
                "procedure".to_string(),
                "sensing".to_string(),
                "video".to_string(),
                "wee_make".to_string(),
                "wood".to_string(),
            ],
            shadow_types,
            shadow_fields,
            file_extensions,
        }
    }
}

// ============ 作品类型枚举 ============
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorkType {
    KITTEN2,
    KITTEN3,
    KITTEN4,
    COCO,
    NEKO,
    NEMO,
    WOOD,
}

impl WorkType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "KITTEN2" => Some(WorkType::KITTEN2),
            "KITTEN3" => Some(WorkType::KITTEN3),
            "KITTEN4" => Some(WorkType::KITTEN4),
            "COCO" => Some(WorkType::COCO),
            "NEKO" => Some(WorkType::NEKO),
            "NEMO" => Some(WorkType::NEMO),
            "WOOD" => Some(WorkType::WOOD),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            WorkType::KITTEN2 => "KITTEN2",
            WorkType::KITTEN3 => "KITTEN3",
            WorkType::KITTEN4 => "KITTEN4",
            WorkType::COCO => "COCO",
            WorkType::NEKO => "NEKO",
            WorkType::NEMO => "NEMO",
            WorkType::WOOD => "WOOD",
        }
    }

    pub fn is_kitten(&self) -> bool {
        matches!(
            self,
            WorkType::KITTEN2 | WorkType::KITTEN3 | WorkType::KITTEN4
        )
    }

    pub fn is_nemo(&self) -> bool {
        matches!(self, WorkType::NEMO)
    }

    pub fn is_neko(&self) -> bool {
        matches!(self, WorkType::NEKO)
    }

    pub fn is_coco(&self) -> bool {
        matches!(self, WorkType::COCO)
    }

    pub fn is_wood(&self) -> bool {
        matches!(self, WorkType::WOOD)
    }
}

// ============ 作品信息值对象 ============
#[derive(Debug, Clone)]
pub struct WorkInfo {
    pub id: i64,
    pub name: String,
    pub work_type: WorkType,
    pub version: String,
    pub user_id: i64,
    pub preview_url: String,
}

impl WorkInfo {
    pub fn from_api_response(data: &Value) -> Result<Self> {
        let work_type_str = data.get("type").and_then(|v| v.as_str()).unwrap_or("NEMO");
        let work_type = WorkType::from_str(work_type_str).unwrap_or(WorkType::NEMO);
        Ok(Self {
            id: data.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
            name: data
                .get("work_name")
                .or_else(|| data.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("未知作品")
                .to_string(),
            work_type,
            version: data
                .get("bcm_version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.16.2")
                .to_string(),
            user_id: data.get("user_id").and_then(|v| v.as_i64()).unwrap_or(0),
            preview_url: data
                .get("preview")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
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

// ============ 文件操作服务 ============
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
        let json_str = to_string_pretty(data)?;
        std::fs::write(path, json_str)?;
        Ok(())
    }

    pub fn write_binary(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }
}

// ============ ID生成器 ============
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
        let mut rng = rand::rng();
        (0..length)
            .map(|_| {
                let idx = rng.random_range(0..self.chars.len());
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

// ============ 加密解密服务 ============
#[derive(Clone)]
pub struct CryptoService {
    salt: Vec<u8>,
}

impl CryptoService {
    pub fn new(salt: &[u8]) -> Self {
        Self {
            salt: salt.to_vec(),
        }
    }

    pub fn sha256(data: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        format!("{:x}", hasher.finalize())
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
        let key = self.generate_aes_key();
        let key = Key::<Aes256Gcm>::from_slice(&key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(iv);
        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| DecompilerError::Crypto(format!("AES解密失败: {}", e)))
    }

    pub fn decrypt_bcmkn(&self, encrypted_content: &str) -> Result<Vec<u8>> {
        let reversed = self.reverse_string(encrypted_content);
        let decoded = self.base64_to_bytes(&reversed)?;
        if decoded.len() < 13 {
            return Err(DecompilerError::Crypto(
                "数据太短，无法分离IV和密文".to_string(),
            ));
        }
        let iv = &decoded[..12];
        let ciphertext = &decoded[12..];
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

// ============ 阴影积木构建器（修复版） ============
#[derive(Clone)]
pub struct ShadowBuilder {
    config: Arc<DecompilerConfig>,
    id_generator: IdGenerator,
}

impl ShadowBuilder {
    pub fn new(config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        Self {
            config,
            id_generator,
        }
    }

    pub fn create(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> String {
        if shadow_type == "logic_empty" {
            let block_id = block_id.unwrap_or_else(|| self.id_generator.generate(20));
            return format!(
                r#"<empty type="logic_empty" id="{}" visible="visible" editable="false"></empty>"#,
                block_id
            );
        }

        let config = self
            .config
            .shadow_fields
            .get(shadow_type)
            .cloned()
            .unwrap_or_default();
        let block_id = block_id.unwrap_or_else(|| self.id_generator.generate(20));
        let display_text = text.unwrap_or(config.get("text").map(|s| s.as_str()).unwrap_or(""));

        let mut xml = String::new();

        xml.push_str(&format!(
            r#"<shadow type="{}" id="{}" visible="visible" editable="true">"#,
            shadow_type, block_id
        ));

        xml.push_str(&format!(
            r#"<field name="{}""#,
            config.get("name").map(|s| s.as_str()).unwrap_or("")
        ));

        for attr in ["constraints", "allow_text", "has_been_edited"] {
            if let Some(value) = config.get(attr) {
                xml.push_str(&format!(r#" {}="{}""#, attr, value));
            }
        }

        xml.push_str(&format!(">{}</field>", display_text));
        xml.push_str("</shadow>");

        xml
    }
}

// ============ HTTP客户端协议 ============
pub trait HttpClient: Send + Sync {
    fn get_json(&self, url: &str, headers: Option<HashMap<String, String>>) -> Result<Value>;
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
    client: &'static CodeMaoClient,
}

impl CodeMaoHttpClient {
    pub fn new(client: &'static CodeMaoClient) -> Self {
        Self { client }
    }
}

impl HttpClient for CodeMaoHttpClient {
    fn get_json(&self, url: &str, headers: Option<HashMap<String, String>>) -> Result<Value> {
        let mut request_builder = self.client.build_request(HttpMethod::GET, url, None);
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
            .build_request(HttpMethod::GET, url, None)
            .send()
            .map_err(|e| DecompilerError::Http(format!("请求失败: {}", e)))?;
        self.client
            .response_to_binary(response)
            .map_err(|e| DecompilerError::Http(format!("Binary解析失败: {}", e)))
    }

    fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .build_request(HttpMethod::GET, url, None)
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

// ============ 积木反编译器行为 trait ============
pub trait BlockDecompilerBehavior: Send + Sync {
    fn get_child_input_name(&self, index: usize, conditions_count: usize) -> String;
}

pub struct DefaultBlockBehavior;

impl BlockDecompilerBehavior for DefaultBlockBehavior {
    fn get_child_input_name(&self, _index: usize, _conditions_count: usize) -> String {
        "DO".to_string()
    }
}

pub struct IfBlockBehavior {
    conditions_count: usize,
}

impl IfBlockBehavior {
    pub fn new(conditions_count: usize) -> Self {
        Self { conditions_count }
    }
}

impl BlockDecompilerBehavior for IfBlockBehavior {
    fn get_child_input_name(&self, index: usize, _conditions_count: usize) -> String {
        if index < self.conditions_count {
            format!("DO {}", index)
        } else {
            "ELSE".to_string()
        }
    }
}

// ============ 积木反编译上下文（修复版） ============
#[derive(Clone)]
pub struct BlockContext {
    pub actor_data: Value,
    pub functions: Arc<HashMap<String, Value>>,
    pub shadow_builder: ShadowBuilder,
    pub blocks: HashMap<String, Value>,
    pub connections: HashMap<String, Value>,
    pub shadows: HashMap<String, String>,
}

impl BlockContext {
    pub fn new(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
    ) -> Self {
        Self {
            actor_data,
            functions,
            shadow_builder,
            blocks: HashMap::new(),
            connections: HashMap::new(),
            shadows: HashMap::new(),
        }
    }
}

// ============ 积木反编译器核心（递归处理，修复版） ============
const OUTPUT_BLOCK_TYPES: &[&str] = &["logic_boolean", "procedures_2_stable_parameter"];

pub struct BlockDecompilerCore {
    compiled: Value,
    config: Arc<DecompilerConfig>,
    id_generator: IdGenerator,
    behavior: Box<dyn BlockDecompilerBehavior>,
}

impl BlockDecompilerCore {
    pub fn new(
        compiled: Value,
        config: Arc<DecompilerConfig>,
        id_generator: IdGenerator,
        behavior: Box<dyn BlockDecompilerBehavior>,
    ) -> Self {
        Self {
            compiled,
            config,
            id_generator,
            behavior,
        }
    }

    pub fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let id = self
            .compiled
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let block_type = self
            .compiled
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let is_shadow = self.config.shadow_types.contains(&block_type);
        let is_output = is_shadow || OUTPUT_BLOCK_TYPES.contains(&block_type.as_str());

        let mut block = serde_json::Map::new();
        block.insert("id".to_string(), Value::String(id.clone()));
        block.insert("type".to_string(), Value::String(block_type.clone()));
        block.insert("location".to_string(), json!([0, 0]));
        block.insert("is_shadow".to_string(), Value::Bool(is_shadow));
        block.insert("is_output".to_string(), Value::Bool(is_output));
        block.insert("collapsed".to_string(), Value::Bool(false));
        block.insert("disabled".to_string(), Value::Bool(false));
        block.insert("deletable".to_string(), Value::Bool(true));
        block.insert("movable".to_string(), Value::Bool(true));
        block.insert("editable".to_string(), Value::Bool(true));
        block.insert("visible".to_string(), Value::String("visible".to_string()));
        block.insert("shadows".to_string(), Value::Object(serde_json::Map::new()));
        block.insert("fields".to_string(), Value::Object(serde_json::Map::new()));
        block.insert(
            "field_constraints".to_string(),
            Value::Object(serde_json::Map::new()),
        );
        block.insert(
            "field_extra_attr".to_string(),
            Value::Object(serde_json::Map::new()),
        );
        block.insert("comment".to_string(), Value::Null);
        block.insert("mutation".to_string(), Value::String("".to_string()));
        block.insert("parent_id".to_string(), Value::Null);

        let block_value = Value::Object(block);
        context.blocks.insert(id.clone(), block_value.clone());

        let mut block_value = block_value;
        self.process_next(context, &mut block_value)?;
        self.process_children(context, &mut block_value)?;
        self.process_conditions(context, &mut block_value)?;
        self.process_params(context, &mut block_value)?;

        if let Some(obj) = block_value.as_object_mut() {
            if let Some(shadows_obj) = obj.get_mut("shadows").and_then(|v| v.as_object_mut()) {
                for (name, xml) in &context.shadows {
                    shadows_obj.insert(name.clone(), Value::String(xml.clone()));
                }
            }
        }

        context.blocks.insert(id, block_value.clone());
        Ok(block_value)
    }

    fn process_next(&self, context: &mut BlockContext, block_value: &mut Value) -> Result<()> {
        if let Some(next_compiled) = self.compiled.get("next_block") {
            if !next_compiled.is_null() {
                let mut decompiler = BlockDecompilerCore::new(
                    next_compiled.clone(),
                    self.config.clone(),
                    self.id_generator.clone(),
                    Box::new(DefaultBlockBehavior),
                );
                let mut next_block = decompiler.decompile(context)?;
                if let Some(obj) = next_block.as_object_mut() {
                    obj.insert(
                        "parent_id".to_string(),
                        block_value.get("id").unwrap().clone(),
                    );
                }
                let next_id = next_block.get("id").unwrap().as_str().unwrap().to_string();
                context.blocks.insert(next_id.clone(), next_block);
                context.connections.insert(next_id, json!({"type": "next"}));
            }
        }
        Ok(())
    }

    fn process_children(&self, context: &mut BlockContext, block_value: &mut Value) -> Result<()> {
        if let Some(children) = self.compiled.get("child_block").and_then(|v| v.as_array()) {
            let conditions_count = self
                .compiled
                .get("conditions")
                .and_then(|v| v.as_array())
                .map(|arr| arr.len())
                .unwrap_or(0);

            for (i, child) in children.iter().enumerate() {
                if !child.is_null() {
                    let mut decompiler = BlockDecompilerCore::new(
                        child.clone(),
                        self.config.clone(),
                        self.id_generator.clone(),
                        Box::new(DefaultBlockBehavior),
                    );
                    let mut child_block = decompiler.decompile(context)?;
                    if let Some(obj) = child_block.as_object_mut() {
                        obj.insert(
                            "parent_id".to_string(),
                            block_value.get("id").unwrap().clone(),
                        );
                    }
                    let child_id = child_block.get("id").unwrap().as_str().unwrap().to_string();
                    context.blocks.insert(child_id.clone(), child_block);
                    let input_name = self.behavior.get_child_input_name(i, conditions_count);
                    context.connections.insert(
                        child_id,
                        json!({
                            "type": "input",
                            "input_type": "statement",
                            "input_name": input_name
                        }),
                    );
                    context.shadows.insert(input_name, String::new());
                }
            }
        }
        Ok(())
    }

    fn process_conditions(
        &self,
        context: &mut BlockContext,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(conditions) = self.compiled.get("conditions").and_then(|v| v.as_array()) {
            for (i, condition) in conditions.iter().enumerate() {
                let input_name = format!("IF{}", i);
                if condition.is_null() {
                    let shadow_xml = context.shadow_builder.create("logic_empty", None, None);
                    context.shadows.insert(input_name, shadow_xml);
                } else {
                    let mut decompiler = BlockDecompilerCore::new(
                        condition.clone(),
                        self.config.clone(),
                        self.id_generator.clone(),
                        Box::new(DefaultBlockBehavior),
                    );
                    let mut condition_block = decompiler.decompile(context)?;
                    if let Some(obj) = condition_block.as_object_mut() {
                        obj.insert(
                            "parent_id".to_string(),
                            block_value.get("id").unwrap().clone(),
                        );
                    }
                    let cond_id = condition_block
                        .get("id")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();
                    context.blocks.insert(cond_id.clone(), condition_block);
                    context.connections.insert(
                        cond_id.clone(),
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": input_name
                        }),
                    );
                    let shadow_xml =
                        context
                            .shadow_builder
                            .create("logic_empty", Some(cond_id), None);
                    context.shadows.insert(input_name, shadow_xml);
                }
            }
        }
        Ok(())
    }

    fn process_params(&self, context: &mut BlockContext, block_value: &mut Value) -> Result<()> {
        if let Some(params) = self.compiled.get("params").and_then(|v| v.as_object()) {
            for (name, value) in params {
                if value.is_object() {
                    let mut decompiler = BlockDecompilerCore::new(
                        value.clone(),
                        self.config.clone(),
                        self.id_generator.clone(),
                        Box::new(DefaultBlockBehavior),
                    );
                    let mut param_block = decompiler.decompile(context)?;
                    if let Some(obj) = param_block.as_object_mut() {
                        obj.insert(
                            "parent_id".to_string(),
                            block_value.get("id").unwrap().clone(),
                        );
                    }
                    let param_id = param_block.get("id").unwrap().as_str().unwrap().to_string();
                    context.blocks.insert(param_id.clone(), param_block.clone());
                    let input_name = name.clone();
                    context.connections.insert(
                        param_id.clone(),
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": input_name
                        }),
                    );

                    let param_type = param_block
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if self.config.shadow_types.contains(param_type) {
                        let field_value = param_block
                            .get("fields")
                            .and_then(|v| v.as_object())
                            .and_then(|fields| fields.values().next())
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let shadow_xml = context.shadow_builder.create(
                            param_type,
                            Some(param_id),
                            Some(field_value),
                        );
                        context.shadows.insert(input_name, shadow_xml);
                    } else {
                        let shadow_type = if name == "condition" || name == "BOOL" {
                            "logic_empty"
                        } else {
                            "math_number"
                        };
                        let shadow_xml = context.shadow_builder.create(shadow_type, None, None);
                        context.shadows.insert(input_name, shadow_xml);
                    }
                } else {
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

// ============ 反编译器上下文 ============
pub struct DecompilerContext {
    pub work_info: WorkInfo,
    pub http_client: Box<dyn HttpClient>,
    pub file_service: FileService,
    pub id_generator: IdGenerator,
    pub config: Arc<DecompilerConfig>,
    pub crypto_service: Option<CryptoService>,
}

// ============ 反编译器基类 ============
pub trait BaseDecompiler: Send + Sync {
    fn decompile(&mut self) -> Result<DecompileResult>;
    fn save_result(&self, result: &DecompileResult, output_dir: Option<&Path>) -> Result<String>;
}

#[derive(Debug)]
pub enum DecompileResult {
    Json(Value),
    Path(String),
}

// ============ NEKO反编译器 ============
pub struct NekoDecompiler {
    context: DecompilerContext,
}

impl NekoDecompiler {
    pub fn new(context: DecompilerContext) -> Self {
        Self { context }
    }
}

impl BaseDecompiler for NekoDecompiler {
    fn decompile(&mut self) -> Result<DecompileResult> {
        let detail_url = format!(
            "{}/neko/community/player/published-work-detail/{}",
            self.context.config.creation_base_url, self.context.work_info.id
        );

        let mut auth = CloudAuthenticator::new(None);
        let device_auth = auth
            .generate_x_device_auth()
            .map_err(|e| DecompilerError::Other(format!("生成设备认证失败: {}", e)))?;

        let device_auth_str = serde_json::to_string(&device_auth)?;

        let mut headers = HashMap::new();
        headers.insert("x-creation-tools-device-auth".to_string(), device_auth_str);

        let detail = self
            .context
            .http_client
            .get_json(&detail_url, Some(headers))?;

        let encrypted_url = detail
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;

        let encrypted_content = self.context.http_client.get_text(encrypted_url)?;
        let crypto_service = self
            .context
            .crypto_service
            .as_ref()
            .ok_or_else(|| DecompilerError::Crypto("NEKO作品需要有效的加密服务".to_string()))?
            .clone();
        let decryptor = BCMKNDecryptor::new(crypto_service);
        let decrypted = decryptor.decrypt(&encrypted_content)?;

        Ok(DecompileResult::Json(decrypted))
    }

    fn save_result(&self, result: &DecompileResult, output_dir: Option<&Path>) -> Result<String> {
        match result {
            DecompileResult::Json(json) => {
                let output_path = output_dir.unwrap_or(&self.context.config.default_output_dir);
                FileService::ensure_dir(output_path)?;

                let filename = FileService::safe_filename(
                    &self.context.work_info.name,
                    self.context.work_info.id,
                    self.context
                        .work_info
                        .file_extension(&self.context.config)
                        .trim_start_matches('.'),
                );

                let filepath = output_path.join(filename);
                FileService::write_json(&filepath, json)?;
                Ok(filepath.to_string_lossy().to_string())
            }
            _ => Err(DecompilerError::Decompile(
                "NEKO反编译器应返回JSON".to_string(),
            )),
        }
    }
}

// ============ NEMO作品资源管理器 ============
pub struct NemoResourceManager {
    context: DecompilerContext,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
}

impl NemoResourceManager {
    pub fn new(context: DecompilerContext, work_dir: PathBuf) -> Self {
        Self {
            context,
            work_dir,
            dirs: HashMap::new(),
        }
    }

    pub fn create_directories(&mut self, work_id: i64) -> Result<&HashMap<String, PathBuf>> {
        self.dirs.insert(
            "material".to_string(),
            FileService::ensure_dir(&self.work_dir.join("user_material"))?,
        );
        self.dirs.insert(
            "works".to_string(),
            FileService::ensure_dir(&self.work_dir.join("user_works").join(work_id.to_string()))?,
        );
        self.dirs.insert(
            "record".to_string(),
            FileService::ensure_dir(
                &self
                    .work_dir
                    .join("user_works")
                    .join(work_id.to_string())
                    .join("record"),
            )?,
        );

        Ok(&self.dirs)
    }

    pub fn save_core_files(
        &self,
        work_id: i64,
        bcm_data: &Value,
        source_info: &Value,
    ) -> Result<()> {
        let bcm_path = self
            .dirs
            .get("works")
            .ok_or_else(|| DecompilerError::Other("works目录不存在".to_string()))?
            .join(format!("{}.bcm", work_id));
        FileService::write_json(&bcm_path, bcm_data)?;

        let user_images = self.build_user_images(bcm_data)?;
        let userimg_path = self
            .dirs
            .get("works")
            .unwrap()
            .join(format!("{}.userimg", work_id));
        FileService::write_json(&userimg_path, &user_images)?;

        let meta_data = self.build_metadata(work_id, source_info)?;
        let meta_path = self
            .dirs
            .get("works")
            .unwrap()
            .join(format!("{}.meta", work_id));
        FileService::write_json(&meta_path, &meta_data)?;

        if let Some(preview) = source_info.get("preview").and_then(|v| v.as_str()) {
            if !preview.is_empty() {
                match self.context.http_client.get_binary(preview) {
                    Ok(cover_data) => {
                        let cover_path = self
                            .dirs
                            .get("works")
                            .unwrap()
                            .join(format!("{}.cover", work_id));
                        FileService::write_binary(&cover_path, &cover_data)?;
                    }
                    Err(e) => println!("封面下载失败: {}", e),
                }
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
                    let mut style_info = serde_json::Map::new();
                    style_info.insert("id".to_string(), Value::String(style_id.clone()));
                    style_info.insert(
                        "path".to_string(),
                        Value::String(format!(
                            "user_material/{}.webp",
                            CryptoService::sha256(image_url)
                        )),
                    );
                    img_dict.insert(style_id.clone(), Value::Object(style_info));
                }
            }
        }

        user_images.insert("user_img_dict".to_string(), Value::Object(img_dict));
        Ok(Value::Object(user_images))
    }

    fn build_metadata(&self, work_id: i64, source_info: &Value) -> Result<Value> {
        let work_name = source_info
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let work_urls = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let bcm_version = source_info
            .get("bcm_version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let preview = source_info
            .get("preview")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

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
                "work_id": work_id,
                "have_uploaded": 2,
            },
        }))
    }

    pub fn download_resources(&self, bcm_data: &Value) -> Result<()> {
        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for style_data in styles.values() {
                if let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) {
                    match self.context.http_client.get_binary(image_url) {
                        Ok(image_data) => {
                            let filename = format!("{}.webp", CryptoService::sha256(image_url));
                            let image_path = self.dirs.get("material").unwrap().join(filename);
                            FileService::write_binary(&image_path, &image_data)?;
                        }
                        Err(e) => println!("资源下载失败 {}: {}", image_url, e),
                    }
                }
            }
        }
        Ok(())
    }
}

// ============ NEMO反编译器 ============
pub struct NemoDecompiler {
    context: DecompilerContext,
}

impl NemoDecompiler {
    pub fn new(context: DecompilerContext) -> Self {
        Self { context }
    }
}

impl BaseDecompiler for NemoDecompiler {
    fn decompile(&mut self) -> Result<DecompileResult> {
        let work_id = self.context.work_info.id;
        let folder_name = FileService::safe_filename(&self.context.work_info.name, work_id, "");

        let base_dir = &self.context.config.default_output_dir;
        let work_dir = base_dir.join(folder_name);

        let source_url = format!(
            "{}/creation-tools/v1/works/{}/source/public",
            self.context.config.base_url, work_id
        );

        let source_info = self.context.http_client.get_json(&source_url, None)?;

        let bcm_url = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取work_urls".to_string()))?;

        let bcm_data = self.context.http_client.get_json(bcm_url, None)?;

        let mut resource_manager = NemoResourceManager::new(
            DecompilerContext {
                work_info: self.context.work_info.clone(),
                http_client: self.context.http_client.clone(),
                file_service: FileService::new(self.context.config.clone()),
                id_generator: IdGenerator::new(),
                config: self.context.config.clone(),
                crypto_service: None,
            },
            work_dir.clone(),
        );

        resource_manager.create_directories(work_id)?;
        resource_manager.save_core_files(work_id, &bcm_data, &source_info)?;
        resource_manager.download_resources(&bcm_data)?;

        println!("NEMO作品解密成功!");
        println!("将反编译的文件复制到: /data/data/com.codemao.nemo/files/nemo_users_db");

        Ok(DecompileResult::Path(
            work_dir.to_string_lossy().to_string(),
        ))
    }

    fn save_result(&self, result: &DecompileResult, _output_dir: Option<&Path>) -> Result<String> {
        match result {
            DecompileResult::Path(path) => Ok(path.clone()),
            _ => Err(DecompilerError::Decompile(
                "NEMO反编译器应返回路径".to_string(),
            )),
        }
    }
}

// ============ 积木反编译器接口 ============
pub trait BlockDecompiler: Send + Sync {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value>;
}

// ============ 具体积木反编译器实现 ============
pub struct DefaultBlockDecompiler {
    core: BlockDecompilerCore,
}

impl DefaultBlockDecompiler {
    pub fn new(compiled: Value, config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        Self {
            core: BlockDecompilerCore::new(
                compiled,
                config,
                id_generator,
                Box::new(DefaultBlockBehavior),
            ),
        }
    }
}

impl BlockDecompiler for DefaultBlockDecompiler {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        self.core.decompile(context)
    }
}

pub struct IfBlockDecompiler {
    core: BlockDecompilerCore,
    compiled: Value,
    config: Arc<DecompilerConfig>,
}

impl IfBlockDecompiler {
    pub fn new(compiled: Value, config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        let conditions_count = compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0);

        let behavior = Box::new(IfBlockBehavior::new(conditions_count));
        let core =
            BlockDecompilerCore::new(compiled.clone(), config.clone(), id_generator, behavior);

        Self {
            core,
            compiled,
            config,
        }
    }
}

impl BlockDecompiler for IfBlockDecompiler {
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

        if children.len() == 2 && children[1].is_null() {
            context
                .shadows
                .insert("EXTRA_ADD_ELSE".to_string(), String::new());
        } else {
            let mutation = format!(
                r#"<mutation elseif="{}" else="1"></mutation>"#,
                conditions_len.saturating_sub(1)
            );
            let block = block_value.as_object_mut().unwrap();
            block.insert("mutation".to_string(), Value::String(mutation));
            context
                .shadows
                .insert("ELSE_TEXT".to_string(), String::new());
        }

        Ok(block_value)
    }
}

pub struct TextJoinDecompiler {
    core: BlockDecompilerCore,
    compiled: Value,
    config: Arc<DecompilerConfig>,
}

impl TextJoinDecompiler {
    pub fn new(compiled: Value, config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        Self {
            core: BlockDecompilerCore::new(
                compiled.clone(),
                config.clone(),
                id_generator,
                Box::new(DefaultBlockBehavior),
            ),
            compiled,
            config,
        }
    }
}

impl BlockDecompiler for TextJoinDecompiler {
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

pub struct FunctionDefDecompiler {
    core: BlockDecompilerCore,
    compiled: Value,
    config: Arc<DecompilerConfig>,
    id_generator: IdGenerator,
}

impl FunctionDefDecompiler {
    pub fn new(compiled: Value, config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        Self {
            core: BlockDecompilerCore::new(
                compiled.clone(),
                config.clone(),
                id_generator.clone(),
                Box::new(DefaultBlockBehavior),
            ),
            compiled,
            config,
            id_generator,
        }
    }
}

impl BlockDecompiler for FunctionDefDecompiler {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;

        let procedure_name = self
            .compiled
            .get("procedure_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .unwrap_or(&serde_json::Map::new())
            .clone();

        let block_id = {
            let block = block_value.as_object().unwrap();
            block.get("id").unwrap().as_str().unwrap().to_string()
        };

        let block = block_value.as_object_mut().unwrap();

        context
            .shadows
            .insert("PROCEDURES_2_DEFNORETURN_DEFINE".to_string(), String::new());
        context.shadows.insert(
            "PROCEDURES_2_DEFNORETURN_MUTATOR".to_string(),
            String::new(),
        );

        let mut mutation_args = Vec::new();
        for (i, (param_name, _)) in params.iter().enumerate() {
            let input_name = format!("PARAMS {}", i);

            mutation_args.push(format!(r#"<arg name="{}"/>"#, input_name));

            let shadow_value = context.shadow_builder.create("math_number", None, None);
            context.shadows.insert(input_name.clone(), shadow_value);

            let param_block_id = self.id_generator.generate(20);
            let param_block = json!({
                "id": param_block_id,
                "kind": "domain_block",
                "type": "procedures_2_stable_parameter",
                "params": {
                    "param_name": param_name,
                    "param_default_value": "",
                }
            });
            let mut param_decompiler = DefaultBlockDecompiler::new(
                param_block,
                self.config.clone(),
                self.id_generator.clone(),
            );
            let mut param_block_value = param_decompiler.decompile(context)?;
            if let Some(obj) = param_block_value.as_object_mut() {
                obj.insert("parent_id".to_string(), Value::String(block_id.clone()));
            }
            context
                .blocks
                .insert(param_block_id.clone(), param_block_value);
            context.connections.insert(
                param_block_id,
                json!({
                    "type": "input",
                    "input_type": "value",
                    "input_name": input_name
                }),
            );
        }

        let mutation = if mutation_args.is_empty() {
            "<mutation></mutation>".to_string()
        } else {
            format!("<mutation>{}</mutation>", mutation_args.join(""))
        };
        block.insert("mutation".to_string(), Value::String(mutation));

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .unwrap();
        fields.insert("NAME".to_string(), Value::String(procedure_name));

        Ok(block_value)
    }
}

pub struct FunctionCallDecompiler {
    core: BlockDecompilerCore,
    compiled: Value,
    config: Arc<DecompilerConfig>,
    id_generator: IdGenerator,
}

impl FunctionCallDecompiler {
    pub fn new(compiled: Value, config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        Self {
            core: BlockDecompilerCore::new(
                compiled.clone(),
                config.clone(),
                id_generator.clone(),
                Box::new(DefaultBlockBehavior),
            ),
            compiled,
            config,
            id_generator,
        }
    }
}

impl BlockDecompiler for FunctionCallDecompiler {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;

        let procedure_name = self
            .compiled
            .get("procedure_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let func_id = context
            .functions
            .get(&procedure_name)
            .and_then(|f| f.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or(&self.id_generator.generate(20))
            .to_string();

        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .unwrap_or(&serde_json::Map::new())
            .clone();

        let block_id = {
            let block = block_value.as_object().unwrap();
            block.get("id").unwrap().as_str().unwrap().to_string()
        };

        let block = block_value.as_object_mut().unwrap();

        if !context.functions.contains_key(&procedure_name) {
            block.insert("disabled".to_string(), Value::Bool(true));
        }

        let mut mutation = String::from("<mutation");
        mutation.push_str(&format!(r#" name="{}""#, procedure_name));
        mutation.push_str(&format!(r#" def_id="{}""#, func_id));
        mutation.push_str(">");
        for (param_name, _) in params.iter() {
            mutation.push_str(&format!(
                r#"<procedures_2_parameter_shadow name="{}" value="0"/>"#,
                param_name
            ));
        }
        mutation.push_str("</mutation>");
        block.insert("mutation".to_string(), Value::String(mutation));

        context.shadows.insert("NAME".to_string(), String::new());

        let mut param_index = 0;
        for (param_name, param_value) in params.iter() {
            let input_name = format!("ARG {}", param_index);

            if let Some(param_block_data) = param_value.as_object() {
                let mut param_decompiler = BlockDecompilerCore::new(
                    param_value.clone(),
                    self.config.clone(),
                    self.id_generator.clone(),
                    Box::new(DefaultBlockBehavior),
                );
                let mut param_block = param_decompiler.decompile(context)?;

                if let Some(obj) = param_block.as_object_mut() {
                    obj.insert("parent_id".to_string(), Value::String(block_id.clone()));
                }

                let param_id = param_block.get("id").unwrap().as_str().unwrap().to_string();
                context.blocks.insert(param_id.clone(), param_block);

                context.connections.insert(
                    param_id.clone(),
                    json!({
                        "type": "input",
                        "input_type": "value",
                        "input_name": input_name
                    }),
                );

                let shadow_xml =
                    context
                        .shadow_builder
                        .create("default_value", Some(param_id), None);
                context.shadows.insert(input_name, shadow_xml);
            } else {
                let shadow_xml = context.shadow_builder.create("default_value", None, None);
                context.shadows.insert(input_name, shadow_xml);
            }

            param_index += 1;
        }

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .unwrap();
        fields.insert("NAME".to_string(), Value::String(procedure_name.clone()));

        Ok(block_value)
    }
}

// ============ 积木反编译器工厂 ============
pub struct BlockDecompilerFactory {
    config: Arc<DecompilerConfig>,
    id_generator: IdGenerator,
}

impl BlockDecompilerFactory {
    pub fn new(config: Arc<DecompilerConfig>, id_generator: IdGenerator) -> Self {
        Self {
            config,
            id_generator,
        }
    }

    pub fn create(&self, compiled: Value) -> Box<dyn BlockDecompiler> {
        let block_type = compiled
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match block_type.as_str() {
            "controls_if" | "controls_if_no_else" => Box::new(IfBlockDecompiler::new(
                compiled,
                self.config.clone(),
                self.id_generator.clone(),
            )),
            "text_join" => Box::new(TextJoinDecompiler::new(
                compiled,
                self.config.clone(),
                self.id_generator.clone(),
            )),
            "procedures_2_defnoreturn" => Box::new(FunctionDefDecompiler::new(
                compiled,
                self.config.clone(),
                self.id_generator.clone(),
            )),
            "procedures_2_callnoreturn" | "procedures_2_callreturn" => {
                Box::new(FunctionCallDecompiler::new(
                    compiled,
                    self.config.clone(),
                    self.id_generator.clone(),
                ))
            }
            _ => Box::new(DefaultBlockDecompiler::new(
                compiled,
                self.config.clone(),
                self.id_generator.clone(),
            )),
        }
    }
}

// ============ KITTEN作品反编译器 ============
pub struct KittenDecompiler {
    context: DecompilerContext,
}

impl KittenDecompiler {
    pub fn new(context: DecompilerContext) -> Self {
        Self { context }
    }

    fn fetch_compiled_data(&self) -> Result<Value> {
        let url = format!(
            "{}/kitten/r2/work/player/load/{}",
            self.context.config.creation_base_url, self.context.work_info.id
        );
        let data = self.context.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.get(0))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;
        self.context.http_client.get_json(compiled_url, None)
    }

    fn get_actor_info(&self, work: &Value, actor_id: &str) -> Value {
        if let Some(theatre) = work.get("theatre").and_then(|v| v.as_object()) {
            if let Some(actors) = theatre.get("actors").and_then(|v| v.as_object()) {
                if let Some(actor) = actors.get(actor_id) {
                    return actor.clone();
                }
            }
            if let Some(scenes) = theatre.get("scenes").and_then(|v| v.as_object()) {
                if let Some(scene) = scenes.get(actor_id) {
                    return scene.clone();
                }
            }
        }
        json!({
            "direction": 90,
            "draggable": false,
            "id": actor_id,
            "name": format!("未知角色_{}", &actor_id[..actor_id.len().min(8)]),
            "rotation_style": "all around",
            "size": 100,
            "type": "sprite",
            "visible": true,
            "x": 0,
            "y": 0,
        })
    }

    fn decompile_actor_blocks(
        &self,
        actor_compiled: &Value,
        context: &mut BlockContext,
        factory: &BlockDecompilerFactory,
    ) -> Result<()> {
        if let Some(compiled_blocks) = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object())
        {
            for block_data in compiled_blocks.values() {
                let mut decompiler = factory.create(block_data.clone());
                let _ = decompiler.decompile(context)?;
            }
        }

        let actor_data_obj = context.actor_data.as_object_mut().unwrap();
        actor_data_obj.insert(
            "block_data_json".to_string(),
            json!({
                "blocks": context.blocks,
                "connections": context.connections,
                "comments": {},
            }),
        );
        Ok(())
    }

    fn update_work_info(&self, work: &mut Value) {
        let work_obj = work.as_object_mut().unwrap();
        work_obj.insert(
            "hidden_toolbox".to_string(),
            json!({
                "toolbox": [],
                "blocks": [],
            }),
        );
        work_obj.insert("work_source_label".to_string(), json!(0));
        work_obj.insert("sample_id".to_string(), json!(""));
        work_obj.insert(
            "project_name".to_string(),
            json!(self.context.work_info.name),
        );
        work_obj.insert(
            "toolbox_order".to_string(),
            json!(self.context.config.toolbox_categories),
        );
        work_obj.insert(
            "last_toolbox_order".to_string(),
            json!(self.context.config.toolbox_categories),
        );
    }

    fn clean_work_data(&self, work: &mut Value) {
        let work_obj = work.as_object_mut().unwrap();
        work_obj.remove("compile_result");
        work_obj.remove("preview");
        work_obj.remove("author_nickname");
    }
}

impl BaseDecompiler for KittenDecompiler {
    fn decompile(&mut self) -> Result<DecompileResult> {
        let mut work = self.fetch_compiled_data()?;
        let shadow_builder = ShadowBuilder::new(
            self.context.config.clone(),
            self.context.id_generator.clone(),
        );
        let compile_result = work
            .get("compile_result")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DecompilerError::InvalidResponse("compile_result不存在".to_string()))?
            .clone();

        let mut functions = HashMap::new();
        for actor_compiled in &compile_result {
            if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
                for (name, func_data) in procedures {
                    functions.insert(name.clone(), func_data.clone());
                }
            }
        }
        let functions = Arc::new(functions);

        let block_factory = BlockDecompilerFactory::new(
            self.context.config.clone(),
            self.context.id_generator.clone(),
        );
        let mut functions_result = HashMap::new();
        for (name, func_data) in functions.iter() {
            let mut func_context = BlockContext::new(
                json!({}),
                Arc::clone(&functions),
                ShadowBuilder::new(
                    self.context.config.clone(),
                    self.context.id_generator.clone(),
                ),
            );
            let mut decompiler = block_factory.create(func_data.clone());
            let decompiled = decompiler.decompile(&mut func_context)?;
            functions_result.insert(name.clone(), decompiled);
        }
        let functions_result = Arc::new(functions_result);

        for actor_compiled in &compile_result {
            let actor_id = actor_compiled
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let actor_info = self.get_actor_info(&work, &actor_id);
            let mut actor_context = BlockContext::new(
                actor_info,
                Arc::clone(&functions_result),
                ShadowBuilder::new(
                    self.context.config.clone(),
                    self.context.id_generator.clone(),
                ),
            );
            self.decompile_actor_blocks(actor_compiled, &mut actor_context, &block_factory)?;
        }

        self.update_work_info(&mut work);
        self.clean_work_data(&mut work);
        Ok(DecompileResult::Json(work))
    }

    fn save_result(&self, result: &DecompileResult, output_dir: Option<&Path>) -> Result<String> {
        match result {
            DecompileResult::Json(json) => {
                let output_path = output_dir.unwrap_or(&self.context.config.default_output_dir);
                FileService::ensure_dir(output_path)?;
                let filename = FileService::safe_filename(
                    &self.context.work_info.name,
                    self.context.work_info.id,
                    self.context
                        .work_info
                        .file_extension(&self.context.config)
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

// ============ COCO数据重组器（修复版） ============
pub struct CocoDataReorganizer {
    context: DecompilerContext,
}

impl CocoDataReorganizer {
    pub fn new(context: DecompilerContext) -> Self {
        Self { context }
    }

    pub fn reorganize(&self, work: &mut Value) -> Result<()> {
        let work_obj = work.as_object_mut().unwrap();

        work_obj.insert(
            "authorId".to_string(),
            json!(self.context.work_info.user_id),
        );
        work_obj.insert("title".to_string(), json!(self.context.work_info.name));
        work_obj.insert("screens".to_string(), json!({}));
        work_obj.insert("screenIds".to_string(), json!([]));

        self.process_screens(work_obj)?;
        self.process_blocks(work_obj)?;
        self.process_resources(work_obj);
        self.process_variables(work_obj)?;

        if let Some(widget_map) = work_obj.get("widgetMap").cloned() {
            work_obj.insert("globalWidgets".to_string(), widget_map);
        }

        if let Some(widget_map) = work_obj.get("widgetMap").and_then(|v| v.as_object()) {
            let widget_ids: Vec<String> = widget_map.keys().cloned().collect();
            work_obj.insert("globalWidgetIds".to_string(), json!(widget_ids));
        }

        work_obj.insert("sourceId".to_string(), json!(""));
        work_obj.insert("sourceTag".to_string(), json!(1));

        self.clean_data(work_obj);

        Ok(())
    }

    fn process_screens(&self, work: &mut serde_json::Map<String, Value>) -> Result<()> {
        let screen_list = work
            .get("screenList")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DecompilerError::InvalidResponse("screenList不存在".to_string()))?
            .clone();

        let mut screens = serde_json::Map::new();
        let mut screen_ids = Vec::new();

        let mut widget_map = work
            .get("widgetMap")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_else(|| serde_json::Map::new());

        for mut screen in screen_list {
            let screen_obj = screen.as_object_mut().unwrap();
            let screen_id = screen_obj
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
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

            for widget_id_value in widget_ids.iter().chain(invisible_widget_ids.iter()) {
                if let Some(widget_id) = widget_id_value.as_str() {
                    if let Some(widget) = widget_map.remove(widget_id) {
                        screen_widgets.insert(widget_id.to_string(), widget);
                    }
                }
            }

            screen_obj.insert("widgets".to_string(), Value::Object(screen_widgets));

            screens.insert(screen_id.clone(), Value::Object(screen_obj.clone()));
            screen_ids.push(Value::String(screen_id));
        }

        work.insert("screens".to_string(), Value::Object(screens));
        work.insert("screenIds".to_string(), Value::Array(screen_ids));
        work.insert("widgetMap".to_string(), Value::Object(widget_map));

        Ok(())
    }

    fn process_blocks(&self, work: &mut serde_json::Map<String, Value>) -> Result<()> {
        let block_json_map = work
            .get("blockJsonMap")
            .and_then(|v| v.as_object())
            .ok_or_else(|| DecompilerError::InvalidResponse("blockJsonMap不存在".to_string()))?;

        let mut blockly = serde_json::Map::new();

        for (screen_id, blocks) in block_json_map {
            let screen_data = json!({
                "screenId": screen_id,
                "workspaceJson": blocks,
                "workspaceOffset": {
                    "x": 0,
                    "y": 0
                }
            });
            blockly.insert(screen_id.clone(), screen_data);
        }

        work.insert("blockly".to_string(), Value::Object(blockly));

        Ok(())
    }

    fn process_resources(&self, work: &mut serde_json::Map<String, Value>) {
        let resource_maps = [
            ("imageFileMap", "imageFileList"),
            ("soundFileMap", "soundFileList"),
            ("iconFileMap", "iconFileList"),
            ("fontFileMap", "fontFileList"),
        ];

        for (map_name, list_name) in resource_maps {
            if let Some(map) = work.get(map_name).and_then(|v| v.as_object()) {
                let values: Vec<Value> = map.values().cloned().collect();
                work.insert(list_name.to_string(), Value::Array(values));
            }
        }
    }

    fn process_variables(&self, work: &mut serde_json::Map<String, Value>) -> Result<()> {
        let variable_map = work
            .get("variableMap")
            .and_then(|v| v.as_object())
            .ok_or_else(|| DecompilerError::InvalidResponse("variableMap不存在".to_string()))?;

        let mut var_list = Vec::new();
        let mut list_list = Vec::new();
        let mut dict_list = Vec::new();

        let mut var_counter = 0;
        let mut list_counter = 0;
        let mut dict_counter = 0;

        for (var_id, value) in variable_map {
            if let Some(arr) = value.as_array() {
                list_counter += 1;
                list_list.push(json!({
                    "id": var_id,
                    "name": format!("列表{}", list_counter),
                    "defaultValue": arr,
                    "value": arr,
                }));
            } else if let Some(obj) = value.as_object() {
                dict_counter += 1;
                dict_list.push(json!({
                    "id": var_id,
                    "name": format!("字典{}", dict_counter),
                    "defaultValue": obj,
                    "value": obj,
                }));
            } else {
                var_counter += 1;
                var_list.push(json!({
                    "id": var_id,
                    "name": format!("变量{}", var_counter),
                    "defaultValue": value,
                    "value": value,
                }));
            }
        }

        work.insert("globalVariableList".to_string(), json!(var_list));
        work.insert("globalArrayList".to_string(), json!(list_list));
        work.insert("globalObjectList".to_string(), json!(dict_list));

        Ok(())
    }

    fn clean_data(&self, work: &mut serde_json::Map<String, Value>) {
        let remove_keys = [
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
        ];

        for key in &remove_keys {
            work.remove(*key);
        }
    }
}

// ============ COCO反编译器 ============
pub struct CocoDecompiler {
    context: DecompilerContext,
}

impl CocoDecompiler {
    pub fn new(context: DecompilerContext) -> Self {
        Self { context }
    }

    fn fetch_compiled_data(&self) -> Result<Value> {
        let url = format!(
            "{}/coconut/web/work/{}/load",
            self.context.config.creation_base_url, self.context.work_info.id
        );

        let data = self.context.http_client.get_json(&url, None)?;

        let compiled_url = data
            .get("data")
            .and_then(|v| v.get("bcmc_url"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取bcmc_url".to_string()))?;

        self.context.http_client.get_json(compiled_url, None)
    }
}

impl BaseDecompiler for CocoDecompiler {
    fn decompile(&mut self) -> Result<DecompileResult> {
        let mut work = self.fetch_compiled_data()?;

        let reorganizer = CocoDataReorganizer::new(DecompilerContext {
            work_info: self.context.work_info.clone(),
            http_client: self.context.http_client.clone(),
            file_service: FileService::new(self.context.config.clone()),
            id_generator: IdGenerator::new(),
            config: self.context.config.clone(),
            crypto_service: None,
        });

        reorganizer.reorganize(&mut work)?;

        Ok(DecompileResult::Json(work))
    }

    fn save_result(&self, result: &DecompileResult, output_dir: Option<&Path>) -> Result<String> {
        match result {
            DecompileResult::Json(json) => {
                let output_path = output_dir.unwrap_or(&self.context.config.default_output_dir);
                FileService::ensure_dir(output_path)?;

                let filename = FileService::safe_filename(
                    &self.context.work_info.name,
                    self.context.work_info.id,
                    self.context
                        .work_info
                        .file_extension(&self.context.config)
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

// ============ WOOD作品资源管理器 ============
pub struct WoodResourceManager {
    context: DecompilerContext,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
}

impl WoodResourceManager {
    pub fn new(context: DecompilerContext, work_dir: PathBuf) -> Self {
        Self {
            context,
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
        let work_id = work_data
            .get("work_id")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let work_name = work_data
            .get("work_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let language_type = work_data
            .get("language_type")
            .and_then(|v| v.as_i64())
            .unwrap_or(3);

        let run_mode = work_data
            .get("run_mode")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        let code_visible = work_data
            .get("code_visible")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let addition = work_data.get("addition").cloned().unwrap_or(json!({}));

        let work_info = json!({
            "id": work_id,
            "name": work_name,
            "type": "WOOD",
            "language_type": language_type,
            "run_mode": run_mode,
            "code_visible": code_visible,
            "addition": addition,
        });

        let info_path = self.dirs.get("root").unwrap().join("work_info.json");

        FileService::write_json(&info_path, &work_info)?;

        Ok(())
    }

    fn save_code_files(&self, work_data: &Value) -> Result<()> {
        if let Some(content) = work_data.get("content").and_then(|v| v.as_array()) {
            for file_info in content {
                if let Some(file_type) = file_info.get("file_type").and_then(|v| v.as_i64()) {
                    if file_type == 2 {
                        if let Some(file_name) = file_info.get("file_name").and_then(|v| v.as_str())
                        {
                            if file_name.ends_with(".py") {
                                if let Some(source) =
                                    file_info.get("source").and_then(|v| v.as_str())
                                {
                                    let file_path = self.dirs.get("root").unwrap().join(file_name);
                                    std::fs::write(file_path, source)?;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn extract_filename_from_url(&self, url: &str) -> String {
        if let Some(last_slash) = url.rfind('/') {
            let filename_part = &url[last_slash + 1..];

            if let Some(question_mark) = filename_part.find('?') {
                return filename_part[..question_mark].to_string();
            }

            if let Some(hash_mark) = filename_part.find('#') {
                return filename_part[..hash_mark].to_string();
            }

            return filename_part.to_string();
        }

        String::new()
    }

    fn download_images(&self, work_data: &Value) -> Result<()> {
        if let Some(content) = work_data.get("content").and_then(|v| v.as_array()) {
            for file_info in content {
                if let Some(file_type) = file_info.get("file_type").and_then(|v| v.as_i64()) {
                    if file_type == 3 {
                        if let Some(image_url) = file_info.get("url").and_then(|v| v.as_str()) {
                            match self.context.http_client.get_binary(image_url) {
                                Ok(image_data) => {
                                    let file_name = file_info
                                        .get("file_name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();

                                    let file_name = if file_name.is_empty() {
                                        self.extract_filename_from_url(image_url)
                                    } else {
                                        file_name
                                    };

                                    let file_name = if file_name.is_empty() {
                                        "image.png".to_string()
                                    } else {
                                        file_name
                                    };

                                    let file_path =
                                        self.dirs.get("images").unwrap().join(file_name);

                                    FileService::write_binary(&file_path, &image_data)?;
                                }
                                Err(e) => println!("图片下载失败 {}: {}", image_url, e),
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ============ WOOD反编译器 ============
pub struct WoodDecompiler {
    context: DecompilerContext,
}

impl WoodDecompiler {
    pub fn new(context: DecompilerContext) -> Self {
        Self { context }
    }
}

impl BaseDecompiler for WoodDecompiler {
    fn decompile(&mut self) -> Result<DecompileResult> {
        let work_id = self.context.work_info.id;
        let folder_name = FileService::safe_filename(&self.context.work_info.name, work_id, "");

        let base_dir = &self.context.config.default_output_dir;
        let work_dir = base_dir.join(folder_name);

        let publish_url = format!(
            "{}/wood/work/{}/publish?channel_type=0",
            self.context.config.creation_base_url, work_id
        );

        let work_data = self.context.http_client.get_json(&publish_url, None)?;

        let mut resource_manager = WoodResourceManager::new(
            DecompilerContext {
                work_info: self.context.work_info.clone(),
                http_client: self.context.http_client.clone(),
                file_service: FileService::new(self.context.config.clone()),
                id_generator: IdGenerator::new(),
                config: self.context.config.clone(),
                crypto_service: None,
            },
            work_dir.clone(),
        );

        resource_manager.create_directories()?;
        resource_manager.save_work_files(&work_data)?;

        Ok(DecompileResult::Path(
            work_dir.to_string_lossy().to_string(),
        ))
    }

    fn save_result(&self, result: &DecompileResult, _output_dir: Option<&Path>) -> Result<String> {
        match result {
            DecompileResult::Path(path) => Ok(path.clone()),
            _ => Err(DecompilerError::Decompile(
                "WOOD反编译器应返回路径".to_string(),
            )),
        }
    }
}

// ============ 反编译器工厂 ============
pub struct DecompilerFactory;

impl DecompilerFactory {
    pub fn create(
        work_info: WorkInfo,
        context: DecompilerContext,
    ) -> Result<Box<dyn BaseDecompiler>> {
        match work_info.work_type {
            WorkType::NEKO => Ok(Box::new(NekoDecompiler::new(context))),
            WorkType::NEMO => Ok(Box::new(NemoDecompiler::new(context))),
            WorkType::WOOD => Ok(Box::new(WoodDecompiler::new(context))),
            WorkType::KITTEN2 | WorkType::KITTEN3 | WorkType::KITTEN4 => {
                Ok(Box::new(KittenDecompiler::new(context)))
            }
            WorkType::COCO => Ok(Box::new(CocoDecompiler::new(context))),
        }
    }
}

// ============ 主接口 ============
pub struct CodemaoDecompiler {
    config: Arc<DecompilerConfig>,
    client: CodeMaoClient,
    id_generator: IdGenerator,
}

impl CodemaoDecompiler {
    pub fn new(config: Option<DecompilerConfig>) -> Self {
        let config = Arc::new(config.unwrap_or_default());
        Self {
            config,
            client: ClientFactory::create_global_client(None),
            id_generator: IdGenerator::new(),
        }
    }

    pub fn decompile(&self, work_id: i64, output_dir: Option<&Path>) -> Result<String> {
        let context = self.create_context(work_id)?;
        let mut decompiler = DecompilerFactory::create(context.work_info.clone(), context)?;
        let result = decompiler.decompile()?;
        decompiler.save_result(&result, output_dir)
    }

    fn create_context(&self, work_id: i64) -> Result<DecompilerContext> {
        let raw_client = CodeMaoClient::global();
        let http_client = CodeMaoHttpClient::new(raw_client);
        let work_info = self.fetch_work_info(&http_client, work_id)?;
        let crypto_service = if work_info.work_type.is_neko() {
            Some(CryptoService::new(&self.config.crypto_salt))
        } else {
            None
        };
        Ok(DecompilerContext {
            work_info,
            http_client: Box::new(http_client),
            file_service: FileService::new(self.config.clone()),
            id_generator: self.id_generator.clone(),
            config: self.config.clone(),
            crypto_service,
        })
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

pub fn decompile_work(work_id: i64, output_dir: Option<&Path>) -> Result<String> {
    let decompiler = CodemaoDecompiler::new(None);
    decompiler.decompile(work_id, output_dir)
}
