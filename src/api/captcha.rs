use crate::utils::requests::{ClientAccess, CodeMaoClient, HttpMethod, MewResult};
use log::debug;
use serde_json::{Value, json};

/// 人机验证接口(极验/阿里云/网易/腾讯/防水墙等)
/// 对应 OpenAPI 中「人机验证」分组
pub struct CaptchaManager {
    client: CodeMaoClient,
}

impl CaptchaManager {
    pub fn new() -> Self {
        Self::new_with_client(CodeMaoClient::global().clone())
    }

    pub fn new_with_client(client: CodeMaoClient) -> Self {
        Self { client }
    }

    /// 获取验证码规则
    pub fn fetch_captcha_rule(&self) -> MewResult<Value> {
        debug!("获取验证码规则");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/captcha/rule", None);
        self.send_and_parse(builder)
    }

    /// 阿里云验证码校验
    pub fn verify_aliyun_captcha(&self) -> MewResult<Value> {
        debug!("校验阿里云验证码");
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/captcha/aliyun", None);
        self.send_and_parse(builder)
    }

    /// 极验验证码注册(获取挑战参数)
    pub fn register_geetest_captcha(&self) -> MewResult<Value> {
        debug!("注册极验验证码");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/captcha/geetest/register", None);
        self.send_and_parse(builder)
    }

    /// 极验验证码校验
    pub fn verify_geetest_captcha(
        &self,
        challenge: &str,
        validate: &str,
        seccode: &str,
    ) -> MewResult<Value> {
        debug!("校验极验验证码");
        let payload = json!({
            "geetest_challenge": challenge,
            "geetest_validate": validate,
            "geetest_seccode": seccode,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/captcha/geetest/verify", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 网易易盾验证码校验
    pub fn verify_netease_captcha(&self) -> MewResult<Value> {
        debug!("校验网易易盾验证码");
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/captcha/netease", None);
        self.send_and_parse(builder)
    }

    /// NextData 验证码注册
    pub fn register_nextdata_captcha(&self) -> MewResult<Value> {
        debug!("注册 NextData 验证码");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/captcha/nextdata", None);
        self.send_and_parse(builder)
    }

    /// 腾讯验证码校验
    pub fn verify_tencent_captcha(&self) -> MewResult<Value> {
        debug!("校验腾讯验证码");
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/captcha/tencent", None);
        self.send_and_parse(builder)
    }

    /// 获取图形验证码规则
    pub fn fetch_graph_captcha_rule(&self) -> MewResult<Value> {
        debug!("获取图形验证码规则");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/tiger/captcha/graph/rule", None);
        self.send_and_parse(builder)
    }

    /// 极验滑块验证码注册
    pub fn register_geetest_slide_captcha(&self) -> MewResult<Value> {
        debug!("注册极验滑块验证码");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/tiger/captcha/graph/geetest/register_slide",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 极验滑块票据校验
    pub fn verify_geetest_slide_ticket(&self, ticket: Value) -> MewResult<Value> {
        debug!("校验极验滑块票据");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/captcha/graph/tickets/geetest",
                None,
            )
            .with_payload(ticket);
        self.send_and_parse(builder)
    }

    /// 防水墙票据校验
    pub fn verify_waterproof_wall_ticket(&self, ticket: Value) -> MewResult<Value> {
        debug!("校验防水墙票据");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/captcha/graph/tickets/waterproof-wall",
                None,
            )
            .with_payload(ticket);
        self.send_and_parse(builder)
    }
}

impl Default for CaptchaManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for CaptchaManager {
    fn client(&self) -> &CodeMaoClient {
        &self.client
    }
}
