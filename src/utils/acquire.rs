use rand::Rng;
use reqwest::{Client, RequestBuilder, Response, header::HeaderMap, multipart};
use serde_json::Value;
use std::path::Path;
use std::str::FromStr;
use std::sync::{
    Arc, OnceLock, RwLock,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use thiserror::Error as ThisError;

// ==================== 错误定义（使用 thiserror） ====================
#[derive(ThisError, Debug)]
pub enum MewError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Pagination error: {0}")]
    Pagination(String),
    #[error("Other error: {0}")]
    Other(String),
}

pub type MewResult<T> = std::result::Result<T, MewError>;

// ==================== 基础 URL 键枚举 ====================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseKey {
    Default,
    Creation,
    Whale,
    Education,
}

impl BaseKey {
    /// 获取枚举对应的字符串键（用于内部使用）
    pub fn as_str(&self) -> &'static str {
        match self {
            BaseKey::Default => "default",
            BaseKey::Creation => "creation",
            BaseKey::Whale => "whale",
            BaseKey::Education => "education",
        }
    }

    /// 所有可用的基础键
    pub const ALL: [BaseKey; 4] = [
        BaseKey::Default,
        BaseKey::Creation,
        BaseKey::Whale,
        BaseKey::Education,
    ];

    /// 根据枚举值获取对应的基础 URL
    pub fn url(&self) -> &'static str {
        match self {
            BaseKey::Default => "https://api.codemao.cn",
            BaseKey::Creation => "https://api-creation.codemao.cn",
            BaseKey::Whale => "https://api-whale.codemao.cn",
            BaseKey::Education => "https://eduzone.codemao.cn",
        }
    }
}

impl FromStr for BaseKey {
    type Err = MewError;
    fn from_str(s: &str) -> MewResult<Self> {
        match s {
            "default" => Ok(BaseKey::Default),
            "creation" => Ok(BaseKey::Creation),
            "whale" => Ok(BaseKey::Whale),
            "education" => Ok(BaseKey::Education),
            _ => Err(MewError::Other(format!("invalid base key: {}", s))),
        }
    }
}

impl Default for BaseKey {
    fn default() -> Self {
        BaseKey::Default
    }
}

// ==================== 常量定义 ====================
/// 默认请求头静态切片（萌化命名）
const KITTY_HEADERS: &[(&str, &str)] = &[
    ("Accept", "application/json, text/plain, */*"),
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

// ==================== 身份枚举（萌化：Catsona） ====================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Catsona {
    Fluffy,  // 普通用户（原 Average）
    Scholar, // 教育（原 Edu）
    Judge,   // 评审（原 Judgement）
    Blanky,  // 空白（原 Blank）
}

impl Catsona {
    /// 转换为数组索引 (0..3)
    fn index(self) -> usize {
        match self {
            Catsona::Fluffy => 0,
            Catsona::Scholar => 1,
            Catsona::Judge => 2,
            Catsona::Blanky => 3,
        }
    }

    /// 所有身份变体
    pub const ALL: [Catsona; 4] = [
        Catsona::Fluffy,
        Catsona::Scholar,
        Catsona::Judge,
        Catsona::Blanky,
    ];
}

impl FromStr for Catsona {
    type Err = MewError;
    fn from_str(s: &str) -> MewResult<Self> {
        match s {
            "average" => Ok(Catsona::Fluffy),
            "edu" => Ok(Catsona::Scholar),
            "judgement" => Ok(Catsona::Judge),
            "blank" => Ok(Catsona::Blanky),
            _ => Err(MewError::Auth(format!("invalid identity: {}", s))),
        }
    }
}

impl AsRef<str> for Catsona {
    fn as_ref(&self) -> &str {
        match self {
            Catsona::Fluffy => "average",
            Catsona::Scholar => "edu",
            Catsona::Judge => "judgement",
            Catsona::Blanky => "blank",
        }
    }
}

// ==================== 身份管理器（核心单例 - 扁平化设计）====================
/// 全局身份管理器，使用固定长度数组存储令牌
/// 采用 RwLock + AtomicUsize 分离 token 存储和 current 索引
#[derive(Debug)]
pub struct KittyIdentityManager {
    token_bowl: RwLock<[Option<Arc<str>>; 4]>, // 令牌碗
    current_cat: AtomicUsize,                  // 当前猫猫
}

