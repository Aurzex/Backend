use rand::RngExt;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;
use ureq::http::Response;
use ureq::unversioned::multipart::Form;
use ureq::{Agent, Body, RequestBuilder};

// ==================== 错误定义 ====================
#[derive(Debug)]
pub enum Error {
    Http(ureq::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Auth(String),
    Pagination(String),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Http(e) => write!(f, "HTTP error: {}", e),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Json(e) => write!(f, "JSON error: {}", e),
            Error::Auth(e) => write!(f, "Auth error: {}", e),
            Error::Pagination(e) => write!(f, "Pagination error: {}", e),
            Error::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<ureq::Error> for Error {
    fn from(err: ureq::Error) -> Self {
        Error::Http(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// ==================== 常量定义 ====================
/// 预定义的基础 URL 静态切片
const BASE_URLS: &[(&str, &str)] = &[
    ("default", "https://api.codemao.cn"),
    ("creation", "https://api-creation.codemao.cn"),
    ("whale", "https://api-whale.codemao.cn"),
    ("education", "https://eduzone.codemao.cn"),
];

/// 默认请求头静态切片
const DEFAULT_HEADERS: &[(&str, &str)] = &[
    (
        "User-Agent",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    ),
    ("Accept-Encoding", "gzip, deflate, br, zstd"),
    (
        "Accept-Language",
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6",
    ),
];

// ==================== 身份枚举 ====================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Identity {
    Average,
    Edu,
    Judgement,
    Blank,
}

impl Identity {
    /// 转换为数组索引 (0..3)
    fn index(self) -> usize {
        match self {
            Identity::Average => 0,
            Identity::Edu => 1,
            Identity::Judgement => 2,
            Identity::Blank => 3,
        }
    }

    /// 所有身份变体
    pub const ALL: [Identity; 4] = [
        Identity::Average,
        Identity::Edu,
        Identity::Judgement,
        Identity::Blank,
    ];
}

impl FromStr for Identity {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "average" => Ok(Identity::Average),
            "edu" => Ok(Identity::Edu),
            "judgement" => Ok(Identity::Judgement),
            "blank" => Ok(Identity::Blank),
            _ => Err(Error::Auth(format!("invalid identity: {}", s))),
        }
    }
}

impl AsRef<str> for Identity {
    fn as_ref(&self) -> &str {
        match self {
            Identity::Average => "average",
            Identity::Edu => "edu",
            Identity::Judgement => "judgement",
            Identity::Blank => "blank",
        }
    }
}

// ==================== 身份管理器（核心单例）====================
/// 全局身份管理器，使用固定长度数组存储令牌
#[derive(Debug)]
pub struct GlobalIdentityManager {
    tokens: [Option<String>; 4],
    current: Identity,
}

impl GlobalIdentityManager {
    fn new() -> Self {
        Self {
            tokens: Default::default(),
            current: Identity::Average,
        }
    }

    /// 设置指定身份的令牌
    fn set_token(&mut self, identity: Identity, token: impl Into<String>) {
        let token = token.into();
        if !token.is_empty() {
            self.tokens[identity.index()] = Some(token);
        }
    }

    /// 切换到指定身份
    fn switch_identity(&mut self, identity: Identity) -> Result<()> {
        if identity == Identity::Blank || self.tokens[identity.index()].is_some() {
            self.current = identity;
            Ok(())
        } else {
            Err(Error::Auth(format!("No token for identity {:?}", identity)))
        }
    }

    /// 当前身份
    fn current_identity(&self) -> Identity {
        self.current
    }

    /// 当前身份对应的令牌
    fn current_token(&self) -> Option<&str> {
        self.tokens[self.current.index()].as_deref()
    }

    /// 生成认证头
    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.current_token()
            .map(|token| ("Authorization", format!("Bearer {}", token)))
    }
}

/// 全局身份管理器单例
static GLOBAL_IDENTITY_MANAGER: OnceLock<Arc<RwLock<GlobalIdentityManager>>> = OnceLock::new();

fn get_global_identity_manager() -> Arc<RwLock<GlobalIdentityManager>> {
    GLOBAL_IDENTITY_MANAGER
        .get_or_init(|| Arc::new(RwLock::new(GlobalIdentityManager::new())))
        .clone()
}

// ==================== 客户端配置 ====================
#[derive(Debug, Clone)]
pub struct ClientConfig {
    default_base_url_key: &'static str,
    timeout: Duration,
    log_requests: bool,
    /// 是否使用全局身份管理器
    use_global_auth: bool,
}

impl ClientConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取指定 key 的基础 URL
    pub fn get_base_url(&self, key: Option<&str>) -> &'static str {
        let key = key.unwrap_or(self.default_base_url_key);
        BASE_URLS
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| {
                BASE_URLS
                    .iter()
                    .find(|(k, _)| *k == self.default_base_url_key)
                    .expect("default base url must exist")
                    .1
            })
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_log_requests(mut self, log: bool) -> Self {
        self.log_requests = log;
        self
    }

    /// 设置为独立身份模式（不使用全局身份管理器）
    pub fn with_independent_auth(mut self) -> Self {
        self.use_global_auth = false;
        self
    }
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            default_base_url_key: "default",
            timeout: Duration::from_secs(30),
            log_requests: true,
            use_global_auth: true, // 默认使用全局身份管理器
        }
    }
}

