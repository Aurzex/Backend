use std::collections::{HashMap, HashSet, VecDeque};
use std::thread;

use log::warn;
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
use crate::utils::requests::{
    BaseKey, Catsona, CodeMaoClient, DEFAULT_PAGE_SIZE, MewError, PaginatedIter, PaginationMethod,
};

// 评论流默认值与上限(各语义独立:用户上限/每作品抽样/分页元数据)
const DEFAULT_COMMENT_STREAM_LIMIT: usize = 500;
const MAX_COMMENT_STREAM_LIMIT: usize = 1000;
const COMMENT_DETAIL_PER_WORK: usize = 20;

// 错误类型

#[derive(Error, Debug)]
pub enum DataQueryError {
    #[error("无效的来源类型: {0}")]
    InvalidSource(String),
    #[error("数据解析失败: {0}")]
    ParseError(String),
    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),
    #[error("外部错误: {0}")]
    External(#[from] MewError),
}

// 枚举定义

/// 评论来源类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommentSource {
    Work,  // 作品评论
    Forum, // 论坛帖子评论
    Shop,  // 工坊讨论评论
}

impl CommentSource {
    pub(crate) fn as_str(&self) -> &'static str {
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

/// 通知类型分类
#[derive(Debug, Clone, Copy)]
pub enum NotificationCategory {
    LikeFork,     // 点赞/收藏
    CommentReply, // 评论/回复
    System,       // 系统通知
}

impl NotificationCategory {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            NotificationCategory::LikeFork => "LIKE_FORK",
            NotificationCategory::CommentReply => "COMMENT_REPLY",
            NotificationCategory::System => "SYSTEM",
        }
    }
}

// 数据类型别名

pub(crate) type JsonValue = Value;
pub(crate) type JsonObject = Map<String, Value>;
/// 惰性值流(统一迭代器特征对象签名)
pub type JsonValueIter = Box<dyn Iterator<Item = Result<JsonValue, DataQueryError>>>;
/// 惰性对象流
pub type JsonObjIter = Box<dyn Iterator<Item = Result<JsonObject, DataQueryError>>>;
/// 惰性字符串流
pub type JsonStrIter = Box<dyn Iterator<Item = Result<String, DataQueryError>>>;
/// 惰性字符串对流
pub type JsonPairIter = Box<dyn Iterator<Item = Result<(String, String), DataQueryError>>>;

// 惰性去重迭代器

/// 惰性去重迭代器,仅保留第一次出现的元素(基于 `HashSet` 记录)
/// 遇到 `Err` 直接返回并停止去重状态,但后续 `Ok` 仍会继续去重
pub(crate) struct UniqueIter<I: Iterator> {
    iter: I,
    seen: HashSet<String>,
}

impl<I: Iterator<Item = Result<String, DataQueryError>>> Iterator for UniqueIter<I> {
    type Item = Result<String, DataQueryError>;

    fn next(&mut self) -> Option<Self::Item> {
        for item in &mut self.iter {
            match item {
                Ok(val) => {
                    if self.seen.insert(val.clone()) {
                        return Some(Ok(val));
                    }
                    // 重复值,跳过,继续循环
                }
                Err(e) => return Some(Err(e)),
            }
        }
        None
    }
}

// 辅助函数

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

// 评论查询构建器

/// 评论查询构建器(纯惰性流式)
pub struct CommentQueryBuilder {
    source: Option<CommentSource>,
    target_id: Option<i32>,
    limit: Option<usize>,
}

