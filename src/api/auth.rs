use crate::utils::acquire::{
    BaseKey, Catsona, ClientAccess, CodeMaoClient, DEFAULT_PID, HttpMethod, MewError, MewResult,
    current_timestamp_13, current_timestamp_secs, generate_random_id,
};
use crate::utils::data::{CodeMaoFile, PathConfig, value_to_i64};
use log::{debug, warn};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use std::sync::{Arc, OnceLock};

// 枚举定义

/// 登录方式枚举,涵盖用户与管理员的各种登录途径
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

    /// 判断该登录方法是否属于普通用户
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

impl std::str::FromStr for LoginMethod {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "password_v0" => Ok(LoginMethod::PasswordV0),
            "password_v1" => Ok(LoginMethod::PasswordV1),
            "password_v2" => Ok(LoginMethod::PasswordV2),
            "token" => Ok(LoginMethod::Token),
            "admin_token" => Ok(LoginMethod::AdminToken),
            "admin_password" => Ok(LoginMethod::AdminPassword),
            _ => Err(()),
        }
    }
}

/// 退出登录方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoutMethod {
    /// v0 旧版登出
    V0,
    /// v1/v2 Web 端登出
    Web,
    /// v1/v2 移动端登出
    Mobile,
    /// 管理员登出
    Admin,
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

/// 账号状态/类型(普通,评审,教育)
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

    /// 映射为身份枚举 `Catsona`
    pub fn to_identity(self) -> Catsona {
        match self {
            AccountStatus::Judgement => Catsona::Judge,
            AccountStatus::Average => Catsona::Fluffy,
            AccountStatus::Edu => Catsona::Scholar,
        }
    }
}

// 数据结构

/// 登录凭据,聚合身份,密码,令牌,状态等信息
#[derive(Debug, Clone)]
pub struct LoginCredentials {
    pub identity: String,
    pub password: String,
    pub token: String,
    pub pid: String,
    pub status: AccountStatus,
    pub role: UserRole,
    pub timestamp: Option<i64>,
    pub captcha: Option<String>,
}

impl Default for LoginCredentials {
    fn default() -> Self {
        Self {
            identity: String::new(),
            password: String::new(),
            token: String::new(),
            pid: DEFAULT_PID.to_string(),
            status: AccountStatus::Average,
            role: UserRole::User,
            timestamp: None,
            captcha: None,
        }
    }
}

/// 登录结果,包含成功标志,登录方式,令牌等信息
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

/// 管理员信息(`GET /admins/info` 响应的 `admin` 对象)
#[derive(Debug, Clone)]
pub struct AdminInfo {
    pub id: i64,
    pub username: String,
    pub role_name: String,
    pub full_name: String,
}

impl AdminInfo {
    /// 从 `GET /admins/info` 响应中提取管理员信息。
    /// 固定读取 `admin` 对象下的 id/username/role_name/full_name 四个字段,
    /// 不做任何形态回退尝试;任一字段缺失或类型不符时返回 None。
    pub fn from_details(details: &Value) -> Option<AdminInfo> {
        let admin = details.get("admin")?;
        Some(AdminInfo {
            id: value_to_i64(admin.get("id")?)?,
            username: admin.get("username")?.as_str()?.to_string(),
            role_name: admin.get("role_name")?.as_str()?.to_string(),
            full_name: admin.get("full_name")?.as_str()?.to_string(),
        })
    }
}

// 客户端提供者特质

/// 抽象客户端提供者,便于依赖注入和测试
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

// 全局单例

static GLOBAL_AUTH_MANAGER: OnceLock<Arc<AuthManager>> = OnceLock::new();

pub fn global_auth_manager() -> Arc<AuthManager> {
    GLOBAL_AUTH_MANAGER
        .get_or_init(|| Arc::new(AuthManager::new()))
        .clone()
}

// 辅助函数

