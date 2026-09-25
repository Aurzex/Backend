//! 反编译引擎的基础设施:加密(BCMKN/AES-GCM)、HTTP 客户端、文件落盘、JSON 取值扩展。
//!
//! 这些是**无状态工具**,只被 `decompile` / `translate` 的动作层调用;
//! 与「配置大表」(`config.rs`)、「领域模型」(`model.rs`)刻意分开。

use super::config::DecompilerConfig;
use super::error::{DecompilerError, Result};
use crate::utils::requests::{CodeMaoClient, HttpMethod};
use aes_gcm::aead::array::Array;
use aes_gcm::aead::array::typenum::{U12, U32};
use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose};
use serde_json::{Value, to_string};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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

    /// 写 JSON:**流式**(`to_writer` + `BufWriter`),不产生整份中间 `String`。
    ///
    /// 与 `to_string` 逐字节相同(同一个序列化器),只省掉"文档大小 ×1 的中间串
    /// + 一次整块拷贝"与相应峰值内存(方案 23 P0-1)。10 MB 级作品实测占
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