impl Default for CommentQueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 分块并行映射评论流:每块至多 8 条主评论,仅论坛来源在线程内并行执行映射
/// (其余来源回复内联、无网络开销,串行更省)。输出保持原顺序,错误项透传。
fn map_comments_chunked<E, T, F>(
    source: CommentSource,
    mut raw: impl Iterator<Item = Result<Value, E>>,
    f: F,
) -> impl Iterator<Item = Result<T, E>>
where
    E: Send,
    T: Send,
    F: Fn(&Value) -> Vec<Result<T, E>> + Sync,
{
    const CHUNK: usize = 8;
    let parallel = source == CommentSource::Forum;
    let mut buf: Vec<Result<Value, E>> = Vec::with_capacity(CHUNK);
    std::iter::from_fn(move || {
        // 填满一块(或流结束)
        while buf.len() < CHUNK {
            match raw.next() {
                Some(item) => buf.push(item),
                None => break,
            }
        }
        if buf.is_empty() {
            return None;
        }
        let chunk = std::mem::take(&mut buf);
        let items = if parallel && chunk.len() > 1 {
            // 错误项保留原位,有效值在线程中并行映射后按原顺序合并
            let mut slots: Vec<Option<Vec<Result<T, E>>>> = chunk.iter().map(|_| None).collect();
            std::thread::scope(|s| {
                for (slot, item) in slots.iter_mut().zip(&chunk) {
                    if let Ok(v) = item {
                        // 借用捕获(f 仅需 Sync);move 会按值捕获 F,要求 Send 且与顺序路径冲突
                        let _ = s.spawn(|| {
                            *slot = Some(f(v));
                        });
                    }
                }
            });
            chunk
                .into_iter()
                .zip(slots)
                .flat_map(|(item, slot)| match item {
                    Err(e) => vec![Err(e)],
                    Ok(_) => slot.unwrap_or_default(),
                })
                .collect()
        } else {
            let mut out = Vec::new();
            for item in chunk {
                match item {
                    Ok(v) => out.extend(f(&v)),
                    Err(e) => out.push(Err(e)),
                }
            }
            out
        };
        Some(items)
    })
    .flatten()
}

impl CommentQueryBuilder {
    pub fn new() -> Self {
        Self {
            source: None,
            target_id: None,
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

    pub fn limit(mut self, limit: Option<usize>) -> Self {
        self.limit = limit;
        self
    }

    /// 构建基础评论流(原始 `JsonValue`)
    fn build_raw_stream(&self) -> Result<JsonValueIter, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let target_id = self
            .target_id
            .ok_or_else(|| DataQueryError::InvalidSource("未设置源ID".into()))?;
        let safe_limit = self
            .limit
            .unwrap_or(DEFAULT_COMMENT_STREAM_LIMIT)
            .min(MAX_COMMENT_STREAM_LIMIT);

        match source {
            CommentSource::Work => {
                let iter = WorkDataFetcher::new()
                    .fetch_work_comments_gen(target_id, Some(safe_limit))
                    .map(|item| item.map_err(DataQueryError::from));
                Ok(Box::new(iter))
            }
            CommentSource::Forum => {
                let iter = ForumDataFetcher::new()
                    .fetch_post_replies_gen(target_id, None, Some(safe_limit))
                    .map(|item| item.map_err(DataQueryError::from));
                Ok(Box::new(iter))
            }
            CommentSource::Shop => {
                let iter = WorkshopDataFetcher::new()
                    .fetch_workshop_discussions_gen(target_id, None, None, Some(safe_limit))
                    .map(|item| item.map_err(DataQueryError::from));
                Ok(Box::new(iter))
            }
        }
    }

    /// 获取某条主评论下的所有回复对象
    /// 论坛来源需额外请求回复接口;其余来源直接取内联的 `replies.items` 字段
    fn reply_items(
        source: CommentSource,
        comment_id: i64,
        comment_obj: &JsonObject,
    ) -> Vec<Result<JsonObject, DataQueryError>> {
        if source == CommentSource::Forum {
            ForumDataFetcher::new()
                .fetch_reply_comments_gen(i32::try_from(comment_id).unwrap_or(0), None)
                .map(|r| {
                    r.map_err(DataQueryError::from).and_then(|v| {
                        v.as_object()
                            .cloned()
                            .ok_or_else(|| DataQueryError::ParseError("回复不是对象".into()))
                    })
                })
                .collect()
        } else {
            comment_obj
                .get("replies")
                .and_then(|r| r.as_object())
                .and_then(|r| r.get("items"))
                .and_then(|items| items.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_object().cloned())
                        .map(Ok)
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    /// 惰性获取评论原始数据流
    pub fn stream_raw_comments(self) -> Result<JsonValueIter, DataQueryError> {
        self.build_raw_stream()
    }

    /// 惰性获取去重后的用户ID流
    pub fn stream_user_ids(self) -> Result<JsonStrIter, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let raw_stream = self.build_raw_stream()?;

        let user_field = match source {
            CommentSource::Work | CommentSource::Shop => "reply_user",
            CommentSource::Forum => "user",
        };

        let extract_reply_user_id = move |reply: &JsonObject| -> Option<i64> {
            reply
                .get(user_field)
                .and_then(|u| u.as_object())
                .and_then(|u| u.get("id"))
                .and_then(serde_json::Value::as_i64)
        };

        let mapped = map_comments_chunked(source, raw_stream, move |comment| {
            let mut ids: Vec<Result<String, DataQueryError>> = Vec::new();

            // 主评论用户
            if let Some(obj) = comment.as_object()
                && let Some(uid) = obj
                    .get("user")
                    .and_then(|u| u.as_object())
                    .and_then(|u| u.get("id"))
                    .and_then(serde_json::Value::as_i64)
            {
                ids.push(Ok(uid.to_string()));
            }

            let comment_id = comment
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            // 借用而非克隆:仅读取回复无需整对象深拷贝
            if let Some(comment_obj) = comment.as_object() {
                for reply in Self::reply_items(source, comment_id, comment_obj) {
                    match reply {
                        Ok(reply_obj) => {
                            if let Some(uid) = extract_reply_user_id(&reply_obj) {
                                ids.push(Ok(uid.to_string()));
                            }
                        }
                        Err(e) => ids.push(Err(e)),
                    }
                }
            }
            ids
        });

        Ok(Box::new(UniqueIter {
            iter: mapped,
            seen: HashSet::new(),
        }))
    }

    /// 惰性获取去重后的评论ID流,格式为 "主评论ID" 或 "主评论ID.回复ID"
    pub fn stream_comment_ids(self) -> Result<JsonStrIter, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let raw_stream = self.build_raw_stream()?;

        let mapped = map_comments_chunked(source, raw_stream, move |comment| {
            let mut ids: Vec<Result<String, DataQueryError>> = Vec::new();
            if let Some(comment_obj) = comment.as_object() {
                let comment_id = comment_obj
                    .get("id")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);

                if comment_id != 0 {
                    ids.push(Ok(comment_id.to_string()));
                }

                for reply in Self::reply_items(source, comment_id, comment_obj) {
                    match reply {
                        Ok(reply_obj) => {
                            if let Some(rid) =
                                reply_obj.get("id").and_then(serde_json::Value::as_i64)
                            {
                                ids.push(Ok(format!("{}.{}", comment_id, rid)));
                            }
                        }
                        Err(e) => ids.push(Err(e)),
                    }
                }
            }
            ids
        });

        Ok(Box::new(UniqueIter {
            iter: mapped,
            seen: HashSet::new(),
        }))
    }

