use crate::utils::acquire::{CodeMaoClient, HttpMethod};
use serde_json::{Value, json};
use std::collections::HashMap;

// ==================== 漫画相关枚举 ====================
// 暂无特定枚举，直接使用结构体

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
#[repr(i32)]
pub enum NovelSortId {
    Default = 0,
    MostViewed = 1,
    MostFavorited = 2,
    RecentlyUpdated = 3,
}

// 小说分类ID枚举
#[repr(i32)]
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
#[repr(i32)]
pub enum NovelStatus {
    All = 0,
    Ongoing = 1,
    Completed = 2,
}

// ==================== 图鉴相关枚举 ====================

// 图鉴星级枚举
#[repr(i32)]
pub enum BookStar {
    OneStar = 1,
    TwoStar = 2,
    ThreeStar = 3,
    FourStar = 4,
    FiveStar = 5,
    SixStar = 6,
}

// 图鉴属性ID枚举
#[repr(i32)]
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
    pub fn fetch_all_cartoons(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response =
            self.client
                .send_request(HttpMethod::GET, "/api/comic/list/all", None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取漫画信息
    pub fn fetch_cartoon_info(&self, comic_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/comic/{}", comic_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取漫画某个章节信息
    pub fn fetch_cartoon_chapter(
        &self,
        chapter_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/comic/page/list/{}", chapter_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
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
    pub fn fetch_novel_categories(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response =
            self.client
                .send_request(HttpMethod::GET, "/api/fanfic/type", None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取推荐小说
    pub fn fetch_recommend_novel(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::GET,
            "/api/fanfic/list/recommend",
            None,
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
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
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("sort_id".to_string(), (sort_id as i32).to_string());
        params.insert("type_id".to_string(), (category_id as i32).to_string());
        params.insert("status".to_string(), (status as i32).to_string());
        params.insert("page".to_string(), page.unwrap_or(1).to_string());
        params.insert("limit".to_string(), limit.unwrap_or(20).to_string());

        let endpoint = format!("/api/fanfic/list/{}", list_type.as_str());
        let response =
            self.client
                .send_request(HttpMethod::GET, &endpoint, Some(&params), None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取收藏的小说列表
    pub fn fetch_favorite_novels(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("page".to_string(), page.unwrap_or(1).to_string());
        params.insert("limit".to_string(), limit.unwrap_or(10).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/fanfic/collection",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取小说详情
    pub fn fetch_novel_details(&self, novel_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/fanfic/{}", novel_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取小说章节信息
    pub fn fetch_chapter_details(
        &self,
        chapter_id: i32,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/fanfic/section/{}", chapter_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取小说评论
    pub fn fetch_novel_comments(
        &self,
        novel_id: i32,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("page".to_string(), page.unwrap_or(0).to_string());
        params.insert("limit".to_string(), limit.unwrap_or(10).to_string());

        let endpoint = format!("/api/fanfic/comments/list/{}", novel_id);
        let response =
            self.client
                .send_request(HttpMethod::GET, &endpoint, Some(&params), None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取搜索小说结果
    pub fn search_novels(
        &self,
        keyword: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("searchContent".to_string(), keyword.to_string());
        params.insert("page".to_string(), page.unwrap_or(0).to_string());
        params.insert("limit".to_string(), limit.unwrap_or(10).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/api/fanfic/list/search",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取小说的所有章节
    pub fn fetch_all_chapters(
        &self,
        novel_id: i32,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("amount_items".to_string(), limit.unwrap_or(200).to_string());
        params.insert("page_number".to_string(), page.unwrap_or(1).to_string());

        let endpoint = format!("/web/fanfic/{}/sections", novel_id);
        let response =
            self.client
                .send_request(HttpMethod::GET, &endpoint, Some(&params), None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取我的小说
    pub fn fetch_my_novels(
        &self,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("amount_items".to_string(), limit.unwrap_or(200).to_string());
        params.insert("page_number".to_string(), page.unwrap_or(1).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/fanfic/my",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取已删除的章节
    pub fn fetch_deleted_chapters(
        &self,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("amount_items".to_string(), limit.unwrap_or(200).to_string());
        params.insert("page_number".to_string(), page.unwrap_or(1).to_string());

        let response = self.client.send_request(
            HttpMethod::GET,
            "/web/fanfic/section/deleted",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
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
    pub fn execute_toggle_novel_favorite(
        &self,
        novel_id: i32,
        favorite: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let method = if favorite {
            HttpMethod::POST
        } else {
            HttpMethod::DELETE
        };
        let endpoint = format!("/web/fanfic/collect/{}", novel_id);

        let response = self
            .client
            .send_request(method, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 发布小说评论
    pub fn create_novel_comment(
        &self,
        content: &str,
        novel_id: i32,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/fanfic/comments/{}", novel_id);
        let payload = json!({
            "content": content
        });

        let response =
            self.client
                .send_request(HttpMethod::POST, &endpoint, None, Some(&payload), None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
        }
    }

    // 点赞 / 取消点赞小说评论
    pub fn execute_toggle_comment_like(
        &self,
        comment_id: i32,
        like: bool,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let method = if like {
            HttpMethod::POST
        } else {
            HttpMethod::DELETE
        };
        let endpoint = format!("/api/fanfic/comments/praise/{}", comment_id);

        let response = self
            .client
            .send_request(method, &endpoint, None, None, None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
        }
    }

    // 删除小说评论
    pub fn delete_novel_comment(
        &self,
        comment_id: i32,
        return_data: bool,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/fanfic/comments/{}", comment_id);

        let response = self
            .client
            .send_request(HttpMethod::DELETE, &endpoint, None, None, None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
        }
    }

    // 更新章节
    pub fn update_chapter(
        &self,
        chapter_id: i32,
        title: &str,
        content: &str,
        words_num: i32,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/fanfic/section/{}", chapter_id);
        let payload = json!({
            "title": title,
            "content": content,
            "draft_words_num": words_num,
        });

        let response =
            self.client
                .send_request(HttpMethod::PUT, &endpoint, None, Some(&payload), None)?;

        Ok(response.status() == 204)
    }

    // 发布章节
    pub fn publish_chapter(&self, chapter_id: i32) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/fanfic/section/{}/publish", chapter_id);

        let response =
            self.client
                .send_request(HttpMethod::PUT, &endpoint, None, Some(&json!({})), None)?;

        Ok(response.status() == 204)
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
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/fanfic/{}", novel_id);
        let payload = json!({
            "title": title,
            "introduction": introduction,
            "fanfic_type_id": category_id,
            "status": status,
        });

        let response =
            self.client
                .send_request(HttpMethod::PUT, &endpoint, None, Some(&payload), None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
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
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let payload = json!({
            "title": title,
            "section_title": section_title,
            "draft": draft,
            "cover_pic": cover_pic,
            "draft_words_num": words_num,
        });

        let response = self.client.send_request(
            HttpMethod::POST,
            "/web/fanfic",
            None,
            Some(&payload),
            None,
        )?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
        }
    }

    // 删除小说
    pub fn delete_novel(&self, novel_id: i32) -> Result<bool, Box<dyn std::error::Error>> {
        let endpoint = format!("/web/fanfic/{}", novel_id);

        let response = self
            .client
            .send_request(HttpMethod::DELETE, &endpoint, None, None, None)?;

        Ok(response.status() == 204)
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
    pub fn fetch_all_books(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response =
            self.client
                .send_request(HttpMethod::GET, "/api/sprite/list/all", None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取所有属性
    pub fn fetch_all_attributes(&self) -> Result<Value, Box<dyn std::error::Error>> {
        let response =
            self.client
                .send_request(HttpMethod::GET, "/api/sprite/factio", None, None, None)?;
        Ok(self.client.response_to_json(response)?)
    }

    // 按星级获取图鉴
    pub fn fetch_books_by_star(&self, star: BookStar) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("star".to_string(), (star as i32).to_string());
        self._get_books_by_params(params)
    }

    // 按属性获取图鉴
    pub fn fetch_books_by_attribute(
        &self,
        attribute_id: BookAttributeId,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let mut params = HashMap::new();
        params.insert("faction_id".to_string(), (attribute_id as i32).to_string());
        self._get_books_by_params(params)
    }

    // 通用获取图鉴方法
    fn _get_books_by_params(
        &self,
        params: HashMap<String, String>,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let response = self.client.send_request(
            HttpMethod::GET,
            "/api/sprite/list/all",
            Some(&params),
            None,
            None,
        )?;
        Ok(self.client.response_to_json(response)?)
    }

    // 获取指定图鉴详情
    pub fn fetch_book_details(&self, book_id: i32) -> Result<Value, Box<dyn std::error::Error>> {
        let endpoint = format!("/api/sprite/{}", book_id);
        let response = self
            .client
            .send_request(HttpMethod::GET, &endpoint, None, None, None)?;
        Ok(self.client.response_to_json(response)?)
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
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let method = if like {
            HttpMethod::POST
        } else {
            HttpMethod::DELETE
        };
        let endpoint = format!("/api/sprite/praise/{}", book_id);

        let response = self
            .client
            .send_request(method, &endpoint, None, None, None)?;

        if return_data {
            Ok(self.client.response_to_json(response)?)
        } else {
            Ok(json!({ "success": response.status() == 200 }))
        }
    }
}

impl Default for BookActionHandler {
    fn default() -> Self {
        Self::new()
    }
}