impl KittyIdentityManager {
    fn new() -> Self {
        Self {
            token_bowl: RwLock::new(Default::default()),
            current_cat: AtomicUsize::new(Catsona::Fluffy.index()),
        }
    }

    /// 设置指定身份的令牌
    fn set_token(&self, identity: Catsona, token: String) {
        let mut bowl = self.token_bowl.write().unwrap();
        if !token.is_empty() {
            bowl[identity.index()] = Some(Arc::from(token));
        }
    }

    /// 切换到指定身份
    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        if identity == Catsona::Blanky
            || self.token_bowl.read().unwrap()[identity.index()].is_some()
        {
            self.current_cat.store(identity.index(), Ordering::Relaxed);
            Ok(())
        } else {
            Err(MewError::Auth(format!(
                "No token for identity {:?}",
                identity
            )))
        }
    }

    /// 当前身份（无锁读取）
    fn current_identity(&self) -> Catsona {
        let idx = self.current_cat.load(Ordering::Relaxed);
        Catsona::ALL[idx]
    }

    /// 当前身份对应的令牌
    fn current_token(&self) -> Option<Arc<str>> {
        let idx = self.current_cat.load(Ordering::Relaxed);
        let bowl = self.token_bowl.read().unwrap();
        bowl[idx].clone()
    }

    /// 生成认证头
    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.current_token()
            .map(|token| ("Authorization", format!("Bearer {}", token)))
    }
}

/// 全局身份管理器单例
static GLOBAL_IDENTITY_MANAGER: OnceLock<KittyIdentityManager> = OnceLock::new();

fn get_global_identity_manager() -> &'static KittyIdentityManager {
    GLOBAL_IDENTITY_MANAGER.get_or_init(|| KittyIdentityManager::new())
}

// ==================== 客户端配置 ====================
#[derive(Debug, Clone)]
pub struct KittyConfig {
    default_base_key: BaseKey,
    timeout: Duration,
    log_requests: bool,
    /// 是否使用全局身份管理器
    use_global_auth: bool,
}

impl KittyConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取指定 key 的基础 URL
    pub fn get_base_url(&self, key: Option<BaseKey>) -> &'static str {
        key.unwrap_or(self.default_base_key).url()
    }

    pub fn with_default_base_key(mut self, key: BaseKey) -> Self {
        self.default_base_key = key;
        self
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

impl Default for KittyConfig {
    fn default() -> Self {
        Self {
            default_base_key: BaseKey::Default,
            timeout: Duration::from_secs(30),
            log_requests: false,
            use_global_auth: true,
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

// ==================== 认证特质 ====================
/// 认证提供者特质，允许不同的认证实现
pub trait KittyAuth: Send + Sync + std::fmt::Debug {
    fn current_identity(&self) -> Catsona;
    fn current_token(&self) -> Option<Arc<str>>;
    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.current_token()
            .map(|token| ("Authorization", format!("Bearer {}", token)))
    }
    fn set_token(&self, identity: Catsona, token: String) -> MewResult<()>;
    fn switch_identity(&self, identity: Catsona) -> MewResult<()>;
}

/// 全局认证提供者
#[derive(Debug, Clone)]
pub struct GlobalKittyAuth;

impl GlobalKittyAuth {
    pub fn new() -> Self {
        Self
    }
}

impl KittyAuth for GlobalKittyAuth {
    fn current_identity(&self) -> Catsona {
        get_global_identity_manager().current_identity()
    }

    fn current_token(&self) -> Option<Arc<str>> {
        get_global_identity_manager().current_token()
    }

    fn set_token(&self, identity: Catsona, token: String) -> MewResult<()> {
        get_global_identity_manager().set_token(identity, token);
        Ok(())
    }

    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        get_global_identity_manager().switch_identity(identity)
    }
}

/// 本地认证提供者（独立实例）
#[derive(Debug, Clone)]
pub struct LocalKittyAuth {
    inner: Arc<KittyIdentityManager>,
}

impl LocalKittyAuth {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(KittyIdentityManager::new()),
        }
    }
}

