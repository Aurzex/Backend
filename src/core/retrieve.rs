use std::collections::HashMap;
use std::sync::OnceLock;

use rand::RngExt;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::api::auth::AuthManager;
use crate::api::community::{
    DataFetcher as CommunityDataFetcher, MessageMethod, ReplyTypes,
    UserAction as CommunityUserAction,
};
use crate::api::education::{DataFetcher as EduDataFetcher, UserAction as EduUserAction};
use crate::api::forum::{ForumActionHandler, ForumDataFetcher};
use crate::api::library::{NovelActionHandler, NovelDataFetcher};
use crate::api::shop::{WorkshopActionHandler, WorkshopDataFetcher};
use crate::api::user::{UserDataFetcher, UserManager};
use crate::api::whale::{
    CommentReportFilterType, CommentSourceType, ReportFetcher, ReportHandler, ReportStatus,
    WorkReportFilterType, WorkSourceType,
};
use crate::api::work::{KittenWorkManager, NemoWorkType, WorkDataFetcher};
use crate::utils::acquire::{BaseKey, ClientFactory, CodeMaoClient, HttpMethod, Identity};
use crate::utils::data::{
    CacheManager, CodeMaoFile, DataManager, HistoryManager, PathConfig, SettingManager,
};

/// 全局协调器：集中管理所有 API 模块和全局资源
pub struct Coordinator {
    // 认证模块
    pub auth: AuthManager,

    // 社区模块
    pub community_action: CommunityUserAction,
    pub community_fetcher: CommunityDataFetcher,

    // 教育模块
    pub edu_action: EduUserAction,
    pub edu_fetcher: EduDataFetcher,

    // 论坛模块
    pub forum_action: ForumActionHandler,
    pub forum_fetcher: ForumDataFetcher,

    // 小说模块
    pub novel_action: NovelActionHandler,
    pub novel_fetcher: NovelDataFetcher,

    // 工坊模块
    pub shop_action: WorkshopActionHandler,
    pub shop_fetcher: WorkshopDataFetcher,

    // 用户模块
    pub user_action: UserManager,
    pub user_fetcher: UserDataFetcher,

    // 举报模块（鲸鱼系统）
    pub report_action: ReportHandler,
    pub report_fetcher: ReportFetcher,

    // 作品模块
    pub work_action: KittenWorkManager,
    pub work_fetcher: WorkDataFetcher,

    // 全局资源
    pub client: &'static CodeMaoClient,
    pub data_manager: &'static DataManager,
    pub path_config: PathConfig,
    pub setting_manager: &'static SettingManager,
    pub cache_manager: &'static CacheManager,
    pub history_manager: &'static HistoryManager,
    pub file_manager: CodeMaoFile,
}

static COORDINATOR: OnceLock<Coordinator> = OnceLock::new();

/// 初始化全局协调器
pub fn init_coordinator() {
    let coordinator = Coordinator {
        auth: AuthManager::new(),
        community_action: CommunityUserAction::new(),
        community_fetcher: CommunityDataFetcher::new(),
        client: ClientFactory::global_client(),
        edu_action: EduUserAction::new(),
        edu_fetcher: EduDataFetcher::new(),
        forum_action: ForumActionHandler::new(),
        forum_fetcher: ForumDataFetcher::new(),
        novel_action: NovelActionHandler::new(),
        novel_fetcher: NovelDataFetcher::new(),
        shop_action: WorkshopActionHandler::new(),
        shop_fetcher: WorkshopDataFetcher::new(),
        user_action: UserManager::new(),
        user_fetcher: UserDataFetcher::new(),
        report_action: ReportHandler::new(),
        report_fetcher: ReportFetcher::new(),
        work_action: KittenWorkManager::new(),
        work_fetcher: WorkDataFetcher::new(),
        data_manager: DataManager::global(),
        path_config: PathConfig,
        setting_manager: SettingManager::global(),
        cache_manager: CacheManager::global(),
        history_manager: HistoryManager::global(),
        file_manager: CodeMaoFile,
    };

    COORDINATOR.set(coordinator);
}

