use crate::utils::acquire::{BaseKey, CodeMaoClient, HttpMethod, PaginatedIter};
use serde_json::{Value, json};

// ==================== 用户相关枚举 ====================

// 作品类型枚举
#[repr(i32)]
pub enum WorkType {
    Kitten = 1,
    Nemo = 3,
    CodeGame = 5,
}

impl WorkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkType::Kitten => "1",
            WorkType::Nemo => "3",
            WorkType::CodeGame => "5",
        }
    }
}

// 作品列表类型枚举
pub enum WorksListType {
    Newest,
    Hot,
}

impl WorksListType {
    fn as_str(&self) -> &'static str {
        match self {
            WorksListType::Newest => "newest",
            WorksListType::Hot => "hot",
        }
    }
}

// 平台类型枚举
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

// 云端作品类型枚举
pub enum CloudWorkType {
    Nemo,
    Kitten,
}

impl CloudWorkType {
    fn as_value(&self) -> i32 {
        match self {
            CloudWorkType::Nemo => 8,
            CloudWorkType::Kitten => 1,
        }
    }
}

// 作品发布状态枚举
pub enum PublishStatus {
    Published,
    Unpublished,
    All,
}

impl PublishStatus {
    fn as_str(&self) -> &'static str {
        match self {
            PublishStatus::Published => "PUBLISHED",
            PublishStatus::Unpublished => "UNPUBLISHED",
            PublishStatus::All => "all",
        }
    }
}

// Kitten版本枚举
pub enum KittenVersion {
    V4,
    V3,
}

impl KittenVersion {
    fn as_str(&self) -> &'static str {
        match self {
            KittenVersion::V4 => "KITTEN_V4",
            KittenVersion::V3 => "KITTEN_V3",
        }
    }
}

// 作品显示状态枚举
pub enum WorkShowStatus {
    Show,
}

impl WorkShowStatus {
    fn as_str(&self) -> &'static str {
        match self {
            WorkShowStatus::Show => "SHOW",
        }
    }
}

// 性别枚举
#[repr(i32)]
pub enum Gender {
    Female = 0,
    Male = 1,
}

// 头像框ID枚举
#[repr(i32)]
pub enum AvatarFrameId {
    Lv2 = 2,
    Lv3 = 3,
    Lv4 = 4,
}

// ==================== 用户数据获取器 ====================
pub struct UserDataFetcher {
    client: &'static CodeMaoClient,
}

