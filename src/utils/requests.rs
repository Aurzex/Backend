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
use ureq::typestate::{WithBody, WithoutBody};
use ureq::unversioned::multipart::Form;
use ureq::{Agent, Body, RequestBuilder};

// 引入日志宏(库使用者需自行选择日志实现,如 env_logger)
use log::{debug, warn};

// 错误定义(使用 thiserror)
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
    #[error("Other error: {0}")]
    Other(String),
}

pub type MewResult<T> = std::result::Result<T, MewError>;

// 基础 URL 键枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BaseKey {
    #[default]
    Default,
    Creation,
    Whale,
    Education,
    CodeGame,
    Collaboration,
    SocketCV,
    OpenService,
    Nemo,
    C,
    Player,
    Time,
    Update,
    KnCdn,
    WeChatSbp,
}

impl BaseKey {
    /// 获取枚举对应的字符串键(用于内部使用)
    pub fn as_str(&self) -> &'static str {
        match self {
            BaseKey::Default => "default",
            BaseKey::Creation => "creation",
            BaseKey::Whale => "whale",
            BaseKey::Education => "education",
            BaseKey::CodeGame => "codegame",
            BaseKey::Collaboration => "collaboration",
            BaseKey::SocketCV => "socketcv",
            BaseKey::OpenService => "open_service",
            BaseKey::Nemo => "nemo",
            BaseKey::C => "c",
            BaseKey::Player => "player",
            BaseKey::Time => "time",
            BaseKey::Update => "update",
            BaseKey::KnCdn => "kn_cdn",
            BaseKey::WeChatSbp => "wechat_sbp",
        }
    }

    /// 所有可用的基础键
    pub const ALL: [BaseKey; 15] = [
        BaseKey::Default,
        BaseKey::Creation,
        BaseKey::Whale,
        BaseKey::Education,
        BaseKey::CodeGame,
        BaseKey::Collaboration,
        BaseKey::SocketCV,
        BaseKey::OpenService,
        BaseKey::Nemo,
        BaseKey::C,
        BaseKey::Player,
        BaseKey::Time,
        BaseKey::Update,
        BaseKey::KnCdn,
        BaseKey::WeChatSbp,
    ];

    /// 根据枚举值获取对应的基础 URL
    pub fn url(&self) -> &'static str {
        match self {
            BaseKey::Default => "https://api.codemao.cn",
            BaseKey::Creation => "https://api-creation.codemao.cn",
            BaseKey::Whale => "https://api-whale.codemao.cn",
            BaseKey::Education => "https://eduzone.codemao.cn",
            BaseKey::CodeGame => "https://oversea-api.code.game",
            BaseKey::Collaboration => "https://socketcoll.codemao.cn",
            BaseKey::SocketCV => "https://socketcv.codemao.cn",
            BaseKey::OpenService => "https://open-service.codemao.cn",
            BaseKey::Nemo => "https://nemo.codemao.cn",
            BaseKey::C => "https://c.codemao.cn",
            BaseKey::Player => "https://player.codemao.cn",
            BaseKey::Time => "https://time.codemao.cn",
            BaseKey::Update => "https://update.codemao.cn",
            BaseKey::KnCdn => "https://kn-cdn.codemao.cn",
            BaseKey::WeChatSbp => "https://api-wechatsbp-codemaster.codemao.cn",
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
            "codegame" => Ok(BaseKey::CodeGame),
            "collaboration" => Ok(BaseKey::Collaboration),
            "socketcv" => Ok(BaseKey::SocketCV),
            "open_service" => Ok(BaseKey::OpenService),
            "nemo" => Ok(BaseKey::Nemo),
            "c" => Ok(BaseKey::C),
            "player" => Ok(BaseKey::Player),
            "time" => Ok(BaseKey::Time),
            "update" => Ok(BaseKey::Update),
            "kn_cdn" => Ok(BaseKey::KnCdn),
            "wechat_sbp" => Ok(BaseKey::WeChatSbp),
            _ => Err(MewError::Other(format!("无效的基础 URL 键: {}", s))),
        }
    }
}

// 常量定义
/// 默认分页大小(多数接口的单页上限)
pub const DEFAULT_PAGE_SIZE: usize = 15;
/// 默认单次拉取数量
pub const DEFAULT_LIMIT: usize = 20;
/// 默认偏移量
pub const DEFAULT_OFFSET: usize = 0;
/// 默认项目 ID(未指定时使用)
pub const DEFAULT_PID: &str = "65edCTyg";
/// 全量拉取上限标记:传给 `with_limit` 表示不设上限,
/// 迭代直到服务端返回空页或 total 字段耗尽(依赖服务端契约,否则可能无限翻页)
pub const FETCH_ALL: usize = usize::MAX;

/// 默认请求头静态切片(萌化命名)
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

/// 判断额外请求头是否已覆盖指定默认头/认证头(HTTP 头名称大小写不敏感)。
fn is_header_overridden(name: &str, extra_headers: &[(String, String)]) -> bool {
    extra_headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case(name))
}

// 身份枚举(萌化:Catsona)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Catsona {
    Fluffy,  // 普通用户(原 Average)
    Scholar, // 教育(原 Edu)
    Judge,   // 评审(原 Judgement)
    Blanky,  // 空白(原 Blank),此身份不应持有令牌
}

impl Catsona {
    /// 转换为数组索引(范围 0 到 3), 与 `ALL` 中声明顺序一致
    fn index(self) -> usize {
        self as usize
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

// 身份管理器(核心单例,扁平化设计)
/// 全局身份管理器,使用固定长度数组存储令牌
/// 采用 `RwLock` + `AtomicUsize` 分离 token 存储和当前身份索引
/// `AtomicUsize` 使用 `Release/Acquire` 排序保证身份切换后令牌可见
#[derive(Debug)]
pub struct KittyIdentityManager {
    token_bowl: RwLock<[Option<Arc<str>>; 4]>,
    current_cat: AtomicUsize,
}

impl KittyIdentityManager {
    fn new() -> Self {
        Self {
            token_bowl: RwLock::new(Default::default()),
            current_cat: AtomicUsize::new(Catsona::Fluffy.index()),
        }
    }
}

/// 全局身份管理器单例
/// 使用 `OnceLock` 确保线程安全的懒加载初始化
static GLOBAL_IDENTITY_MANAGER: OnceLock<KittyIdentityManager> = OnceLock::new();

fn get_global_identity_manager() -> &'static KittyIdentityManager {
    GLOBAL_IDENTITY_MANAGER.get_or_init(KittyIdentityManager::new)
}

// 客户端配置
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

