use crate::utils::acquire::{
    BaseKey, CodeMaoClient, HttpMethod, MewError, MewResult, PaginatedIter, PaginationMethod,
};
use serde_json::{Value, json};

// ==================== 枚举定义 ====================

#[derive(Clone, Copy)]
pub enum ReplyTypes {
    LikeFork,
    CommentReply,
    System,
}

impl ReplyTypes {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReplyTypes::LikeFork => "LIKE_FORK",
            ReplyTypes::CommentReply => "COMMENT_REPLY",
            ReplyTypes::System => "SYSTEM",
        }
    }
}

pub enum MessageMethod {
    Web,
    Nemo,
}

impl MessageMethod {
    fn endpoint(&self) -> &'static str {
        match self {
            MessageMethod::Web => "/web/message-record/count",
            MessageMethod::Nemo => "/nemo/v2/user/message/count",
        }
    }
}

pub enum BannerType {
    FloatBanner,
    Official,
    CodeTv,
    WokeShop,
    MaterialNormal,
}

impl BannerType {
    pub fn as_str(&self) -> &'static str {
        match self {
            BannerType::FloatBanner => "FLOAT_BANNER",
            BannerType::Official => "OFFICIAL",
            BannerType::CodeTv => "CODE_TV",
            BannerType::WokeShop => "WOKE_SHOP",
            BannerType::MaterialNormal => "MATERIAL_NORMAL",
        }
    }
}

pub enum NemoBannerType {
    Type1 = 1,
    Type2 = 2,
    Type3 = 3,
}

pub enum WorkRecommendType {
    Type1 = 1,
    Type2 = 2,
}

pub enum WorkChannelType {
    Kitten,
    Nemo,
}

impl WorkChannelType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkChannelType::Kitten => "KITTEN",
            WorkChannelType::Nemo => "NEMO",
        }
    }
}

pub enum SubjectId {
    Basic = 1,
    Advanced = 2,
}

pub enum CommunityStatusType {
    WebForumStatus,
    WebFictionStatus,
}

impl CommunityStatusType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommunityStatusType::WebForumStatus => "WEB_FORUM_STATUS",
            CommunityStatusType::WebFictionStatus => "WEB_FICTION_STATUS",
        }
    }
}

pub enum OrderBy {
    UpdateTime,
    ViewTimes,
}

impl OrderBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            OrderBy::UpdateTime => "update_time",
            OrderBy::ViewTimes => "view_times",
        }
    }
}

pub enum ReadStatus {
    Read,
    Unread,
}

impl ReadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReadStatus::Read => "READ",
            ReadStatus::Unread => "UNREAD",
        }
    }
}

// ==================== CommunityDataFetcher ====================

pub struct CommunityDataFetcher {
    client: &'static CodeMaoClient,
}

