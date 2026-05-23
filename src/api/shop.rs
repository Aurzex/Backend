use crate::utils::acquire::{CodeMaoClient, HttpMethod, MewError, MewResult, PaginatedIter};
use serde_json::{Value, json};

// ==================== 工作室相关枚举 ====================

pub enum Source {
    WorkShop,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::WorkShop => "WORK_SHOP",
        }
    }
}

pub enum AuditStatus {
    Unaccepted,
    Accepted,
}

impl AuditStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditStatus::Unaccepted => "UNACCEPTED",
            AuditStatus::Accepted => "ACCEPTED",
        }
    }
}

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

// ==================== WorkshopDataFetcher ====================

pub struct WorkshopDataFetcher {
    client: &'static CodeMaoClient,
}

impl WorkshopDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取工作室简介 (简易, 需登录工作室成员账号)
    pub async fn fetch_workshop_info(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/work_shops/simple", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取工作室详情
    pub async fn fetch_workshop_details(&self, workshop_id: &str) -> MewResult<Value> {
        let endpoint = format!("/web/shops/{}", workshop_id);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取工作室列表
    pub async fn fetch_workshops(
        &self,
        level: Option<i32>,
        limit: Option<i32>,
        works_limit: Option<i32>,
        offset: Option<i32>,
        sort: Option<Vec<String>>,
    ) -> MewResult<Value> {
        let sort_value = sort
            .map(|s| s.join(","))
            .unwrap_or_else(|| "-created_at,-latest_joined_at".to_string());

        self.client
            .build_request(HttpMethod::GET, "/web/work-shops/search", None)
            .with_param("level", level.unwrap_or(4).to_string())
            .with_param("works_limit", works_limit.unwrap_or(4).to_string())
            .with_param("limit", limit.unwrap_or(14).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("sort", sort_value)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取工作室成员生成器
    pub fn fetch_workshop_members_gen(
        &self,
        workshop_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/shops/{}/users", workshop_id);

        self.client
            .paginated(&endpoint)
            .with_param("limit", "40")
            .with_param("offset", "0")
            .with_total_key("total")
            .with_limit(limit.unwrap_or(40))
    }

    /// 获取工作室详情列表, 包括成员和作品
    pub async fn fetch_workshop_details_list(
        &self,
        levels: Option<Vec<i32>>,
        max_number: Option<i32>,
        works_limit: Option<i32>,
        sort: Option<Vec<String>>,
    ) -> MewResult<Value> {
        let levels_str = levels
            .map(|l| {
                l.iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_else(|| "1,2,3,4".to_string());

        let sort_value = sort
            .map(|s| s.join(","))
            .unwrap_or_else(|| "-ordinal,-updated_at".to_string());

        self.client
            .build_request(HttpMethod::GET, "/web/shops", None)
            .with_param("levels", levels_str)
            .with_param("max_number", max_number.unwrap_or(4).to_string())
            .with_param("works_limit", works_limit.unwrap_or(4).to_string())
            .with_param("sort", sort_value)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取工作室讨论生成器
    pub fn fetch_workshop_discussions_gen(
        &self,
        shop_id: i32,
        source: Option<Source>,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/discussions/{}/comments", shop_id);

        self.client
            .paginated(&endpoint)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .with_param("sort", sort.unwrap_or_else(|| "-created_at".to_string()))
            .with_param("limit", "20")
            .with_param("offset", "0")
            .with_limit(limit.unwrap_or(15))
    }

    /// 获取工作室投稿作品生成器
    pub fn fetch_workshop_works_gen(
        &self,
        workshop_id: i32,
        user_id: i32,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/works/subjects/{}/works", workshop_id);

        self.client
            .paginated(&endpoint)
            .with_param("limit", "20")
            .with_param("offset", "0")
            .with_param(
                "sort",
                sort.unwrap_or_else(|| "-created_at,-id".to_string()),
            )
            .with_param("user_id", user_id.to_string())
            .with_param("work_subject_id", workshop_id.to_string())
            .with_limit(limit.unwrap_or(20))
    }

    /// 获取与工作室关系
    pub async fn fetch_workshop_relation(&self, relation_id: i32) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/work_shops/users/relation", None)
            .with_param("id", relation_id.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取工作室讨论区的帖子生成器
    pub fn fetch_workshop_posts_gen(&self, label_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/works/subjects/labels/{}/posts", label_id);

        self.client
            .paginated(&endpoint)
            .with_param("limit", "20")
            .with_param("offset", "0")
            .with_limit(limit.unwrap_or(20))
    }

    /// 获取工作室待审核成员
    pub async fn fetch_workshop_unaudited_member(
        &self,
        workshop_id: i32,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        self.client
            .build_request(
                HttpMethod::GET,
                "https://api.codemao.cn/web/work_shops/users/unaudited/list",
                None,
            )
            .with_param("limit", limit.unwrap_or(40).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("id", workshop_id.to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }
}

impl Default for WorkshopDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== WorkshopActionHandler ====================

pub struct WorkshopActionHandler {
    client: &'static CodeMaoClient,
}

impl WorkshopActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 更新工作室简介
    pub async fn update_workshop_details(
        &self,
        description: &str,
        workshop_id: &str,
        name: &str,
        preview_url: &str,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/update", None)
            .with_payload(json!({
                "description": description,
                "id": workshop_id,
                "name": name,
                "preview_url": preview_url,
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 创建工作室
    pub async fn create_workshop(
        &self,
        name: &str,
        description: &str,
        preview_url: &str,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::POST, "/web/work_shops/create", None)
            .with_payload(json!({
                "name": name,
                "description": description,
                "preview_url": preview_url,
            }))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 解散工作室
    pub async fn delete_workshop(&self, workshop_id: i32) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/dissolve", None)
            .with_payload(json!({ "id": workshop_id }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 在指定工作室投稿作品
    pub async fn create_work_contribution(
        &self,
        workshop_id: i32,
        work_id: i32,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/works/contribute", None)
            .with_payload(json!({
                "id": workshop_id,
                "work_id": work_id,
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 在指定工作室删除作品
    pub async fn delete_workshop_work(&self, workshop_id: i32, work_id: i32) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/works/remove", None)
            .with_payload(json!({
                "id": workshop_id,
                "work_id": work_id,
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 申请加入工作室
    pub async fn execute_apply_to_join(
        &self,
        workshop_id: i32,
        qq: Option<&str>,
    ) -> MewResult<bool> {
        let mut payload = json!({ "id": workshop_id });
        payload["qq"] = match qq {
            Some(val) => json!(val),
            None => Value::Null,
        };

        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/users/apply/join", None)
            .with_payload(payload)
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 审核已经申请加入工作室的用户
    pub async fn execute_review_join_application(
        &self,
        workshop_id: i32,
        status: AuditStatus,
        user_id: i32,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/work_shops/users/audit", None)
            .with_payload(json!({
                "id": workshop_id,
                "status": status.as_str(),
                "user_id": user_id,
            }))
            .send()
            .await?;

        Ok(response.status().is_success())
    }

    /// 举报讨论区下的评论
    pub async fn execute_report_comment(
        &self,
        comment_id: i32,
        reason_content: &str,
        reason_id: WorkShopReportReasonId,
        reporter_id: i32,
        comment_source: Option<Source>,
        comment_parent_id: Option<i32>,
        description: Option<&str>,
    ) -> MewResult<bool> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/reports/comments", None)
            .with_payload(json!({
                "comment_id": comment_id,
                "comment_parent_id": comment_parent_id.unwrap_or(0),
                "description": description.unwrap_or(""),
                "reason_content": reason_content,
                "reason_id": (reason_id as i32).to_string(),
                "reporter_id": reporter_id,
                "comment_source": comment_source.unwrap_or(Source::WorkShop).as_str(),
            }))
            .send()
            .await?;

        Ok(response.status().as_u16() == 201)
    }

    /// 回复评论
    pub async fn create_comment_reply(
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

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({
                "parent_id": parent_id.unwrap_or(0),
                "content": content,
                "source": source.unwrap_or(Source::WorkShop).as_str(),
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }

    /// 删除回复
    pub async fn delete_reply(&self, comment_id: i32, source: Option<Source>) -> MewResult<bool> {
        let endpoint = format!("/web/discussions/replies/{}", comment_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 评论
    pub async fn create_comment(
        &self,
        workshop_id: i32,
        content: &str,
        rich_content: &str,
        source: Option<Source>,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/discussions/{}/comment", workshop_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({
                "content": content,
                "rich_content": rich_content,
                "source": source.unwrap_or(Source::WorkShop).as_str(),
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }

    /// 删除评论
    pub async fn delete_comment(&self, comment_id: i32, source: Option<Source>) -> MewResult<bool> {
        let endpoint = format!("/web/discussions/comments/{}", comment_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .with_param("source", source.unwrap_or(Source::WorkShop).as_str())
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }
}

impl Default for WorkshopActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