// ==================== HTTP 方法 ====================
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpMethod {
    GET,
    POST,
    DELETE,
    PATCH,
    PUT,
    HEAD,
}

impl From<HttpMethod> for &'static str {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::PUT => "PUT",
            HttpMethod::HEAD => "HEAD",
        }
    }
}

// ==================== 独立身份管理器 ====================
/// 用于独立客户端的身份管理器
#[derive(Debug, Clone)]
pub struct LocalIdentityManager {
    tokens: [Option<String>; 4],
    current: Identity,
}

impl LocalIdentityManager {
    pub fn new() -> Self {
        Self {
            tokens: Default::default(),
            current: Identity::Average,
        }
    }

    pub fn set_token(&mut self, identity: Identity, token: impl Into<String>) {
        let token = token.into();
        if !token.is_empty() {
            self.tokens[identity.index()] = Some(token);
        }
    }

    pub fn switch_identity(&mut self, identity: Identity) -> Result<()> {
        if identity == Identity::Blank || self.tokens[identity.index()].is_some() {
            self.current = identity;
            Ok(())
        } else {
            Err(Error::Auth(format!("No token for identity {:?}", identity)))
        }
    }

    pub fn current_identity(&self) -> Identity {
        self.current
    }

    pub fn current_token(&self) -> Option<&str> {
        self.tokens[self.current.index()].as_deref()
    }

    pub fn auth_header(&self) -> Option<(&'static str, String)> {
        self.current_token()
            .map(|token| ("Authorization", format!("Bearer {}", token)))
    }
}

// ==================== 认证特质 ====================
/// 认证提供者特质，允许不同的认证实现
pub trait AuthProvider: Send + Sync + std::fmt::Debug {
    fn current_identity(&self) -> Identity;
    fn current_token(&self) -> Option<String>;
    fn auth_header(&self) -> Option<(&'static str, String)>;
    fn set_token(&self, identity: Identity, token: String) -> Result<()>;
    fn switch_identity(&self, identity: Identity) -> Result<()>;
}

/// 全局认证提供者
#[derive(Debug, Clone)]
pub struct GlobalAuthProvider;

impl GlobalAuthProvider {
    pub fn new() -> Self {
        Self
    }
}

impl AuthProvider for GlobalAuthProvider {
    fn current_identity(&self) -> Identity {
        get_global_identity_manager()
            .read()
            .map(|guard| guard.current_identity())
            .unwrap_or(Identity::Blank)
    }

    fn current_token(&self) -> Option<String> {
        get_global_identity_manager()
            .read()
            .ok()
            .and_then(|guard| guard.current_token().map(String::from))
    }

    fn auth_header(&self) -> Option<(&'static str, String)> {
        get_global_identity_manager()
            .read()
            .ok()
            .and_then(|guard| guard.auth_header())
    }

    fn set_token(&self, identity: Identity, token: String) -> Result<()> {
        get_global_identity_manager()
            .write()
            .map(|mut guard| {
                guard.set_token(identity, token);
            })
            .map_err(|_| Error::Auth("Failed to acquire write lock".into()))
    }

    fn switch_identity(&self, identity: Identity) -> Result<()> {
        get_global_identity_manager()
            .write()
            .map_err(|_| Error::Auth("Failed to acquire write lock".into()))?
            .switch_identity(identity)
    }
}

/// 本地认证提供者
#[derive(Debug, Clone)]
pub struct LocalAuthProvider {
    inner: Arc<RwLock<LocalIdentityManager>>,
}

impl LocalAuthProvider {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(LocalIdentityManager::new())),
        }
    }
}

impl AuthProvider for LocalAuthProvider {
    fn current_identity(&self) -> Identity {
        self.inner
            .read()
            .map(|guard| guard.current_identity())
            .unwrap_or(Identity::Blank)
    }

