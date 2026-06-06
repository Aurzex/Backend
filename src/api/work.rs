use crate::utils::acquire::{
    BaseKey, CodeMaoClient, HTTPStatus, HttpMethod, MewResult, PaginatedIter, PaginationMethod,
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

// ==================== 作品相关枚举 ====================

// HTTP方法选择
pub enum SelectMethod {
    Post,
    Delete,
}

impl SelectMethod {
    fn as_str(&self) -> &'static str {
        match self {
            SelectMethod::Post => "POST",
            SelectMethod::Delete => "DELETE",
        }
    }

    fn to_http_method(&self) -> HttpMethod {
        match self {
            SelectMethod::Post => HttpMethod::POST,
            SelectMethod::Delete => HttpMethod::DELETE,
        }
    }
}

// 发布状态枚举
pub enum PublishStatus {
    Published,
    Unpublished,
}

impl PublishStatus {
    fn as_str(&self) -> &'static str {
        match self {
            PublishStatus::Published => "PUBLISHED",
            PublishStatus::Unpublished => "UNPUBLISHED",
        }
    }
}

// 作品类型枚举
pub enum WorkType {
    Kitten = 1,
    Nemo = 3,
    CodeGame = 5,
}

// Kitten版本枚举
pub enum KittenVersion {
    V3,
    V4,
}

impl KittenVersion {
    fn as_str(&self) -> &'static str {
        match self {
            KittenVersion::V3 => "KITTEN_V3",
            KittenVersion::V4 => "KITTEN_V4",
        }
    }
}

// 协作作品类型枚举
pub enum CollabWorkType {
    Kitten,
    Coco,
}

impl CollabWorkType {
    fn as_str(&self) -> &'static str {
        match self {
            CollabWorkType::Kitten => "kitten",
            CollabWorkType::Coco => "coco",
        }
    }
}

// 协作权限枚举
pub enum CollabPermission {
    Edit,
    View,
}

impl CollabPermission {
    fn as_code(&self) -> i32 {
        match self {
            CollabPermission::Edit => 1,
            CollabPermission::View => 2,
        }
    }
}

// Nemo作品类型枚举
pub enum NemoWorkType {
    CourseWork,
    Template,
    Original,
    Fork,
}

impl NemoWorkType {
    fn as_str(&self) -> &'static str {
        match self {
            NemoWorkType::CourseWork => "course-work",
            NemoWorkType::Template => "template",
            NemoWorkType::Original => "original",
            NemoWorkType::Fork => "fork",
        }
    }
}

// 资源包类型枚举
pub enum ResourcePackType {
    Block,
    Character,
}

impl ResourcePackType {
    fn as_value(&self) -> i32 {
        match self {
            ResourcePackType::Block => 1,
            ResourcePackType::Character => 0,
        }
    }
}

// ==================== 基础操作类 ====================
pub struct BaseWorkOperations {
    client: &'static CodeMaoClient,
}

