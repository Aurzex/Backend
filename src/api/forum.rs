use crate::utils::acquire::{
    CodeMaoClient, HttpMethod, MewError, MewResult, PaginatedIter, PaginationMethod,
};
use serde_json::{Value, json};

// ==================== 枚举定义 ====================

pub enum PostType {
    Created,
    Replied,
}

impl PostType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PostType::Created => "created",
            PostType::Replied => "replied",
        }
    }
}

pub enum ItemType {
    Reply,
    Comment,
}

impl ItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemType::Reply => "REPLY",
            ItemType::Comment => "COMMENT",
        }
    }
}

pub enum DeleteItemType {
    Reply,
    Comment,
    Post,
}

impl DeleteItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeleteItemType::Reply => "reply",
            DeleteItemType::Comment => "comment",
            DeleteItemType::Post => "post",
        }
    }

    fn endpoint(&self, item_id: i32) -> String {
        match self {
            DeleteItemType::Reply => format!("/web/forums/replies/{}", item_id),
            DeleteItemType::Comment => format!("/web/forums/comments/{}", item_id),
            DeleteItemType::Post => format!("/web/forums/posts/{}", item_id),
        }
    }
}

pub enum TargetType {
    Board,
    Workshop,
}

impl TargetType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetType::Board => "board",
            TargetType::Workshop => "workshop",
        }
    }
}

