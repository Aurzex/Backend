use crate::utils::acquire::{CodeMaoClient, HttpMethod, PaginatedIter, PaginationMethod};
use serde_json::{Value, json};
use std::collections::HashMap;

// 帖子类型枚举
pub enum PostType {
    Created,
    Replied,
}

impl PostType {
    fn as_str(&self) -> &'static str {
        match self {
            PostType::Created => "created",
            PostType::Replied => "replied",
        }
    }
}

// 项目类型枚举（用于点赞）
pub enum ItemType {
    Reply,
    Comment,
}

impl ItemType {
    fn as_str(&self) -> &'static str {
        match self {
            ItemType::Reply => "REPLY",
            ItemType::Comment => "COMMENT",
        }
    }
}

// 删除项目类型枚举
pub enum DeleteItemType {
    Reply,
    Comment,
    Post,
}

impl DeleteItemType {
    fn as_str(&self) -> &'static str {
        match self {
            DeleteItemType::Reply => "reply",
            DeleteItemType::Comment => "comment",
            DeleteItemType::Post => "post",
        }
    }
}

// 目标类型枚举（用于发布帖子）
pub enum TargetType {
    Board,
    Workshop,
}

impl TargetType {
    fn as_str(&self) -> &'static str {
        match self {
            TargetType::Board => "board",
            TargetType::Workshop => "workshop",
        }
    }
}