impl KittyAuth for LocalKittyAuth {
    fn current_identity(&self) -> Catsona {
        self.inner.current_identity()
    }

    fn current_token(&self) -> Option<Arc<str>> {
        self.inner.current_token()
    }

    fn set_token(&self, identity: Catsona, token: String) -> MewResult<()> {
        self.inner.set_token(identity, token);
        Ok(())
    }

    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        self.inner.switch_identity(identity)
    }
}

// ==================== 请求构建器 ====================
/// 请求构建器，支持链式设置可选参数（萌化名：KittyRequestBuilder）
pub struct KittyRequestBuilder {
    client: CodeMaoClient,
    method: HttpMethod,
    endpoint: String,
    base_key: Option<BaseKey>,
    params: Vec<(String, String)>,
    payload: Option<Value>,
    headers: Vec<(String, String)>,
}

impl KittyRequestBuilder {
    fn new(
        client: CodeMaoClient,
        method: HttpMethod,
        endpoint: impl Into<String>,
        base_key: Option<BaseKey>,
    ) -> Self {
        Self {
            client,
            method,
            endpoint: endpoint.into(),
            base_key,
            params: Vec::new(),
            payload: None,
            headers: Vec::new(),
        }
    }

    /// 设置查询参数，多次调用将合并参数（后添加的覆盖同名参数需要自行处理）
    pub fn with_params(mut self, params: &[(String, String)]) -> Self {
        self.params.extend_from_slice(params);
        self
    }

    /// 添加单个查询参数
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push((key.into(), value.into()));
        self
    }

    /// 设置请求体（JSON 负载），多次调用将替换之前的 payload
    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    /// 设置额外请求头，多次调用将合并头字段
    /// 这些头仅用于当前请求，不会持久化到后续请求
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// 添加单个临时请求头
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// 异步发送请求
    pub async fn send(self) -> MewResult<Response> {
        self.client
            .inner
            .send_request(
                self.method,
                &self.endpoint,
                self.base_key,
                &self.params,
                self.payload.as_ref(),
                &self.headers,
            )
            .await
    }
    pub async fn send_multipart(self, form: multipart::Form) -> MewResult<Response> {
        self.client
            .inner
            .send_multipart_request(
                self.method,
                &self.endpoint,
                self.base_key,
                &self.params,
                form,
                &self.headers,
            )
            .await
    }
}

// ==================== 内部客户端核心（萌化名：KittyCore） ====================
#[derive(Clone)]
struct KittyCore {
    client: Client,
    config: KittyConfig,
    auth: Arc<dyn KittyAuth>,
}

