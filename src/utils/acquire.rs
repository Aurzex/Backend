use serde_json::Value;
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::sync::{
    Arc, OnceLock, RwLock,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use thiserror::Error as ThisError;
use ureq::http::Response;
use ureq::unversioned::multipart::Form;
use ureq::{Agent, Body, RequestBuilder};

// ==================== 错误定义（使用 thiserror） ====================
#[derive(ThisError, Debug)]
pub enum MewError {
    #[error("HTTP error: {0}")]
    Http(#[from] ureq::Error),
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
    /// 携带状态码的其他错误
    #[error("Other error: {0} (status: {1})")]
    OtherWithCode(String, u16),
}

pub type MewResult<T> = std::result::Result<T, MewError>;

// ==================== 基础 URL 键枚举 ====================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BaseKey {
    #[default]
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
    Blanky,  // 空白（原 Blank）- 此身份不应持有令牌
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
    /// 注意：Blanky 身份不应持有令牌，尝试设置将返回错误
    fn set_token(&self, identity: Catsona, token: String) -> MewResult<()> {
        // Blanky 身份不允许设置令牌
        if identity == Catsona::Blanky {
            return Err(MewError::Auth("Blanky identity cannot hold a token".into()));
        }
        let mut bowl = self.token_bowl.write().unwrap();
        if !token.is_empty() {
            bowl[identity.index()] = Some(Arc::from(token));
        }
        Ok(())
    }

    /// 切换到指定身份
    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        // Blanky 可以切换（不需要令牌），其他身份需要有令牌
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
/// 使用 OnceLock 确保线程安全的懒加载初始化
static GLOBAL_IDENTITY_MANAGER: OnceLock<KittyIdentityManager> = OnceLock::new();

fn get_global_identity_manager() -> &'static KittyIdentityManager {
    GLOBAL_IDENTITY_MANAGER.get_or_init(KittyIdentityManager::new)
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

impl Default for KittyConfig {
    fn default() -> Self {
        Self {
            default_base_key: BaseKey::Default,
            timeout: Duration::from_secs(30),
            log_requests: true,
            use_global_auth: true,
        }
    }
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

// ==================== HTTP 方法 ====================
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Delete,
    Patch,
    Put,
    Head,
}

impl From<HttpMethod> for &'static str {
    fn from(m: HttpMethod) -> Self {
        match m {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Put => "PUT",
            HttpMethod::Head => "HEAD",
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
        // 委托给全局管理器，会检查 Blanky 限制
        get_global_identity_manager().set_token(identity, token)
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
        // 委托给本地管理器，会检查 Blanky 限制
        self.inner.set_token(identity, token)
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

    /// 设置查询参数，多次调用将合并参数
    pub fn with_params(mut self, params: Vec<(String, String)>) -> Self {
        self.params.extend(params);
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
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// 添加单个临时请求头
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// 发送普通请求，返回响应
    pub fn send(self) -> MewResult<Response<Body>> {
        self.client.inner.send_request(
            self.method,
            &self.endpoint,
            self.base_key,
            &self.params,
            self.payload.as_ref(),
            &self.headers,
        )
    }

    /// 发送 multipart/form-data 请求
    pub fn send_multipart(self, form: Form) -> MewResult<Response<Body>> {
        self.client.inner.send_multipart_request(
            self.method,
            &self.endpoint,
            self.base_key,
            &self.params,
            form,
            &self.headers,
        )
    }
}

// ==================== 内部客户端核心（萌化名：KittyCore） ====================
#[derive(Clone)]
struct KittyCore {
    agent: Agent,
    config: KittyConfig,
    auth: Arc<dyn KittyAuth>,
}

impl KittyCore {
    fn new(config: KittyConfig, auth: Arc<dyn KittyAuth>) -> Self {
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

    fn log_response(&self, url: &str, response: &Response<Body>) -> MewResult<()> {
        if !self.config.log_requests {
            return Ok(());
        }

        println!("\n========== 响应信息 ==========");
        println!("请求 URL: {}", url);
        println!(
            "状态: {} {}",
            response.status(),
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

    /// 为 ureq RequestBuilder 统一设置默认头、认证头、额外头及查询参数
    fn apply_to_request_builder<B>(
        mut builder: RequestBuilder<B>,
        auth: &dyn KittyAuth,
        params: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> RequestBuilder<B> {
        for (k, v) in KITTY_HEADERS {
            builder = builder.header(*k, *v);
        }
        if let Some((k, v)) = auth.auth_header() {
            builder = builder.header(k, &v);
        }
        for (k, v) in extra_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        for (k, v) in params {
            builder = builder.query(k.as_str(), v.as_str());
        }
        builder
    }

    fn send_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        base_key: Option<BaseKey>,
        params: &[(String, String)],
        payload: Option<&Value>,
        extra_headers: &[(String, String)],
    ) -> MewResult<Response<Body>> {
        let url = self.build_url(endpoint, base_key);
        self.log_request(method, &url, params, payload);

        let response = match method {
            HttpMethod::Get => {
                let builder = self.agent.get(&url);
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    params,
                    extra_headers,
                );
                builder.call()?
            }
            HttpMethod::Post => {
                let builder = self.agent.post(&url);
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    params,
                    extra_headers,
                );
                if let Some(payload) = payload {
                    builder.send_json(payload)?
                } else {
                    builder.send_empty()?
                }
            }
            HttpMethod::Delete => {
                let builder = self.agent.delete(&url);
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    params,
                    extra_headers,
                );
                builder.call()?
            }
            HttpMethod::Patch => {
                let builder = self.agent.patch(&url);
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    params,
                    extra_headers,
                );
                if let Some(payload) = payload {
                    builder.send_json(payload)?
                } else {
                    builder.send_empty()?
                }
            }
            HttpMethod::Put => {
                let builder = self.agent.put(&url);
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    params,
                    extra_headers,
                );
                if let Some(payload) = payload {
                    builder.send_json(payload)?
                } else {
                    builder.send_empty()?
                }
            }
            HttpMethod::Head => {
                let builder = self.agent.head(&url);
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    params,
                    extra_headers,
                );
                builder.call()?
            }
        };

        self.log_response(&url, &response)?;
        Ok(response)
    }

    fn send_multipart_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        base_key: Option<BaseKey>,
        params: &[(String, String)],
        form: Form,
        extra_headers: &[(String, String)],
    ) -> MewResult<Response<Body>> {
        let url = self.build_url(endpoint, base_key);
        self.log_request(method, &url, params, None); // multipart 无 JSON 负载

        let builder = match method {
            HttpMethod::Post => self.agent.post(&url),
            HttpMethod::Put => self.agent.put(&url),
            HttpMethod::Patch => self.agent.patch(&url),
            _ => {
                return Err(MewError::Other(
                    "Multipart only supports POST/PUT/PATCH".into(),
                ));
            }
        };

        let builder =
            Self::apply_to_request_builder(builder, self.auth.as_ref(), params, extra_headers);

        let response = builder.send(form)?;
        self.log_response(&url, &response)?;
        Ok(response)
    }

    /// 将响应体解析为 JSON
    fn response_to_json(&self, response: Response<Body>) -> MewResult<Value> {
        let mut body = response.into_body();
        let bytes = body.read_to_vec()?;
        if bytes.is_empty() {
            if self.config.log_requests {
                println!("响应体: (空)");
            }
            return Ok(Value::Null);
        }

        let json: Value = serde_json::from_slice(&bytes)?;
        if self.config.log_requests {
            println!("\n---------- 响应体 (JSON) ----------");
            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                for line in pretty.lines() {
                    println!("  {}", line);
                }
            }
            println!("------------------------------------\n");
        }
        Ok(json)
    }

    /// 将响应体读取为字符串
    fn response_to_string(&self, response: Response<Body>) -> MewResult<String> {
        let mut body = response.into_body();
        let text = body.read_to_string()?;
        if self.config.log_requests {
            println!("\n---------- 响应体 (文本) ----------");
            if text.len() > 1000 {
                let preview: String = text.chars().take(1000).collect();
                println!("  {}", preview);
                println!("  ... (剩余 {} 字符被截断)", text.len() - 1000);
            } else {
                println!("  {}", text);
            }
            println!("-------------------------------------\n");
        }
        Ok(text)
    }

    /// 将响应体读取为二进制数据
    fn response_to_binary(&self, response: Response<Body>) -> MewResult<Vec<u8>> {
        let mut body = response.into_body();
        let data = body.read_to_vec()?;
        if self.config.log_requests {
            println!("\n---------- 响应体 (二进制) ----------");
            println!("  大小: {} 字节", data.len());
            if let Ok(text) = std::str::from_utf8(&data) {
                println!("  内容预览:");
                let preview: String = text.chars().take(200).collect();
                println!("  {}", preview);
                if text.len() > 200 {
                    println!("  ... (截断)");
                }
            } else {
                let preview_len = std::cmp::min(data.len(), 64);
                print!("  十六进制预览: ");
                for byte in &data[..preview_len] {
                    print!("{:02x} ", byte);
                }
                println!();
                if data.len() > 64 {
                    println!("  ... (截断)");
                }
            }
            println!("--------------------------------------\n");
        }
        Ok(data)
    }
}

