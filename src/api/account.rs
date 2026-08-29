use crate::utils::requests::{
    ClientAccess, CodeMaoClient, DEFAULT_PID, HTTPStatus, HttpMethod, MewResult, ResponseMode,
};
use log::debug;
use serde_json::{Value, json};

// 账号相关枚举

/// 性别
#[derive(Debug, Clone, Copy)]
pub enum Gender {
    Female = 0,
    Male = 1,
}

/// 平台类型
#[derive(Debug, Clone, Copy)]
pub enum PlatformMethod {
    Web,
    App,
}

impl PlatformMethod {
    fn as_str(&self) -> &'static str {
        match self {
            PlatformMethod::Web => "web",
            PlatformMethod::App => "app",
        }
    }
}

// 数据结构

/// 更新个人资料详细信息的参数
pub struct UpdateProfileDetailsArgs<'a> {
    pub avatar_url: &'a str,
    pub nickname: &'a str,
    pub birthday: i32,
    pub description: &'a str,
    pub fullname: &'a str,
    pub qq: &'a str,
    pub sex: Gender,
}

/// 账号管理接口(验证码、注册、资料、OAuth、手机号、密码、令牌等)
/// 对应 OpenAPI 中「用户 - 认证」分组下的 `/tiger/v3/web/accounts/*` 端点
pub struct AccountManager {
    client: CodeMaoClient,
}

impl AccountManager {
    pub fn new() -> Self {
        Self::new_with_client(CodeMaoClient::global().clone())
    }

    pub fn new_with_client(client: CodeMaoClient) -> Self {
        Self { client }
    }

    // ---- 验证码发送与校验 ----

