use crate::utils::acquire::{CodeMaoClient, HTTPStatus, HttpMethod, MewResult};
use serde_json::{Value, json};

/// 海外平台数据访问客户端
pub struct OverseaDataClient {
    client: &'static CodeMaoClient,
}

impl OverseaDataClient {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取 Tiger 账号信息
    pub fn fetch_tiger_accounts(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://oversea-api.code.game/tiger/accounts",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取平台配置信息
    pub fn fetch_platform_config(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://oversea-api.code.game/config",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for OverseaDataClient {
    fn default() -> Self {
        Self::new()
    }
}

/// 语言类型枚举
#[derive(Debug, Clone, Copy, Default)]
pub enum Language {
    #[default]
    En,
}

impl Language {
    fn as_str(&self) -> &'static str {
        match self {
            Language::En => "en",
        }
    }
}

/// 用户操作处理器
pub struct UserActionHandler {
    client: &'static CodeMaoClient,
}

impl UserActionHandler {
    const DEFAULT_PID: &'static str = "LHnQoPMr";

    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 通过邮箱注册账号
    ///
    /// # Arguments
    /// * `email` - 用户邮箱
    /// * `password` - 账号密码
    /// * `pid` - 产品 ID, 默认 "LHnQoPMr"
    /// * `language` - 语言, 目前仅支持 "en"
    ///
    /// # Returns
    /// 注册成功返回 true, 否则返回 false
    pub fn register_with_email(
        &self,
        email: &str,
        password: &str,
        pid: Option<&str>,
        language: Option<Language>,
    ) -> MewResult<bool> {
        let payload = json!({
            "email": email,
            "language": language.unwrap_or_default().as_str(),
            "password": password,
            "pid": pid.unwrap_or(Self::DEFAULT_PID),
        });

        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://oversea-api.code.game/tiger/accounts/register/email",
                None,
            )
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Created as u16)
    }

    /// 使用账号密码登录
    ///
    /// # Arguments
    /// * `identity` - 身份标识 (邮箱或用户名)
    /// * `password` - 账号密码
    /// * `pid` - 产品 ID, 默认 "LHnQoPMr"
    ///
    /// # Returns
    /// 登录成功返回 true, 否则返回 false
    pub fn authenticate_with_credentials(
        &self,
        identity: &str,
        password: &str,
        pid: Option<&str>,
    ) -> MewResult<bool> {
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid.unwrap_or(Self::DEFAULT_PID),
        });

        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "https://oversea-api.code.game/tiger/accounts/login",
                None,
            )
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }
}

impl Default for UserActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
