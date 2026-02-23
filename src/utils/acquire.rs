use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rand::RngExt;
use serde_json::Value;
use ureq::Body;

use ureq::http::{Response, StatusCode};
use ureq::typestate::{WithBody, WithoutBody};
use ureq::unversioned::multipart::Form;
use ureq::{Agent, Error as UreqError, RequestBuilder};



// HTTP 方法枚举
#[derive(Debug, Clone, Copy)]
pub enum HttpMethod {
    GET,
    POST,
    DELETE,
    PATCH,
    PUT,
    HEAD,
}

impl From<HttpMethod> for &'static str {
    fn from(method: HttpMethod) -> Self {
        match method {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::PATCH => "PATCH",
            HttpMethod::PUT => "PUT",
            HttpMethod::HEAD => "HEAD",
        }
    }
}

// HTTP 状态码枚举
#[derive(Debug, PartialEq, Eq)]
pub enum HttpStatus {
    OK = 200,
    Created = 201,
    NoContent = 204,
    NotModified = 304,
    Forbidden = 403,
    NotFound = 404,
}

impl HttpStatus {
    pub fn from_code(code: u16) -> Option<Self> {
        match code {
            200 => Some(HttpStatus::OK),
            201 => Some(HttpStatus::Created),
            204 => Some(HttpStatus::NoContent),
            304 => Some(HttpStatus::NotModified),
            403 => Some(HttpStatus::Forbidden),
            404 => Some(HttpStatus::NotFound),
            _ => None,
        }
    }
}

// 分页方式
#[derive(Debug, Clone, Copy)]
pub enum PaginationMethod {
    Offset,
    Page,
}
// 添加这个结构体
#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub data: Value,
}

impl HttpResponse {
    pub fn from_response(response: Response<Body>) -> Result<Self, UreqError> {
        let status = response.status().as_u16();
        let data = response.into_body().read_json()?;

        Ok(Self { status, data })
    }

    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    pub fn is_client_error(&self) -> bool {
        self.status >= 400 && self.status < 500
    }

    pub fn is_server_error(&self) -> bool {
        self.status >= 500
    }
}
// 客户端配置
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub api_base_urls: HashMap<&'static str, &'static str>,
    pub default_base_url_key: &'static str,
    pub timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub log_requests: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        let mut api_base_urls: HashMap<&str, &str> = HashMap::new();
        api_base_urls.insert("default", "https://api.codemao.cn");
        api_base_urls.insert("creation", "https://api-creation.codemao.cn");
        api_base_urls.insert("whale", "https://api-whale.codemao.cn");
        api_base_urls.insert("education", "https://eduzone.codemao.cn");

        Self {
            api_base_urls,
            default_base_url_key: "default",
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(1000),
            log_requests: false,
        }
    }
}

impl ClientConfig {
    pub fn get_base_url(&self, key: Option<&str>) -> &str {
        let key = key.unwrap_or(self.default_base_url_key);
        self.api_base_urls
            .get(key)
            .copied()
            .unwrap_or(self.api_base_urls[self.default_base_url_key])
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_retry_delay(mut self, retry_delay: Duration) -> Self {
        self.retry_delay = retry_delay;
        self
    }

    pub fn with_log_requests(mut self, log_requests: bool) -> Self {
        self.log_requests = log_requests;
        self
    }
}

// 分页配置
#[derive(Debug, Clone, Default)]
pub struct PaginationConfig {
    pub amount_key: Option<String>,
    pub offset_key: Option<String>,
    pub response_amount_key: Option<String>,
    pub response_offset_key: Option<String>,
}

impl PaginationConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_amount_key(mut self, key: impl Into<String>) -> Self {
        self.amount_key = Some(key.into());
        self
    }

    pub fn with_offset_key(mut self, key: impl Into<String>) -> Self {
        self.offset_key = Some(key.into());
        self
    }

    pub fn with_response_amount_key(mut self, key: impl Into<String>) -> Self {
        self.response_amount_key = Some(key.into());
        self
    }

    pub fn with_response_offset_key(mut self, key: impl Into<String>) -> Self {
        self.response_offset_key = Some(key.into());
        self
    }
}

// Token 管理
#[derive(Debug, Clone, Default)]
pub struct Token {
    pub average: String,
    pub edu: String,
    pub judgement: String,
    pub blank: String,
}

