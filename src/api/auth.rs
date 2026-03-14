use crate::utils::acquire::{BaseKey, CodeMaoClient, HttpMethod, Identity};
use crate::utils::data::{CodeMaoFile, FileContent, PathConfig};
use rand::RngExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use ureq::http::HeaderValue;

// ==================== 枚举定义 ====================

// 登录方法枚举
#[derive(Debug, Clone, Copy)]
pub enum LoginMethod {
    PasswordV0,
    PasswordV1,
    PasswordV2,
    Token,
    AdminToken,
    AdminPassword,
}

impl LoginMethod {
    fn as_str(&self) -> &'static str {
        match self {
            LoginMethod::PasswordV0 => "password_v0",
            LoginMethod::PasswordV1 => "password_v1",
            LoginMethod::PasswordV2 => "password_v2",
            LoginMethod::Token => "token",
            LoginMethod::AdminToken => "admin_token",
            LoginMethod::AdminPassword => "admin_password",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "password_v0" => Some(LoginMethod::PasswordV0),
            "password_v1" => Some(LoginMethod::PasswordV1),
            "password_v2" => Some(LoginMethod::PasswordV2),
            "token" => Some(LoginMethod::Token),
            "admin_token" => Some(LoginMethod::AdminToken),
            "admin_password" => Some(LoginMethod::AdminPassword),
            _ => None,
        }
    }
}

// 用户角色枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    User,
    Admin,
}

impl UserRole {
    fn as_str(&self) -> &'static str {
        match self {
            UserRole::User => "user",
            UserRole::Admin => "admin",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(UserRole::User),
            "admin" => Some(UserRole::Admin),
            _ => None,
        }
    }
}

// 账号状态枚举 - 映射到 Identity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Judgement,
    Average,
    Edu,
}

impl AccountStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AccountStatus::Judgement => "judgement",
            AccountStatus::Average => "average",
            AccountStatus::Edu => "edu",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "judgement" => Some(AccountStatus::Judgement),
            "average" => Some(AccountStatus::Average),
            "edu" => Some(AccountStatus::Edu),
            _ => None,
        }
    }

    // 转换为 Identity
    pub fn to_identity(&self) -> Identity {
        match self {
            AccountStatus::Judgement => Identity::Judgement,
            AccountStatus::Average => Identity::Average,
            AccountStatus::Edu => Identity::Edu,
        }
    }
}

// ==================== 数据结构 ====================

// 登录凭证
#[derive(Debug, Clone)]
pub struct LoginCredentials {
    pub identity: String,
    pub password: String,
    pub token: String,
    pub pid: String,
    pub status: AccountStatus,
    pub role: UserRole,
}

impl Default for LoginCredentials {
    fn default() -> Self {
        Self {
            identity: String::new(),
            password: String::new(),
            token: String::new(),
            pid: "65edCTyg".to_string(),
            status: AccountStatus::Average,
            role: UserRole::User,
        }
    }
}

// 登录结果
#[derive(Debug, Clone)]
pub struct LoginResult {
    pub success: bool,
    pub method: LoginMethod,
    pub message: String,
    pub token: String,
    pub data: Value,
    pub auth_details: Option<Value>,
}

impl LoginResult {
    fn new(success: bool, method: LoginMethod, message: &str) -> Self {
        Self {
            success,
            method,
            message: message.to_string(),
            token: String::new(),
            data: Value::Null,
            auth_details: None,
        }
    }

    fn with_token(mut self, token: &str) -> Self {
        self.token = token.to_string();
        self
    }

    fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    fn with_auth_details(mut self, details: Value) -> Self {
        self.auth_details = Some(details);
        self
    }
}

// ==================== 客户端提供者特质 ====================

/// 客户端提供者特质，用于依赖注入
pub trait ClientProvider: Send + Sync + std::fmt::Debug {
    /// 获取客户端实例
    fn client(&self) -> &CodeMaoClient;

