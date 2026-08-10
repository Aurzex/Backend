use crate::utils::acquire::{BaseKey, ClientAccess, CodeMaoClient, HTTPStatus, HttpMethod, MewResult};
use log::debug;
use serde_json::{Value, json};

/// 海外平台数据访问客户端
/// 提供获取 Tiger 账号信息和平台配置的能力
pub struct OverseaDataClient {
    client: &'static CodeMaoClient,
}

impl OverseaDataClient {
    /// 创建新实例,使用全局客户端
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取 Tiger 账号信息列表
    pub fn fetch_tiger_accounts(&self) -> MewResult<Value> {
        debug!("获取海外 Tiger 账号信息");
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, "/tiger/accounts", Some(BaseKey::CodeGame)),
        )
    }

    /// 获取海外平台配置信息
    pub fn fetch_platform_config(&self) -> MewResult<Value> {
        debug!("获取海外平台配置");
        self.send_and_parse(
            self.client
                .build_request(HttpMethod::Get, "/config", Some(BaseKey::CodeGame)),
        )
    }
}

impl Default for OverseaDataClient {
    fn default() -> Self {
        Self::new()
    }
}

/// 语言类型枚举(当前仅支持英语)
#[derive(Debug, Clone, Copy, Default)]
pub enum Language {
    #[default]
    En,
}

impl Language {
    /// 获取语言对应的 API 字符串标识
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::En => "en",
        }
    }
}

/// 用户操作处理器
/// 负责海外平台的注册与登录功能
pub struct UserActionHandler {
    client: &'static CodeMaoClient,
}

impl UserActionHandler {
    /// 默认产品标识
    const DEFAULT_PID: &'static str = "LHnQoPMr";

    /// 创建新实例,使用全局客户端
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 通过邮箱注册新账号
    pub fn register_with_email(
        &self,
        email: &str,
        password: &str,
        pid: Option<&str>,
        language: Option<Language>,
    ) -> MewResult<bool> {
        let language_str = language.unwrap_or_default().as_str();
        let pid_val = pid.unwrap_or(Self::DEFAULT_PID);
        let payload = json!({
            "email": email,
            "language": language_str,
            "password": password,
            "pid": pid_val,
        });

        debug!(
            "邮箱注册 - email: {}, language: {}, pid: {}",
            email, language_str, pid_val
        );
        self.check_status(
            self.client
                .build_request(
                    HttpMethod::Post,
                    "/tiger/accounts/register/email",
                    Some(BaseKey::CodeGame),
                )
                .with_payload(payload),
            HTTPStatus::Created,
        )
    }

    /// 使用身份(邮箱或用户名)和密码登录
    pub fn authenticate_with_credentials(
        &self,
        identity: &str,
        password: &str,
        pid: Option<&str>,
    ) -> MewResult<bool> {
        let pid_val = pid.unwrap_or(Self::DEFAULT_PID);
        let payload = json!({
            "identity": identity,
            "password": password,
            "pid": pid_val,
        });

        debug!("身份登录 - identity: {}, pid: {}", identity, pid_val);
        self.check_status(
            self.client
                .build_request(
                    HttpMethod::Post,
                    "/tiger/accounts/login",
                    Some(BaseKey::CodeGame),
                )
                .with_payload(payload),
            HTTPStatus::Ok,
        )
    }
}

impl Default for UserActionHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for OverseaDataClient {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for UserActionHandler {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}
