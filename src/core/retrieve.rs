use std::collections::{HashMap, VecDeque};

use serde_json::{Map, Value};
use thiserror::Error;

use crate::api::community::{CommunityDataFetcher, MessageMethod, ReplyTypes};
use crate::api::education::{EduDataFetcher, EduUserAction};
use crate::api::forum::ForumDataFetcher;
use crate::api::shop::WorkshopDataFetcher;
use crate::api::user::UserDataFetcher;
use crate::api::whale::{
    CommentReportFilterType, CommentSourceType, ReportStatus, WhaleReportFetcher,
    WorkReportFilterType, WorkSourceType,
};
use crate::api::work::{NemoWorkType, WorkDataFetcher};
use crate::utils::acquire::{BaseKey, Catsona, CodeMaoClient, HttpMethod, MewError};

// ==================== 错误类型 ====================

#[derive(Error, Debug)]
pub enum DataQueryError {
    #[error("无效的来源类型: {0}")]
    InvalidSource(String),
    #[error("无效的查询方法: {0}")]
    InvalidMethod(String),
    #[error("数据解析失败: {0}")]
    ParseError(String),
    #[error("未找到请求的资源")]
    NotFound,
    #[error("内部错误: {0}")]
    Internal(String),
    #[error("外部错误: {0}")]
    External(MewError),
}

/// 使得可以直接将 `MewError` 转换成 `DataQueryError`，内部直接使用 `?` 传播
impl From<MewError> for DataQueryError {
    fn from(e: MewError) -> Self {
        DataQueryError::External(e)
    }
}

/// 字符串解析错误也可转换为 `DataQueryError`
impl From<serde_json::Error> for DataQueryError {
    fn from(e: serde_json::Error) -> Self {
        DataQueryError::ParseError(e.to_string())
    }
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

    /// 根据来源获取评论数据的惰性迭代器（已统一为 MewError）
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

