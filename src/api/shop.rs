use crate::utils::acquire::{CodeMaoClient, HTTPStatus, HttpMethod, MewResult, PaginatedIter};
use log::debug;
use serde_json::{Value, json};

// ==================== 工作室相关枚举 ====================

/// 内容来源
#[derive(Debug, Clone, Copy)]
pub enum Source {
    WorkShop,
}

impl Source {
    fn as_str(&self) -> &'static str {
        match self {
            Source::WorkShop => "WORK_SHOP",
        }
    }
}

/// 审核状态
#[derive(Debug, Clone, Copy)]
pub enum AuditStatus {
    Unaccepted,
    Accepted,
}

impl AuditStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AuditStatus::Unaccepted => "UNACCEPTED",
            AuditStatus::Accepted => "ACCEPTED",
        }
    }
}

/// 工作室举报原因 ID
#[derive(Debug, Clone, Copy)]
pub enum WorkShopReportReasonId {
    Custom = 0,
    Reason1 = 1,
    Reason2 = 2,
    Reason3 = 3,
    Reason4 = 4,
    Reason5 = 5,
    Reason6 = 6,
    Reason7 = 7,
    Reason8 = 8,
}

// ==================== 工作室数据获取器 ====================

/// 工作室相关数据查询接口。
pub struct WorkshopDataFetcher {
    client: &'static CodeMaoClient,
}