/// 通过任意 `ClientProvider` 获取服务器当前时间戳(秒)
pub fn fetch_current_timestamp_with_provider(provider: &dyn ClientProvider) -> MewResult<i64> {
    let client = provider.client();
    let response = client
        .build_request(HttpMethod::Get, "/coconut/clouddb/currentTime", None)
        .send()?;
    let json = client.response_to_json(response)?;
    // 服务端可能返回数字或数字字符串,统一经 value_to_i64 解析
    Ok(json.get("data").and_then(value_to_i64).unwrap_or(0))
}

/// 使用全局客户端获取当前时间戳
pub fn fetch_current_timestamp() -> MewResult<i64> {
    fetch_current_timestamp_with_provider(&GlobalClientProvider::new())
}

/// 根据提供的令牌,身份,密码自动推断普通用户登录方式
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
    Err(MewError::Auth("缺少必要的登录凭据".into()))
}

/// 根据提供的令牌,身份,密码自动推断管理员登录方式
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

// 认证处理器

/// 处理原始 HTTP 认证请求,如获取 ticket,验证码,登录接口调用
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

    /// 根据令牌获取用户详细信息
    pub fn fetch_auth_details(&self, token: &str) -> MewResult<Value> {
        let client = self.client();
        let cookie_str = format!("authorization={}", token);
        let response = client
            .build_request(HttpMethod::Get, "/web/users/details", None)
            .with_header("Cookie", cookie_str)
            .send()?;
        client.response_to_json(response)
    }

    /// 获取管理员后台信息(需已设置管理员令牌)
    pub fn fetch_admin_details(&self) -> MewResult<Value> {
        let client = self.client();
        self.send_and_parse(client.build_request(
            HttpMethod::Get,
            "/admins/info",
            Some(BaseKey::Whale),
        ))
    }

    /// 获取登录 ticket(用于新版登录流程)
    pub fn get_login_ticket(&self, identity: &str, timestamp: i64, pid: &str) -> MewResult<Value> {
        let client = self.client();
        let payload = json!({
            "identity": identity,
            "pid": pid,
            "timestamp": timestamp,
        });
        self.send_and_parse(
            client
                .build_request(
                    HttpMethod::Post,
                    "/captcha/rule/v3",
                    Some(BaseKey::OpenService),
                )
                .with_payload(payload),
        )
    }

    /// 发送安全登录请求(v2 密码登录)
    pub fn get_login_security_info(
        &self,
        identity: &str,
        password: &str,
        ticket: &str,
        pid: &str,
    ) -> MewResult<Value> {
        let client = self.client();
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
            "agreement_ids": [-1],
        });
        let response = client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/login/security",
                None,
            )
            .with_header("x-captcha-ticket", ticket)
            .with_payload(payload)
            .send()?;
        let status = response.status();
        if status != 200 {
            let body = response.into_body().read_to_string().unwrap_or_default();
            return Err(MewError::Auth(format!(
                "API返回错误状态码: {}, Body: {}",
                status, body
            )));
        }
        let body = response.into_body().read_to_string()?;
        Ok(serde_json::from_str(&body)?)
    }

    /// 管理员用户名密码认证
    pub fn authenticate_admin_user(
        &self,
        username: &str,
        password: &str,
        key: i64,
        code: &str,
    ) -> MewResult<Value> {
        let client = self.client();
        let payload = json!({
            "username": username,
            "password": password,
            "key": key,
            "code": code,
        });
        self.send_and_parse(
            client
                .build_request(HttpMethod::Post, "/admins/login", Some(BaseKey::Whale))
                .with_payload(payload)
                // 保留 4xx/5xx 响应体,使上层能区分验证码错误与密码错误
                .with_error_body(),
        )
    }

    /// 获取管理员验证码图片并保存到文件
    /// 返回时间戳,供后续登录使用
    pub fn fetch_admin_captcha(&self) -> MewResult<i64> {
        let timestamp = current_timestamp_13() as i64;

        let client = self.client();
        let endpoint = format!("/admins/captcha/{}", timestamp);
        let response = client
            .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Whale))
            .send()?;

        if response.status() == 200 {
            let bytes = response.into_body().read_to_vec()?;
            CodeMaoFile::write_bytes(&PathConfig::global().captcha_file_path(), &bytes)
                .map_err(|e| MewError::Other(format!("验证码文件写入失败: {}", e)))?;
            debug!("管理员验证码已保存至文件");
            Ok(timestamp)
        } else {
            Err(MewError::Auth(format!(
                "获取验证码失败, 状态码: {}",
                response.status()
            )))
        }
    }

    /// v0 密码登录请求
    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> MewResult<Value> {
        let client = self.client();
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
        });
        self.send_and_parse(
            client
                .build_request(HttpMethod::Post, "/tiger/accounts/login", None)
                .with_payload(payload),
        )
    }

    /// v1 密码登录请求
    pub fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> MewResult<Value> {
        let client = self.client();
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
        });
        self.send_and_parse(
            client
                .build_request(HttpMethod::Post, "/tiger/v3/web/accounts/login", None)
                .with_payload(payload),
        )
    }

    /// v2 密码登录(带 ticket 的安全流程)
    pub fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> MewResult<Value> {
        // 登录票据时间戳使用本地毫秒(与官方客户端一致),不依赖服务器时钟
        let timestamp = current_timestamp_13() as i64;
        let ticket_response = self.get_login_ticket(identity, timestamp, pid)?;
        let ticket = ticket_response
            .get("ticket")
            .and_then(Value::as_str)
            .ok_or_else(|| MewError::Auth("无法获取ticket".into()))?;
        self.get_login_security_info(identity, password, ticket, pid)
    }
}