impl BaseWorkOperations {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 关注或取消关注用户
    pub fn execute_toggle_follow(&self, user_id: i32, method: SelectMethod) -> MewResult<bool> {
        let endpoint = format!("/nemo/v2/user/{}/follow", user_id);

        let response = self
            .client
            .build_request(method.to_http_method(), &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 收藏或取消收藏作品
    pub fn execute_toggle_collection(&self, work_id: i32, method: SelectMethod) -> MewResult<bool> {
        let endpoint = format!("/nemo/v2/works/{}/collection", work_id);

        let response = self
            .client
            .build_request(method.to_http_method(), &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 点赞或取消点赞作品
    pub fn execute_toggle_like(&self, work_id: i32, method: SelectMethod) -> MewResult<bool> {
        let endpoint = format!("/nemo/v2/works/{}/like", work_id);

        let response = self
            .client
            .build_request(method.to_http_method(), &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 再创作作品
    pub fn execute_fork_work(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/nemo/v2/works/{}/fork", work_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 分享作品
    pub fn execute_share_work(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/nemo/v2/works/{}/share", work_id);

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 举报作品
    pub fn execute_report_work(
        &self,
        work_id: i32,
        describe: &str,
        reason: &str,
    ) -> MewResult<bool> {
        let data = json!({
            "work_id": work_id,
            "report_reason": reason,
            "report_describe": describe,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/nemo/v2/report/work", None)
            .with_payload(data)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 重命名作品
    pub fn update_work_name(
        &self,
        work_id: i32,
        name: &str,
        work_type: Option<i32>,
        is_check_name: bool,
    ) -> MewResult<bool> {
        let timestamp = current_timestamp_13();
        let endpoint = format!("/work/works/{}/rename", work_id);

        let mut builder = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("is_check_name", is_check_name.to_string())
            .with_param("name", name);

        if let Some(wt) = work_type {
            builder = builder.with_param("work_type", wt.to_string());
        }

        let response = builder.send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }
}

impl Default for BaseWorkOperations {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 评论操作类 ====================
pub struct CommentOperations {
    client: &'static CodeMaoClient,
}

impl CommentOperations {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 添加作品评论
    pub fn create_work_comment(
        &self,
        work_id: i32,
        comment: &str,
        emoji: Option<&str>,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!("/creation-tools/v1/works/{}/comment", work_id);

        let mut payload_map = serde_json::Map::new();
        payload_map.insert("content".to_string(), Value::String(comment.to_string()));
        payload_map.insert(
            "emoji_content".to_string(),
            Value::String(emoji.unwrap_or("").to_string()),
        );

        let payload = Value::Object(payload_map);

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

    // 回复作品评论
    pub fn create_comment_reply(
        &self,
        comment: &str,
        work_id: i32,
        comment_id: i32,
        parent_id: Option<i32>,
        return_data: bool,
    ) -> MewResult<Value> {
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}/reply",
            work_id, comment_id
        );

        let data = json!({
            "parent_id": parent_id.unwrap_or(0),
            "content": comment,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(data)
            .send()?;

        if return_data {
            self.client.response_to_json(response)
        } else {
            Ok(json!({ "success": response.status() == HTTPStatus::Created as u16 }))
        }
    }

    // 删除作品评论
    pub fn delete_comment(&self, work_id: i32, comment_id: i32) -> MewResult<bool> {
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}",
            work_id, comment_id
        );

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, None)
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 置顶或取消置顶评论
    pub fn execute_toggle_comment_pin(
        &self,
        method: HttpMethod,
        work_id: i32,
        comment_id: i32,
    ) -> MewResult<bool> {
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}/top",
            work_id, comment_id
        );

        let response = self
            .client
            .build_request(method, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 点赞或取消点赞评论
    pub fn execute_toggle_comment_like(
        &self,
        work_id: i32,
        comment_id: i32,
        method: SelectMethod,
    ) -> MewResult<bool> {
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}/liked",
            work_id, comment_id
        );

        let response = self
            .client
            .build_request(method.to_http_method(), &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Created as u16)
    }

    // 举报作品评论
    pub fn execute_report_comment(
        &self,
        work_id: i32,
        comment_id: i32,
        reason: &str,
    ) -> MewResult<bool> {
        let endpoint = format!("/creation-tools/v1/works/{}/comment/report", work_id);

        let data = json!({
            "comment_id": comment_id,
            "report_reason": reason,
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(data)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }
}

impl Default for CommentOperations {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== KITTEN 作品管理类 ====================
pub struct KittenWorkManager {
    client: &'static CodeMaoClient,
    pub operations: BaseWorkOperations,
    pub comments: CommentOperations,
}

impl KittenWorkManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
            operations: BaseWorkOperations::new(),
            comments: CommentOperations::new(),
        }
    }

    // 创建 Kitten 作品
    pub fn create_kitten_work(
        &self,
        name: &str,
        work_url: &str,
        preview: &str,
        version: &str,
        orientation: Option<i32>,
        sample_id: Option<&str>,
        work_source_label: Option<i32>,
        save_type: Option<i32>,
    ) -> MewResult<Value> {
        let mut payload_map = serde_json::Map::new();
        payload_map.insert("name".to_string(), Value::String(name.to_string()));
        payload_map.insert("work_url".to_string(), Value::String(work_url.to_string()));
        payload_map.insert("preview".to_string(), Value::String(preview.to_string()));
        payload_map.insert(
            "orientation".to_string(),
            Value::Number(serde_json::Number::from(orientation.unwrap_or(1))),
        );
        payload_map.insert(
            "sample_id".to_string(),
            Value::String(sample_id.unwrap_or("").to_string()),
        );
        payload_map.insert("version".to_string(), Value::String(version.to_string()));
        payload_map.insert(
            "work_source_label".to_string(),
            Value::Number(serde_json::Number::from(work_source_label.unwrap_or(1))),
        );
        payload_map.insert(
            "save_type".to_string(),
            Value::Number(serde_json::Number::from(save_type.unwrap_or(2))),
        );

        let payload = Value::Object(payload_map);

        let response = self
            .client
            .build_request(HttpMethod::POST, "/kitten/r2/work", Some(BaseKey::Creation))
            .with_payload(payload)
            .send()?;

        self.client.response_to_json(response)
    }

    // 发布 Kitten 作品
    pub fn execute_publish_kitten_work(
        &self,
        work_id: i32,
        name: &str,
        description: &str,
        operation: &str,
        labels: Vec<Value>,
        cover_url: &str,
        bcmc_url: &str,
        work_url: &str,
        fork_enable: i32,
        if_default_cover: i32,
        version: &str,
        cover_type: Option<i32>,
        user_labels: Option<Vec<Value>>,
    ) -> MewResult<bool> {
        let endpoint = format!("/kitten/r2/work/{}/publish", work_id);

        let payload = json!({
            "name": name,
            "description": description,
            "operation": operation,
            "labels": labels,
            "cover_url": cover_url,
            "bcmc_url": bcmc_url,
            "work_url": work_url,
            "fork_enable": fork_enable,
            "if_default_cover": if_default_cover,
            "version": version,
            "cover_type": cover_type.unwrap_or(1),
            "user_labels": user_labels.unwrap_or_default(),
        });

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 删除未发布的 Kitten 作品草稿
    pub fn delete_kitten_draft(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/kitten/common/work/{}/temporarily", work_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 取消发布作品
    pub fn execute_unpublish_work(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/tiger/work/{}/unpublish", work_id);

        let response = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 通过 Web 端取消发布作品
    pub fn execute_unpublish_work_web(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/web/works/r2/unpublish/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 清空 Kitten 作品回收站
    pub fn execute_empty_kitten_trash(&self) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::DELETE,
                "/work/user/works/permanently",
                Some(BaseKey::Creation),
            )
            .send()?;

        Ok(response.status() == HTTPStatus::NoContent as u16)
    }

    // 翻译 Kitten 作品
    pub fn translate_kitten_work(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/kitten/work/translate",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for KittenWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== NEKO (Kitten N) 作品管理类 ====================
pub struct NekoWorkManager {
    client: &'static CodeMaoClient,
    pub operations: BaseWorkOperations,
    pub comments: CommentOperations,
}

impl NekoWorkManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
            operations: BaseWorkOperations::new(),
            comments: CommentOperations::new(),
        }
    }

    // 创建 KN 作品
    pub fn create_kn_work(
        &self,
        name: &str,
        work_url: &str,
        preview_url: &str,
        bcm_version: &str,
        save_type: Option<i32>,
        stage_type: Option<i32>,
        n_blocks: Option<i32>,
        n_roles: Option<i32>,
        n_scenes: Option<i32>,
        pic_need_check_file_url: Option<&str>,
    ) -> MewResult<Value> {
        let payload = json!({
            "bcm_version": bcm_version,
            "save_type": save_type.unwrap_or(2),
            "name": name,
            "work_url": work_url,
            "preview_url": preview_url,
            "stage_type": stage_type.unwrap_or(2),
            "n_blocks": n_blocks.unwrap_or(0),
            "n_roles": n_roles.unwrap_or(2),
            "n_scenes": n_scenes.unwrap_or(1),
            "pic_need_check_file_url": pic_need_check_file_url.unwrap_or(""),
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/neko/works", Some(BaseKey::Creation))
            .with_payload(payload)
            .send()?;

        self.client.response_to_json(response)
    }

    // 发布 KN 作品
    pub fn execute_publish_kn_work(
        &self,
        work_id: i32,
        name: &str,
        preview_url: &str,
        description: &str,
        operation: &str,
        fork_enable: i32,
        if_default_cover: i32,
        bcmc_url: &str,
        work_url: &str,
        bcm_version: &str,
        cover_url: Option<&str>,
    ) -> MewResult<bool> {
        let endpoint = format!("/neko/community/work/publish/{}", work_id);

        let payload = json!({
            "name": name,
            "preview_url": preview_url,
            "description": description,
            "operation": operation,
            "fork_enable": fork_enable,
            "if_default_cover": if_default_cover,
            "bcmc_url": bcmc_url,
            "work_url": work_url,
            "bcm_version": bcm_version,
            "cover_url": cover_url.unwrap_or(""),
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, Some(BaseKey::Creation))
            .with_payload(payload)
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 删除未发布的 KN 作品草稿
    pub fn delete_kn_draft(&self, work_id: i32, force: i32) -> MewResult<bool> {
        let timestamp = current_timestamp_13();
        let endpoint = format!("/neko/works/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("force", force.to_string())
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 取消发布 KN 作品
    pub fn execute_unpublish_kn_work(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/neko/community/work/unpublish/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 清空 KN 作品回收站
    pub fn execute_empty_kn_trash(&self) -> MewResult<bool> {
        let response = self
            .client
            .build_request(
                HttpMethod::DELETE,
                "/neko/works/permanently",
                Some(BaseKey::Creation),
            )
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 恢复 KN 作品回收站作品
    pub fn execute_recover_kn_trash(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/neko/works/{}/recover", work_id);

        let response = self
            .client
            .build_request(HttpMethod::PATCH, &endpoint, Some(BaseKey::Creation))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 保存教师作品
    pub fn save_teacher_work(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/works/teacher",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 复制作品
    pub fn copy_work(&self, work_id: i32) -> MewResult<Value> {
        let data = json!({ "work_id": work_id });

        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/works/copy",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 作品图片故障排查
    pub fn troubleshoot_work_pics(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/works/pic-troubleshoot/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for NekoWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== WOOD (海龟编辑器) 作品管理类 ====================
pub struct WoodWorkManager {
    client: &'static CodeMaoClient,
}

impl WoodWorkManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取海龟编辑器项目信息
    pub fn fetch_wood_project(&self, work_id: i32) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/wood/project", Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 创建海龟编辑器作品
    pub fn create_wood_project(
        &self,
        work_name: Option<&str>,
        language_type: Option<i32>,
        run_mode: Option<i32>,
        files: Option<Vec<Value>>,
        preview_code: Option<&str>,
        preview_url: Option<&str>,
        is_turn_on_debug: Option<bool>,
        editor_mode: Option<&str>,
        update_time: Option<i32>,
    ) -> MewResult<Value> {
        let payload = json!({
            "work_name": work_name.unwrap_or("新的作品"),
            "language_type": language_type.unwrap_or(3),
            "run_mode": run_mode.unwrap_or(0),
            "update_time": update_time.unwrap_or(0),
            "addition": {
                "readonly_paths": [],
                "locking_file_lines": {},
                "isTurnOnDebug": is_turn_on_debug.unwrap_or(true),
                "editorMode": editor_mode.unwrap_or("code"),
            },
            "files": files.unwrap_or_default(),
            "preview_url": preview_url.unwrap_or(""),
            "preview_code": preview_code.unwrap_or(""),
        });

        let response = self
            .client
            .build_request(HttpMethod::POST, "/wood/project", Some(BaseKey::Creation))
            .with_payload(payload)
            .send()?;

        self.client.response_to_json(response)
    }

    // 删除海龟编辑器草稿
    pub fn delete_wood_draft(&self, work_id: i32) -> MewResult<bool> {
        let endpoint = format!("/wood/project/{}/temporarily", work_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 搜索用户的Wood作品
    pub fn search_user_wood_projects(
        &self,
        query: Option<&str>,
        page: Option<i32>,
        limit: Option<i32>,
        language_type: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/wood/user/project/search",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("query", query.unwrap_or(""))
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("language_type", language_type.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 在海龟编辑器作品中创建文件
    pub fn create_wood_file(
        &self,
        work_id: i32,
        file_name: Option<&str>,
        source_code: Option<&str>,
        file_type: Option<i32>,
        is_open: bool,
    ) -> MewResult<Value> {
        // 先获取现有项目
        let project = self.fetch_wood_project(work_id)?;

        let mut files = project["files"]
            .as_array()
            .cloned()
            .unwrap_or_else(std::vec::Vec::new);

        let file_data = json!({
            "work_id": work_id,
            "file_id": -1,
            "file_name": file_name.unwrap_or("main.py"),
            "source": source_code.unwrap_or(""),
            "open": is_open,
            "pid": 0,
            "file_type": file_type.unwrap_or(2),
        });

        files.push(file_data);

        // 更新项目
        self.create_wood_project(
            Some(project["work_name"].as_str().unwrap_or("新的作品")),
            Some(project["language_type"].as_i64().unwrap_or(3) as i32),
            Some(project["run_mode"].as_i64().unwrap_or(0) as i32),
            Some(files),
            project["preview_code"].as_str(),
            project["preview_url"].as_str(),
            Some(
                project["addition"]["isTurnOnDebug"]
                    .as_bool()
                    .unwrap_or(true),
            ),
            project["addition"]["editorMode"].as_str(),
            Some(project["update_time"].as_i64().unwrap_or(0) as i32),
        )
    }
}

impl Default for WoodWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== COCO (Coconut) 平台管理类 ====================
pub struct CocoWorkManager {
    client: &'static CodeMaoClient,
}

impl CocoWorkManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取 Coco 平台的主要课程列表
    pub fn fetch_coco_primary_courses(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/primary-course/list",
                Some(BaseKey::Creation),
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Coco 的自定义控件列表生成器
    pub fn fetch_custom_widgets_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/coconut/web/widget/list")
            .with_param("TIME", timestamp.to_string())
            .with_param("current_page", "1")
            .with_param("page_size", "100")
            .with_total_key("data.total")
            .with_data_key("data.items")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("page_size")
            .with_offset_key("current_page")
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(100);
        }

        paginated
    }

    // 获取 Coco 的示范教程列表
    pub fn fetch_demo_courses(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/sample/list",
                Some(BaseKey::Creation),
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Coco 的白名单作品链接
    pub fn fetch_whitelisted_works(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "https://static.bcmcdn.com/coco/whitelist.json",
                None,
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Coco 的 web 控件
    pub fn fetch_web_widget(&self, page: Option<i32>, page_size: Option<i32>) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/coconut/web/user/widget/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("current_page", page.unwrap_or(1).to_string())
            .with_param("page_size", page_size.unwrap_or(100).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 更新 Coco 作品
    pub fn execute_update_coco_work(
        &self,
        work_id: i32,
        work_name: &str,
        bcm_url: &str,
        preview_url: &str,
        archive_version: Option<&str>,
        save_type: Option<i32>,
    ) -> MewResult<Value> {
        let data = json!({
            "id": work_id,
            "name": work_name,
            "preview_url": preview_url,
            "bcm_url": bcm_url,
            "archive_version": archive_version.unwrap_or("0.1.0"),
            "save_type": save_type.unwrap_or(1),
        });

        let response = self
            .client
            .build_request(
                HttpMethod::PUT,
                "/coconut/web/work",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 发布 Coco 作品
    pub fn execute_publish_coco_work(
        &self,
        work_id: i32,
        work_name: &str,
        bcmc_url: &str,
        cover_url: &str,
        description: &str,
        operation: &str,
    ) -> MewResult<Value> {
        let endpoint = format!("/coconut/web/work/{}/publish", work_id);

        let data = json!({
            "name": work_name,
            "description": description,
            "operation": operation,
            "cover_url": cover_url,
            "bcmc_url": bcmc_url,
            "player_url": format!("https://coco.codemao.cn/editor/player/{}?channel=community", work_id),
        });

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for CocoWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 协作功能管理类 ====================
pub struct CollaborationManager {
    client: &'static CodeMaoClient,
}

impl CollaborationManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取或删除 Kitten 协作邀请码
    pub fn fetch_kitten_collaboration_code(
        &self,
        work_id: i32,
        method: HttpMethod,
    ) -> MewResult<Value> {
        let endpoint = format!(
            "https://socketcoll.codemao.cn/coll/kitten/collaborator/code/{}",
            work_id
        );

        let response = self.client.build_request(method, &endpoint, None).send()?;

        self.client.response_to_json(response)
    }

    // 获取 Coco 协作邀请码
    pub fn fetch_coco_collaboration_code(
        &self,
        work_id: i32,
        permission: CollabPermission,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let endpoint = format!(
            "https://socketcoll.codemao.cn/coll/coco/collaborator/code/{}",
            work_id
        );

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_param("edit_permission", permission.as_code().to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取协作者列表生成器
    pub fn fetch_collaborators_gen(
        &self,
        work_type: CollabWorkType,
        work_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();
        let endpoint = format!(
            "https://socketcoll.codemao.cn/coll/{}/collaborator/{}",
            work_type.as_str(),
            work_id
        );

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_param("TIME", timestamp.to_string())
            .with_param("current_page", "1")
            .with_param("page_size", "100")
            .with_total_key("data.total")
            .with_data_key("data.items")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("page_size")
            .with_offset_key("current_page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(100);
        }

        paginated
    }

    // 获取协作状态
    pub fn fetch_collaboration_status(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/collaboration/user/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取协作用户
    pub fn fetch_collaboration_user(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/collaboration/user/edited/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 启用 Kitten/Coco 作品协作功能
    pub fn execute_enable_collaboration(
        &self,
        work_id: i32,
        work_type: CollabWorkType,
    ) -> MewResult<bool> {
        let endpoint = format!(
            "https://socketcoll.codemao.cn/coll/{}/{}",
            work_type.as_str(),
            work_id
        );

        let response = self
            .client
            .build_request(HttpMethod::POST, &endpoint, None)
            .with_payload(json!({}))
            .send()?;

        Ok(response.status() == HTTPStatus::Ok as u16)
    }

    // 获取协作的 Coco 作品生成器
    pub fn fetch_collaboration_coco_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("https://socketcoll.codemao.cn/coll/coco/coll_works")
            .with_param("TIME", timestamp.to_string())
            .with_param("current_page", "1")
            .with_param("page_size", "40")
            .with_total_key("data.total")
            .with_data_key("data.items")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("page_size")
            .with_offset_key("current_page");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(40);
        }

        paginated
    }
}

impl Default for CollaborationManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== AI 服务类 ====================
pub struct AIServices {
    client: &'static CodeMaoClient,
}

impl AIServices {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取文生图提示词
    pub fn fetch_text2img_prompt(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/text2img/prompt",
                Some(BaseKey::Creation),
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 AI 绘画模板
    pub fn fetch_ai_painting_templates(&self, template_type: &str) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/ai-painting/templates",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", template_type)
            .send()?;

        self.client.response_to_json(response)
    }

    // AI 绘画匹配
    pub fn match_ai_painting(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/ai-painting/match",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 添加到灵感池
    pub fn add_to_inspiration_pool(
        &self,
        img_url: &str,
        prompt: &str,
        style: &str,
        img_type: &str,
        generation_type: &str,
    ) -> MewResult<Value> {
        let data = json!({
            "img_url": img_url,
            "prompt": prompt,
            "style": style,
            "img_type": img_type,
            "generation_type": generation_type,
        });

        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/inspiration-pool",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for AIServices {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 教学计划管理类 ====================
pub struct TeachingPlanManager {
    client: &'static CodeMaoClient,
}

impl TeachingPlanManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 保存团队作品 (教学计划)
    pub fn save_team_work(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/teaching-plan/save/team/work",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取教学计划操作日志
    pub fn fetch_teaching_plan_logs(
        &self,
        work_id: i32,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/teaching-plan/list/opr/log",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(20).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 添加教学计划操作日志
    pub fn add_teaching_plan_log(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/teaching-plan/add/opr/log",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取作品编辑状态
    pub fn fetch_work_editing_status(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/teaching-plan/work/editing-status/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 设置作品编辑状态
    pub fn set_work_editing_status(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/teaching-plan/set/work/editing-status",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 更新课程进度
    pub fn update_course_progress(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/course/user/progress",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 提交课程作品
    pub fn submit_course_work(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/course/user/course-work",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 保存教师课程邀请链接
    pub fn save_teacher_course_invite_url(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/works/save-teacher-course-invite-url",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for TeachingPlanManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 图像分类管理类 ====================
pub struct ImageClassifyManager {
    client: &'static CodeMaoClient,
}

impl ImageClassifyManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取图像分类列表
    pub fn fetch_image_classify_list(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/image-classify/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.unwrap_or(20).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 提交图像分类
    pub fn submit_image_classify(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::POST,
                "/neko/image-classify",
                Some(BaseKey::Creation),
            )
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 更新图像分类
    pub fn update_image_classify(&self, classify_id: &str, data: Value) -> MewResult<Value> {
        let endpoint = format!("/neko/image-classify/{}", classify_id);

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 删除图像分类
    pub fn delete_image_classify(&self, classify_id: &str) -> MewResult<Value> {
        let endpoint = format!("/neko/image-classify/{}", classify_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for ImageClassifyManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 包管理类 ====================
pub struct PackageManager {
    client: &'static CodeMaoClient,
}

impl PackageManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取包列表
    pub fn fetch_package_list(
        &self,
        package_type: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/package/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", package_type)
            .with_param("limit", limit.unwrap_or(20).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 创建包
    pub fn create_package(&self, data: Value) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::POST, "/neko/package", Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 更新包信息
    pub fn update_package(
        &self,
        package_id: &str,
        name: &str,
        description: &str,
    ) -> MewResult<Value> {
        let endpoint = format!("/neko/package/{}", package_id);

        let data = json!({
            "name": name,
            "description": description,
        });

        let response = self
            .client
            .build_request(HttpMethod::PUT, &endpoint, Some(BaseKey::Creation))
            .with_payload(data)
            .send()?;

        self.client.response_to_json(response)
    }

    // 删除包
    pub fn delete_package(&self, package_id: &str) -> MewResult<Value> {
        let endpoint = format!("/neko/package/{}", package_id);

        let response = self
            .client
            .build_request(HttpMethod::DELETE, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for PackageManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 示例管理类 ====================
pub struct SampleManager {
    client: &'static CodeMaoClient,
}

impl SampleManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // 获取 Kitten N 示例详情
    pub fn fetch_sample_detail(&self, params: Vec<(String, String)>) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/sample/detail",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string());

        for (key, value) in params {
            builder = builder.with_param(key, value);
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // 获取示例列表
    pub fn fetch_sample_list(&self, subject_id: &str) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/sample/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("subject_id", subject_id)
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for SampleManager {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== 作品数据获取类 ====================
pub struct WorkDataFetcher {
    client: &'static CodeMaoClient,
}

impl WorkDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    // ---------- 作品详情 ----------

    // 获取作品详细信息
    pub fn fetch_work_details(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/creation-tools/v1/works/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Kitten 作品详细信息
    pub fn fetch_kitten_work_details(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/kitten/work/detail/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 KN 作品详细信息
    pub fn fetch_kn_work_details(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/works/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Coco 作品信息
    pub fn fetch_coco_work_info(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/coconut/web/work/{}/info", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 KN 作品发布状态
    pub fn fetch_kn_publish_status(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/community/work/detail/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 KN 作品状态
    pub fn fetch_kn_work_state(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/works/status/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 KN 作品详情
    pub fn fetch_kn_work_detail(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/community/player/published-work-detail/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取玩家作品详情
    pub fn fetch_player_work_detail(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/works/player/work-detail/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 通过课程代码获取作品
    pub fn fetch_work_by_course_code(&self, course_code: &str) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/works/get-player-by-course-code",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("course_code", course_code)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取作品状态
    pub fn fetch_work_status(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/works/status/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取作品参加的活动信息
    pub fn fetch_work_activity(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/works/activity/info/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 检查用户操作状态
    pub fn check_user_operation_status(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/community/check-user-opr-work-status/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 评论相关 ----------

    // 获取作品评论生成器
    pub fn fetch_work_comments_gen(&self, work_id: i32, limit: Option<usize>) -> PaginatedIter {
        let timestamp = current_timestamp_13();
        let endpoint = format!("/creation-tools/v1/works/{}/comments", work_id);

        let mut paginated = self
            .client
            .paginated(&endpoint)
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", "15")
            .with_param("offset", "0")
            .with_total_key("page_total");

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(15);
        }

        paginated
    }

    // ---------- 源代码 ----------

    // 获取作品源代码
    pub fn fetch_work_source_code(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/creation-tools/v1/works/{}/source/public", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Kitten 作品源代码
    pub fn fetch_kitten_source_code(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/kitten/work/ide/load/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 游玩端 Kitten 作品代码
    pub fn fetch_kitten_player_code(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/kitten/r2/work/player/load/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Coco 作品源代码
    pub fn fetch_coco_source_code(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/coconut/web/work/{}/content", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 游玩端 Coco 作品代码
    pub fn fetch_coco_player_code(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/coconut/web/work/{}/load", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 游玩端 Wood 作品代码
    pub fn fetch_wood_player_code(&self, work_id: i32) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let endpoint = format!("/wood/work/{}/publish", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("channel_type", "0")
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 KN 作品历史版本
    pub fn fetch_kn_work_versions(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/neko/works/archive/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 作品列表和推荐 ----------

    // 获取 Web 端相关作品推荐
    pub fn fetch_web_recommendations(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/nemo/v2/works/web/{}/recommended", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Nemo 端相关作品推荐
    pub fn fetch_nemo_recommendations(&self, work_id: i32) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/nemo/v3/work-details/recommended/list",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Web 端最新作品
    pub fn fetch_new_works_web(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
        origin: bool,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/pc/discover/newest-work",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string());

        if origin {
            builder = builder.with_param("work_origin_type", "ORIGINAL_WORK");
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // 获取 Web 端主题作品
    pub fn fetch_themed_works_web(
        &self,
        limit: i32,
        offset: Option<i32>,
        subject_id: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let mut builder = self
            .client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/pc/discover/subject-work",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.unwrap_or(0).to_string());

        if let Some(sid) = subject_id {
            builder = builder.with_param("subject_id", sid.to_string());
        }

        let response = builder.send()?;
        self.client.response_to_json(response)
    }

    // 获取 Nemo 端发现页作品
    pub fn fetch_nemo_discover(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/creation-tools/v1/home/discover", None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Nemo 端最新作品
    pub fn fetch_new_works_nemo(
        &self,
        types: NemoWorkType,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let endpoint = format!("/nemo/v3/newest/work/{}/list", types.as_str());

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取动态作品
    pub fn fetch_activity_feed(&self, limit: Option<i32>, offset: Option<i32>) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v3/work/dynamic", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取动态推荐用户
    pub fn fetch_recommended_users(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/nemo/v3/dynamic/focus/user/recommend",
                None,
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 主题相关 ----------

    // 获取随机作品主题 ID 列表
    pub fn fetch_random_subjects(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v3/work-subject/random", None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取主题详细信息
    pub fn fetch_subject_details(&self, ids: i32) -> MewResult<Value> {
        let endpoint = format!("/nemo/v3/work-subject/{}/info", ids);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取主题下作品
    pub fn fetch_subject_works(
        &self,
        ids: i32,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();
        let endpoint = format!("/nemo/v3/work-subject/{}/works", ids);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取所有主题作品
    pub fn fetch_all_subject_works(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v3/work-subject/home", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 作品谱系 ----------

    // 获取 Web 端作品谱系
    pub fn fetch_work_lineage_web(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/tiger/work/tree/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Nemo 端作品谱系
    pub fn fetch_work_lineage_nemo(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/nemo/v2/works/root/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 回收站 ----------

    // 获取 Kitten 回收站作品生成器
    pub fn fetch_kitten_trash_gen(
        &self,
        version: KittenVersion,
        work_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/tiger/work/recycle/list")
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", "30")
            .with_param("offset", "0")
            .with_param("version_no", version.as_str())
            .with_param("work_status", work_status.unwrap_or("CYCLED"))
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取海龟编辑器回收站作品生成器
    pub fn fetch_wood_trash_gen(
        &self,
        language_type: Option<i32>,
        work_status: Option<&str>,
        published_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/wood/comm/work/list")
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", "30")
            .with_param("offset", "0")
            .with_param("language_type", language_type.unwrap_or(0).to_string())
            .with_param("work_status", work_status.unwrap_or("CYCLED"))
            .with_param("published_status", published_status.unwrap_or("undefined"))
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取代码岛回收站作品生成器
    pub fn fetch_box_trash_gen(
        &self,
        work_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/box/v2/work/list")
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", "30")
            .with_param("offset", "0")
            .with_param("work_status", work_status.unwrap_or("CYCLED"))
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取小说回收站生成器
    pub fn fetch_fiction_trash_gen(
        &self,
        fiction_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/web/fanfic/my/new")
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", "30")
            .with_param("offset", "0")
            .with_param("fiction_status", fiction_status.unwrap_or("CYCLED"));

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(30);
        }

        paginated
    }

    // 获取 KN 回收站作品生成器
    pub fn fetch_kn_trash_gen(
        &self,
        name: Option<&str>,
        work_business_classify: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/neko/works/v2/list/user")
            .with_param("TIME", timestamp.to_string())
            .with_param("name", name.unwrap_or(""))
            .with_param("limit", "24")
            .with_param("offset", "0")
            .with_param("status", "-99")
            .with_param(
                "work_business_classify",
                work_business_classify.unwrap_or(1).to_string(),
            )
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(24);
        }

        paginated
    }

    // ---------- 搜索 ----------

    // 搜索 KN 作品生成器
    pub fn search_kn_works_gen(
        &self,
        name: &str,
        status: Option<i32>,
        work_business_classify: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/neko/works/v2/list/user")
            .with_param("TIME", timestamp.to_string())
            .with_param("name", name)
            .with_param("limit", "24")
            .with_param("offset", "0")
            .with_param("status", status.unwrap_or(1).to_string())
            .with_param(
                "work_business_classify",
                work_business_classify.unwrap_or(1).to_string(),
            )
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(24);
        }

        paginated
    }

    // 搜索已发布 KN 作品生成器
    pub fn search_published_kn_works_gen(
        &self,
        name: &str,
        work_business_classify: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        let timestamp = current_timestamp_13();

        let mut paginated = self
            .client
            .paginated("/neko/works/list/user/published")
            .with_param("TIME", timestamp.to_string())
            .with_param("name", name)
            .with_param("limit", "24")
            .with_param("offset", "0")
            .with_param(
                "work_business_classify",
                work_business_classify.unwrap_or(1).to_string(),
            )
            .with_base_key(BaseKey::Creation);

        if let Some(limit_val) = limit {
            paginated = paginated.with_limit(limit_val);
        } else {
            paginated = paginated.with_limit(24);
        }

        paginated
    }

    // 通过名称搜索作品
    pub fn search_works_by_name_web(
        &self,
        name: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/community/work/name/search", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("query", name)
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(20).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 通过名称搜索作品 (版本 2)
    pub fn search_works_by_name_nemo(
        &self,
        name: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/nemo/v2/work/name/search", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("key", name)
            .with_param("offset", offset.unwrap_or(0).to_string())
            .with_param("limit", limit.unwrap_or(20).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 标签和元数据 ----------

    // 获取作品元数据
    pub fn fetch_work_metadata(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/api/work/info/{}", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取作品标签
    pub fn fetch_work_tags(&self, work_id: i32) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/creation-tools/v1/work-details/work-labels",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取所有 Kitten 作品标签
    pub fn fetch_kitten_tags(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/kitten/work/labels",
                Some(BaseKey::Creation),
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 Kitten 默认封面
    pub fn fetch_kitten_default_covers(&self) -> MewResult<Value> {
        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/kitten/work/cover/defaultCovers",
                Some(BaseKey::Creation),
            )
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取作品最近使用的封面
    pub fn fetch_recent_covers(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!("/kitten/work/cover/{}/recentCovers", work_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, Some(BaseKey::Creation))
            .send()?;

        self.client.response_to_json(response)
    }

    // 验证作品名称是否可用
    pub fn validate_work_name(&self, name: &str, work_id: i32) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/tiger/work/checkname", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("name", name)
            .with_param("work_id", work_id.to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 作者相关 ----------

    // 获取作者作品集
    pub fn fetch_author_portfolio(&self, user_id: i32) -> MewResult<Value> {
        let endpoint = format!("/web/works/users/{}", user_id);

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // ---------- 其他 ----------

    // 根据喵口令获取作品数据
    pub fn fetch_work_by_miao_code(&self, token: &str) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(HttpMethod::GET, "/tiger/nemo/miao-codes", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("token", token)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取 KN 作品变量列表
    pub fn fetch_kn_variables(&self, work_id: i32) -> MewResult<Value> {
        let endpoint = format!(
            "https://socketcv.codemao.cn/neko/cv/list/variables/{}",
            work_id
        );

        let response = self
            .client
            .build_request(HttpMethod::GET, &endpoint, None)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取积木或角色资源包
    pub fn fetch_resource_pack(
        &self,
        types: ResourcePackType,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/package/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", types.as_value().to_string())
            .with_param("limit", limit.unwrap_or(16).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取素材分类
    pub fn fetch_material_categories(&self, material_type: &str) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/material/categories",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", material_type)
            .send()?;

        self.client.response_to_json(response)
    }

    // 获取素材列表
    pub fn fetch_material_list(
        &self,
        second_id: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        let timestamp = current_timestamp_13();

        let response = self
            .client
            .build_request(
                HttpMethod::GET,
                "/neko/material/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("second_id", second_id)
            .with_param("limit", limit.unwrap_or(20).to_string())
            .with_param("offset", offset.unwrap_or(0).to_string())
            .send()?;

        self.client.response_to_json(response)
    }
}

impl Default for WorkDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}