    /// 惰性获取详细评论数据流,每个元素为一个精简的 `JsonObject`,包含其下所有回复
    pub fn stream_detailed_comments(self) -> Result<JsonObjIter, DataQueryError> {
        let source = self
            .source
            .ok_or_else(|| DataQueryError::InvalidSource("未设置来源".into()))?;
        let raw_stream = self.build_raw_stream()?;

        let user_field = match source {
            CommentSource::Work | CommentSource::Shop => "reply_user",
            CommentSource::Forum => "user",
        };

        let extract_reply_user_id = move |reply: &JsonObject| -> Option<i64> {
            reply
                .get(user_field)
                .and_then(|u| u.as_object())
                .and_then(|u| u.get("id"))
                .and_then(serde_json::Value::as_i64)
        };

        let mapped = map_comments_chunked(source, raw_stream, move |comment| {
            let comment_obj = match comment.as_object() {
                Some(obj) => obj,
                None => return vec![Err(DataQueryError::ParseError("评论不是对象".into()))],
            };
            let comment_id = comment_obj
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);

            // 收集该评论的所有回复(此处回复数量通常较少,收集为 Vec 可以接受)
            let replies: Vec<JsonObject> = Self::reply_items(source, comment_id, comment_obj)
                .into_iter()
                .filter_map(|r| {
                    if let Err(e) = &r {
                        warn!("拉取回复失败,已跳过: {e}");
                    }
                    r.ok()
                })
                .filter_map(|reply| build_compact_reply(&reply, user_field, &extract_reply_user_id))
                .collect();

            let mut comment_data = JsonObject::new();
            if let Some(user) = comment_obj.get("user").and_then(|u| u.as_object()) {
                if let Some(id) = user.get("id") {
                    comment_data.insert("user_id".into(), id.clone());
                }
                if let Some(nick) = user.get("nickname") {
                    comment_data.insert("nickname".into(), nick.clone());
                }
            }
            if let Some(id) = comment_obj.get("id") {
                comment_data.insert("id".into(), id.clone());
            }
            if let Some(content) = comment_obj.get("content") {
                comment_data.insert("content".into(), content.clone());
            }
            if let Some(content) = comment_obj.get("emoji_content") {
                comment_data.insert("emoji_content".into(), content.clone());
            }
            if let Some(created_at) = comment_obj.get("created_at") {
                comment_data.insert("created_at".into(), created_at.clone());
            }
            comment_data.insert(
                "is_top".into(),
                Value::Bool(
                    comment_obj
                        .get("is_top")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                ),
            );
            comment_data.insert(
                "replies".into(),
                Value::Array(replies.into_iter().map(Value::Object).collect()),
            );

            vec![Ok(comment_data)]
        });

