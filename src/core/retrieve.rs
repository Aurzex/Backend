use std::collections::HashMap;
use std::sync::OnceLock;

use rand::RngExt;
use serde_json::{Map, Value};
use thiserror::Error;

// ==================== 导入真实的 API 模块 ====================
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
// ==================== 导入工具模块 ====================
use crate::utils::acquire::{
    BaseKey, ClientFactory, CodeMaoClient, HttpMethod, Identity, PaginatedIter,
};
use crate::utils::data::{
    CacheManager, CodeMaoFile, DataManager, HistoryManager, PathConfig, SettingManager,
};

// ==================== Coordinator 单例 ====================
pub struct Coordinator {
    pub auth: AuthManager,
    pub community_motion: CommunityUserAction,
    pub community_obtain: CommunityDataFetcher,
    pub edu_motion: EduUserAction,
    pub edu_obtain: EduDataFetcher,
    pub forum_motion: ForumActionHandler,
    pub forum_obtain: ForumDataFetcher,
    pub novel_motion: NovelActionHandler,
    pub novel_obtain: NovelDataFetcher,
    pub shop_motion: WorkshopActionHandler,
    pub shop_obtain: WorkshopDataFetcher,
    pub user_motion: UserManager,
    pub user_obtain: UserDataFetcher,
    pub whale_motion: ReportHandler,
    pub whale_obtain: ReportFetcher,
    pub work_motion: KittenWorkManager,
    pub work_obtain: WorkDataFetcher,
    pub client: &'static CodeMaoClient,
    pub data_manager: &'static DataManager,
    pub path_config: PathConfig,
    pub setting_manager: &'static SettingManager,
    pub cache_manager: &'static CacheManager,
    pub history_manager: &'static HistoryManager,
    pub file_manager: CodeMaoFile,
}

static COORDINATOR: OnceLock<Coordinator> = OnceLock::new();

pub fn init_coordinator() {
    let coordinator = Coordinator {
        auth: AuthManager::new(),
        community_motion: CommunityUserAction::new(),
        community_obtain: CommunityDataFetcher::new(),
        client: ClientFactory::global_client(),
        edu_motion: EduUserAction::new(),
        edu_obtain: EduDataFetcher::new(),
        forum_motion: ForumActionHandler::new(),
        forum_obtain: ForumDataFetcher::new(),
        novel_motion: NovelActionHandler::new(),
        novel_obtain: NovelDataFetcher::new(),
        shop_motion: WorkshopActionHandler::new(),
        shop_obtain: WorkshopDataFetcher::new(),
        user_motion: UserManager::new(),
        user_obtain: UserDataFetcher::new(),
        whale_motion: ReportHandler::new(),
        whale_obtain: ReportFetcher::new(),
        work_motion: KittenWorkManager::new(),
        work_obtain: WorkDataFetcher::new(),
        data_manager: DataManager::global(),
        path_config: PathConfig,
        setting_manager: SettingManager::global(),
        cache_manager: CacheManager::global(),
        history_manager: HistoryManager::global(),
        file_manager: CodeMaoFile,
    };

    COORDINATOR.set(coordinator);
}

pub fn coordinator() -> &'static Coordinator {
    COORDINATOR.get().unwrap()
}

// ==================== 错误类型 ====================

#[derive(Error, Debug)]
pub enum ObtainError {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuerySource {
    Work,
    Forum,
    Shop,
}

impl QuerySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            QuerySource::Work => "work",
            QuerySource::Forum => "forum",
            QuerySource::Shop => "shop",
        }
    }
}

impl std::str::FromStr for QuerySource {
    type Err = ObtainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "work" => Ok(QuerySource::Work),
            "forum" => Ok(QuerySource::Forum),
            "shop" => Ok(QuerySource::Shop),
            _ => Err(ObtainError::InvalidSource(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryMethod {
    UserId,
    CommentId,
    Comments,
}

impl QueryMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueryMethod::UserId => "user_id",
            QueryMethod::CommentId => "comment_id",
            QueryMethod::Comments => "comments",
        }
    }
}