    fn current_token(&self) -> Option<String> {
        self.inner
            .read()
            .ok()
            .and_then(|guard| guard.current_token().map(String::from))
    }

    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.inner.read().ok().and_then(|guard| guard.auth_header())
    }

    fn set_token(&self, identity: Identity, token: String) -> Result<()> {
        self.inner
            .write()
            .map(|mut guard| {
                guard.set_token(identity, token);
            })
            .map_err(|_| Error::Auth("Failed to acquire write lock".into()))
    }

    fn switch_identity(&self, identity: Identity) -> Result<()> {
        self.inner
            .write()
            .map_err(|_| Error::Auth("Failed to acquire write lock".into()))?
            .switch_identity(identity)
    }
}

// ==================== 内部客户端结构 ====================
#[derive(Clone)]
struct InnerClient {
    agent: Agent,
    config: ClientConfig,
    auth: Arc<dyn AuthProvider>,
}

impl InnerClient {
    fn new(config: ClientConfig, auth: Arc<dyn AuthProvider>) -> Self {
        let agent = Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .build()
            .into();
        Self {
            agent,
            config,
            auth,
        }
    }

    fn agent(&self) -> &Agent {
        &self.agent
    }

    fn build_url(&self, endpoint: &str, base_key: Option<&str>) -> String {
        if endpoint.starts_with("http") {
            endpoint.to_string()
        } else {
            let base = self.config.get_base_url(base_key);
            format!(
                "{}/{}",
                base.trim_end_matches('/'),
                endpoint.trim_start_matches('/')
            )
        }
    }

    fn prepare_request<T>(&self, builder: RequestBuilder<T>) -> RequestBuilder<T> {
        let mut builder = builder;
        for (k, v) in DEFAULT_HEADERS {
            builder = builder.header(*k, *v);
        }

        // 从认证提供者获取认证信息
        if let Some((k, v)) = self.auth.auth_header() {
            builder = builder.header(k, v);
        }
        builder
    }

    fn log_request(
        &self,
        method: HttpMethod,
        url: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
    ) {
        if !self.config.log_requests {
            return;
        }
        println!("\n========== 网络请求信息 ==========");
        println!("方法: {}", Into::<&str>::into(method));
        println!("URL: {}", url);

        println!("请求头:");
        for (k, v) in DEFAULT_HEADERS {
            println!("  {}: {}", k, v);
        }

        if self.auth.auth_header().is_some() {
            println!("  Authorization: Bearer [已隐藏]");
        }

        if let Some(params) = params {
            if !params.is_empty() {
                println!("查询参数:");
                for (k, v) in params {
                    println!("  {}: {}", k, v);
                }
            }
        }

        if let Some(payload) = payload {
            println!("请求体:");
            if let Ok(pretty) = serde_json::to_string_pretty(payload) {
                for line in pretty.lines() {
                    println!("  {}", line);
                }
            }
        }
    }

    fn log_response(&self, url: &str, response: &Response<Body>) -> Result<()> {
        if !self.config.log_requests {
            return Ok(());
        }
        println!("{}", url);
        println!("\n---------- 响应信息 ----------");
        println!(
            "状态: {} {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("")
        );

        println!("响应头:");
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                println!("  {}: {}", key, value_str);
            }
        }

        println!("================================\n");
        Ok(())
    }

    fn send_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
        base_key: Option<&str>,
    ) -> Result<Response<Body>> {
        let url = self.build_url(endpoint, base_key);

        self.log_request(method, &url, params, payload);

        let response = match method {
            HttpMethod::GET | HttpMethod::DELETE | HttpMethod::HEAD => {
                let mut req = self.prepare_request(match method {
                    HttpMethod::GET => self.agent.get(&url),
                    HttpMethod::DELETE => self.agent.delete(&url),
                    HttpMethod::HEAD => self.agent.head(&url),
                    _ => unreachable!(),
                });
                if let Some(params) = params {
                    for (k, v) in params {
                        req = req.query(k, v);
                    }
                }
                req.call()?
            }
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => {
                let mut req = self.prepare_request(match method {
                    HttpMethod::POST => self.agent.post(&url),
                    HttpMethod::PUT => self.agent.put(&url),
                    HttpMethod::PATCH => self.agent.patch(&url),
                    _ => unreachable!(),
                });
                if let Some(params) = params {
                    for (k, v) in params {
                        req = req.query(k, v);
                    }
                }
                if let Some(payload) = payload {
                    req.send_json(payload)?
                } else {
                    req.send_empty()?
                }
            }
        };

        self.log_response(&url, &response)?;
        Ok(response)
    }

    fn response_to_json(&self, response: Response<Body>) -> Result<Value> {
        let mut body = response.into_body();
        let bytes = body.read_to_vec()?;

        if self.config.log_requests && !bytes.is_empty() {
            if let Ok(json) = serde_json::from_slice::<Value>(&bytes) {
                println!("响应体 (JSON):");
                if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                    for line in pretty.lines() {
                        println!("  {}", line);
                    }
                }
                return Ok(json);
            } else if let Ok(text) = String::from_utf8(bytes.clone()) {
                println!("响应体 (文本):");
                println!("  {}", text);
            }
        }

        Ok(serde_json::from_slice(&bytes)?)
    }

    fn response_to_string(&self, response: Response<Body>) -> Result<String> {
        let mut body = response.into_body();
        let text = body.read_to_string()?;
        Ok(text)
    }

    fn response_to_binary(&self, response: Response<Body>) -> Result<Vec<u8>> {
        let mut body = response.into_body();
        let text = body.read_to_vec()?;
        Ok(text)
    }
}

