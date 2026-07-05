use crate::utils::acquire::{
    CodeMaoClient, HTTPStatus, HttpMethod, KittyRequestBuilder, MewResult,
};
use serde_json::{Value, json};

// ==================== 小说相关枚举 ====================

// 小说列表类型枚举
pub enum NovelListType {
    All,
    Recommend,
}

impl NovelListType {
    fn as_str(&self) -> &'static str {
        match self {
            NovelListType::All => "all",
            NovelListType::Recommend => "recommend",
        }
    }
}

// 小说排序方式枚举
pub enum NovelSortId {
    Default = 0,
    MostViewed = 1,
    MostFavorited = 2,
    RecentlyUpdated = 3,
}

// 小说分类ID枚举
pub enum NovelCategoryId {
    All = 0,
    Magic = 1,
    SciFi = 2,
    Game = 3,
    Mystery = 4,
    Healing = 5,
    Adventure = 6,
    Daily = 7,
    School = 8,
    Fighting = 9,
    Ancient = 10,
    Horror = 11,
}

// 小说状态枚举
pub enum NovelStatus {
    All = 0,
    Ongoing = 1,
    Completed = 2,
}

// ==================== 图鉴相关枚举 ====================

// 图鉴星级枚举
pub enum BookStar {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
}

// 图鉴属性ID枚举
pub enum BookAttributeId {
    Normal = 2,
    Grass = 3,
    Ground = 4,
    Electric = 5,
    Bug = 6,
    Water = 7,
    Fire = 8,
    Mechanical = 9,
    Flying = 10,
    Psychic = 11,
    Holy = 12,
}

// ==================== 漫画数据获取器 ====================
pub struct CartoonDataFetcher {
    client: &'static CodeMaoClient,
}

