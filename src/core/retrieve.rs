use std::collections::HashMap;
use std::sync::OnceLock;

use rand::RngExt; // 导入随机扩展 trait
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
use crate::utils::acquire::{BaseKey, ClientFactory, CodeMaoClient, HttpMethod, Identity}; // 移除了未使用的 PaginatedIter
use crate::utils::data::{
    CacheManager, CodeMaoFile, DataManager, HistoryManager, PathConfig, SettingManager,
};

// ==================== Coordinator 单例 ====================// 新增 Debug 派生
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
    COORDINATOR.get().expect("Coordinator not initialized")
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

    /// 根据 source 获取评论数据的惰性迭代器
    fn comments_iter(
        &self,
    ) -> Result<Box<dyn Iterator<Item = Result<JsonValue, ObtainError>>>, ObtainError> {
        let source = self
            .source
            .ok_or_else(|| ObtainError::InvalidSource("未设置来源".into()))?;
        let source_id = self
            .source_id
            .ok_or_else(|| ObtainError::InvalidSource("未设置源ID".into()))?;
        let limit = self.limit;
        let coord = coordinator();

        match source {
            QuerySource::Work => {
                let iter = coord
                    .work_obtain
                    .fetch_work_comments_gen(source_id, limit)
                    .map(|item| item.map_err(|e| ObtainError::External(e.into())));
                Ok(Box::new(iter))
            }
            QuerySource::Forum => {
                let iter = coord
                    .forum_obtain
                    .fetch_post_replies_gen(source_id, None, limit)
                    .map(|item| item.map_err(|e| ObtainError::External(e.into())));
                Ok(Box::new(iter))
            }
            QuerySource::Shop => {
                let iter = coord
                    .shop_obtain
                    .fetch_workshop_discussions_gen(source_id, None, None, limit)
                    .map(|item| item.map_err(|e| ObtainError::External(e.into())));
                Ok(Box::new(iter))
            }
        }
    }

    pub fn execute(self) -> Result<CommentsResult, ObtainError> {
        let source = self
            .source
            .ok_or_else(|| ObtainError::InvalidSource("未设置来源".into()))?;
        let method = self.method;

        let comments_iter = self.comments_iter()?;
        // 惰性收集成 Vec 以便聚合
        let comments: Vec<JsonValue> = comments_iter.collect::<Result<Vec<_>, _>>()?;

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

    pub fn query(&self) -> QueryBuilder {
        QueryBuilder::new()
    }

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

    // ==================== 社区新回复（迭代器版本） ====================
    pub fn get_new_replies_iter(
        &self,
        type_item: ReplyTypes,
        limit: i32,
    ) -> Box<dyn Iterator<Item = Result<JsonObject, ObtainError>> + 'static> {
        let coord = coordinator();
        let total = match coord
            .community_obtain
            .fetch_message_count(MessageMethod::Web)
            .map_err(|e| ObtainError::External(e.into()))
        {
            Ok(data) => data.get("count").and_then(|c| c.as_i64()).unwrap_or(0) as i32,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };

        let remaining = if limit == 0 { total } else { limit.min(total) };

        Box::new(NewRepliesIter {
            coord,
            type_item,
            remaining,
            offset: 0,
            buffer: std::collections::VecDeque::new(),
        })
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

    // ==================== 作品数据集成（返回惰性迭代器） ====================
    pub fn integrate_work_data_iter(
        &self,
        limit: i32,
    ) -> Box<dyn Iterator<Item = Result<JsonObject, ObtainError>> + 'static> {
        let coord = coordinator();
        let per_source_limit = Some(limit / 2);

        let nemo_mapping: HashMap<&str, &str> = [
            ("work_id", "work_id"),
            ("work_name", "work_name"),
            ("user_name", "user_name"),
            ("user_id", "user_id"),
            ("like_count", "like_count"),
            ("updated_at", "updated_at"),
        ]
        .into_iter()
        .collect();

        let web_mapping: HashMap<&str, &str> = [
            ("work_id", "work_id"),
            ("work_name", "work_name"),
            ("user_name", "nickname"),
            ("user_id", "user_id"),
            ("like_count", "likes_count"),
            ("updated_at", "updated_at"),
        ]
        .into_iter()
        .collect();

        // 获取原始数据（可能为错误）
        let nemo_result =
            coord
                .work_obtain
                .fetch_new_works_nemo(NemoWorkType::Original, per_source_limit, None);
        let web_result = coord
            .work_obtain
            .fetch_new_works_web(per_source_limit, None, true);

        // 提取 items 数组并映射，错误直接转为单元素错误迭代器
        let process_result = |res: Result<Value, _>, mapping: HashMap<&str, &str>| match res {
            Ok(val) => {
                let items = val
                    .get("items")
                    .and_then(|i| i.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mapped: Vec<Result<JsonObject, ObtainError>> = items
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
                    as Box<dyn Iterator<Item = Result<JsonObject, ObtainError>>>
            }
            Err(e) => Box::new(std::iter::once::<Result<JsonObject, ObtainError>>(Err(
                ObtainError::External(e),
            ))),
        };

        let nemo_iter = process_result(nemo_result, nemo_mapping);
        let web_iter = process_result(web_result, web_mapping);

        Box::new(nemo_iter.chain(web_iter))
    }

    // ==================== 收集作品评论（返回聚合后的 Vec） ====================
    pub fn collect_work_comments(&self, limit: i32) -> Result<Vec<JsonObject>, ObtainError> {
        let works: Vec<JsonObject> = self
            .integrate_work_data_iter(limit)
            .collect::<Result<Vec<_>, _>>()?;

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
                entry.2.push(cont);
                entry.3 += 1;
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
                    WorkSourceType::All,
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

        for fan_result in fans_iter {
            let fan = fan_result.map_err(|e| ObtainError::External(e.into()))?;
            total_fans += 1;

            let total_likes = fan.get("total_likes").and_then(|l| l.as_i64()).unwrap_or(0) as i32;

            if total_likes >= like_num {
                let mut obj = JsonObject::new();
                if let Some(id) = fan.get("id").and_then(|i| i.as_i64()) {
                    obj.insert("user_id".into(), Value::Number(id.into()));
                    let honors = coord
                        .user_obtain
                        .fetch_user_honors(id as i32)
                        .map_err(|e| ObtainError::External(e.into()));
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
                    if let Some(nick) = fan.get("nickname") {
                        obj.insert("nickname".into(), nick.clone());
                    }
                    obj.insert("total_likes".into(), Value::Number(total_likes.into()));
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

    // ==================== 教育账号迭代器 ====================
    pub fn switch_edu_account_iter(
        &self,
        limit: Option<usize>,
    ) -> Box<dyn Iterator<Item = Result<(String, String), ObtainError>> + 'static> {
        let coord = coordinator();

        // 切换身份（仅一次）
        if let Err(e) = coord.client.switch_identity(Identity::Edu) {
            return Box::new(std::iter::once(Err(ObtainError::External(e.into()))));
        }

        let students = match coord
            .edu_obtain
            .fetch_class_students_gen(1, limit)
            .collect()
            .map_err(|e| ObtainError::External(e.into()))
        {
            Ok(s) => s,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };

        let mut rng = rand::rng();
        let mut students = students;
        // 随机打乱顺序
        students.sort_by(|_, _| rng.random::<u8>().cmp(&128));

        let iter = students.into_iter().filter_map(move |student| {
            let id = student.get("id").and_then(|i| i.as_i64())? as i32;
            let username = student
                .get("username")
                .and_then(|u| u.as_str())?
                .to_string();
            let pwd_response = coord
                .edu_motion
                .reset_student_password(id)
                .map_err(|e| ObtainError::External(e.into()));
            match pwd_response {
                Ok(pwd) => {
                    let password = pwd
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(Ok((username, password)))
                }
                Err(e) => Some(Err(e)),
            }
        });

        Box::new(iter)
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

// ==================== 辅助迭代器实现 ====================
/// 社区新回复分页迭代器
struct NewRepliesIter {
    coord: &'static Coordinator,
    type_item: ReplyTypes,
    remaining: i32,
    offset: i32,
    buffer: std::collections::VecDeque<JsonObject>,
}

impl Iterator for NewRepliesIter {
    type Item = Result<JsonObject, ObtainError>;

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
            .community_obtain
            .fetch_replies(self.type_item, batch_size, self.offset)
            .map_err(|e| ObtainError::External(e.into()))
        {
            Ok(response) => {
                let items: Vec<JsonObject> = response
                    .get("items")
                    .and_then(|i| i.as_array())
                    .into_iter()
                    .flat_map(|arr| arr.iter().filter_map(|v| v.as_object().cloned()))
                    .collect();
                let take_cnt = items.len().min(self.remaining as usize);
                self.remaining -= take_cnt as i32;
                self.offset += batch_size;
                self.buffer.extend(items.into_iter().take(take_cnt));
                self.buffer.pop_front().map(Ok)
            }
            Err(e) => Some(Err(e)),
        }
    }
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