    /// 设置为独立身份模式(不使用全局身份管理器)
    pub fn with_independent_auth(mut self) -> Self {
        self.use_global_auth = false;
        self
    }
}

// HTTP 方法
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpMethod {
    Get,
    Post,
    Delete,
    Patch,
    Put,
    Head,
}

impl HttpMethod {
    /// 返回 HTTP 方法名
    pub fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Put => "PUT",
            HttpMethod::Head => "HEAD",
        }
    }
}

impl From<HttpMethod> for &'static str {
    fn from(method: HttpMethod) -> Self {
        method.as_str()
    }
}

// 认证特质
/// 认证提供者特质,允许不同的认证实现
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

// 身份管理器实现认证特质
/// 直接为 `KittyIdentityManager` 实现 `KittyAuth`,
/// 使其成为唯一的认证逻辑实现点,全局与本地提供者只做轻量委托
impl KittyAuth for KittyIdentityManager {
    /// 当前身份(使用 `Acquire` 加载,与切换时的 `Release` 配对,保证可见性)
    fn current_identity(&self) -> Catsona {
        let idx = self.current_cat.load(Ordering::Acquire);
        Catsona::ALL[idx]
    }

    /// 当前身份对应的令牌
    fn current_token(&self) -> Option<Arc<str>> {
        let idx = self.current_cat.load(Ordering::Acquire);
        let bowl = self.token_bowl.read().unwrap();
        bowl[idx].clone()
    }

    /// 设置指定身份的令牌
    /// - 若 token 非空,则设置该身份的令牌
    /// - 若 token 为空字符串,则**清除**该身份的令牌(设为 None)
    /// - `Blanky` 身份不允许持有令牌,尝试设置将返回错误
    fn set_token(&self, identity: Catsona, token: String) -> MewResult<()> {
        if identity == Catsona::Blanky {
            return Err(MewError::Auth("Blanky identity cannot hold a token".into()));
        }
        let mut bowl = self.token_bowl.write().unwrap();
        // 空字符串表示清空令牌,否则存入新令牌
        bowl[identity.index()] = if token.is_empty() {
            None
        } else {
            Some(Arc::from(token))
        };
        Ok(())
    }

    /// 切换到指定身份
    /// 切换使用 `Release` 存储,确保之前的 token 写入对后续 `current_token` 可见
    /// Blanky 可以无条件切换(不需要令牌),其他身份必须已持有令牌
    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        if identity != Catsona::Blanky
            && self.token_bowl.read().unwrap()[identity.index()].is_none()
        {
            return Err(MewError::Auth(format!(
                "No token for identity {:?}",
                identity
            )));
        }
        self.current_cat.store(identity.index(), Ordering::Release);
        Ok(())
    }
}

/// 全局认证提供者
#[derive(Debug, Clone)]
pub struct GlobalKittyAuth;

impl Default for GlobalKittyAuth {
    fn default() -> Self {
        Self::new()
    }
}

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
        // 委托给全局管理器,会检查 Blanky 限制和空字符串清除
        get_global_identity_manager().set_token(identity, token)
    }

    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        get_global_identity_manager().switch_identity(identity)
    }
}

/// 本地认证提供者(独立实例)
#[derive(Debug, Clone)]
pub struct LocalKittyAuth {
    inner: Arc<KittyIdentityManager>,
}

impl Default for LocalKittyAuth {
    fn default() -> Self {
        Self::new()
    }
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
        // 委托给本地管理器,会检查 Blanky 限制和空字符串清除
        self.inner.set_token(identity, token)
    }

    fn switch_identity(&self, identity: Catsona) -> MewResult<()> {
        self.inner.switch_identity(identity)
    }
}

// 请求构建器
/// 请求构建器,支持链式设置可选参数(萌化名:KittyRequestBuilder)
pub struct KittyRequestBuilder {
    client: CodeMaoClient,
    method: HttpMethod,
    endpoint: String,
    base_key: Option<BaseKey>,
    params: Vec<(String, String)>,
    payload: Option<Value>,
    headers: Vec<(String, String)>,
    /// 是否将 4xx/5xx 状态码视为错误(ureq 默认行为)。
    /// 置为 false 时保留错误响应体,供调用方读取服务器错误信息。
    status_as_error: bool,
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
            status_as_error: true,
        }
    }

    /// 保留 4xx/5xx 错误响应体,使 `send` 返回可读的响应(而非直接报错)
    pub fn with_error_body(mut self) -> Self {
        self.status_as_error = false;
        self
    }

    /// 设置查询参数,多次调用将合并参数
    pub fn with_params(mut self, params: Vec<(String, String)>) -> Self {
        self.params.extend(params);
        self
    }

    /// 添加单个查询参数
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.push((key.into(), value.into()));
        self
    }

    /// 批量添加 `limit`/`offset` 分页参数(offset 缺省为 `DEFAULT_OFFSET`,limit 缺省为 `default_limit`)
    pub fn with_page(
        mut self,
        limit: Option<i32>,
        offset: Option<i32>,
        default_limit: usize,
    ) -> Self {
        self.params.push((
            "limit".into(),
            limit
                .unwrap_or(i32::try_from(default_limit).unwrap_or(0))
                .to_string(),
        ));
        self.params.push((
            "offset".into(),
            offset
                .unwrap_or(i32::try_from(DEFAULT_OFFSET).unwrap_or(0))
                .to_string(),
        ));
        self
    }

    /// 设置请求体(JSON 负载),多次调用将替换之前的 payload
    pub fn with_payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    /// 设置额外请求头,多次调用将合并头字段
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// 添加单个临时请求头
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// 发送普通请求(JSON 负载或空请求体),返回响应
    pub fn send(self) -> MewResult<Response<Body>> {
        let body = match &self.payload {
            Some(payload) => RequestBody::Json(payload),
            None => RequestBody::Empty,
        };
        self.client.inner.send((&self).into(), body)
    }

    /// 发送请求但复用外部持有的请求体(借用,避免克隆),适用于分页等重复发送场景
    pub fn send_with_payload_ref(&self, payload: &Value) -> MewResult<Response<Body>> {
        let spec: RequestSpec<'_> = self.into();
        self.client.inner.send(spec, RequestBody::Json(payload))
    }

    /// 发送 multipart/form-data 请求
    pub fn send_multipart(self, form: Form) -> MewResult<Response<Body>> {
        self.client
            .inner
            .send((&self).into(), RequestBody::Form(form))
    }
}