// 身份管理器
pub struct IdentityManager {
    tokens: Token,
    current_identity: String,
    backup_tokens: HashMap<String, String>,
}

impl IdentityManager {
    pub fn new() -> Self {
        Self {
            tokens: Token::default(),
            current_identity: "blank".to_string(),
            backup_tokens: HashMap::new(),
        }
    }

    pub fn switch_identity(&mut self, identity: &str, token: &str) -> Result<(), String> {
        let valid_identities = ["average", "edu", "judgement", "blank"];
        if !valid_identities.contains(&identity) {
            return Err(format!("无效的身份: {}", identity));
        }

        // 备份当前令牌
        let current_token = self.get_current_token();
        if !current_token.is_empty() && self.current_identity != "blank" {
            self.backup_tokens
                .insert(self.current_identity.clone(), current_token);
        }

        // 设置新令牌
        if !token.trim().is_empty() {
            match identity {
                "average" => self.tokens.average = token.to_string(),
                "edu" => self.tokens.edu = token.to_string(),
                "judgement" => self.tokens.judgement = token.to_string(),
                "blank" => self.tokens.blank = token.to_string(),
                _ => unreachable!(),
            }
            self.current_identity = identity.to_string();
            Ok(())
        } else if identity != "blank" {
            Err(format!("警告: 尝试设置空令牌到身份 {}", identity))
        } else {
            Ok(())
        }
    }

    pub fn restore_identity(&mut self, identity: &str) -> bool {
        if let Some(token) = self.backup_tokens.get(identity) {
            if !token.trim().is_empty() {
                match identity {
                    "average" => self.tokens.average = token.clone(),
                    "edu" => self.tokens.edu = token.clone(),
                    "judgement" => self.tokens.judgement = token.clone(),
                    "blank" => self.tokens.blank = token.clone(),
                    _ => return false,
                }
                self.current_identity = identity.to_string();
                println!("已恢复身份: {}", identity);
                return true;
            }
        }
        false
    }

    pub fn backup_current_token(&mut self) {
        if self.current_identity != "blank" {
            let current_token = self.get_current_token();
            if !current_token.is_empty() {
                self.backup_tokens
                    .insert(self.current_identity.clone(), current_token);
            }
        }
    }

    pub fn get_current_token(&self) -> String {
        match self.current_identity.as_str() {
            "average" => self.tokens.average.clone(),
            "edu" => self.tokens.edu.clone(),
            "judgement" => self.tokens.judgement.clone(),
            _ => self.tokens.blank.clone(),
        }
    }

    pub fn get_auth_header(&self) -> Option<(String, String)> {
        let token = self.get_current_token();
        if token.is_empty() || token.trim().is_empty() {
            println!(
                "警告: 身份 '{}' 的令牌为空, 无法生成认证头",
                self.current_identity
            );
            None
        } else {
            Some(("Authorization".to_string(), format!("Bearer {}", token)))
        }
    }

    pub fn current_identity(&self) -> &str {
        &self.current_identity
    }
}

impl Default for IdentityManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HttpClient {
    config: ClientConfig,
    agent: Agent,
    headers: Arc<Mutex<HashMap<String, String>>>, // Use Mutex
}
impl HttpClient {
    pub fn new(config: ClientConfig) -> Self {
        let agent: Agent = ureq::Agent::config_builder()
            .timeout_global(Some(config.timeout))
            .build()
            .into();

        Self {
            config,
            agent,
            headers: Arc::from(Mutex::from(HashMap::new())),
        }
    }

    pub fn update_headers(&mut self, headers: HashMap<String, String>) {
        for (k, v) in headers {
            if !v.trim().is_empty() {
                self.headers.lock().unwrap().insert(k, v);
            }
        }
        // 移除空的 Authorization 头
        if let Some(auth) = self.headers.lock().unwrap().get("Authorization") {
            if auth.trim().is_empty() || auth == "Bearer" {
                self.headers.lock().unwrap().remove("Authorization");
                println!("警告: Authorization 头为空, 移除该头");
            }
        }
    }

    // 无 Body 请求构建函数
    fn build_request_without_body(
        &self,
        method: HttpMethod,
        url: &str,
    ) -> RequestBuilder<WithoutBody> {
        // 根据 HTTP 方法调用对应的 Agent 方法
        let req = match method {
            HttpMethod::GET => self.agent.get(url),
            HttpMethod::DELETE => self.agent.delete(url),
            HttpMethod::HEAD => self.agent.head(url),
            _ => panic!("不支持的无 body HTTP 方法: {:?}", method),
        };

        // 添加通用 headers
        self.add_headers_to_request(req)
    }