// ==================== 公开的 CodeMaoClient ====================
/// 主客户端，支持全局单例和独立实例两种模式
#[derive(Clone)]
pub struct CodeMaoClient {
    inner: Arc<InnerClient>,
}

impl CodeMaoClient {
    /// 获取全局单例实例（使用全局身份管理器）
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<CodeMaoClient> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            CodeMaoClient::new_with_auth(
                ClientConfig::default(),
                Arc::new(GlobalAuthProvider::new()),
            )
        })
    }

    /// 初始化全局实例（如果尚未初始化）
    pub fn init_global(config: ClientConfig) -> &'static Self {
        static INSTANCE: OnceLock<CodeMaoClient> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            CodeMaoClient::new_with_auth(config, Arc::new(GlobalAuthProvider::new()))
        })
    }

    /// 创建使用全局身份管理器的客户端
    pub fn new_with_global_auth(config: ClientConfig) -> Self {
        Self::new_with_auth(config, Arc::new(GlobalAuthProvider::new()))
    }

    /// 创建使用独立身份管理器的客户端
    pub fn new_independent(config: ClientConfig) -> Self {
        Self::new_with_auth(config, Arc::new(LocalAuthProvider::new()))
    }

    /// 使用自定义认证提供者创建客户端
    pub fn new_with_auth(config: ClientConfig, auth: Arc<dyn AuthProvider>) -> Self {
        Self {
            inner: Arc::new(InnerClient::new(config, auth)),
        }
    }

    /// 创建新实例（向后兼容，使用全局身份管理器）
    pub fn new(config: ClientConfig) -> Self {
        if config.use_global_auth {
            Self::new_with_global_auth(config)
        } else {
            Self::new_independent(config)
        }
    }

    /// 获取底层 Agent
    pub fn agent(&self) -> &Agent {
        self.inner.agent()
    }

    /// 设置指定身份的令牌
    pub fn set_token(&self, identity: Identity, token: impl Into<String>) -> Result<()> {
        self.inner.auth.set_token(identity, token.into())
    }

    /// 切换到指定身份
    pub fn switch_identity(&self, identity: Identity) -> Result<()> {
        self.inner.auth.switch_identity(identity)
    }

    /// 获取当前身份
    pub fn current_identity(&self) -> Identity {
        self.inner.auth.current_identity()
    }

    /// 获取当前令牌
    pub fn current_token(&self) -> Option<String> {
        self.inner.auth.current_token()
    }

    /// 发送 HTTP 请求
    pub fn send_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
        base_key: Option<&str>,
    ) -> Result<Response<Body>> {
        self.inner
            .send_request(method, endpoint, params, payload, base_key)
    }

    /// 将响应体解析为 JSON
    pub fn response_to_json(&self, response: Response<Body>) -> Result<Value> {
        self.inner.response_to_json(response)
    }

    /// 将响应体读取为字符串
    pub fn response_to_string(&self, response: Response<Body>) -> Result<String> {
        self.inner.response_to_string(response)
    }

    /// 将响应体读取为字符串
    pub fn response_to_binary(&self, response: Response<Body>) -> Result<Vec<u8>> {
        self.inner.response_to_binary(response)
    }

    /// 创建分页迭代器
    pub fn paginated(&self, endpoint: impl Into<String>) -> PaginatedIter {
        PaginatedIter::new(self.clone(), endpoint)
    }

    /// 创建文件上传器
    pub fn file_uploader(&self) -> FileUploader {
        FileUploader::new(self.clone())
    }
}

// ==================== 分页配置 ====================
#[derive(Debug, Clone, Copy)]
pub enum PaginationMethod {
    Offset,
    Page,
}

#[derive(Debug, Clone, Default)]
pub struct PaginationConfig {
    pub amount_key: Option<String>,
    pub offset_key: Option<String>,
    pub response_amount_key: Option<String>,
    pub response_offset_key: Option<String>,
}

// ==================== 分页迭代器 ====================
pub struct PaginatedIter {
    client: CodeMaoClient,
    method: HttpMethod,
    endpoint: String,
    base_params: HashMap<String, String>,
    payload: Option<Value>,
    limit: Option<usize>,
    total_key: String,
    data_key: String,
    pagination_method: PaginationMethod,
    config: PaginationConfig,
    base_key: Option<String>,

