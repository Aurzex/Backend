use crate::utils::acquire::{
    BaseKey, CodeMaoClient, DEFAULT_PID, HTTPStatus, HttpMethod, MewResult, PaginatedIter,
    PaginationMethod,
};
use log::{debug, warn};
use serde_json::{Value, json};

// 枚举定义

/// 回复消息类型
#[derive(Debug, Clone, Copy)]
pub enum ReplyTypes {
    LikeFork,
    CommentReply,
    System,
}

impl ReplyTypes {
    fn as_str(&self) -> &'static str {
        match self {
            ReplyTypes::LikeFork => "LIKE_FORK",
            ReplyTypes::CommentReply => "COMMENT_REPLY",
            ReplyTypes::System => "SYSTEM",
        }
    }
}

/// 消息平台(Web / Nemo)
#[derive(Debug, Clone, Copy)]
pub enum MessageMethod {
    Web,
    Nemo,
}

/// 网页头图类型
#[derive(Debug, Clone, Copy)]
pub enum BannerType {
    FloatBanner,
    Official,
    CodeTv,
    WokeShop,
    MaterialNormal,
}

impl BannerType {
    fn as_str(&self) -> &'static str {
        match self {
            BannerType::FloatBanner => "FLOAT_BANNER",
            BannerType::Official => "OFFICIAL",
            BannerType::CodeTv => "CODE_TV",
            BannerType::WokeShop => "WOKE_SHOP",
            BannerType::MaterialNormal => "MATERIAL_NORMAL",
        }
    }
}

/// Nemo 端头图类型
#[derive(Debug, Clone, Copy)]
pub enum NemoBannerType {
    Type1 = 1,
    Type2 = 2,
    Type3 = 3,
}

/// 作品推荐类型
#[derive(Debug, Clone, Copy)]
pub enum WorkRecommendType {
    Type1 = 1,
    Type2 = 2,
}

/// 作品频道类型
#[derive(Debug, Clone, Copy)]
pub enum WorkChannelType {
    Kitten,
    Nemo,
}

impl WorkChannelType {
    fn as_str(&self) -> &'static str {
        match self {
            WorkChannelType::Kitten => "KITTEN",
            WorkChannelType::Nemo => "NEMO",
        }
    }
}

/// 学科 ID
#[derive(Debug, Clone, Copy)]
pub enum SubjectId {
    Basic = 1,
    Advanced = 2,
}

/// 社区各模块开启状态查询类型
#[derive(Debug, Clone, Copy)]
pub enum CommunityStatusType {
    WebForumStatus,
    WebFictionStatus,
}

impl CommunityStatusType {
    fn as_str(&self) -> &'static str {
        match self {
            CommunityStatusType::WebForumStatus => "WEB_FORUM_STATUS",
            CommunityStatusType::WebFictionStatus => "WEB_FICTION_STATUS",
        }
    }
}

/// 作品排序方式
#[derive(Debug, Clone, Copy)]
pub enum OrderBy {
    UpdateTime,
    ViewTimes,
}

impl OrderBy {
    fn as_str(&self) -> &'static str {
        match self {
            OrderBy::UpdateTime => "update_time",
            OrderBy::ViewTimes => "view_times",
        }
    }
}

/// 消息阅读状态
#[derive(Debug, Clone, Copy)]
pub enum ReadStatus {
    Read,
    Unread,
}

impl ReadStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ReadStatus::Read => "READ",
            ReadStatus::Unread => "UNREAD",
        }
    }
}

// 社区数据获取器

/// 社区相关数据与配置获取
pub struct CommunityDataFetcher {
    client: &'static CodeMaoClient,
}

