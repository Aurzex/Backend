use crate::utils::acquire::{
    BaseKey, CodeMaoClient, HTTPStatus, HttpMethod, PaginatedIter, PaginationMethod,
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
pub enum ReportStatus {
    Tobedone,
    Done,
    All,
}

impl ReportStatus {
    fn as_str(&self) -> &'static str {
        match self {
            ReportStatus::Tobedone => "TOBEDONE",
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
pub struct ReportFetcher {
    client: &'static CodeMaoClient,
}

impl ReportFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    fn add_timestamp_to_builder(
        builder: crate::utils::acquire::InnerBuilder,
    ) -> crate::utils::acquire::InnerBuilder {
        let timestamp = current_timestamp_13();
        builder.with_param("TIME", timestamp.to_string())
    }

    fn add_timestamp_to_paginated(paginated: PaginatedIter) -> PaginatedIter {
        let timestamp = current_timestamp_13();
        paginated.with_param("TIME", timestamp.to_string())
    }

    // 获取作品举报列表生成器
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
            .paginated("https://whale.codemao.cn/reports/works/search")
            .with_param("type", source_type.as_str())
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_param(filter.as_str(), id.to_string());
        }

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取作品举报总数
    pub fn fetch_work_reports_total(
        &self,
        source_type: WorkSourceType,
        status: ReportStatus,
        filter_type: Option<WorkReportFilterType>,
        target_id: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://whale.codemao.cn/reports/works/search",
                None,
            )
            .with_param("type", source_type.as_str())
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15");

        builder = Self::add_timestamp_to_builder(builder);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            builder = builder.with_param(filter.as_str(), id.to_string());
        }

        let response = builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取作品举报额外总数
    pub fn fetch_work_reports_total_extra(
        &self,
        source_type: WorkSourceType,
        status: ReportStatus,
        filter_type: Option<WorkReportFilterType>,
        target_id: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://whale.codemao.cn/reports/works",
                None,
            )
            .with_param("type", source_type.as_str())
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15");

        builder = Self::add_timestamp_to_builder(builder);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            builder = builder.with_param(filter.as_str(), id.to_string());
        }

        let response = builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取评论举报列表生成器
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
            .paginated("https://whale.codemao.cn/reports/comments/search")
            .with_param("source", source_type.as_str())
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_param(filter.as_str(), id.to_string());
        }

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取评论举报总数
    pub fn fetch_comment_reports_total(
        &self,
        source_type: CommentSourceType,
        status: ReportStatus,
        filter_type: Option<CommentReportFilterType>,
        target_id: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://whale.codemao.cn/reports/comments/search",
                None,
            )
            .with_param("source", source_type.as_str())
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15");

        builder = Self::add_timestamp_to_builder(builder);

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            builder = builder.with_param(filter.as_str(), id.to_string());
        }

        let response = builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取帖子举报列表生成器
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
            .paginated("https://whale.codemao.cn/reports/posts")
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let Some(board) = board_id {
            paginated = paginated.with_param("board_id", board.to_string());
        }

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_param(filter.as_str(), id.to_string());
        }

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取帖子举报总数
    pub fn fetch_post_reports_total(
        &self,
        status: ReportStatus,
        board_id: Option<i32>,
        filter_type: Option<PostReportFilterType>,
        target_id: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://whale.codemao.cn/reports/posts",
                None,
            )
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15");

        builder = Self::add_timestamp_to_builder(builder);

        if let Some(board) = board_id {
            builder = builder.with_param("board_id", board.to_string());
        }

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            builder = builder.with_param(filter.as_str(), id.to_string());
        }

        let response = builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取讨论区举报列表生成器
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
            .paginated("https://whale.codemao.cn/reports/posts/discussions")
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15")
            .with_pagination_method(PaginationMethod::Offset)
            .with_offset_key("offset")
            .with_amount_key("limit");

        paginated = Self::add_timestamp_to_paginated(paginated);

        if let Some(board) = board_id {
            paginated = paginated.with_param("board_id", board.to_string());
        }

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            paginated = paginated.with_param(filter.as_str(), id.to_string());
        }

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取讨论区举报总数
    pub fn fetch_discussion_reports_total(
        &self,
        status: ReportStatus,
        board_id: Option<i32>,
        filter_type: Option<PostReportFilterType>,
        target_id: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://whale.codemao.cn/reports/posts/discussions",
                None,
            )
            .with_param("status", status.as_str())
            .with_param("offset", "0")
            .with_param("limit", "15");

        builder = Self::add_timestamp_to_builder(builder);

        if let Some(board) = board_id {
            builder = builder.with_param("board_id", board.to_string());
        }

        if let (Some(filter), Some(id)) = (filter_type, target_id) {
            builder = builder.with_param(filter.as_str(), id.to_string());
        }

        let response = builder.send()?;
        Ok(self.client.response_to_json(response)?)
    }
}

impl Default for ReportFetcher {
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

    // 处理帖子举报
    pub fn execute_process_post_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("https://whale.codemao.cn/reports/posts/{}", report_id);

        let payload = json!({
            "admin_id": admin_id,
            "status": resolution.as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 处理讨论区举报
    pub fn execute_process_discussion_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!(
            "https://whale.codemao.cn/reports/posts/discussions/{}",
            report_id
        );

        let payload = json!({
            "admin_id": admin_id,
            "status": resolution.as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 处理评论举报
    pub fn execute_process_comment_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("https://whale.codemao.cn/reports/comments/{}", report_id);

        let payload = json!({
            "admin_id": admin_id,
            "status": resolution.as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 处理作品举报
    pub fn execute_process_work_report(
        &self,
        report_id: i32,
        admin_id: i32,
        resolution: Resolution,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // 作品举报只有特定的决议类型
        let resolution_str = match resolution {
            Resolution::Pass | Resolution::Delete | Resolution::Unload | Resolution::Tobedone => {
                resolution.as_str()
            }
            _ => return Err("作品举报不支持此决议类型".into()),
        };

        let endpoint = format!("https://whale.codemao.cn/reports/works/{}", report_id);

        let payload = json!({
            "admin_id": admin_id,
            "status": resolution_str,
        });

        let response = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, None)
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