impl std::str::FromStr for QueryMethod {
    type Err = ObtainError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_id" => Ok(QueryMethod::UserId),
            "comment_id" => Ok(QueryMethod::CommentId),
            "comments" => Ok(QueryMethod::Comments),
            _ => Err(ObtainError::InvalidMethod(s.to_string())),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TypeItem {
    LikeFork,
    CommentReply,
    System,
}

impl TypeItem {
    pub fn as_str(&self) -> &'static str {
        match self {
            TypeItem::LikeFork => "LIKE_FORK",
            TypeItem::CommentReply => "COMMENT_REPLY",
            TypeItem::System => "SYSTEM",
        }
    }
}

// ==================== 数据类型别名 ====================

pub type JsonValue = Value;
pub type JsonObject = Map<String, Value>;

// ==================== 结果类型 ====================

#[derive(Debug, Clone)]
pub enum CommentsResult {
    UserIdList(Vec<String>),
    CommentIdList(Vec<String>),
    DetailedComments(Vec<JsonObject>),
}

// ==================== 查询构建器 ====================

pub struct QueryBuilder {
    source: Option<QuerySource>,
    source_id: Option<i32>,
    method: QueryMethod,
    limit: Option<usize>,
}

impl QueryBuilder {
    fn new() -> Self {
        Self {
            source: None,
            source_id: None,
            method: QueryMethod::UserId,
            limit: Some(500),
        }
    }

    pub fn source(mut self, source: QuerySource) -> Self {
        self.source = Some(source);
        self
    }

    pub fn source_id(mut self, id: i32) -> Self {
        self.source_id = Some(id);
        self
    }

    pub fn method(mut self, method: QueryMethod) -> Self {
        self.method = method;
        self
    }

