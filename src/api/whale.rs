use crate::utils::acquire::{
    BaseKey, ClientAccess, CodeMaoClient, HTTPStatus, HttpMethod, MewError, MewResult,
    PaginatedIter, PaginationMethod, current_timestamp_13,
};
use log::debug;
use serde_json::json;

// 举报相关枚举

/// 作品来源类型
#[derive(Debug, Clone, Copy)]
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

/// 评论来源类型
#[derive(Debug, Clone, Copy)]
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

/// 举报状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportStatus {
    ToBeDone,
    Done,
    All,
}

impl ReportStatus {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ReportStatus::ToBeDone => "TOBEDONE",
            ReportStatus::Done => "DONE",
            ReportStatus::All => "ALL",
        }
    }
}

/// 作品举报过滤类型
#[derive(Debug, Clone, Copy)]
pub enum WorkReportFilterType {
    Admin,
    WorkUser,
    Work,
}

impl WorkReportFilterType {
    fn as_str(&self) -> &'static str {
        match self {
            WorkReportFilterType::Admin => "admin_id",
            WorkReportFilterType::WorkUser => "work_user_id",
            WorkReportFilterType::Work => "work_id",
        }
    }
}

/// 评论举报过滤类型
#[derive(Debug, Clone, Copy)]
pub enum CommentReportFilterType {
    Admin,
    CommentUser,
    Comment,
}

impl CommentReportFilterType {
    fn as_str(&self) -> &'static str {
        match self {
            CommentReportFilterType::Admin => "admin_id",
            CommentReportFilterType::CommentUser => "comment_user_id",
            CommentReportFilterType::Comment => "comment_id",
        }
    }
}

/// 帖子举报过滤类型
#[derive(Debug, Clone, Copy)]
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

/// 处理决议
#[derive(Debug, Clone, Copy)]
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

// 举报数据获取器

/// 管理员举报数据查询接口
pub struct WhaleReportFetcher {
    client: &'static CodeMaoClient,
}

impl WhaleReportFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 私有辅助

    /// 为分页迭代器附加当前时间戳参数 `TIME`
    fn add_timestamp_to_paginated(paginated: PaginatedIter) -> PaginatedIter {
        let timestamp = current_timestamp_13();
        paginated.with_iter_param("TIME", timestamp.to_string())
    }

    /// 构建基础举报分页迭代器
    fn build_report_paginated(&self, endpoint: &str, default_limit: usize) -> PaginatedIter {
        self.client
            .build_paginated(endpoint)
            .with_base_key(BaseKey::Whale)
            .with_page_size(15)
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit")
            .with_limit(default_limit)
    }

    /// 为分页迭代器添加可选的过滤参数
    fn apply_optional_filter(
        paginated: PaginatedIter,
        filter_type: Option<impl AsRef<str>>,
        target_id: Option<i32>,
    ) -> PaginatedIter {
        match (filter_type, target_id) {
            (Some(filter), Some(id)) => paginated.with_iter_param(filter.as_ref(), id.to_string()),
            _ => paginated,
        }
    }

    // 公共方法

    /// 作品举报分页迭代器
    pub fn fetch_work_reports_gen(
        &self,
        source_type: WorkSourceType,
        status: ReportStatus,
        filter_type: Option<WorkReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取作品举报: source={:?}, status={:?}",
            source_type, status
        );
        let mut paginated = self
            .build_report_paginated("/reports/works", limit.unwrap_or(15))
            .with_iter_param("type", source_type.as_str())
            .with_iter_param("status", status.as_str());

        paginated =
            Self::apply_optional_filter(paginated, filter_type.map(|f| f.as_str()), target_id);
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 评论举报分页迭代器
    pub fn fetch_comment_reports_gen(
        &self,
        source_type: CommentSourceType,
        status: ReportStatus,
        filter_type: Option<CommentReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取评论举报: source={:?}, status={:?}",
            source_type, status
        );
        let mut paginated = self
            .build_report_paginated("/reports/comments/search", limit.unwrap_or(15))
            .with_iter_param("source", source_type.as_str())
            .with_iter_param("status", status.as_str());

        paginated =
            Self::apply_optional_filter(paginated, filter_type.map(|f| f.as_str()), target_id);
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 帖子举报分页迭代器
    pub fn fetch_post_reports_gen(
        &self,
        status: ReportStatus,
        board_id: Option<i32>,
        filter_type: Option<PostReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取帖子举报: status={:?}, board_id={:?}", status, board_id);
        let mut paginated = self
            .build_report_paginated("/reports/posts", limit.unwrap_or(15))
            .with_iter_param("status", status.as_str());

        if let Some(board) = board_id {
            paginated = paginated.with_iter_param("board_id", board.to_string());
        }

        paginated =
            Self::apply_optional_filter(paginated, filter_type.map(|f| f.as_str()), target_id);
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }

    /// 讨论区举报分页迭代器
    pub fn fetch_discussion_reports_gen(
        &self,
        status: ReportStatus,
        board_id: Option<i32>,
        filter_type: Option<PostReportFilterType>,
        target_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取讨论区举报: status={:?}, board_id={:?}",
            status, board_id
        );
        let mut paginated = self
            .build_report_paginated("/reports/posts/discussions", limit.unwrap_or(15))
            .with_iter_param("status", status.as_str());

        if let Some(board) = board_id {
            paginated = paginated.with_iter_param("board_id", board.to_string());
        }

        paginated =
            Self::apply_optional_filter(paginated, filter_type.map(|f| f.as_str()), target_id);
        paginated = Self::add_timestamp_to_paginated(paginated);
        paginated
    }
}