impl KittyCore {
    fn new(config: KittyConfig, auth: Arc<dyn KittyAuth>) -> Self {
        let mut default_headers = HeaderMap::new();
        for &(k, v) in KITTY_HEADERS {
            if let (Ok(key), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                default_headers.insert(key, val);
            }
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .default_headers(default_headers)
            .cookie_store(true)
            .build()
            .expect("Failed to build reqwest client");

        Self {
            client,
            config,
            auth,
        }
    }

    fn client(&self) -> &Client {
        &self.client
    }

    fn build_url(&self, endpoint: &str, base_key: Option<BaseKey>) -> String {
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

    fn log_request(
        &self,
        method: HttpMethod,
        url: &str,
        params: &[(String, String)],
        payload: Option<&Value>,
    ) {
        if !self.config.log_requests {
            return;
        }
        println!("\n========== 网络请求信息 ==========");
        println!("方法: {}", Into::<&str>::into(method));
        println!("URL: {}", url);

        println!("请求头:");
        for (k, v) in KITTY_HEADERS {
            println!("  {}: {}", k, v);
        }

        if self.auth.auth_header().is_some() {
            println!("  Authorization: Bearer [已隐藏]");
        }

        if !params.is_empty() {
            println!("查询参数:");
            for (k, v) in params {
                println!("  {}: {}", k, v);
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

    fn log_response(&self, url: &str, response: &Response) -> MewResult<()> {
        if !self.config.log_requests {
            return Ok(());
        }

        println!("\n========== 响应信息 ==========");
        println!("请求 URL: {}", url);
        println!(
            "状态: {} {}",
            response.status().as_u16(),
            response.status().canonical_reason().unwrap_or("")
        );

        println!("\n响应头:");
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                println!("  {}: {}", key, value_str);
            }
        }

        println!("================================\n");
        Ok(())
    }

    async fn send_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        base_key: Option<BaseKey>,
        params: &[(String, String)],
        payload: Option<&Value>,
        extra_headers: &[(String, String)],
    ) -> MewResult<Response> {
        let url = self.build_url(endpoint, base_key);
        self.log_request(method, &url, params, payload);

        // 构建基础请求
        let mut builder = match method {
            HttpMethod::GET => self.client.get(&url),
            HttpMethod::POST => self.client.post(&url),
            HttpMethod::DELETE => self.client.delete(&url),
            HttpMethod::PATCH => self.client.patch(&url),
            HttpMethod::PUT => self.client.put(&url),
            HttpMethod::HEAD => self.client.head(&url),
        };

        // 正确添加查询参数
        if !params.is_empty() {
            let query_pairs: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            builder = builder.query(&query_pairs);
        }

        // 链式添加认证头和临时请求头
        if let Some((k, v)) = self.auth.auth_header() {
            builder = builder.header(k, v);
        }
        for (k, v) in extra_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        // 根据方法发送 payload
        let response = match method {
            HttpMethod::POST | HttpMethod::PATCH | HttpMethod::PUT => {
                if let Some(p) = payload {
                    builder.json(p).send().await?
                } else {
                    builder.send().await?
                }
            }
            _ => builder.send().await?,
        };

        self.log_response(&url, &response)?;
        Ok(response)
    }

    async fn send_multipart_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        base_key: Option<BaseKey>,
        params: &[(String, String)],
        form: multipart::Form,
        extra_headers: &[(String, String)],
    ) -> MewResult<Response> {
        let url = self.build_url(endpoint, base_key);
        self.log_request(method, &url, params, None);

        let mut builder = match method {
            HttpMethod::POST => self.client.post(&url),
            HttpMethod::PUT => self.client.put(&url),
            HttpMethod::PATCH => self.client.patch(&url),
            _ => {
                return Err(MewError::Other(
                    "Multipart only supports POST/PUT/PATCH".into(),
                ));
            }
        };

        if !params.is_empty() {
            let query_pairs: Vec<(&str, &str)> = params
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            builder = builder.query(&query_pairs);
        }

        if let Some((k, v)) = self.auth.auth_header() {
            builder = builder.header(k, v);
        }
        for (k, v) in extra_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }

        let response = builder.multipart(form).send().await?;

        self.log_response(&url, &response)?;
        Ok(response)
    }
}

// ==================== 公开的 CodeMaoClient ====================
/// 主客户端，支持全局单例和独立实例两种模式
#[derive(Clone)]
pub struct CodeMaoClient {
    inner: Arc<KittyCore>,
}