#[repr(i32)]
pub enum ForumReportReasonId {
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

#[repr(i32)]
pub enum PostReportReasonId {
    Reason1 = 1,
    Reason2 = 2,
    Reason3 = 3,
    Reason4 = 4,
    Reason5 = 5,
    Reason6 = 6,
    Reason7 = 7,
    Reason8 = 8,
}

#[repr(i32)]
pub enum BoardId {
    Board17 = 17,
    Board2 = 2,
    Board10 = 10,
    Board5 = 5,
    Board3 = 3,
    Board6 = 6,
    Board27 = 27,
    Board11 = 11,
    Board26 = 26,
    Board13 = 13,
    Board7 = 7,
    Board4 = 4,
    Board28 = 28,
}

// ==================== ForumDataFetcher ====================

pub struct ForumDataFetcher {
    client: &'static CodeMaoClient,
}

impl ForumDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取多个帖子信息
    pub async fn fetch_posts_details(&self, post_ids: Vec<i32>) -> MewResult<Value> {
        if post_ids.len() >= 20 {
            return Err(MewError::Other("数据长度需小于 20".into()));
        }

        let ids_str: Vec<String> = post_ids.iter().map(|id| id.to_string()).collect();

        self.client
            .build_request(HttpMethod::GET, "/web/forums/posts/all", None)
            .with_param("ids", ids_str.join(","))
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取单个帖子信息
    pub async fn fetch_single_post_details(&self, post_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/forums/posts/{}/details", post_id);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取帖子回帖生成器
    pub fn fetch_post_replies_gen(
        &self,
        post_id: i32,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/forums/posts/{}/replies", post_id);

        self.client
            .paginated(&endpoint)
            .with_param("page", "1")
            .with_param("limit", "10")
            .with_param("sort", sort.unwrap_or_else(|| "-created_at".to_string()))
            .with_pagination_method(PaginationMethod::Page)
            .with_total_key("total")
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(limit.unwrap_or(15))
    }

    /// 获取回帖评论生成器
    pub fn fetch_reply_comments_gen(&self, reply_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/forums/replies/{}/comments", reply_id);

        self.client
            .paginated(&endpoint)
            .with_param("page", "1")
            .with_param("limit", "10")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(limit.unwrap_or(10))
    }

    /// 获取我的帖子或回复的帖子生成器
    pub fn fetch_my_posts_gen(&self, post_type: PostType, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/forums/posts/mine/{}", post_type.as_str());

        self.client
            .paginated(&endpoint)
            .with_param("page", "1")
            .with_param("limit", "10")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(limit.unwrap_or(10))
    }

    /// 获取我的帖子或回复的帖子数目
    pub async fn fetch_my_post_num(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/forums/posts/mine/count", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取论坛帖子各个栏目
    pub async fn fetch_post_boards(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/forums/boards/simples/all", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取论坛单个版块详细信息
    pub async fn fetch_board_details(&self, board_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/forums/boards/{}", board_id);

        self.client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取社区所有热门帖子 ID
    pub async fn fetch_hot_posts_ids(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/forums/posts/hots/all", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取论坛顶部公告
    pub async fn fetch_top_notices(&self, limit: Option<i32>) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/forums/notice-boards", None)
            .with_param("limit", limit.unwrap_or(4).to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取论坛本周精选帖子
    pub async fn fetch_key_content(
        &self,
        content_key: &str,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/contents/get-key", None)
            .with_param("content_key", content_key)
            .with_param("limit", limit.unwrap_or(4).to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取社区精品合集帖子
    pub async fn fetch_selection_posts(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/forums/posts/selections", None)
            .with_param("limit", limit.unwrap_or(20).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 获取论坛举报原因
    pub async fn fetch_report_reasons(&self) -> MewResult<Value> {
        self.client
            .build_request(HttpMethod::GET, "/web/reports/posts/reasons/all", None)
            .send()
            .await?
            .json()
            .await
            .map_err(MewError::from)
    }

    /// 通过标题搜索帖子生成器
    pub fn search_posts_gen(&self, title: &str, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/web/forums/posts/search")
            .with_param("title", title)
            .with_param("page", "1")
            .with_param("limit", "20")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(limit.unwrap_or(20))
    }

    /// 获取热门帖子 (7天内) 生成器
    pub fn fetch_7day_hot_posts_gen(
        &self,
        board_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = match board_id {
            Some(id) => format!("/web/forums/boards/posts/7dayHot?board_id={}", id),
            None => "/web/forums/boards/posts/7dayHot".to_string(),
        };

        self.client
            .paginated(&endpoint)
            .with_param("page", "1")
            .with_param("limit", "10")
            .with_pagination_method(PaginationMethod::Page)
            .with_total_key("total")
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(limit.unwrap_or(15))
    }

    /// 获取求助帖子生成器
    pub fn fetch_ask_help_posts_gen(&self, limit: Option<usize>) -> PaginatedIter {
        self.client
            .paginated("/web/forums/boards/posts/ask-help")
            .with_param("page", "1")
            .with_param("limit", "10")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(limit.unwrap_or(10))
    }
}

impl Default for ForumDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== ForumActionHandler ====================

pub struct ForumActionHandler {
    client: &'static CodeMaoClient,
}

impl ForumActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 对某个帖子回帖
    pub async fn create_post_reply(
        &self,
        post_id: i32,
        content: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/forums/posts/{}/replies", post_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({ "content": content }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }

    /// 对某个回帖评论进行回复
    pub async fn create_comment_reply(
        &self,
        reply_id: i32,
        parent_id: i32,
        content: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/forums/replies/{}/comments", reply_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({
                "content": content,
                "parent_id": parent_id
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }

    /// 点赞或取消点赞某个回帖或评论
    pub async fn execute_toggle_like(
        &self,
        action: &str,
        item_id: i32,
        item_type: ItemType,
    ) -> MewResult<bool> {
        let method = match action {
            "like" => HttpMethod::PUT,
            "unlike" => HttpMethod::DELETE,
            _ => {
                return Err(MewError::Other(
                    "无效的action，必须是 'like' 或 'unlike'".into(),
                ));
            }
        };

        let endpoint = format!("/web/forums/comments/{}/liked", item_id);

        let response = self
            .client
            .build_request(method, &endpoint, None)
            .with_param("source", item_type.as_str())
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 举报某个回帖或评论
    pub async fn report_item(
        &self,
        item_id: i32,
        reason_id: ForumReportReasonId,
        description: &str,
        item_type: ItemType,
        return_data: bool,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/reports/posts/discussions", None)
            .with_payload(json!({
                "reason_id": reason_id as i32,
                "description": description,
                "discussion_id": item_id,
                "source": item_type.as_str(),
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }

    /// 举报某个帖子
    pub async fn report_post(
        &self,
        post_id: i32,
        reason_id: PostReportReasonId,
        description: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/web/reports/posts", None)
            .with_payload(json!({
                "reason_id": reason_id as i32,
                "description": description,
                "post_id": post_id,
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }

    /// 删除某个回帖或评论或帖子
    pub async fn delete_item(&self, item_id: i32, item_type: DeleteItemType) -> MewResult<bool> {
        let endpoint = item_type.endpoint(item_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 置顶或取消置顶某个回帖
    pub async fn execute_toggle_comment_top_status(
        &self,
        comment_id: i32,
        should_top: bool,
    ) -> MewResult<bool> {
        let method = if should_top {
            HttpMethod::PUT
        } else {
            HttpMethod::DELETE
        };
        let endpoint = format!("/web/forums/replies/{}/top", comment_id);

        let response = self
            .client
            .build_request(method, &endpoint, None)
            .send()
            .await?;

        Ok(response.status().as_u16() == 204)
    }

    /// 发布帖子
    pub async fn create_post(
        &self,
        target_type: TargetType,
        title: &str,
        content: &str,
        board_id: Option<BoardId>,
        workshop_id: Option<i32>,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = match target_type {
            TargetType::Board => match board_id {
                Some(id) => format!("/web/forums/boards/{}/posts", id as i32),
                None => {
                    return Err(MewError::Other(
                        "board_id is required when target_type is 'board'".into(),
                    ));
                }
            },
            TargetType::Workshop => match workshop_id {
                Some(id) => format!("/web/works/subjects/{}/post", id),
                None => {
                    return Err(MewError::Other(
                        "workshop_id is required when target_type is 'workshop'".into(),
                    ));
                }
            },
        };

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({
                "title": title,
                "content": content
            }))
            .send()
            .await?;

        if return_data {
            response.json().await.map_err(MewError::from)
        } else {
            Ok(json!({ "success": response.status().as_u16() == 201 }))
        }
    }
}

impl Default for ForumActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