    // 带 Body 请求构建函数
    fn build_request_with_body(&self, method: HttpMethod, url: &str) -> RequestBuilder<WithBody> {
        // 根据 HTTP 方法调用对应的 Agent 方法
        let req = match method {
            HttpMethod::POST => self.agent.post(url),
            HttpMethod::PUT => self.agent.put(url),
            HttpMethod::PATCH => self.agent.patch(url),
            _ => panic!("不支持的有 body HTTP 方法: {:?}", method),
        };

        // 添加通用 headers
        self.add_headers_to_request(req)
    }

    // 通用 headers 添加函数
    fn add_headers_to_request<T>(&self, mut req: RequestBuilder<T>) -> RequestBuilder<T> {
        let headers = self.headers.lock().unwrap();
        for (key, value) in headers.iter() {
            req = req.header(key, value);
        }
        req
    }

    // 提取响应处理逻辑
    fn handle_response(&self, url: String, response: Response<Body>) -> Result<Value, UreqError> {
        if self.config.log_requests {
            self.log_request(&url, &response);
        }
        Ok(response.into_body().read_json()?)
    }

    // 发送请求的通用方法
    pub fn send_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
        base_url_key: Option<&str>,
    ) -> Result<Value, UreqError> {
        let url = self.build_url(endpoint, base_url_key);

        // 根据 HTTP 方法类型选择不同的请求构建方式
        match method {
            // 无 Body 请求
            HttpMethod::GET | HttpMethod::DELETE | HttpMethod::HEAD => {
                let mut req = self.build_request_without_body(method, &url);

                // 添加查询参数
                if let Some(params) = params {
                    for (key, value) in params {
                        req = req.query(key, value);
                    }
                }

                let response = req.call()?;
                self.handle_response(url, response)
            }
            // 带 Body 请求
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => {
                let mut req = self.build_request_with_body(method, &url);

                // 添加查询参数
                if let Some(params) = params {
                    for (key, value) in params {
                        req = req.query(key, value);
                    }
                }

                let response = if let Some(payload) = payload {
                    req.send_json(payload)
                } else {
                    req.send_empty()
                }?;

                self.handle_response(url, response)
            }
        }
    }

    // 带重试的请求
    pub fn send_request_with_retry(
        &self,
        method: HttpMethod,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
        base_url_key: Option<&str>,
    ) -> Result<Value, UreqError> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            match self.send_request(method, endpoint, params, payload, base_url_key) {
                Ok(response) => return Ok(response),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.config.max_retries - 1 {
                        std::thread::sleep(self.config.retry_delay * (2_u32.pow(attempt)));
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }

    // 从嵌套值中提取
    fn get_nested_value<'a>(&self, data: &'a Value, key: &str) -> Option<&'a Value> {
        if key.is_empty() {
            return None;
        }

        let keys: Vec<&str> = key.split('.').collect();
        let mut current = data;

        for k in keys {
            match current.get(k) {
                Some(v) => current = v,
                None => return None,
            }
        }

        Some(current)
    }

    // 安全提取总数
    fn safe_extract_total(&self, data: &Value, total_key: &str) -> usize {
        if let Some(total_raw) = self.get_nested_value(data, total_key) {
            if let Some(num) = total_raw.as_u64() {
                return num as usize;
            }
            if let Some(num) = total_raw.as_i64() {
                return num as usize;
            }
            if let Some(num) = total_raw.as_f64() {
                return num as usize;
            }
            if let Some(s) = total_raw.as_str() {
                if let Ok(num) = s.parse::<usize>() {
                    return num;
                }
            }
        }
        0
    }

    fn log_request(&self, url: &str, response: &Response<Body>) {
        println!("[Request] URL: {}, Status: {}", url, response.status());
    }

    fn build_url(&self, endpoint: &str, base_url_key: Option<&str>) -> String {
        if endpoint.starts_with("http") {
            endpoint.to_string()
        } else {
            let base_url = self.config.get_base_url(base_url_key);
            if base_url.ends_with('/') {
                format!("{}{}", base_url, endpoint.trim_start_matches('/'))
            } else if endpoint.starts_with('/') {
                format!("{}{}", base_url, endpoint)
            } else {
                format!("{}/{}", base_url, endpoint)
            }
        }
    }
}