    // 内部状态
    total_items: usize,
    items_per_page: usize,
    current_page: usize,
    current_page_data: Vec<Value>,
    current_index: usize,
    yielded_count: usize,
    finished: bool,
    initialized: bool,
}

impl PaginatedIter {
    const DEFAULT_PAGE_SIZE: usize = 15;

    pub fn new(client: CodeMaoClient, endpoint: impl Into<String>) -> Self {
        Self {
            client,
            method: HttpMethod::GET,
            endpoint: endpoint.into(),
            base_params: HashMap::new(),
            payload: None,
            limit: None,
            total_key: "total".to_string(),
            data_key: "items".to_string(),
            pagination_method: PaginationMethod::Offset,
            config: PaginationConfig::default(),
            base_key: None,
            total_items: 0,
            items_per_page: Self::DEFAULT_PAGE_SIZE,
            current_page: 0,
            current_page_data: Vec::new(),
            current_index: 0,
            yielded_count: 0,
            finished: false,
            initialized: false,
        }
    }

    // 链式构建方法
    pub fn with_method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.base_params.insert(key.into(), value.into());
        self
    }

    pub fn with_params(mut self, params: HashMap<String, String>) -> Self {
        self.base_params.extend(params);
        self
    }

    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn with_total_key(mut self, key: impl Into<String>) -> Self {
        self.total_key = key.into();
        self
    }

    pub fn with_data_key(mut self, key: impl Into<String>) -> Self {
        self.data_key = key.into();
        self
    }

    pub fn with_pagination_method(mut self, method: PaginationMethod) -> Self {
        self.pagination_method = method;
        self
    }

    pub fn with_config(mut self, config: PaginationConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_amount_key(mut self, key: impl Into<String>) -> Self {
        let mut config = self.config.clone();
        config.amount_key = Some(key.into());
        self.config = config;
        self
    }
    pub fn with_offset_key(mut self, key: impl Into<String>) -> Self {
        let mut config = self.config.clone();
        config.offset_key = Some(key.into());
        self.config = config;
        self
    }
    pub fn with_response_amount_key(mut self, key: impl Into<String>) -> Self {
        let mut config = self.config.clone();
        config.response_amount_key = Some(key.into());
        self.config = config;
        self
    }
    pub fn with_response_offset_key(mut self, key: impl Into<String>) -> Self {
        let mut config = self.config.clone();
        config.response_offset_key = Some(key.into());
        self.config = config;
        self
    }
    pub fn with_base_url(mut self, key: impl Into<String>) -> Self {
        self.base_key = Some(key.into());
        self
    }

    // 内部辅助方法
    fn merge_config(&self) -> PaginationConfig {
        self.config.clone()
    }

    /// 构建指定页的请求参数
    fn build_page_params(&self, page: usize, size: usize) -> HashMap<String, String> {
        let mut params = self.base_params.clone();
        let cfg = self.merge_config();

        if let Some(amount_key) = &cfg.amount_key {
            params.insert(amount_key.clone(), size.to_string());
        }
        if let Some(offset_key) = &cfg.offset_key {
            match self.pagination_method {
                PaginationMethod::Offset => {
                    let base_offset = self
                        .base_params
                        .get(offset_key)
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    params.insert(offset_key.clone(), (base_offset + page * size).to_string());
                }
                PaginationMethod::Page => {
                    params.insert(offset_key.clone(), (page + 1).to_string());
                }
            }
        }
        params
    }

    /// 获取指定页数据
    fn fetch_page(&self, page: usize) -> Result<Vec<Value>> {
        let params = self.build_page_params(page, self.items_per_page);
        let response = self.client.send_request(
            self.method,
            &self.endpoint,
            Some(&params),
            self.payload.as_ref(),
            self.base_key.as_deref(),
        )?;
        let json = self.client.response_to_json(response)?;

        let data = json
            .pointer(&format!("/{}", self.data_key.replace('.', "/")))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(data)
    }

    /// 初始化：获取总数和每页大小
    fn initialize(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        let first_page_params = self.build_page_params(0, Self::DEFAULT_PAGE_SIZE);
        let response = self.client.send_request(
            self.method,
            &self.endpoint,
            Some(&first_page_params),
            self.payload.as_ref(),
            self.base_key.as_deref(),
        )?;
        let json = self.client.response_to_json(response)?;

        // 提取总数
        self.total_items = Self::extract_total(&json, &self.total_key)?;

        // 尝试从响应中获取实际每页大小
        if let Some(response_amount_key) = &self.merge_config().response_amount_key {
            if let Some(amount) = Self::extract_nested_u64(&json, response_amount_key) {
                self.items_per_page = amount as usize;
            }
        } else if let Some(amount_key) = &self.merge_config().amount_key {
            if let Some(amount) = first_page_params
                .get(amount_key)
                .and_then(|v| v.parse().ok())
            {
                self.items_per_page = amount;
            }
        }

        // 提取第一页数据
        if let Some(items) = json
            .pointer(&format!("/{}", self.data_key.replace('.', "/")))
            .and_then(|v| v.as_array())
        {
            self.current_page_data = items.clone();
            self.current_page = 0;
        } else {
            return Err(Error::Pagination("No data array found in response".into()));
        }

        self.initialized = true;
        Ok(())
    }

    /// 从 JSON 中提取总数
    fn extract_total(json: &Value, total_key: &str) -> Result<usize> {
        let total = json
            .pointer(&format!("/{}", total_key.replace('.', "/")))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_i64().map(|i| i as u64))
                    .or_else(|| v.as_f64().map(|f| f as u64))
            })
            .ok_or_else(|| Error::Pagination(format!("Total key '{}' not found", total_key)))?;
        Ok(total as usize)
    }

    fn extract_nested_u64(json: &Value, path: &str) -> Option<u64> {
        json.pointer(&format!("/{}", path.replace('.', "/")))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_i64().map(|i| i as u64))
                    .or_else(|| v.as_f64().map(|f| f as u64))
            })
    }

    /// 检查是否达到限制
    fn reached_limit(&self) -> bool {
        self.limit
            .map(|lim| self.yielded_count >= lim)
            .unwrap_or(false)
    }

    /// 获取下一个项目
    pub fn next(&mut self) -> Option<Result<Value>> {
        if !self.initialized {
            if let Err(e) = self.initialize() {
                return Some(Err(e));
            }
        }
        if self.finished || self.reached_limit() {
            return None;
        }

        while self.current_index >= self.current_page_data.len() {
            let next_page = self.current_page + 1;
            if next_page * self.items_per_page >= self.total_items {
                self.finished = true;
                return None;
            }
            match self.fetch_page(next_page) {
                Ok(data) => {
                    self.current_page_data = data;
                    self.current_page = next_page;
                    self.current_index = 0;
                }
                Err(e) => return Some(Err(e)),
            }
        }

        let item = self.current_page_data[self.current_index].clone();
        self.current_index += 1;
        self.yielded_count += 1;
        Some(Ok(item))
    }

    /// 一次性收集所有数据
    pub fn collect(mut self) -> Result<Vec<Value>> {
        let mut items = Vec::new();
        while let Some(item) = self.next() {
            items.push(item?);
        }
        Ok(items)
    }
}

