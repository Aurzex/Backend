use crate::utils::acquire::{BaseKey, CodeMaoClient, HttpMethod, Identity};
use crate::utils::data::{CodeMaoFile, FileContent, PathConfig};
use rand::RngExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use ureq::http::HeaderValue;

// ==================== 枚举定义 ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    PasswordV0,
    PasswordV1,
    PasswordV2,
    Token,
    AdminToken,
    AdminPassword,
}

impl LoginMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            LoginMethod::PasswordV0 => "password_v0",
            LoginMethod::PasswordV1 => "password_v1",
            LoginMethod::PasswordV2 => "password_v2",
            LoginMethod::Token => "token",
            LoginMethod::AdminToken => "admin_token",
            LoginMethod::AdminPassword => "admin_password",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
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

    /// 判断该登录方法是否属于用户
    pub fn is_user_method(&self) -> bool {
        matches!(
            self,
            LoginMethod::PasswordV0
                | LoginMethod::PasswordV1
                | LoginMethod::PasswordV2
                | LoginMethod::Token
        )
    }

    /// 判断该登录方法是否属于管理员
    pub fn is_admin_method(&self) -> bool {
        matches!(self, LoginMethod::AdminToken | LoginMethod::AdminPassword)
    }
}

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

    pub fn to_identity(&self) -> Identity {
        match self {
            AccountStatus::Judgement => Identity::Judgement,
            AccountStatus::Average => Identity::Average,
            AccountStatus::Edu => Identity::Edu,
        }
    }
}

// ==================== 数据结构 ====================

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

pub trait ClientProvider: Send + Sync + std::fmt::Debug {
    fn client(&self) -> &CodeMaoClient;
    fn clone_box(&self) -> Box<dyn ClientProvider>;
}

impl Clone for Box<dyn ClientProvider> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

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

static GLOBAL_AUTH_MANAGER: OnceLock<Arc<AuthManager>> = OnceLock::new();

pub fn global_auth_manager() -> Arc<AuthManager> {
    GLOBAL_AUTH_MANAGER
        .get_or_init(|| Arc::new(AuthManager::new()))
        .clone()
}

pub fn init_global_auth_manager() -> Arc<AuthManager> {
    global_auth_manager()
}

// ==================== 辅助函数 ====================

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

pub fn fetch_current_timestamp() -> Result<i64, Box<dyn std::error::Error>> {
    fetch_current_timestamp_with_provider(&GlobalClientProvider::new())
}

fn determine_user_login_method(
    token: Option<&str>,
    identity: Option<&str>,
    password: Option<&str>,
) -> Result<LoginMethod, Box<dyn std::error::Error>> {
    if token.is_some() {
        return Ok(LoginMethod::Token);
    }
    if identity.is_some() && password.is_some() {
        return Ok(LoginMethod::PasswordV2);
    }
    Err("缺少必要的登录凭据".into())
}

fn determine_admin_login_method(
    token: Option<&str>,
    identity: Option<&str>,
    password: Option<&str>,
) -> Result<LoginMethod, Box<dyn std::error::Error>> {
    if token.is_some() {
        return Ok(LoginMethod::AdminToken);
    }
    if identity.is_some() || password.is_some() {
        return Ok(LoginMethod::AdminPassword);
    }
    Err("缺少必要的管理员登录凭据".into())
}

// ==================== 认证处理器 ====================

#[derive(Clone, Debug)]
pub struct AuthProcessor {
    client_provider: Box<dyn ClientProvider>,
    client_secret: &'static str,
}

impl AuthProcessor {
    const CLIENT_SECRET: &'static str = "pBlYqXbJDu";

    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        Self {
            client_provider: provider,
            client_secret: Self::CLIENT_SECRET,
        }
    }

    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    pub fn fetch_auth_details(&self, token: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();
        let cookie_str = format!("authorization={}", token);

        let response = client
            .agent()
            .get("https://api.codemao.cn/web/users/details")
            .header("Cookie", &cookie_str)
            .call()?;

        let headers = response.headers();
        let set_cookie_headers = headers.get_all("set-cookie");
        let cookies: Vec<String> = set_cookie_headers
            .iter()
            .map(|header: &HeaderValue| header.to_str().unwrap_or("").to_string())
            .collect();

        for cookie in &cookies {
            println!("Received cookie: {}", cookie);
        }

        let json = client.response_to_json(response)?;
        Ok(json)
    }

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

        let response = client
            .agent()
            .post("https://api.codemao.cn/tiger/v3/web/accounts/login/security")
            .header("x-captcha-ticket", ticket)
            .send_json(&payload)?;

        let status = response.status();
        if status != 200 {
            let body = response.into_body().read_to_string().unwrap_or_default();
            return Err(format!("API返回错误状态码: {}, Body: {}", status, body).into());
        }

        let body = response.into_body().read_to_string()?;
        let json_value: Value = serde_json::from_str(&body)?;
        Ok(json_value)
    }

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

        let security_response = self.get_login_security_info(identity, password, ticket, pid)?;
        println!(
            "Security API response: {}",
            serde_json::to_string_pretty(&security_response)?
        );

        Ok(security_response)
    }

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
#[derive(Debug)]
pub struct LoginHandler {
    processor: AuthProcessor,
}

