use crate::utils::requests::{
    BaseKey, ClientAccess, CodeMaoClient, DEFAULT_LIMIT, DEFAULT_PAGE_SIZE, HTTPStatus, HttpMethod,
    MewResult, PaginatedIter,
};
use log::debug;
use serde_json::{Value, json};

// 分页单页上限(各端点服务端契约)
const CENTER_LIST_PAGE_SIZE: usize = 5;
const WORK_LIST_PAGE_SIZE: usize = 30;
const COMMENT_PAGE_SIZE: usize = 10;

// 用户相关枚举

/// 作品类型
#[derive(Debug, Clone, Copy)]
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

/// 作品列表类型
#[derive(Debug, Clone, Copy)]
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

/// 云端作品类型
#[derive(Debug, Clone, Copy)]
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

/// 作品发布状态
#[derive(Debug, Clone, Copy)]
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

/// Kitten 版本
#[derive(Debug, Clone, Copy)]
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

/// 作品显示状态
#[derive(Debug, Clone, Copy)]
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

/// 头像框 ID
#[derive(Debug, Clone, Copy)]
pub enum AvatarFrameId {
    Lv2 = 2,
    Lv3 = 3,
    Lv4 = 4,
}

// 用户数据获取器

/// 用户相关数据查询接口
pub struct UserDataFetcher {
    client: CodeMaoClient,
}

impl UserDataFetcher {
    pub fn new() -> Self {
        Self::new_with_client(CodeMaoClient::global().clone())
    }

    pub fn new_with_client(client: CodeMaoClient) -> Self {
        Self { client }
    }

    // 私有辅助

    // 公共方法