impl Default for AuthProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for AuthProcessor {
    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }
}

// 登录处理器

/// 负责编排具体登录流程,调用 `AuthProcessor` 完成请求并设置令牌
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

    /// 将令牌设置到客户端,并切换到对应身份
    fn set_token_and_identity(&self, token: &str, identity: Catsona) -> MewResult<()> {
        self.client().set_token(identity, token)?;
        self.client().switch_identity(identity)?;
        Ok(())
    }

    /// v0 密码登录处理
    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> MewResult<LoginResult> {
        let client = self.client();
        client.switch_identity(Catsona::Blanky)?;

        match self.processor.handle_password_v0(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data.get("token").and_then(|t| t.as_str()) {
                    self.set_token_and_identity(token, status.to_identity())?;
                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV0, "v0 密码登录成功")
                            .with_token(token)
                            .with_data(data),
                    )
                } else {
                    Err(MewError::Auth("v0 密码登录失败:未获取到token".into()))
                }
            }
            Err(e) => match e {
                // 保留底层错误变体(Http/Json/Io 等),便于调用方区分凭据与网络错误
                MewError::Auth(msg) => Err(MewError::Auth(format!("v0 登录失败: {msg}"))),
                other => Err(other),
            },
        }
    }

    /// v1 密码登录处理
    pub fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> MewResult<LoginResult> {
        let client = self.client();
        client.switch_identity(Catsona::Blanky)?;

        match self.processor.handle_password_v1(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    self.set_token_and_identity(token, status.to_identity())?;
                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV1, "v1 密码登录成功")
                            .with_token(token)
                            .with_data(data),
                    )
                } else {
                    Err(MewError::Auth("v1 密码登录失败:未获取到token".into()))
                }
            }
            Err(e) => match e {
                // 保留底层错误变体(Http/Json/Io 等),便于调用方区分凭据与网络错误
                MewError::Auth(msg) => Err(MewError::Auth(format!("v1 登录失败: {msg}"))),
                other => Err(other),
            },
        }
    }

    /// v2 密码登录处理
    pub fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: AccountStatus,
    ) -> MewResult<LoginResult> {
        let client = self.client();
        client.switch_identity(Catsona::Blanky)?;

        match self.processor.handle_password_v2(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    self.set_token_and_identity(token, status.to_identity())?;
                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV2, "v2 密码登录成功")
                            .with_token(token)
                            .with_data(data),
                    )
                } else {
                    Err(MewError::Auth("v2 密码登录失败:未获取到token".into()))
                }
            }
            Err(e) => match e {
                // 保留底层错误变体(Http/Json/Io 等),便于调用方区分凭据与网络错误
                MewError::Auth(msg) => Err(MewError::Auth(format!("v2 登录失败: {msg}"))),
                other => Err(other),
            },
        }
    }

    /// Token 登录处理(普通用户)
    pub fn handle_token(&self, token: &str, status: AccountStatus) -> MewResult<LoginResult> {
        if token.trim().is_empty() {
            return Err(MewError::Auth("Token 不能为空".into()));
        }

        let auth_details = self.processor.fetch_auth_details(token)?;
        self.set_token_and_identity(token, status.to_identity())?;
        Ok(LoginResult::new(true, LoginMethod::Token, "Token 登录成功")
            .with_token(token)
            .with_auth_details(auth_details))
    }

    /// 管理员 Token 登录(所有参数通过传参获取)
    pub fn handle_admin_token(&self, token: &str) -> MewResult<LoginResult> {
        if token.trim().is_empty() {
            return Err(MewError::Auth("管理员 Token 不能为空".into()));
        }

        self.client().set_token(Catsona::Judge, token)?;
        self.client().switch_identity(Catsona::Judge)?;

        match self.processor.fetch_admin_details() {
            Ok(data) => {
                if data.get("admin").is_some() {
                    Ok(
                        LoginResult::new(true, LoginMethod::AdminToken, "管理员 Token 登录成功")
                            .with_token(token)
                            .with_auth_details(data),
                    )
                } else {
                    let _ = self.client().set_token(Catsona::Judge, "");
                    Err(MewError::Auth("管理员 Token 无效或已过期".into()))
                }
            }
            Err(e) => {
                let _ = self.client().set_token(Catsona::Judge, "");
                Err(MewError::Auth(format!("管理员 Token 验证失败: {}", e)))
            }
        }
    }

    /// 管理员用户名密码登录(完全由参数驱动,不再交互与重试)
    /// 需要调用者先通过 `AuthProcessor::fetch_admin_captcha` 获取 `timestamp` 和验证码图片,
    /// 然后将用户识别的验证码字符串传入
    pub fn handle_admin_password(
        &self,
        username: &str,
        password: &str,
        timestamp: i64,
        captcha: &str,
    ) -> MewResult<LoginResult> {
        // 参数校验
        if username.trim().is_empty() {
            return Err(MewError::Auth("管理员用户名不能为空".into()));
        }
        if password.trim().is_empty() {
            return Err(MewError::Auth("管理员密码不能为空".into()));
        }
        if captcha.trim().is_empty() {
            return Err(MewError::Auth("验证码不能为空 (Captcha)".into()));
        }

        let response = self
            .processor
            .authenticate_admin_user(username, password, timestamp, captcha)?;

        if let Some(token) = response.get("token").and_then(|t| t.as_str()) {
            self.client().set_token(Catsona::Judge, token)?;
            self.client().switch_identity(Catsona::Judge)?;
            return Ok(
                LoginResult::new(true, LoginMethod::AdminPassword, "管理员账密登录成功")
                    .with_token(token),
            );
        }

        // 提取错误信息
        let error_code = response
            .get("error_code")
            .and_then(|e| e.as_str())
            .unwrap_or("");
        let error_msg = response
            .get("error_msg")
            .and_then(|e| e.as_str())
            .unwrap_or("未知错误");

        // 根据错误码返回具体的错误信息
        match error_code {
            "Admin-Password-Error@Community-Admin" | "Param-Invalid@Common" => {
                Err(MewError::Auth("管理员用户名或密码错误".into()))
            }
            "Captcha-Error@Community-Admin" | "Captcha-Expired@Community-Admin" => Err(
                MewError::Auth(format!("验证码错误或已过期 (Captcha): {error_msg}")),
            ),
            _ => Err(MewError::Auth(format!("管理员登录失败: {}", error_msg))),
        }
    }
}