impl UserDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取用户详细信息
    pub fn fetch_user_profile(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/user/info/detail/{}", user_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户 Tiger 信息
    pub fn fetch_user_tiger(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/tiger/user/{}", user_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户 info 信息
    pub fn fetch_user_info(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/api/user/info/detail/{}", user_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户荣誉信息
    pub fn fetch_user_honors(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/user/center/honor",
                None,
            )
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户业务数据指标
    pub fn fetch_user_metrics(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v2/works/business/total", None)
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户基本信息
    pub fn fetch_user_intro(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v2/user/dynamic/info", None)
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户动态信息
    pub fn fetch_user_dynamic(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/user/dynamic/{}", user_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户加入的工作室信息
    pub fn fetch_user_studio(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/work-shops/{}/participators", user_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户年度总结
    pub fn fetch_user_annual_summary(
        &self,
        user_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/activities/annual-summary", None)
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取账户信息 (简略)
    pub fn get_account_info(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/api/user/info", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取当前账号详细信息
    pub fn fetch_account_details(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/users/details", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取平台账号资料
    pub fn fetch_account_platform_profile(
        &self,
        method: PlatformMethod,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/tiger/v3/{}/accounts/profile", method.as_str());
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取账号隐私设置
    pub fn fetch_account_privacy(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/tiger/v3/web/accounts/privacy", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取 Tiger 账号信息
    pub fn fetch_account_tiger(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/tiger/user", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户评分数据
    pub fn fetch_account_scores(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v3/user/grade/details", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户等级信息
    pub fn fetch_account_level(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v3/user/level/info", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户动态信息
    pub fn fetch_account_dynamic(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/api/user/dynamic", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户作品
    pub fn fetch_account_works(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/api/work/list", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户注册时间
    pub fn fetch_account_register_time(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/new-people/user-info", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取课程账号信息
    pub fn fetch_account_lesson_info(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/api/v2/pc/lesson/user/info", None)
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户作品列表生成器 (Web 端)
    pub fn fetch_user_works_web_gen(
        &self,
        user_id: i32,
        types: Option<WorksListType>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/creation-tools/v2/user/center/work-list")
            .with_param("type", types.unwrap_or(WorksListType::Newest).as_str())
            .with_param("user_id", user_id.to_string())
            .with_param("offset", "0")
            .with_param("limit", "5")
            .with_total_key("total");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(5);
        }

        paginated
    }

    // 搜索用户作品 (Nemo 端)
    pub fn search_user_works_nemo(
        &self,
        query: &str,
        query_type: Option<&str>,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "tiger/nemo/user/works/search", None)
            .with_param("query", query)
            .with_param("query_type", query_type.unwrap_or("name"))
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户云端作品
    pub fn fetch_cloud_works(
        &self,
        types: CloudWorkType,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/creation-tools/v1/works/list/user", None)
            .with_param("limit", limit.unwrap_or(10).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("work_type", types.as_value().to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户证书信息
    pub fn fetch_user_certificate(
        &self,
        user_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://api-wechatsbp-codemaster.codemao.cn/user/info/certificate",
                None,
            )
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户已发布 Nemo 作品生成器
    pub fn fetch_published_nemo_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/nemo/v2/works/list/user/published")
            .with_param("limit", "15")
            .with_param("offset", "0");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取用户 KN 作品生成器
    pub fn fetch_kn_works_gen(
        &self,
        method: PublishStatus,
        extra_params: Option<Vec<(String, String)>>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let url = match method {
            PublishStatus::Published => "/neko/works/list/user/published",
            _ => "/neko/works/v2/list/user",
        };

        let mut paginated = self
            .client
            .paginated(url)
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_base_key(BaseKey::Creation);

        if let Some(extra) = extra_params {
            for (key, value) in extra {
                paginated = paginated.with_param(key, value);
            }
        }

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取用户 Kitten 作品生成器
    pub fn fetch_kitten_works_gen(
        &self,
        version: KittenVersion,
        status: PublishStatus,
        work_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/kitten/common/work/list2")
            .with_param("offset", "0")
            .with_param("limit", "30")
            .with_param("version_no", version.as_str())
            .with_param(
                "work_status",
                work_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_param("published_status", status.as_str())
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取用户 Nemo 作品生成器
    pub fn fetch_nemo_works_gen(
        &self,
        status: PublishStatus,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/creation-tools/v1/works/list")
            .with_param("offset", "0")
            .with_param("limit", "30")
            .with_param("published_status", status.as_str());

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取用户海龟编辑器作品生成器
    pub fn fetch_wood_works_gen(
        &self,
        status: PublishStatus,
        language_type: Option<i32>,
        work_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/wood/comm/work/list")
            .with_param("offset", "0")
            .with_param("limit", "30")
            .with_param("language_type", language_type.unwrap_or(0).to_string())
            .with_param(
                "work_status",
                work_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_param("published_status", status.as_str())
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取用户 Box 作品生成器
    pub fn fetch_box_works_gen(
        &self,
        status: PublishStatus,
        work_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/box/v2/work/list")
            .with_param("offset", "0")
            .with_param("limit", "30")
            .with_param(
                "work_status",
                work_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_param("published_status", status.as_str())
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取用户小说生成器
    pub fn fetch_fanfics_gen(
        &self,
        fiction_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/web/fanfic/my/new")
            .with_param("offset", "0")
            .with_param("limit", "30")
            .with_param(
                "fiction_status",
                fiction_status.unwrap_or(WorkShowStatus::Show).as_str(),
            );

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取用户 Coco 作品生成器
    pub fn fetch_coco_works_gen(
        &self,
        status: Option<i32>,
        published: Option<bool>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/coconut/web/work/list")
            .with_param("offset", "0")
            .with_param("limit", "30")
            .with_param("status", status.unwrap_or(1).to_string())
            .with_data_key("data.items")
            .with_total_key("data.total")
            .with_base_key(BaseKey::Creation);

        if let Some(pub_val) = published {
            paginated = paginated.with_param("published", pub_val.to_string());
        }

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取用户所有Coco作品
    pub fn fetch_coco_all_works(
        &self,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/web/work/list/all",
                Some(BaseKey::Creation),
            )
            .with_param("limit", limit.unwrap_or(20).to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户粉丝列表生成器
    pub fn fetch_followers_gen(&self, user_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/creation-tools/v1/user/fans")
            .with_param("user_id", user_id.to_string())
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_total_key("total");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取用户关注列表生成器
    pub fn fetch_following_gen(&self, user_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/creation-tools/v1/user/followers")
            .with_param("user_id", user_id.to_string())
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_total_key("total");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取用户已发布作品
    pub fn fetch_published_works(
        &self,
        user_id: i32,
        types: Vec<WorkType>,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let types_str: Vec<String> = types.iter().map(|t| t.as_str().to_string()).collect();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/api/user/works/published", None)
            .with_param("user_id", user_id.to_string())
            .with_param("types", types_str.join(","))
            .with_param("limit", limit.unwrap_or(10).to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户关注列表
    pub fn fetch_user_attention(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/api/user/me/attention", None)
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户粉丝列表
    pub fn fetch_user_followers(&self, user_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/api/user/attention/me", None)
            .with_param("user_id", user_id.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户收藏的作品
    pub fn fetch_user_collections(
        &self,
        user_id: i32,
        types: Vec<WorkType>,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let types_str: Vec<String> = types.iter().map(|t| t.as_str().to_string()).collect();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/api/user/works/collection", None)
            .with_param("user_id", user_id.to_string())
            .with_param("types", types_str.join(","))
            .with_param("limit", limit.unwrap_or(10).to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取用户收藏作品生成器
    pub fn fetch_collections_gen(&self, user_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/creation-tools/v2/user/center/collect/list")
            .with_param("user_id", user_id.to_string())
            .with_param("offset", "0")
            .with_param("limit", "5")
            .with_total_key("total");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(5);
        }

        paginated
    }

    // 获取用户头像框列表
    pub fn fetch_avatar_frames(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/user/avatar-frame/list",
                None,
            )
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 检查用户是否为新用户
    pub fn check_new_user_status(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/works/isNewUser",
                Some(BaseKey::Creation),
            )
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }
}

impl Default for UserDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 用户管理器 ====================
pub struct UserManager {
    client: &'static CodeMaoClient,
}

impl UserManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 更新用户状态
    pub fn update_status(
        &self,
        doing: Option<&str>,
        avatar: Option<&str>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let mut payload_map = serde_json::Map::new();

        if let Some(doing_val) = doing {
            payload_map.insert("doing".to_string(), Value::String(doing_val.to_string()));
        }
        if let Some(avatar_val) = avatar {
            payload_map.insert(
                "avatar_url".to_string(),
                Value::String(avatar_val.to_string()),
            );
        }

        let payload = Value::Object(payload_map);

        let response = self
            .client
            .build_request(HttpMethod::PUT, "/nemo/v2/user/basic", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == 200)
    }

    // 验证手机号码
    pub fn validate_phone_number(
        &self,
        phone_num: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/web/users/phone_number/is_consistent",
                None,
            )
            .with_param("phone_number", phone_num.to_string())
            .send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 修改密码
    pub fn update_password(
        &self,
        old_password: &str,
        new_password: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let payload = json!({
            "old_password": old_password,
            "password": new_password,
            "confirm_password": new_password,
        });

        let response = self
            .client
            .build_request(HttpMethod::PATCH, "/tiger/v3/web/accounts/password", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == 204)
    }

    // 请求更换手机号验证码
    pub fn execute_request_phone_change_verification(
        &self,
        old_phonenum: i32,
        new_phonenum: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let payload = json!({
            "phone_number": new_phonenum,
            "old_phone_number": old_phonenum,
        });

        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/tiger/v3/web/accounts/captcha/phone/change",
                None,
            )
            .with_payload(payload)
            .send()?;

        Ok(response.status() == 204)
    }

    // 更新手机号码
    pub fn update_phone_number(
        &self,
        captcha: i32,
        phonenum: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "phone_number": phonenum,
            "captcha": captcha,
        });

        let response = self
            .client
            .build_request(
                HttpMethod::PATCH,
                "/tiger/v3/web/accounts/phone/change",
                None,
            )
            .with_payload(payload)
            .send()?;

        Ok(self.client.response_to_json(response)?)
    }

    // 移除头像框
    pub fn delete_avatar_frame(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/creation-tools/v1/user/avatar-frame/cancel",
                None,
            )
            .send()?;

        Ok(response.status() == 200)
    }

    // 应用头像框
    pub fn execute_apply_avatar_frame(
        &self,
        frame_id: AvatarFrameId,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("/creation-tools/v1/user/avatar-frame/{}", frame_id as i32);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, None)
            .send()?;

        Ok(response.status() == 200)
    }

    // 更新个人资料详细信息
    pub fn update_profile_details(
        &self,
        avatar_url: &str,
        nickname: &str,
        birthday: i32,
        description: &str,
        fullname: &str,
        qq: &str,
        sex: Gender,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let payload = json!({
            "avatar_url": avatar_url,
            "nickname": nickname,
            "birthday": birthday,
            "description": description,
            "fullname": fullname,
            "qq": qq,
            "sex": sex as i32,
        });

        let response = self
            .client
            .build_request(HttpMethod::PATCH, "/tiger/v3/web/accounts/info", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == 204)
    }

    // 更新个人主页封面
    pub fn update_profile_cover(
        &self,
        cover_url: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let payload = json!({
            "preview": cover_url,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/nemo/v2/user/preview", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == 200)
    }

    // 注销用户
    pub fn delete_user(
        &self,
        reason: &str,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "closeReason": reason,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/tiger/v3/web/accounts/close", None)
            .with_payload(payload)
            .send()?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
        }
    }
}

impl Default for UserManager {
    fn default() -> Self {
        Self::new()
    }
}
