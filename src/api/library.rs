use crate::utils::acquire::{ClientAccess, CodeMaoClient, HTTPStatus, HttpMethod, MewResult};
use log::debug;
use serde_json::{Value, json};

// 小说相关枚举

/// 小说列表类型
#[derive(Debug, Clone, Copy)]
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

/// 小说排序方式
#[derive(Debug, Clone, Copy)]
pub enum NovelSortId {
    Default = 0,
    MostViewed = 1,
    MostFavorited = 2,
    RecentlyUpdated = 3,
}

/// 小说分类 ID
#[derive(Debug, Clone, Copy)]
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

/// 小说连载状态
#[derive(Debug, Clone, Copy)]
pub enum NovelStatus {
    All = 0,
    Ongoing = 1,
    Completed = 2,
}

// 图鉴相关枚举

/// 图鉴星级
#[derive(Debug, Clone, Copy)]
pub enum BookStar {
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
    Five = 5,
    Six = 6,
}

/// 图鉴属性 ID
#[derive(Debug, Clone, Copy)]
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

// 漫画数据获取器

/// 漫画相关数据查询接口
pub struct CartoonDataFetcher {
    client: &'static CodeMaoClient,
}

impl CartoonDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取全部漫画列表
    pub fn fetch_all_cartoons(&self) -> MewResult<Value> {
        debug!("获取全部漫画");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/comic/list/all", None);
        self.send_and_parse(builder)
    }

    /// 获取指定漫画信息
    pub fn fetch_cartoon_info(&self, comic_id: i32) -> MewResult<Value> {
        debug!("获取漫画信息: comic_id={}", comic_id);
        let endpoint = format!("/api/comic/{}", comic_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取漫画指定章节内容(分页图片)
    pub fn fetch_cartoon_chapter(&self, chapter_id: i32) -> MewResult<Value> {
        debug!("获取漫画章节: chapter_id={}", chapter_id);
        let endpoint = format!("/api/comic/page/list/{}", chapter_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }
}

impl Default for CartoonDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for CartoonDataFetcher {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

// 小说数据获取器

/// 小说相关数据查询接口
pub struct NovelDataFetcher {
    client: &'static CodeMaoClient,
}

impl NovelDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取小说分类列表
    pub fn fetch_novel_categories(&self) -> MewResult<Value> {
        debug!("获取小说分类");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/fanfic/type", None);
        self.send_and_parse(builder)
    }

    /// 获取推荐小说
    pub fn fetch_recommend_novel(&self) -> MewResult<Value> {
        debug!("获取推荐小说");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/api/fanfic/list/recommend", None);
        self.send_and_parse(builder)
    }

    /// 获取小说列表(支持分类,排序,状态筛选)
    pub fn fetch_novel_list(
        &self,
        list_type: NovelListType,
        sort_id: NovelSortId,
        category_id: NovelCategoryId,
        status: NovelStatus,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "获取小说列表: type={:?}, sort={:?}, category={:?}, status={:?}, page={:?}, limit={:?}",
            list_type, sort_id, category_id, status, page, limit
        );
        let endpoint = format!("/api/fanfic/list/{}", list_type.as_str());
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("sort_id", (sort_id as i32).to_string())
            .with_param("type_id", (category_id as i32).to_string())
            .with_param("status", (status as i32).to_string())
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(20).to_string());
        self.send_and_parse(builder)
    }

    /// 获取已收藏的小说列表
    pub fn fetch_favorite_novels(&self, page: Option<i32>, limit: Option<i32>) -> MewResult<Value> {
        debug!("获取收藏小说: page={:?}, limit={:?}", page, limit);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/fanfic/collection", None)
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string());
        self.send_and_parse(builder)
    }

    /// 获取小说详情
    pub fn fetch_novel_details(&self, novel_id: i32) -> MewResult<Value> {
        debug!("获取小说详情: novel_id={}", novel_id);
        let endpoint = format!("/api/fanfic/{}", novel_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取指定章节内容
    pub fn fetch_chapter_details(&self, chapter_id: i32) -> MewResult<Value> {
        debug!("获取章节内容: chapter_id={}", chapter_id);
        let endpoint = format!("/api/fanfic/section/{}", chapter_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取小说评论列表
    pub fn fetch_novel_comments(
        &self,
        novel_id: i32,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "获取小说评论: novel_id={}, page={:?}, limit={:?}",
            novel_id, page, limit
        );
        let endpoint = format!("/api/fanfic/comments/list/{}", novel_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("page", page.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string());
        self.send_and_parse(builder)
    }

    /// 搜索小说
    pub fn search_novels(
        &self,
        keyword: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "搜索小说: keyword={}, page={:?}, limit={:?}",
            keyword, page, limit
        );
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/fanfic/list/search", None)
            .with_param("searchContent", keyword)
            .with_param("page", page.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(10).to_string());
        self.send_and_parse(builder)
    }

    /// 获取小说的所有章节
    pub fn fetch_all_chapters(
        &self,
        novel_id: i32,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "获取所有章节: novel_id={}, limit={:?}, page={:?}",
            novel_id, limit, page
        );
        let endpoint = format!("/web/fanfic/{}/sections", novel_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("amount_items", limit.unwrap_or(200).to_string())
            .with_param("page_number", page.unwrap_or(1).to_string());
        self.send_and_parse(builder)
    }

    /// 获取我的小说列表
    pub fn fetch_my_novels(&self, limit: Option<i32>, page: Option<i32>) -> MewResult<Value> {
        debug!("获取我的小说: limit={:?}, page={:?}", limit, page);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/fanfic/my", None)
            .with_param("amount_items", limit.unwrap_or(200).to_string())
            .with_param("page_number", page.unwrap_or(1).to_string());
        self.send_and_parse(builder)
    }

    /// 获取已删除的章节
    pub fn fetch_deleted_chapters(
        &self,
        limit: Option<i32>,
        page: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取已删除章节: limit={:?}, page={:?}", limit, page);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/web/fanfic/section/deleted", None)
            .with_param("amount_items", limit.unwrap_or(200).to_string())
            .with_param("page_number", page.unwrap_or(1).to_string());
        self.send_and_parse(builder)
    }
}

impl Default for NovelDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for NovelDataFetcher {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

// 小说操作处理器

/// 小说相关操作接口(收藏,评论,发布章节等)
pub struct NovelActionHandler {
    client: &'static CodeMaoClient,
}

impl NovelActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 收藏 / 取消收藏小说
    pub fn execute_toggle_novel_favorite(&self, novel_id: i32, favorite: bool) -> MewResult<Value> {
        let method = if favorite {
            HttpMethod::Post
        } else {
            HttpMethod::Delete
        };
        debug!("收藏操作: novel_id={}, favorite={}", novel_id, favorite);
        let endpoint = format!("/web/fanfic/collect/{}", novel_id);
        let builder = self.client.build_request(method, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 发表小说评论
    pub fn create_novel_comment(
        &self,
        content: &str,
        novel_id: i32,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("发表小说评论: novel_id={}", novel_id);
        let endpoint = format!("/api/fanfic/comments/{}", novel_id);
        let payload = json!({ "content": content });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Ok)
    }

    /// 点赞 / 取消点赞小说评论
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
        debug!("评论点赞: comment_id={}, like={}", comment_id, like);
        let endpoint = format!("/api/fanfic/comments/praise/{}", comment_id);
        let builder = self.client.build_request(method, &endpoint, None);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Ok)
    }

    /// 删除小说评论
    pub fn delete_novel_comment(&self, comment_id: i32, return_data: bool) -> MewResult<Value> {
        debug!("删除评论: comment_id={}", comment_id);
        let endpoint = format!("/api/fanfic/comments/{}", comment_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Ok)
    }

    /// 更新章节内容
    pub fn update_chapter(
        &self,
        chapter_id: i32,
        title: &str,
        content: &str,
        words_num: i32,
    ) -> MewResult<bool> {
        debug!("更新章节: chapter_id={}, title={}", chapter_id, title);
        let endpoint = format!("/web/fanfic/section/{}", chapter_id);
        let payload = json!({
            "title": title,
            "content": content,
            "draft_words_num": words_num,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 发布章节
    pub fn publish_chapter(&self, chapter_id: i32) -> MewResult<bool> {
        debug!("发布章节: chapter_id={}", chapter_id);
        let endpoint = format!("/web/fanfic/section/{}/publish", chapter_id);
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 创建章节
    pub fn create_section(
        &self,
        title: &str,
        draft: &str,
        draft_words_num: i32,
    ) -> MewResult<Value> {
        debug!("创建章节: title={}", title);
        let payload = json!({
            "title": title,
            "draft": draft,
            "draft_words_num": draft_words_num,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/fanfic/section", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 永久删除章节
    pub fn permanently_delete_section(&self, section_id: i32) -> MewResult<bool> {
        debug!("永久删除章节: section_id={}", section_id);
        let endpoint = format!("/web/fanfic/section/{}/permanently", section_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 恢复已删除章节
    pub fn recover_section(&self, section_id: i32) -> MewResult<bool> {
        debug!("恢复章节: section_id={}", section_id);
        let endpoint = format!("/web/fanfic/section/{}/recover", section_id);
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 取消发布章节
    pub fn unpublish_section(&self, section_id: i32) -> MewResult<bool> {
        debug!("取消发布章节: section_id={}", section_id);
        let endpoint = format!("/web/fanfic/section/{}/unpublish", section_id);
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 更新小说信息
    pub fn update_novel(
        &self,
        novel_id: i32,
        title: &str,
        introduction: &str,
        category_id: i32,
        status: i32,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("更新小说: novel_id={}, title={}", novel_id, title);
        let endpoint = format!("/web/fanfic/{}", novel_id);
        let payload = json!({
            "title": title,
            "introduction": introduction,
            "fanfic_type_id": category_id,
            "status": status,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Ok)
    }

    /// 创建新小说
    pub fn create_novel(
        &self,
        title: &str,
        section_title: &str,
        draft: &str,
        cover_pic: &str,
        words_num: i32,
        return_data: bool,
    ) -> MewResult<Value> {
        debug!("创建小说: title={}", title);
        let payload = json!({
            "title": title,
            "section_title": section_title,
            "draft": draft,
            "cover_pic": cover_pic,
            "draft_words_num": words_num,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/web/fanfic", None)
            .with_payload(payload);
        self.send_maybe_parse(builder, return_data, HTTPStatus::Ok)
    }

    /// 删除小说
    pub fn delete_novel(&self, novel_id: i32) -> MewResult<bool> {
        debug!("删除小说: novel_id={}", novel_id);
        let endpoint = format!("/web/fanfic/{}", novel_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None);
        self.check_status(builder, HTTPStatus::NoContent)
    }
}

impl Default for NovelActionHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for NovelActionHandler {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

// 图鉴数据获取器

/// 图鉴相关数据查询接口
pub struct BookDataFetcher {
    client: &'static CodeMaoClient,
}

impl BookDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取全部图鉴
    pub fn fetch_all_books(&self) -> MewResult<Value> {
        debug!("获取全部图鉴");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/sprite/list/all", None);
        self.send_and_parse(builder)
    }

    /// 获取所有图鉴属性列表
    pub fn fetch_all_attributes(&self) -> MewResult<Value> {
        debug!("获取图鉴属性列表");
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/sprite/factio", None);
        self.send_and_parse(builder)
    }

    /// 按星级筛选图鉴
    pub fn fetch_books_by_star(&self, star: BookStar) -> MewResult<Value> {
        debug!("按星级获取图鉴: star={:?}", star);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/sprite/list/all", None)
            .with_param("star", (star as i32).to_string());
        self.send_and_parse(builder)
    }

    /// 按属性筛选图鉴
    pub fn fetch_books_by_attribute(&self, attribute_id: BookAttributeId) -> MewResult<Value> {
        debug!("按属性获取图鉴: attribute_id={:?}", attribute_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/api/sprite/list/all", None)
            .with_param("faction_id", (attribute_id as i32).to_string());
        self.send_and_parse(builder)
    }

    /// 获取指定图鉴详情
    pub fn fetch_book_details(&self, book_id: i32) -> MewResult<Value> {
        debug!("获取图鉴详情: book_id={}", book_id);
        let endpoint = format!("/api/sprite/{}", book_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }
}

impl Default for BookDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for BookDataFetcher {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

// 图鉴操作处理器

/// 图鉴相关操作接口(点赞等)
pub struct BookActionHandler {
    client: &'static CodeMaoClient,
}

impl BookActionHandler {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 点赞 / 取消点赞图鉴
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
        debug!("图鉴点赞: book_id={}, like={}", book_id, like);
        let endpoint = format!("/api/sprite/praise/{}", book_id);
        self.send_maybe_parse(
            self.client.build_request(method, &endpoint, None),
            return_data,
            HTTPStatus::Ok,
        )
    }
}

impl Default for BookActionHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientAccess for BookActionHandler {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}