    /// 发送通用验证码
    pub fn send_universal_captcha(&self, target: &str, pid: &str) -> MewResult<Value> {
        debug!("发送通用验证码: target={}", target);
        let payload = json!({ "target": target, "pid": pid });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/common",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 校验通用验证码
    pub fn verify_universal_captcha(&self, target: &str, captcha: &str) -> MewResult<Value> {
        debug!("校验通用验证码: target={}", target);
        let payload = json!({ "target": target, "captcha": captcha });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/common/check",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发起登录验证码流程
    pub fn send_login_captcha(&self) -> MewResult<Value> {
        debug!("发起登录验证码流程");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/login",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 登录验证码后处理
    pub fn login_captcha_post_process(&self) -> MewResult<Value> {
        debug!("登录验证码后处理");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/login/post-process",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 静默登录验证码
    pub fn login_captcha_silence(&self) -> MewResult<Value> {
        debug!("静默登录验证码");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/login/silence",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 第三方登录绑定手机号(端点名为 `oaut`,与官方一致)
    pub fn bind_phone_for_third_party(&self) -> MewResult<Value> {
        debug!("第三方登录绑定手机号");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/oaut",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 发送 OAuth 绑定手机号验证码
    pub fn send_oauth_bind_phone_captcha(&self, target: &str, pid: &str) -> MewResult<Value> {
        debug!("发送OAuth绑定手机号验证码: target={}", target);
        let payload = json!({ "target": target, "pid": pid });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/oauth",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 校验重置密码令牌
    pub fn verify_reset_password_token(&self, target: &str, captcha: &str) -> MewResult<Value> {
        debug!("校验重置密码令牌: target={}", target);
        let payload = json!({ "target": target, "captcha": captcha });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/password/check",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发送重置密码验证码
    pub fn send_reset_password_captcha(&self, target: &str, pid: &str) -> MewResult<Value> {
        debug!("发送重置密码验证码: target={}", target);
        let payload = json!({ "target": target, "pid": pid });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/password/reset",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发送修改密码验证码
    pub fn send_change_password_captcha(&self) -> MewResult<Value> {
        debug!("发送修改密码验证码");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/password/update",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 发送绑定手机号验证码
    pub fn send_bind_phone_captcha(&self, target: &str, pid: &str) -> MewResult<Value> {
        debug!("发送绑定手机号验证码: target={}", target);
        let payload = json!({ "target": target, "pid": pid });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/phone/bind",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 注册发送验证码
    pub fn register_send_captcha(&self) -> MewResult<Value> {
        debug!("注册发送验证码");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/register/phone",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 发送注册验证码(带协议)
    pub fn send_register_captcha(&self, target: &str, pid: &str) -> MewResult<Value> {
        debug!("发送注册验证码: target={}", target);
        let payload = json!({ "target": target, "pid": pid });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/register/phone/with-agreement",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发送语音验证码
    pub fn send_voice_captcha(&self, phone: &str) -> MewResult<Value> {
        debug!("发送语音验证码: phone={}", phone);
        let payload = json!({ "phone": phone });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/voice/captcha/send",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发送邮箱验证码
    pub fn send_email_captcha(&self, target: &str, pid: &str) -> MewResult<Value> {
        debug!("发送邮箱验证码: target={}", target);
        let payload = json!({ "target": target, "pid": pid });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/email/captcha/send",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 校验邮箱验证码
    pub fn verify_email_captcha(&self, target: &str, captcha: &str) -> MewResult<Value> {
        debug!("校验邮箱验证码: target={}", target);
        let payload = json!({ "target": target, "captcha": captcha });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/email/captcha/check",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    // ---- 注册 / 资料 ----

    /// 邮箱注册账号
    pub fn register_by_email(
        &self,
        pid: &str,
        email: &str,
        password: &str,
        captcha: &str,
    ) -> MewResult<Value> {
        debug!("邮箱注册账号: email={}", email);
        let payload = json!({
            "pid": pid,
            "email": email,
            "password": password,
            "captcha": captcha,
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/email/register",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 手机号注册账号(不带协议)
    pub fn register_by_phone(&self) -> MewResult<Value> {
        debug!("手机号注册账号");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/register/phone",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 手机号注册账号(带协议)
    pub fn create_account(
        &self,
        identity: &str,
        password: &str,
        captcha: &str,
        pid: Option<&str>,
        agreement_ids: Option<Vec<i32>>,
    ) -> MewResult<Value> {
        debug!("注册账号: identity={}", identity);
        let payload = json!({
            "identity": identity,
            "password": password,
            "captcha": captcha,
            "pid": pid.unwrap_or(DEFAULT_PID),
            "agreement_ids": match agreement_ids {
                Some(ids) => ids.into_iter().map(Value::from).collect::<Vec<_>>(),
                None => vec![Value::from(186), Value::from(13)],
            },
        });

        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/register/phone/with-agreement",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 更新个人资料详细信息
    pub fn update_profile_details(&self, args: UpdateProfileDetailsArgs<'_>) -> MewResult<bool> {
        debug!("更新个人资料: nickname={}", args.nickname);
        let payload = json!({
            "avatar_url": args.avatar_url,
            "nickname": args.nickname,
            "birthday": args.birthday,
            "description": args.description,
            "fullname": args.fullname,
            "qq": args.qq,
            "sex": args.sex as i32,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Patch, "/tiger/v3/web/accounts/info", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 获取平台账号资料
    pub fn fetch_account_platform_profile(&self, method: PlatformMethod) -> MewResult<Value> {
        debug!("获取平台账号资料: method={:?}", method);
        let endpoint = format!("/tiger/v3/{}/accounts/profile", method.as_str());
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取账号隐私设置
    pub fn fetch_account_privacy(&self) -> MewResult<Value> {
        debug!("获取账号隐私设置");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/tiger/v3/web/accounts/privacy", None);
        self.send_and_parse(builder)
    }

    /// 获取年级列表
    pub fn fetch_grade_list(&self) -> MewResult<Value> {
        debug!("获取年级列表");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/tiger/v3/web/accounts/grade-list", None);
        self.send_and_parse(builder)
    }

    /// 获取协议列表
    pub fn fetch_protocol_list(&self) -> MewResult<Value> {
        debug!("获取协议列表");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/tiger/v3/web/accounts/protocol/list",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 获取用户协议列表
    pub fn fetch_agreements(&self) -> MewResult<Value> {
        debug!("获取用户协议");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/tiger/v3/web/accounts/agreements", None);
        self.send_and_parse(builder)
    }

    /// 获取待签署协议列表
    pub fn fetch_agreements_need_sign(&self) -> MewResult<Value> {
        debug!("获取待签署协议列表");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/tiger/v3/web/accounts/agreements/need-sign",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 签署协议
    pub fn sign_agreements(&self, agreement_ids: Vec<i32>) -> MewResult<Value> {
        debug!("签署协议: ids={:?}", agreement_ids);
        let payload = json!({ "agreement_ids": agreement_ids });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/agreements/sign",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    // ---- OAuth ----

    /// QQ OAuth 登录
    pub fn login_by_qq(&self, code: &str, redirect_uri: Option<&str>) -> MewResult<Value> {
        debug!("QQ OAuth 登录");
        let mut payload = serde_json::Map::new();
        payload.insert("code".to_string(), Value::String(code.to_string()));
        if let Some(uri) = redirect_uri {
            payload.insert("redirect_uri".to_string(), Value::String(uri.to_string()));
        }
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/tiger/v3/web/accounts/oauth/qq", None)
            .with_payload(Value::Object(payload));
        self.send_and_parse(builder)
    }

    /// 绑定 QQ
    pub fn bind_qq(&self, code: &str) -> MewResult<Value> {
        debug!("绑定QQ");
        let payload = json!({ "code": code });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/oauth/qq/bind",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 解绑 QQ(DELETE 或 POST)
    pub fn unbind_qq(&self, method: HttpMethod) -> MewResult<Value> {
        debug!("解绑QQ: method={:?}", method);
        let builder =
            self.client
                .build_request(method, "/tiger/v3/web/accounts/oauth/qq/unbind", None);
        self.send_and_parse(builder)
    }

    /// 微信 OAuth 登录
    pub fn login_by_wechat(&self, code: &str) -> MewResult<Value> {
        debug!("微信 OAuth 登录");
        let payload = json!({ "code": code });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/oauth/wechat",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 绑定微信
    pub fn bind_wechat(&self, code: &str) -> MewResult<Value> {
        debug!("绑定微信");
        let payload = json!({ "code": code });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/oauth/wechat/bind",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 解绑微信(DELETE 或 POST)
    pub fn unbind_wechat(&self, method: HttpMethod) -> MewResult<Value> {
        debug!("解绑微信: method={:?}", method);
        let builder =
            self.client
                .build_request(method, "/tiger/v3/web/accounts/oauth/wechat/unbind", None);
        self.send_and_parse(builder)
    }

    /// 第三方登录创建用户
    pub fn create_user_for_third_party(
        &self,
        third_party_token: &str,
        phone: Option<&str>,
        captcha: Option<&str>,
    ) -> MewResult<Value> {
        debug!("第三方登录创建用户");
        let mut payload = serde_json::Map::new();
        payload.insert(
            "third_party_token".to_string(),
            Value::String(third_party_token.to_string()),
        );
        if let Some(p) = phone {
            payload.insert("phone".to_string(), Value::String(p.to_string()));
        }
        if let Some(c) = captcha {
            payload.insert("captcha".to_string(), Value::String(c.to_string()));
        }
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/oauth/third-party",
                None,
            )
            .with_payload(Value::Object(payload));
        self.send_and_parse(builder)
    }

    /// 获取 OAuth 绑定列表
    pub fn fetch_oauth_bindings(&self) -> MewResult<Value> {
        debug!("获取OAuth绑定列表");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/tiger/v3/web/accounts/oauths", None);
        self.send_and_parse(builder)
    }

    // ---- 手机号 ----

    /// 绑定手机号(PUT 或 POST)
    pub fn bind_phone(&self, method: HttpMethod, phone: &str, captcha: &str) -> MewResult<Value> {
        debug!("绑定手机号: phone={}", phone);
        let payload = json!({ "phone": phone, "captcha": captcha });
        let builder = self
            .client
            .build_request(method, "/tiger/v3/web/accounts/phone/bind", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 校验手机号是否存在
    pub fn check_phone(&self, phone: &str) -> MewResult<Value> {
        debug!("校验手机号: phone={}", phone);
        let payload = json!({ "phone": phone });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/tiger/v3/web/accounts/phone/check", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 手机号登录
    pub fn login_by_phone(&self) -> MewResult<Value> {
        debug!("手机号登录");
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/tiger/v3/web/accounts/phone/login", None)
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 手机号登录后处理
    pub fn phone_login_post_process(&self) -> MewResult<Value> {
        debug!("手机号登录后处理");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/phone/login/post-process",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 静默手机号登录
    pub fn phone_login_silence(&self) -> MewResult<Value> {
        debug!("静默手机号登录");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/phone/login/silence",
                None,
            )
            .with_payload(json!({}));
        self.send_and_parse(builder)
    }

    /// 更新手机号码
    pub fn update_phone_number(&self, captcha: &str, phonenum: &str) -> MewResult<Value> {
        debug!("更新手机号: phonenum={}", phonenum);
        let payload = json!({
            "phone_number": phonenum,
            "captcha": captcha,
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Patch,
                "/tiger/v3/web/accounts/phone/change",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 验证手机号码是否一致
    pub fn validate_phone_number(&self, phone_num: &str) -> MewResult<Value> {
        debug!("验证手机号: phone_num={}", phone_num);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/web/users/phone_number/is_consistent",
                None,
            )
            .with_param("phone_number", phone_num);
        self.send_and_parse(builder)
    }

    /// 请求更换手机号验证码
    pub fn request_phone_change_verification(
        &self,
        old_phonenum: &str,
        new_phonenum: &str,
    ) -> MewResult<bool> {
        debug!(
            "请求更换手机号验证码: old={}, new={}",
            old_phonenum, new_phonenum
        );
        let payload = json!({
            "phone_number": new_phonenum,
            "old_phone_number": old_phonenum,
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/captcha/phone/change",
                None,
            )
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    // ---- 密码 ----

    /// 修改密码
    pub fn update_password(&self, old_password: &str, new_password: &str) -> MewResult<bool> {
        debug!("修改密码");
        let payload = json!({
            "old_password": old_password,
            "password": new_password,
            "confirm_password": new_password,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Patch, "/tiger/v3/web/accounts/password", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 通过手机号修改密码(PUT 或 POST)
    pub fn change_password_by_phone(
        &self,
        method: HttpMethod,
        phone: &str,
        captcha: &str,
        new_password: &str,
    ) -> MewResult<Value> {
        debug!("通过手机号修改密码: phone={}", phone);
        let payload = json!({
            "phone": phone,
            "captcha": captcha,
            "new_password": new_password,
        });
        let builder = self
            .client
            .build_request(method, "/tiger/v3/web/accounts/password/phone", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 重置密码
    pub fn reset_password(
        &self,
        phone: &str,
        captcha: &str,
        new_password: &str,
    ) -> MewResult<Value> {
        debug!("重置密码: phone={}", phone);
        let payload = json!({
            "phone": phone,
            "captcha": captcha,
            "new_password": new_password,
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/password/reset",
                None,
            )
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 初始化密码(PUT 或 POST)
    pub fn init_password(&self, method: HttpMethod, password: &str) -> MewResult<Value> {
        debug!("初始化密码");
        let payload = json!({ "password": password });
        let builder = self
            .client
            .build_request(method, "/tiger/v3/web/accounts/password/setting", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    // ---- 令牌 ----

    /// 删除令牌
    pub fn delete_token(&self) -> MewResult<Value> {
        debug!("删除令牌");
        let builder =
            self.client
                .build_request(HttpMethod::Delete, "/tiger/v3/web/accounts/tokens", None);
        self.send_and_parse(builder)
    }

    /// 旧 Cookie 令牌转换
    pub fn convert_old_cookie_token(&self) -> MewResult<Value> {
        debug!("转换旧Cookie令牌");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/tiger/v3/web/accounts/tokens/convert",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 刷新令牌
    pub fn refresh_token(&self) -> MewResult<Value> {
        debug!("刷新令牌");
        let builder = self.client.build_request(
            HttpMethod::Post,
            "/tiger/v3/web/accounts/tokens/refresh",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 设置用户名(PUT 或 PATCH)
    pub fn set_username(&self, method: HttpMethod, username: &str) -> MewResult<Value> {
        debug!("设置用户名: username={}", username);
        let payload = json!({ "username": username });
        let builder = self
            .client
            .build_request(method, "/tiger/v3/web/accounts/username", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    // ---- 注销 ----

    /// 注销用户
    pub fn delete_user(&self, reason: &str, mode: ResponseMode) -> MewResult<Value> {
        debug!("注销用户: reason={}", reason);
        let payload = json!({ "closeReason": reason });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/tiger/v3/web/accounts/close", None)
            .with_payload(payload);
        self.send_maybe_parse(builder, mode, HTTPStatus::Ok)
    }
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for AccountManager {
    fn client(&self) -> &CodeMaoClient {
        &self.client
    }
}

#[cfg(test)]
mod tests {
    use super::AccountManager;
    use crate::utils::requests::{Catsona, ClientAccess, CodeMaoClient, KittyConfig};

    #[test]
    fn manager_new_with_client_uses_injected_client() {
        let a = CodeMaoClient::new_independent(KittyConfig::default());
        let b = CodeMaoClient::new_independent(KittyConfig::default());
        let m = AccountManager::new_with_client(a.clone());
        a.set_token(Catsona::Fluffy, "tok-a").unwrap();
        assert_eq!(m.client().current_token().as_deref(), Some("tok-a"));
        assert_eq!(b.current_token(), None); // b 独立,不受影响
    }
}