    /// 克隆客户端提供者
    fn clone_box(&self) -> Box<dyn ClientProvider>;
}

impl Clone for Box<dyn ClientProvider> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// 全局客户端提供者（默认实现）
#[derive(Debug, Clone)]
pub struct GlobalClientProvider;

impl GlobalClientProvider {
    pub fn new() -> Self {
        Self
    }
}

impl ClientProvider for GlobalClientProvider {
    fn client(&self) -> &CodeMaoClient {
        CodeMaoClient::global()
    }

    fn clone_box(&self) -> Box<dyn ClientProvider> {
        Box::new(self.clone())
    }
}

impl Default for GlobalClientProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 全局单例 ====================

// 全局 AuthManager 单例（使用默认的全局客户端提供者）
static GLOBAL_AUTH_MANAGER: OnceLock<Arc<AuthManager>> = OnceLock::new();

// 获取全局 AuthManager 实例
pub fn global_auth_manager() -> Arc<AuthManager> {
    GLOBAL_AUTH_MANAGER
        .get_or_init(|| Arc::new(AuthManager::new()))
        .clone()
}

// 初始化全局 AuthManager（带配置）
pub fn init_global_auth_manager() -> Arc<AuthManager> {
    global_auth_manager() // 直接返回已初始化的实例
}

// ==================== 辅助函数 ====================

// 获取当前服务器时间戳（接受客户端提供者）
pub fn fetch_current_timestamp_with_provider(
    provider: &dyn ClientProvider,
) -> Result<i64, Box<dyn std::error::Error>> {
    let client = provider.client();
    let response = client
        .build_request(HttpMethod::GET, "/coconut/clouddb/currentTime", None)
        .send()?;
    let json = client.response_to_json(response)?;
    Ok(json["data"].as_i64().unwrap_or(0))
}

// 获取当前服务器时间戳（使用默认全局客户端）
pub fn fetch_current_timestamp() -> Result<i64, Box<dyn std::error::Error>> {
    fetch_current_timestamp_with_provider(&GlobalClientProvider::new())
}

// 确定用户登录方法
fn determine_user_login_method(
    token: Option<&str>,
    identity: Option<&str>,
    password: Option<&str>,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    if token.is_some() {
        return Ok("token");
    }
    if identity.is_some() && password.is_some() {
        return Ok("password_v2");
    }
    Err("缺少必要的登录凭据".into())
}

// 确定管理员登录方法
fn determine_admin_login_method(
    token: Option<&str>,
    identity: Option<&str>,
    password: Option<&str>,
) -> Result<&'static str, Box<dyn std::error::Error>> {
    if token.is_some() {
        return Ok("admin_token");
    }
    if identity.is_some() || password.is_some() {
        return Ok("admin_password");
    }
    Err("缺少必要的管理员登录凭据".into())
}

// ==================== 认证处理器 ====================

#[derive(Clone)]
pub struct AuthProcessor {
    client_provider: Box<dyn ClientProvider>,
    client_secret: &'static str,
}

impl AuthProcessor {
    const CLIENT_SECRET: &'static str = "pBlYqXbJDu";