    /// 获取用户详细信息
    pub fn fetch_user_profile(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户详细信息: user_id={}", user_id);
        let endpoint = format!("/api/user/info/detail/{}", user_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取当前登录用户信息
    pub fn fetch_own_user_info(&self) -> MewResult<Value> {
        debug!("获取当前用户信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/user/info", None);
        self.send_and_parse(builder)
    }

    /// 获取用户收藏列表(v1)
    pub fn fetch_user_collections_v1(
        &self,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取用户收藏列表v1: offset={:?}, limit={:?}", offset, limit);
        let mut builder = self.client.build_request(
            HttpMethod::Get,
            "/creation-tools/v1/user/center/collect/list",
            None,
        );
        if let Some(off) = offset {
            builder = builder.with_param("offset", off.to_string());
        }
        if let Some(lim) = limit {
            builder = builder.with_param("limit", lim.to_string());
        }
        self.send_and_parse(builder)
    }

    /// 获取用户作品列表(v1)
    pub fn fetch_user_work_list_v1(
        &self,
        user_id: i32,
        offset: i32,
        limit: i32,
    ) -> MewResult<Value> {
        debug!("获取用户作品列表v1: user_id={}", user_id);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/user/center/work-list",
                None,
            )
            .with_param("user_id", user_id.to_string())
            .with_param("offset", offset.to_string())
            .with_param("limit", limit.to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户 Tiger 信息
    pub fn fetch_user_tiger(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户Tiger信息: user_id={}", user_id);
        let endpoint = format!("/tiger/user/{}", user_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取用户 info 信息
    pub fn fetch_user_info(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户info信息: user_id={}", user_id);
        let endpoint = format!("/web/api/user/info/detail/{}", user_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取用户荣誉信息
    pub fn fetch_user_honors(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户荣誉信息: user_id={}", user_id);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/user/center/honor",
                None,
            )
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户业务数据指标
    pub fn fetch_user_metrics(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户业务指标: user_id={}", user_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v2/works/business/total", None)
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户基本信息
    pub fn fetch_user_intro(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户基本信息: user_id={}", user_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v2/user/dynamic/info", None)
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户动态信息
    pub fn fetch_user_dynamic(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户动态: user_id={}", user_id);
        let endpoint = format!("/api/user/dynamic/{}", user_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取用户加入的工作室信息
    pub fn fetch_user_studio(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户工作室: user_id={}", user_id);
        let endpoint = format!("/web/work-shops/{}/participators", user_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取用户年度总结
    pub fn fetch_user_annual_summary(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户年度总结: user_id={}", user_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/activities/annual-summary", None)
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取账户简要信息
    pub fn get_account_info(&self) -> MewResult<Value> {
        debug!("获取账户简要信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/api/user/info", None);
        self.send_and_parse(builder)
    }

    /// 获取当前账号详细信息
    pub fn fetch_account_details(&self) -> MewResult<Value> {
        debug!("获取当前账号详细信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/users/details", None);
        self.send_and_parse(builder)
    }

    /// 获取 Tiger 账号信息
    pub fn fetch_account_tiger(&self) -> MewResult<Value> {
        debug!("获取Tiger账号信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/tiger/user", None);
        self.send_and_parse(builder)
    }

    /// 获取用户评分数据
    pub fn fetch_account_scores(&self) -> MewResult<Value> {
        debug!("获取用户评分数据");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/nemo/v3/user/grade/details", None);
        self.send_and_parse(builder)
    }

    /// 获取用户等级信息
    pub fn fetch_account_level(&self) -> MewResult<Value> {
        debug!("获取用户等级信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v3/user/level/info", None);
        self.send_and_parse(builder)
    }

    /// 获取用户动态信息
    pub fn fetch_account_dynamic(&self) -> MewResult<Value> {
        debug!("获取用户动态信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/user/dynamic", None);
        self.send_and_parse(builder)
    }

    /// 获取用户作品列表
    pub fn fetch_account_works(&self) -> MewResult<Value> {
        debug!("获取用户作品列表");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/work/list", None);
        self.send_and_parse(builder)
    }

    /// 获取用户注册时间
    pub fn fetch_account_register_time(&self) -> MewResult<Value> {
        debug!("获取用户注册时间");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/nemo/new-people/user-info", None);
        self.send_and_parse(builder)
    }

    /// 获取课程账号信息
    pub fn fetch_account_lesson_info(&self) -> MewResult<Value> {
        debug!("获取课程账号信息");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/api/v2/pc/lesson/user/info", None);
        self.send_and_parse(builder)
    }

    /// 用户作品列表分页迭代器(Web 端)
    pub fn fetch_user_works_web_iter(
        &self,
        user_id: i32,
        types: Option<WorksListType>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取用户Web作品迭代器: user_id={}, type={:?}",
            user_id, types
        );
        self.client
            .build_paginated("/creation-tools/v2/user/center/work-list")
            .with_iter_param("type", types.unwrap_or(WorksListType::Newest).as_str())
            .with_iter_param("user_id", user_id.to_string())
            .with_page_size(5)
            .with_total_key("total")
            .with_limit(limit.unwrap_or(CENTER_LIST_PAGE_SIZE))
    }

    /// 搜索用户作品(Nemo 端)
    pub fn search_user_works_nemo(
        &self,
        query: &str,
        query_type: Option<&str>,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "搜索Nemo用户作品: query={}, query_type={:?}",
            query, query_type
        );
        let builder = self
            .client
            .build_request(HttpMethod::Get, "tiger/nemo/user/works/search", None)
            .with_param("query", query)
            .with_param("query_type", query_type.unwrap_or("name"))
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param(
                "limit",
                limit.unwrap_or(COMMENT_PAGE_SIZE as i32).to_string(),
            );
        self.send_and_parse(builder)
    }

    /// 获取用户云端作品
    pub fn fetch_cloud_works(
        &self,
        types: CloudWorkType,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取云端作品: type={:?}", types);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/creation-tools/v1/works/list/user", None)
            .with_param(
                "limit",
                limit.unwrap_or(COMMENT_PAGE_SIZE as i32).to_string(),
            )
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("work_type", types.as_value().to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户证书信息
    pub fn fetch_user_certificate(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取用户证书: user_id={}", user_id);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/user/info/certificate",
                Some(BaseKey::WeChatSbp),
            )
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 用户已发布 Nemo 作品分页迭代器
    pub fn fetch_published_nemo_works_iter(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取已发布Nemo作品迭代器");
        self.client
            .build_paginated("/nemo/v2/works/list/user/published")
            .with_page_size(15)
            .with_limit(limit.unwrap_or(DEFAULT_PAGE_SIZE))
    }

    /// 用户 KN 作品分页迭代器
    pub fn fetch_kn_works_iter(
        &self,
        method: PublishStatus,
        extra_params: Option<Vec<(String, String)>>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let url = match method {
            PublishStatus::Published => "/neko/works/list/user/published",
            _ => "/neko/works/v2/list/user",
        };
        debug!("获取KN作品迭代器: method={:?}", method);
        let mut paginated = self
            .client
            .build_paginated(url)
            .with_page_size(15)
            .with_limit(limit.unwrap_or(DEFAULT_PAGE_SIZE))
            .with_base_key(BaseKey::Creation);
        if let Some(extra) = extra_params {
            for (key, value) in extra {
                paginated = paginated.with_iter_param(key, value);
            }
        }
        paginated
    }

    /// 用户 Kitten 作品分页迭代器
    pub fn fetch_kitten_works_iter(
        &self,
        version: KittenVersion,
        status: PublishStatus,
        work_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取Kitten作品迭代器: version={:?}, status={:?}",
            version, status
        );
        self.client
            .build_paginated("/kitten/common/work/list2")
            .with_page_size(30)
            .with_iter_param("version_no", version.as_str())
            .with_iter_param(
                "work_status",
                work_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_iter_param("published_status", status.as_str())
            .with_base_key(BaseKey::Creation)
            .with_limit(limit.unwrap_or(WORK_LIST_PAGE_SIZE))
    }

    /// 用户 Nemo 作品分页迭代器
    pub fn fetch_nemo_works_iter(
        &self,
        status: PublishStatus,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取Nemo作品迭代器: status={:?}", status);
        self.client
            .build_paginated("/creation-tools/v1/works/list")
            .with_page_size(30)
            .with_iter_param("published_status", status.as_str())
            .with_limit(limit.unwrap_or(WORK_LIST_PAGE_SIZE))
    }

    /// 用户海龟编辑器作品分页迭代器
    pub fn fetch_wood_works_iter(
        &self,
        status: PublishStatus,
        language_type: Option<i32>,
        work_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取海龟编辑器作品迭代器: status={:?}, language_type={:?}",
            status, language_type
        );
        self.client
            .build_paginated("/wood/comm/work/list")
            .with_page_size(30)
            .with_iter_param("language_type", language_type.unwrap_or(0).to_string())
            .with_iter_param(
                "work_status",
                work_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_iter_param("published_status", status.as_str())
            .with_base_key(BaseKey::Creation)
            .with_limit(limit.unwrap_or(WORK_LIST_PAGE_SIZE))
    }

    /// 用户 Box 作品分页迭代器
    pub fn fetch_box_works_iter(
        &self,
        status: PublishStatus,
        work_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取Box作品迭代器: status={:?}", status);
        self.client
            .build_paginated("/box/v2/work/list")
            .with_page_size(30)
            .with_iter_param(
                "work_status",
                work_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_iter_param("published_status", status.as_str())
            .with_base_key(BaseKey::Creation)
            .with_limit(limit.unwrap_or(WORK_LIST_PAGE_SIZE))
    }

    /// 用户小说分页迭代器
    pub fn fetch_fanfics_iter(
        &self,
        fiction_status: Option<WorkShowStatus>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取小说迭代器");
        self.client
            .build_paginated("/web/fanfic/my/new")
            .with_page_size(30)
            .with_iter_param(
                "fiction_status",
                fiction_status.unwrap_or(WorkShowStatus::Show).as_str(),
            )
            .with_limit(limit.unwrap_or(WORK_LIST_PAGE_SIZE))
    }

    /// 用户 Coco 作品分页迭代器
    pub fn fetch_coco_works_iter(
        &self,
        status: Option<i32>,
        published: Option<bool>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取Coco作品迭代器: status={:?}, published={:?}",
            status, published
        );
        let mut paginated = self
            .client
            .build_paginated("/coconut/web/work/list")
            .with_page_size(30)
            .with_iter_param("status", status.unwrap_or(1).to_string())
            .with_data_key("data.items")
            .with_total_key("data.total")
            .with_base_key(BaseKey::Creation)
            .with_limit(limit.unwrap_or(WORK_LIST_PAGE_SIZE));
        if let Some(pub_val) = published {
            paginated = paginated.with_iter_param("published", pub_val.to_string());
        }
        paginated
    }

    /// 获取用户所有 Coco 作品
    pub fn fetch_coco_all_works(&self, limit: Option<i32>) -> MewResult<Value> {
        debug!("获取所有Coco作品: limit={:?}", limit);
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/coconut/web/work/list/all",
                Some(BaseKey::Creation),
            )
            .with_param("limit", limit.unwrap_or(DEFAULT_LIMIT as i32).to_string());
        self.send_and_parse(builder)
    }

    /// 用户粉丝列表分页迭代器
    pub fn fetch_followers_iter(&self, user_id: i32, limit: Option<usize>) -> PaginatedIter {
        debug!("获取粉丝迭代器: user_id={}", user_id);
        self.client
            .build_paginated("/creation-tools/v1/user/fans")
            .with_iter_param("user_id", user_id.to_string())
            .with_page_size(15)
            .with_total_key("total")
            .with_limit(limit.unwrap_or(DEFAULT_PAGE_SIZE))
    }

    /// 用户关注列表分页迭代器
    pub fn fetch_following_iter(&self, user_id: i32, limit: Option<usize>) -> PaginatedIter {
        debug!("获取关注迭代器: user_id={}", user_id);
        self.client
            .build_paginated("/creation-tools/v1/user/followers")
            .with_iter_param("user_id", user_id.to_string())
            .with_page_size(15)
            .with_total_key("total")
            .with_limit(limit.unwrap_or(DEFAULT_PAGE_SIZE))
    }

    /// 获取用户已发布作品
    pub fn fetch_published_works(
        &self,
        user_id: i32,
        types: Vec<WorkType>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let types_str: Vec<String> = types.iter().map(|t| t.as_str().to_string()).collect();
        debug!("获取已发布作品: user_id={}, types={:?}", user_id, types_str);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/api/user/works/published", None)
            .with_param("user_id", user_id.to_string())
            .with_param("types", types_str.join(","))
            .with_param(
                "limit",
                limit.unwrap_or(COMMENT_PAGE_SIZE as i32).to_string(),
            );
        self.send_and_parse(builder)
    }

    /// 获取用户关注列表
    pub fn fetch_user_attention(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取关注列表: user_id={}", user_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/api/user/me/attention", None)
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户粉丝列表
    pub fn fetch_user_followers(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取粉丝列表: user_id={}", user_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/api/user/attention/me", None)
            .with_param("user_id", user_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取用户收藏的作品
    pub fn fetch_user_collections(
        &self,
        user_id: i32,
        types: Vec<WorkType>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let types_str: Vec<String> = types.iter().map(|t| t.as_str().to_string()).collect();
        debug!("获取收藏作品: user_id={}, types={:?}", user_id, types_str);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/api/user/works/collection", None)
            .with_param("user_id", user_id.to_string())
            .with_param("types", types_str.join(","))
            .with_param(
                "limit",
                limit.unwrap_or(COMMENT_PAGE_SIZE as i32).to_string(),
            );
        self.send_and_parse(builder)
    }

    /// 用户收藏作品分页迭代器
    pub fn fetch_collections_iter(&self, user_id: i32, limit: Option<usize>) -> PaginatedIter {
        debug!("获取收藏作品迭代器: user_id={}", user_id);
        self.client
            .build_paginated("/creation-tools/v2/user/center/collect/list")
            .with_iter_param("user_id", user_id.to_string())
            .with_page_size(5)
            .with_total_key("total")
            .with_limit(limit.unwrap_or(CENTER_LIST_PAGE_SIZE))
    }

    /// 获取用户头像框列表
    pub fn fetch_avatar_frames(&self) -> MewResult<Value> {
        debug!("获取头像框列表");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/creation-tools/v1/user/avatar-frame/list",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 检查用户是否为新用户
    pub fn check_new_user_status(&self) -> MewResult<Value> {
        debug!("检查新用户状态");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/neko/works/isNewUser",
            Some(BaseKey::Creation),
        );
        self.send_and_parse(builder)
    }
}

impl Default for UserDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for UserDataFetcher {
    fn client(&self) -> &CodeMaoClient {
        &self.client
    }
}

// 用户管理器

/// 用户相关操作接口(更新状态,头像框,主页封面等)
pub struct UserManager {
    client: CodeMaoClient,
}

impl UserManager {
    pub fn new() -> Self {
        Self::new_with_client(CodeMaoClient::global().clone())
    }

    pub fn new_with_client(client: CodeMaoClient) -> Self {
        Self { client }
    }

    // 私有辅助

    // 公共方法

    /// 更新用户状态(动态/头像)
    pub fn update_status(&self, doing: Option<&str>, avatar: Option<&str>) -> MewResult<bool> {
        debug!("更新用户状态: doing={:?}, avatar={:?}", doing, avatar);
        let payload = json!({
            "doing": doing,
            "avatar_url": avatar,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Put, "/nemo/v2/user/basic", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 移除头像框
    pub fn delete_avatar_frame(&self) -> MewResult<bool> {
        debug!("移除头像框");
        let builder = self.client.build_request(
            HttpMethod::Put,
            "/creation-tools/v1/user/avatar-frame/cancel",
            None,
        );
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 应用头像框
    pub fn apply_avatar_frame(&self, frame_id: AvatarFrameId) -> MewResult<bool> {
        debug!("应用头像框: frame_id={:?}", frame_id);
        let endpoint = format!("/creation-tools/v1/user/avatar-frame/{}", frame_id as i32);
        let builder = self.client.build_request(HttpMethod::Put, &endpoint, None);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 更新个人主页封面
    pub fn update_profile_cover(&self, cover_url: &str) -> MewResult<bool> {
        debug!("更新个人主页封面: cover_url={}", cover_url);
        let payload = json!({ "preview": cover_url });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/nemo/v2/user/preview", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }
}

impl Default for UserManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for UserManager {
    fn client(&self) -> &CodeMaoClient {
        &self.client
    }
}