impl WorkshopDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ---------- 私有辅助 ----------

    /// 发送请求并将响应解析为 JSON。
    fn send_and_parse(
        &self,
        builder: crate::utils::acquire::KittyRequestBuilder,
    ) -> MewResult<Value> {
        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // ---------- 公共方法 ----------

    /// 获取工作室简要信息（需登录工作室成员账号）
    pub fn fetch_workshop_info(&self) -> MewResult<Value> {
        debug!("获取工作室简要信息");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/work_shops/simple", None);
        self.send_and_parse(builder)
    }

    /// 获取工作室详细信息
    pub fn fetch_workshop_details(&self, workshop_id: &str) -> MewResult<Value> {
        debug!("获取工作室详情: workshop_id={}", workshop_id);
        let endpoint = format!("/web/shops/{}", workshop_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 搜索工作室列表
    pub fn fetch_workshops(
        &self,
        level: Option<i32>,
        limit: Option<i32>,
        works_limit: Option<i32>,
        offset: Option<i32>,
        sort: Option<Vec<String>>,
    ) -> MewResult<Value> {
        debug!(
            "获取工作室列表: level={:?}, limit={:?}, offset={:?}",
            level, limit, offset
        );
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/work-shops/search", None)
            .with_param("level", level.unwrap_or(4).to_string())
            .with_param("works_limit", works_limit.unwrap_or(4).to_string())
            .with_param("limit", limit.unwrap_or(14).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param(
                "sort",
                sort.map(|v| v.join(","))
                    .unwrap_or_else(|| "-created_at,-latest_joined_at".to_string()),
            );
        self.send_and_parse(builder)
    }

    /// 工作室成员列表分页迭代器
    pub fn fetch_workshop_members_gen(
        &self,
        workshop_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/shops/{}/users", workshop_id);
        debug!("获取工作室成员迭代器: workshop_id={}", workshop_id);
        self.client
            .paginated(&endpoint)
            .with_page_size(40)
            .with_total_key("total")
            .with_limit(limit.unwrap_or(40))
    }

    /// 获取工作室详情列表（含成员和作品）
    pub fn fetch_workshop_details_list(
        &self,
        levels: Option<Vec<i32>>,
        max_number: Option<i32>,
        works_limit: Option<i32>,
        sort: Option<Vec<String>>,
    ) -> MewResult<Value> {
        debug!("获取工作室详情列表");
        let levels_str = levels
            .map(|v| {
                v.iter()
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "1,2,3,4".to_string());

        let sort_str = sort
            .map(|v| v.join(","))
            .unwrap_or_else(|| "-ordinal,-updated_at".to_string());

        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/shops", None)
            .with_param("levels", levels_str)
            .with_param("max_number", max_number.unwrap_or(4).to_string())
            .with_param("works_limit", works_limit.unwrap_or(4).to_string())
            .with_param("sort", sort_str);
        self.send_and_parse(builder)
    }

    /// 工作室讨论分页迭代器
    pub fn fetch_workshop_discussions_gen(
        &self,
        shop_id: i32,
        source: Option<Source>,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/discussions/{}/comments", shop_id);
        debug!("获取工作室讨论迭代器: shop_id={}", shop_id);
        self.client
            .paginated(&endpoint)
            .with_iter_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .with_iter_param("sort", sort.unwrap_or_else(|| "-created_at".to_string()))
            .with_page_size(20)
            .with_limit(limit.unwrap_or(15))
    }

    /// 工作室投稿作品分页迭代器
    pub fn fetch_workshop_works_gen(
        &self,
        workshop_id: i32,
        user_id: i32,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/works/subjects/{}/works", workshop_id);
        debug!(
            "获取工作室作品迭代器: workshop_id={}, user_id={}",
            workshop_id, user_id
        );
        self.client
            .paginated(&endpoint)
            .with_page_size(20)
            .with_iter_param(
                "sort",
                sort.unwrap_or_else(|| "-created_at,-id".to_string()),
            )
            .with_iter_param("user_id", user_id.to_string())
            .with_iter_param("work_subject_id", workshop_id.to_string())
            .with_limit(limit.unwrap_or(20))
    }

    /// 获取与工作室的关系
    pub fn fetch_workshop_relation(&self, relation_id: i32) -> MewResult<Value> {
        debug!("获取工作室关系: relation_id={}", relation_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/work_shops/users/relation", None)
            .with_param("id", relation_id.to_string());
        self.send_and_parse(builder)
    }

    /// 工作室讨论区帖子分页迭代器
    pub fn fetch_workshop_posts_gen(&self, label_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/works/subjects/labels/{}/posts", label_id);
        debug!("获取工作室帖子迭代器: label_id={}", label_id);
        self.client
            .paginated(&endpoint)
            .with_page_size(20)
            .with_limit(limit.unwrap_or(20))
    }

    /// 获取工作室待审核成员列表
    pub fn fetch_workshop_unaudited_member(
        &self,
        workshop_id: i32,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "获取待审核成员: workshop_id={}, limit={:?}, offset={:?}",
            workshop_id, limit, offset
        );
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "https://api.codemao.cn/web/work_shops/users/unaudited/list",
                None,
            )
            .with_param("limit", limit.unwrap_or(40).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("id", workshop_id.to_string());
        self.send_and_parse(builder)
    }
}

impl Default for WorkshopDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 工作室操作处理器 ====================

/// 举报讨论区评论的参数。
pub struct ReportCommentArgs<'a> {
    pub comment_id: i32,
    pub reason_content: &'a str,
    pub reason_id: WorkShopReportReasonId,
    pub reporter_id: i32,
    pub comment_source: Option<Source>,
    pub comment_parent_id: Option<i32>,
    pub description: Option<&'a str>,
}

/// 工作室相关操作接口（创建、投稿、评论、审核等）。
pub struct WorkshopActionHandler {
    client: &'static CodeMaoClient,
}

impl WorkshopActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ---------- 私有辅助 ----------

    /// 发送请求并返回 status == 预期状态码。
    fn check_status(
        &self,
        builder: crate::utils::acquire::KittyRequestBuilder,
        expected: HTTPStatus,
    ) -> MewResult<bool> {
        let response = builder.send()?;
        Ok(response.status() == expected as u16)
    }

    /// 发送请求并根据 `return_data` 决定返回 JSON 数据或成功标志。
    fn send_maybe_parse(
        &self,
        builder: crate::utils::acquire::KittyRequestBuilder,
        return_data: bool,
        expected: HTTPStatus,
    ) -> MewResult<Value> {
        let response = builder.send()?;
        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == expected as u16 }))
        }
    }

    // ---------- 公共方法 ----------

    /// 更新工作室简介
    pub fn update_workshop_details(
        &self,
        description: &str,
        workshop_id: &str,
        name: &str,
        preview_url: &str,
    ) -> MewResult<bool> {
        debug!("更新工作室详情: workshop_id={}, name={}", workshop_id, name);
        let payload = json!({
            "description": description,
            "id": workshop_id,
            "name": name,
            "preview_url": preview_url,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/update", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 创建工作室
    pub fn create_workshop(
        &self,
        name: &str,
        description: &str,
        preview_url: &str,
    ) -> MewResult<Value> {
        debug!("创建工作室: name={}", name);
        let payload = json!({
            "name": name,
            "description": description,
            "preview_url": preview_url,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/create", None)
            .with_payload(payload);
        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    /// 解散工作室
    pub fn delete_workshop(&self, workshop_id: i32) -> MewResult<bool> {
        debug!("解散工作室: workshop_id={}", workshop_id);
        let payload = json!({ "id": workshop_id });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/dissolve", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 向工作室投稿作品
    pub fn create_work_contribution(&self, workshop_id: i32, work_id: i32) -> MewResult<bool> {
        debug!("投稿作品: workshop_id={}, work_id={}", workshop_id, work_id);
        let payload = json!({
            "id": workshop_id,
            "work_id": work_id,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/works/contribute", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 从工作室移除作品
    pub fn delete_workshop_work(&self, workshop_id: i32, work_id: i32) -> MewResult<bool> {
        debug!("移除作品: workshop_id={}, work_id={}", workshop_id, work_id);
        let payload = json!({
            "id": workshop_id,
            "work_id": work_id,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/works/remove", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 申请加入工作室
    pub fn execute_apply_to_join(&self, workshop_id: i32, qq: Option<&str>) -> MewResult<bool> {
        debug!("申请加入工作室: workshop_id={}", workshop_id);
        let payload = json!({
            "id": workshop_id,
            "qq": qq,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/users/apply/join", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 审核加入工作室的申请
    pub fn execute_review_join_application(
        &self,
        workshop_id: i32,
        status: AuditStatus,
        user_id: i32,
    ) -> MewResult<bool> {
        debug!(
            "审核加入申请: workshop_id={}, user_id={}, status={:?}",
            workshop_id, user_id, status
        );
        let payload = json!({
            "id": workshop_id,
            "status": status.as_str(),
            "user_id": user_id,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/work_shops/users/audit", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 举报讨论区下的评论
    pub fn execute_report_comment(&self, args: ReportCommentArgs<'_>) -> MewResult<bool> {
        debug!(
            "举报评论: comment_id={}, reason_id={:?}",
            args.comment_id, args.reason_id
        );
        let payload = json!({
            "comment_id": args.comment_id,
            "comment_parent_id": args.comment_parent_id.unwrap_or(0),
            "description": args.description.unwrap_or(""),
            "reason_content": args.reason_content,
            "reason_id": (args.reason_id as i32).to_string(),
            "reporter_id": args.reporter_id,
            "comment_source": args.comment_source.unwrap_or(Source::WorkShop).as_str(),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/reports/comments", None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Created)
    }

    /// 回复评论
    pub fn create_comment_reply(
        &self,
        workshop_id: i32,
        comment_id: i32,
        content: &str,
        source: Option<Source>,
        parent_id: Option<i32>,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!(
            "回复评论: workshop_id={}, comment_id={}",
            workshop_id, comment_id
        );
        let endpoint = format!(
            "/web/discussions/{}/comments/{}/reply",
            workshop_id, comment_id
        );
        let payload = json!({
            "parent_id": parent_id.unwrap_or(0),
            "content": content,
            "source": source.unwrap_or(Source::WorkShop).as_str(),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }

    /// 删除回复
    pub fn delete_reply(&self, comment_id: i32, source: Option<Source>) -> MewResult<bool> {
        debug!("删除回复: comment_id={}", comment_id);
        let endpoint = format!("/web/discussions/replies/{}", comment_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str());
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 发表评论
    pub fn create_comment(
        &self,
        workshop_id: i32,
        content: &str,
        rich_content: &str,
        source: Option<Source>,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("发表评论: workshop_id={}", workshop_id);
        let endpoint = format!("/web/discussions/{}/comment", workshop_id);
        let payload = json!({
            "content": content,
            "rich_content": rich_content,
            "source": source.unwrap_or(Source::WorkShop).as_str(),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }

    /// 删除评论
    pub fn delete_comment(&self, comment_id: i32, source: Option<Source>) -> MewResult<bool> {
        debug!("删除评论: comment_id={}", comment_id);
        let endpoint = format!("/web/discussions/comments/{}", comment_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str());
        self.check_status(builder, HTTPStatus::NoContent)
    }
}

impl Default for WorkshopActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
