use crate::utils::acquire::{CodeMaoClient, HttpMethod, PaginatedIter, PaginationMethod};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

// 回复类型枚举
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

// 消息方法枚举
pub enum MessageMethod {
    Web,
    Nemo,
}

// 头图类型枚举
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

// Nemo头图类型枚举
pub enum NemoBannerType {
    Type1 = 1,
    Type2 = 2,
    Type3 = 3,
}

// 作品推荐类型枚举
pub enum WorkRecommendType {
    Type1 = 1,
    Type2 = 2,
}

// 作品频道类型枚举
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

// 学科ID枚举
pub enum SubjectId {
    Basic = 1,
    Advanced = 2,
}

// 社区状态类型枚举
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

// 作品排序方式枚举
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

// 消息阅读状态枚举
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

pub struct DataFetcher {
    client: Arc<Mutex<CodeMaoClient>>,
}

impl DataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取随机昵称
    pub fn fetch_random_nickname(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/api/user/random/nickname",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取新消息数量
    pub fn fetch_message_count(
        &self,
        method: MessageMethod,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = match method {
            MessageMethod::Web => "/web/message-record/count",
            MessageMethod::Nemo => "/nemo/v2/user/message/count",
        };

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            endpoint,
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取回复
    pub fn fetch_replies(
        &self,
        types: ReplyTypes,
        limit: i32,
        offset: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("query_type".to_string(), types.as_str().to_string());
        params.insert("limit".to_string(), limit.to_string());
        params.insert("offset".to_string(), offset.to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/web/message-record",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取回复生成器
    pub fn fetch_replies_gen(&self, types: ReplyTypes, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("query_type".to_string(), types.as_str().to_string());

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated("/web/message-record")
            .with_params(params)
            .with_method(HttpMethod::GET)
            .with_pagination_method(PaginationMethod::Offset)
            .with_total_key("total")
            .with_data_key("items");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取nemo消息
    pub fn fetch_nemo_messages(&self, types: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let extra_url = if types == "like" { "1" } else { "3" };
        let endpoint = format!("/nemo/v2/user/message/{}", extra_url);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            &endpoint,
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取pc客户端更新
    pub fn fetch_pc_client(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/tiger/pc_client/releases/latest",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取点个猫更新
    pub fn fetch_pickcat_update(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://update.codemao.cn/updatev2/appsdk",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取kitten4更新
    pub fn fetch_kitten4_update(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = self.fetch_current_timestamp_10()?;
        let time_value = timestamp["data"].as_str().unwrap_or("").to_string();

        let mut params = HashMap::new();
        params.insert("TIME".to_string(), time_value);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://kn-cdn.codemao.cn/kitten4/application/kitten4_update_info.json",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取kitten更新
    pub fn fetch_kitten_update(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = self.fetch_current_timestamp_10()?;
        let time_value = timestamp["data"].as_str().unwrap_or("").to_string();

        let mut params = HashMap::new();
        params.insert("timeStamp".to_string(), time_value);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://kn-cdn.codemao.cn/application/kitten_update_info.json",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取海龟编辑器更新
    pub fn fetch_wood_editor_update(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = self.fetch_current_timestamp_10()?;
        let time_value = timestamp["data"].as_str().unwrap_or("").to_string();

        let mut params = HashMap::new();
        params.insert("timeStamp".to_string(), time_value);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://static-am.codemao.cn/wood/client/xp/prod/package.json",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取源码智造编辑器更新
    pub fn fetch_matrix_editor_update(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let timestamp = self.fetch_current_timestamp_10()?;
        let time_value = timestamp["data"].as_str().unwrap_or("").to_string();

        let mut params = HashMap::new();
        params.insert("timeStamp".to_string(), time_value);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://public-static-edu.codemao.cn/matrix/publish/desktop_matrix.json",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取10位时间戳
    pub fn fetch_current_timestamp_10(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/coconut/clouddb/currentTime",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取13位时间戳
    pub fn fetch_current_timestamp_13(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://time.codemao.cn/time/current",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取Web端头图
    pub fn fetch_web_banners(
        &self,
        banner_type: Option<BannerType>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        if let Some(b_type) = banner_type {
            params.insert("type".to_string(), b_type.as_str().to_string());
        }

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/web/banners/all",
            if params.is_empty() {
                None
            } else {
                Some(&params)
            },
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取Nemo端头图
    pub fn fetch_nemo_banners(
        &self,
        banner_type: NemoBannerType,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("banner_type".to_string(), (banner_type as i32).to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/nemo/v2/home/banners",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取Coco端头图
    pub fn fetch_coco_banners(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/coconut/banner/list",
            None,
            None,
            Some("creation"),
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取Coco话题
    pub fn fetch_coco_topic(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/coconut/topic/list",
            None,
            None,
            Some("creation"),
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取举报类型
    pub fn fetch_report_reasons(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/web/reports/reasons/all",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取nemo配置
    pub fn fetch_nemo_config(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://nemo.codemao.cn/config",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取社区网络服务
    pub fn fetch_community_config(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://c.codemao.cn/config",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取编程猫网络服务
    pub fn fetch_client_config(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://player.codemao.cn/new/client_config.json",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取编程猫首页作品
    pub fn fetch_recommended_works(
        &self,
        recommend_type: WorkRecommendType,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("type".to_string(), (recommend_type as i32).to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/creation-tools/v1/pc/home/recommend-work",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取nemo端新作喵喵看作品
    pub fn fetch_new_recommend_works(
        &self,
        limit: i32,
        offset: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), limit.to_string());
        params.insert("offset".to_string(), offset.to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/nemo/v3/new-recommend/more/list",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取编程猫nemo作品推荐
    pub fn fetch_recommended_works_nemo(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/nemo/v2/system/recommended/pool",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取编程猫首页推荐channel
    pub fn fetch_work_channels(
        &self,
        channel_type: WorkChannelType,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("type".to_string(), channel_type.as_str().to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/web/works/channels/list",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取指定channel
    pub fn fetch_channel_works(
        &self,
        channel_id: i32,
        channel_type: WorkChannelType,
        limit: i32,
        page: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("type".to_string(), channel_type.as_str().to_string());
        params.insert("page".to_string(), page.to_string());
        params.insert("limit".to_string(), limit.to_string());

        let endpoint = format!("/web/works/channels/{}/works", channel_id);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            &endpoint,
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取社区星推荐
    pub fn fetch_recommended_users(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/web/users/recommended",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取训练师小课堂
    pub fn fetch_training_courses(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "https://backend.box3.fun/diversion/codemao/post",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取KN课程
    pub fn fetch_kn_courses(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/creation-tools/v1/home/especially/course",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取KN公开课生成器
    pub fn fetch_public_courses_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "10".to_string());
        params.insert("offset".to_string(), "0".to_string());

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated("/neko/course/publish/list")
            .with_params(params)
            .with_total_key("total_course")
            .with_data_key("course_page.items")
            .with_base_url("creation");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }

        paginated
    }

    // 获取KN模板作品
    pub fn fetch_sample_works(
        &self,
        subject_id: SubjectId,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("subject_id".to_string(), (subject_id as i32).to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/neko/sample/list",
            Some(&params),
            None,
            Some("creation"),
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取社区各个部分开启状态
    pub fn fetch_community_status(
        &self,
        status_type: CommunityStatusType,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!(
            "/web/config/tab/on-off/status?config_type={}",
            status_type.as_str()
        );

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            &endpoint,
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取kitten编辑页面精选活动
    pub fn fetch_kitten_activities(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/kitten/activity/choiceness/list",
            None,
            None,
            Some("creation"),
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取nemo端教程合集生成器
    pub fn fetch_course_packages_gen(&self, platform: i32, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "50".to_string());
        params.insert("offset".to_string(), "0".to_string());
        params.insert("platform".to_string(), platform.to_string());

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated("/creation-tools/v1/course/package/list")
            .with_params(params);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(50);
        }

        paginated
    }

    // 获取nemo教程生成器
    pub fn fetch_course_details_gen(
        &self,
        course_package_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert(
            "course_package_id".to_string(),
            course_package_id.to_string(),
        );
        params.insert("limit".to_string(), "50".to_string());
        params.insert("offset".to_string(), "0".to_string());

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated("/creation-tools/v1/course/list/search")
            .with_params(params)
            .with_data_key("course_page.items");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(50);
        }

        paginated
    }

    // 获取教学计划生成器
    pub fn fetch_teaching_plans_gen(&self, limit: usize) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), limit.to_string());
        params.insert("offset".to_string(), "0".to_string());

        self.client
            .lock()
            .unwrap()
            .paginated("/neko/teaching-plan/list/team")
            .with_params(params)
            .with_limit(limit)
            .with_base_url("creation")
    }

    // 获取未读板块消息数量
    pub fn fetch_board_unread_count(
        &self,
        board_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/forums/boards/{}/unread-count", board_id);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            &endpoint,
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取活动页面
    pub fn fetch_studio_info(&self, studio_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/studios/{}", studio_id);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            &endpoint,
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取活动帖子生成器
    pub fn fetch_studio_posts_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "50".to_string());
        params.insert("offset".to_string(), "0".to_string());
        params.insert("studio_id".to_string(), studio_id.to_string());
        params.insert("sort".to_string(), "-created_at".to_string());

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated("/web/forums/posts")
            .with_params(params);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(24);
        }

        paginated
    }

    // 获取活动教程生成器
    pub fn fetch_studio_courses_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "50".to_string());
        params.insert("offset".to_string(), "0".to_string());

        let endpoint = format!("/web/studios/{}/courses", studio_id);

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated(&endpoint)
            .with_params(params);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(100);
        }

        paginated
    }

    // 获取活动作品生成器
    pub fn fetch_studio_works_gen(&self, studio_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "50".to_string());
        params.insert("offset".to_string(), "0".to_string());
        params.insert("sort".to_string(), "-n_likes".to_string());

        let endpoint = format!("/web/studios/{}/works", studio_id);

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated(&endpoint)
            .with_params(params);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(24);
        }

        paginated
    }

    // 获取活动参加者生成器
    pub fn fetch_studio_participators_gen(
        &self,
        studio_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "50".to_string());
        params.insert("offset".to_string(), "0".to_string());

        let endpoint = format!("/web/studios/{}/participators", studio_id);

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated(&endpoint)
            .with_params(params);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(24);
        }

        paginated
    }

    // 获取旧版全部作品标签
    pub fn fetch_work_labels(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/api/work/label/list",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取旧版全部作品标签
    pub fn fetch_work_category(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/api/label/list",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取推荐作品
    pub fn fetch_recommended_ide_works(
        &self,
        work_type: &str,
        page_number: i32,
        amount_items: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("type".to_string(), work_type.to_string());
        params.insert("page_number".to_string(), page_number.to_string());
        params.insert("amount_items".to_string(), amount_items.to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/tiger/work/ide/recommended",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取推荐作品
    pub fn fetch_recommended_works_all(
        &self,
        work_type: &str,
        page_number: i32,
        amount_items: i32,
        order_by: OrderBy,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("type".to_string(), work_type.to_string());
        params.insert("page".to_string(), page_number.to_string());
        params.insert("per_page".to_string(), amount_items.to_string());
        params.insert("order_by".to_string(), order_by.as_str().to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/tiger/work/list/all",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 获取素材推荐
    pub fn fetch_material_recommend(
        &self,
        category_id: i32,
        limit: i32,
        offset: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("category_id".to_string(), category_id.to_string());
        params.insert("limit".to_string(), limit.to_string());
        params.insert("offset".to_string(), offset.to_string());

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/tiger/material/recommend",
            Some(&params),
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }
}

impl Default for DataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// UserAction结构体
pub struct UserAction {
    client: Arc<Mutex<CodeMaoClient>>,
}

impl UserAction {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 签订友好协议
    pub fn execute_sign_agreement(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/nemo/v3/user/level/signature",
            None,
            None,
            None,
        )?;
        Ok(response.status() == 200)
    }

    // 获取用户协议
    pub fn fetch_agreements(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.lock().unwrap().send_request(
            HttpMethod::GET,
            "/tiger/v3/web/accounts/agreements",
            None,
            None,
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 注册
    pub fn create_account(
        &self,
        identity: &str,
        password: &str,
        captcha: &str,
        pid: Option<&str>,
        agreement_ids: Option<Vec<i32>>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut data = serde_json::Map::new();
        data.insert("identity".to_string(), Value::String(identity.to_string()));
        data.insert("password".to_string(), Value::String(password.to_string()));
        data.insert("captcha".to_string(), Value::String(captcha.to_string()));

        let pid_value = pid.unwrap_or("65edCTyg");
        data.insert("pid".to_string(), Value::String(pid_value.to_string()));

        let agreement_values = match agreement_ids {
            Some(ids) => ids.into_iter().map(|id| Value::Number(id.into())).collect(),
            None => vec![Value::Number(186.into()), Value::Number(13.into())],
        };
        data.insert("agreement_ids".to_string(), Value::Array(agreement_values));

        let payload = Value::Object(data);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::POST,
            "/tiger/v3/web/accounts/register/phone/with-agreement",
            None,
            Some(&payload),
            None,
        )?;
        Ok(CodeMaoClient::response_to_json(response)?)
    }

    // 删除消息
    pub fn delete_message(&self, message_id: i32) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/message-record/{}", message_id);

        let response = self.client.lock().unwrap().send_request(
            HttpMethod::DELETE,
            &endpoint,
            None,
            None,
            None,
        )?;
        Ok(response.status() == 204)
    }

    // 获取广播消息生成器
    pub fn fetch_broadcast_messages_gen(
        &self,
        limit: Option<usize>,
        read_status: ReadStatus,
    ) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), "1".to_string());
        params.insert("offset".to_string(), "0".to_string());
        params.insert("read_status".to_string(), read_status.as_str().to_string());
        params.insert("sort".to_string(), "-created_at".to_string());

        let mut paginated = self
            .client
            .lock()
            .unwrap()
            .paginated("/web/message-record/broadcast")
            .with_params(params);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }

        paginated
    }
}

impl Default for UserAction {
    fn default() -> Self {
        Self::new()
    }
}