impl CodeMaoClient {
    /// 获取全局单例实例（使用全局身份管理器）
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<CodeMaoClient> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            CodeMaoClient::new_with_auth(KittyConfig::default(), Arc::new(GlobalKittyAuth::new()))
        })
    }

    /// 初始化全局实例（如果尚未初始化）
    pub fn init_global(config: KittyConfig) -> &'static Self {
        static INSTANCE: OnceLock<CodeMaoClient> = OnceLock::new();
        INSTANCE
            .get_or_init(|| CodeMaoClient::new_with_auth(config, Arc::new(GlobalKittyAuth::new())))
    }

    /// 创建使用全局身份管理器的客户端
    pub fn new_with_global_auth(config: KittyConfig) -> Self {
        Self::new_with_auth(config, Arc::new(GlobalKittyAuth::new()))
    }

    /// 创建使用独立身份管理器的客户端
    pub fn new_independent(config: KittyConfig) -> Self {
        Self::new_with_auth(config, Arc::new(LocalKittyAuth::new()))
    }

    /// 使用自定义认证提供者创建客户端
    pub fn new_with_auth(config: KittyConfig, auth: Arc<dyn KittyAuth>) -> Self {
        Self {
            inner: Arc::new(KittyCore::new(config, auth)),
        }
    }

    /// 创建新实例（向后兼容，使用全局身份管理器）
    pub fn new(config: KittyConfig) -> Self {
        if config.use_global_auth {
            Self::new_with_global_auth(config)
        } else {
            Self::new_independent(config)
        }
    }

    /// 设置指定身份的令牌
    pub fn set_token(&self, identity: Catsona, token: impl Into<String>) -> MewResult<()> {
        self.inner.auth.set_token(identity, token.into())
    }

    /// 切换到指定身份
    pub fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        self.inner.auth.switch_identity(identity)
    }

    /// 获取当前身份
    pub fn current_identity(&self) -> Catsona {
        self.inner.auth.current_identity()
    }

    /// 获取当前令牌
    pub fn current_token(&self) -> Option<String> {
        self.inner.auth.current_token().map(|t| t.to_string())
    }

    /// 构建请求，返回支持链式设置临时请求头的 KittyRequestBuilder
    pub fn build_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        base_key: Option<BaseKey>,
    ) -> KittyRequestBuilder {
        KittyRequestBuilder::new(self.clone(), method, endpoint, base_key)
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

// ==================== 分页迭代器（使用 Arc<Vec> 共享参数） ====================
pub struct PaginatedIter {
    client: CodeMaoClient,
    method: HttpMethod,
    endpoint: String,
    base_params: Arc<Vec<(String, String)>>,
    payload: Option<Value>,
    limit: Option<usize>,
    total_key: String,
    data_key: String,
    pagination_method: PaginationMethod,
    config: PaginationConfig,
    base_key: Option<BaseKey>,

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
            base_params: Arc::new(Vec::new()),
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
        let mut params = (*self.base_params).clone();
        params.push((key.into(), value.into()));
        self.base_params = Arc::new(params);
        self
    }

    pub fn with_params(mut self, params: &[(String, String)]) -> Self {
        let mut existing = (*self.base_params).clone();
        existing.extend_from_slice(params);
        self.base_params = Arc::new(existing);
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
        self.config.amount_key = Some(key.into());
        self
    }

    pub fn with_offset_key(mut self, key: impl Into<String>) -> Self {
        self.config.offset_key = Some(key.into());
        self
    }

    pub fn with_response_amount_key(mut self, key: impl Into<String>) -> Self {
        self.config.response_amount_key = Some(key.into());
        self
    }

    pub fn with_response_offset_key(mut self, key: impl Into<String>) -> Self {
        self.config.response_offset_key = Some(key.into());
        self
    }

    pub fn with_base_key(mut self, key: BaseKey) -> Self {
        self.base_key = Some(key);
        self
    }

    async fn fetch_page(&self, page: usize) -> MewResult<Vec<Value>> {
        let mut builder = self
            .client
            .build_request(self.method, &self.endpoint, self.base_key);

        for (k, v) in self.base_params.iter() {
            builder = builder.with_param(k.clone(), v.clone());
        }

        if let Some(amount_key) = &self.config.amount_key {
            builder = builder.with_param(amount_key.clone(), self.items_per_page.to_string());
        }
        if let Some(offset_key) = &self.config.offset_key {
            let offset = match self.pagination_method {
                PaginationMethod::Offset => (page * self.items_per_page).to_string(),
                PaginationMethod::Page => (page + 1).to_string(),
            };
            builder = builder.with_param(offset_key.clone(), offset);
        }

        let response = builder.send().await?;
        let json = response.json::<Value>().await?;
        let data = json
            .pointer(&format!("/{}", self.data_key.replace('.', "/")))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(data)
    }

    async fn initialize(&mut self) -> MewResult<()> {
        if self.initialized {
            return Ok(());
        }

        let mut first_page_params: Vec<(String, String)> = (*self.base_params).clone();
        if let Some(amount_key) = &self.config.amount_key {
            first_page_params.push((amount_key.clone(), Self::DEFAULT_PAGE_SIZE.to_string()));
        }
        if let Some(offset_key) = &self.config.offset_key {
            let offset = match self.pagination_method {
                PaginationMethod::Offset => "0".to_string(),
                PaginationMethod::Page => "1".to_string(),
            };
            first_page_params.push((offset_key.clone(), offset));
        }

        let response = self
            .client
            .build_request(self.method, &self.endpoint, self.base_key)
            .with_params(&first_page_params)
            .send()
            .await?;
        let json = response.json().await?;

        self.total_items = Self::extract_total(&json, &self.total_key)?;

        if let Some(response_amount_key) = &self.config.response_amount_key {
            if let Some(amount) = Self::extract_nested_u64(&json, response_amount_key) {
                self.items_per_page = amount as usize;
            }
        }

        if let Some(items) = json
            .pointer(&format!("/{}", self.data_key.replace('.', "/")))
            .and_then(|v| v.as_array())
        {
            self.current_page_data = items.clone();
            self.current_page = 0;
        } else {
            return Err(MewError::Pagination(
                "No data array found in response".into(),
            ));
        }

        self.initialized = true;
        Ok(())
    }

    fn extract_total(json: &Value, total_key: &str) -> MewResult<usize> {
        let total = json
            .pointer(&format!("/{}", total_key.replace('.', "/")))
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_i64().map(|i| i as u64))
                    .or_else(|| v.as_f64().map(|f| f as u64))
            })
            .ok_or_else(|| MewError::Pagination(format!("Total key '{}' not found", total_key)))?;
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

    fn reached_limit(&self) -> bool {
        self.limit
            .map(|lim| self.yielded_count >= lim)
            .unwrap_or(false)
    }

    pub async fn next_item(&mut self) -> Option<MewResult<Value>> {
        if !self.initialized {
            if let Err(e) = self.initialize().await {
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
            match self.fetch_page(next_page).await {
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

    pub async fn collect(mut self) -> MewResult<Vec<Value>> {
        let mut items = Vec::new();
        while let Some(item) = self.next_item().await {
            items.push(item?);
        }
        Ok(items)
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

    pub async fn upload(
        &self,
        file_path: &Path,
        method: &str,
        save_path: &str,
    ) -> MewResult<String> {
        match method {
            "pgaot" => self.upload_pgaot(file_path, save_path).await,
            "codegame" => self.upload_codegame(file_path, save_path).await,
            "codemao" => self.upload_codemao(file_path, save_path).await,
            _ => Err(MewError::Other(format!(
                "Unsupported upload method: {}",
                method
            ))),
        }
    }

    async fn upload_pgaot(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let form = multipart::Form::new()
            .text("path", save_path.to_string())
            .file("file", file_path)
            .await?;

        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "https://api.pgaot.com/user/up_cat_file",
                None,
            )
            .send_multipart(form)
            .await?;

        let json: Value = response.json().await?;
        Ok(json["url"].as_str().unwrap_or("").to_string())
    }

    async fn upload_codegame(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let token_info = self.get_codegame_token(save_path, file_path).await?;

        let form = multipart::Form::new()
            .text("token", token_info.token)
            .text("key", token_info.file_path)
            .text("fname", "avatar".to_string())
            .file("file", file_path)
            .await?;

        let response = self
            .client
            .build_request(HttpMethod::POST, &token_info.upload_url, None)
            .send_multipart(form)
            .await?;

        let json: Value = response.json().await?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}/{}", token_info.pic_host, key))
    }

    async fn upload_codemao(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let unique_filename = format!(
            "{}{}",
            generate_meow_id(4),
            file_path
                .extension()
                .map(|ext| format!(".{}", ext.to_string_lossy()))
                .unwrap_or_default()
        );
        let unique_name = format!("{}/{}", save_path, unique_filename);

        let token_info = self.get_codemao_token(&unique_name).await?;

        let form = multipart::Form::new()
            .text("token", token_info.token)
            .text("key", token_info.file_path)
            .text("fname", unique_filename)
            .file("file", file_path)
            .await?;

        let response = self
            .client
            .build_request(HttpMethod::POST, &token_info.upload_url, None)
            .send_multipart(form)
            .await?;

        let json: Value = response.json().await?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}{}", token_info.bucket_url, key))
    }

    async fn get_codemao_token(&self, file_path: &str) -> MewResult<CodeMaoTokenInfo> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://open-service.codemao.cn/cdn/qi-niu/tokens/uploading",
                Some(BaseKey::Default),
            )
            .with_param("projectName", "community_frontend")
            .with_param("filePaths", file_path)
            .with_param("filePath", file_path)
            .with_param("tokensCount", "1")
            .with_param("fileSign", "p1")
            .with_param("cdnName", "qiniu")
            .send()
            .await?;

        let json: Value = response.json().await?;
        let tokens = json["tokens"]
            .as_array()
            .ok_or_else(|| MewError::Other("No tokens array".into()))?;
        let token_info = tokens
            .get(0)
            .ok_or_else(|| MewError::Other("No token".into()))?;

        Ok(CodeMaoTokenInfo {
            token: token_info["token"].as_str().unwrap_or("").to_string(),
            file_path: token_info["file_path"].as_str().unwrap_or("").to_string(),
            upload_url: json["upload_url"].as_str().unwrap_or("").to_string(),
            bucket_url: json["bucket_url"].as_str().unwrap_or("").to_string(),
        })
    }

    async fn get_codegame_token(
        &self,
        prefix: &str,
        file_path: &Path,
    ) -> MewResult<CodeGameTokenInfo> {
        let extension = file_path
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://oversea-api.code.game/tiger/kitten/cdn/token/1",
                Some(BaseKey::Default),
            )
            .with_param("prefix", prefix)
            .with_param("bucket", "static")
            .with_param("type", extension)
            .send()
            .await?;

        let json: Value = response.json().await?;
        let data = json["data"]
            .as_array()
            .ok_or_else(|| MewError::Other("No data array".into()))?;
        let token_data = data
            .get(0)
            .ok_or_else(|| MewError::Other("No token data".into()))?;

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
fn generate_meow_id(length: usize) -> String {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut rng = rand::rng();
    let mut bytes = vec![0u8; length];
    rng.fill_bytes(&mut bytes);
    for b in &mut bytes {
        *b = CHARSET[(*b as usize) % CHARSET.len()];
    }
    String::from_utf8(bytes).unwrap_or_default()
}