// 分页迭代器
pub struct PaginatedIter<'a> {
    client: &'a HttpClient,
    method: HttpMethod,
    endpoint: String,
    base_params: HashMap<String, String>,
    payload: Option<Value>,
    limit: Option<usize>,
    total_key: String,
    data_key: String,
    pagination_method: PaginationMethod,
    config: PaginationConfig,
    base_url_key: Option<String>,

    // 内部状态
    total_items: usize,
    items_per_page: usize,
    first_page: Vec<Value>,
    yielded_count: usize,
    current_page: usize,
    current_page_data: Vec<Value>,
    current_index: usize,
    finished: bool,
    initialized: bool,
}

impl<'a> PaginatedIter<'a> {
    const DEFAULT_PAGE_SIZE: usize = 15;
    const MIN_PAGE_SIZE: usize = 1;

    pub fn new(client: &'a HttpClient, endpoint: impl Into<String>) -> Self {
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
            config: PaginationConfig::new(),
            base_url_key: None,

            total_items: 0,
            items_per_page: Self::DEFAULT_PAGE_SIZE,
            first_page: Vec::new(),
            yielded_count: 0,
            current_page: 0,
            current_page_data: Vec::new(),
            current_index: 0,
            finished: false,
            initialized: false,
        }
    }

    // 链式配置方法
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

    pub fn with_base_url(mut self, key: impl Into<String>) -> Self {
        self.base_url_key = Some(key.into());
        self
    }

    // 合并分页配置
    fn merge_config(&self) -> PaginationConfig {
        let mut merged = PaginationConfig::new();

        if let Some(amount_key) = &self.config.amount_key {
            merged.amount_key = Some(amount_key.clone());
        }
        if let Some(offset_key) = &self.config.offset_key {
            merged.offset_key = Some(offset_key.clone());
        }
        if let Some(response_amount_key) = &self.config.response_amount_key {
            merged.response_amount_key = Some(response_amount_key.clone());
        }
        if let Some(response_offset_key) = &self.config.response_offset_key {
            merged.response_offset_key = Some(response_offset_key.clone());
        }

        merged
    }

    // 准备分页参数
    fn prepare_pagination_params(&self, include_first_page: bool) -> HashMap<String, String> {
        let mut params = self.base_params.clone();
        let config = self.merge_config();

        if !include_first_page {
            if let Some(amount_key) = &config.amount_key {
                params.insert(amount_key.clone(), Self::DEFAULT_PAGE_SIZE.to_string());
            }
        }

        params
    }

    // 计算每页项目数
    fn calculate_items_per_page(
        &self,
        response_data: &Value,
        request_params: &HashMap<String, String>,
    ) -> usize {
        let config = self.merge_config();

        // 优先级: 请求参数 > 响应参数 > 默认值
        if let Some(amount_key) = &config.amount_key {
            if let Some(value) = request_params.get(amount_key) {
                if let Ok(num) = value.parse::<usize>() {
                    return num.max(Self::MIN_PAGE_SIZE);
                }
            }
        }

        if let Some(response_amount_key) = &config.response_amount_key {
            if let Some(value) = self
                .client
                .get_nested_value(response_data, response_amount_key)
            {
                if let Some(num) = value.as_u64() {
                    return (num as usize).max(Self::MIN_PAGE_SIZE);
                }
            }
        }

        Self::DEFAULT_PAGE_SIZE
    }

    // 提取第一页数据
    fn extract_first_page(&self, response_data: &Value, include_first_page: bool) -> Vec<Value> {
        if !include_first_page {
            return Vec::new();
        }

        if let Some(data) = self.client.get_nested_value(response_data, &self.data_key) {
            if let Some(array) = data.as_array() {
                return array.clone();
            }
        }

        Vec::new()
    }

    // 检查是否达到限制
    fn reached_limit(&self) -> bool {
        if let Some(limit) = self.limit {
            self.yielded_count >= limit
        } else {
            false
        }
    }

    // 计算剩余需要获取的项目数
    fn calculate_remaining_items(&self, first_page_count: usize) -> usize {
        let remaining_from_total = self.total_items.saturating_sub(first_page_count);

        if let Some(limit) = self.limit {
            remaining_from_total.min(limit.saturating_sub(self.yielded_count))
        } else {
            remaining_from_total
        }
    }

    // 构建页面请求参数
    fn build_page_params(&self, page_idx: usize, items_per_page: usize) -> HashMap<String, String> {
        let mut page_params = self.base_params.clone();
        let config = self.merge_config();

        if let Some(amount_key) = &config.amount_key {
            page_params.insert(amount_key.clone(), items_per_page.to_string());
        }

        if let Some(offset_key) = &config.offset_key {
            match self.pagination_method {
                PaginationMethod::Offset => {
                    let current_offset = self
                        .base_params
                        .get(offset_key)
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    page_params.insert(
                        offset_key.clone(),
                        (current_offset + (page_idx * items_per_page)).to_string(),
                    );
                }
                PaginationMethod::Page => {
                    page_params.insert(offset_key.clone(), (page_idx + 1).to_string());
                }
            }
        }

        page_params
    }

    // 获取单个页面的数据
    fn fetch_page_data(&self, params: &HashMap<String, String>) -> Result<Vec<Value>, UreqError> {
        let response = self.client.send_request(
            self.method,
            &self.endpoint,
            Some(params),
            self.payload.as_ref(),
            self.base_url_key.as_deref(),
        )?;

        if let Some(data) = self.client.get_nested_value(&response, &self.data_key) {
            if let Some(array) = data.as_array() {
                return Ok(array.clone());
            }
        }

        Ok(Vec::new())
    }

    // 初始化 - 获取第一页和总数信息
    fn initialize(&mut self) -> Result<(), UreqError> {
        if self.initialized {
            return Ok(());
        }

        let request_params = self.prepare_pagination_params(true);

        let response = self.client.send_request(
            self.method,
            &self.endpoint,
            Some(&request_params),
            self.payload.as_ref(),
            self.base_url_key.as_deref(),
        )?;

        // 提取总数
        self.total_items = self.client.safe_extract_total(&response, &self.total_key);

        // 计算每页项目数
        self.items_per_page = self.calculate_items_per_page(&response, &request_params);

        // 提取第一页数据
        self.first_page = self.extract_first_page(&response, true);
        self.current_page_data = self.first_page.clone();

        self.initialized = true;
        Ok(())
    }

    // 获取下一页
    fn fetch_next_page(&mut self) -> Result<(), UreqError> {
        let next_page = self.current_page + 1;
        let params = self.build_page_params(next_page, self.items_per_page);
        let page_data = self.fetch_page_data(&params)?;

        self.current_page_data = page_data;
        self.current_page = next_page;
        self.current_index = 0;

        Ok(())
    }
}

