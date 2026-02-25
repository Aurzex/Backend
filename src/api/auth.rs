use crate::utils::acquire::{CodeMaoClient, HttpMethod};
use crate::utils::data::{CodeMaoFile, FileContent, PathConfig};
use rand::RngExt;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use ureq::http::HeaderValue;

// ==================== 枚举定义 ====================

// 登录方法枚举
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
#[derive(Clone)]
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

// 账号状态枚举
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
}

// ==================== 数据结构 ====================

// 登录凭证
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

// ==================== 辅助函数 ====================

// 获取当前服务器时间戳
pub fn fetch_current_timestamp(client: &CodeMaoClient) -> Result<i64, Box<dyn std::error::Error>> {
    let response = client.send_request(
        HttpMethod::GET,
        "/coconut/clouddb/currentTime",
        None,
        None,
        None,
    )?;
    let json = CodeMaoClient::response_to_json(response)?;
    Ok(json["data"].as_i64().unwrap_or(0))
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

pub struct AuthProcessor {
    client: Arc<Mutex<CodeMaoClient>>,
    client_secret: &'static str,
    captcha_img_path: PathBuf,
}

impl AuthProcessor {
    const CLIENT_SECRET: &'static str = "pBlYqXbJDu";

    pub fn new(client: Arc<Mutex<CodeMaoClient>>, captcha_img_path: PathBuf) -> Self {
        Self {
            client,
            client_secret: Self::CLIENT_SECRET,
            captcha_img_path: captcha_img_path,
        }
    }

    // 获取认证详情
    pub fn fetch_auth_details(&self, token: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let cookie_str = format!("authorization={}", token);
        let mut headers = HashMap::new();
        headers.insert("cookie".to_string(), cookie_str);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/web/users/details",
            None,
            None,
            None,
        )?;

        // 分步骤处理，让编译器更容易推断类型
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

        let json = CodeMaoClient::response_to_json(response)?;
        Ok(json)
    }

    // 获取登录票据
    pub fn get_login_ticket(
        &self,
        identity: &str,
        timestamp: i64,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "identity": identity,
            "pid": pid,
            "timestamp": timestamp,
        });

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "https://open-service.codemao.cn/captcha/rule/v3",
            None,
            Some(&payload),
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }
    pub fn get_login_security_info(
        &self,
        identity: &str,
        password: &str,
        ticket: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
            "agreement_ids": [-1],
        });

        // 创建包含 x-captcha-ticket 的 headers
        let mut headers = HashMap::new();
        headers.insert("x-captcha-ticket".to_string(), ticket.to_string());

        println!("--- 调用 Security API ---");
        println!("Payload: {}", serde_json::to_string_pretty(&payload)?);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/tiger/v3/web/accounts/login/security",
            None,
            Some(&payload),
            None,
        )?;

        // 检查状态码
        let status = response.status();
        println!("Security API Status: {}", status);

        if status != 200 {
            let body = response.into_body().read_to_string().unwrap_or_default();
            println!("Security API Error Body: {}", body);
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

        println!("Security API Body: {}", body);

        // 解析JSON
        let json_value: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                println!("JSON解析失败: {}", e);
                return Err(Box::new(e));
            }
        };

        println!(
            "Security API JSON: {}",
            serde_json::to_string_pretty(&json_value)?
        );

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
        let payload = json!({
            "username": username,
            "password": password,
            "key": key,
            "code": code,
        });

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/admins/login",
            None,
            Some(&payload),
            Some("whale"),
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取管理员验证码
    pub fn fetch_admin_captcha(
        &self,
        timestamp: i64,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let endpoint = format!("/admins/captcha/{}", timestamp);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            &endpoint,
            None,
            None,
            Some("whale"),
        )?;
        if response.status() == 200 {
            let _res = CodeMaoFile::file_write(
                &self.captcha_img_path,
                &FileContent::Bytes(response.into_body().read_to_vec().unwrap()),
                "b",
            );
            println!("验证码已保存至: {:?}", self.captcha_img_path.to_str());
        } else {
            println!("获取验证码失败, 错误代码: {}", response.status());
        }

        // 返回图片数据
        Ok(Vec::new()) // 实际应该从response获取
    }

    // 处理 v0 版本密码登录
    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
        });

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/tiger/accounts/login",
            None,
            Some(&payload),
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 处理 v1 版本密码登录
    pub fn handle_password_v1(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid,
        });

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/tiger/v3/web/accounts/login",
            None,
            Some(&payload),
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 处理 v2 版本密码登录
    pub fn handle_password_v2(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = fetch_current_timestamp(&self.client.lock().unwrap())?;
        let ticket_response = self.get_login_ticket(identity, timestamp, pid)?;
        println!("Ticket response: {:?}", ticket_response); // 你已经添加了这一行

        let ticket = ticket_response["ticket"].as_str().ok_or("无法获取ticket")?;

        // 调用第三个 API 并打印完整返回数据
        let security_response = self.get_login_security_info(identity, password, ticket, pid)?;
        println!(
            "Security API response: {}",
            serde_json::to_string_pretty(&security_response)?
        ); // <-- 添加这一行

        Ok(security_response)
    }
}