impl CartoonDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取全部漫画
    pub fn fetch_all_cartoons(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/comic/list/all", None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取漫画信息
    pub fn fetch_cartoon_info(&self, comic_id: i32) -> MewResult<Value> {
        let endpoint = format!("/api/comic/{}", comic_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取漫画某个章节信息
    pub fn fetch_cartoon_chapter(&self, chapter_id: i32) -> MewResult<Value> {
        let endpoint = format!("/api/comic/page/list/{}", chapter_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for CartoonDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 小说数据获取器 ====================
pub struct NovelDataFetcher {
    client: &'static CodeMaoClient,
}

impl NovelDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取小说分类列表
    pub fn fetch_novel_categories(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/fanfic/type", None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取推荐小说
    pub fn fetch_recommend_novel(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/fanfic/list/recommend", None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取小说列表
    pub fn fetch_novel_list(
        &self,
        list_type: NovelListType,
        sort_id: NovelSortId,
        category_id: NovelCategoryId,
        status: NovelStatus,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let endpoint = format!("/api/fanfic/list/{}", list_type.as_str());

        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("sort_id", (sort_id as i32).to_string())
            .with_param("type_id", (category_id as i32).to_string())
            .with_param("status", (status as i32).to_string())
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(20).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取收藏的小说列表
    pub fn fetch_favorite_novels(&self, page: Option<i32>, limit: Option<i32>) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/fanfic/collection", None)
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取小说详情
    pub fn fetch_novel_details(&self, novel_id: i32) -> MewResult<Value> {
        let endpoint = format!("/api/fanfic/{}", novel_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取小说章节信息
    pub fn fetch_chapter_details(&self, chapter_id: i32) -> MewResult<Value> {
        let endpoint = format!("/api/fanfic/section/{}", chapter_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取小说评论
    pub fn fetch_novel_comments(
        &self,
        novel_id: i32,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let endpoint = format!("/api/fanfic/comments/list/{}", novel_id);

        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("page", page.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取搜索小说结果
    pub fn search_novels(
        &self,
        keyword: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/fanfic/list/search", None)
            .with_param("searchContent", keyword)
            .with_param("page", page.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取小说的所有章节
    pub fn fetch_all_chapters(
        &self,
        novel_id: i32,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/fanfic/{}/sections", novel_id);

        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("amount_items", limit.unwrap_or(200).to_string())
            .with_param("page_number", page.unwrap_or(1).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取我的小说
    pub fn fetch_my_novels(&self, limit: Option<i32>, page: Option<i32>) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/fanfic/my", None)
            .with_param("amount_items", limit.unwrap_or(200).to_string())
            .with_param("page_number", page.unwrap_or(1).to_string())
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取已删除的章节
    pub fn fetch_deleted_chapters(
        &self,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/web/fanfic/section/deleted", None)
            .with_param("amount_items", limit.unwrap_or(200).to_string())
            .with_param("page_number", page.unwrap_or(1).to_string())
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for NovelDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 小说操作处理器 ====================
pub struct NovelActionHandler {
    client: &'static CodeMaoClient,
}

impl NovelActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 收藏 / 取消收藏小说
    pub fn execute_toggle_novel_favorite(&self, novel_id: i32, favorite: bool) -> MewResult<Value> {
        let method = if favorite {
            HttpMethod::Post
        } else {
            HttpMethod::Delete
        };
        let endpoint = format!("/web/fanfic/collect/{}", novel_id);

        let response = self.client.build_request(method, &endpoint, None).send()?;
        self.client.response_to_json(response)
    }

    // 发布小说评论
    pub fn create_novel_comment(
        &self,
        content: &str,
        novel_id: i32,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/api/fanfic/comments/{}", novel_id);
        let payload = json!({
            "content": content
        });

        let response = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    // 点赞 / 取消点赞小说评论
    pub fn execute_toggle_comment_like(
        &self,
        comment_id: i32,
        like: bool,
        return_data: bool,
    ) -> MewResult<Value> {
        let method = if like {
            HttpMethod::Post
        } else {
            HttpMethod::Delete
        };
        let endpoint = format!("/api/fanfic/comments/praise/{}", comment_id);

        let response = self.client.build_request(method, &endpoint, None).send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    // 删除小说评论
    pub fn delete_novel_comment(&self, comment_id: i32, return_data: bool) -> MewResult<Value> {
        let endpoint = format!("/api/fanfic/comments/{}", comment_id);

        let response = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    // 更新章节
    pub fn update_chapter(
        &self,
        chapter_id: i32,
        title: &str,
        content: &str,
        words_num: i32,
    ) -> MewResult<bool> {
        let endpoint = format!("/web/fanfic/section/{}", chapter_id);
        let payload = json!({
            "title": title,
            "content": content,
            "draft_words_num": words_num,
        });

        let response = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 发布章节
    pub fn publish_chapter(&self, chapter_id: i32) -> MewResult<bool> {
        let endpoint = format!("/web/fanfic/section/{}/publish", chapter_id);

        let response = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 更新小说
    pub fn update_novel(
        &self,
        novel_id: i32,
        title: &str,
        introduction: &str,
        category_id: i32,
        status: i32,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/web/fanfic/{}", novel_id);
        let payload = json!({
            "title": title,
            "introduction": introduction,
            "fanfic_type_id": category_id,
            "status": status,
        });

        let response = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(payload)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    // 创建小说
    pub fn create_novel(
        &self,
        title: &str,
        section_title: &str,
        draft: &str,
        cover_pic: &str,
        words_num: i32,
        return_data: bool,
    ) -> MewResult<Value> {
        let payload = json!({
            "title": title,
            "section_title": section_title,
            "draft": draft,
            "cover_pic": cover_pic,
            "draft_words_num": words_num,
        });

        let response = self
            .client
            .build_request(HttpMethod::Post, "/web/fanfic", None)
            .with_payload(payload)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }

    // 删除小说
    pub fn delete_novel(&self, novel_id: i32) -> MewResult<bool> {
        let endpoint = format!("/web/fanfic/{}", novel_id);

        let response = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }
}

impl Default for NovelActionHandler {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 图鉴数据获取器 ====================
pub struct BookDataFetcher {
    client: &'static CodeMaoClient,
}

impl BookDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取全部图鉴
    pub fn fetch_all_books(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/sprite/list/all", None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 获取所有属性
    pub fn fetch_all_attributes(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::Get, "/api/sprite/factio", None)
            .send()?;
        self.client.response_to_json(response)
    }

    // 按星级获取图鉴
    pub fn fetch_books_by_star(&self, star: BookStar) -> MewResult<Value> {
        self._get_books_by_params(
            self.client
                .build_request(HttpMethod::Get, "/api/sprite/list/all", None)
                .with_param("star", (star as i32).to_string()),
        )
    }

    // 按属性获取图鉴
    pub fn fetch_books_by_attribute(&self, attribute_id: BookAttributeId) -> MewResult<Value> {
        self._get_books_by_params(
            self.client
                .build_request(HttpMethod::Get, "/api/sprite/list/all", None)
                .with_param("faction_id", (attribute_id as i32).to_string()),
        )
    }

    // 通用获取图鉴方法
    fn _get_books_by_params(&self, builder: KittyRequestBuilder) -> MewResult<Value> {
        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // 获取指定图鉴详情
    pub fn fetch_book_details(&self, book_id: i32) -> MewResult<Value> {
        let endpoint = format!("/api/sprite/{}", book_id);
        let response = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .send()?;
        self.client.response_to_json(response)
    }
}

impl Default for BookDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 图鉴操作处理器 ====================
pub struct BookActionHandler {
    client: &'static CodeMaoClient,
}

impl BookActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 点赞 / 取消点赞图鉴
    pub fn execute_toggle_book_like(
        &self,
        book_id: i32,
        like: bool,
        return_data: bool,
    ) -> MewResult<Value> {
        let method = if like {
            HttpMethod::Post
        } else {
            HttpMethod::Delete
        };
        let endpoint = format!("/api/sprite/praise/{}", book_id);

        let response = self.client.build_request(method, &endpoint, None).send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Ok as u16 }))
        }
    }
}

impl Default for BookActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