impl Default for LoginHandler {
    fn default() -> Self {
        Self::new()
    }
}

// 认证管理器

/// 认证管理器,整合登录,登出,凭据管理等功能
/// 可通过 `new()` 创建默认实例,或使用 `new_with_provider` 注入自定义客户端
#[derive(Debug)]
pub struct AuthManager {
    client_provider: Box<dyn ClientProvider>,
    processor: AuthProcessor,
    handler: LoginHandler,
    current_credentials: Option<LoginCredentials>,
    current_method: Option<LoginMethod>,
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
            current_method: None,
        }
    }

    pub fn new() -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()))
    }

    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    /// 执行登录,自动根据角色和凭据选择登录方式
    /// `prefer_method` 可强制指定登录方式,但需与角色匹配
    pub fn login(
        &mut self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<LoginResult> {
        self.validate_login_parameters(credentials, prefer_method)?;
        self.current_credentials = Some(credentials.clone());

        let result = match credentials.role {
            UserRole::Admin => self.admin_login(credentials, prefer_method),
            UserRole::User => self.user_login(credentials, prefer_method),
        }?;
        self.current_method = Some(result.method);
        Ok(result)
    }

    /// 验证登录参数是否与指定方式匹配
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
                        method.as_str()
                    )));
                }
                UserRole::Admin if !method.is_admin_method() => {
                    return Err(MewError::Auth(format!(
                        "管理员角色不支持登录方法 '{}'",
                        method.as_str()
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
                        method.as_str()
                    )));
                }
                LoginMethod::Token | LoginMethod::AdminToken if credentials.token.is_empty() => {
                    return Err(MewError::Auth(format!(
                        "登录方法 '{}' 需要提供 token",
                        method.as_str()
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// 自动推断或确认普通用户的登录方法
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
                method.as_str()
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

    /// 自动推断或确认管理员的登录方法
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
                method.as_str()
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

    fn user_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<LoginResult> {
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
            _ => Err(MewError::Auth(format!(
                "不支持的登录方式: {}",
                method.as_str()
            ))),
        }
    }

    fn admin_login(
        &self,
        credentials: &LoginCredentials,
        prefer_method: Option<LoginMethod>,
    ) -> MewResult<LoginResult> {
        let method = self.get_admin_login_method(credentials, prefer_method)?;
        let mut result = match method {
            LoginMethod::AdminToken => self.handler.handle_admin_token(&credentials.token)?,
            LoginMethod::AdminPassword => {
                let timestamp = credentials
                    .timestamp
                    .ok_or_else(|| MewError::Auth("管理员密码登录需要提供 timestamp".into()))?;
                let captcha = credentials
                    .captcha
                    .as_deref()
                    .ok_or_else(|| MewError::Auth("管理员密码登录需要提供验证码".into()))?;
                self.handler.handle_admin_password(
                    &credentials.identity,
                    &credentials.password,
                    timestamp,
                    captcha,
                )?
            }
            _ => {
                return Err(MewError::Auth(format!(
                    "不支持的管理员登录方式: {}",
                    method.as_str()
                )));
            }
        };

        if result.success && result.auth_details.is_none() {
            // 仅在尚未获取管理员详情时才请求;
            // 与 handle_admin_token 一致,保存完整响应形态(AdminInfo::from_details 依赖)
            match self.processor.fetch_admin_details() {
                Ok(dashboard) => {
                    result = result.with_auth_details(dashboard);
                }
                Err(e) => warn!("获取管理员详情失败: {}", e),
            }
        }
        Ok(result)
    }

    /// v0 登出
    pub fn logout_v0(&self) -> MewResult<bool> {
        let client = self.client();
        let response = client
            .build_request(HttpMethod::Post, "/tiger/accounts/logout", None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == 204)
    }

    /// v1/v2 登出,`method` 为 "web" 或 "mobile"
    pub fn logout_v12(&self, method: LogoutMethod) -> MewResult<bool> {
        let segment = match method {
            LogoutMethod::Web => "web",
            LogoutMethod::Mobile => "mobile",
            LogoutMethod::V0 | LogoutMethod::Admin => {
                return Err(MewError::Auth("v0/Admin 登出走专用路径".into()));
            }
        };
        let client = self.client();
        let endpoint = format!("/tiger/v3/{}/accounts/logout", segment);
        let response = client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == 204)
    }

    /// 管理员登出
    pub fn admin_logout(&self) -> MewResult<bool> {
        let client = self.client();
        let response = client
            .build_request(HttpMethod::Delete, "/admins/logout", Some(BaseKey::Whale))
            .send()?;
        Ok(response.status() == 204)
    }

    /// 统一退出登录,根据当前登录方式自动选择登出端点
    /// 登出成功后清空当前身份令牌
    pub fn logout(&self) -> MewResult<bool> {
        let method = self
            .current_method
            .ok_or_else(|| MewError::Auth("当前未登录,无法退出".into()))?;
        let logout = match method {
            LoginMethod::PasswordV0 => LogoutMethod::V0,
            LoginMethod::PasswordV1 | LoginMethod::PasswordV2 | LoginMethod::Token => {
                LogoutMethod::Web
            }
            LoginMethod::AdminToken | LoginMethod::AdminPassword => LogoutMethod::Admin,
        };
        self.logout_with(logout)
    }

    /// 显式指定方式退出登录
    pub fn logout_with(&self, method: LogoutMethod) -> MewResult<bool> {
        let ok = match method {
            LogoutMethod::V0 => self.logout_v0()?,
            LogoutMethod::Web => self.logout_v12(LogoutMethod::Web)?,
            LogoutMethod::Mobile => self.logout_v12(LogoutMethod::Mobile)?,
            LogoutMethod::Admin => self.admin_logout()?,
        };
        if ok {
            let identity = self.client().current_identity();
            let _ = self.client().set_token(identity, "");
        }
        Ok(ok)
    }

    /// 手动配置认证令牌(不经过登录流程)
    pub fn configure_authentication_token(
        &self,
        token: &str,
        status: AccountStatus,
    ) -> MewResult<()> {
        let client = self.client();
        client.set_token(status.to_identity(), token)?;
        client.switch_identity(status.to_identity())?;
        Ok(())
    }

    /// 获取当前缓存的凭据(成功登录后存在)
    pub fn get_current_credentials(&self) -> Option<&LoginCredentials> {
        self.current_credentials.as_ref()
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

// 云服务认证器

/// 用于生成云端请求所需的 `x-device-auth` 签名头
/// 自动校准本地时间与服务器时间的差值
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
        let client_id = generate_random_id(8, b"abcdefghijklmnopqrstuvwxyz0123456789");
        Self {
            client_provider: provider,
            authorization_token,
            client_id,
            time_difference: 0,
        }
    }

    pub fn new(authorization_token: Option<String>) -> Self {
        Self::new_with_provider(Box::new(GlobalClientProvider::new()), authorization_token)
    }

    fn client(&self) -> &CodeMaoClient {
        self.client_provider.client()
    }

    /// 获取校准后的时间戳(秒),首次调用会计算时差
    pub fn get_calibrated_timestamp(&mut self) -> MewResult<i64> {
        if self.time_difference == 0 {
            let server_time = fetch_current_timestamp_with_provider(&*self.client_provider)?;
            let local_time = current_timestamp_secs();
            self.time_difference = local_time - server_time;
        }
        let now = current_timestamp_secs();
        Ok(now - self.time_difference)
    }

    /// 生成 `x-device-auth` 头所需的 JSON 字符串
    pub fn generate_x_device_auth(&mut self) -> MewResult<String> {
        let timestamp = self.get_calibrated_timestamp()?;
        let sign_str = format!("{}{}{}", Self::CLIENT_SECRET, timestamp, self.client_id);
        let mut hasher = Sha256::new();
        hasher.update(sign_str.as_bytes());
        let result = hasher.finalize();
        // 预分配一次,避免逐字节堆分配;保持大写(已实测与官方客户端一致)
        let mut sign = String::with_capacity(64);
        use std::fmt::Write as _;
        for b in result.iter() {
            let _ = write!(sign, "{b:02X}");
        }

        let auth_json = json!({
            "sign": sign,
            "timestamp": timestamp,
            "client_id": self.client_id,
        });
        Ok(serde_json::to_string(&auth_json)?)
    }

    /// 获取当前的授权令牌(优先返回手动设置的,否则从客户端身份中获取)
    pub fn authorization_token(&self) -> Option<String> {
        if let Some(ref token) = self.authorization_token
            && !token.is_empty()
        {
            return Some(token.clone());
        }
        self.client().current_token()
    }

    pub fn set_authorization_token(&mut self, token: Option<String>) {
        self.authorization_token = token;
    }
}

// 链式调用构建器

/// 登录构建器,支持链式设置参数后执行
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
    timestamp: Option<i64>,
    captcha: Option<String>,
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
            timestamp: None,
            captcha: None,
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

    pub fn timestamp(mut self, val: i64) -> Self {
        self.timestamp = Some(val);
        self
    }

    pub fn captcha(mut self, val: impl Into<String>) -> Self {
        self.captcha = Some(val.into());
        self
    }

    /// 构建登录会话(不含网络请求,构造阶段无副作用)
    pub fn build(self) -> LoginSession {
        let credentials = LoginCredentials {
            identity: self.identity.unwrap_or_default(),
            password: self.password.unwrap_or_default(),
            token: self.token.unwrap_or_default(),
            pid: self.pid.unwrap_or_else(|| DEFAULT_PID.to_string()),
            status: self.status,
            role: self.role,
            timestamp: self.timestamp,
            captcha: self.captcha,
        };
        LoginSession {
            auth_manager: self.auth_manager,
            credentials,
            prefer_method: self.prefer_method,
        }
    }
}