impl Iterator for PaginatedIter {
    type Item = Result<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next()
    }
}

// ==================== 文件上传器 ====================
pub struct FileUploader {
    client: CodeMaoClient,
}

impl FileUploader {
    pub fn new(client: CodeMaoClient) -> Self {
        Self { client }
    }

    /// 统一上传入口
    pub fn upload(&self, file_path: &Path, method: &str, save_path: &str) -> Result<String> {
        match method {
            "pgaot" => self.upload_pgaot(file_path, save_path),
            "codegame" => self.upload_codegame(file_path, save_path),
            "codemao" => self.upload_codemao(file_path, save_path),
            _ => Err(Error::Other(format!(
                "Unsupported upload method: {}",
                method
            ))),
        }
    }

    fn upload_pgaot(&self, file_path: &Path, save_path: &str) -> Result<String> {
        let form = Form::new()
            .text("path", save_path)
            .file("file", file_path)?;

        let response = self
            .client
            .agent()
            .post("https://api.pgaot.com/user/up_cat_file")
            .send(form)?;

        let json: Value = response.into_body().read_json()?;
        Ok(json["url"].as_str().unwrap_or("").to_string())
    }

    fn upload_codegame(&self, file_path: &Path, save_path: &str) -> Result<String> {
        let token_info = self.get_codegame_token(save_path, file_path)?;

        let form = Form::new()
            .text("token", &token_info.token)
            .text("key", &token_info.file_path)
            .text("fname", "avatar")
            .file("file", file_path)?;

        let response = self
            .client
            .agent()
            .post(&token_info.upload_url)
            .send(form)?;

        let json: Value = response.into_body().read_json()?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}/{}", token_info.pic_host, key))
    }

    fn upload_codemao(&self, file_path: &Path, save_path: &str) -> Result<String> {
        let unique_filename = format!(
            "{}{}",
            generate_id(4),
            file_path
                .extension()
                .map(|ext| format!(".{}", ext.to_string_lossy()))
                .unwrap_or_default()
        );
        let unique_name = format!("{}/{}", save_path, unique_filename);

        let token_info = self.get_codemao_token(&unique_name)?;

        let form = Form::new()
            .text("token", &token_info.token)
            .text("key", &token_info.file_path)
            .text("fname", &unique_filename)
            .file("file", file_path)?;

        let response = self
            .client
            .agent()
            .post(&token_info.upload_url)
            .send(form)?;

        let json: Value = response.into_body().read_json()?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}{}", token_info.bucket_url, key))
    }

    fn get_codemao_token(&self, file_path: &str) -> Result<CodeMaoTokenInfo> {
        let mut params = HashMap::new();
        params.insert("projectName".to_string(), "community_frontend".to_string());
        params.insert("filePaths".to_string(), file_path.to_string());
        params.insert("filePath".to_string(), file_path.to_string());
        params.insert("tokensCount".to_string(), "1".to_string());
        params.insert("fileSign".to_string(), "p1".to_string());
        params.insert("cdnName".to_string(), "qiniu".to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "https://open-service.codemao.cn/cdn/qi-niu/tokens/uploading",
            Some(&params),
            None,
            None,
        )?;

        let json = self.client.response_to_json(response)?;
        let tokens = json["tokens"]
            .as_array()
            .ok_or_else(|| Error::Other("No tokens array".into()))?;
        let token_info = tokens
            .get(0)
            .ok_or_else(|| Error::Other("No token".into()))?;

        Ok(CodeMaoTokenInfo {
            token: token_info["token"].as_str().unwrap_or("").to_string(),
            file_path: token_info["file_path"].as_str().unwrap_or("").to_string(),
            upload_url: json["upload_url"].as_str().unwrap_or("").to_string(),
            bucket_url: json["bucket_url"].as_str().unwrap_or("").to_string(),
        })
    }

    fn get_codegame_token(&self, prefix: &str, file_path: &Path) -> Result<CodeGameTokenInfo> {
        let extension = file_path
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();

        let mut params = HashMap::new();
        params.insert("prefix".to_string(), prefix.to_string());
        params.insert("bucket".to_string(), "static".to_string());
        params.insert("type".to_string(), extension);

        let response = self.client.send_request(
            HttpMethod::GET,
            "https://oversea-api.code.game/tiger/kitten/cdn/token/1",
            Some(&params),
            None,
            None,
        )?;

        let json = self.client.response_to_json(response)?;
        let data = json["data"]
            .as_array()
            .ok_or_else(|| Error::Other("No data array".into()))?;
        let token_data = data
            .get(0)
            .ok_or_else(|| Error::Other("No token data".into()))?;

        Ok(CodeGameTokenInfo {
            token: token_data["token"].as_str().unwrap_or("").to_string(),
            file_path: token_data["filename"].as_str().unwrap_or("").to_string(),
            pic_host: json["bucket_url"].as_str().unwrap_or("").to_string(),
            upload_url: "https://upload.qiniup.com".to_string(),
        })
    }
}