impl<'a> Iterator for PaginatedIter<'a> {
    type Item = Result<Value, UreqError>;

    fn next(&mut self) -> Option<Self::Item> {
        // 如果还没初始化，先初始化
        if !self.initialized {
            if let Err(e) = self.initialize() {
                return Some(Err(e));
            }
        }

        // 检查是否完成
        if self.finished || self.reached_limit() {
            return None;
        }

        // 如果当前页数据已用完，获取下一页
        if self.current_index >= self.current_page_data.len() {
            // 检查是否还有更多页
            let total_pages = (self.total_items + self.items_per_page - 1) / self.items_per_page;
            if self.current_page + 1 >= total_pages {
                self.finished = true;
                return None;
            }

            // 获取下一页
            if let Err(e) = self.fetch_next_page() {
                return Some(Err(e));
            }
        }

        // 返回当前页的下一个项目
        if self.current_index < self.current_page_data.len() {
            let item = self.current_page_data[self.current_index].clone();
            self.current_index += 1;
            self.yielded_count += 1;
            Some(Ok(item))
        } else {
            None
        }
    }
}

// 单例模式实现 CodeMaoClient
static CODEMAO_CLIENT_INSTANCE: OnceLock<Arc<Mutex<CodeMaoClient>>> = OnceLock::new();

#[derive(Clone)]
pub struct CodeMaoClient {
    http_client: Arc<HttpClient>,
    identity_manager: Arc<Mutex<IdentityManager>>,
}

impl CodeMaoClient {
    pub fn global() -> Arc<Mutex<Self>> {
        CODEMAO_CLIENT_INSTANCE
            .get_or_init(|| Arc::new(Mutex::new(Self::with_config(ClientConfig::default()))))
            .clone()
    }