impl Default for LoginBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 登录会话:由 [`LoginBuilder::build`] 构造,执行网络登录
#[derive(Debug)]
pub struct LoginSession {
    auth_manager: AuthManager,
    credentials: LoginCredentials,
    prefer_method: Option<LoginMethod>,
}

impl LoginSession {
    /// 执行登录(网络请求),返回登录结果
    pub fn execute(&mut self) -> MewResult<LoginResult> {
        self.auth_manager
            .login(&self.credentials, self.prefer_method)
    }
}

#[cfg(test)]
mod tests {
    use super::AdminInfo;
    use serde_json::json;

    #[test]
    fn admin_info_from_details_reads_fixed_fields() {
        let v =
            json!({"admin": {"id": 42, "username": "u1", "role_name": "r1", "full_name": "F1"}});
        let info = AdminInfo::from_details(&v).expect("固定字段应可提取");
        assert_eq!(info.id, 42);
        assert_eq!(info.username, "u1");
        assert_eq!(info.role_name, "r1");
        assert_eq!(info.full_name, "F1");
    }

    #[test]
    fn admin_info_from_details_rejects_missing_fields() {
        assert!(AdminInfo::from_details(&json!({"admin": {"id": 42}})).is_none());
        assert!(AdminInfo::from_details(&json!({"id": 42})).is_none());
        assert!(
            AdminInfo::from_details(
                &json!({"admin": {"id": "x", "username": "u", "role_name": "r", "full_name": "f"}})
            )
            .is_none()
        );
    }
}