impl LoginHandler {
    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        Self {
            processor: AuthProcessor::new_with_provider(provider),
        }
    }

    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    fn client(&self) -> &CodeMaoClient {
        self.processor.client()
    }

    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();
        let _ = client.switch_identity(Identity::Blank);

        match self.processor.handle_password_v0(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data.get("token").and_then(|t| t.as_str()) {
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

    pub fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();
        let _ = client.switch_identity(Identity::Blank);

        match self.processor.handle_password_v1(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
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

    pub fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();
        let _ = client.switch_identity(Identity::Blank);

        match self.processor.handle_password_v2(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
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
            Err(e) => Err(format!("password_v2 登录失败: {}", e).into()),
        }
    }

    pub fn handle_token(
        &self,
        token: &str,
        status: AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let client = self.client();
        let auth_details = self.processor.fetch_auth_details(token)?;

        client.set_token(status.to_identity(), token)?;
        let _ = client.switch_identity(status.to_identity());

        Ok(LoginResult::new(true, LoginMethod::Token, "Token 登录成功")
            .with_token(token)
            .with_auth_details(auth_details))
    }

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

        client.set_token(Identity::Judgement, &token)?;
        let _ = client.switch_identity(Identity::Judgement);

        Ok(
            LoginResult::new(true, LoginMethod::AdminToken, "管理员 Token 登录成功")
                .with_token(&token),
        )
    }

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
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
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
#[derive(Debug)]
pub struct AuthManager {
    client_provider: Box<dyn ClientProvider>,
    processor: AuthProcessor,
    handler: LoginHandler,
    current_credentials: Option<LoginCredentials>,
}

impl AuthManager {
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

    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    /// 核心登录方法，现在接收 `Option<LoginMethod>` 枚举
    pub fn login(
        &mut self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        self.validate_login_parameters(credentials, prefer_method)?;
        self.current_credentials = Some(credentials.clone());

        match credentials.role {
            UserRole::Admin => self.admin_login(credentials, prefer_method),
            UserRole::User => self.user_login(credentials, prefer_method),
        }
    }

    fn validate_login_parameters(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(method) = prefer_method {
            match credentials.role {
                UserRole::User if !method.is_user_method() => {
                    return Err(format!("用户角色不支持登录方法 '{}'", method.as_str()).into());
                }
                UserRole::Admin if !method.is_admin_method() => {
                    return Err(format!("管理员角色不支持登录方法 '{}'", method.as_str()).into());
                }
                _ => {}
            }

            match method {
                LoginMethod::PasswordV0
                | LoginMethod::PasswordV1
                | LoginMethod::PasswordV2
                | LoginMethod::AdminPassword
                    if credentials.identity.is_empty() || credentials.password.is_empty() =>
                {
                    return Err(format!(
                        "登录方法 '{}' 需要提供 identity 和 password",
                        method.as_str()
                    )
                    .into());
                }
                LoginMethod::Token | LoginMethod::AdminToken if credentials.token.is_empty() => {
                    return Err(format!("登录方法 '{}' 需要提供 token", method.as_str()).into());
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn get_user_login_method(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> Result<LoginMethod, Box<dyn std::error::Error>> {
        if let Some(method) = prefer_method {
            if method.is_user_method() {
                return Ok(method);
            }
            return Err(format!("'{}' 不是有效的用户登录方法", method.as_str()).into());
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
    }

    fn get_admin_login_method(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> Result<LoginMethod, Box<dyn std::error::Error>> {
        if let Some(method) = prefer_method {
            if method.is_admin_method() {
                return Ok(method);
            }
            return Err(format!("'{}' 不是有效的管理员登录方法", method.as_str()).into());
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
    }

    fn user_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let method = self.get_user_login_method(credentials, prefer_method)?;

        match method {
            LoginMethod::PasswordV0 => self.handler.handle_password_v0(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                credentials.status,
            ),
            LoginMethod::PasswordV1 => self.handler.handle_password_v1(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                credentials.status,
            ),
            LoginMethod::PasswordV2 => self.handler.handle_password_v2(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                credentials.status,
            ),
            LoginMethod::Token => self
                .handler
                .handle_token(&credentials.token, credentials.status),
            _ => Err(format!("不支持的登录方式: {}", method.as_str()).into()),
        }
    }

    fn admin_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let method = self.get_admin_login_method(credentials, prefer_method)?;

        match method {
            LoginMethod::AdminToken => self.handler.handle_admin_token(Some(&credentials.token)),
            LoginMethod::AdminPassword => self
                .handler
                .handle_admin_password(Some(&credentials.identity), Some(&credentials.password)),
            _ => Err(format!("不支持的管理员登录方式: {}", method.as_str()).into()),
        }
    }

    pub fn execute_logout_v0(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let client = self.client();
        let response = client
            .build_request(HttpMethod::POST, "/tiger/accounts/logout", None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == 204)
    }

    pub fn execute_logout_v12(&self, method: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let client = self.client();
        let endpoint = format!("/tiger/v3/{}/accounts/logout", method);
        let response = client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == 204)
    }

    pub fn admin_logout(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let client = self.client();
        let response = client
            .build_request(HttpMethod::DELETE, "/admins/logout", Some(BaseKey::Whale))
            .send()?;
        Ok(response.status() == 204)
    }

    pub fn fetch_admin_dashboard_data(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let client = self.client();
        let response = client
            .build_request(HttpMethod::GET, "/admins/info", Some(BaseKey::Whale))
            .send()?;
        Ok(client.response_to_json(response)?)
    }

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

    pub fn new(authorization_token: Option<String>) -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()), authorization_token)
    }

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

    /// 生成设备认证（返回 JSON 字符串）
    pub fn generate_x_device_auth(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let timestamp = self.get_calibrated_timestamp()?;
        let sign_str = format!("{}{}{}", self.client_secret, timestamp, self.client_id);
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let sign = format!("{:X}", hasher.finalize());

        let auth_json = json!({
            "sign": sign,
            "timestamp": timestamp,
            "client_id": self.client_id,
        });

        // 返回 JSON 字符串
        Ok(serde_json::to_string(&auth_json)?)
    }

    /// 获取授权 token
    pub fn authorization_token(&self) -> Option<&String> {
        self.authorization_token.as_ref()
    }

    /// 设置授权 token
    pub fn set_authorization_token(&mut self, token: Option<String>) {
        self.authorization_token = token;
    }
}
// ==================== 链式调用构建器 ====================

/// 登录请求构建器，支持链式调用并强制使用 LoginMethod 枚举
#[derive(Debug)]
pub struct LoginBuilder {
    auth_manager: AuthManager,
    identity: Option<String>,
    password: Option<String>,
    token: Option<String>,
    pid: Option<String>,
    status: AccountStatus,
    role: UserRole,
    prefer_method: Option<LoginMethod>,
}

impl LoginBuilder {
    pub fn new() -> Self {
        Self {
            auth_manager: AuthManager::new(),
            identity: None,
            password: None,
            token: None,
            pid: None,
            status: AccountStatus::Average,
            role: UserRole::User,
            prefer_method: None,
        }
    }

    /// 设置登录标识
    pub fn identity(mut self, val: impl Into<String>) -> Self {
        self.identity = Some(val.into());
        self
    }

    /// 设置密码
    pub fn password(mut self, val: impl Into<String>) -> Self {
        self.password = Some(val.into());
        self
    }

    /// 设置 bearer token
    pub fn token(mut self, val: impl Into<String>) -> Self {
        self.token = Some(val.into());
        self
    }

    /// 设置客户端 PID
    pub fn pid(mut self, val: impl Into<String>) -> Self {
        self.pid = Some(val.into());
        self
    }

    /// 设置账号状态
    pub fn status(mut self, val: AccountStatus) -> Self {
        self.status = val;
        self
    }

    /// 设置用户角色
    pub fn role(mut self, val: UserRole) -> Self {
        self.role = val;
        self
    }

    /// 强制指定登录方法（使用 LoginMethod 枚举），不调用则自动推断
    pub fn method(mut self, val: LoginMethod) -> Self {
        self.prefer_method = Some(val);
        self
    }

    /// 执行登录
    pub fn execute(mut self) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let credentials = LoginCredentials {
            identity: self.identity.unwrap_or_default(),
            password: self.password.unwrap_or_default(),
            token: self.token.unwrap_or_default(),
            pid: self.pid.unwrap_or_else(|| "65edCTyg".to_string()),
            status: self.status,
            role: self.role,
        };

        self.auth_manager.login(&credentials, self.prefer_method)
    }
}

impl Default for LoginBuilder {
    fn default() -> Self {
        Self::new()
    }
}