        Ok(Box::new(mapped))
    }
}

// 数据查询主结构体

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

    /// 惰性原始评论流
    pub fn stream_comments_raw(
        &self,
        source: CommentSource,
        target_id: i32,
        limit: Option<usize>,
    ) -> Result<JsonValueIter, DataQueryError> {
        self.query_comments()
            .source(source)
            .target_id(target_id)
            .limit(limit)
            .stream_raw_comments()
    }

    /// 惰性用户ID流,已去重
    pub fn stream_user_ids(
        &self,
        source: CommentSource,
        target_id: i32,
        limit: Option<usize>,
    ) -> Result<JsonStrIter, DataQueryError> {
        self.query_comments()
            .source(source)
            .target_id(target_id)
            .limit(limit)
            .stream_user_ids()
    }

    /// 惰性评论ID流,已去重,格式为 "主ID" 或 "主ID.回复ID"
    pub fn stream_comment_ids(
        &self,
        source: CommentSource,
        target_id: i32,
        limit: Option<usize>,
    ) -> Result<JsonStrIter, DataQueryError> {
        self.query_comments()
            .source(source)
            .target_id(target_id)
            .limit(limit)
            .stream_comment_ids()
    }

    /// 惰性详细评论流
    pub fn stream_detailed_comments(
        &self,
        source: CommentSource,
        target_id: i32,
        limit: Option<usize>,
    ) -> Result<JsonObjIter, DataQueryError> {
        self.query_comments()
            .source(source)
            .target_id(target_id)
            .limit(limit)
            .stream_detailed_comments()
    }

    /// 构造分页迭代器并获取元数据中的总数
    fn paginated_total(
        client: &CodeMaoClient,
        endpoint: String,
        total_key: &str,
        configure: impl FnOnce(PaginatedIter) -> PaginatedIter,
    ) -> Result<i32, DataQueryError> {
        let mut paginated = configure(client.build_paginated(endpoint).with_total_key(total_key));
        paginated.fetch_metadata().map_err(DataQueryError::from)?;
        let total = paginated
            .total_items()
            .ok_or_else(|| DataQueryError::ParseError("分页元数据缺少总数".into()))?;
        i32::try_from(total)
            .map_err(|_| DataQueryError::ParseError(format!("总数超出 i32 范围: {}", total)))
    }

    /// 获取评论总数
    pub fn count_comments(
        &self,
        source: CommentSource,
        target_id: i32,
    ) -> Result<i32, DataQueryError> {
        let client = CodeMaoClient::global();
        match source {
            CommentSource::Work => Self::paginated_total(
                client,
                format!("/creation-tools/v1/works/{}/comments", target_id),
                "total",
                |p| {
                    p.with_base_key(BaseKey::Default)
                        .with_page_size(DEFAULT_PAGE_SIZE)
                        .with_pagination_method(PaginationMethod::Offset)
                        .with_offset_key("offset")
                        .with_amount_key("limit")
                },
            ),
            CommentSource::Shop => {
                let configure = |p: PaginatedIter| {
                    p.with_base_key(BaseKey::Default)
                        .with_iter_param("source", "WORK_SHOP")
                        .with_iter_param("sort", "-created_at")
                        .with_page_size(DEFAULT_PAGE_SIZE)
                        .with_pagination_method(PaginationMethod::Offset)
                        .with_offset_key("offset")
                        .with_amount_key("limit")
                };
                let endpoint = format!("/web/discussions/{}/comments", target_id);
                let total = Self::paginated_total(client, endpoint.clone(), "total", configure)?;
                let total_reply = Self::paginated_total(client, endpoint, "totalReply", configure)?;
                Ok(total + total_reply)
            }
            CommentSource::Forum => {
                let configure = |p: PaginatedIter| {
                    p.with_base_key(BaseKey::Default)
                        .with_page_size(DEFAULT_PAGE_SIZE)
                        .with_pagination_method(PaginationMethod::Offset)
                        .with_offset_key("offset")
                        .with_amount_key("limit")
                };
                let endpoint = format!("/web/forums/posts/{}/details", target_id);
                let n_replies =
                    Self::paginated_total(client, endpoint.clone(), "n_replies", configure)?;
                let n_comments = Self::paginated_total(client, endpoint, "n_comments", configure)?;
                Ok(n_replies + n_comments)
            }
        }
    }

    /// 合并 Nemo 和 Web 来源的作品数据流
    pub fn stream_works_from_both_sources(&self, limit: i32) -> JsonObjIter {
        if limit <= 0 {
            return Box::new(std::iter::empty());
        }
        // 奇数 limit 时两源各取 ceil(limit/2),合并处再截断,保证恰好 limit 条
        // 用 limit/2 + limit%2 向上取整,避免 (limit+1) 在 i32::MAX 时溢出
        let per_source_limit = Some(limit / 2 + limit % 2);

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
                let items: &[Value] = val
                    .get("items")
                    .and_then(|i| i.as_array())
                    .map(std::vec::Vec::as_slice)
                    .unwrap_or(&[]);
                let mapped: Vec<Result<JsonObject, DataQueryError>> = items
                    .iter()
                    .filter_map(|v| v.as_object())
                    .map(|obj| {
                        let mut mapped_obj = JsonObject::new();
                        for (target, source) in &mapping {
                            if let Some(val) = obj.get(*source) {
                                mapped_obj.insert(target.to_string(), val.clone());
                            }
                        }
                        Ok(mapped_obj)
                    })
                    .collect();
                Box::new(mapped.into_iter()) as JsonObjIter
            }
            Err(e) => Box::new(std::iter::once::<Result<JsonObject, DataQueryError>>(Err(
                DataQueryError::from(e),
            ))),
        };

        let nemo_stream = process_result(nemo_result, nemo_field_mapping);
        let web_stream = process_result(web_result, web_field_mapping);

        Box::new(nemo_stream.chain(web_stream).take(limit as usize))
    }

    /// 流式聚合:从新作品中收集用户评论并统计(按 chunk=8 有界并行提取,按作品顺序合并)
    pub fn aggregate_user_comments_from_works(
        &self,
        work_limit: i32,
    ) -> Result<Vec<JsonObject>, DataQueryError> {
        // 先串行收集作品(任一流错误直接返回,与现状一致)
        let mut works: Vec<JsonObject> = Vec::new();
        for work_result in self.stream_works_from_both_sources(work_limit) {
            works.push(work_result?);
        }

        // 再按 chunk=8 并行提取评论;每线程维护本地 map,按线程顺序合并
        type UserCommentAggregate = (String, String, Vec<String>, i32);
        let results: Vec<Result<HashMap<String, UserCommentAggregate>, DataQueryError>> =
            thread::scope(|s| {
                let mut handles = Vec::new();
                for chunk in works.chunks(8) {
                    handles.push(s.spawn(move || {
                        let mut local_map: HashMap<String, (String, String, Vec<String>, i32)> =
                            HashMap::new();
                        for work in chunk {
                            if let Some(work_id) =
                                work.get("work_id").and_then(serde_json::Value::as_i64)
                            {
                                let comment_stream = self.stream_detailed_comments(
                                    CommentSource::Work,
                                    i32::try_from(work_id).unwrap_or(0),
                                    Some(COMMENT_DETAIL_PER_WORK),
                                )?;
                                for comment_result in comment_stream {
                                    let comment = comment_result?;
                                    let user_id = comment.get("user_id").and_then(|v| {
                                        if v.is_number() {
                                            Some(v.to_string())
                                        } else {
                                            v.as_str().map(|v| v.to_string())
                                        }
                                    });
                                    let content = comment
                                        .get("content")
                                        .and_then(|c| c.as_str())
                                        .map(|v| v.to_string());
                                    let nickname = comment
                                        .get("nickname")
                                        .and_then(|n| n.as_str())
                                        .map(|v| v.to_string());

                                    if let (Some(uid), Some(cont), Some(nick)) =
                                        (user_id, content, nickname)
                                    {
                                        let entry = local_map
                                            .entry(uid.clone())
                                            .or_insert_with(|| (uid, nick, Vec::new(), 0));
                                        entry.2.push(cont);
                                        entry.3 += 1;
                                    }
                                }
                            }
                        }
                        Ok(local_map)
                    }));
                }
                handles
                    .into_iter()
                    .map(|handle| match handle.join() {
                        Ok(r) => r,
                        Err(_) => Err(DataQueryError::ParseError("评论聚合线程 panic".into())),
                    })
                    .collect()
            });

        // 按线程顺序合并;任一线程出错则返回第一个(线程顺序上的)错误
        let mut user_comment_map: HashMap<String, (String, String, Vec<String>, i32)> =
            HashMap::new();
        for r in results {
            {
                let local_map = r?;
                for (uid, (_u, nick, comments, count)) in local_map {
                    let entry = user_comment_map
                        .entry(uid.clone())
                        .or_insert_with(|| (uid, nick, Vec::new(), 0));
                    entry.2.extend(comments);
                    entry.3 += count;
                }
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
            let ca = a
                .get("comment_count")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let cb = b
                .get("comment_count")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
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

        // 每个管理员的两类总数相互独立,并行请求(8 管理员 × 2 请求 → 约 1 个 RTT)
        let results: Vec<Result<(i32, String, i32, i32), DataQueryError>> = thread::scope(|s| {
            admins
                .iter()
                .map(|&(admin_id, admin_name)| {
                    let handle = s.spawn(move || {
                        // 获取评论举报总数
                        let mut comment_paginated = WhaleReportFetcher::new()
                            .fetch_comment_reports_gen(
                                CommentSourceType::All,
                                ReportStatus::All,
                                Some(CommentReportFilterType::Admin),
                                Some(admin_id),
                                None, // 不设置limit,使用默认值
                            );
                        comment_paginated
                            .fetch_metadata()
                            .map_err(DataQueryError::from)?;
                        let comment_total = comment_paginated.total_items().ok_or_else(|| {
                            DataQueryError::ParseError("评论举报元数据缺少总数".into())
                        })?;
                        let comment_count = i32::try_from(comment_total).map_err(|_| {
                            DataQueryError::ParseError(format!(
                                "评论举报总数超出 i32 范围: {}",
                                comment_total
                            ))
                        })?;

                        // 获取作品举报总数
                        let mut work_paginated = WhaleReportFetcher::new().fetch_work_reports_gen(
                            WorkSourceType::All,
                            ReportStatus::All,
                            Some(WorkReportFilterType::Admin),
                            Some(admin_id),
                            None, // 不设置limit,使用默认值
                        );
                        work_paginated
                            .fetch_metadata()
                            .map_err(DataQueryError::from)?;
                        let work_total = work_paginated.total_items().ok_or_else(|| {
                            DataQueryError::ParseError("作品举报元数据缺少总数".into())
                        })?;
                        let work_count = i32::try_from(work_total).map_err(|_| {
                            DataQueryError::ParseError(format!(
                                "作品举报总数超出 i32 范围: {}",
                                work_total
                            ))
                        })?;

                        Ok((admin_id, admin_name.to_string(), comment_count, work_count))
                    });
                    match handle.join() {
                        Ok(r) => r,
                        Err(_) => Err(DataQueryError::ParseError("统计线程 panic".into())),
                    }
                })
                .collect()
        });

        let mut stats = Vec::with_capacity(admins.len());
        let mut total_comment_reports = 0;
        let mut total_work_reports = 0;

        for r in results {
            let (admin_id, admin_name, comment_count, work_count) = r?;
            total_comment_reports += comment_count;
            total_work_reports += work_count;
            stats.push(AdminReportStatsEntry {
                admin_id,
                admin_name,
                comment_reports: comment_count,
                work_reports: work_count,
                total_reports: comment_count + work_count,
                percentage: 0.0,
            });
        }

        let grand_total = total_comment_reports + total_work_reports;
        for stat in &mut stats {
            stat.percentage = if grand_total > 0 {
                ((f64::from(stat.total_reports) / f64::from(grand_total)) * 1000.0).round() / 10.0
            } else {
                0.0
            };
        }

        stats.sort_by_key(|b| std::cmp::Reverse(b.total_reports));

        Ok(AdminReportStatistics {
            total_admins: i32::try_from(stats.len()).unwrap_or(0),
            total_comment_reports,
            total_work_reports,
            total_all_reports: grand_total,
            statistics: stats,
        })
    }

    /// 获取粉丝统计(基于点赞数阈值)
    /// 注意:为每个符合条件的粉丝单独查询荣誉数据(N+1 请求),按 chunk=16 有界并行执行
    pub fn compute_fans_by_like_threshold(
        &self,
        user_id: i32,
        like_threshold: i32,
    ) -> Result<FanByLikesStatistics, DataQueryError> {
        let fans_stream = UserDataFetcher::new().fetch_followers_gen(user_id, None);

        let mut total_fans = 0;
        // 第一段(串行,无 HTTP):按流序过滤出达标的粉丝,保留 (id, fan, total_likes) 三元组
        let mut qualified: Vec<(i64, Value, i64)> = Vec::new();

        for fan_result in fans_stream {
            let fan = fan_result.map_err(DataQueryError::from)?;
            total_fans += 1;

            let total_likes = fan
                .get("total_likes")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);

            if total_likes >= like_threshold as i64
                && let Some(id) = fan.get("id").and_then(serde_json::Value::as_i64)
            {
                qualified.push((id, fan, total_likes));
            }
        }

        // 第二段:按 chunk=16 并行查询荣誉数据(尽力而为),结果按流序写回
        let mut results: Vec<Option<JsonObject>> = vec![None; qualified.len()];
        thread::scope(|s| {
            let mut remaining: &mut [Option<JsonObject>] = &mut results[..];
            for chunk in qualified.chunks(16) {
                let (head, tail) = remaining.split_at_mut(chunk.len());
                remaining = tail;
                s.spawn(move || {
                    for (i, (id, fan, total_likes)) in chunk.iter().enumerate() {
                        let mut fan_obj = JsonObject::new();
                        fan_obj.insert("user_id".into(), Value::Number((*id).into()));
                        // 荣誉数据为尽力而为:ID 超出 i32 范围或请求失败时输出 N/A
                        let honors_data = i32::try_from(*id)
                            .ok()
                            .and_then(|id32| UserDataFetcher::new().fetch_user_honors(id32).ok());
                        if let Some(ref honors_data) = honors_data {
                            if let Some(fans_total) = honors_data.get("fans_total") {
                                fan_obj.insert("fans_total".into(), fans_total.clone());
                            } else {
                                fan_obj.insert("fans_total".into(), Value::String("N/A".into()));
                            }
                            if let Some(collected_total) = honors_data.get("collected_total") {
                                fan_obj.insert("collected_total".into(), collected_total.clone());
                            } else {
                                fan_obj
                                    .insert("collected_total".into(), Value::String("N/A".into()));
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
                        fan_obj.insert("total_likes".into(), Value::Number((*total_likes).into()));
                        fan_obj.insert(
                            "n_works".into(),
                            fan.get("n_works")
                                .cloned()
                                .unwrap_or(Value::Number(0.into())),
                        );
                        head[i] = Some(fan_obj);
                    }
                });
            }
        });

        let qualified_fans: Vec<JsonObject> = results.into_iter().flatten().collect();

        Ok(FanByLikesStatistics {
            target_user_id: user_id,
            like_threshold,
            total_fans,
            qualified_fans_count: i32::try_from(qualified_fans.len()).unwrap_or(0),
            qualified_fans,
        })
    }

    /// 获取教育账号流(切换身份,重置密码)
    /// 为防止一次性加载过多学生造成 OOM,会限制最大学生数(默认 2000)
    /// 保持原始顺序,不再进行随机打乱
    pub fn stream_edu_accounts_with_reset_passwords(&self, limit: Option<usize>) -> JsonPairIter {
        const MAX_EDU_STUDENTS: usize = 2000;

        if let Err(e) = CodeMaoClient::global().switch_identity(Catsona::Scholar) {
            return Box::new(std::iter::once(Err(DataQueryError::from(e))));
        }

        let effective_limit = limit.unwrap_or(MAX_EDU_STUDENTS).min(MAX_EDU_STUDENTS);

        // 直接使用接口返回的迭代器,保留原始顺序
        let stream = EduDataFetcher::new()
            .fetch_class_students_gen(1, Some(effective_limit))
            .filter_map(move |student_result| {
                let student = match student_result {
                    Ok(s) => s,
                    Err(e) => return Some(Err(DataQueryError::from(e))),
                };

                let student_id =
                    i32::try_from(student.get("id").and_then(serde_json::Value::as_i64)?)
                        .unwrap_or(0);
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

    /// 获取社区新回复流(惰性迭代器)
    pub fn stream_new_replies(&self, reply_type: ReplyTypes, limit: i32) -> JsonObjIter {
        let total = match CommunityDataFetcher::new()
            .fetch_message_count(MessageMethod::Web)
            .map_err(DataQueryError::from)
        {
            Ok(data) => data
                .get("count")
                .and_then(serde_json::Value::as_i64)
                .map(|v| i32::try_from(v).unwrap_or(0))
                .unwrap_or(0),
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
}

impl Default for DataQuery {
    fn default() -> Self {
        Self::new()
    }
}

// 辅助数据结构

/// 管理员举报统计条目
#[derive(Debug, Clone)]
pub struct AdminReportStatsEntry {
    pub(crate) admin_id: i32,
    pub(crate) admin_name: String,
    pub(crate) comment_reports: i32,
    pub(crate) work_reports: i32,
    pub(crate) total_reports: i32,
    pub(crate) percentage: f64,
}

/// 管理员举报统计汇总
#[derive(Debug, Clone)]
pub struct AdminReportStatistics {
    pub(crate) total_admins: i32,
    pub(crate) total_comment_reports: i32,
    pub(crate) total_work_reports: i32,
    pub(crate) total_all_reports: i32,
    pub(crate) statistics: Vec<AdminReportStatsEntry>,
}

/// 粉丝点赞统计
#[derive(Debug, Clone)]
pub struct FanByLikesStatistics {
    pub(crate) target_user_id: i32,
    pub(crate) like_threshold: i32,
    pub(crate) total_fans: i32,
    pub(crate) qualified_fans_count: i32,
    pub(crate) qualified_fans: Vec<JsonObject>,
}

// 辅助迭代器实现

/// 社区新回复分页流(健壮版,不再依赖总数)
struct CommunityReplyStream {
    reply_type: ReplyTypes,
    remaining: i32, // 剩余待取数量(i32::MAX 表示无上限)
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

        let batch_size = self.remaining.clamp(5, 200);
        match CommunityDataFetcher::new().fetch_replies(self.reply_type, batch_size, self.offset) {
            Ok(response) => {
                let items: Vec<JsonObject> = response
                    .get("items")
                    .and_then(|i| i.as_array())
                    .into_iter()
                    .flat_map(|arr| arr.iter().filter_map(|v| v.as_object().cloned()))
                    .collect();

                let fetched_count = i32::try_from(items.len()).unwrap_or(0);
                if fetched_count == 0 {
                    return None;
                }

                let take_count = fetched_count.min(self.remaining);
                self.remaining -= take_count;
                self.offset += fetched_count; // 基于实际返回量推进偏移
                self.buffer.extend(
                    items
                        .into_iter()
                        .take(usize::try_from(take_count).unwrap_or(0)),
                );
                self.buffer.pop_front().map(Ok)
            }
            Err(e) => Some(Err(DataQueryError::from(e))),
        }
    }
}