impl CommunityDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 辅助方法

    /// 从 `/coconut/clouddb/currentTime` 获取 10 位时间戳,返回原始 JSON
    /// 内部复用,避免代码重复
    fn raw_timestamp_10(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/coconut/clouddb/currentTime", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 安全地从时间戳 JSON 中提取字符串值,若失败则记录警告并返回空字符串
    fn extract_time_string(json: &Value) -> String {
        match json["data"].as_str() {
            Some(s) => s.to_string(),
            None => {
                warn!("时间戳响应中缺少 'data' 字段或不是字符串: {:?}", json);
                String::new()
            }
        }
    }

    // 公共方法

    /// 获取随机昵称
    pub fn fetch_random_nickname(&self) -> MewResult<Value> {
        debug!("获取随机昵称");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/user/random/nickname", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取新消息数量(根据平台)
    pub fn fetch_message_count(&self, method: MessageMethod) -> MewResult<Value> {
        let endpoint = match method {
            MessageMethod::Web => "/web/message-record/count",
            MessageMethod::Nemo => "/nemo/v2/user/message/count",
        };
        debug!("获取消息数量, 平台: {:?}", method);
        let response = self
            .client
            .build_request(HttpMethod::Get, endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取指定类型的回复列表
    pub fn fetch_replies(&self, types: ReplyTypes, limit: i32, offset: i32) -> MewResult<Value> {
        debug!(
            "获取回复: type={:?}, limit={}, offset={}",
            types, limit, offset
        );
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/message-record", None)
            .with_param("query_type", types.as_str())
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取回复的分页迭代器
    pub fn fetch_replies_gen(&self, types: ReplyTypes, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/web/message-record")
            .with_iter_param("query_type", types.as_str())
            .with_iter_method(HttpMethod::Get)
            .with_pagination_method(PaginationMethod::Offset)
            .with_total_key("total")
            .with_data_key("items")
            .with_limit(limit.unwrap_or(15))
    }

    /// 获取 Nemo 消息(喜欢或评论)
    pub fn fetch_nemo_messages(&self, types: &str) -> MewResult<Value> {
        let extra_url = if types == "like" { "1" } else { "3" };
        let endpoint = format!("/nemo/v2/user/message/{}", extra_url);
        debug!("获取Nemo消息: type={}, endpoint={}", types, endpoint);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 PC 客户端最新版本信息
    pub fn fetch_pc_client(&self) -> MewResult<Value> {
        debug!("获取PC客户端更新");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/tiger/pc_client/releases/latest", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取点个猫更新信息
    pub fn fetch_pickcat_update(&self) -> MewResult<Value> {
        debug!("获取点个猫更新");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://update.codemao.cn/updatev2/appsdk",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Kitten4 更新信息
    pub fn fetch_kitten4_update(&self) -> MewResult<Value> {
        let timestamp = self.raw_timestamp_10()?;
        let time_value = Self::extract_time_string(&timestamp);

        debug!("获取Kitten4更新");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://kn-cdn.codemao.cn/kitten4/application/kitten4_update_info.json",
                None,
            )
            .with_param("TIME", &time_value)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Kitten 更新信息
    pub fn fetch_kitten_update(&self) -> MewResult<Value> {
        let timestamp = self.raw_timestamp_10()?;
        let time_value = Self::extract_time_string(&timestamp);

        debug!("获取Kitten更新");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://kn-cdn.codemao.cn/application/kitten_update_info.json",
                None,
            )
            .with_param("timeStamp", &time_value)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取海龟编辑器更新信息
    pub fn fetch_wood_editor_update(&self) -> MewResult<Value> {
        let timestamp = self.raw_timestamp_10()?;
        let time_value = Self::extract_time_string(&timestamp);

        debug!("获取海龟编辑器更新");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://static-am.codemao.cn/wood/client/xp/prod/package.json",
                None,
            )
            .with_param("timeStamp", &time_value)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取源码智造编辑器更新信息
    pub fn fetch_matrix_editor_update(&self) -> MewResult<Value> {
        let timestamp = self.raw_timestamp_10()?;
        let time_value = Self::extract_time_string(&timestamp);

        debug!("获取源码智造编辑器更新");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://public-static-edu.codemao.cn/matrix/publish/desktop_matrix.json",
                None,
            )
            .with_param("timeStamp", &time_value)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 10 位时间戳(保持原有公共接口)
    pub fn fetch_current_timestamp_10(&self) -> MewResult<Value> {
        self.raw_timestamp_10()
    }

    /// 获取 13 位时间戳(独立接口)
    pub fn fetch_current_timestamp_13(&self) -> MewResult<Value> {
        debug!("获取13位时间戳");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://time.codemao.cn/time/current",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Web 端头图
    pub fn fetch_web_banners(&self, banner_type: Option<BannerType>) -> MewResult<Value> {
        let mut builder = self
            .client
            .build_request(HttpMethod::Get, "/web/banners/all", None);

        if let Some(b_type) = banner_type {
            builder = builder.with_param("type", b_type.as_str());
        }

        debug!("获取Web端头图: {:?}", banner_type);
        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Nemo 端头图
    pub fn fetch_nemo_banners(&self, banner_type: NemoBannerType) -> MewResult<Value> {
        debug!("获取Nemo端头图: {:?}", banner_type);
        let response = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v2/home/banners", None)
            .with_param("banner_type", (banner_type as i32).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Coco 端头图
    pub fn fetch_coco_banners(&self) -> MewResult<Value> {
        debug!("获取Coco端头图");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/coconut/banner/list",
                Some(BaseKey::Creation),
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Coco 话题
    pub fn fetch_coco_topic(&self) -> MewResult<Value> {
        debug!("获取Coco话题");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/coconut/topic/list",
                Some(BaseKey::Creation),
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取举报原因列表
    pub fn fetch_report_reasons(&self) -> MewResult<Value> {
        debug!("获取举报原因");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/reports/reasons/all", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Nemo 配置
    pub fn fetch_nemo_config(&self) -> MewResult<Value> {
        debug!("获取Nemo配置");
        let response = self
            .client
            .build_request(HttpMethod::Get, "https://nemo.codemao.cn/config", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取社区配置
    pub fn fetch_community_config(&self) -> MewResult<Value> {
        debug!("获取社区配置");
        let response = self
            .client
            .build_request(HttpMethod::Get, "https://c.codemao.cn/config", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取编程猫客户端配置
    pub fn fetch_client_config(&self) -> MewResult<Value> {
        debug!("获取客户端配置");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://player.codemao.cn/new/client_config.json",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取编程猫首页推荐作品
    pub fn fetch_recommended_works(&self, recommend_type: WorkRecommendType) -> MewResult<Value> {
        debug!("获取推荐作品: {:?}", recommend_type);
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/pc/home/recommend-work",
                None,
            )
            .with_param("type", (recommend_type as i32).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Nemo 新作喵喵看更多列表
    pub fn fetch_new_recommend_works(&self, limit: i32, offset: i32) -> MewResult<Value> {
        debug!("获取新推荐作品: limit={}, offset={}", limit, offset);
        let response = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v3/new-recommend/more/list", None)
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Nemo 系统推荐池
    pub fn fetch_recommended_works_nemo(&self) -> MewResult<Value> {
        debug!("获取Nemo推荐池");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v2/system/recommended/pool", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取作品频道列表
    pub fn fetch_work_channels(&self, channel_type: WorkChannelType) -> MewResult<Value> {
        debug!("获取作品频道: {:?}", channel_type);
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/works/channels/list", None)
            .with_param("type", channel_type.as_str())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取指定频道下的作品列表
    pub fn fetch_channel_works(
        &self,
        channel_id: i32,
        channel_type: WorkChannelType,
        limit: i32,
        page: i32,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/works/channels/{}/works", channel_id);
        debug!(
            "获取频道作品: id={}, type={:?}, limit={}, page={}",
            channel_id, channel_type, limit, page
        );
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("type", channel_type.as_str())
            .with_param("page", page.to_string())
            .with_param("limit", limit.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取社区星推荐用户
    pub fn fetch_recommended_users(&self) -> MewResult<Value> {
        debug!("获取推荐用户");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/users/recommended", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取训练师小课堂内容
    pub fn fetch_training_courses(&self) -> MewResult<Value> {
        debug!("获取训练师小课堂");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://backend.box3.fun/diversion/codemao/post",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 KN 课程
    pub fn fetch_kn_courses(&self) -> MewResult<Value> {
        debug!("获取KN课程");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/home/especially/course",
                None,
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// KN 公开课分页迭代器
    pub fn fetch_public_courses_gen(&self, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/neko/course/publish/list")
            .with_page_size(10)
            .with_total_key("total_course")
            .with_data_key("course_page.items")
            .with_base_key(BaseKey::Creation)
            .with_limit(limit.unwrap_or(10))
    }

    /// 获取 KN 模板作品
    pub fn fetch_sample_works(&self, subject_id: SubjectId) -> MewResult<Value> {
        debug!("获取模板作品: subject_id={:?}", subject_id);
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/sample/list",
                Some(BaseKey::Creation),
            )
            .with_param("subject_id", (subject_id as i32).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取社区各模块开启状态
    pub fn fetch_community_status(&self, status_type: CommunityStatusType) -> MewResult<Value> {
        let endpoint = format!(
            "/web/config/tab/on-off/status?config_type={}",
            status_type.as_str()
        );
        debug!("获取社区状态: {:?}", status_type);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取 Kitten 编辑页精选活动
    pub fn fetch_kitten_activities(&self) -> MewResult<Value> {
        debug!("获取Kitten活动");
        let response = self
            .client
            .build_request(
                HttpMethod::Get,
                "/kitten/activity/choiceness/list",
                Some(BaseKey::Creation),
            )
            .send()?;
        self.client.response_to_json(response)
    }

    /// Nemo 端教程合集分页迭代器
    pub fn fetch_course_packages_gen(&self, platform: i32, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/creation-tools/v1/course/package/list")
            .with_page_size(50)
            .with_iter_param("platform", platform.to_string())
            .with_limit(limit.unwrap_or(50))
    }

    /// Nemo 教程详情分页迭代器
    pub fn fetch_course_details_gen(
        &self,
        course_package_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        self.client
            .paginated("/creation-tools/v1/course/list/search")
            .with_iter_param("course_package_id", course_package_id.to_string())
            .with_page_size(50)
            .with_data_key("course_page.items")
            .with_limit(limit.unwrap_or(50))
    }

    /// 教学计划分页迭代器
    pub fn fetch_teaching_plans_gen(&self, limit: usize) -> PaginatedIter {
        debug!("获取教学计划迭代器, limit={}", limit);
        self.client
            .paginated("/neko/teaching-plan/list/team")
            .with_limit(limit)
            .with_base_key(BaseKey::Creation)
    }

    /// 获取板块未读消息数量
    pub fn fetch_board_unread_count(&self, board_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/forums/boards/{}/unread-count", board_id);
        debug!("获取板块未读数: board_id={}", board_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取活动(工作室)信息
    pub fn fetch_studio_info(&self, studio_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/studios/{}", studio_id);
        debug!("获取活动信息: studio_id={}", studio_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 活动帖子分页迭代器
    pub fn fetch_studio_posts_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/web/forums/posts")
            .with_page_size(50)
            .with_iter_param("studio_id", studio_id.to_string())
            .with_iter_param("sort", "-created_at")
            .with_limit(limit.unwrap_or(24))
    }

    /// 活动教程分页迭代器
    pub fn fetch_studio_courses_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/studios/{}/courses", studio_id);
        self.client
            .paginated(&endpoint)
            .with_page_size(50)
            .with_limit(limit.unwrap_or(100))
    }

    /// 活动作品分页迭代器
    pub fn fetch_studio_works_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/studios/{}/works", studio_id);
        self.client
            .paginated(&endpoint)
            .with_page_size(50)
            .with_iter_param("sort", "-n_likes")
            .with_limit(limit.unwrap_or(24))
    }

    /// 活动参与者分页迭代器
    pub fn fetch_studio_participators_gen(
        &self,
        studio_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/studios/{}/participators", studio_id);
        self.client
            .paginated(&endpoint)
            .with_page_size(50)
            .with_limit(limit.unwrap_or(24))
    }

    /// 获取旧版作品标签
    pub fn fetch_work_labels(&self) -> MewResult<Value> {
        debug!("获取作品标签");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/work/label/list", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取旧版标签分类
    pub fn fetch_work_category(&self) -> MewResult<Value> {
        debug!("获取作品分类");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/label/list", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取推荐作品(IDE 专用)
    pub fn fetch_recommended_ide_works(
        &self,
        work_type: &str,
        page_number: i32,
        amount_items: i32,
    ) -> MewResult<Value> {
        debug!(
            "获取IDE推荐作品: type={}, page={}, amount={}",
            work_type, page_number, amount_items
        );
        let response = self
            .client
            .build_request(HttpMethod::Get, "/tiger/work/ide/recommended", None)
            .with_param("type", work_type)
            .with_param("page_number", page_number.to_string())
            .with_param("amount_items", amount_items.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取所有推荐作品(带排序)
    pub fn fetch_recommended_works_all(
        &self,
        work_type: &str,
        page_number: i32,
        amount_items: i32,
        order_by: OrderBy,
    ) -> MewResult<Value> {
        debug!(
            "获取全部推荐作品: type={}, page={}, per_page={}, order={:?}",
            work_type, page_number, amount_items, order_by
        );
        let response = self
            .client
            .build_request(HttpMethod::Get, "/tiger/work/list/all", None)
            .with_param("type", work_type)
            .with_param("page", page_number.to_string())
            .with_param("per_page", amount_items.to_string())
            .with_param("order_by", order_by.as_str())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取推荐素材
    pub fn fetch_material_recommend(
        &self,
        category_id: i32,
        limit: i32,
        offset: i32,
    ) -> MewResult<Value> {
        debug!(
            "获取推荐素材: category={}, limit={}, offset={}",
            category_id, limit, offset
        );
        let response = self
            .client
            .build_request(HttpMethod::Get, "/tiger/material/recommend", None)
            .with_param("category_id", category_id.to_string())
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.to_string())
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for CommunityDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// 用户操作接口

/// 用户相关操作(注册,协议签署,消息管理等)
pub struct UserAction {
    client: &'static CodeMaoClient,
}

impl UserAction {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 签订 Nemo 友好协议
    pub fn execute_sign_agreement(&self) -> MewResult<bool> {
        debug!("签订友好协议");
        let response = self
            .client
            .build_request(HttpMethod::Post, "/nemo/v3/user/level/signature", None)
            .with_payload(json!({}))
            .send()?;
        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    /// 获取用户协议列表
    pub fn fetch_agreements(&self) -> MewResult<Value> {
        debug!("获取用户协议");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/tiger/v3/web/accounts/agreements", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 手机号注册账号
    pub fn create_account(
        &self,
        identity: &str,
        password: &str,
        captcha: &str,
        pid: Option<&str>,
        agreement_ids: Option<Vec<i32>>,
    ) -> MewResult<Value> {
        debug!("注册账号: identity={}", identity);
        let mut data = serde_json::Map::new();
        data.insert("identity".to_string(), Value::String(identity.to_string()));
        data.insert("password".to_string(), Value::String(password.to_string()));
        data.insert("captcha".to_string(), Value::String(captcha.to_string()));

        let pid_value = pid.unwrap_or(DEFAULT_PID);
        data.insert("pid".to_string(), Value::String(pid_value.to_string()));

        let agreement_values = match agreement_ids {
            Some(ids) => ids.into_iter().map(|id| Value::Number(id.into())).collect(),
            None => vec![Value::Number(186.into()), Value::Number(13.into())],
        };
        data.insert("agreement_ids".to_string(), Value::Array(agreement_values));

        let payload = Value::Object(data);

        let response = self
            .client
            .build_request(
                HttpMethod::Post,
                "/tiger/v3/web/accounts/register/phone/with-agreement",
                None,
            )
            .with_payload(payload)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 删除指定消息
    pub fn delete_message(&self, message_id: i32) -> MewResult<bool> {
        let endpoint = format!("/web/message-record/{}", message_id);
        debug!("删除消息: id={}", message_id);
        let response = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .send()?;
        Ok(response.status() == 204)
    }

    /// 获取广播消息分页迭代器
    pub fn fetch_broadcast_messages_gen(
        &self,
        limit: Option<usize>,
        read_status: ReadStatus,
    ) -> PaginatedIter {
        self.client
            .paginated("/web/message-record/broadcast")
            .with_page_size(1)
            .with_iter_param("read_status", read_status.as_str())
            .with_iter_param("sort", "-created_at")
            .with_limit(limit.unwrap_or(10))
    }
}

impl Default for UserAction {
    fn default() -> Self {
        Self::new()
    }
}