// 请求体抽象
/// 请求发送时的负载形态:JSON(借用)/ 表单(拥有)/ 空
enum RequestBody<'a> {
    Json(&'a Value),
    Form(Form<'a>),
    Empty,
}

// 请求参数聚合
/// 聚合一次 HTTP 请求的元参数(方法/地址/查询/头),请求体由 `RequestBody` 单独承载
struct RequestSpec<'a> {
    method: HttpMethod,
    endpoint: &'a str,
    base_key: Option<BaseKey>,
    params: &'a [(String, String)],
    extra_headers: &'a [(String, String)],
    status_as_error: bool,
}

impl<'a> From<&'a KittyRequestBuilder> for RequestSpec<'a> {
    fn from(builder: &'a KittyRequestBuilder) -> Self {
        RequestSpec {
            method: builder.method,
            endpoint: &builder.endpoint,
            base_key: builder.base_key,
            params: &builder.params,
            extra_headers: &builder.headers,
            status_as_error: builder.status_as_error,
        }
    }
}

// 内部客户端核心(萌化名:KittyCore)
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
        if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
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
        extra_headers: &[(String, String)],
        payload: Option<&Value>,
    ) {
        if !self.config.log_requests || !log::log_enabled!(log::Level::Debug) {
            return;
        }
        debug!("========== 网络请求信息 ==========");
        debug!("方法: {}", Into::<&str>::into(method));
        debug!("URL: {}", url);

        debug!("请求头:");
        for (k, v) in KITTY_HEADERS {
            if !is_header_overridden(k, extra_headers) {
                debug!("  {}: {}", k, v);
            }
        }

        if let Some((k, _)) = self.auth.auth_header()
            && !is_header_overridden(k, extra_headers)
        {
            debug!("  {}: {}", k, "[已隐藏]");
        }

        for (k, v) in extra_headers {
            let value = if k.eq_ignore_ascii_case("authorization") {
                "[已隐藏]"
            } else {
                v.as_str()
            };
            debug!("  {}: {}", k, value);
        }

        if !params.is_empty() {
            debug!("查询参数:");
            for (k, v) in params {
                debug!("  {}: {}", k, v);
            }
        }

        if let Some(payload) = payload {
            debug!("请求体:");
            if let Ok(pretty) = serde_json::to_string_pretty(payload) {
                for line in pretty.lines() {
                    debug!("  {}", line);
                }
            }
        }
    }

    fn log_response(&self, url: &str, response: &Response<Body>) -> MewResult<()> {
        if !self.config.log_requests || !log::log_enabled!(log::Level::Debug) {
            return Ok(());
        }

        debug!("========== 响应信息 ==========");
        debug!("请求 URL: {}", url);
        debug!(
            "状态: {} {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("")
        );

        debug!("响应头:");
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                debug!("  {}: {}", key, value_str);
            }
        }

        debug!("================================\n");
        Ok(())
    }

    /// 为 ureq RequestBuilder 统一设置默认头,认证头,额外头及查询参数
    fn apply_to_request_builder<B>(
        mut builder: RequestBuilder<B>,
        auth: &dyn KittyAuth,
        params: &[(String, String)],
        extra_headers: &[(String, String)],
    ) -> RequestBuilder<B> {
        // 额外头按名覆盖默认头与认证头:ureq 的 header() 是追加语义,
        // 同名单例头(如 User-Agent / Authorization)若重复发送会被服务端 400 拒绝。
        // 比较时不区分大小写(HTTP 头名称大小写不敏感,避免 "user-agent" 无法覆盖 "User-Agent")。
        for (k, v) in KITTY_HEADERS {
            if !is_header_overridden(k, extra_headers) {
                builder = builder.header(*k, *v);
            }
        }
        // 添加 Authorization 头
        if let Some((k, v)) = auth.auth_header()
            && !is_header_overridden(k, extra_headers)
        {
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

    /// 创建无请求体方法的构建器(GET/DELETE/HEAD)
    fn bodyless_builder(
        &self,
        method: HttpMethod,
        url: &str,
    ) -> MewResult<RequestBuilder<WithoutBody>> {
        match method {
            HttpMethod::Get => Ok(self.agent.get(url)),
            HttpMethod::Delete => Ok(self.agent.delete(url)),
            HttpMethod::Head => Ok(self.agent.head(url)),
            _ => Err(MewError::Other("该 HTTP 方法不支持请求体".into())),
        }
    }

    /// 创建带请求体方法的构建器(POST/PATCH/PUT)
    fn bodied_builder(&self, method: HttpMethod, url: &str) -> MewResult<RequestBuilder<WithBody>> {
        match method {
            HttpMethod::Post => Ok(self.agent.post(url)),
            HttpMethod::Patch => Ok(self.agent.patch(url)),
            HttpMethod::Put => Ok(self.agent.put(url)),
            _ => Err(MewError::Other("该 HTTP 方法需要请求体".into())),
        }
    }

    /// 统一发送请求:按方法选择无体/有体构建器,按 `RequestBody` 决定负载形态
    fn send(&self, spec: RequestSpec<'_>, body: RequestBody<'_>) -> MewResult<Response<Body>> {
        let url = self.build_url(spec.endpoint, spec.base_key);
        let payload = match &body {
            RequestBody::Json(v) => Some(*v),
            _ => None,
        };
        self.log_request(spec.method, &url, spec.params, spec.extra_headers, payload);

        let response = match spec.method {
            // 无请求体方法: 直接发送
            HttpMethod::Get | HttpMethod::Delete | HttpMethod::Head => {
                let builder = self.bodyless_builder(spec.method, &url)?;
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    spec.params,
                    spec.extra_headers,
                );
                let builder = Self::with_error_body_config(builder, spec.status_as_error);
                builder.call()?
            }
            // 带请求体方法: 按负载形态发送 JSON/表单/空请求体
            HttpMethod::Post | HttpMethod::Patch | HttpMethod::Put => {
                let builder = self.bodied_builder(spec.method, &url)?;
                let builder = Self::apply_to_request_builder(
                    builder,
                    self.auth.as_ref(),
                    spec.params,
                    spec.extra_headers,
                );
                let builder = Self::with_error_body_config(builder, spec.status_as_error);
                match body {
                    RequestBody::Json(payload) => builder.send_json(payload)?,
                    RequestBody::Form(form) => builder.send(form)?,
                    RequestBody::Empty => builder.send_empty()?,
                }
            }
        };

        self.log_response(&url, &response)?;
        Ok(response)
    }

    /// 需要保留错误响应体时,在请求级关闭 ureq 的 http_status_as_error
    fn with_error_body_config<B>(
        builder: RequestBuilder<B>,
        status_as_error: bool,
    ) -> RequestBuilder<B> {
        if status_as_error {
            builder
        } else {
            builder.config().http_status_as_error(false).build()
        }
    }

    /// 将响应体解析为 JSON
    fn response_to_json(&self, response: Response<Body>) -> MewResult<Value> {
        let mut body = response.into_body();
        let bytes = body.read_to_vec()?;
        if bytes.is_empty() {
            if self.config.log_requests && log::log_enabled!(log::Level::Debug) {
                debug!("响应体: (空)");
            }
            return Ok(Value::Null);
        }

        let json: Value = serde_json::from_slice(&bytes)?;
        if self.config.log_requests && log::log_enabled!(log::Level::Debug) {
            debug!("---------- 响应体 (JSON) ----------");
            if let Ok(pretty) = serde_json::to_string_pretty(&json) {
                for line in pretty.lines() {
                    debug!("  {}", line);
                }
            }
            debug!("------------------------------------");
        }
        Ok(json)
    }

    /// 将响应体读取为字符串
    fn response_to_string(&self, response: Response<Body>) -> MewResult<String> {
        let mut body = response.into_body();
        let text = body.read_to_string()?;
        if self.config.log_requests && log::log_enabled!(log::Level::Debug) {
            debug!("---------- 响应体 (文本) ----------");
            if text.len() > 1000 {
                let preview: String = text.chars().take(1000).collect();
                debug!("  {}", preview);
                debug!("  ... (剩余 {} 字符被截断)", text.len() - 1000);
            } else {
                debug!("  {}", text);
            }
            debug!("-------------------------------------");
        }
        Ok(text)
    }

    /// 将响应体读取为二进制数据
    fn response_to_binary(&self, response: Response<Body>) -> MewResult<Vec<u8>> {
        let mut body = response.into_body();
        let data = body.read_to_vec()?;
        if self.config.log_requests && log::log_enabled!(log::Level::Debug) {
            debug!("---------- 响应体 (二进制) ----------");
            debug!("  大小: {} 字节", data.len());
            if let Ok(text) = std::str::from_utf8(&data) {
                debug!("  内容预览:");
                let preview: String = text.chars().take(200).collect();
                debug!("  {}", preview);
                if text.len() > 200 {
                    debug!("  ... (截断)");
                }
            } else {
                let preview_len = std::cmp::min(data.len(), 64);
                let hex: String = data[..preview_len]
                    .iter()
                    .map(|b| format!("{:02x} ", b))
                    .collect();
                debug!("  十六进制预览: {}", hex);
                if data.len() > 64 {
                    debug!("  ... (截断)");
                }
            }
            debug!("--------------------------------------");
        }
        Ok(data)
    }
}

