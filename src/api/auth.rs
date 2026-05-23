use crate::utils::acquire::{BaseKey, Catsona, CodeMaoClient, HttpMethod, MewError, MewResult};
use crate::utils::data::{CodeMaoFile, FileContent, PathConfig};
use rand::{Rng, RngExt};
use reqwest::header::HeaderValue;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

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

    pub fn is_user_method(&self) -> bool {
        matches!(
            self,
            LoginMethod::PasswordV0
                | LoginMethod::PasswordV1
                | LoginMethod::PasswordV2
                | LoginMethod::Token
        )
    }

    pub fn is_admin_method(&self) -> bool {
        matches!(self, LoginMethod::AdminToken | LoginMethod::AdminPassword)
    }
}

impl std::fmt::Display for LoginMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    User,
    Admin,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::User => "user",
            UserRole::Admin => "admin",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
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
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountStatus::Judgement => "judgement",
            AccountStatus::Average => "average",
            AccountStatus::Edu => "edu",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "judgement" => Some(AccountStatus::Judgement),
            "average" => Some(AccountStatus::Average),
            "edu" => Some(AccountStatus::Edu),
            _ => None,
        }
    }

    pub fn to_identity(&self) -> Catsona {
        match self {
            AccountStatus::Judgement => Catsona::Judge,
            AccountStatus::Average => Catsona::Fluffy,
            AccountStatus::Edu => Catsona::Scholar,
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

impl LoginCredentials {
    pub fn new() -> Self {
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
    pub fn new(success: bool, method: LoginMethod, message: impl Into<String>) -> Self {
        Self {
            success,
            method,
            message: message.into(),
            token: String::new(),
            data: Value::Null,
            auth_details: None,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = token.into();
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }

    pub fn with_auth_details(mut self, details: Value) -> Self {
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

#[derive(Debug, Clone, Default)]
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

pub async fn fetch_current_timestamp_with_provider(
    provider: &dyn ClientProvider,
) -> MewResult<i64> {
    let response = provider
        .client()
        .build_request(HttpMethod::GET, "/coconut/clouddb/currentTime", None)
        .send()
        .await?;
    let json: Value = response.json().await?;
    Ok(json["data"].as_i64().unwrap_or(0))
}

pub async fn fetch_current_timestamp() -> MewResult<i64> {
    fetch_current_timestamp_with_provider(&GlobalClientProvider::new()).await
}

fn determine_user_login_method(
    token: Option<&str>,
    identity: Option<&str>,
    password: Option<&str>,
) -> MewResult<LoginMethod> {
    if token.is_some() {
        return Ok(LoginMethod::Token);
    }
    if identity.is_some() && password.is_some() {
        return Ok(LoginMethod::PasswordV2);
    }
    Err(MewError::Auth(
        "缺少必要的登录凭据（需要 token 或 identity+password）".into(),
    ))
}

fn determine_admin_login_method(
    token: Option<&str>,
    identity: Option<&str>,
    password: Option<&str>,
) -> MewResult<LoginMethod> {
    if token.is_some() {
        return Ok(LoginMethod::AdminToken);
    }
    if identity.is_some() || password.is_some() {
        return Ok(LoginMethod::AdminPassword);
    }
    Err(MewError::Auth("缺少必要的管理员登录凭据".into()))
}

// ==================== 认证处理器 ====================

#[derive(Clone, Debug)]
pub struct AuthProcessor {
    client_provider: Box<dyn ClientProvider>,
}

impl AuthProcessor {
    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        Self {
            client_provider: provider,
        }
    }

    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    pub async fn fetch_auth_details(&self, token: &str) -> MewResult<Value> {
        let response = self
            .client()
            .build_request(HttpMethod::GET, "/web/users/details", None)
            .with_header("Cookie", &format!("authorization={}", token))
            .send()
            .await?;

        let cookies: Vec<String> = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|h: &HeaderValue| h.to_str().unwrap_or("").to_string())
            .collect();

        for cookie in &cookies {
            println!("Received cookie: {}", cookie);
        }

        response.json().await.map_err(MewError::from)
    }

    pub async fn get_login_ticket(
        &self,
        identity: &str,
        timestamp: i64,
        pid: &str,
    ) -> MewResult<Value> {
        let response = self
            .client()
            .build_request(
                HttpMethod::POST,
                "https://open-service.codemao.cn/captcha/rule/v3",
                None,
            )
            .with_payload(json!({
                "identity": identity,
                "pid": pid,
                "timestamp": timestamp,
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    pub async fn get_login_security_info(
        &self,
        identity: &str,
        password: &str,
        ticket: &str,
        pid: &str,
    ) -> MewResult<Value> {
        let response = self
            .client()
            .build_request(
                HttpMethod::POST,
                "/tiger/v3/web/accounts/login/security",
                None,
            )
            .with_header("x-captcha-ticket", ticket)
            .with_payload(json!({
                "identity": identity,
                "password": password,
                "pid": pid,
                "agreement_ids": [-1],
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    pub async fn authenticate_admin_user(
        &self,
        username: &str,
        password: &str,
        key: i64,
        code: &str,
    ) -> MewResult<Value> {
        let response = self
            .client()
            .build_request(HttpMethod::POST, "/admins/login", Some(BaseKey::Whale))
            .with_payload(json!({
                "username": username,
                "password": password,
                "key": key,
                "code": code,
            }))
            .send()
            .await?;
        response.json().await.map_err(MewError::from)
    }

    pub async fn fetch_admin_captcha(&self, timestamp: i64) -> MewResult<Vec<u8>> {
        let endpoint = format!("/admins/captcha/{}", timestamp);
        let response = self
            .client()
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Whale))
            .send()
            .await?;

        if response.status().is_success() {
            let bytes = response.bytes().await?.to_vec();
            CodeMaoFile::file_write(
                &PathConfig::captcha_file_path(),
                &FileContent::Bytes(bytes.clone()),
                "b",
            )
            .unwrap();
            println!(
                "验证码已保存至: {:?}",
                PathConfig::captcha_file_path().to_str()
            );
            Ok(bytes)
        } else {
            println!("获取验证码失败, 状态代码: {}", response.status());
            Ok(Vec::new())
        }
    }

    pub async fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> MewResult<Value> {
        self.client()
            .build_request(HttpMethod::POST, "/tiger/accounts/login", None)
            .with_payload(json!({
                "identity": identity,
                "password": password,
                "pid": pid,
            }))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    pub async fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> MewResult<Value> {
        self.client()
            .build_request(HttpMethod::POST, "/tiger/v3/web/accounts/login", None)
            .with_payload(json!({
                "identity": identity,
                "password": password,
                "pid": pid,
            }))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    pub async fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> MewResult<Value> {
        let timestamp = fetch_current_timestamp_with_provider(&*self.client_provider).await?;
        let ticket_response = self.get_login_ticket(identity, timestamp, pid).await?;
        println!("Ticket response: {:?}", ticket_response);

        let ticket = ticket_response["ticket"]
            .as_str()
            .ok_or_else(|| MewError::Auth("无法获取ticket".into()))?;

        let security_response = self
            .get_login_security_info(identity, password, ticket, pid)
            .await?;
        println!(
            "Security API response: {}",
            serde_json::to_string_pretty(&security_response)?
        );

        Ok(security_response)
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

    pub async fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> MewResult<LoginResult> {
        self.client().switch_identity(Catsona::Blanky)?;

        match self
            .processor
            .handle_password_v0(identity, password, pid)
            .await
        {
            Ok(data) => {
                if let Some(token) = data.get("token").and_then(|t| t.as_str()) {
                    self.client().set_token(status.to_identity(), token)?;
                    self.client().switch_identity(status.to_identity())?;
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
                format!("登录失败: {}", e),
            )),
        }
    }

    pub async fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> MewResult<LoginResult> {
        self.client().switch_identity(Catsona::Blanky)?;

        match self
            .processor
            .handle_password_v1(identity, password, pid)
            .await
        {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    self.client().set_token(status.to_identity(), token)?;
                    self.client().switch_identity(status.to_identity())?;
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
                format!("登录失败: {}", e),
            )),
        }
    }

    pub async fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> MewResult<LoginResult> {
        self.client().switch_identity(Catsona::Blanky)?;

        match self
            .processor
            .handle_password_v2(identity, password, pid)
            .await
        {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    self.client().set_token(status.to_identity(), token)?;
                    self.client().switch_identity(status.to_identity())?;
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
            Err(e) => Err(MewError::Auth(format!("password_v2 登录失败: {}", e))),
        }
    }

    pub async fn handle_token(&self, token: &str, status: AccountStatus) -> MewResult<LoginResult> {
        let auth_details = self.processor.fetch_auth_details(token).await?;
        self.client().set_token(status.to_identity(), token)?;
        self.client().switch_identity(status.to_identity())?;

        Ok(LoginResult::new(true, LoginMethod::Token, "Token 登录成功")
            .with_token(token)
            .with_auth_details(auth_details))
    }

    pub fn handle_admin_token(&self, token: Option<&str>) -> MewResult<LoginResult> {
        let token = match token {
            Some(t) => t.to_string(),
            None => {
                println!("请输入 Authorization Token:");
                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| MewError::Io(e))?;
                input.trim().to_string()
            }
        };

        self.client().set_token(Catsona::Judge, &token)?;
        self.client().switch_identity(Catsona::Judge)?;

        Ok(
            LoginResult::new(true, LoginMethod::AdminToken, "管理员 Token 登录成功")
                .with_token(&token),
        )
    }

    pub async fn handle_admin_password(
        &self,
        username: Option<&str>,
        password: Option<&str>,
    ) -> MewResult<LoginResult> {
        let mut username = match username {
            Some(u) => u.to_string(),
            None => Self::read_input("请输入用户名:")?,
        };

        let mut password = match password {
            Some(p) => p.to_string(),
            None => Self::read_input("请输入密码:")?,
        };

        loop {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| MewError::Other(e.to_string()))?
                .as_millis() as i64;

            println!("正在获取验证码...");
            self.processor.fetch_admin_captcha(timestamp).await?;

            let captcha = Self::read_input("请输入验证码:")?;

            match self
                .processor
                .authenticate_admin_user(&username, &password, timestamp, &captcha)
                .await
            {
                Ok(response) => {
                    if let Some(token) = response.get("token").and_then(|t| t.as_str()) {
                        self.client().set_token(Catsona::Judge, token)?;
                        self.client().switch_identity(Catsona::Judge)?;
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
                            username = Self::read_input("请输入用户名:")?;
                            password = Self::read_input("请输入密码:")?;
                        }
                    }
                }
                Err(e) => println!("认证请求失败: {}", e),
            }
        }
    }

    fn read_input(prompt: &str) -> MewResult<String> {
        println!("{}", prompt);
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| MewError::Io(e))?;
        Ok(input.trim().to_string())
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
        Self {
            processor: AuthProcessor::new_with_provider(provider.clone_box()),
            handler: LoginHandler::new_with_provider(provider.clone_box()),
            client_provider: provider,
            current_credentials: None,
        }
    }

    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    pub async fn login(
        &mut self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<LoginResult> {
        self.validate_login_parameters(credentials, prefer_method)?;
        self.current_credentials = Some(credentials.clone());

        match credentials.role {
            UserRole::Admin => self.admin_login(credentials, prefer_method).await,
            UserRole::User => self.user_login(credentials, prefer_method).await,
        }
    }

    fn validate_login_parameters(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<()> {
        if let Some(method) = prefer_method {
            match credentials.role {
                UserRole::User if !method.is_user_method() => {
                    return Err(MewError::Auth(format!(
                        "用户角色不支持登录方法 '{}'",
                        method
                    )));
                }
                UserRole::Admin if !method.is_admin_method() => {
                    return Err(MewError::Auth(format!(
                        "管理员角色不支持登录方法 '{}'",
                        method
                    )));
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
                    return Err(MewError::Auth(format!(
                        "登录方法 '{}' 需要提供 identity 和 password",
                        method
                    )));
                }
                LoginMethod::Token | LoginMethod::AdminToken if credentials.token.is_empty() => {
                    return Err(MewError::Auth(format!(
                        "登录方法 '{}' 需要提供 token",
                        method
                    )));
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
    ) -> MewResult<LoginMethod> {
        if let Some(method) = prefer_method {
            if method.is_user_method() {
                return Ok(method);
            }
            return Err(MewError::Auth(format!(
                "'{}' 不是有效的用户登录方法",
                method
            )));
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
    ) -> MewResult<LoginMethod> {
        if let Some(method) = prefer_method {
            if method.is_admin_method() {
                return Ok(method);
            }
            return Err(MewError::Auth(format!(
                "'{}' 不是有效的管理员登录方法",
                method
            )));
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

    async fn user_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<LoginResult> {
        let method = self.get_user_login_method(credentials, prefer_method)?;

        match method {
            LoginMethod::PasswordV0 => {
                self.handler
                    .handle_password_v0(
                        &credentials.identity,
                        &credentials.password,
                        &credentials.pid,
                        credentials.status,
                    )
                    .await
            }
            LoginMethod::PasswordV1 => {
                self.handler
                    .handle_password_v1(
                        &credentials.identity,
                        &credentials.password,
                        &credentials.pid,
                        credentials.status,
                    )
                    .await
            }
            LoginMethod::PasswordV2 => {
                self.handler
                    .handle_password_v2(
                        &credentials.identity,
                        &credentials.password,
                        &credentials.pid,
                        credentials.status,
                    )
                    .await
            }
            LoginMethod::Token => {
                self.handler
                    .handle_token(&credentials.token, credentials.status)
                    .await
            }
            _ => Err(MewError::Auth(format!("不支持的登录方式: {}", method))),
        }
    }

    async fn admin_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<LoginResult> {
        let method = self.get_admin_login_method(credentials, prefer_method)?;

        match method {
            LoginMethod::AdminToken => self.handler.handle_admin_token(Some(&credentials.token)),
            LoginMethod::AdminPassword => {
                self.handler
                    .handle_admin_password(Some(&credentials.identity), Some(&credentials.password))
                    .await
            }
            _ => Err(MewError::Auth(format!(
                "不支持的管理员登录方式: {}",
                method
            ))),
        }
    }

    pub async fn execute_logout_v0(&self) -> MewResult<bool> {
        let response = self
            .client()
            .build_request(HttpMethod::POST, "/tiger/accounts/logout", None)
            .with_payload(json!({}))
            .send()
            .await?;
        Ok(response.status().as_u16() == 204)
    }

    pub async fn execute_logout_v12(&self, method: &str) -> MewResult<bool> {
        let endpoint = format!("/tiger/v3/{}/accounts/logout", method);
        let response = self
            .client()
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()
            .await?;
        Ok(response.status().as_u16() == 204)
    }

    pub async fn admin_logout(&self) -> MewResult<bool> {
        let response = self
            .client()
            .build_request(HttpMethod::DELETE, "/admins/logout", Some(BaseKey::Whale))
            .send()
            .await?;
        Ok(response.status().as_u16() == 204)
    }

    pub async fn fetch_admin_dashboard_data(&self) -> MewResult<Value> {
        self.client()
            .build_request(HttpMethod::GET, "/admins/info", Some(BaseKey::Whale))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    pub fn configure_authentication_token(
        &self,
        token: &str,
        status: AccountStatus,
    ) -> MewResult<()> {
        self.client().set_token(status.to_identity(), token)?;
        self.client().switch_identity(status.to_identity())?;
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
}

impl CloudAuthenticator {
    const CLIENT_SECRET: &'static str = "pBlYqXbJDu";

    pub fn new_with_provider(
        provider: Box<dyn ClientProvider>,
        authorization_token: Option<String>,
    ) -> Self {
        Self {
            client_provider: provider,
            authorization_token,
            client_id: Self::generate_client_id(8),
            time_difference: 0,
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

    pub async fn get_calibrated_timestamp(&mut self) -> MewResult<i64> {
        if self.time_difference == 0 {
            let server_time = fetch_current_timestamp_with_provider(&*self.client_provider).await?;
            let local_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| MewError::Other(e.to_string()))?
                .as_secs() as i64;
            self.time_difference = local_time - server_time;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| MewError::Other(e.to_string()))?
            .as_secs() as i64;
        Ok(now - self.time_difference)
    }

    pub async fn generate_x_device_auth(&mut self) -> MewResult<String> {
        let timestamp = self.get_calibrated_timestamp().await?;
        let sign_str = format!("{}{}{}", Self::CLIENT_SECRET, timestamp, self.client_id);
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let sign = format!("{:x}", hasher.finalize());

        let auth_json = json!({
            "sign": sign,
            "timestamp": timestamp,
            "client_id": self.client_id,
        });

        Ok(serde_json::to_string(&auth_json)?)
    }

    pub fn authorization_token(&self) -> Option<String> {
        if let Some(ref token) = self.authorization_token {
            if !token.is_empty() {
                return Some(token.clone());
            }
        }
        self.client().current_token()
    }

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

    pub fn identity(mut self, val: impl Into<String>) -> Self {
        self.identity = Some(val.into());
        self
    }

    pub fn password(mut self, val: impl Into<String>) -> Self {
        self.password = Some(val.into());
        self
    }

    pub fn token(mut self, val: impl Into<String>) -> Self {
        self.token = Some(val.into());
        self
    }

    pub fn pid(mut self, val: impl Into<String>) -> Self {
        self.pid = Some(val.into());
        self
    }

    pub fn status(mut self, val: AccountStatus) -> Self {
        self.status = val;
        self
    }

    pub fn role(mut self, val: UserRole) -> Self {
        self.role = val;
        self
    }

    pub fn method(mut self, val: LoginMethod) -> Self {
        self.prefer_method = Some(val);
        self
    }

    pub async fn execute(self) -> MewResult<LoginResult> {
        let credentials = LoginCredentials {
            identity: self.identity.unwrap_or_default(),
            password: self.password.unwrap_or_default(),
            token: self.token.unwrap_or_default(),
            pid: self.pid.unwrap_or_else(|| "65edCTyg".to_string()),
            status: self.status,
            role: self.role,
        };

        let mut auth_manager = self.auth_manager;
        auth_manager.login(&credentials, self.prefer_method).await
    }
}