// ==================== 登录处理器 ====================

pub struct LoginHandler {
    client: Arc<Mutex<CodeMaoClient>>,
    processor: AuthProcessor,
}

impl LoginHandler {
    pub fn new(client: Arc<Mutex<CodeMaoClient>>, processor: AuthProcessor) -> Self {
        Self { client, processor }
    }

    // 处理 v0 版本密码登录
    pub fn handle_password_v0(
        &self,
        identity: &str,
        password: &str,
        pid: &str,
        status: &AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let _ = self.client.lock().unwrap().switch_identity("", "blank");

        match self.processor.handle_password_v0(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data.get("token").and_then(|t| t.as_str()) {
                    let _ = self
                        .client
                        .lock()
                        .unwrap()
                        .switch_identity(token, status.as_str());
                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV0, "v0 密码登录成功")
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
        status: &AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        self.client.lock().unwrap().switch_identity("", "blank");

        match self.processor.handle_password_v1(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    let _ = self
                        .client
                        .lock()
                        .unwrap()
                        .switch_identity(token, status.as_str());
                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV1, "v1 密码登录成功")
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
        status: &AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        self.client.lock().unwrap().switch_identity("", "blank");

        match self.processor.handle_password_v2(identity, password, pid) {
            Ok(data) => {
                if let Some(token) = data
                    .get("auth")
                    .and_then(|a| a.get("token"))
                    .and_then(|t| t.as_str())
                {
                    let _ = self
                        .client
                        .lock()
                        .unwrap()
                        .switch_identity(token, status.as_str());
                    Ok(
                        LoginResult::new(true, LoginMethod::PasswordV2, "v2 密码登录成功")
                            .with_data(data),
                    )
                } else {
                    Ok(
                        LoginResult::new(false, LoginMethod::PasswordV2, "v2 密码登录失败")
                            .with_data(data),
                    )
                }
            }
            Err(e) => Ok(LoginResult::new(
                false,
                LoginMethod::PasswordV2,
                &format!("登录失败: {}", e),
            )),
        }
    }

    // 处理 token 登录
    pub fn handle_token(
        &self,
        token: &str,
        status: &AccountStatus,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let auth_details = self.processor.fetch_auth_details(token)?;
        let _ = self
            .client
            .lock()
            .unwrap()
            .switch_identity(token, status.as_str());

        Ok(LoginResult::new(true, LoginMethod::Token, "Token 登录成功")
            .with_token(token)
            .with_auth_details(auth_details))
    }

    // 处理管理员 token 登录
    pub fn handle_admin_token(
        &self,
        token: Option<&str>,
    ) -> Result<LoginResult, Box<dyn std::error::Error>> {
        let token = match token {
            Some(t) => t.to_string(),
            None => {
                // 在实际应用中，这里应该从其他地方获取token
                println!("请输入 Authorization Token:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                input.trim().to_string()
            }
        };

        let _ = self
            .client
            .lock()
            .unwrap()
            .switch_identity(&token, "judgement");
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
                        let _ = self
                            .client
                            .lock()
                            .unwrap()
                            .switch_identity(token, "judgement");
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

// ==================== 认证管理器 ====================

pub struct AuthManager {
    client: Arc<Mutex<CodeMaoClient>>,
    processor: AuthProcessor,
    handler: LoginHandler,
    current_credentials: Option<LoginCredentials>,
}

impl AuthManager {
    pub fn new() -> Self {
        let client = Arc::new(Mutex::new(CodeMaoClient::new()));
        let processor = AuthProcessor::new(client.clone(), PathConfig::captcha_file_path());
        let handler = LoginHandler::new(client.clone(), processor.clone());

        Self {
            client,
            processor,
            handler,
            current_credentials: None,
        }
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

        let role = credentials.role.clone(); // 先取出 role
        self.current_credentials = Some(credentials); // 再移动

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
        .map(|s| s.to_string()) // 将静态 &str 转为 String
    }

    // 修复生命周期问题：get_admin_login_method 返回 String
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

    // 修改 user_login 方法以使用 String
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
                &credentials.status,
            ),
            "password_v1" => self.handler.handle_password_v1(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                &credentials.status,
            ),
            "password_v2" => self.handler.handle_password_v2(
                &credentials.identity,
                &credentials.password,
                &credentials.pid,
                &credentials.status,
            ),
            "token" => self
                .handler
                .handle_token(&credentials.token, &credentials.status),
            _ => Err(format!("不支持的登录方式: {}", method).into()),
        }
    }

    // 修改 admin_login 方法以使用 String
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
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/tiger/accounts/logout",
            None,
            Some(&json!({})),
            None,
        )?;
        Ok(response.status() == 204)
    }

    // 执行 v12 版本用户登出
    pub fn execute_logout_v12(&self, method: &str) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("/tiger/v3/{}/accounts/logout", method);
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            &endpoint,
            None,
            Some(&json!({})),
            None,
        )?;
        Ok(response.status() == 204)
    }

    // 管理员登出
    pub fn admin_logout(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::DELETE,
            "/admins/logout",
            None,
            None,
            Some("whale"),
        )?;
        Ok(response.status() == 204)
    }

    // 获取管理员仪表板数据
    pub fn fetch_admin_dashboard_data(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/admins/info",
            None,
            None,
            Some("whale"),
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 配置认证 Token
    pub fn configure_authentication_token(&self, token: &str, identity: &str) {
        let _ = self.client.lock().unwrap().switch_identity(token, identity);
    }

    // 获取当前客户端
    pub fn get_current_client(&self) -> Arc<Mutex<CodeMaoClient>> {
        self.client.clone()
    }

    // 获取当前登录凭证
    pub fn get_current_credentials(&self) -> Option<&LoginCredentials> {
        self.current_credentials.as_ref()
    }
}

// ==================== 云服务认证器 ====================

pub struct CloudAuthenticator {
    authorization_token: Option<String>,
    client_id: String,
    time_difference: i64,
    client: Arc<Mutex<CodeMaoClient>>,
    client_secret: &'static str,
}

impl CloudAuthenticator {
    const CLIENT_SECRET: &'static str = "pBlYqXbJDu";

    pub fn new(authorization_token: Option<String>) -> Self {
        let client_id = Self::generate_client_id(8);

        Self {
            authorization_token,
            client_id,
            time_difference: 0,
            client: Arc::new(Mutex::new(CodeMaoClient::new())),
            client_secret: Self::CLIENT_SECRET,
        }
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
            let server_time = fetch_current_timestamp(&self.client.lock().unwrap())?;
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

// 实现Clone trait以便在需要时复制AuthProcessor
impl Clone for AuthProcessor {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            client_secret: self.client_secret,
            captcha_img_path: self.captcha_img_path.clone(),
        }
    }
}