// 公开的 CodeMaoClient
/// 主客户端,支持全局单例和独立实例两种模式
#[derive(Clone)]
pub struct CodeMaoClient {
    inner: Arc<KittyCore>,
}

impl CodeMaoClient {
    /// 获取全局单例实例(使用全局身份管理器,默认配置)
    /// 首次调用时自动初始化,后续调用返回同一实例
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<CodeMaoClient> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            CodeMaoClient::new_with_auth(KittyConfig::default(), Arc::new(GlobalKittyAuth::new()))
        })
    }

    /// 创建使用全局身份管理器的客户端(可自定义配置)
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

    /// 创建新实例(向后兼容,根据配置决定使用全局或独立身份管理器)
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
    /// - 非空 token:设置该身份的令牌
    /// - 空字符串:清除该身份的令牌(设为 None)
    /// - `Blanky` 身份不允许设置令牌
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

    /// 构建请求,返回支持链式设置的 KittyRequestBuilder
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
    pub fn build_paginated(&self, endpoint: impl Into<String>) -> PaginatedIter {
        PaginatedIter::new(self.clone(), endpoint)
    }

    /// 创建文件上传器
    pub fn file_uploader(&self) -> FileUploader {
        FileUploader::new(self.clone())
    }
}

// 分页配置

#[derive(Debug, Clone, Copy)]
pub enum PaginationMethod {
    /// 偏移量分页:offset = page * page_size
    Offset,
    /// 页码分页:page = page + 1
    Page,
}

impl PaginationMethod {
    /// 根据当前页号(内部从 0 开始)计算偏移参数值
    fn calc_offset(&self, page: usize, page_size: usize) -> String {
        match self {
            PaginationMethod::Offset => (page * page_size).to_string(),
            PaginationMethod::Page => (page + 1).to_string(),
        }
    }
}