    pub fn limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }

    /// 从 PaginatedIter 中惰性收集所有数据
    fn collect_paginated(paginated: PaginatedIter) -> Result<Vec<Value>, ObtainError> {
        let mut items = Vec::new();
        for item in paginated {
            match item {
                Ok(value) => items.push(value),
                Err(e) => return Err(ObtainError::External(e.into())),
            }
        }
        Ok(items)
    }

    pub fn execute(self) -> Result<CommentsResult, ObtainError> {
        let source = self
            .source
            .ok_or(ObtainError::InvalidSource("未设置来源".into()))?;
        let source_id = self
            .source_id
            .ok_or(ObtainError::InvalidSource("未设置源ID".into()))?;
        let method = self.method;
        let limit = self.limit;
        let coord = coordinator();

        // 获取分页迭代器
        let paginated_iter = match source {
            QuerySource::Work => coord.work_obtain.fetch_work_comments_gen(source_id, limit),
            QuerySource::Forum => coord
                .forum_obtain
                .fetch_post_replies_gen(source_id, None, limit),
            QuerySource::Shop => coord
                .shop_obtain
                .fetch_workshop_discussions_gen(source_id, None, None, limit),
        };

        // 惰性收集所有评论数据
        let comments = Self::collect_paginated(paginated_iter)?;

        let user_field = match source {
            QuerySource::Work | QuerySource::Shop => "reply_user",
            QuerySource::Forum => "user",
        };

        let mut reply_cache: HashMap<i64, Vec<JsonObject>> = HashMap::new();

        let extract_reply_user_id = |reply: &JsonObject| -> Option<i64> {
            reply
                .get(user_field)
                .and_then(|u| u.as_object())
                .and_then(|u| u.get("id"))
                .and_then(|id| id.as_i64())
        };

        let mut get_replies = |comment: &Value| -> Result<Vec<JsonObject>, ObtainError> {
            let comment_obj = comment
                .as_object()
                .ok_or_else(|| ObtainError::ParseError("评论数据格式错误".into()))?;

            if source == QuerySource::Forum {
                let comment_id = comment_obj
                    .get("id")
                    .and_then(|id| id.as_i64())
                    .ok_or(ObtainError::ParseError("缺少评论ID".into()))?;

                if !reply_cache.contains_key(&comment_id) {
                    let replies_iter = coordinator()
                        .forum_obtain
                        .fetch_reply_comments_gen(comment_id as i32, None);

                    let replies: Vec<JsonObject> = replies_iter
                        .filter_map(|item| match item {
                            Ok(value) => value.as_object().cloned(),
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

        match method {
            QueryMethod::UserId => {
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
                    for reply in get_replies(comment)? {
                        if let Some(uid) = extract_reply_user_id(&reply) {
                            user_ids.push(uid.to_string());
                        }
                    }
                }
                Ok(CommentsResult::UserIdList(deduplicate(&user_ids)))
            }
            QueryMethod::CommentId => {
                let mut comment_ids = Vec::new();
                for comment in &comments {
                    if let Some(id) = comment.get("id").and_then(|id| id.as_i64()) {
                        comment_ids.push(id.to_string());
                    }
                    let comment_id = comment.get("id").and_then(|id| id.as_i64());
                    for reply in get_replies(comment)? {
                        if let (Some(cid), Some(rid)) =
                            (comment_id, reply.get("id").and_then(|id| id.as_i64()))
                        {
                            comment_ids.push(format!("{}.{}", cid, rid));
                        }
                    }
                }
                Ok(CommentsResult::CommentIdList(deduplicate(&comment_ids)))
            }
            QueryMethod::Comments => {
                let mut detailed = Vec::new();
                for comment in &comments {
                    let replies: Vec<JsonObject> = get_replies(comment)?
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

// ==================== Obtain 主结构体 ====================

pub struct Obtain;

impl Obtain {
    pub fn new() -> Self {
        Obtain
    }

    /// 创建链式查询构建器
    pub fn query(&self) -> QueryBuilder {
        QueryBuilder::new()
    }

    /// 便捷方法：直接获取评论数据
    pub fn get_comments(
        &self,
        source: QuerySource,
        source_id: i32,
        method: QueryMethod,
        limit: Option<usize>,
    ) -> Result<CommentsResult, ObtainError> {
        self.query()
            .source(source)
            .source_id(source_id)
            .method(method)
            .limit(limit)
            .execute()
    }

    // ==================== 社区新回复 ====================

    pub fn get_new_replies(
        &self,
        limit: i32,
        type_item: ReplyTypes,
    ) -> Result<Vec<JsonObject>, ObtainError> {
        let coord = coordinator();
        let message_data = coord
            .community_obtain
            .fetch_message_count(MessageMethod::Web)
            .map_err(|e| ObtainError::External(e.into()))?;

        let total_replies = message_data
            .get("count")
            .and_then(|c| c.as_i64())
            .unwrap_or(0) as i32;

        if total_replies == 0 && limit == 0 {
            return Ok(vec![]);
        }

        let effective_limit = if limit == 0 {
            total_replies
        } else {
            limit.min(total_replies)
        };

        let mut offset = 0;
        let mut remaining = effective_limit;
        let mut replies = Vec::new();

        while remaining > 0 {
            let batch_size = remaining.min(200).max(5);
            let response = coord
                .community_obtain
                .fetch_replies(type_item, batch_size, offset)
                .map_err(|e| ObtainError::External(e.into()))?;

            let items = response
                .get("items")
                .and_then(|i| i.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_object().cloned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let take = items.len().min(remaining as usize);
            replies.extend(items.into_iter().take(take));
            remaining -= take as i32;
            offset += batch_size;

            if (take as i32) < batch_size {
                break;
            }
        }

        Ok(replies)
    }

    // ==================== 评论总数 ====================
    pub fn get_comment_total(
        &self,
        source_type: QuerySource,
        source_id: i32,
    ) -> Result<i32, ObtainError> {
        let coord = coordinator();
        match source_type {
            QuerySource::Work => {
                // 先尝试从评论接口获取总数
                let comments_response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "15")
                    .send()
                    .map_err(|e| ObtainError::External(e.into()))?;

                let json = coord
                    .client
                    .response_to_json(comments_response)
                    .map_err(|e| ObtainError::External(e.into()))?;

                if let Some(total) = json.get("total").and_then(|t| t.as_i64()) {
                    return Ok(total as i32);
                }

                // 如果评论接口没有 total，尝试从作品详情获取
                let work_response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}", source_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .map_err(|e| ObtainError::External(e.into()))?;

                let work_json = coord
                    .client
                    .response_to_json(work_response)
                    .map_err(|e| ObtainError::External(e.into()))?;

                Ok(work_json
                    .get("comment_times")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32)
            }
            QuerySource::Shop => {
                let response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/discussions/{}/comments", source_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("source", "WORK_SHOP")
                    .with_param("sort", "-created_at")
                    .with_param("limit", "15")
                    .with_param("offset", "0")
                    .send()
                    .map_err(|e| ObtainError::External(e.into()))?;

                let json = coord
                    .client
                    .response_to_json(response)
                    .map_err(|e| ObtainError::External(e.into()))?;

                let total = json.get("total").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                let total_reply =
                    json.get("totalReply").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                Ok(total + total_reply)
            }
            QuerySource::Forum => {
                let response = coord
                    .client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/forums/posts/{}/details", source_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .map_err(|e| ObtainError::External(e.into()))?;

                let json = coord
                    .client
                    .response_to_json(response)
                    .map_err(|e| ObtainError::External(e.into()))?;

                let n_replies = json.get("n_replies").and_then(|r| r.as_i64()).unwrap_or(0) as i32;
                let n_comments =
                    json.get("n_comments").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
                Ok(n_replies + n_comments)
            }
        }
    }

    // ==================== 作品数据集成 ====================

    pub fn integrate_work_data(&self, limit: i32) -> Result<Vec<JsonObject>, ObtainError> {
        let coord = coordinator();
        let per_source_limit = Some(limit / 2); // 包装为 Option<i32>

        let nemo_data = coord
            .work_obtain
            .fetch_new_works_nemo(
                NemoWorkType::Original, // 使用枚举而不是字符串
                per_source_limit,
                None, // offset 参数
            )
            .map_err(|e| ObtainError::External(e.into()))?;

        let web_data = coord
            .work_obtain
            .fetch_new_works_web(
                per_source_limit,
                None, // offset 参数
                true, // origin 参数，过滤原创作品
            )
            .map_err(|e| ObtainError::External(e.into()))?;

        let sources = vec![(nemo_data, "nemo"), (web_data, "web")];

        let field_mapping: HashMap<&str, HashMap<&str, &str>> = [
            (
                "nemo",
                [
                    ("work_id", "work_id"),
                    ("work_name", "work_name"),
                    ("user_name", "user_name"),
                    ("user_id", "user_id"),
                    ("like_count", "like_count"),
                    ("updated_at", "updated_at"),
                ]
                .iter()
                .cloned()
                .collect(),
            ),
            (
                "web",
                [
                    ("work_id", "work_id"),
                    ("work_name", "work_name"),
                    ("user_name", "nickname"),
                    ("user_id", "user_id"),
                    ("like_count", "likes_count"),
                    ("updated_at", "updated_at"),
                ]
                .iter()
                .cloned()
                .collect(),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let mut results = Vec::new();
        for (source_data, source) in sources {
            if let Some(items) = source_data.get("items").and_then(|i| i.as_array()) {
                let mapping = &field_mapping[source];
                for item in items {
                    if let Some(obj) = item.as_object() {
                        let mut mapped = JsonObject::new();
                        for (target, source_field) in mapping {
                            if let Some(value) = obj.get(*source_field) {
                                mapped.insert(target.to_string(), value.clone());
                            }
                        }
                        results.push(mapped);
                    }
                }
            }
        }

        Ok(results)
    }

    /// 收集作品评论（返回用户评论统计）
    pub fn collect_work_comments(&self, limit: i32) -> Result<Vec<JsonObject>, ObtainError> {
        let works = self.integrate_work_data(limit)?;
        let mut all_comments = Vec::new();

        for work in &works {
            if let Some(work_id) = work.get("work_id").and_then(|id| id.as_i64()) {
                if let Ok(CommentsResult::DetailedComments(comments)) = self.get_comments(
                    QuerySource::Work,
                    work_id as i32,
                    QueryMethod::Comments,
                    Some(20),
                ) {
                    all_comments.extend(comments);
                }
            }
        }

        // 聚合用户评论
        // 元组：(user_id, nickname, comments_vec, count)
        let mut user_map: HashMap<String, (String, String, Vec<String>, i32)> = HashMap::new();

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
                let entry = user_map
                    .entry(uid.clone())
                    .or_insert_with(|| (uid, nick, Vec::new(), 0));
                entry.2.push(cont); // entry.2 = Vec<String>
                entry.3 += 1; // entry.3 = i32
            }
        }

        let mut result: Vec<JsonObject> = user_map
            .into_values()
            .map(|(uid, nick, coms, count)| {
                let mut obj = JsonObject::new();
                obj.insert("user_id".into(), Value::String(uid));
                obj.insert("nickname".into(), Value::String(nick));
                obj.insert(
                    "comments".into(),
                    Value::Array(coms.into_iter().map(Value::String).collect()),
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

    // ==================== 管理员统计 ====================
    pub fn get_admin_statistics(&self) -> Result<AdminStatistics, ObtainError> {
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
        let mut total_comment = 0;
        let mut total_work = 0;

        for &(id, name) in &admins {
            let comment_count = coord
                .whale_obtain
                .fetch_comment_reports_total(
                    CommentSourceType::All,
                    ReportStatus::All,
                    Some(CommentReportFilterType::AdminId),
                    Some(id),
                )
                .map_err(|e| ObtainError::External(e.into()))?
                .get("total")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32;

            let work_count = coord
                .whale_obtain
                .fetch_work_reports_total(
                    WorkSourceType::All, // 如果 work 的函数签名类似，需要对应修改
                    ReportStatus::All,
                    Some(WorkReportFilterType::AdminId),
                    Some(id),
                )
                .map_err(|e| ObtainError::External(e.into()))?
                .get("total")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32;

            let total = comment_count + work_count;

            total_comment += comment_count;
            total_work += work_count;

            stats.push(AdminStatEntry {
                admin_id: id,
                admin_name: name.to_string(),
                comment_reports: comment_count,
                work_reports: work_count,
                total_reports: total,
                percentage: 0.0,
            });
        }

        let grand_total = total_comment + total_work;
        for stat in &mut stats {
            stat.percentage = if grand_total > 0 {
                ((stat.total_reports as f64 / grand_total as f64) * 1000.0).round() / 10.0
            } else {
                0.0
            };
        }

        stats.sort_by(|a, b| b.total_reports.cmp(&a.total_reports));

        Ok(AdminStatistics {
            total_admins: stats.len() as i32,
            total_comment_reports: total_comment,
            total_work_reports: total_work,
            total_all_reports: grand_total,
            statistics: stats,
        })
    }

    // ==================== 粉丝统计 ====================
    pub fn get_fans_statistics(
        &self,
        user_id: i32,
        like_num: i32,
    ) -> Result<FanStatistics, ObtainError> {
        let coord = coordinator();
        let fans_iter = coord.user_obtain.fetch_followers_gen(user_id, None);

        let mut qualified = Vec::new();
        let mut total_fans = 0;

        // 直接惰性迭代，不预先收集
        for fan_result in fans_iter {
            let fan = fan_result.map_err(|e| ObtainError::External(e.into()))?;
            total_fans += 1;

            let total_likes = fan.get("total_likes").and_then(|l| l.as_i64()).unwrap_or(0) as i32;

            if total_likes >= like_num {
                println!("\n 符合条件的粉丝:");
                if let Some(nick) = fan.get("nickname").and_then(|n| n.as_str()) {
                    println!("昵称: {}", nick);
                }
                if let Some(id) = fan.get("id").and_then(|i| i.as_i64()) {
                    println!("ID: {}", id);

                    let honors = coord
                        .user_obtain
                        .fetch_user_honors(id as i32)
                        .map_err(|e| ObtainError::External(e.into()));

                    if let Ok(ref h) = honors {
                        if let Some(fans_count) = h.get("fans_total") {
                            println!("粉丝数: {}", fans_count);
                        }
                        if let Some(collected) = h.get("collected_total") {
                            println!("作品收藏数: {}", collected);
                        }
                        if let Some(level) = h.get("author_level") {
                            println!("作者等级: {}", level);
                        }
                    }

                    let mut obj = JsonObject::new();
                    obj.insert("user_id".into(), Value::Number(id.into()));
                    if let Some(nick) = fan.get("nickname") {
                        obj.insert("nickname".into(), nick.clone());
                    }
                    obj.insert("total_likes".into(), Value::Number(total_likes.into()));

                    if let Ok(ref h) = honors {
                        if let Some(f) = h.get("fans_total") {
                            obj.insert("fans_total".into(), f.clone());
                        } else {
                            obj.insert("fans_total".into(), Value::String("N/A".into()));
                        }
                        if let Some(c) = h.get("collected_total") {
                            obj.insert("collected_total".into(), c.clone());
                        } else {
                            obj.insert("collected_total".into(), Value::String("N/A".into()));
                        }
                        if let Some(l) = h.get("author_level") {
                            obj.insert("author_level".into(), l.clone());
                        } else {
                            obj.insert("author_level".into(), Value::String("N/A".into()));
                        }
                    } else {
                        obj.insert("fans_total".into(), Value::String("N/A".into()));
                        obj.insert("collected_total".into(), Value::String("N/A".into()));
                        obj.insert("author_level".into(), Value::String("N/A".into()));
                    }

                    obj.insert(
                        "n_works".into(),
                        fan.get("n_works")
                            .cloned()
                            .unwrap_or(Value::Number(0.into())),
                    );
                    qualified.push(obj);
                }
            }
        }

        Ok(FanStatistics {
            target_user_id: user_id,
            like_threshold: like_num,
            total_fans,
            qualified_fans_count: qualified.len() as i32,
            qualified_fans: qualified,
        })
    }

    // ==================== 教育账号 ====================

    pub fn switch_edu_account_to_list(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<(String, String)>, ObtainError> {
        let coord = coordinator();
        let mut students = coord
            .edu_obtain
            .fetch_class_students_gen(1, limit)
            .map_err(|e| ObtainError::External(e.into()))?;

        if students.is_empty() {
            println!("没有可用的教育账号");
            return Ok(vec![]);
        }

        // 切换身份
        coord
            .client
            .switch_identity(Identity::Edu)
            .map_err(|e| ObtainError::External(e.into()))?;

        let mut rng = rand::rng();
        let mut result = Vec::new();

        while !students.is_empty() {
            let idx = rng.random_range(0..students.len());
            let student = students.remove(idx);

            let id = student
                .get("id")
                .and_then(|i| i.as_i64())
                .ok_or(ObtainError::ParseError("学生ID缺失".into()))? as i32;
            let username = student
                .get("username")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();

            let pwd_response = coord
                .edu_motion
                .reset_student_password(id)
                .map_err(|e| ObtainError::External(e.into()))?;
            let password = pwd_response
                .get("password")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();

            result.push((username, password));
        }

        Ok(result)
    }

    pub fn switch_edu_account_to_iter(
        &self,
        limit: Option<usize>,
    ) -> Result<Box<dyn Iterator<Item = (String, String)> + '_>, ObtainError> {
        let list = self.switch_edu_account_to_list(limit)?;
        Ok(Box::new(list.into_iter()))
    }
}

impl Default for Obtain {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 辅助数据结构 ====================

#[derive(Debug, Clone)]
pub struct AdminStatEntry {
    pub admin_id: i32,
    pub admin_name: String,
    pub comment_reports: i32,
    pub work_reports: i32,
    pub total_reports: i32,
    pub percentage: f64,
}

#[derive(Debug, Clone)]
pub struct AdminStatistics {
    pub total_admins: i32,
    pub total_comment_reports: i32,
    pub total_work_reports: i32,
    pub total_all_reports: i32,
    pub statistics: Vec<AdminStatEntry>,
}

#[derive(Debug, Clone)]
pub struct FanStatistics {
    pub target_user_id: i32,
    pub like_threshold: i32,
    pub total_fans: i32,
    pub qualified_fans_count: i32,
    pub qualified_fans: Vec<JsonObject>,
}

// ==================== 工具函数 ====================

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