// ==================== HTTP 状态码枚举 ====================
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HTTPStatus {
    Continue = 100,
    SwitchingProtocols = 101,
    Processing = 102,
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NonAuthoritativeInfo = 203,
    NoContent = 204,
    ResetContent = 205,
    PartialContent = 206,
    MultipleChoices = 300,
    MovedPermanently = 301,
    Found = 302,
    SeeOther = 303,
    NotModified = 304,
    TemporaryRedirect = 307,
    PermanentRedirect = 308,
    BadRequest = 400,
    Unauthorized = 401,
    PaymentRequired = 402,
    Forbidden = 403,
    NotFound = 404,
    MethodNotAllowed = 405,
    NotAcceptable = 406,
    Conflict = 409,
    Gone = 410,
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
    GatewayTimeout = 504,
}

impl HTTPStatus {
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

impl From<HTTPStatus> for u16 {
    fn from(status: HTTPStatus) -> Self {
        status as u16
    }
}

impl std::fmt::Display for HTTPStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", *self as u16, self.reason_phrase())
    }
}

// ==================== 简化的工厂（萌化名：KittyFactory） ====================
pub struct KittyFactory;

impl KittyFactory {
    /// 创建使用全局身份管理器的 HTTP 客户端
    pub fn create_global_client(config: Option<KittyConfig>) -> CodeMaoClient {
        CodeMaoClient::new_with_global_auth(config.unwrap_or_default())
    }

    /// 创建使用独立身份管理器的 HTTP 客户端
    pub fn create_independent_client(config: Option<KittyConfig>) -> CodeMaoClient {
        CodeMaoClient::new_independent(config.unwrap_or_default())
    }

    /// 创建文件上传器
    pub fn create_file_uploader(client: CodeMaoClient) -> FileUploader {
        FileUploader::new(client)
    }

    /// 初始化全局客户端实例
    pub fn init_global_client(config: Option<KittyConfig>) -> &'static CodeMaoClient {
        CodeMaoClient::init_global(config.unwrap_or_default())
    }

    /// 获取全局客户端实例
    pub fn global_client() -> &'static CodeMaoClient {
        CodeMaoClient::global()
    }

    /// 获取全局身份管理器
    pub fn global_identity_manager() -> &'static KittyIdentityManager {
        get_global_identity_manager()
    }
}
