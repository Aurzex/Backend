use crate::utils::acquire::{
    ClientAccess, CodeMaoClient, HTTPStatus, HttpMethod, MewError, MewResult, PaginatedIter,
    PaginationMethod,
};
use log::debug;
use serde_json::{Value, json};

// ==================== 枚举定义 ====================

/// 帖子类型(我创建的 / 我回复的).
#[derive(Debug, Clone, Copy)]
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

/// 点赞项目类型(回帖 / 评论).
#[derive(Debug, Clone, Copy)]
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

/// 删除项目类型(回帖 / 评论 / 帖子).
#[derive(Debug, Clone, Copy)]
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

/// 发布帖子目标类型(板块 / 工作室).
#[derive(Debug, Clone, Copy)]
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

/// 回帖/评论举报原因 ID.
#[derive(Debug, Clone, Copy)]
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

/// 帖子举报原因 ID.
#[derive(Debug, Clone, Copy)]
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

/// 论坛板块 ID.
#[derive(Debug, Clone, Copy)]
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

// ==================== 论坛数据获取器 ====================

/// 论坛数据查询接口.
pub struct ForumDataFetcher {
    client: &'static CodeMaoClient,
}

impl ForumDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ==================== 私有辅助 ====================

    /// 构建基础分页迭代器,使用 Page 分页方式,初始页码 1.
    fn build_paginated(
        &self,
        endpoint: &str,
        page_size: usize,
        default_limit: usize,
    ) -> PaginatedIter {
        self.client
            .paginated(endpoint)
            .with_iter_param("page", "1")
            .with_page_size(page_size)
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("limit")
            .with_offset_key("page")
            .with_limit(default_limit)
    }

    // ==================== 公共方法 ====================

    /// 批量获取帖子详情(最多 19 个).
    pub fn fetch_posts_details(&self, post_ids: Vec<i32>) -> MewResult<Value> {
        if post_ids.len() >= 20 {
            return Err(MewError::Other("数据长度需小于 20".into()));
        }
        debug!("批量获取帖子详情: count={}", post_ids.len());
        let ids_str: Vec<String> = post_ids.iter().map(|id| id.to_string()).collect();
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/forums/posts/all", None)
            .with_param("ids", ids_str.join(","))
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取单个帖子详情.
    pub fn fetch_single_post_details(&self, post_id: i32) -> MewResult<Value> {
        debug!("获取帖子详情: post_id={}", post_id);
        let endpoint = format!("/web/forums/posts/{}/details", post_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 帖子回帖分页迭代器.
    pub fn fetch_post_replies_gen(
        &self,
        post_id: i32,
        sort: Option<String>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = format!("/web/forums/posts/{}/replies", post_id);
        debug!("获取回帖迭代器: post_id={}, sort={:?}", post_id, sort);

        self.build_paginated(&endpoint, 10, limit.unwrap_or(15))
            .with_iter_param("sort", sort.unwrap_or_else(|| "-created_at".to_string()))
            .with_total_key("total")
    }

    /// 回帖评论分页迭代器.
    pub fn fetch_reply_comments_gen(&self, reply_id: i32, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/forums/replies/{}/comments", reply_id);
        debug!("获取评论迭代器: reply_id={}", reply_id);

        self.build_paginated(&endpoint, 10, limit.unwrap_or(10))
    }

    /// 我的帖子(创建/回复)分页迭代器.
    pub fn fetch_my_posts_gen(&self, post_type: PostType, limit: Option<usize>) -> PaginatedIter {
        let endpoint = format!("/web/forums/posts/mine/{}", post_type.as_str());
        debug!("获取我的帖子迭代器: type={:?}", post_type);

        self.build_paginated(&endpoint, 10, limit.unwrap_or(10))
    }

    /// 获取我的帖子/回复数量.
    pub fn fetch_my_post_num(&self) -> MewResult<Value> {
        debug!("获取我的帖子数量");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/forums/posts/mine/count", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取所有板块简要信息.
    pub fn fetch_post_boards(&self) -> MewResult<Value> {
        debug!("获取所有板块信息");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/forums/boards/simples/all", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取单个板块详细信息.
    pub fn fetch_board_details(&self, board_id: i32) -> MewResult<Value> {
        debug!("获取板块详情: board_id={}", board_id);
        let endpoint = format!("/web/forums/boards/{}", board_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取所有热门帖子 ID.
    pub fn fetch_hot_posts_ids(&self) -> MewResult<Value> {
        debug!("获取热门帖子ID");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/forums/posts/hots/all", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取顶部公告(默认 4 条).
    pub fn fetch_top_notices(&self, limit: Option<i32>) -> MewResult<Value> {
        debug!("获取顶部公告: limit={:?}", limit);
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/forums/notice-boards", None)
            .with_param("limit", limit.unwrap_or(4).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取论坛精选内容.
    pub fn fetch_key_content(&self, content_key: &str, limit: Option<i32>) -> MewResult<Value> {
        debug!("获取精选内容: key={}, limit={:?}", content_key, limit);
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/contents/get-key", None)
            .with_param("content_key", content_key)
            .with_param("limit", limit.unwrap_or(4).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取精品合集帖子.
    pub fn fetch_selection_posts(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取精品合集: limit={:?}, offset={:?}", limit, offset);
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/forums/posts/selections", None)
            .with_param("limit", limit.unwrap_or(20).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    /// 获取帖子举报原因列表.
    pub fn fetch_report_reasons(&self) -> MewResult<Value> {
        debug!("获取举报原因列表");
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/reports/posts/reasons/all", None)
            .send()?;
        self.client.response_to_json(response)
    }

    /// 按标题搜索帖子分页迭代器.
    pub fn search_posts_gen(&self, title: &str, limit: Option<usize>) -> PaginatedIter {
        debug!("搜索帖子: title={}", title);

        self.build_paginated("/web/forums/posts/search", 20, limit.unwrap_or(20))
            .with_iter_param("title", title)
    }

    /// 7 天内热门帖子分页迭代器.
    pub fn fetch_7day_hot_posts_gen(
        &self,
        board_id: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let endpoint = match board_id {
            Some(id) => format!("/web/forums/boards/posts/7dayHot?board_id={}", id),
            None => "/web/forums/boards/posts/7dayHot".to_string(),
        };
        debug!("获取7天热门: board_id={:?}", board_id);

        self.build_paginated(&endpoint, 10, limit.unwrap_or(15))
            .with_total_key("total")
    }

    /// 求助帖子分页迭代器.
    pub fn fetch_ask_help_posts_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取求助帖子迭代器");

        self.build_paginated("/web/forums/boards/posts/ask-help", 10, limit.unwrap_or(10))
    }
}

impl Default for ForumDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for ForumDataFetcher {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

// ==================== 论坛操作处理器 ====================

/// 论坛操作接口(发帖,回复,点赞,举报等).
pub struct ForumActionHandler {
    client: &'static CodeMaoClient,
}

impl ForumActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ==================== 公共方法 ====================

    /// 回复帖子.
    pub fn create_post_reply(
        &self,
        post_id: i32,
        content: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("回复帖子: post_id={}", post_id);
        let endpoint = format!("/web/forums/posts/{}/replies", post_id);
        let payload = json!({ "content": content });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }

    /// 回复评论(在回帖下评论).
    pub fn create_comment_reply(
        &self,
        reply_id: i32,
        parent_id: i32,
        content: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("回复评论: reply_id={}, parent_id={}", reply_id, parent_id);
        let endpoint = format!("/web/forums/replies/{}/comments", reply_id);
        let payload = json!({
            "content": content,
            "parent_id": parent_id
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }

    /// 点赞或取消点赞,`action` 为 "like" 或 "unlike".
    pub fn execute_toggle_like(
        &self,
        action: &str,
        item_id: i32,
        item_type: ItemType,
    ) -> MewResult<bool> {
        let method = match action {
            "like" => HttpMethod::Put,
            "unlike" => HttpMethod::Delete,
            _ => {
                return Err(MewError::Other(
                    "无效的action,必须是 'like' 或 'unlike'".into(),
                ));
            }
        };
        debug!(
            "点赞操作: action={}, item_id={}, type={:?}",
            action, item_id, item_type
        );
        let endpoint = format!("/web/forums/comments/{}/liked", item_id);
        let builder = self
            .client
            .build_request(method, &endpoint, None)
            .with_param("source", item_type.as_str());
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 举报回帖或评论.
    pub fn report_item(
        &self,
        item_id: i32,
        reason_id: ForumReportReasonId,
        description: &str,
        item_type: ItemType,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("举报内容: item_id={}, type={:?}", item_id, item_type);
        let payload = json!({
            "reason_id": reason_id as i32,
            "description": description,
            "discussion_id": item_id,
            "source": item_type.as_str(),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/reports/posts/discussions", None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }

    /// 举报帖子.
    pub fn report_post(
        &self,
        post_id: i32,
        reason_id: PostReportReasonId,
        description: &str,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("举报帖子: post_id={}", post_id);
        let payload = json!({
            "reason_id": reason_id as i32,
            "description": description,
            "post_id": post_id,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/reports/posts", None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }

    /// 删除回帖 / 评论 / 帖子.
    pub fn delete_item(&self, item_id: i32, item_type: DeleteItemType) -> MewResult<bool> {
        let endpoint = match item_type {
            DeleteItemType::Reply => format!("/web/forums/replies/{}", item_id),
            DeleteItemType::Comment => format!("/web/forums/comments/{}", item_id),
            DeleteItemType::Post => format!("/web/forums/posts/{}", item_id),
        };
        debug!("删除项目: item_id={}, type={:?}", item_id, item_type);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 置顶 / 取消置顶回帖.
    pub fn execute_toggle_comment_top_status(
        &self,
        comment_id: i32,
        should_top: bool,
    ) -> MewResult<bool> {
        let method = if should_top {
            HttpMethod::Put
        } else {
            HttpMethod::Delete
        };
        debug!(
            "置顶操作: comment_id={}, should_top={}",
            comment_id, should_top
        );
        let endpoint = format!("/web/forums/replies/{}/top", comment_id);
        let builder = self.client.build_request(method, &endpoint, None);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 发布帖子.
    pub fn create_post(
        &self,
        target_type: TargetType,
        title: &str,
        content: &str,
        board_id: Option<BoardId>,
        workshop_id: Option<i32>,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("发布帖子: target={:?}, title={}", target_type, title);
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

        let payload = json!({
            "title": title,
            "content": content
        });

        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Created)
    }
}

impl Default for ForumActionHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for ForumActionHandler {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}
