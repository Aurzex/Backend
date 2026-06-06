use crate::utils::acquire::{
    CodeMaoClient, HTTPStatus, HttpMethod, MewError, MewResult, PaginatedIter,
};
use serde_json::{Value, json};

// ==================== 工作室相关枚举 ====================

// 来源枚举
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

// 审核状态枚举
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

// 举报原因ID枚举
#[repr(i32)]
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
pub struct WorkshopDataFetcher {
    client: &'static CodeMaoClient,
}

impl WorkshopDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取工作室简介 (简易, 需登录工作室成员账号)
    pub fn fetch_workshop_info(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/work_shops/simple", None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取工作室详情
    pub fn fetch_workshop_details(&self, workshop_id: &str) -> MewResult<Value> {
        let endpoint = format!("/web/shops/{}", workshop_id);
        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取工作室列表
    pub fn fetch_workshops(
        &self,
        level: Option<i32>,
        limit: Option<i32>,
        works_limit: Option<i32>,
        offset: Option<i32>,
        sort: Option<Vec<String>>,
    ) -> MewResult<Value> {
        let mut builder = self
            .client
            .build_request(HttpMethod::GET, "/web/work-shops/search", None)
            .with_param("level", level.unwrap_or(4).to_string())
            .with_param("works_limit", works_limit.unwrap_or(4).to_string())
            .with_param("limit", limit.unwrap_or(14).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string());

        if let Some(sort_vec) = sort {
            builder = builder.with_param("sort", sort_vec.join(","));
        } else {
            builder = builder.with_param("sort", "-created_at,-latest_joined_at");
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // 获取工作室成员生成器
    pub fn fetch_workshop_members_gen(
        &self,
        workshop_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/shops/{}/users", workshop_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_param("limit", "40")
            .with_param("offset", "0")
            .with_total_key("total");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(40);
        }

        paginated
    }

    // 获取工作室详情列表, 包括成员和作品
    pub fn fetch_workshop_details_list(
        &self,
        levels: Option<Vec<i32>>,
        max_number: Option<i32>,
        works_limit: Option<i32>,
        sort: Option<Vec<String>>,
    ) -> MewResult<Value> {
        let mut builder = self
            .client
            .build_request(HttpMethod::GET, "/web/shops", None);

        if let Some(levels_vec) = levels {
            let levels_str: Vec<String> = levels_vec.iter().map(|l| l.to_string()).collect();
            builder = builder.with_param("levels", levels_str.join(","));
        } else {
            builder = builder.with_param("levels", "1,2,3,4");
        }

        builder = builder
            .with_param("max_number", max_number.unwrap_or(4).to_string())
            .with_param("works_limit", works_limit.unwrap_or(4).to_string());

        if let Some(sort_vec) = sort {
            builder = builder.with_param("sort", sort_vec.join(","));
        } else {
            builder = builder.with_param("sort", "-ordinal,-updated_at");
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // 获取工作室讨论生成器
    pub fn fetch_workshop_discussions_gen(
        &self,
        shop_id: i32,
        source: Option<Source>,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/discussions/{}/comments", shop_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .with_param("sort", sort.unwrap_or_else(|| "-created_at".to_string()))
            .with_param("limit", "20")
            .with_param("offset", "0");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取工作室投稿作品生成器
    pub fn fetch_workshop_works_gen(
        &self,
        workshop_id: i32,
        user_id: i32,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/works/subjects/{}/works", workshop_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_param("limit", "20")
            .with_param("offset", "0")
            .with_param(
                "sort",
                sort.unwrap_or_else(|| "-created_at,-id".to_string()),
            )
            .with_param("user_id", user_id.to_string())
            .with_param("work_subject_id", workshop_id.to_string());

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(20);
        }

        paginated
    }

    // 获取与工作室关系
    pub fn fetch_workshop_relation(&self, relation_id: i32) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/web/work_shops/users/relation", None)
            .with_param("id", relation_id.to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取工作室讨论区的帖子生成器
    pub fn fetch_workshop_posts_gen(&self, label_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/works/subjects/labels/{}/posts", label_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_param("limit", "20")
            .with_param("offset", "0");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(20);
        }

        paginated
    }

    // 获取工作室待审核成员
    pub fn fetch_workshop_unaudited_member(
        &self,
        workshop_id: i32,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://api.codemao.cn/web/work_shops/users/unaudited/list",
                None,
            )
            .with_param("limit", limit.unwrap_or(40).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("id", workshop_id.to_string())
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for WorkshopDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 工作室操作处理器 ====================
pub struct WorkshopActionHandler {
    client: &'static CodeMaoClient,
}

impl WorkshopActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 更新工作室简介
    pub fn update_workshop_details(
        &self,
        description: &str,
        workshop_id: &str,
        name: &str,
        preview_url: &str,
    ) -> MewResult<bool> {
        let payload = json!({
            "description": description,
            "id": workshop_id,
            "name": name,
            "preview_url": preview_url,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/update", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 创建工作室
    pub fn create_workshop(
        &self,
        name: &str,
        description: &str,
        preview_url: &str,
    ) -> MewResult<Value> {
        let payload = json!({
            "name": name,
            "description": description,
            "preview_url": preview_url,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/create", None)
            .with_payload(payload)
            .send()?;

        self.client.response_to_json(response)
    }

    // 解散工作室
    pub fn delete_workshop(&self, workshop_id: i32) -> MewResult<bool> {
        let payload = json!({
            "id": workshop_id,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/dissolve", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 在指定工作室投稿作品
    pub fn create_work_contribution(&self, workshop_id: i32, work_id: i32) -> MewResult<bool> {
        let payload = json!({
            "id": workshop_id,
            "work_id": work_id,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/works/contribute", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 在指定工作室删除作品
    pub fn delete_workshop_work(&self, workshop_id: i32, work_id: i32) -> MewResult<bool> {
        let payload = json!({
            "id": workshop_id,
            "work_id": work_id,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/works/remove", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 申请加入工作室
    pub fn execute_apply_to_join(&self, workshop_id: i32, qq: Option<&str>) -> MewResult<bool> {
        let mut payload_map = serde_json::Map::new();
        payload_map.insert("id".to_string(), Value::Number(workshop_id.into()));

        if let Some(qq_val) = qq {
            payload_map.insert("qq".to_string(), Value::String(qq_val.to_string()));
        } else {
            payload_map.insert("qq".to_string(), Value::Null);
        }

        let payload = Value::Object(payload_map);

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/users/apply/join", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 审核已经申请加入工作室的用户
    pub fn execute_review_join_application(
        &self,
        workshop_id: i32,
        status: AuditStatus,
        user_id: i32,
    ) -> MewResult<bool> {
        let payload = json!({
            "id": workshop_id,
            "status": status.as_str(),
            "user_id": user_id,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/users/audit", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 举报讨论区下的评论
    pub fn execute_report_comment(
        &self,
        comment_id: i32,
        reason_content: &str,
        reason_id: WorkShopReportReasonId,
        reporter_id: i32,
        comment_source: Option<Source>,
        comment_parent_id: Option<i32>,
        description: Option<&str>,
    ) -> MewResult<bool> {
        let payload = json!({
            "comment_id": comment_id,
            "comment_parent_id": comment_parent_id.unwrap_or(0),
            "description": description.unwrap_or(""),
            "reason_content": reason_content,
            "reason_id": (reason_id as i32).to_string(),
            "reporter_id": reporter_id,
            "comment_source": comment_source.unwrap_or(Source::WorkShop).as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/reports/comments", None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Created as u16)
    }

    // 回复评论
    pub fn create_comment_reply(
        &self,
        workshop_id: i32,
        comment_id: i32,
        content: &str,
        source: Option<Source>,
        parent_id: Option<i32>,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!(
            "/web/discussions/{}/comments/{}/reply",
            workshop_id, comment_id
        );

        let payload = json!({
            "parent_id": parent_id.unwrap_or(0),
            "content": content,
            "source": source.unwrap_or(Source::WorkShop).as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(payload)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Created as u16 }))
        }
    }

    // 删除回复
    pub fn delete_reply(&self, comment_id: i32, source: Option<Source>) -> MewResult<bool> {
        let endpoint = format!("/web/discussions/replies/{}", comment_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 评论
    pub fn create_comment(
        &self,
        workshop_id: i32,
        content: &str,
        rich_content: &str,
        source: Option<Source>,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/discussions/{}/comment", workshop_id);

        let payload = json!({
            "content": content,
            "rich_content": rich_content,
            "source": source.unwrap_or(Source::WorkShop).as_str(),
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(payload)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Created as u16 }))
        }
    }

    // 删除评论
    pub fn delete_comment(&self, comment_id: i32, source: Option<Source>) -> MewResult<bool> {
        let endpoint = format!("/web/discussions/comments/{}", comment_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }
}

impl Default for WorkshopActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