    pub fn new() -> Self {
        Self::with_config(ClientConfig::default())
    }

    pub fn with_config(config: ClientConfig) -> Self {
        let http_client = Arc::new(HttpClient::new(config));
        let identity_manager = Arc::new(Mutex::new(IdentityManager::new()));

        Self {
            http_client,
            identity_manager,
        }
    }

    // 切换身份
    pub fn switch_identity(&self, identity: &str, token: &str) -> Result<(), String> {
        if token.trim().is_empty() {
            println!("警告: 尝试为身份 '{}' 设置空令牌", identity);
            return Ok(());
        }

        let valid_identities = ["average", "edu", "judgement", "blank"];
        if !valid_identities.contains(&identity) {
            return Err(format!(
                "无效的身份类型 '{}', 有效身份: {:?}",
                identity, valid_identities
            ));
        }

        // 使用身份管理器切换身份
        {
            let mut identity_manager = self.identity_manager.lock().unwrap();
            identity_manager.switch_identity(identity, token)?;
        }

        // 更新认证头
        self.update_auth_header();

        println!("已切换到身份: {}", identity);
        Ok(())
    }

    // 更新认证头
    fn update_auth_header(&self) {
        if let Some((key, value)) = self.identity_manager.lock().unwrap().get_auth_header() {
            let mut headers = HashMap::new();
            headers.insert(key, value);

            // Safe: using Mutex
            *self.http_client.headers.lock().unwrap() = headers;
        }
    }

    // 获取当前身份
    pub fn current_identity(&self) -> String {
        self.identity_manager
            .lock()
            .unwrap()
            .current_identity()
            .to_string()
    }

    // 获取认证头
    pub fn get_auth_header(&self) -> Option<(String, String)> {
        self.identity_manager.lock().unwrap().get_auth_header()
    }

    // 发送请求
    pub fn send_request(
        &self,
        method: HttpMethod,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
        base_url_key: Option<&str>,
    ) -> Result<Value, UreqError> {
        self.http_client
            .send_request(method, endpoint, params, payload, base_url_key)
    }

    // 带重试的请求
    pub fn send_request_with_retry(
        &self,
        method: HttpMethod,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        payload: Option<&Value>,
        base_url_key: Option<&str>,
    ) -> Result<Value, UreqError> {
        self.http_client
            .send_request_with_retry(method, endpoint, params, payload, base_url_key)
    }

    // 分页查询
    pub fn paginated(&self, endpoint: &str) -> PaginatedIter {
        PaginatedIter::new(&self.http_client, endpoint)
    }

    // 带 base_url 的分页查询
    pub fn paginated_with_base(&self, endpoint: &str, base_url_key: &str) -> PaginatedIter {
        PaginatedIter::new(&self.http_client, endpoint).with_base_url(base_url_key)
    }

    // 获取分页总数
    pub fn get_pagination_total(
        &self,
        endpoint: &str,
        params: &HashMap<String, String>,
        payload: Option<&Value>,
        total_key: &str,
        data_key: &str,
        config: Option<&PaginationConfig>,
        base_url_key: Option<&str>,
    ) -> Result<(usize, usize), UreqError> {
        let mut temp_iter = PaginatedIter::new(&self.http_client, endpoint)
            .with_params(params.clone())
            .with_total_key(total_key)
            .with_data_key(data_key);

        if let Some(payload) = payload {
            temp_iter = temp_iter.with_payload(payload.clone());
        }

        if let Some(config) = config {
            temp_iter = temp_iter.with_config(config.clone());
        }

        if let Some(base_url_key) = base_url_key {
            temp_iter = temp_iter.with_base_url(base_url_key);
        }

        // 初始化但不获取第一页数据
        temp_iter.initialize()?;

        let total_pages =
            (temp_iter.total_items + temp_iter.items_per_page - 1) / temp_iter.items_per_page;
        Ok((temp_iter.total_items, total_pages))
    }
}

impl Default for CodeMaoClient {
    fn default() -> Self {
        Self::new()
    }
}

// 文件上传器
pub struct FileUploader {
    client: Arc<CodeMaoClient>,
    upload_agent: Agent,
}