// 举报原因ID枚举
#[repr(i32)]
pub enum ReportReasonId {
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

// 帖子举报原因ID枚举
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

// 板块ID枚举
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

// 论坛数据获取器
pub struct ForumDataFetcher {
    client: &'static CodeMaoClient,
}

impl ForumDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取多个帖子信息
    pub fn fetch_posts_details(
        &self,
        post_ids: Vec<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        if post_ids.len() >= 20 {
            return Err("数据长度需小于 20".into());
        }

        let ids_str: Vec<String> = post_ids.iter().map(|id| id.to_string()).collect();
        let mut params = HashMap::new();
        params.insert("ids".to_string(), ids_str.join(","));

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/forums/posts/all",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取单个帖子信息
    pub fn fetch_single_post_details(
        &self,
        post_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/forums/posts/{}/details", post_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取帖子回帖生成器
    pub fn fetch_post_replies_gen(
        &self,
        post_id: i32,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());
        params.insert(
            "sort".to_string(),
            sort.unwrap_or_else(|| "-created_at".to_string()),
        );

        let endpoint = format!("/web/forums/posts/{}/replies", post_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_total_key("total")
            .with_amount_key("limit")
            .with_offset_key("page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取回帖评论生成器
    pub fn fetch_reply_comments_gen(&self, reply_id: i32, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let endpoint = format!("/web/forums/replies/{}/comments", reply_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }

        paginated
    }

    // 获取我的帖子或回复的帖子生成器
    pub fn fetch_my_posts_gen(&self, post_type: PostType, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let endpoint = format!("/web/forums/posts/mine/{}", post_type.as_str());

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }

        paginated
    }

    // 获取我的帖子或回复的帖子数目
    pub fn fetch_my_post_num(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/forums/posts/mine/count",
            None,
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取论坛帖子各个栏目
    pub fn fetch_post_boards(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/forums/boards/simples/all",
            None,
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取论坛单个版块详细信息
    pub fn fetch_board_details(&self, board_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/forums/boards/{}", board_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取社区所有热门帖子 ID
    pub fn fetch_hot_posts_ids(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/forums/posts/hots/all",
            None,
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取论坛顶部公告
    pub fn fetch_top_notices(
        &self,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), limit.unwrap_or(4).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/forums/notice-boards",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取论坛本周精选帖子
    pub fn fetch_key_content(
        &self,
        content_key: &str,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("content_key".to_string(), content_key.to_string());
        params.insert("limit".to_string(), limit.unwrap_or(4).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/contents/get-key",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取社区精品合集帖子
    pub fn fetch_selection_posts(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("limit".to_string(), limit.unwrap_or(20).to_string());
        params.insert("offset".to_string(), offset.unwrap_or(0).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/forums/posts/selections",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取论坛举报原因
    pub fn fetch_report_reasons(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/reports/posts/reasons/all",
            None,
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 通过标题搜索帖子生成器
    pub fn search_posts_gen(&self, title: &str, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("title".to_string(), title.to_string());
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "20".to_string());

        let mut paginated = self
            .client
            .paginated("/web/forums/posts/search")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(20);
        }

        paginated
    }

    // 获取热门帖子 (7天内) 生成器
    pub fn fetch_7day_hot_posts_gen(
        &self,
        board_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let endpoint = match board_id {
            Some(id) => format!("/web/forums/boards/posts/7dayHot?board_id={}", id),
            None => "/web/forums/boards/posts/7dayHot".to_string(),
        };

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_total_key("total")
            .with_amount_key("limit")
            .with_offset_key("page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // 获取求助帖子生成器
    pub fn fetch_ask_help_posts_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let mut params = HashMap::new();
        params.insert("page".to_string(), "1".to_string());
        params.insert("limit".to_string(), "10".to_string());

        let mut paginated = self
            .client
            .paginated("/web/forums/boards/posts/ask-help")
            .with_params(params)
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(10);
        }

        paginated
    }
}

impl Default for ForumDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// 论坛操作处理器
pub struct ForumActionHandler {
    client: &'static CodeMaoClient,
}

impl ForumActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 对某个帖子回帖
    pub fn create_post_reply(
        &self,
        post_id: i32,
        content: &str,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/forums/posts/{}/replies", post_id);
        let payload = json!({
            "content": content
        });

        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&payload), None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 201 }))
        }
    }

    // 对某个回帖评论进行回复
    pub fn create_comment_reply(
        &self,
        reply_id: i32,
        parent_id: i32,
        content: &str,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/forums/replies/{}/comments", reply_id);
        let payload = json!({
            "content": content,
            "parent_id": parent_id
        });

        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&payload), None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 201 }))
        }
    }

    // 点赞或取消点赞某个回帖或评论
    pub fn execute_toggle_like(
        &self,
        action: &str,
        item_id: i32,
        item_type: ItemType,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let method = match action {
            "like" => HttpMethod::PUT,
            "unlike" => HttpMethod::DELETE,
            _ => return Err("无效的action，必须是 'like' 或 'unlike'".into()),
        };

        let mut params = HashMap::new();
        params.insert("source".to_string(), item_type.as_str().to_string());

        let endpoint = format!("/web/forums/comments/{}/liked", item_id);

        let response = self
            .client
            .send_request(method, &endpoint, Some(&params), None, None)?;

        Ok(response.status() == 204)
    }

    // 举报某个回帖或评论
    pub fn report_item(
        &self,
        item_id: i32,
        reason_id: ReportReasonId,
        description: &str,
        item_type: ItemType,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "reason_id": reason_id as i32,
            "description": description,
            "discussion_id": item_id,
            "source": item_type.as_str(),
        });

        let response = self.client.send_request(
            HttpMethod::POST,
            "/web/reports/posts/discussions",
            None,
            Some(&payload),
            None,
        )?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 201 }))
        }
    }

    // 举报某个帖子
    pub fn report_post(
        &self,
        post_id: i32,
        reason_id: PostReportReasonId,
        description: &str,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "reason_id": reason_id as i32,
            "description": description,
            "post_id": post_id,
        });

        let response = self.client.send_request(
            HttpMethod::POST,
            "/web/reports/posts",
            None,
            Some(&payload),
            None,
        )?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 201 }))
        }
    }

    // 删除某个回帖或评论或帖子
    pub fn delete_item(
        &self,
        item_id: i32,
        item_type: DeleteItemType,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = match item_type {
            DeleteItemType::Reply => format!("/web/forums/replies/{}", item_id),
            DeleteItemType::Comment => format!("/web/forums/comments/{}", item_id),
            DeleteItemType::Post => format!("/web/forums/posts/{}", item_id),
        };

        let response = self
            .client
            .send_request(HttpMethod::DELETE, &endpoint, None, None, None)?;

        Ok(response.status() == 204)
    }

    // 置顶或取消置顶某个回帖
    pub fn execute_toggle_comment_top_status(
        &self,
        comment_id: i32,
        should_top: bool,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let method = if should_top {
            HttpMethod::PUT
        } else {
            HttpMethod::DELETE
        };
        let endpoint = format!("/web/forums/replies/{}/top", comment_id);

        let response = self
            .client
            .send_request(method, &endpoint, None, None, None)?;

        Ok(response.status() == 204)
    }

    // 发布帖子
    pub fn create_post(
        &self,
        target_type: TargetType,
        title: &str,
        content: &str,
        board_id: Option<BoardId>,
        workshop_id: Option<i32>,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = match target_type {
            TargetType::Board => match board_id {
                Some(id) => format!("/web/forums/boards/{}/posts", id as i32),
                None => return Err("board_id is required when target_type is 'board'".into()),
            },
            TargetType::Workshop => match workshop_id {
                Some(id) => format!("/web/works/subjects/{}/post", id),
                None => return Err("workshop_id is required when target_type is 'workshop'".into()),
            },
        };

        let payload = json!({
            "title": title,
            "content": content
        });

        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&payload), None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 201 }))
        }
    }
}

impl Default for ForumActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