    /// 创建新的认证处理器（使用指定的客户端提供者）
    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        Self {
            client_provider: provider,
            client_secret: Self::CLIENT_SECRET,
        }
    }

    /// 创建新的认证处理器（使用默认的全局客户端）
    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    // 获取客户端
    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    // 获取认证详情
    pub fn fetch_auth_details(&self, token: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        // 先切换到 blank 身份发送请求，然后手动添加 cookie
        let cookie_str = format!("authorization={}", token);

        // 通过 agent 发送自定义请求
        let response = client
            .agent()
            .get("https://api.codemao.cn/web/users/details")
            .header("Cookie", &cookie_str)
            .call()?;

        // 处理 cookies
        let headers = response.headers();
        let set_cookie_headers = headers.get_all("set-cookie");

        let cookies: Vec<String> = set_cookie_headers
            .iter()
            .map(|header: &HeaderValue| header.to_str().unwrap_or("").to_string())
            .collect();

        // 如果需要，可以存储 cookies 供后续使用
        for cookie in &cookies {
            println!("Received cookie: {}", cookie);
        }

        let json = client.response_to_json(response)?;
        Ok(json)
    }

    // 获取登录票据
    pub fn get_login_ticket(
        &self,
        identity: &str,
        timestamp: i64,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        let payload = json!({
            "identity": identity,
            "pid": pid,
            "timestamp": timestamp,
        });

        let response = client
            .build_request(
                HttpMethod::POST,
                "https://open-service.codemao.cn/captcha/rule/v3",
                None,
            )
            .with_payload(payload)
            .send()?;
        Ok(client.response_to_json(response)?)
    }

    pub fn get_login_security_info(
        &self,
        identity: &str,
        password: &str,
        ticket: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
            "agreement_ids": [-1],
        });

        // 由于 CodeMaoClient 的 send_request 不支持自定义头，需要使用 agent
        let response = client
            .agent()
            .post("https://api.codemao.cn/tiger/v3/web/accounts/login/security")
            .header("x-captcha-ticket", ticket)
            .send_json(&payload)?;

        // 检查状态码
        let status = response.status();

        if status != 200 {
            let body = response.into_body().read_to_string().unwrap_or_default();
            return Err(format!("API返回错误状态码: {}, Body: {}", status, body).into());
        }

        // 读取响应体
        let body = match response.into_body().read_to_string() {
            Ok(b) => b,
            Err(e) => {
                println!("读取响应体失败: {}", e);
                return Err(Box::new(e));
            }
        };

        // 解析JSON
        let json_value: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                println!("JSON解析失败: {}", e);
                return Err(Box::new(e));
            }
        };

        Ok(json_value)
    }

    // 管理员用户认证
    pub fn authenticate_admin_user(
        &self,
        username: &str,
        password: &str,
        key: i64,
        code: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        let payload = json!({
            "username": username,
            "password": password,
            "key": key,
            "code": code,
        });

        let response = client
            .build_request(HttpMethod::POST, "/admins/login", Some(BaseKey::Whale))
            .with_payload(payload)
            .send()?;
        Ok(client.response_to_json(response)?)
    }

    // 获取管理员验证码
    pub fn fetch_admin_captcha(
        &self,
        timestamp: i64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let client = self.client();

        let endpoint = format!("/admins/captcha/{}", timestamp);

        let response = client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Whale))
            .send()?;

        if response.status() == 200 {
            let bytes = response.into_body().read_to_vec()?;
            CodeMaoFile::file_write(
                &PathConfig::captcha_file_path(),
                &FileContent::Bytes(bytes.clone()),
                "b",
            )?;
            println!(
                "验证码已保存至: {:?}",
                &PathConfig::captcha_file_path().to_str()
            );
            Ok(bytes)
        } else {
            println!("获取验证码失败, 错误代码: {}", response.status());
            Ok(Vec::new())
        }
    }

    // 处理 v0 版本密码登录
    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
        });

        let response = client
            .build_request(HttpMethod::POST, "/tiger/accounts/login", None)
            .with_payload(payload)
            .send()?;
        Ok(client.response_to_json(response)?)
    }

    // 处理 v1 版本密码登录
    pub fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
        });

        let response = client
            .build_request(HttpMethod::POST, "/tiger/v3/web/accounts/login", None)
            .with_payload(payload)
            .send()?;
        Ok(client.response_to_json(response)?)
    }

    // 处理 v2 版本密码登录
    pub fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = self.fetch_current_timestamp()?;
        let ticket_response = self.get_login_ticket(identity, timestamp, pid)?;
        println!("Ticket response: {:?}", ticket_response);

        let ticket = ticket_response["ticket"].as_str().ok_or("无法获取ticket")?;

        // 调用第三个 API 并打印完整返回数据
        let security_response = self.get_login_security_info(identity, password, ticket, pid)?;
        println!(
            "Security API response: {}",
            serde_json::to_string_pretty(&security_response)?
        );

        Ok(security_response)
    }

    // 获取当前时间戳（使用当前客户端）
    fn fetch_current_timestamp(&self) -> Result<i64, Box<dyn std::error::Error>> {
        fetch_current_timestamp_with_provider(&*self.client_provider)
    }
}

