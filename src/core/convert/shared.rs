//! 作品文件转换域共享地基:`decompile`(读)与 `translate`(写)两个子域**真正共用**的东西 ——
//! 错误模型、JSON 取值扩展、编辑器/作品 id 领域类型、id 生成器、文件服务、加密、HTTP。
//!
//! 只装两个子域都在用的东西:上传编排在域级工具 `upload.rs`;反编译私有的配置大表与
//! 影子构建器在 `decompile/{config,shadow}.rs`,`WorkInfo` / `RawWorkData` / `WorkFetcher`
//! 在 `decompile/work.rs`(见 `docs/rounds/39` §W2a / §W2b)。
//!
//! 不对外暴露;对外只需要 `convert/mod.rs` 里那一条 `pub use`
//! (`ConvertError` / `EditorType` / `WorkId`)。
//!
//! 本文件由 `shared/{mod,error,model,infra}.rs` 合并而成(见
//! `docs/rounds/31-convert-layout-consolidation-plan.md`),分节注释保留原文件的模块说明。

// ===== 外部依赖 =====
use crate::utils::requests::MewError;
use crate::utils::requests::{CodeMaoClient, DOWNLOAD_TIMEOUT, HttpMethod};
use aes_gcm::aead::array::Array;
use aes_gcm::aead::array::typenum::{U12, U32};
use base64::{Engine as _, engine::general_purpose};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::error::Error;
// 注意:`thiserror::Error` 是**派生宏**(宏命名空间),与上面 std 的 trait 同名但可共存
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

// ---------------------------------------------------------------------------
// 来自 shared/error.rs

// 错误定义
#[derive(Error, Debug)]
pub enum ConvertError {
    #[error("外部错误: {0}")]
    Mew(#[from] MewError),
    #[error("加密错误: {0}")]
    Crypto(String),
    #[error("作品解析失败: {0}")]
    Decompile(String),
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

impl From<std::io::Error> for ConvertError {
    fn from(e: std::io::Error) -> Self {
        ConvertError::Mew(e.into())
    }
}

impl From<serde_json::Error> for ConvertError {
    fn from(e: serde_json::Error) -> Self {
        ConvertError::Mew(e.into())
    }
}

pub(crate) type Result<T> = std::result::Result<T, ConvertError>;

// 错误上下文扩展
pub(crate) trait ResultExt<T> {
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|e| ConvertError::Other {
            msg: f(),
            source: Some(Box::new(e)),
        })
    }
}

// ---------------------------------------------------------------------------
// 来自 shared/model.rs
// 领域模型:编辑器判别、id 生成器(作品信息 / 抓取契约已归位 `decompile/work.rs`,见 §W2b)。

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
// 反编译子域的配置与影子大表在 `decompile/{config,shadow}.rs`(消费者只有 decompile;见 `docs/rounds/39` §W2b)

// ---------------------------------------------------------------------------
// 来自 shared/infra.rs
// 反编译引擎的基础设施:加密(BCMKN/AES-GCM)、HTTP 客户端、文件落盘、JSON 取值扩展。
// 这些是**无状态工具**,只被 `decompile` / `translate` 的动作层调用;
// 与「配置大表」(现在在 `decompile/config.rs`)、「领域模型」(现在在 `decompile/work.rs`)刻意分开。

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
            .map_err(|e| ConvertError::Crypto(format!("Base64解码失败: {}", e)))
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
            .map_err(|e| ConvertError::Crypto(format!("Invalid AES key: {}", e)))?;
        let cipher = Aes256Gcm::new(&key_array);
        let nonce = Nonce::try_from(iv)
            .map_err(|e| ConvertError::Crypto(format!("Invalid nonce: {}", e)))?;

        cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| ConvertError::Crypto(format!("AES解密失败: {}", e)))
    }

    pub(crate) fn decrypt_bcmkn(&self, encrypted_content: &str) -> Result<Vec<u8>> {
        let reversed = Self::reverse_string(encrypted_content);
        let decoded = Self::base64_to_bytes(&reversed)?;
        if decoded.len() <= NONCE_SIZE {
            return Err(ConvertError::Crypto(format!(
                "数据长度 {} 不足,至少需要 {} 字节",
                decoded.len(),
                NONCE_SIZE + 1
            )));
        }
        let (iv, ciphertext) = decoded
            .split_at_checked(NONCE_SIZE)
            .ok_or_else(|| ConvertError::Crypto("IV 长度不足".into()))?;
        self.decrypt_aes_gcm(ciphertext, iv)
    }

    /// NEKO 播放器下发的密文 → KN 文档 JSON:
    /// `base64(reverse(content))` → AES-GCM(前 12 字节为 IV)→ UTF-8 → JSON
    pub(crate) fn decrypt_bcmkn_json(&self, encrypted_content: &str) -> Result<Value> {
        let decrypted_bytes = self.decrypt_bcmkn(encrypted_content)?;
        let decrypted_str = String::from_utf8(decrypted_bytes)
            .map_err(|e| ConvertError::Crypto(format!("UTF-8转换失败: {}", e)))?;
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
        let mut request_builder = self
            .client
            .build_request(HttpMethod::Get, url, None)
            // 下载侧请求级超时覆盖(见 [`DOWNLOAD_TIMEOUT`]);全局默认 30 s 兜不住大作品
            .with_timeout(DOWNLOAD_TIMEOUT);
        if let Some(headers_map) = headers {
            request_builder = request_builder.with_headers(headers_map);
        }
        let response = request_builder.send()?;
        // 作品文档可到几十 MB,用大体读取(10 MB 默认上限会先失败)
        Ok(self.client.response_to_json_large(response, url)?)
    }

    fn get_binary(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .with_timeout(DOWNLOAD_TIMEOUT)
            .send()?;
        Ok(self.client.response_to_binary_large(response, url)?)
    }

    fn get_text(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .build_request(HttpMethod::Get, url, None)
            .with_timeout(DOWNLOAD_TIMEOUT)
            .send()?;
        Ok(self.client.response_to_string_large(response, url)?)
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