/// 分页详细配置,可通过 `with_config` 整体设置
#[derive(Debug, Clone)]
pub struct PaginationConfig {
    /// 每页大小,默认 15
    pub page_size: usize,
    /// 请求中表示"每页数量"的参数名(默认 "limit")
    pub amount_key: Option<String>,
    /// 请求中表示"页码/偏移"的参数名(默认 "offset")
    pub offset_key: Option<String>,
    /// 响应中表示"实际每页大小"的 JSON 键(可选)
    pub response_amount_key: Option<String>,
    /// 响应中表示"偏移/页码"的 JSON 键(可选)
    pub response_offset_key: Option<String>,
}

impl Default for PaginationConfig {
    fn default() -> Self {
        Self {
            page_size: 15,
            amount_key: Some("limit".into()),
            offset_key: Some("offset".into()),
            response_amount_key: None,
            response_offset_key: None,
        }
    }
}

// 分页迭代器状态(三态枚举)

/// 分页迭代器的内部状态机
enum IterState {
    /// 还未发送任何请求
    Uninit,
    /// 已初始化,缓存了当前页数据及元信息
    Ready {
        /// 当前页码(从 0 开始)
        current_page: usize,
        /// 当前页的所有数据
        current_page_data: Vec<Value>,
        /// 当前页内的消费指针
        current_index: usize,
        /// 总数(若响应未提供则为 None)
        total: Option<usize>,
    },
    /// 迭代已终止
    Finished,
}

// PaginatedIter 完整优化版

pub struct PaginatedIter {
    client: CodeMaoClient,
    method: HttpMethod,
    endpoint: String,
    /// 基础查询参数,所有分页请求都会携带
    base_params: Vec<(String, String)>,
    /// POST/PUT 等请求的 JSON 负载(可选)
    payload: Option<Value>,
    /// 用户设定的获取上限(最多产出多少条)
    limit: Option<usize>,
    /// 可选的基础 URL 键
    base_key: Option<BaseKey>,

    /// 响应中总数的 JSON Pointer 路径
    total_pointer: String,
    /// 响应中数据数组的 JSON Pointer 路径
    data_pointer: String,

    /// 分页计算方式(偏移量 / 页码)
    pagination_method: PaginationMethod,
    /// 分页配置详情
    config: PaginationConfig,

    /// 迭代器内部状态
    state: IterState,
    /// 已经通过 next() 产出的元素总数
    yielded: usize,
}

impl PaginatedIter {
    // 构造与基础配置

    /// 创建一个新的分页迭代器,默认 GET 请求,endpoint 可拼接完整 URL 或相对路径
    pub fn new(client: CodeMaoClient, endpoint: impl Into<String>) -> Self {
        Self {
            client,
            method: HttpMethod::Get,
            endpoint: endpoint.into(),
            base_params: Vec::new(),
            payload: None,
            limit: None,
            base_key: None,
            total_pointer: Self::key_to_pointer("total"),
            data_pointer: Self::key_to_pointer("items"),
            pagination_method: PaginationMethod::Offset,
            config: PaginationConfig::default(),
            state: IterState::Uninit,
            yielded: 0,
        }
    }

    /// 将点分隔的 key 转换为 JSON Pointer 路径
    /// 例如 "data.items" -> "/data/items"
    fn key_to_pointer(key: &str) -> String {
        format!("/{}", key.replace('.', "/"))
    }

    // 链式配置方法(保留重要参数)