/// 获取全局协调器实例
pub fn coordinator() -> &'static Coordinator {
    COORDINATOR.get().expect("Coordinator not initialized")
}

// ==================== 错误类型 ====================

#[derive(Error, Debug)]
pub enum DataQueryError {
    #[error("无效的来源类型: {0}")]
    InvalidSource(String),
    #[error("无效的查询方法: {0}")]
    InvalidMethod(String),
    #[error("网络请求失败: {0}")]
    RequestFailed(String),
    #[error("数据解析失败: {0}")]
    ParseError(String),
    #[error("未找到请求的资源")]
    NotFound,
    #[error("内部错误: {0}")]
    Internal(String),
    #[error("外部错误: {0}")]
    External(#[from] Box<dyn std::error::Error>),
}

// ==================== 枚举定义 ====================

/// 评论来源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentSource {
    Work,  // 作品评论
    Forum, // 论坛帖子评论
    Shop,  // 工坊讨论评论
}

impl CommentSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommentSource::Work => "work",
            CommentSource::Forum => "forum",
            CommentSource::Shop => "shop",
        }
    }
}

impl std::str::FromStr for CommentSource {
    type Err = DataQueryError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "work" => Ok(CommentSource::Work),
            "forum" => Ok(CommentSource::Forum),
            "shop" => Ok(CommentSource::Shop),
            _ => Err(DataQueryError::InvalidSource(s.to_string())),
        }
    }
}

/// 评论查询模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentQueryMode {
    UserId,    // 提取用户ID列表
    CommentId, // 提取评论ID列表
    Comments,  // 获取详细评论数据
}

impl CommentQueryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommentQueryMode::UserId => "user_id",
            CommentQueryMode::CommentId => "comment_id",
            CommentQueryMode::Comments => "comments",
        }
    }
}

impl std::str::FromStr for CommentQueryMode {
    type Err = DataQueryError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_id" => Ok(CommentQueryMode::UserId),
            "comment_id" => Ok(CommentQueryMode::CommentId),
            "comments" => Ok(CommentQueryMode::Comments),
            _ => Err(DataQueryError::InvalidMethod(s.to_string())),
        }
    }
}

/// 通知类型分类
#[derive(Debug, Clone, Copy)]
pub enum NotificationCategory {
    LikeFork,     // 点赞/收藏
    CommentReply, // 评论/回复
    System,       // 系统通知
}

impl NotificationCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotificationCategory::LikeFork => "LIKE_FORK",
            NotificationCategory::CommentReply => "COMMENT_REPLY",
            NotificationCategory::System => "SYSTEM",
        }
    }
}

// ==================== 数据类型别名 ====================

pub type JsonValue = Value;
pub type JsonObject = Map<String, Value>;

// ==================== 结果类型 ====================

/// 评论查询结果枚举
#[derive(Debug, Clone)]
pub enum CommentsResult {
    UserIdList(Vec<String>),           // 用户ID列表
    CommentIdList(Vec<String>),        // 评论ID列表
    DetailedComments(Vec<JsonObject>), // 详细评论数据
}

// ==================== 查询构建器 ====================

/// 评论查询构建器
pub struct CommentQueryBuilder {
    source: Option<CommentSource>,
    target_id: Option<i32>,
    mode: CommentQueryMode,
    limit: Option<usize>,
}

impl CommentQueryBuilder {
    fn new() -> Self {
        Self {
            source: None,
            target_id: None,
            mode: CommentQueryMode::UserId,
            limit: Some(500),
        }
    }