        match source {
            CommentSource::Work => {
                let iter = WorkDataFetcher::new()
                    .fetch_work_comments_gen(target_id, limit)
                    .map(|item| item.map_err(DataQueryError::from));
                Ok(Box::new(iter))
            }
            CommentSource::Forum => {
                let iter = ForumDataFetcher::new()
                    .fetch_post_replies_gen(target_id, None, limit)
                    .map(|item| item.map_err(DataQueryError::from));
                Ok(Box::new(iter))
            }
            CommentSource::Shop => {
                let iter = WorkshopDataFetcher::new()
                    .fetch_workshop_discussions_gen(target_id, None, None, limit)
                    .map(|item| item.map_err(DataQueryError::from));
                Ok(Box::new(iter))
            }
        }
    }

    /// 执行评论查询
    pub fn execute(mut self) -> Result<CommentsResult, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let mode = self.mode;

        // 安全上限：防止一次加载过多评论
        const MAX_COMMENTS: usize = 1000;
        let safe_limit = self.limit.unwrap_or(500).min(MAX_COMMENTS);
        self.limit = Some(safe_limit);

        let comment_stream = self.build_comment_stream()?;
        let comments: Vec<JsonValue> = comment_stream
            .take(safe_limit)
            .collect::<Result<Vec<_>, _>>()?;

        let user_field = match source {
            CommentSource::Work | CommentSource::Shop => "reply_user",
            CommentSource::Forum => "user",
        };

        let extract_reply_user_id = |reply: &JsonObject| -> Option<i64> {
            reply
                .get(user_field)
                .and_then(|u| u.as_object())
                .and_then(|u| u.get("id"))
                .and_then(|id| id.as_i64())
        };

        match mode {
            CommentQueryMode::UserId => {
                let mut user_ids = Vec::new();
                for comment in &comments {
                    // 主评论用户
                    if let Some(id) = comment
                        .get("user")
                        .and_then(|u| u.as_object())
                        .and_then(|u| u.get("id"))
                        .and_then(|id| id.as_i64())
                    {
                        user_ids.push(id.to_string());
                    }

                    let comment_id = comment.get("id").and_then(|id| id.as_i64()).unwrap_or(0);

                    // 回复中的用户
                    if source == CommentSource::Forum {
                        let reply_stream = ForumDataFetcher::new()
                            .fetch_reply_comments_gen(comment_id as i32, None);
                        for reply_result in reply_stream {
                            if let Ok(reply_val) = reply_result {
                                if let Some(reply_obj) = reply_val.as_object() {
                                    if let Some(uid) = extract_reply_user_id(reply_obj) {
                                        user_ids.push(uid.to_string());
                                    }
                                }
                            }
                        }
                    } else {
                        if let Some(replies) = comment
                            .get("replies")
                            .and_then(|r| r.as_object())
                            .and_then(|r| r.get("items"))
                            .and_then(|items| items.as_array())
                        {
                            for reply_val in replies {
                                if let Some(reply_obj) = reply_val.as_object() {
                                    if let Some(uid) = extract_reply_user_id(reply_obj) {
                                        user_ids.push(uid.to_string());
                                    }
                                }
                            }
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
                    let comment_id = comment.get("id").and_then(|id| id.as_i64()).unwrap_or(0);

                    if source == CommentSource::Forum {
                        let reply_stream = ForumDataFetcher::new()
                            .fetch_reply_comments_gen(comment_id as i32, None);
                        for reply_result in reply_stream {
                            if let Ok(reply_val) = reply_result {
                                if let Some(reply_obj) = reply_val.as_object() {
                                    if let Some(rid) =
                                        reply_obj.get("id").and_then(|id| id.as_i64())
                                    {
                                        comment_ids.push(format!("{}.{}", comment_id, rid));
                                    }
                                }
                            }
                        }
                    } else {
                        if let Some(replies) = comment
                            .get("replies")
                            .and_then(|r| r.as_object())
                            .and_then(|r| r.get("items"))
                            .and_then(|items| items.as_array())
                        {
                            for reply_val in replies {
                                if let Some(reply_obj) = reply_val.as_object() {
                                    if let Some(rid) =
                                        reply_obj.get("id").and_then(|id| id.as_i64())
                                    {
                                        comment_ids.push(format!("{}.{}", comment_id, rid));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(CommentsResult::CommentIdList(deduplicate(&comment_ids)))
            }
            CommentQueryMode::Comments => {
                let mut detailed = Vec::new();
                for comment in &comments {
                    let comment_id = comment.get("id").and_then(|id| id.as_i64()).unwrap_or(0);

                    let replies: Vec<JsonObject> = if source == CommentSource::Forum {
                        let reply_stream = ForumDataFetcher::new()
                            .fetch_reply_comments_gen(comment_id as i32, None);
                        reply_stream
                            .filter_map(|r| r.ok())
                            .filter_map(|v| v.as_object().cloned())
                            .filter_map(|reply| {
                                build_compact_reply(&reply, user_field, &extract_reply_user_id)
                            })
                            .collect()
                    } else {
                        comment
                            .get("replies")
                            .and_then(|r| r.as_object())
                            .and_then(|r| r.get("items"))
                            .and_then(|items| items.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_object())
                                    .filter_map(|reply| {
                                        build_compact_reply(
                                            reply,
                                            user_field,
                                            &extract_reply_user_id,
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default()
                    };

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
                    if let Some(content) = comment.get("emoji_content") {
                        comment_data.insert("emoji_content".into(), content.clone());
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

/// 辅助函数：从原始回复对象构建精简的 JsonObject
fn build_compact_reply(
    reply: &JsonObject,
    user_field: &str,
    extract_id: &dyn Fn(&JsonObject) -> Option<i64>,
) -> Option<JsonObject> {
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
    let user_id = extract_id(reply)?;
    let nickname = reply
        .get(user_field)
        .and_then(|u| u.as_object())
        .and_then(|u| u.get("nickname"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let emoji_content = reply
        .get("emoji_content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let mut obj = JsonObject::new();
    obj.insert("id".into(), Value::Number(id.into()));
    obj.insert("content".into(), Value::String(content));
    obj.insert("created_at".into(), Value::String(created_at));
    obj.insert("user_id".into(), Value::Number(user_id.into()));
    obj.insert("nickname".into(), Value::String(nickname));
    obj.insert("emoji_content".into(), Value::String(emoji_content));
    Some(obj)
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
        let total = match CommunityDataFetcher::new()
            .fetch_message_count(MessageMethod::Web)
            .map_err(DataQueryError::from)
        {
            Ok(data) => data.get("count").and_then(|c| c.as_i64()).unwrap_or(0) as i32,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };

        let remaining = if limit == 0 { total } else { limit.min(total) };

        Box::new(CommunityReplyStream {
            reply_type,
            remaining,
            offset: 0,
            buffer: VecDeque::new(),
        })
    }

    /// 获取评论总数
    pub fn count_comments(
        &self,
        source: CommentSource,
        target_id: i32,
    ) -> Result<i32, DataQueryError> {
        let client = CodeMaoClient::global();
        match source {
            CommentSource::Work => {
                let response = client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}/comments", target_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "15")
                    .send()
                    .map_err(DataQueryError::from)?;

                let json = client
                    .response_to_json(response)
                    .map_err(DataQueryError::from)?;

                if let Some(total) = json.get("total").and_then(|t| t.as_i64()) {
                    return Ok(total as i32);
                }

                let work_response = client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}", target_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .map_err(DataQueryError::from)?;

                let work_json = client
                    .response_to_json(work_response)
                    .map_err(DataQueryError::from)?;

                Ok(work_json
                    .get("comment_times")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32)
            }
            CommentSource::Shop => {
                let response = client
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
                    .map_err(DataQueryError::from)?;

                let json = client
                    .response_to_json(response)
                    .map_err(DataQueryError::from)?;

                let total = json.get("total").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                let total_reply =
                    json.get("totalReply").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                Ok(total + total_reply)
            }
            CommentSource::Forum => {
                let response = client
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/forums/posts/{}/details", target_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .map_err(DataQueryError::from)?;

                let json = client
                    .response_to_json(response)
                    .map_err(DataQueryError::from)?;

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

        let nemo_result = WorkDataFetcher::new().fetch_new_works_nemo(
            NemoWorkType::Original,
            per_source_limit,
            None,
        );
        let web_result = WorkDataFetcher::new().fetch_new_works_web(per_source_limit, None, true);

        let process_result = |res: Result<Value, MewError>, mapping: HashMap<&str, &str>| match res
        {
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
                DataQueryError::from(e),
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
            let comment_count = WhaleReportFetcher::new()
                .fetch_comment_reports_total(
                    CommentSourceType::All,
                    ReportStatus::All,
                    Some(CommentReportFilterType::AdminId),
                    Some(admin_id),
                )
                .map_err(DataQueryError::from)?
                .get("total")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32;

            let work_count = WhaleReportFetcher::new()
                .fetch_work_reports_total(
                    WorkSourceType::All,
                    ReportStatus::All,
                    Some(WorkReportFilterType::AdminId),
                    Some(admin_id),
                )
                .map_err(DataQueryError::from)?
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
    ///
    /// 注意：为每个符合条件的粉丝单独查询荣誉数据（N+1 请求），需评估性能影响。
    pub fn compute_fans_by_like_threshold(
        &self,
        user_id: i32,
        like_threshold: i32,
    ) -> Result<FanByLikesStatistics, DataQueryError> {
        let fans_stream = UserDataFetcher::new().fetch_followers_gen(user_id, None);

        let mut qualified_fans = Vec::new();
        let mut total_fans = 0;

        for fan_result in fans_stream {
            let fan = fan_result.map_err(DataQueryError::from)?;
            total_fans += 1;

            let total_likes = fan.get("total_likes").and_then(|l| l.as_i64()).unwrap_or(0) as i32;

            if total_likes >= like_threshold {
                let mut fan_obj = JsonObject::new();
                if let Some(id) = fan.get("id").and_then(|i| i.as_i64()) {
                    fan_obj.insert("user_id".into(), Value::Number(id.into()));
                    let honors_result = UserDataFetcher::new()
                        .fetch_user_honors(id as i32)
                        .map_err(DataQueryError::from);
                    if let Ok(ref honors_data) = honors_result {
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
    ///
    /// 为防止一次性加载过多学生造成 OOM，会限制最大学生数（默认 2000）。
    /// 保持原始顺序，不再进行随机打乱。
    pub fn stream_edu_accounts_with_reset_passwords(
        &self,
        limit: Option<usize>,
    ) -> Box<dyn Iterator<Item = Result<(String, String), DataQueryError>> + 'static> {
        const MAX_EDU_STUDENTS: usize = 2000;

        if let Err(e) = CodeMaoClient::global().switch_identity(Catsona::Scholar) {
            return Box::new(std::iter::once(Err(DataQueryError::from(e))));
        }

        let effective_limit = limit.unwrap_or(MAX_EDU_STUDENTS).min(MAX_EDU_STUDENTS);

        // 直接使用接口返回的迭代器，保留原始顺序
        let stream = EduDataFetcher::new()
            .fetch_class_students_gen(1, Some(effective_limit))
            .filter_map(move |student_result| {
                let student = match student_result {
                    Ok(s) => s,
                    Err(e) => return Some(Err(DataQueryError::from(e))),
                };

                let student_id = student.get("id").and_then(|i| i.as_i64())? as i32;
                let username = student
                    .get("username")
                    .and_then(|u| u.as_str())?
                    .to_string();

                let password_result = EduUserAction::new()
                    .reset_student_password(student_id)
                    .map_err(DataQueryError::from);

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
            })
            .take(effective_limit); // 额外安全限流

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

/// 社区新回复分页流（健壮版，不再依赖总数）
struct CommunityReplyStream {
    reply_type: ReplyTypes,
    remaining: i32, // 剩余待取数量（i32::MAX 表示无上限）
    offset: i32,
    buffer: VecDeque<JsonObject>,
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
        match CommunityDataFetcher::new().fetch_replies(self.reply_type, batch_size, self.offset) {
            Ok(response) => {
                let items: Vec<JsonObject> = response
                    .get("items")
                    .and_then(|i| i.as_array())
                    .into_iter()
                    .flat_map(|arr| arr.iter().filter_map(|v| v.as_object().cloned()))
                    .collect();

                let fetched_count = items.len() as i32;
                if fetched_count == 0 {
                    return None;
                }

                let take_count = fetched_count.min(self.remaining) as usize;
                self.remaining -= take_count as i32;
                self.offset += fetched_count; // 基于实际返回量推进偏移
                self.buffer.extend(items.into_iter().take(take_count));
                self.buffer.pop_front().map(Ok)
            }
            Err(e) => Some(Err(DataQueryError::from(e))),
        }
    }
}

// ==================== 工具函数 ====================

/// 字符串数组去重（保留原始顺序）
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