    /// 设置 HTTP 方法(默认 GET)
    pub fn with_iter_method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    /// 添加单个基础查询参数
    pub fn with_iter_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.base_params.push((key.into(), value.into()));
        self
    }

    /// 批量添加基础查询参数
    pub fn with_iter_params(mut self, params: Vec<(String, String)>) -> Self {
        self.base_params.extend(params);
        self
    }

    /// 设置 POST/PUT 请求的 JSON 负载
    pub fn with_iter_payload(mut self, payload: Value) -> Self {
        self.payload = Some(payload);
        self
    }

    /// 设置最多获取的元素数量
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// 设置响应中总数的 JSON 键路径(点分隔,如 "data.total")
    pub fn with_total_key(mut self, key: impl Into<String>) -> Self {
        self.total_pointer = Self::key_to_pointer(&key.into());
        self
    }

    /// 设置响应中数据数组的 JSON 键路径(点分隔,如 "data.items")
    pub fn with_data_key(mut self, key: impl Into<String>) -> Self {
        self.data_pointer = Self::key_to_pointer(&key.into());
        self
    }

    /// 设置分页计算方式(偏移量或页码)
    pub fn with_pagination_method(mut self, method: PaginationMethod) -> Self {
        self.pagination_method = method;
        self
    }

    /// 完整设置分页配置(可覆盖默认的每页大小,参数名等)
    pub fn with_config(mut self, config: PaginationConfig) -> Self {
        self.config = config;
        self
    }
    /// 设置请求中"每页数量"的参数名
    pub fn with_amount_key(mut self, key: impl Into<String>) -> Self {
        self.config.amount_key = Some(key.into());
        self
    }
    /// 设置请求中"页码/偏移"的参数名
    pub fn with_offset_key(mut self, key: impl Into<String>) -> Self {
        self.config.offset_key = Some(key.into());
        self
    }
    /// 设置响应中"实际每页大小"的 JSON 键
    pub fn with_response_amount_key(mut self, key: impl Into<String>) -> Self {
        self.config.response_amount_key = Some(key.into());
        self
    }
    /// 设置响应中"偏移/页码"的 JSON 键
    pub fn with_response_offset_key(mut self, key: impl Into<String>) -> Self {
        self.config.response_offset_key = Some(key.into());
        self
    }

    /// 设置每页大小
    pub fn with_page_size(mut self, size: usize) -> Self {
        self.config.page_size = size;
        self
    }

    /// 设置基础 URL 键
    pub fn with_base_key(mut self, key: BaseKey) -> Self {
        self.base_key = Some(key);
        self
    }

    // 内部请求逻辑

    /// 构造指定页的请求参数:基础参数 + 分页参数
    /// 过滤与分页键同名的基础参数,避免重复键(服务端对重复键的取值框架相关)
    fn build_params(&self, page: usize) -> Vec<(String, String)> {
        let mut params = Vec::with_capacity(self.base_params.len() + 2);
        params.extend(
            self.base_params
                .iter()
                .filter(|(k, _)| {
                    Some(k.as_str()) != self.config.amount_key.as_deref()
                        && Some(k.as_str()) != self.config.offset_key.as_deref()
                })
                .cloned(),
        );
        if let Some(key) = &self.config.amount_key {
            params.push((key.clone(), self.config.page_size.to_string()));
        }
        if let Some(key) = &self.config.offset_key {
            let offset = self
                .pagination_method
                .calc_offset(page, self.config.page_size);
            params.push((key.clone(), offset));
        }
        params
    }

    /// 统一发送一页请求,返回完整的响应 JSON
    /// 封装了参数构建,负载附加,发送与 JSON 解析
    fn request_page(&self, page: usize) -> MewResult<Value> {
        let params = self.build_params(page);
        let builder = self
            .client
            .build_request(self.method, &self.endpoint, self.base_key)
            .with_params(params);
        // 请求体借用发送,避免每翻一页克隆一次 payload
        let response = if let Some(payload) = &self.payload {
            builder.send_with_payload_ref(payload)?
        } else {
            builder.send()?
        };
        self.client.response_to_json(response)
    }

    /// 从响应 JSON 中提取数据数组,缺失或类型不符时返回空数组
    fn take_page_data(json: &mut Value, data_pointer: &str) -> Vec<Value> {
        json.pointer_mut(data_pointer)
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// 从响应 JSON 中尝试提取总数(total),支持多种数字类型,并对浮点数做范围检查
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "转换前有非负且 usize 范围守卫"
    )]
    fn try_extract_total(json: &Value, total_pointer: &str) -> Option<usize> {
        let value = json.pointer(total_pointer)?;
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|i| u64::try_from(i).ok()))
            // 浮点数需检查非负且在 usize 范围内(守卫保证下方转换安全)
            .or_else(|| {
                let f = value.as_f64()?;
                (f >= 0.0 && f <= usize::MAX as f64).then_some(f as u64)
            })
            .and_then(|n| usize::try_from(n).ok())
    }

    /// 惰性初始化:发送第一页请求,解析元数据并设置为 Ready 状态
    fn initialize(&mut self) -> MewResult<()> {
        let mut json = self.request_page(0)?;
        let total = Self::try_extract_total(&json, &self.total_pointer);

        // 若配置了响应中的实际每页大小键,则用服务器返回值覆盖 page_size
        if let Some(key) = &self.config.response_amount_key {
            let pointer = Self::key_to_pointer(key);
            if let Some(n) = json.pointer(&pointer).and_then(serde_json::Value::as_u64) {
                self.config.page_size = usize::try_from(n).unwrap_or(usize::MAX);
            }
        }

        let data = Self::take_page_data(&mut json, &self.data_pointer);
        self.state = IterState::Ready {
            current_page: 0,
            current_page_data: data,
            current_index: 0,
            total,
        };
        Ok(())
    }

    /// 是否已达到用户设置的获取上限
    fn reached_limit(&self) -> bool {
        self.limit.is_some_and(|lim| self.yielded >= lim)
    }

    // 迭代器核心:next_item

    /// 获取下一个元素,内部惰性初始化并自动翻页
    fn next_item(&mut self) -> Option<MewResult<Value>> {
        // 首次调用时自动初始化
        if matches!(self.state, IterState::Uninit)
            && let Err(e) = self.initialize()
        {
            self.state = IterState::Finished;
            return Some(Err(e));
        }

        loop {
            // 取出状态,用 Finished 占位,避免复杂借用
            let state = std::mem::replace(&mut self.state, IterState::Finished);
            let IterState::Ready {
                current_page,
                mut current_page_data,
                current_index,
                total,
            } = state
            else {
                // Finished / Uninit 均视为迭代结束
                return None;
            };

            // 达到用户设置的获取上限后立即结束
            if self.reached_limit() {
                return None;
            }

            // 当前页还有数据,直接返回
            if current_index < current_page_data.len() {
                let item = std::mem::replace(&mut current_page_data[current_index], Value::Null);
                self.state = IterState::Ready {
                    current_page,
                    current_page_data,
                    current_index: current_index + 1,
                    total,
                };
                self.yielded += 1;
                return Some(Ok(item));
            }

            // 已知总数且累计产出已达总数,则不再请求下一页;总数未知时必须尝试。
            // 按累计产出而非 (页号+1)*页大小 判定:页被服务端截断/过滤时不再提前终止
            if total.is_some_and(|t| self.yielded >= t) {
                return None;
            }

            let next_page = current_page + 1;
            let mut json = match self.request_page(next_page) {
                Ok(json) => json,
                Err(e) => {
                    // 恢复当前页状态:瞬时错误后 next() 可重试同一页,而非把流置为 Finished 永久终止
                    // (原实现此时 state 已是 Finished,单个页请求失败会截断整个数据流)
                    self.state = IterState::Ready {
                        current_page,
                        current_page_data,
                        current_index,
                        total,
                    };
                    return Some(Err(e));
                }
            };

            let data = Self::take_page_data(&mut json, &self.data_pointer);
            if data.is_empty() {
                return None; // 空数据表示结束
            }

            self.state = IterState::Ready {
                current_page: next_page,
                current_page_data: data,
                current_index: 0,
                total,
            };
            // 循环继续,下一次将返回新页第一条
        }
    }

    /// 一次性收集所有剩余元素
    pub fn collect(mut self) -> MewResult<Vec<Value>> {
        let mut items = Vec::new();
        while let Some(item) = self.next_item() {
            items.push(item?);
        }
        Ok(items)
    }

    /// 仅获取元数据(总数,页数等),不迭代数据
    pub fn fetch_metadata(&mut self) -> MewResult<()> {
        if matches!(self.state, IterState::Uninit) {
            self.initialize()?;
        }
        Ok(())
    }

    // 新增页面元数据查询方法(&self,不触发网络请求)

    /// 获取当前页码(从 1 开始),仅在已成功请求至少一页时返回 Some
    pub fn current_page_number(&self) -> Option<usize> {
        match &self.state {
            IterState::Ready { current_page, .. } => Some(current_page + 1),
            _ => None,
        }
    }

    /// 获取服务器返回的总条目数,仅在成功解析后返回 Some
    pub fn total_items(&self) -> Option<usize> {
        match &self.state {
            IterState::Ready { total, .. } => *total,
            _ => None,
        }
    }

    /// 获取已通过迭代器产出的元素个数
    pub fn yielded_count(&self) -> usize {
        self.yielded
    }

    /// 计算总页数(基于 total / page_size),仅在 total 已知时返回 Some
    pub fn total_pages(&self) -> Option<usize> {
        let total = self.total_items()?;
        let page_size = self.config.page_size;
        // 每页大小为 0 时无法计算, 返回 None
        (page_size > 0).then(|| total.div_ceil(page_size))
    }

    /// 获取当前使用的每页大小(可能因响应调整而变化)
    pub fn page_size(&self) -> usize {
        self.config.page_size
    }

    /// 获取预估的剩余元素数量,与 size_hint().1 一致
    pub fn remaining_items(&self) -> Option<usize> {
        self.size_hint().1
    }
}

