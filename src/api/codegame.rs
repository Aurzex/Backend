use crate::utils::acquire::{CodeMaoClient, HttpMethod, MewError, MewResult};
use serde_json::{Value, json};

/// 语言类型枚举
#[derive(Debug, Clone, Copy, Default)]
pub enum Language {
    #[default]
    En,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::En => "en",
        }
    }
}

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
    pub async fn fetch_tiger_accounts(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "https://oversea-api.code.game/tiger/accounts",
                None,
            )
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取平台配置信息
    pub async fn fetch_platform_config(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "https://oversea-api.code.game/config",
                None,
            )
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }
}

impl Default for OverseaDataClient {
    fn default() -> Self {
        Self::new()
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
    pub async fn register_with_email(
        &self,
        email: &str,
        password: &str,
        pid: Option<&str>,
        language: Option<Language>,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "https://oversea-api.code.game/tiger/accounts/register/email",
                None,
            )
            .with_payload(json!({
                "email": email,
                "language": language.unwrap_or_default().as_str(),
                "password": password,
                "pid": pid.unwrap_or(Self::DEFAULT_PID),
            }))
            .send()
            .await?;

        Ok(response.status().as_u16() == 201)
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
    pub async fn authenticate_with_credentials(
        &self,
        identity: &str,
        password: &str,
        pid: Option<&str>,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "https://oversea-api.code.game/tiger/accounts/login",
                None,
            )
            .with_payload(json!({
                "identity": identity,
                "password": password,
                "pid": pid.unwrap_or(Self::DEFAULT_PID),
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }
}

impl Default for UserActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