impl Default for WhaleReportFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// 举报处理器

/// 举报处理接口(处理作品,评论,帖子,讨论区举报)
pub struct ReportHandler {
    client: &'static CodeMaoClient,
}

impl ReportHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 私有辅助

    /// 发送 PATCH 请求处理举报,并检查状态码是否为 204
    fn process_report(&self, endpoint: &str, admin_id: i32, resolution: &str) -> MewResult<bool> {
        let payload = json!({
            "admin_id": admin_id,
            "status": resolution,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Patch, endpoint, Some(BaseKey::Whale))
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    // 公共方法

    /// 处理帖子举报
    pub fn execute_process_post_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        debug!(
            "处理帖子举报: report_id={}, resolution={:?}",
            report_id, resolution
        );
        let endpoint = format!("/reports/posts/{}", report_id);
        self.process_report(&endpoint, admin_id, resolution.as_str())
    }

    /// 处理讨论区举报
    pub fn execute_process_discussion_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        debug!(
            "处理讨论区举报: report_id={}, resolution={:?}",
            report_id, resolution
        );
        let endpoint = format!("/reports/posts/discussions/{}", report_id);
        self.process_report(&endpoint, admin_id, resolution.as_str())
    }

    /// 处理评论举报
    pub fn execute_process_comment_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        debug!(
            "处理评论举报: report_id={}, resolution={:?}",
            report_id, resolution
        );
        let endpoint = format!("/reports/comments/{}", report_id);
        self.process_report(&endpoint, admin_id, resolution.as_str())
    }

    /// 处理作品举报(仅支持 Pass/Delete/Unload/Tobedone)
    pub fn execute_process_work_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> MewResult<bool> {
        debug!(
            "处理作品举报: report_id={}, resolution={:?}",
            report_id, resolution
        );
        // 作品举报仅支持部分决议类型
        let resolution_str = match resolution {
            Resolution::Pass | Resolution::Delete | Resolution::Unload | Resolution::Tobedone => {
                resolution.as_str()
            }
            _ => return Err(MewError::Other("作品举报不支持此决议类型".into())),
        };

        let endpoint = format!("/reports/works/{}", report_id);
        self.process_report(&endpoint, admin_id, resolution_str)
    }
}

impl Default for ReportHandler {
    fn default() -> Self {
        Self::new()
    }
}

// 共享请求辅助(ClientAccess)

impl ClientAccess for WhaleReportFetcher {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for ReportHandler {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}