// ==================== 公开的 CodeMaoClient ====================
/// 主客户端，支持全局单例和独立实例两种模式。
///
/// # 全局单例
/// 通过 `CodeMaoClient::global()` 获取全局共享实例，该实例使用默认配置。
/// 如需自定义配置（如超时、日志、独立身份管理），请使用 `new` / `new_with_global_auth` / `new_independent` 创建独立实例。
#[derive(Clone)]
pub struct CodeMaoClient {
    inner: Arc<KittyCore>,
}

impl CodeMaoClient {
    /// 获取全局单例实例（使用全局身份管理器，默认配置）
    /// 首次调用时自动初始化，后续调用返回同一实例
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<CodeMaoClient> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            CodeMaoClient::new_with_auth(KittyConfig::default(), Arc::new(GlobalKittyAuth::new()))
        })
    }

    /// 创建使用全局身份管理器的客户端（可自定义配置）
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

    /// 创建新实例（向后兼容，根据配置决定使用全局或独立身份管理器）
    pub fn new(config: KittyConfig) -> Self {
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
    /// 注意：Blanky 身份不允许设置令牌
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

    /// 构建请求，返回支持链式设置的 KittyRequestBuilder
    pub fn build_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        base_key: Option<BaseKey>,
    ) -> KittyRequestBuilder {
        KittyRequestBuilder::new(self.clone(), method, endpoint, base_key)
    }

    /// 将响应体解析为 JSON
    pub fn response_to_json(&self, response: Response<Body>) -> MewResult<Value> {
        self.inner.response_to_json(response)
    }

    /// 将响应体读取为字符串
    pub fn response_to_string(&self, response: Response<Body>) -> MewResult<String> {
        self.inner.response_to_string(response)
    }

    /// 将响应体读取为二进制数据
    pub fn response_to_binary(&self, response: Response<Body>) -> MewResult<Vec<u8>> {
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
    base_params: Vec<(String, String)>, // 不再使用 Arc，直接使用 Vec
    payload: Option<Value>,
    limit: Option<usize>,
    total_pointer: String, // 缓存的总数字段 pointer 路径
    data_pointer: String,  // 缓存的数据数组 pointer 路径
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
        let endpoint = endpoint.into();
        let total_key = "total".to_string();
        let data_key = "items".to_string();
        Self {
            client,
            method: HttpMethod::Get,
            endpoint,
            base_params: Vec::new(),
            payload: None,
            limit: None,
            total_pointer: Self::key_to_pointer(&total_key),
            data_pointer: Self::key_to_pointer(&data_key),
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

    // 将点分隔的 key 转换为 JSON Pointer 路径
    fn key_to_pointer(key: &str) -> String {
        format!("/{}", key.replace('.', "/"))
    }

    // 链式构建方法 ---------------------------------------------
    pub fn with_method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.base_params.push((key.into(), value.into()));
        self
    }

    pub fn with_params(mut self, params: Vec<(String, String)>) -> Self {
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
        self.total_pointer = Self::key_to_pointer(&key.into());
        self
    }

    pub fn with_data_key(mut self, key: impl Into<String>) -> Self {
        self.data_pointer = Self::key_to_pointer(&key.into());
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

    // 核心方法 -------------------------------------------------
    /// 获取指定页数据
    fn fetch_page(&self, page: usize) -> MewResult<Vec<Value>> {
        let mut params = self.base_params.clone();
        if let Some(ref key) = self.config.amount_key {
            params.push((key.clone(), self.items_per_page.to_string()));
        }
        if let Some(ref key) = self.config.offset_key {
            let offset = match self.pagination_method {
                PaginationMethod::Offset => (page * self.items_per_page).to_string(),
                PaginationMethod::Page => (page + 1).to_string(),
            };
            params.push((key.clone(), offset));
        }

        let mut builder = self
            .client
            .build_request(self.method, &self.endpoint, self.base_key);
        builder = builder.with_params(params);
        if let Some(ref payload) = self.payload {
            // 这里克隆一份 payload 以支持多次请求
            builder = builder.with_payload(payload.clone());
        }
        let response = builder.send()?;
        let json = self.client.response_to_json(response)?;

        let data = json
            .pointer(&self.data_pointer)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(data)
    }

    /// 初始化：发送第一页请求，获取总数、每页大小和首页数据
    fn initialize(&mut self) -> MewResult<()> {
        if self.initialized {
            return Ok(());
        }

        let mut first_page_params = self.base_params.clone();
        if let Some(ref key) = self.config.amount_key {
            first_page_params.push((key.clone(), Self::DEFAULT_PAGE_SIZE.to_string()));
        }
        if let Some(ref key) = self.config.offset_key {
            let offset = match self.pagination_method {
                PaginationMethod::Offset => "0".to_string(),
                PaginationMethod::Page => "1".to_string(),
            };
            first_page_params.push((key.clone(), offset));
        }

        let mut builder = self
            .client
            .build_request(self.method, &self.endpoint, self.base_key);
        builder = builder.with_params(first_page_params);
        if let Some(ref payload) = self.payload {
            builder = builder.with_payload(payload.clone());
        }
        let response = builder.send()?;
        let json = self.client.response_to_json(response)?;

        // 提取总条数
        self.total_items = Self::extract_total(&json, &self.total_pointer)?;

        // 提取实际每页大小（如果响应中有该字段）
        if let Some(ref key) = self.config.response_amount_key {
            let pointer = Self::key_to_pointer(key);
            if let Some(amount) = Self::extract_nested_u64(&json, &pointer) {
                self.items_per_page = amount as usize;
            }
        }

        // 缓存第一页数据
        if let Some(items) = json.pointer(&self.data_pointer).and_then(|v| v.as_array()) {
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

    /// 从 JSON 中提取总条数
    fn extract_total(json: &Value, total_pointer: &str) -> MewResult<usize> {
        let total = json
            .pointer(total_pointer)
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_i64().map(|i| i as u64))
                    .or_else(|| v.as_f64().map(|f| f as u64))
            })
            .ok_or_else(|| {
                MewError::Pagination(format!("Total key '{}' not found", total_pointer))
            })?;
        Ok(total as usize)
    }

    /// 从 JSON 指定路径提取 u64 值
    fn extract_nested_u64(json: &Value, pointer: &str) -> Option<u64> {
        json.pointer(pointer).and_then(|v| {
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

    /// 获取下一个项目（同步迭代）
    pub fn next_item(&mut self) -> Option<MewResult<Value>> {
        // 初始化
        if !self.initialized
            && let Err(e) = self.initialize()
        {
            return Some(Err(e));
        }

        // 检查终止条件
        if self.finished || self.reached_limit() {
            return None;
        }

        // 当前页还有数据，直接返回（快速路径）
        if self.current_index < self.current_page_data.len() {
            let item = self.current_page_data[self.current_index].clone();
            self.current_index += 1;
            self.yielded_count += 1;
            return Some(Ok(item));
        }

        // 请求下一页
        let next_page = self.current_page + 1;

        // 如果 total 可靠且已超出范围，提前终止
        if self.total_items > 0 && next_page * self.items_per_page >= self.total_items {
            self.finished = true;
            return None;
        }

        match self.fetch_page(next_page) {
            Ok(data) => {
                if data.is_empty() {
                    self.finished = true; // 空数据视为结束
                    return None;
                }
                self.current_page_data = data;
                self.current_page = next_page;
                self.current_index = 0;
                // 从新页取出第一条
                let item = self.current_page_data[self.current_index].clone();
                self.current_index += 1;
                self.yielded_count += 1;
                Some(Ok(item))
            }
            Err(e) => {
                self.finished = true; // 发生错误也终止迭代
                Some(Err(e))
            }
        }
    }

    /// 一次性收集所有数据
    pub fn collect(mut self) -> MewResult<Vec<Value>> {
        let mut items = Vec::new();
        while let Some(item) = self.next_item() {
            items.push(item?);
        }
        Ok(items)
    }
}

// 实现标准库 Iterator trait，使其可用于 for 循环等
impl Iterator for PaginatedIter {
    type Item = MewResult<Value>;
    fn next(&mut self) -> Option<Self::Item> {
        self.next_item()
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
    pub fn upload(&self, file_path: &Path, method: &str, save_path: &str) -> MewResult<String> {
        match method {
            "pgaot" => self.upload_pgaot(file_path, save_path),
            "codegame" => self.upload_codegame(file_path, save_path),
            "codemao" => self.upload_codemao(file_path, save_path),
            _ => Err(MewError::Other(format!(
                "Unsupported upload method: {}",
                method
            ))),
        }
    }

    fn upload_pgaot(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let form = Form::new()
            .text("path", save_path)
            .file("file", file_path)?;

        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://api.pgaot.com/user/up_cat_file",
                None,
            )
            .send_multipart(form)?;

        let json = self.client.response_to_json(response)?;
        Ok(json["url"].as_str().unwrap_or("").to_string())
    }

    fn upload_codegame(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let token_info = self.get_codegame_token(save_path, file_path)?;

        let form = Form::new()
            .text("token", &token_info.token)
            .text("key", &token_info.file_path)
            .text("fname", "avatar")
            .file("file", file_path)?;

        let response = self
            .client
            .build_request(HttpMethod::Post, &token_info.upload_url, None)
            .send_multipart(form)?;

        let json = self.client.response_to_json(response)?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}/{}", token_info.pic_host, key))
    }

    fn upload_codemao(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let unique_filename = format!(
            "{}{}",
            generate_meow_id(4),
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
            .build_request(HttpMethod::Post, &token_info.upload_url, None)
            .send_multipart(form)?;

        let json = self.client.response_to_json(response)?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}{}", token_info.bucket_url, key))
    }

    fn get_codemao_token(&self, file_path: &str) -> MewResult<CodeMaoTokenInfo> {
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://open-service.codemao.cn/cdn/qi-niu/tokens/uploading",
                Some(BaseKey::Default),
            )
            .with_param("projectName", "community_frontend")
            .with_param("filePaths", file_path)
            .with_param("filePath", file_path)
            .with_param("tokensCount", "1")
            .with_param("fileSign", "p1")
            .with_param("cdnName", "qiniu")
            .send()?;

        let json = self.client.response_to_json(response)?;
        let tokens = json["tokens"]
            .as_array()
            .ok_or_else(|| MewError::Other("No tokens array".into()))?;
        let token_info = tokens
            .first()
            .ok_or_else(|| MewError::Other("No token".into()))?;

        Ok(CodeMaoTokenInfo {
            token: token_info["token"].as_str().unwrap_or("").to_string(),
            file_path: token_info["file_path"].as_str().unwrap_or("").to_string(),
            upload_url: json["upload_url"].as_str().unwrap_or("").to_string(),
            bucket_url: json["bucket_url"].as_str().unwrap_or("").to_string(),
        })
    }

    fn get_codegame_token(&self, prefix: &str, file_path: &Path) -> MewResult<CodeGameTokenInfo> {
        let extension = file_path
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();

        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://oversea-api.code.game/tiger/kitten/cdn/token/1",
                Some(BaseKey::Default),
            )
            .with_param("prefix", prefix)
            .with_param("bucket", "static")
            .with_param("type", extension)
            .send()?;

        let json = self.client.response_to_json(response)?;
        let data = json["data"]
            .as_array()
            .ok_or_else(|| MewError::Other("No data array".into()))?;
        let token_data = data
            .first()
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

    (0..length)
        .map(|_| CHARSET[fastrand::usize(0..CHARSET.len())] as char)
        .collect()
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

impl fmt::Display for HTTPStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

    /// 获取全局客户端实例（使用默认配置）
    pub fn global_client() -> &'static CodeMaoClient {
        CodeMaoClient::global()
    }

    /// 获取全局身份管理器
    pub fn global_identity_manager() -> &'static KittyIdentityManager {
        get_global_identity_manager()
    }
}
