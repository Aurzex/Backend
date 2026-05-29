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
use crate::utils::acquire::{
    BaseKey, Catsona, HttpMethod, KittyFactory, MewError, MewResult, PaginatedIter,
};

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
    External(Box<dyn std::error::Error>),
}

impl From<MewError> for DataQueryError {
    fn from(e: MewError) -> Self {
        DataQueryError::External(Box::new(e))
    }
}

impl From<reqwest::Error> for DataQueryError {
    fn from(e: reqwest::Error) -> Self {
        DataQueryError::External(Box::new(e))
    }
}

fn to_external_err<E: std::error::Error + 'static>(e: E) -> DataQueryError {
    DataQueryError::External(Box::new(e))
}

// ==================== 枚举定义 ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentSource {
    Work,
    Forum,
    Shop,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentQueryMode {
    UserId,
    CommentId,
    Comments,
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

#[derive(Debug, Clone, Copy)]
pub enum NotificationCategory {
    LikeFork,
    CommentReply,
    System,
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

#[derive(Debug, Clone)]
pub enum CommentsResult {
    UserIdList(Vec<String>),
    CommentIdList(Vec<String>),
    DetailedComments(Vec<JsonObject>),
}

// ==================== 查询构建器 ====================

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

    fn build_comment_iter(&self) -> Result<PaginatedIter, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let target_id = self
            .target_id
            .ok_or_else(|| DataQueryError::InvalidSource("未设置源ID".into()))?;
        let limit = self.limit;

        Ok(match source {
            CommentSource::Work => WorkDataFetcher::new().fetch_work_comments_gen(target_id, limit),
            CommentSource::Forum => {
                ForumDataFetcher::new().fetch_post_replies_gen(target_id, None, limit)
            }
            CommentSource::Shop => WorkshopDataFetcher::new()
                .fetch_workshop_discussions_gen(target_id, None, None, limit),
        })
    }

    pub async fn execute(self) -> Result<CommentsResult, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let mode = self.mode;

        let mut comment_iter = self.build_comment_iter()?;

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

        let max_limit = self.limit.unwrap_or(500).min(1000);

        match mode {
            CommentQueryMode::UserId => {
                let mut user_ids = Vec::new();
                let mut count = 0;

                while let Some(item) = comment_iter.next_item().await {
                    let comment = item?;
                    count += 1;
                    if count > max_limit {
                        break;
                    }

                    if let Some(id) = comment
                        .get("user")
                        .and_then(|u| u.as_object())
                        .and_then(|u| u.get("id"))
                        .and_then(|id| id.as_i64())
                    {
                        user_ids.push(id.to_string());
                    }

                    let comment_id = comment.get("id").and_then(|id| id.as_i64()).unwrap_or(0);

                    if source == CommentSource::Forum {
                        let mut reply_iter = ForumDataFetcher::new()
                            .fetch_reply_comments_gen(comment_id as i32, None);
                        while let Some(reply_result) = reply_iter.next_item().await {
                            let reply_val = reply_result?;
                            if let Some(reply_obj) = reply_val.as_object()
                                && let Some(uid) = extract_reply_user_id(reply_obj)
                            {
                                user_ids.push(uid.to_string());
                            }
                        }
                    } else if let Some(replies) = comment
                        .get("replies")
                        .and_then(|r| r.as_object())
                        .and_then(|r| r.get("items"))
                        .and_then(|items| items.as_array())
                    {
                        for reply_val in replies {
                            let Some(reply_obj) = reply_val.as_object() else {
                                continue;
                            };
                            if let Some(uid) = extract_reply_user_id(reply_obj) {
                                user_ids.push(uid.to_string());
                            }
                        }
                    }
                }
                Ok(CommentsResult::UserIdList(deduplicate(&user_ids)))
            }
            CommentQueryMode::CommentId => {
                let mut comment_ids = Vec::new();
                let mut count = 0;

                while let Some(item) = comment_iter.next_item().await {
                    let comment = item?;
                    count += 1;
                    if count > max_limit {
                        break;
                    }

                    if let Some(id) = comment.get("id").and_then(|id| id.as_i64()) {
                        comment_ids.push(id.to_string());
                    }

                    let comment_id = comment.get("id").and_then(|id| id.as_i64()).unwrap_or(0);

                    if source == CommentSource::Forum {
                        let mut reply_iter = ForumDataFetcher::new()
                            .fetch_reply_comments_gen(comment_id as i32, None);
                        while let Some(reply_result) = reply_iter.next_item().await {
                            let reply_val = reply_result?;
                            if let Some(reply_obj) = reply_val.as_object()
                                && let Some(rid) = reply_obj.get("id").and_then(|id| id.as_i64())
                            {
                                comment_ids.push(format!("{}.{}", comment_id, rid));
                            }
                        }
                    } else if let Some(replies) = comment
                        .get("replies")
                        .and_then(|r| r.as_object())
                        .and_then(|r| r.get("items"))
                        .and_then(|items| items.as_array())
                    {
                        for reply_val in replies {
                            if let Some(reply_obj) = reply_val.as_object()
                                && let Some(rid) = reply_obj.get("id").and_then(|id| id.as_i64())
                            {
                                comment_ids.push(format!("{}.{}", comment_id, rid));
                            }
                        }
                    }
                }
                Ok(CommentsResult::CommentIdList(deduplicate(&comment_ids)))
            }
            CommentQueryMode::Comments => {
                let mut detailed = Vec::new();
                let mut count = 0;

                while let Some(item) = comment_iter.next_item().await {
                    let comment = item?;
                    count += 1;
                    if count > max_limit {
                        break;
                    }

                    let comment_id = comment.get("id").and_then(|id| id.as_i64()).unwrap_or(0);

                    let replies: Vec<JsonObject> = if source == CommentSource::Forum {
                        let mut reply_iter = ForumDataFetcher::new()
                            .fetch_reply_comments_gen(comment_id as i32, None);
                        let mut replies_list = Vec::new();
                        while let Some(reply_result) = reply_iter.next_item().await {
                            let reply_val = reply_result?;
                            if let Some(reply_obj) = reply_val.as_object()
                                && let Some(compact) = build_compact_reply(
                                    reply_obj,
                                    user_field,
                                    &extract_reply_user_id,
                                )
                            {
                                replies_list.push(compact);
                            }
                        }
                        replies_list
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

pub struct DataQuery;

impl DataQuery {
    pub fn new() -> Self {
        DataQuery
    }

    pub fn query_comments(&self) -> CommentQueryBuilder {
        CommentQueryBuilder::new()
    }

    pub async fn fetch_comments(
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
            .await
    }

    pub async fn stream_new_replies(
        &self,
        reply_type: ReplyTypes,
        limit: i32,
    ) -> CommunityReplyStream {
        // 直接返回类型，不需要 Box
        let data = CommunityDataFetcher::new()
            .fetch_message_count(MessageMethod::Web)
            .await
            .unwrap();
        let total = data.get("count").and_then(|c| c.as_i64()).unwrap_or(0) as i32;

        CommunityReplyStream::new(reply_type, total, limit)
    }

    pub async fn count_comments(
        &self,
        source: CommentSource,
        target_id: i32,
    ) -> Result<i32, DataQueryError> {
        match source {
            CommentSource::Work => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}/comments", target_id),
                        Some(BaseKey::Default),
                    )
                    .with_param("offset", "0")
                    .with_param("limit", "15")
                    .send()
                    .await?;
                let json: Value = resp.json().await?;

                if let Some(total) = json.get("total").and_then(|t| t.as_i64()) {
                    return Ok(total as i32);
                }

                let work_resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/creation-tools/v1/works/{}", target_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .await?;
                let work_json: Value = work_resp.json().await?;
                Ok(work_json
                    .get("comment_times")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32)
            }
            CommentSource::Shop => {
                let resp = KittyFactory::global_client()
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
                    .await?;
                let json: Value = resp.json().await?;
                let total = json.get("total").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                let total_reply =
                    json.get("totalReply").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                Ok(total + total_reply)
            }
            CommentSource::Forum => {
                let resp = KittyFactory::global_client()
                    .build_request(
                        HttpMethod::GET,
                        &format!("/web/forums/posts/{}/details", target_id),
                        Some(BaseKey::Default),
                    )
                    .send()
                    .await?;
                let json: Value = resp.json().await?;
                let n_replies = json.get("n_replies").and_then(|r| r.as_i64()).unwrap_or(0) as i32;
                let n_comments =
                    json.get("n_comments").and_then(|c| c.as_i64()).unwrap_or(0) as i32;
                Ok(n_replies + n_comments)
            }
        }
    }

    pub async fn stream_works_from_both_sources(&self, limit: i32) -> MergedWorksStream {
        let per_source_limit: Option<i32> = Some(limit / 2);

        let nemo_iter = WorkDataFetcher::new().fetch_new_works_nemo(
            NemoWorkType::Original,
            per_source_limit,
            None,
        );
        let web_iter = WorkDataFetcher::new().fetch_new_works_web(per_source_limit, None, true);

        MergedWorksStream::new(nemo_iter, web_iter, per_source_limit.map(|x| x as usize))
    }

    pub async fn aggregate_user_comments_from_works(
        &self,
        work_limit: i32,
    ) -> Result<Vec<JsonObject>, DataQueryError> {
        let mut work_stream = self.stream_works_from_both_sources(work_limit).await;
        let mut works = Vec::new();

        while let Some(work_result) = work_stream.next().await {
            works.push(work_result?);
        }

        let mut all_comments = Vec::new();
        for work in &works {
            if let Some(work_id) = work.get("work_id").and_then(|id| id.as_i64())
                && let Ok(CommentsResult::DetailedComments(comments)) = self
                    .fetch_comments(
                        CommentSource::Work,
                        work_id as i32,
                        CommentQueryMode::Comments,
                        Some(20),
                    )
                    .await
            {
                all_comments.extend(comments);
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

    pub async fn compute_admin_report_stats(
        &self,
    ) -> Result<AdminReportStatistics, DataQueryError> {
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
                .await
                .map_err(|e| DataQueryError::External(e.into()))?
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
                .await
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

        stats.sort_by_key(|b| std::cmp::Reverse(b.total_reports));

        Ok(AdminReportStatistics {
            total_admins: stats.len() as i32,
            total_comment_reports,
            total_work_reports,
            total_all_reports: grand_total,
            statistics: stats,
        })
    }

    pub async fn compute_fans_by_like_threshold(
        &self,
        user_id: i32,
        like_threshold: i32,
    ) -> Result<FanByLikesStatistics, DataQueryError> {
        let mut fans_stream = UserDataFetcher::new().fetch_followers_gen(user_id, None);

        let mut qualified_fans = Vec::new();
        let mut total_fans = 0;

        while let Some(fan_result) = fans_stream.next_item().await {
            let fan = fan_result.map_err(to_external_err)?;
            total_fans += 1;

            let total_likes = fan.get("total_likes").and_then(|l| l.as_i64()).unwrap_or(0) as i32;

            if total_likes >= like_threshold {
                let mut fan_obj = JsonObject::new();
                if let Some(id) = fan.get("id").and_then(|i| i.as_i64()) {
                    fan_obj.insert("user_id".into(), Value::Number(id.into()));
                    let honors = UserDataFetcher::new()
                        .fetch_user_honors(id as i32)
                        .await
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

    pub fn stream_edu_accounts_with_reset_passwords(
        &self,
        limit: Option<usize>,
    ) -> Result<EduStudentAccountIter, MewError> {
        KittyFactory::global_client().switch_identity(Catsona::Scholar)?;
        Ok(EduStudentAccountIter::new(1, limit))
    }
}

impl Default for DataQuery {
    fn default() -> Self {
        Self::new()
    }
}
// 定义学生账号迭代器
pub struct EduStudentAccountIter {
    student_iter: PaginatedIter,
    limit: usize,
    yielded: usize,
}

impl EduStudentAccountIter {
    fn new(class_id: i32, limit: Option<usize>) -> Self {
        let limit = limit.unwrap_or(2000).min(2000);
        let student_iter = EduDataFetcher::new().fetch_class_students_gen(class_id, Some(limit));

        Self {
            student_iter,
            limit,
            yielded: 0,
        }
    }

    pub async fn next(&mut self) -> Option<Result<(String, String), MewError>> {
        if self.yielded >= self.limit {
            return None;
        }

        while let Some(student_result) = self.student_iter.next_item().await {
            let student = match student_result {
                Ok(s) => s,
                Err(e) => return Some(Err(e)),
            };

            let student_id = match student.get("id").and_then(|i| i.as_i64()) {
                Some(id) => id as i32,
                None => return Some(Err(MewError::Other("Missing student id".into()))),
            };

            let username = match student.get("username").and_then(|u| u.as_str()) {
                Some(u) => u.to_string(),
                None => return Some(Err(MewError::Other("Missing username".into()))),
            };

            // 重置密码
            match EduUserAction::new()
                .reset_student_password(student_id)
                .await
            {
                Ok(password_data) => {
                    let password = password_data
                        .get("password")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    self.yielded += 1;
                    return Some(Ok((username, password)));
                }
                Err(e) => return Some(Err(MewError::Other(e.to_string()))),
            }
        }
        None
    }

    pub async fn collect(mut self) -> Vec<Result<(String, String), MewError>> {
        let mut results = Vec::new();
        while let Some(result) = self.next().await {
            results.push(result);
        }
        results
    }
}

// ==================== 合并作品流 ====================

pub struct MergedWorksStream {
    current: MergedSource,
    next: MergedSource,
    current_remaining: usize,
    next_remaining: usize,
    current_done: bool,
    next_done: bool,
}

enum MergedSource {
    Nemo(PaginatedIter),
    Web(PaginatedIter),
}

impl MergedWorksStream {
    fn new(
        nemo_iter: PaginatedIter,
        web_iter: PaginatedIter,
        per_source_limit: Option<usize>,
    ) -> Self {
        let limit = per_source_limit.unwrap_or(usize::MAX);
        MergedWorksStream {
            current: MergedSource::Nemo(nemo_iter),
            next: MergedSource::Web(web_iter),
            current_remaining: limit,
            next_remaining: limit,
            current_done: false,
            next_done: false,
        }
    }

    pub async fn next(&mut self) -> Option<Result<JsonObject, DataQueryError>> {
        loop {
            if self.current_done || self.current_remaining == 0 {
                if self.next_done || self.next_remaining == 0 {
                    return None;
                }
                std::mem::swap(&mut self.current, &mut self.next);
                std::mem::swap(&mut self.current_remaining, &mut self.next_remaining);
                std::mem::swap(&mut self.current_done, &mut self.next_done);
                continue;
            }

            let iter = match &mut self.current {
                MergedSource::Nemo(iter) => iter,
                MergedSource::Web(iter) => iter,
            };

            match iter.next_item().await {
                Some(Ok(value)) => {
                    self.current_remaining -= 1;
                    if let Some(obj) = value.as_object() {
                        let mapped = match &self.current {
                            MergedSource::Nemo(_) => map_nemo_work(obj),
                            MergedSource::Web(_) => map_web_work(obj),
                        };
                        return Some(Ok(mapped));
                    }
                }
                Some(Err(e)) => return Some(Err(DataQueryError::External(Box::new(e)))),
                None => {
                    self.current_done = true;
                    continue;
                }
            }
        }
    }
}

fn map_nemo_work(obj: &JsonObject) -> JsonObject {
    let mut mapped = JsonObject::new();
    for (target, source) in &[
        ("work_id", "work_id"),
        ("work_name", "work_name"),
        ("user_name", "user_name"),
        ("user_id", "user_id"),
        ("like_count", "like_count"),
        ("updated_at", "updated_at"),
    ] {
        if let Some(val) = obj.get(*source) {
            mapped.insert(target.to_string(), val.clone());
        }
    }
    mapped
}

fn map_web_work(obj: &JsonObject) -> JsonObject {
    let mut mapped = JsonObject::new();
    for (target, source) in &[
        ("work_id", "work_id"),
        ("work_name", "work_name"),
        ("user_name", "nickname"),
        ("user_id", "user_id"),
        ("like_count", "likes_count"),
        ("updated_at", "updated_at"),
    ] {
        if let Some(val) = obj.get(*source) {
            mapped.insert(target.to_string(), val.clone());
        }
    }
    mapped
}

// ==================== 辅助数据结构 ====================

#[derive(Debug, Clone)]
pub struct AdminReportStatsEntry {
    pub admin_id: i32,
    pub admin_name: String,
    pub comment_reports: i32,
    pub work_reports: i32,
    pub total_reports: i32,
    pub percentage: f64,
}

#[derive(Debug, Clone)]
pub struct AdminReportStatistics {
    pub total_admins: i32,
    pub total_comment_reports: i32,
    pub total_work_reports: i32,
    pub total_all_reports: i32,
    pub statistics: Vec<AdminReportStatsEntry>,
}

#[derive(Debug, Clone)]
pub struct FanByLikesStatistics {
    pub target_user_id: i32,
    pub like_threshold: i32,
    pub total_fans: i32,
    pub qualified_fans_count: i32,
    pub qualified_fans: Vec<JsonObject>,
}

// ==================== 辅助迭代器实现 ====================

pub struct CommunityReplyStream {
    reply_type: ReplyTypes,
    remaining: i32,
    offset: i32,
    buffer: VecDeque<JsonObject>,
}

impl CommunityReplyStream {
    pub fn new(reply_type: ReplyTypes, total: i32, limit: i32) -> Self {
        let remaining = if limit == 0 { total } else { limit.min(total) };
        Self {
            reply_type,
            remaining,
            offset: 0,
            buffer: VecDeque::new(),
        }
    }

    pub async fn next(&mut self) -> Option<Result<JsonObject, DataQueryError>> {
        if let Some(obj) = self.buffer.pop_front() {
            return Some(Ok(obj));
        }
        if self.remaining <= 0 {
            return None;
        }

        let batch_size = self.remaining.clamp(5, 200);
        match CommunityDataFetcher::new()
            .fetch_replies(self.reply_type, batch_size, self.offset)
            .await
        {
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
                self.offset += fetched_count;
                self.buffer.extend(items.into_iter().take(take_count));
                self.buffer.pop_front().map(Ok)
            }
            Err(e) => Some(Err(DataQueryError::External(e.into()))),
        }
    }

    pub async fn collect(mut self) -> Vec<Result<JsonObject, DataQueryError>> {
        let mut results = Vec::new();
        while let Some(result) = self.next().await {
            results.push(result);
        }
        results
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