impl FileUploader {
    pub fn new() -> Self {
        Self {
            client: Arc::new(CodeMaoClient::global().lock().unwrap().clone()),
            upload_agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(120)))
                .build()
                .into(),
        }
    }

    pub fn with_client(client: Arc<CodeMaoClient>) -> Self {
        Self {
            client,
            upload_agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(120)))
                .build()
                .into(),
        }
    }

    // 生成随机 ID
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

    // 上传文件
    pub fn upload(
        &self,
        file_path: &Path,
        method: &str,
        save_path: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        match method {
            "pgaot" => self.upload_pgaot(file_path, save_path),
            "codegame" => self.upload_codegame(file_path, save_path),
            "codemao" => self.upload_codemao(file_path, save_path),
            _ => Err(format!("不支持的上传方式: {}", method).into()),
        }
    }

    // Pgaot 上传
    fn upload_pgaot(
        &self,
        file_path: &Path,
        save_path: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let form = Form::new()
            .text("path", save_path)
            .file("file", file_path)?; // 注意：这里需要文件路径

        // 发送 multipart 表单
        let response = self
            .upload_agent
            .post("https://api.pgaot.com/user/up_cat_file")
            .send(form)?;
        let json: Value = response.into_body().read_json()?;
        Ok(json["url"].as_str().unwrap_or("").to_string())
    }

    // CodeGame 上传
    fn upload_codegame(
        &self,
        file_path: &Path,
        save_path: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let token_info = self.get_codegame_token(save_path, file_path)?;
        let form = Form::new()
            .text("token", &token_info.token)
            .text("key", &token_info.file_path)
            .text("fname", "avatar");
        let response = self.upload_agent.post(&token_info.upload_url).send(form)?;
        let json: Value = response.into_body().read_json()?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}/{}", token_info.pic_host, key))
    }

    // CodeMao 上传
    fn upload_codemao(
        &self,
        file_path: &Path,
        save_path: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let unique_filename = format!(
            "{}{}",
            Self::generate_id(4),
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
            .file("file", file_path)?; // 直接使用文件路径

        let response = self.upload_agent.post(&token_info.upload_url).send(form)?;

        let json: Value = response.into_body().read_json()?;
        let key = json["key"].as_str().unwrap_or("");
        Ok(format!("{}{}", token_info.bucket_url, key))
    }

    // 获取 CodeMao 上传 token
    fn get_codemao_token(
        &self,
        file_path: &str,
    ) -> Result<CodeMaoTokenInfo, Box<dyn std::error::Error>> {
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

        let tokens = response["tokens"]
            .as_array()
            .ok_or("无法获取 tokens 数组")?;

        let token_info = tokens.get(0).ok_or("无法获取第一个 token")?;

        Ok(CodeMaoTokenInfo {
            token: token_info["token"].as_str().unwrap_or("").to_string(),
            file_path: token_info["file_path"].as_str().unwrap_or("").to_string(),
            upload_url: response["upload_url"].as_str().unwrap_or("").to_string(),
            bucket_url: response["bucket_url"].as_str().unwrap_or("").to_string(),
        })
    }

    // 获取 CodeGame 上传 token
    fn get_codegame_token(
        &self,
        prefix: &str,
        file_path: &Path,
    ) -> Result<CodeGameTokenInfo, Box<dyn std::error::Error>> {
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

        let data = response["data"].as_array().ok_or("无法获取 data 数组")?;

        let token_data = data.get(0).ok_or("无法获取第一个数据项")?;

        Ok(CodeGameTokenInfo {
            token: token_data["token"].as_str().unwrap_or("").to_string(),
            file_path: token_data["filename"].as_str().unwrap_or("").to_string(),
            pic_host: response["bucket_url"].as_str().unwrap_or("").to_string(),
            upload_url: "https://upload.qiniup.com".to_string(),
        })
    }
}

impl Default for FileUploader {
    fn default() -> Self {
        Self::new()
    }
}

// Token 信息结构
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

// 客户端工厂
pub struct ClientFactory;

impl ClientFactory {
    pub fn create_http_client(config: Option<ClientConfig>) -> HttpClient {
        HttpClient::new(config.unwrap_or_default())
    }

    pub fn create_codemao_client() -> CodeMaoClient {
        CodeMaoClient::new()
    }

    pub fn create_codemao_client_with_config(config: ClientConfig) -> CodeMaoClient {
        CodeMaoClient::with_config(config)
    }

    pub fn create_file_uploader() -> FileUploader {
        FileUploader::new()
    }
}