// ==================== 内部数据结构 ====================
struct CodeMaoTokenInfo {
    token: String,
    file_path: String,
    upload_url: String,
    bucket_url: String,
}

struct CodeGameTokenInfo {
    token: String,
    file_path: String,
    pic_host: String,
    upload_url: String,
}

// ==================== 辅助函数 ====================
fn generate_id(length: usize) -> String {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut rng = rand::rng();
    (0..length)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// 定义HTTP状态码枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HTTPStatus {
    // 1xx Informational
    Continue = 100,
    SwitchingProtocols = 101,
    Processing = 102,

    // 2xx Success
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NonAuthoritativeInfo = 203,
    NoContent = 204,
    ResetContent = 205,
    PartialContent = 206,

    // 3xx Redirection
    MultipleChoices = 300,
    MovedPermanently = 301,
    Found = 302,
    SeeOther = 303,
    NotModified = 304,
    TemporaryRedirect = 307,
    PermanentRedirect = 308,

    // 4xx Client Error
    BadRequest = 400,
    Unauthorized = 401,
    PaymentRequired = 402,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    NotAcceptable = 406,
    Conflict = 409,
    Gone = 410,

    // 5xx Server Error
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
}

impl HTTPStatus {
    // 从u16创建HTTPStatus
    pub fn from_code(code: u16) -> Option<Self> {
        match code {
            100 => Some(HTTPStatus::Continue),
            101 => Some(HTTPStatus::SwitchingProtocols),
            102 => Some(HTTPStatus::Processing),
            200 => Some(HTTPStatus::Ok),
            201 => Some(HTTPStatus::Created),
            202 => Some(HTTPStatus::Accepted),
            203 => Some(HTTPStatus::NonAuthoritativeInfo),
            204 => Some(HTTPStatus::NoContent),
            205 => Some(HTTPStatus::ResetContent),
            206 => Some(HTTPStatus::PartialContent),
            300 => Some(HTTPStatus::MultipleChoices),
            301 => Some(HTTPStatus::MovedPermanently),
            302 => Some(HTTPStatus::Found),
            303 => Some(HTTPStatus::SeeOther),
            304 => Some(HTTPStatus::NotModified),
            307 => Some(HTTPStatus::TemporaryRedirect),
            308 => Some(HTTPStatus::PermanentRedirect),
            400 => Some(HTTPStatus::BadRequest),
            401 => Some(HTTPStatus::Unauthorized),
            402 => Some(HTTPStatus::PaymentRequired),
            403 => Some(HTTPStatus::Forbidden),
            404 => Some(HTTPStatus::NotFound),
            405 => Some(HTTPStatus::MethodNotAllowed),
            406 => Some(HTTPStatus::NotAcceptable),
            409 => Some(HTTPStatus::Conflict),
            410 => Some(HTTPStatus::Gone),
            500 => Some(HTTPStatus::InternalServerError),
            501 => Some(HTTPStatus::NotImplemented),
            502 => Some(HTTPStatus::BadGateway),
            503 => Some(HTTPStatus::ServiceUnavailable),
            504 => Some(HTTPStatus::GatewayTimeout),
            _ => None,
        }
    }