impl Default for AuthProcessor {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 登录处理器 ====================

pub struct LoginHandler {
    processor: AuthProcessor,
}

impl LoginHandler {
    /// 创建新的登录处理器（使用指定的客户端提供者）
    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        Self {
            processor: AuthProcessor::new_with_provider(provider),
        }
    }

    /// 创建新的登录处理器（使用默认的全局客户端）
    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    // 获取客户端
    fn client(&self) -> &CodeMaoClient {
        self.processor.client()
    }

    // 处理 v0 版本密码登录
    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();

        // 切换到 blank 身份
        let _ = client.switch_identity(Identity::Blank);

        match self.processor.handle_password_v0(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data.get("token").and_then(|t| t.as_str()) {
                    // 设置令牌并切换到对应身份
                    client.set_token(status.to_identity(), token)?;
                    let _ = client.switch_identity(status.to_identity());

                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV0, "v0 密码登录成功")
                            .with_token(token)
                            .with_data(data),
                    )
                } else {
                    Ok(
                        LoginResult::new(false, LoginMethod::PasswordV0, "v0 密码登录失败")
                            .with_data(data),
                    )
                }
            }
            Err(e) => Ok(LoginResult::new(
                false,
                LoginMethod::PasswordV0,
                &format!("登录失败: {}", e),
            )),
        }
    }

    // 处理 v1 版本密码登录
    pub fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();

        // 切换到 blank 身份
        let _ = client.switch_identity(Identity::Blank);

        match self.processor.handle_password_v1(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    // 设置令牌并切换到对应身份
                    client.set_token(status.to_identity(), token)?;
                    let _ = client.switch_identity(status.to_identity());

                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV1, "v1 密码登录成功")
                            .with_token(token)
                            .with_data(data),
                    )
                } else {
                    Ok(
                        LoginResult::new(false, LoginMethod::PasswordV1, "v1 密码登录失败")
                            .with_data(data),
                    )
                }
            }
            Err(e) => Ok(LoginResult::new(
                false,
                LoginMethod::PasswordV1,
                &format!("登录失败: {}", e),
            )),
        }
    }

    // 处理 v2 版本密码登录
    pub fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();

        // 切换到 blank 身份
        let _ = client.switch_identity(Identity::Blank);

        match self.processor.handle_password_v2(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    // 设置令牌并切换到对应身份
                    client.set_token(status.to_identity(), token)?;
                    let _ = client.switch_identity(status.to_identity());

                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV2, "v2 密码登录成功")
                            .with_token(token)
                            .with_data(data),
                    )
                } else {
                    Ok(
                        LoginResult::new(false, LoginMethod::PasswordV2, "v2 密码登录失败")
                            .with_data(data),
                    )
                }
            }
            Err(e) => {
                // 返回真正的错误，而不是 Ok
                return Err(format!("password_v2 登录失败: {}", e).into());
            }
        }
    }

    // 处理 token 登录
    pub fn handle_token(
        &self,
        token: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();

        let auth_details = self.processor.fetch_auth_details(token)?;

        // 设置令牌并切换到对应身份
        client.set_token(status.to_identity(), token)?;
        let _ = client.switch_identity(status.to_identity());

        Ok(LoginResult::new(true, LoginMethod::Token, "Token 登录成功")
            .with_token(token)
            .with_auth_details(auth_details))
    }

    // 处理管理员 token 登录
    pub fn handle_admin_token(
        &self,
        token: Option<&str>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();

        let token = match token {
            Some(t) => t.to_string(),
            None => {
                println!("请输入 Authorization Token:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };

        // 设置令牌并切换到 Judgement 身份
        client.set_token(Identity::Judgement, &token)?;
        let _ = client.switch_identity(Identity::Judgement);

        Ok(
            LoginResult::new(true, LoginMethod::AdminToken, "管理员 Token 登录成功")
                .with_token(&token),
        )
    }

    // 处理管理员密码登录
    pub fn handle_admin_password(
        &self,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();

        let mut username = match username {
            Some(u) => u.to_string(),
            None => {
                println!("请输入用户名:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };

        let mut password = match password {
            Some(p) => p.to_string(),
            None => {
                println!("请输入密码:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };

        loop {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            println!("正在获取验证码...");
            self.processor.fetch_admin_captcha(timestamp)?;

            println!("请输入验证码:");
            let mut captcha = String::new();
            std::io::stdin().read_line(&mut captcha)?;
            let captcha = captcha.trim();

            match self
                .processor
                .authenticate_admin_user(&username, &password, timestamp, captcha)
            {
                Ok(response) => {
                    if let Some(token) = response.get("token").and_then(|t| t.as_str()) {
                        // 设置令牌并切换到 Judgement 身份
                        client.set_token(Identity::Judgement, token)?;
                        let _ = client.switch_identity(Identity::Judgement);

                        return Ok(LoginResult::new(
                            true,
                            LoginMethod::AdminPassword,
                            "管理员账密登录成功",
                        )
                        .with_token(token));
                    }

                    let error_msg = response
                        .get("error_msg")
                        .and_then(|e| e.as_str())
                        .unwrap_or("未知错误");
                    println!("登录失败: {}", error_msg);

                    if let Some(error_code) = response.get("error_code").and_then(|e| e.as_str()) {
                        if error_code == "Admin-Password-Error@Community-Admin"
                            || error_code == "Param-Invalid@Common"
                        {
                            println!("请输入用户名:");
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            username = input.trim().to_string();

                            println!("请输入密码:");
                            let mut input = String::new();
                            std::io::stdin().read_line(&mut input)?;
                            password = input.trim().to_string();
                        }
                    }
                }
                Err(e) => {
                    println!("认证请求失败: {}", e);
                }
            }
        }
    }
}

impl Default for LoginHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 认证管理器 ====================

pub struct AuthManager {
    client_provider: Box<dyn ClientProvider>,
    processor: AuthProcessor,
    handler: LoginHandler,
    current_credentials: Option<LoginCredentials>,
}

impl AuthManager {
    /// 创建新的认证管理器（使用指定的客户端提供者）
    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        let processor = AuthProcessor::new_with_provider(provider.clone_box());
        let handler = LoginHandler::new_with_provider(provider.clone_box());

        Self {
            client_provider: provider,
            processor,
            handler,
            current_credentials: None,
        }
    }

    /// 创建新的认证管理器（使用默认的全局客户端）
    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    // 获取客户端
    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    // 统一的登录接口
    pub fn login(
        &mut self,
        identity: Option<&str>,
        password: Option<&str>,
        token: Option<&str>,
        pid: Option<&str>,
        status: Option<&str>,
        role: Option<&str>,
        prefer_method: Option<&str>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        self.validate_login_parameters(identity, password, token, role, prefer_method)?;

        let credentials = LoginCredentials {
            identity: identity.unwrap_or("").to_string(),
            password: password.unwrap_or("").to_string(),
            token: token.unwrap_or("").to_string(),
            pid: pid.unwrap_or("65edCTyg").to_string(),
            status: AccountStatus::from_str(status.unwrap_or("average"))
                .unwrap_or(AccountStatus::Average),
            role: UserRole::from_str(role.unwrap_or("user")).unwrap_or(UserRole::User),
        };

        let role = credentials.role;
        self.current_credentials = Some(credentials);

        match role {
            UserRole::Admin => {
                self.admin_login(self.current_credentials.as_ref().unwrap(), prefer_method)
            }
            UserRole::User => {
                self.user_login(self.current_credentials.as_ref().unwrap(), prefer_method)
            }
        }
    }

    fn validate_login_parameters(
        &self,
        identity: Option<&str>,
        password: Option<&str>,
        token: Option<&str>,
        role: Option<&str>,
        prefer_method: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(method) = prefer_method {
            let user_methods = ["password_v0", "password_v1", "password_v2", "token"];
            let admin_methods = ["admin_token", "admin_password"];

            match role.unwrap_or("user") {
                "user" if !user_methods.contains(&method) => {
                    return Err(format!(
                        "用户角色不支持登录方法 '{}', 可用方法: {:?}",
                        method, user_methods
                    )
                    .into());
                }
                "admin" if !admin_methods.contains(&method) => {
                    return Err(format!(
                        "管理员角色不支持登录方法 '{}', 可用方法: {:?}",
                        method, admin_methods
                    )
                    .into());
                }
                _ => {}
            }

            match method {
                "password_v0" | "password_v1" | "password_v2" | "admin_password"
                    if identity.is_none() || password.is_none() =>
                {
                    return Err(format!(
                        "登录方法 '{}' 需要提供 identity 和 password 参数",
                        method
                    )
                    .into());
                }
                "token" | "admin_token" if token.is_none() => {
                    return Err(format!("登录方法 '{}' 需要提供 token 参数", method).into());
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn get_user_login_method(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(method) = prefer_method {
            if ["password_v0", "password_v1", "password_v2", "token"].contains(&method) {
                return Ok(method.to_string());
            }
            return Err(format!("'{}' 不是有效的用户登录方法", method).into());
        }

        determine_user_login_method(
            if credentials.token.is_empty() {
                None
            } else {
                Some(&credentials.token)
            },
            if credentials.identity.is_empty() {
                None
            } else {
                Some(&credentials.identity)
            },
            if credentials.password.is_empty() {
                None
            } else {
                Some(&credentials.password)
            },
        )
        .map(|s| s.to_string())
    }

    fn get_admin_login_method(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<&str>,
    ) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(method) = prefer_method {
            if ["admin_token", "admin_password"].contains(&method) {
                return Ok(method.to_string());
            }
            return Err(format!("'{}' 不是有效的管理员登录方法", method).into());
        }

        determine_admin_login_method(
            if credentials.token.is_empty() {
                None
            } else {
                Some(&credentials.token)
            },
            if credentials.identity.is_empty() {
                None
            } else {
                Some(&credentials.identity)
            },
            if credentials.password.is_empty() {
                None
            } else {
                Some(&credentials.password)
            },
        )
        .map(|s| s.to_string())
    }

    fn user_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<&str>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let method = self.get_user_login_method(credentials, prefer_method)?;

        match method.as_str() {
            "password_v0" => self.handler.handle_password_v0(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                credentials.status,
            ),
            "password_v1" => self.handler.handle_password_v1(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                credentials.status,
            ),
            "password_v2" => self.handler.handle_password_v2(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                credentials.status,
            ),
            "token" => self
                .handler
                .handle_token(&credentials.token, credentials.status),
            _ => Err(format!("不支持的登录方式: {}", method).into()),
        }
    }

    fn admin_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<&str>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let method = self.get_admin_login_method(credentials, prefer_method)?;

        match method.as_str() {
            "admin_token" => self.handler.handle_admin_token(Some(&credentials.token)),
            "admin_password" => self
                .handler
                .handle_admin_password(Some(&credentials.identity), Some(&credentials.password)),
            _ => Err(format!("不支持的管理员登录方式: {}", method).into()),
        }
    }

    // 执行 v0 版本用户登出
    pub fn execute_logout_v0(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let client = self.client();

        let response = client
            .build_request(HttpMethod::POST, "/tiger/accounts/logout", None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == 204)
    }

    // 执行 v12 版本用户登出
    pub fn execute_logout_v12(&self, method: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let client = self.client();

        let endpoint = format!("/tiger/v3/{}/accounts/logout", method);
        let response = client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == 204)
    }

    // 管理员登出
    pub fn admin_logout(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let client = self.client();

        let response = client
            .build_request(HttpMethod::DELETE, "/admins/logout", Some(BaseKey::Whale))
            .send()?;
        Ok(response.status() == 204)
    }

    // 获取管理员仪表板数据
    pub fn fetch_admin_dashboard_data(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();

        let response = client
            .build_request(HttpMethod::GET, "/admins/info", Some(BaseKey::Whale))
            .send()?;
        Ok(client.response_to_json(response)?)
    }

    // 配置认证 Token
    pub fn configure_authentication_token(
        &self,
        token: &str,
        status: AccountStatus,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let client = self.client();

        client.set_token(status.to_identity(), token)?;
        let _ = client.switch_identity(status.to_identity());
        Ok(())
    }

    // 获取当前登录凭证
    pub fn get_current_credentials(&self) -> Option<&LoginCredentials> {
        self.current_credentials.as_ref()
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 云服务认证器 ====================

pub struct CloudAuthenticator {
    client_provider: Box<dyn ClientProvider>,
    authorization_token: Option<String>,
    client_id: String,
    time_difference: i64,
    client_secret: &'static str,
}

impl CloudAuthenticator {
    const CLIENT_SECRET: &'static str = "pBlYqXbJDu";

    /// 创建新的云服务认证器（使用指定的客户端提供者）
    pub fn new_with_provider(
        provider: Box<dyn ClientProvider>,
        authorization_token: Option<String>,
    ) -> Self {
        let client_id = Self::generate_client_id(8);

        Self {
            client_provider: provider,
            authorization_token,
            client_id,
            time_difference: 0,
            client_secret: Self::CLIENT_SECRET,
        }
    }

    /// 创建新的云服务认证器（使用默认的全局客户端）
    pub fn new(authorization_token: Option<String>) -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()), authorization_token)
    }

    // 获取客户端
    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    fn generate_client_id(length: usize) -> String {
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        let mut rng = rand::rng();

        (0..length)
            .map(|_| {
                let idx = rng.random_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    // 获取校准后的时间戳
    pub fn get_calibrated_timestamp(&mut self) -> Result<i64, Box<dyn std::error::Error>> {
        if self.time_difference == 0 {
            let server_time = fetch_current_timestamp_with_provider(&*self.client_provider)?;
            let local_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            self.time_difference = local_time - server_time;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        Ok(now - self.time_difference)
    }

    // 生成设备认证信息
    pub fn generate_x_device_auth(&mut self) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = self.get_calibrated_timestamp()?;
        let sign_str = format!("{}{}{}", self.client_secret, timestamp, self.client_id);
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let sign = format!("{:X}", hasher.finalize());

        Ok(json!({
            "sign": sign,
            "timestamp": timestamp,
            "client_id": self.client_id,
        }))
    }
}

// ==================== 便捷函数 ====================

/// 使用默认全局客户端执行登录
pub fn login(
    identity: Option<&str>,
    password: Option<&str>,
    token: Option<&str>,
    pid: Option<&str>,
    status: Option<&str>,
    role: Option<&str>,
    prefer_method: Option<&str>,
) -> Result<LoginResult, Box<dyn std::error::Error>> {
    let mut auth_manager = AuthManager::new();
    auth_manager.login(identity, password, token, pid, status, role, prefer_method)
}