    pub fn source(mut self, source: CommentSource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn target_id(mut self, id: i32) -> Self {
        self.target_id = Some(id);
        self
    }

    pub fn mode(mut self, mode: CommentQueryMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }

    /// 根据来源获取评论数据的惰性迭代器
    fn build_comment_stream(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Result<JsonValue, DataQueryError>>>, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let target_id = self
            .target_id
            .ok_or_else(|| DataQueryError::InvalidSource("未设置源ID".into()))?;
        let limit = self.limit;
        let coord = coordinator();

        match source {
            CommentSource::Work => {
                let iter = coord
                    .work_fetcher
                    .fetch_work_comments_gen(target_id, limit)
                    .map(|item| item.map_err(|e| DataQueryError::External(e.into())));
                Ok(Box::new(iter))
            }
            CommentSource::Forum => {
                let iter = coord
                    .forum_fetcher
                    .fetch_post_replies_gen(target_id, None, limit)
                    .map(|item| item.map_err(|e| DataQueryError::External(e.into())));
                Ok(Box::new(iter))
            }
            CommentSource::Shop => {
                let iter = coord
                    .shop_fetcher
                    .fetch_workshop_discussions_gen(target_id, None, None, limit)
                    .map(|item| item.map_err(|e| DataQueryError::External(e.into())));
                Ok(Box::new(iter))
            }
        }
    }

    /// 执行评论查询
    pub fn execute(self) -> Result<CommentsResult, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let mode = self.mode;

        let comment_stream = self.build_comment_stream()?;
        let comments: Vec<JsonValue> = comment_stream.collect::<Result<Vec<_>, _>>()?;

        let user_field = match source {
            CommentSource::Work | CommentSource::Shop => "reply_user",
            CommentSource::Forum => "user",
        };

        let mut reply_cache: HashMap<i64, Vec<JsonObject>> = HashMap::new();

        let extract_reply_user_id = |reply: &JsonObject| -> Option<i64> {
            reply
                .get(user_field)
                .and_then(|u| u.as_object())
                .and_then(|u| u.get("id"))
                .and_then(|id| id.as_i64())
        };

        let mut fetch_replies = |comment: &Value| -> Result<Vec<JsonObject>, DataQueryError> {
            let comment_obj = comment
                .as_object()
                .ok_or_else(|| DataQueryError::ParseError("评论数据格式错误".into()))?;

            if source == CommentSource::Forum {
                let comment_id = comment_obj
                    .get("id")
                    .and_then(|id| id.as_i64())
                    .ok_or(DataQueryError::ParseError("缺少评论ID".into()))?;

                if !reply_cache.contains_key(&comment_id) {
                    let replies_iter = coordinator()
                        .forum_fetcher
                        .fetch_reply_comments_gen(comment_id as i32, None);
                    let replies: Vec<JsonObject> = replies_iter
                        .filter_map(|item| match item {
                            Ok(val) => val.as_object().cloned(),
                            Err(_) => None,
                        })
                        .collect();
                    reply_cache.insert(comment_id, replies);
                }
                Ok(reply_cache.get(&comment_id).cloned().unwrap_or_default())
            } else {
                let replies = comment
                    .get("replies")
                    .and_then(|r| r.as_object())
                    .and_then(|r| r.get("items"))
                    .and_then(|items| items.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_object().cloned()).collect())
                    .unwrap_or_default();
                Ok(replies)
            }
        };