// 实现 Iterator trait(含 size_hint)

impl Iterator for PaginatedIter {
    type Item = MewResult<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_item()
    }

    /// 提供剩余元素数量的上下界,供标准库适配器预分配容量
    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.state {
            IterState::Finished => (0, Some(0)),
            IterState::Uninit => {
                // 未初始化时,上限由 limit 决定(若无则为 None)
                let upper = self.limit;
                (0, upper)
            }
            IterState::Ready {
                current_page_data,
                current_index,
                total,
                ..
            } => {
                // 当前页未消费的元素数
                let remaining_in_page = current_page_data.len().saturating_sub(*current_index);

                // 根据 total 和 limit 计算精确剩余上限
                let known_remaining = total.map(|t| t.saturating_sub(self.yielded));
                let limit_remaining = self.limit.map(|lim| lim.saturating_sub(self.yielded));
                let exact_upper = match (known_remaining, limit_remaining) {
                    (Some(k), Some(l)) => Some(k.min(l)),
                    (Some(k), None) => Some(k),
                    (None, Some(l)) => Some(l),
                    (None, None) => None,
                };

                // 下限:至少是当前页剩余,但不超过已知上限(上限未知时视为无限)
                let lower = remaining_in_page.min(exact_upper.unwrap_or(usize::MAX));
                (lower, exact_upper)
            }
        }
    }
}

// 文件上传器
pub struct FileUploader {
    client: CodeMaoClient,
}

impl FileUploader {
    pub fn new(client: CodeMaoClient) -> Self {
        Self { client }
    }

    /// 统一上传入口
    pub fn upload(
        &self,
        file_path: &Path,
        channel: UploadChannel,
        save_path: &str,
    ) -> MewResult<String> {
        match channel {
            UploadChannel::Pgaot => self.upload_pgaot(file_path, save_path),
            UploadChannel::Codegame => self.upload_codegame(file_path, save_path),
            UploadChannel::Codemao => self.upload_codemao(file_path, save_path),
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
        Self::required_str_field(&json, "url")
    }

    fn upload_codegame(&self, file_path: &Path, save_path: &str) -> MewResult<String> {
        let token_info = self.get_codegame_token(save_path, file_path)?;

        let json = self.upload_qiniu_form(
            &token_info.token,
            &token_info.file_path,
            "avatar",
            file_path,
            &token_info.upload_url,
        )?;
        let key = Self::required_str_field(&json, "key")?;
        Ok(format!("{}/{}", token_info.bucket_url, key))
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
        let json = self.upload_qiniu_form(
            &token_info.token,
            &token_info.file_path,
            &unique_filename,
            file_path,
            &token_info.upload_url,
        )?;
        let key = Self::required_str_field(&json, "key")?;
        Ok(format!("{}{}", token_info.bucket_url, key))
    }

    /// 发送七牛云上传表单(token/key/fname/file), 返回响应 JSON
    fn upload_qiniu_form(
        &self,
        token: &str,
        key: &str,
        fname: &str,
        file_path: &Path,
        upload_url: &str,
    ) -> MewResult<Value> {
        let form = Form::new()
            .text("token", token)
            .text("key", key)
            .text("fname", fname)
            .file("file", file_path)?;

        let response = self
            .client
            .build_request(HttpMethod::Post, upload_url, None)
            .send_multipart(form)?;
        self.client.response_to_json(response)
    }

    /// 从响应 JSON 中取出指定键的数组, 并返回数组首元素
    fn first_token_entry<'a>(json: &'a Value, key: &str, error: &str) -> MewResult<&'a Value> {
        json.get(key)
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .ok_or_else(|| MewError::Other(error.into()))
    }

    /// 从响应 JSON 中提取必填字符串字段,缺失时报错而非静默返回空串
    fn required_str_field(json: &Value, key: &str) -> MewResult<String> {
        json.get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| MewError::Other(format!("上传响应缺少必填字段: {}", key)))
    }

    fn get_codemao_token(&self, file_path: &str) -> MewResult<UploadTokenInfo> {
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/cdn/qi-niu/tokens/uploading",
                Some(BaseKey::OpenService),
            )
            .with_param("projectName", "community_frontend")
            .with_param("filePaths", file_path)
            .with_param("filePath", file_path)
            .with_param("tokensCount", "1")
            .with_param("fileSign", "p1")
            .with_param("cdnName", "qiniu")
            .send()?;

        let json = self.client.response_to_json(response)?;
        let token_info = Self::first_token_entry(&json, "tokens", "No tokens array")?;

        Ok(UploadTokenInfo {
            token: Self::required_str_field(token_info, "token")?,
            file_path: Self::required_str_field(token_info, "file_path")?,
            upload_url: Self::required_str_field(&json, "upload_url")?,
            bucket_url: Self::required_str_field(&json, "bucket_url")?,
        })
    }

    fn get_codegame_token(&self, prefix: &str, file_path: &Path) -> MewResult<UploadTokenInfo> {
        let extension = file_path
            .extension()
            .map(|ext| format!(".{}", ext.to_string_lossy()))
            .unwrap_or_default();

        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/tiger/kitten/cdn/token/1",
                Some(BaseKey::CodeGame),
            )
            .with_param("prefix", prefix)
            .with_param("bucket", "static")
            .with_param("type", extension)
            .send()?;

        let json = self.client.response_to_json(response)?;
        let token_data = Self::first_token_entry(&json, "data", "No data array")?;

        Ok(UploadTokenInfo {
            token: Self::required_str_field(token_data, "token")?,
            file_path: Self::required_str_field(token_data, "filename")?,
            upload_url: "https://upload.qiniup.com".to_string(),
            bucket_url: Self::required_str_field(&json, "bucket_url")?,
        })
    }
}