    // 获取状态码对应的原因短语
    pub fn reason_phrase(&self) -> &'static str {
        match self {
            HTTPStatus::Continue => "Continue",
            HTTPStatus::SwitchingProtocols => "Switching Protocols",
            HTTPStatus::Processing => "Processing",
            HTTPStatus::Ok => "OK",
            HTTPStatus::Created => "Created",
            HTTPStatus::Accepted => "Accepted",
            HTTPStatus::NonAuthoritativeInfo => "Non-Authoritative Information",
            HTTPStatus::NoContent => "No Content",
            HTTPStatus::ResetContent => "Reset Content",
            HTTPStatus::PartialContent => "Partial Content",
            HTTPStatus::MultipleChoices => "Multiple Choices",
            HTTPStatus::MovedPermanently => "Moved Permanently",
            HTTPStatus::Found => "Found",
            HTTPStatus::SeeOther => "See Other",
            HTTPStatus::NotModified => "Not Modified",
            HTTPStatus::TemporaryRedirect => "Temporary Redirect",
            HTTPStatus::PermanentRedirect => "Permanent Redirect",
            HTTPStatus::BadRequest => "Bad Request",
            HTTPStatus::Unauthorized => "Unauthorized",
            HTTPStatus::PaymentRequired => "Payment Required",
            HTTPStatus::Forbidden => "Forbidden",
            HTTPStatus::NotFound => "Not Found",
            HTTPStatus::MethodNotAllowed => "Method Not Allowed",
            HTTPStatus::NotAcceptable => "Not Acceptable",
            HTTPStatus::Conflict => "Conflict",
            HTTPStatus::Gone => "Gone",
            HTTPStatus::InternalServerError => "Internal Server Error",
            HTTPStatus::NotImplemented => "Not Implemented",
            HTTPStatus::BadGateway => "Bad Gateway",
            HTTPStatus::ServiceUnavailable => "Service Unavailable",
            HTTPStatus::GatewayTimeout => "Gateway Timeout",
        }
    }
}

// 实现从HTTPStatus到u16的转换
impl From<HTTPStatus> for u16 {
    fn from(status: HTTPStatus) -> Self {
        status as u16
    }
}

// 实现Display trait
impl fmt::Display for HTTPStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", *self as u16, self.reason_phrase())
    }
}
// ==================== 简化的工厂 ====================
pub struct ClientFactory;

impl ClientFactory {
    /// 创建使用全局身份管理器的 HTTP 客户端
    pub fn create_global_client(config: Option<ClientConfig>) -> CodeMaoClient {
        CodeMaoClient::new_with_global_auth(config.unwrap_or_default())
    }

    /// 创建使用独立身份管理器的 HTTP 客户端
    pub fn create_independent_client(config: Option<ClientConfig>) -> CodeMaoClient {
        CodeMaoClient::new_independent(config.unwrap_or_default())
    }

    /// 创建文件上传器
    pub fn create_file_uploader(client: CodeMaoClient) -> FileUploader {
        FileUploader::new(client)
    }

    /// 初始化全局客户端实例
    pub fn init_global_client(config: Option<ClientConfig>) -> &'static CodeMaoClient {
        CodeMaoClient::init_global(config.unwrap_or_default())
    }

    /// 获取全局客户端实例
    pub fn global_client() -> &'static CodeMaoClient {
        CodeMaoClient::global()
    }

    /// 获取全局身份管理器
    pub fn global_identity_manager() -> Arc<RwLock<GlobalIdentityManager>> {
        get_global_identity_manager()
    }
}