impl CommunityDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取随机昵称
    pub async fn fetch_random_nickname(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/api/user/random/nickname", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取新消息数量
    pub async fn fetch_message_count(&self, method: MessageMethod) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, method.endpoint(), None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取回复
    pub async fn fetch_replies(
        &self,
        types: ReplyTypes,
        limit: i32,
        offset: i32,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/message-record", None)
            .with_param("query_type", types.as_str())
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取回复生成器
    pub fn fetch_replies_gen(&self, types: ReplyTypes, limit: Option<usize>) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/web/message-record")
            .with_param("query_type", types.as_str())
            .with_method(HttpMethod::GET)
            .with_pagination_method(PaginationMethod::Offset)
            .with_total_key("total")
            .with_data_key("items");

        paginated = paginated.with_limit(limit.unwrap_or(15));
        paginated
    }

    /// 获取nemo消息
    pub async fn fetch_nemo_messages(&self, types: &str) -> MewResult<Value> {
        let extra_url = if types == "like" { "1" } else { "3" };
        let endpoint = format!("/nemo/v2/user/message/{}", extra_url);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取pc客户端更新
    pub async fn fetch_pc_client(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/tiger/pc_client/releases/latest", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取点个猫更新 (外部URL)
    pub async fn fetch_pickcat_update(&self) -> MewResult<Value> {
        let response = reqwest::get("https://update.codemao.cn/updatev2/appsdk")
            .await
            .map_err(MewError::from)?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取10位时间戳
    pub async fn fetch_current_timestamp_10(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/coconut/clouddb/currentTime", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取13位时间戳 (外部URL)
    pub async fn fetch_current_timestamp_13(&self) -> MewResult<Value> {
        let response = reqwest::get("https://time.codemao.cn/time/current")
            .await
            .map_err(MewError::from)?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取kitten4更新
    pub async fn fetch_kitten4_update(&self) -> MewResult<Value> {
        let timestamp = self.fetch_current_timestamp_10().await?;
        let time_value = timestamp["data"].as_str().unwrap_or("");

        let response = reqwest::get(&format!(
            "https://kn-cdn.codemao.cn/kitten4/application/kitten4_update_info.json?TIME={}",
            time_value
        ))
        .await
        .map_err(MewError::from)?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取kitten更新
    pub async fn fetch_kitten_update(&self) -> MewResult<Value> {
        let timestamp = self.fetch_current_timestamp_10().await?;
        let time_value = timestamp["data"].as_str().unwrap_or("");

        let response = reqwest::get(&format!(
            "https://kn-cdn.codemao.cn/application/kitten_update_info.json?timeStamp={}",
            time_value
        ))
        .await
        .map_err(MewError::from)?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取海龟编辑器更新
    pub async fn fetch_wood_editor_update(&self) -> MewResult<Value> {
        let timestamp = self.fetch_current_timestamp_10().await?;
        let time_value = timestamp["data"].as_str().unwrap_or("");

        let response = reqwest::get(&format!(
            "https://static-am.codemao.cn/wood/client/xp/prod/package.json?timeStamp={}",
            time_value
        ))
        .await
        .map_err(MewError::from)?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取源码智造编辑器更新
    pub async fn fetch_matrix_editor_update(&self) -> MewResult<Value> {
        let timestamp = self.fetch_current_timestamp_10().await?;
        let time_value = timestamp["data"].as_str().unwrap_or("");

        let response = reqwest::get(&format!(
            "https://public-static-edu.codemao.cn/matrix/publish/desktop_matrix.json?timeStamp={}",
            time_value
        ))
        .await
        .map_err(MewError::from)?;
        response.json().await.map_err(MewError::from)
    }

    /// 获取Web端头图
    pub async fn fetch_web_banners(&self, banner_type: Option<BannerType>) -> MewResult<Value> {
        let mut builder = self
            .client
            .build_request(HttpMethod::GET, "/web/banners/all", None);

        if let Some(b_type) = banner_type {
            builder = builder.with_param("type", b_type.as_str());
        }

        builder.send().await?.json().await.map_err(MewError::from)
    }

    /// 获取Nemo端头图
    pub async fn fetch_nemo_banners(&self, banner_type: NemoBannerType) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/nemo/v2/home/banners", None)
            .with_param("banner_type", (banner_type as i32).to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取Coco端头图
    pub async fn fetch_coco_banners(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "/coconut/banner/list",
                Some(BaseKey::Creation),
            )
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取Coco话题
    pub async fn fetch_coco_topic(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "/coconut/topic/list",
                Some(BaseKey::Creation),
            )
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取举报类型
    pub async fn fetch_report_reasons(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/reports/reasons/all", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取nemo配置 (外部URL)
    pub async fn fetch_nemo_config(&self) -> MewResult<Value> {
        reqwest::get("https://nemo.codemao.cn/config")
            .await
            .map_err(MewError::from)?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取社区网络服务 (外部URL)
    pub async fn fetch_community_config(&self) -> MewResult<Value> {
        reqwest::get("https://c.codemao.cn/config")
            .await
            .map_err(MewError::from)?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取编程猫网络服务 (外部URL)
    pub async fn fetch_client_config(&self) -> MewResult<Value> {
        reqwest::get("https://player.codemao.cn/new/client_config.json")
            .await
            .map_err(MewError::from)?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取编程猫首页作品
    pub async fn fetch_recommended_works(
        &self,
        recommend_type: WorkRecommendType,
    ) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/pc/home/recommend-work",
                None,
            )
            .with_param("type", (recommend_type as i32).to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取nemo端新作喵喵看作品
    pub async fn fetch_new_recommend_works(&self, limit: i32, offset: i32) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/nemo/v3/new-recommend/more/list", None)
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取编程猫nemo作品推荐
    pub async fn fetch_recommended_works_nemo(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/nemo/v2/system/recommended/pool", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取编程猫首页推荐channel
    pub async fn fetch_work_channels(&self, channel_type: WorkChannelType) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/works/channels/list", None)
            .with_param("type", channel_type.as_str())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取指定channel
    pub async fn fetch_channel_works(
        &self,
        channel_id: i32,
        channel_type: WorkChannelType,
        limit: i32,
        page: i32,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/works/channels/{}/works", channel_id);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .with_param("type", channel_type.as_str())
            .with_param("page", page.to_string())
            .with_param("limit", limit.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取社区星推荐
    pub async fn fetch_recommended_users(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/users/recommended", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取训练师小课堂 (外部URL)
    pub async fn fetch_training_courses(&self) -> MewResult<Value> {
        reqwest::get("https://backend.box3.fun/diversion/codemao/post")
            .await
            .map_err(MewError::from)?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取KN课程
    pub async fn fetch_kn_courses(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/home/especially/course",
                None,
            )
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取KN公开课生成器
    pub fn fetch_public_courses_gen(&self, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/neko/course/publish/list")
            .with_param("limit", "10")
            .with_param("offset", "0")
            .with_total_key("total_course")
            .with_data_key("course_page.items")
            .with_limit(limit.unwrap_or(10))
            .with_base_key(BaseKey::Creation)
    }

    /// 获取KN模板作品
    pub async fn fetch_sample_works(&self, subject_id: SubjectId) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "/neko/sample/list",
                Some(BaseKey::Creation),
            )
            .with_param("subject_id", (subject_id as i32).to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取社区各个部分开启状态
    pub async fn fetch_community_status(
        &self,
        status_type: CommunityStatusType,
    ) -> MewResult<Value> {
        let endpoint = format!(
            "/web/config/tab/on-off/status?config_type={}",
            status_type.as_str()
        );

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取kitten编辑页面精选活动
    pub async fn fetch_kitten_activities(&self) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "/kitten/activity/choiceness/list",
                Some(BaseKey::Creation),
            )
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取nemo端教程合集生成器
    pub fn fetch_course_packages_gen(&self, platform: i32, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/creation-tools/v1/course/package/list")
            .with_param("limit", "50")
            .with_param("offset", "0")
            .with_param("platform", platform.to_string())
            .with_limit(limit.unwrap_or(50))
    }

    /// 获取nemo教程生成器
    pub fn fetch_course_details_gen(
        &self,
        course_package_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        self.client
            .paginated("/creation-tools/v1/course/list/search")
            .with_param("course_package_id", course_package_id.to_string())
            .with_param("limit", "50")
            .with_param("offset", "0")
            .with_data_key("course_page.items")
            .with_limit(limit.unwrap_or(50))
    }

    /// 获取教学计划生成器
    pub fn fetch_teaching_plans_gen(&self, limit: usize) -> PaginatedIter {
        self.client
            .paginated("/neko/teaching-plan/list/team")
            .with_param("limit", limit.to_string())
            .with_param("offset", "0")
            .with_limit(limit)
            .with_base_key(BaseKey::Creation)
    }

    /// 获取未读板块消息数量
    pub async fn fetch_board_unread_count(&self, board_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/forums/boards/{}/unread-count", board_id);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取活动页面
    pub async fn fetch_studio_info(&self, studio_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/studios/{}", studio_id);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取活动帖子生成器
    pub fn fetch_studio_posts_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/web/forums/posts")
            .with_param("limit", "50")
            .with_param("offset", "0")
            .with_param("studio_id", studio_id.to_string())
            .with_param("sort", "-created_at")
            .with_limit(limit.unwrap_or(24))
    }

    /// 获取活动教程生成器
    pub fn fetch_studio_courses_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/studios/{}/courses", studio_id);

        self.client
            .paginated(&endpoint)
            .with_param("limit", "50")
            .with_param("offset", "0")
            .with_limit(limit.unwrap_or(100))
    }

    /// 获取活动作品生成器
    pub fn fetch_studio_works_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/studios/{}/works", studio_id);

        self.client
            .paginated(&endpoint)
            .with_param("limit", "50")
            .with_param("offset", "0")
            .with_param("sort", "-n_likes")
            .with_limit(limit.unwrap_or(24))
    }

    /// 获取活动参加者生成器
    pub fn fetch_studio_participators_gen(
        &self,
        studio_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/studios/{}/participators", studio_id);

        self.client
            .paginated(&endpoint)
            .with_param("limit", "50")
            .with_param("offset", "0")
            .with_limit(limit.unwrap_or(24))
    }

    /// 获取旧版全部作品标签
    pub async fn fetch_work_labels(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/api/work/label/list", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取旧版全部作品标签
    pub async fn fetch_work_category(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/api/label/list", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取推荐作品
    pub async fn fetch_recommended_ide_works(
        &self,
        work_type: &str,
        page_number: i32,
        amount_items: i32,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/tiger/work/ide/recommended", None)
            .with_param("type", work_type)
            .with_param("page_number", page_number.to_string())
            .with_param("amount_items", amount_items.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取推荐作品
    pub async fn fetch_recommended_works_all(
        &self,
        work_type: &str,
        page_number: i32,
        amount_items: i32,
        order_by: OrderBy,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/tiger/work/list/all", None)
            .with_param("type", work_type)
            .with_param("page", page_number.to_string())
            .with_param("per_page", amount_items.to_string())
            .with_param("order_by", order_by.as_str())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取素材推荐
    pub async fn fetch_material_recommend(
        &self,
        category_id: i32,
        limit: i32,
        offset: i32,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/tiger/material/recommend", None)
            .with_param("category_id", category_id.to_string())
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }
}

impl Default for CommunityDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== UserAction ====================

pub struct UserAction {
    client: &'static CodeMaoClient,
}

impl UserAction {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 签订友好协议
    pub async fn execute_sign_agreement(&self) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/nemo/v3/user/level/signature", None)
            .with_payload(json!({}))
            .send()
            .await?;
        Ok(response.status().as_u16() == 200)
    }

    /// 获取用户协议
    pub async fn fetch_agreements(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/tiger/v3/web/accounts/agreements", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 注册
    pub async fn create_account(
        &self,
        identity: &str,
        password: &str,
        captcha: &str,
        pid: Option<&str>,
        agreement_ids: Option<Vec<i32>>,
    ) -> MewResult<Value> {
        let pid_value = pid.unwrap_or("65edCTyg");
        let agreement_values: Vec<Value> = match agreement_ids {
            Some(ids) => ids.into_iter().map(|id| json!(id)).collect(),
            None => vec![json!(186), json!(13)],
        };

        let payload = json!({
            "identity": identity,
            "password": password,
            "captcha": captcha,
            "pid": pid_value,
            "agreement_ids": agreement_values,
        });

        self.client
            .build_request(
                HttpMethod::POST,
                "/tiger/v3/web/accounts/register/phone/with-agreement",
                None,
            )
            .with_payload(payload)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 删除消息
    pub async fn delete_message(&self, message_id: i32) -> MewResult<bool> {
        let endpoint = format!("/web/message-record/{}", message_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .send()
            .await?;
        Ok(response.status().as_u16() == 204)
    }

    /// 获取广播消息生成器
    pub fn fetch_broadcast_messages_gen(
        &self,
        limit: Option<usize>,
        read_status: ReadStatus,
    ) -> PaginatedIter {
        self.client
            .paginated("/web/message-record/broadcast")
            .with_param("limit", "1")
            .with_param("offset", "0")
            .with_param("read_status", read_status.as_str())
            .with_param("sort", "-created_at")
            .with_limit(limit.unwrap_or(10))
    }
}

impl Default for UserAction {
    fn default() -> Self {
        Self::new()
    }
}