// 内部数据结构
/// 七牛云上传令牌信息, codegame 与 codemao 共用
struct UploadTokenInfo {
    token: String,
    file_path: String,
    upload_url: String,
    bucket_url: String,
}

// 辅助函数

/// 生成指定长度、指定字符集的随机 ID
/// 字符集由调用方给定(如上传文件名用全字符集,客户端/会话 ID 用小写字母+数字);仅支持单字节 ASCII 字符集
pub fn generate_random_id(length: usize, charset: &[u8]) -> String {
    (0..length)
        .map(|_| charset[fastrand::usize(0..charset.len())] as char)
        .collect()
}

/// 生成指定长度的随机字母数字串(62 字符集,用于上传文件名等)
fn generate_meow_id(length: usize) -> String {
    const CHARSET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    generate_random_id(length, CHARSET)
}

// HTTP 状态码枚举
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

// 简化的工厂(萌化名:KittyFactory)
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

    /// 获取全局客户端实例(使用默认配置)
    pub fn global_client() -> &'static CodeMaoClient {
        CodeMaoClient::global()
    }

    /// 获取全局身份管理器
    pub fn global_identity_manager() -> &'static KittyIdentityManager {
        get_global_identity_manager()
    }
}

/// 获取 13 位毫秒时间戳(本地时间)
/// 若系统时间异常(早于 Unix 纪元),则返回 0 并记录警告
pub fn current_timestamp_13() -> u128 {
    if let Ok(dur) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        dur.as_millis()
    } else {
        warn!("系统时间异常,无法获取时间戳,返回 0");
        0
    }
}

/// 获取秒级时间戳(本地时间)
/// 基于 `current_timestamp_13` 换算,系统时间异常时同样返回 0
pub fn current_timestamp_secs() -> i64 {
    (current_timestamp_13() / 1000) as i64
}

// 共享请求辅助(trait)
/// 发送请求后返回的数据形态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseMode {
    /// 返回响应 JSON 数据
    Data,
    /// 仅返回成功标志 `{ "success": bool }`
    Status,
}

/// 开关型动作(点赞/收藏/置顶/关注等)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleAction {
    On,
    Off,
}

impl ToggleAction {
    pub(crate) fn to_http_method(
        self,
        on_method: HttpMethod,
        off_method: HttpMethod,
    ) -> HttpMethod {
        match self {
            ToggleAction::On => on_method,
            ToggleAction::Off => off_method,
        }
    }
}

/// 上传通道
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadChannel {
    Pgaot,
    Codegame,
    Codemao,
}

impl UploadChannel {
    /// 从通道字符串解析(非法值返回 None)
    pub fn parse(s: &str) -> Option<UploadChannel> {
        match s {
            "pgaot" => Some(UploadChannel::Pgaot),
            "codegame" => Some(UploadChannel::Codegame),
            "codemao" => Some(UploadChannel::Codemao),
            _ => None,
        }
    }
}

/// 各 API 管理器的共享请求辅助,消除每个 Manager 中重复的
/// `check_status`/`send_and_parse`/`send_maybe_parse` 样板
/// 实现方只需提供 `client()` 访问器,其余方法由默认实现提供
pub trait ClientAccess {
    /// 返回管理器持有的客户端
    fn client(&self) -> &CodeMaoClient;

    /// 发送请求并检查响应状态码是否为预期值
    fn check_status(&self, builder: KittyRequestBuilder, expected: HTTPStatus) -> MewResult<bool> {
        let response = send_checked(builder)?;
        Ok(response.status() == expected as u16)
    }

    /// 发送请求并将响应解析为 JSON
    fn send_and_parse(&self, builder: KittyRequestBuilder) -> MewResult<Value> {
        let response = send_checked(builder)?;
        self.client().response_to_json(response)
    }

    /// 发送请求,根据 `mode` 决定返回 JSON 数据或成功标志
    fn send_maybe_parse(
        &self,
        builder: KittyRequestBuilder,
        mode: ResponseMode,
        expected: HTTPStatus,
    ) -> MewResult<Value> {
        let response = send_checked(builder)?;
        if mode == ResponseMode::Data {
            self.client().response_to_json(response)
        } else {
            Ok(serde_json::json!({ "success": response.status() == expected as u16 }))
        }
    }
}

/// 发送请求并统一处理 4xx/5xx:错误时读取服务端错误体并包装为 `MewError`。
/// builder 已持有客户端,无需额外 client 参数;供 `ClientAccess` 默认方法复用。
fn send_checked(builder: KittyRequestBuilder) -> MewResult<Response<Body>> {
    let response = builder.with_error_body().send()?;
    let status = response.status();
    if status.is_client_error() || status.is_server_error() {
        let body = response.into_body().read_to_string().unwrap_or_default();
        return Err(MewError::Other(format!("HTTP {status}: {body}")));
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_override_is_case_insensitive() {
        let extra_headers = vec![("user-agent".to_string(), "okhttp/4.2.2".to_string())];

        assert!(is_header_overridden("User-Agent", &extra_headers));
        assert!(is_header_overridden("USER-AGENT", &extra_headers));
        assert!(!is_header_overridden("Accept", &extra_headers));
    }
}