        match mode {
            CommentQueryMode::UserId => {
                let mut user_ids = Vec::new();
                for comment in &comments {
                    if let Some(id) = comment
                        .get("user")
                        .and_then(|u| u.as_object())
                        .and_then(|u| u.get("id"))
                        .and_then(|id| id.as_i64())
                    {
                        user_ids.push(id.to_string());
                    }
                    for reply in fetch_replies(comment)? {
                        if let Some(uid) = extract_reply_user_id(&reply) {
                            user_ids.push(uid.to_string());
                        }
                    }
                }
                Ok(CommentsResult::UserIdList(deduplicate(&user_ids)))
            }
            CommentQueryMode::CommentId => {
                let mut comment_ids = Vec::new();
                for comment in &comments {
                    if let Some(id) = comment.get("id").and_then(|id| id.as_i64()) {
                        comment_ids.push(id.to_string());
                    }
                    let comment_id = comment.get("id").and_then(|id| id.as_i64());
                    for reply in fetch_replies(comment)? {
                        if let (Some(cid), Some(rid)) =
                            (comment_id, reply.get("id").and_then(|id| id.as_i64()))
                        {
                            comment_ids.push(format!("{}.{}", cid, rid));
                        }
                    }
                }
                Ok(CommentsResult::CommentIdList(deduplicate(&comment_ids)))
            }
            CommentQueryMode::Comments => {
                let mut detailed = Vec::new();
                for comment in &comments {
                    let replies: Vec<JsonObject> = fetch_replies(comment)?
                        .into_iter()
                        .filter_map(|reply| {
                            let id = reply.get("id")?.as_i64()?;
                            let content = reply
                                .get("content")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string();
                            let created_at = reply
                                .get("created_at")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string();
                            let user_id = extract_reply_user_id(&reply)?;
                            let nickname = reply
                                .get(user_field)
                                .and_then(|u| u.as_object())
                                .and_then(|u| u.get("nickname"))
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string();

                            let mut obj = JsonObject::new();
                            obj.insert("id".into(), Value::Number(id.into()));
                            obj.insert("content".into(), Value::String(content));
                            obj.insert("created_at".into(), Value::String(created_at));
                            obj.insert("user_id".into(), Value::Number(user_id.into()));
                            obj.insert("nickname".into(), Value::String(nickname));
                            Some(obj)
                        })
                        .collect();

                    let mut comment_data = JsonObject::new();
                    if let Some(user) = comment.get("user").and_then(|u| u.as_object()) {
                        if let Some(id) = user.get("id") {
                            comment_data.insert("user_id".into(), id.clone());
                        }
                        if let Some(nick) = user.get("nickname") {
                            comment_data.insert("nickname".into(), nick.clone());
                        }
                    }
                    if let Some(id) = comment.get("id") {
                        comment_data.insert("id".into(), id.clone());
                    }
                    if let Some(content) = comment.get("content") {
                        comment_data.insert("content".into(), content.clone());
                    }
                    if let Some(created_at) = comment.get("created_at") {
                        comment_data.insert("created_at".into(), created_at.clone());
                    }
                    comment_data.insert(
                        "is_top".into(),
                        Value::Bool(
                            comment
                                .get("is_top")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        ),
                    );
                    comment_data.insert(
                        "replies".into(),
                        Value::Array(replies.into_iter().map(Value::Object).collect()),
                    );

                    detailed.push(comment_data);
                }
                Ok(CommentsResult::DetailedComments(detailed))
            }
        }
    }
}

// ==================== 数据查询主结构体 ====================

/// 数据查询与统计入口
pub struct DataQuery;

impl DataQuery {
    pub fn new() -> Self {
        DataQuery
    }

    /// 创建评论查询构建器
    pub fn query_comments(&self) -> CommentQueryBuilder {
        CommentQueryBuilder::new()
    }

    /// 获取评论数据（快捷方法）
    pub fn fetch_comments(
        &self,
        source: CommentSource,
        target_id: i32,
        mode: CommentQueryMode,
        limit: Option<usize>,
    ) -> Result<CommentsResult, DataQueryError> {
        self.query_comments()
            .source(source)
            .target_id(target_id)
            .mode(mode)
            .limit(limit)
            .execute()
    }

    /// 获取社区新回复流（惰性迭代器）
    pub fn stream_new_replies(
        &self,
        reply_type: ReplyTypes,
        limit: i32,
    ) -> Box<dyn Iterator<Item = Result<JsonObject, DataQueryError>> + 'static> {
        let coord = coordinator();
        let total = match coord
            .community_fetcher
            .fetch_message_count(MessageMethod::Web)
            .map_err(|e| DataQueryError::External(e.into()))
        {
            Ok(data) => data.get("count").and_then(|c| c.as_i64()).unwrap_or(0) as i32,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };

        let remaining = if limit == 0 { total } else { limit.min(total) };

        Box::new(CommunityReplyStream {
            coord,
            reply_type,
            remaining,
            offset: 0,
            buffer: std::collections::VecDeque::new(),
        })
    }

    /// 获取评论总数
    pub fn count_comments(
        &self,
        source: CommentSource,
        target_id: i32,
    ) -> Result<i32, DataQueryError> {
        let coord = coordinator();
        match source {
            CommentSource::Work => {
                let comments_response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}/comments", target_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "15")
                    .send()
                    .map_err(|e| DataQueryError::External(e.into()))?;

                let json = coord
                    .client
                    .response_to_json(comments_response)
                    .map_err(|e| DataQueryError::External(e.into()))?;

                if let Some(total) = json.get("total").and_then(|t| t.as_i64()) {
                    return Ok(total as i32);
                }

                let work_response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}", target_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .map_err(|e| DataQueryError::External(e.into()))?;

                let work_json = coord
                    .client
                    .response_to_json(work_response)
                    .map_err(|e| DataQueryError::External(e.into()))?;

                Ok(work_json
                    .get("comment_times")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32)
            }
            CommentSource::Shop => {
                let response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/discussions/{}/comments", target_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("source", "WORK_SHOP")
                    .with_param("sort", "-created_at")
                    .with_param("limit", "15")
                    .with_param("offset", "0")
                    .send()
                    .map_err(|e| DataQueryError::External(e.into()))?;

                let json = coord
                    .client
                    .response_to_json(response)
                    .map_err(|e| DataQueryError::External(e.into()))?;

                let total = json.get("total").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                let total_reply =
                    json.get("totalReply").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                Ok(total + total_reply)
            }
            CommentSource::Forum => {
                let response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/forums/posts/{}/details", target_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .map_err(|e| DataQueryError::External(e.into()))?;

                let json = coord
                    .client
                    .response_to_json(response)
                    .map_err(|e| DataQueryError::External(e.into()))?;

                let n_replies = json.get("n_replies").and_then(|r| r.as_i64()).unwrap_or(0) as i32;
                let n_comments =
                    json.get("n_comments").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
                Ok(n_replies + n_comments)
            }
        }
    }

    /// 合并 Nemo 和 Web 来源的作品数据流
    pub fn stream_works_from_both_sources(
        &self,
        limit: i32,
    ) -> Box<dyn Iterator<Item = Result<JsonObject, DataQueryError>> + 'static> {
        let coord = coordinator();
        let per_source_limit = Some(limit / 2);

        let nemo_field_mapping: HashMap<&str, &str> = [
            ("work_id", "work_id"),
            ("work_name", "work_name"),
            ("user_name", "user_name"),
            ("user_id", "user_id"),
            ("like_count", "like_count"),
            ("updated_at", "updated_at"),
        ]
        .into_iter()
        .collect();

        let web_field_mapping: HashMap<&str, &str> = [
            ("work_id", "work_id"),
            ("work_name", "work_name"),
            ("user_name", "nickname"),
            ("user_id", "user_id"),
            ("like_count", "likes_count"),
            ("updated_at", "updated_at"),
        ]
        .into_iter()
        .collect();

        let nemo_result =
            coord
                .work_fetcher
                .fetch_new_works_nemo(NemoWorkType::Original, per_source_limit, None);
        let web_result = coord
            .work_fetcher
            .fetch_new_works_web(per_source_limit, None, true);

        let process_result = |res: Result<Value, _>, mapping: HashMap<&str, &str>| match res {
            Ok(val) => {
                let items = val
                    .get("items")
                    .and_then(|i| i.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mapped: Vec<Result<JsonObject, DataQueryError>> = items
                    .into_iter()
                    .filter_map(|v| v.as_object().cloned())
                    .map(move |obj| {
                        let mut mapped_obj = JsonObject::new();
                        for (target, source) in &mapping {
                            if let Some(val) = obj.get(*source) {
                                mapped_obj.insert(target.to_string(), val.clone());
                            }
                        }
                        Ok(mapped_obj)
                    })
                    .collect();
                Box::new(mapped.into_iter())
                    as Box<dyn Iterator<Item = Result<JsonObject, DataQueryError>>>
            }
            Err(e) => Box::new(std::iter::once::<Result<JsonObject, DataQueryError>>(Err(
                DataQueryError::External(e),
            ))),
        };

        let nemo_stream = process_result(nemo_result, nemo_field_mapping);
        let web_stream = process_result(web_result, web_field_mapping);

        Box::new(nemo_stream.chain(web_stream))
    }

    /// 从作品中收集用户评论并聚合统计
    pub fn aggregate_user_comments_from_works(
        &self,
        work_limit: i32,
    ) -> Result<Vec<JsonObject>, DataQueryError> {
        let works: Vec<JsonObject> = self
            .stream_works_from_both_sources(work_limit)
            .collect::<Result<Vec<_>, _>>()?;

        let mut all_comments = Vec::new();
        for work in &works {
            if let Some(work_id) = work.get("work_id").and_then(|id| id.as_i64()) {
                if let Ok(CommentsResult::DetailedComments(comments)) = self.fetch_comments(
                    CommentSource::Work,
                    work_id as i32,
                    CommentQueryMode::Comments,
                    Some(20),
                ) {
                    all_comments.extend(comments);
                }
            }
        }

        let mut user_comment_map: HashMap<String, (String, String, Vec<String>, i32)> =
            HashMap::new();
        for comment in &all_comments {
            let user_id = comment.get("user_id").and_then(|v| {
                if v.is_number() {
                    Some(v.to_string())
                } else {
                    v.as_str().map(|s| s.to_string())
                }
            });
            let content = comment
                .get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string());
            let nickname = comment
                .get("nickname")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());

            if let (Some(uid), Some(cont), Some(nick)) = (user_id, content, nickname) {
                let entry = user_comment_map
                    .entry(uid.clone())
                    .or_insert_with(|| (uid, nick, Vec::new(), 0));
                entry.2.push(cont);
                entry.3 += 1;
            }
        }

        let mut result: Vec<JsonObject> = user_comment_map
            .into_values()
            .map(|(uid, nick, comments, count)| {
                let mut obj = JsonObject::new();
                obj.insert("user_id".into(), Value::String(uid));
                obj.insert("nickname".into(), Value::String(nick));
                obj.insert(
                    "comments".into(),
                    Value::Array(comments.into_iter().map(Value::String).collect()),
                );
                obj.insert("comment_count".into(), Value::Number(count.into()));
                obj
            })
            .collect();

        result.sort_by(|a, b| {
            let ca = a.get("comment_count").and_then(|c| c.as_i64()).unwrap_or(0);
            let cb = b.get("comment_count").and_then(|c| c.as_i64()).unwrap_or(0);
            cb.cmp(&ca)
        });

        Ok(result)
    }

    /// 获取管理员举报处理统计
    pub fn compute_admin_report_stats(&self) -> Result<AdminReportStatistics, DataQueryError> {
        let coord = coordinator();
        let admins = [
            (220, "石榴 Grant"),
            (222, "shidang88"),
            (223, "喵鱼 a"),
            (224, "沙雕的初小白"),
            (225, "旁观者 JErS"),
            (226, "宜壳乐 Cat"),
            (227, "凌风光耀 Aug"),
            (228, "奇怪的小蜜桃"),
        ];

        let mut stats = Vec::new();
        let mut total_comment_reports = 0;
        let mut total_work_reports = 0;

        for &(admin_id, admin_name) in &admins {
            let comment_count = coord
                .report_fetcher
                .fetch_comment_reports_total(
                    CommentSourceType::All,
                    ReportStatus::All,
                    Some(CommentReportFilterType::AdminId),
                    Some(admin_id),
                )
                .map_err(|e| DataQueryError::External(e.into()))?
                .get("total")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32;

            let work_count = coord
                .report_fetcher
                .fetch_work_reports_total(
                    WorkSourceType::All,
                    ReportStatus::All,
                    Some(WorkReportFilterType::AdminId),
                    Some(admin_id),
                )
                .map_err(|e| DataQueryError::External(e.into()))?
                .get("total")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32;

            let total = comment_count + work_count;

            total_comment_reports += comment_count;
            total_work_reports += work_count;

            stats.push(AdminReportStatsEntry {
                admin_id,
                admin_name: admin_name.to_string(),
                comment_reports: comment_count,
                work_reports: work_count,
                total_reports: total,
                percentage: 0.0,
            });
        }

        let grand_total = total_comment_reports + total_work_reports;
        for stat in &mut stats {
            stat.percentage = if grand_total > 0 {
                ((stat.total_reports as f64 / grand_total as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
        }

        stats.sort_by(|a, b| b.total_reports.cmp(&a.total_reports));

        Ok(AdminReportStatistics {
            total_admins: stats.len() as i32,
            total_comment_reports,
            total_work_reports,
            total_all_reports: grand_total,
            statistics: stats,
        })
    }

    /// 获取粉丝统计（基于点赞数阈值）
    pub fn compute_fans_by_like_threshold(
        &self,
        user_id: i32,
        like_threshold: i32,
    ) -> Result<FanByLikesStatistics, DataQueryError> {
        let coord = coordinator();
        let fans_stream = coord.user_fetcher.fetch_followers_gen(user_id, None);

        let mut qualified_fans = Vec::new();
        let mut total_fans = 0;

        for fan_result in fans_stream {
            let fan = fan_result.map_err(|e| DataQueryError::External(e.into()))?;
            total_fans += 1;

            let total_likes = fan.get("total_likes").and_then(|l| l.as_i64()).unwrap_or(0) as i32;

            if total_likes >= like_threshold {
                let mut fan_obj = JsonObject::new();
                if let Some(id) = fan.get("id").and_then(|i| i.as_i64()) {
                    fan_obj.insert("user_id".into(), Value::Number(id.into()));
                    let honors = coord
                        .user_fetcher
                        .fetch_user_honors(id as i32)
                        .map_err(|e| DataQueryError::External(e.into()));
                    if let Ok(ref honors_data) = honors {
                        if let Some(fans_total) = honors_data.get("fans_total") {
                            fan_obj.insert("fans_total".into(), fans_total.clone());
                        } else {
                            fan_obj.insert("fans_total".into(), Value::String("N/A".into()));
                        }
                        if let Some(collected_total) = honors_data.get("collected_total") {
                            fan_obj.insert("collected_total".into(), collected_total.clone());
                        } else {
                            fan_obj.insert("collected_total".into(), Value::String("N/A".into()));
                        }
                        if let Some(author_level) = honors_data.get("author_level") {
                            fan_obj.insert("author_level".into(), author_level.clone());
                        } else {
                            fan_obj.insert("author_level".into(), Value::String("N/A".into()));
                        }
                    } else {
                        fan_obj.insert("fans_total".into(), Value::String("N/A".into()));
                        fan_obj.insert("collected_total".into(), Value::String("N/A".into()));
                        fan_obj.insert("author_level".into(), Value::String("N/A".into()));
                    }
                    if let Some(nickname) = fan.get("nickname") {
                        fan_obj.insert("nickname".into(), nickname.clone());
                    }
                    fan_obj.insert("total_likes".into(), Value::Number(total_likes.into()));
                    fan_obj.insert(
                        "n_works".into(),
                        fan.get("n_works")
                            .cloned()
                            .unwrap_or(Value::Number(0.into())),
                    );
                    qualified_fans.push(fan_obj);
                }
            }
        }

        Ok(FanByLikesStatistics {
            target_user_id: user_id,
            like_threshold,
            total_fans,
            qualified_fans_count: qualified_fans.len() as i32,
            qualified_fans,
        })
    }

    /// 获取教育账号流（切换身份、重置密码）
    pub fn stream_edu_accounts_with_reset_passwords(
        &self,
        limit: Option<usize>,
    ) -> Box<dyn Iterator<Item = Result<(String, String), DataQueryError>> + 'static> {
        let coord = coordinator();

        if let Err(e) = coord.client.switch_identity(Identity::Edu) {
            return Box::new(std::iter::once(Err(DataQueryError::External(e.into()))));
        }

        let students = match coord
            .edu_fetcher
            .fetch_class_students_gen(1, limit)
            .collect()
            .map_err(|e| DataQueryError::External(e.into()))
        {
            Ok(students) => students,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };

        let mut rng = rand::rng();
        let mut shuffled_students = students;
        shuffled_students.sort_by(|_, _| rng.random::<u8>().cmp(&128));

        let stream = shuffled_students.into_iter().filter_map(move |student| {
            let student_id = student.get("id").and_then(|i| i.as_i64())? as i32;
            let username = student
                .get("username")
                .and_then(|u| u.as_str())?
                .to_string();
            let password_result = coord
                .edu_action
                .reset_student_password(student_id)
                .map_err(|e| DataQueryError::External(e.into()));
            match password_result {
                Ok(password_data) => {
                    let password = password_data
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(Ok((username, password)))
                }
                Err(e) => Some(Err(e)),
            }
        });

        Box::new(stream)
    }
}

impl Default for DataQuery {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 辅助数据结构 ====================

/// 管理员举报统计条目
#[derive(Debug, Clone)]
pub struct AdminReportStatsEntry {
    pub admin_id: i32,
    pub admin_name: String,
    pub comment_reports: i32,
    pub work_reports: i32,
    pub total_reports: i32,
    pub percentage: f64,
}

/// 管理员举报统计汇总
#[derive(Debug, Clone)]
pub struct AdminReportStatistics {
    pub total_admins: i32,
    pub total_comment_reports: i32,
    pub total_work_reports: i32,
    pub total_all_reports: i32,
    pub statistics: Vec<AdminReportStatsEntry>,
}

/// 粉丝点赞统计
#[derive(Debug, Clone)]
pub struct FanByLikesStatistics {
    pub target_user_id: i32,
    pub like_threshold: i32,
    pub total_fans: i32,
    pub qualified_fans_count: i32,
    pub qualified_fans: Vec<JsonObject>,
}

// ==================== 辅助迭代器实现 ====================

/// 社区新回复分页流
struct CommunityReplyStream {
    coord: &'static Coordinator,
    reply_type: ReplyTypes,
    remaining: i32,
    offset: i32,
    buffer: std::collections::VecDeque<JsonObject>,
}

impl Iterator for CommunityReplyStream {
    type Item = Result<JsonObject, DataQueryError>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(obj) = self.buffer.pop_front() {
            return Some(Ok(obj));
        }
        if self.remaining <= 0 {
            return None;
        }
        let batch_size = self.remaining.min(200).max(5);
        match self
            .coord
            .community_fetcher
            .fetch_replies(self.reply_type, batch_size, self.offset)
            .map_err(|e| DataQueryError::External(e.into()))
        {
            Ok(response) => {
                let items: Vec<JsonObject> = response
                    .get("items")
                    .and_then(|i| i.as_array())
                    .into_iter()
                    .flat_map(|arr| arr.iter().filter_map(|v| v.as_object().cloned()))
                    .collect();
                let take_count = items.len().min(self.remaining as usize);
                self.remaining -= take_count as i32;
                self.offset += batch_size;
                self.buffer.extend(items.into_iter().take(take_count));
                self.buffer.pop_front().map(Ok)
            }
            Err(e) => Some(Err(e)),
        }
    }
}

// ==================== 工具函数 ====================

/// 字符串数组去重
fn deduplicate(items: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for item in items {
        if seen.insert(item.clone()) {
            result.push(item.clone());
        }
    }
    result
}
