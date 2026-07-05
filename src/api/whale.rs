use crate::utils::acquire::{
    BaseKey, CodeMaoClient, HTTPStatus, HttpMethod, KittyRequestBuilder, MewError, MewResult,
    PaginatedIter, PaginationMethod,
};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};

// 工具函数：获取13位时间戳
fn current_timestamp_13() -> u128 {
    let start = SystemTime::now();
    let since_the_epoch = start
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    since_the_epoch.as_millis()
}

// ==================== 举报相关枚举 ====================

// 作品来源类型枚举
pub enum WorkSourceType {
    Kitten,
    Box2,
    All,
}

impl WorkSourceType {
    fn as_str(&self) -> &'static str {
        match self {
            WorkSourceType::Kitten => "KITTEN",
            WorkSourceType::Box2 => "BOX2",
            WorkSourceType::All => "ALL",
        }
    }
}

// 评论来源类型枚举
pub enum CommentSourceType {
    All,
    Kitten,
    Box2,
    Fiction,
    Comic,
    WorkSubject,
}

impl CommentSourceType {
    fn as_str(&self) -> &'static str {
        match self {
            CommentSourceType::All => "ALL",
            CommentSourceType::Kitten => "KITTEN",
            CommentSourceType::Box2 => "BOX2",
            CommentSourceType::Fiction => "FICTION",
            CommentSourceType::Comic => "COMIC",
            CommentSourceType::WorkSubject => "WORK_SUBJECT",
        }
    }
}

// 举报状态枚举
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum ReportStatus {
    ToBeDone,
    Done,
    All,
}

impl ReportStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ReportStatus::ToBeDone => "TOBEDONE",
            ReportStatus::Done => "DONE",
            ReportStatus::All => "ALL",
        }
    }
}

// 作品举报过滤类型枚举
pub enum WorkReportFilterType {
    AdminId,
    WorkUserId,
    WorkId,
}

impl WorkReportFilterType {
    fn as_str(&self) -> &'static str {
        match self {
            WorkReportFilterType::AdminId => "admin_id",
            WorkReportFilterType::WorkUserId => "work_user_id",
            WorkReportFilterType::WorkId => "work_id",
        }
    }
}

// 评论举报过滤类型枚举
pub enum CommentReportFilterType {
    AdminId,
    CommentUserId,
    CommentId,
}

impl CommentReportFilterType {
    fn as_str(&self) -> &'static str {
        match self {
            CommentReportFilterType::AdminId => "admin_id",
            CommentReportFilterType::CommentUserId => "comment_user_id",
            CommentReportFilterType::CommentId => "comment_id",
        }
    }
}

// 帖子举报过滤类型枚举
pub enum PostReportFilterType {
    PostId,
}

impl PostReportFilterType {
    fn as_str(&self) -> &'static str {
        match self {
            PostReportFilterType::PostId => "post_id",
        }
    }
}

// 处理决议枚举
pub enum Resolution {
    Pass,
    Delete,
    Unload,
    MuteSevenDays,
    MuteThreeMonths,
    Tobedone,
}

impl Resolution {
    fn as_str(&self) -> &'static str {
        match self {
            Resolution::Pass => "PASS",
            Resolution::Delete => "DELETE",
            Resolution::Unload => "UNLOAD",
            Resolution::MuteSevenDays => "MUTE_SEVEN_DAYS",
            Resolution::MuteThreeMonths => "MUTE_THREE_MONTHS",
            Resolution::Tobedone => "TOBEDONE",
        }
    }
}

// ==================== 举报数据获取器 ====================
pub struct WhaleReportFetcher {
    client: &'static CodeMaoClient,
}

impl WhaleReportFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    fn add_timestamp_to_builder(builder: KittyRequestBuilder) -> KittyRequestBuilder {
        let timestamp = current_timestamp_13();
        builder.with_param("TIME", timestamp.to_string())
    }

    fn add_timestamp_to_paginated(paginated: PaginatedIter) -> PaginatedIter {
        let timestamp = current_timestamp_13();
        paginated.with_iter_param("TIME", timestamp.to_string())
    }

    // 作品举报（分页）
    pub fn fetch_work_reports_gen(
        &self,
        source_type: WorkSourceType,
        status: ReportStatus,
        filter_type: Option<WorkReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/reports/works")
            .with_base_key(BaseKey::Whale)
            .with_iter_param("type", source_type.as_str())
            .with_iter_param("status", status.as_str())
            .with_page_size(15)
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_iter_param(filter.as_str(), id.to_string());
        }
        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }
        paginated
    }

    // 评论举报（分页）
    pub fn fetch_comment_reports_gen(
        &self,
        source_type: CommentSourceType,
        status: ReportStatus,
        filter_type: Option<CommentReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/reports/comments/search")
            .with_base_key(BaseKey::Whale)
            .with_iter_param("source", source_type.as_str())
            .with_iter_param("status", status.as_str())
            .with_page_size(15)
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_iter_param(filter.as_str(), id.to_string());
        }
        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }
        paginated
    }

    // 帖子举报（分页）
    pub fn fetch_post_reports_gen(
        &self,
        status: ReportStatus,
        board_id: Option<i32>,
        filter_type: Option<PostReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/reports/posts")
            .with_base_key(BaseKey::Whale)
            .with_iter_param("status", status.as_str())
            .with_page_size(15)
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let Some(board) = board_id {
            paginated = paginated.with_iter_param("board_id", board.to_string());
        }
        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_iter_param(filter.as_str(), id.to_string());
        }
        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }
        paginated
    }

    // 讨论区举报（分页）
    pub fn fetch_discussion_reports_gen(
        &self,
        status: ReportStatus,
        board_id: Option<i32>,
        filter_type: Option<PostReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut paginated = self
            .client
            .paginated("/reports/posts/discussions")
            .with_base_key(BaseKey::Whale)
            .with_iter_param("status", status.as_str())
            .with_page_size(15)
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let Some(board) = board_id {
            paginated = paginated.with_iter_param("board_id", board.to_string());
        }
        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_iter_param(filter.as_str(), id.to_string());
        }
        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }
        paginated
    }
}

impl Default for WhaleReportFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 举报处理器 ====================
pub struct ReportHandler {
    client: &'static CodeMaoClient,
}

impl ReportHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    pub fn execute_process_post_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        let endpoint = format!("/reports/posts/{}", report_id);
        let payload = json!({
            "admin_id": admin_id,
            "status": resolution.as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, Some(BaseKey::Whale))
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    pub fn execute_process_discussion_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        let endpoint = format!("/reports/posts/discussions/{}", report_id);
        let payload = json!({
            "admin_id": admin_id,
            "status": resolution.as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, Some(BaseKey::Whale))
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    pub fn execute_process_comment_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        let endpoint = format!("/reports/comments/{}", report_id);
        let payload = json!({
            "admin_id": admin_id,
            "status": resolution.as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, Some(BaseKey::Whale))
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    pub fn execute_process_work_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        let resolution_str = match resolution {
            Resolution::Pass | Resolution::Delete | Resolution::Unload | Resolution::Tobedone => {
                resolution.as_str()
            }
            _ => return Err(MewError::Other("作品举报不支持此决议类型".into())),
        };

        let endpoint = format!("/reports/works/{}", report_id);
        let payload = json!({
            "admin_id": admin_id,
            "status": resolution_str,
        });

        let response = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, Some(BaseKey::Whale))
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }
}

impl Default for ReportHandler {
    fn default() -> Self {
        Self::new()
    }
}
