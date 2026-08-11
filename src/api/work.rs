use crate::utils::acquire::{
    BaseKey, ClientAccess, CodeMaoClient, DEFAULT_LIMIT, DEFAULT_PAGE_SIZE, HTTPStatus, HttpMethod,
    MewResult, PaginatedIter, PaginationMethod, ResponseMode, ToggleAction, current_timestamp_13,
};

/// 萌新盒子套餐列表接口的单页上限
const NEKO_PACKAGE_PAGE_SIZE: usize = 16;
use log::debug;
use serde_json::{Value, json};

// 工具函数

// 作品相关枚举

/// 作品类型
#[derive(Debug, Clone, Copy)]
pub enum WorkType {
    Kitten = 1,
    Nemo = 3,
    CodeGame = 5,
}

/// Kitten 版本
#[derive(Debug, Clone, Copy)]
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

/// 协作作品类型
#[derive(Debug, Clone, Copy)]
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

/// 协作权限
#[derive(Debug, Clone, Copy)]
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

/// Nemo 作品类型
#[derive(Debug, Clone, Copy)]
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

/// 资源包类型
#[derive(Debug, Clone, Copy)]
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

// 基础操作类

/// 基础作品操作接口(关注,收藏,点赞,再创作,分享,举报,重命名)
pub struct BaseWorkOperations {
    client: &'static CodeMaoClient,
}

impl BaseWorkOperations {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 关注或取消关注用户
    pub fn toggle_follow(&self, user_id: i32, action: ToggleAction) -> MewResult<bool> {
        debug!("切换关注状态: user_id={}, action={:?}", user_id, action);
        let endpoint = format!("/nemo/v2/user/{}/follow", user_id);
        let builder = self
            .client
            .build_request(
                action.to_http_method(HttpMethod::Post, HttpMethod::Delete),
                &endpoint,
                None,
            )
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 收藏或取消收藏作品
    pub fn toggle_collection(&self, work_id: i32, action: ToggleAction) -> MewResult<bool> {
        debug!("切换收藏状态: work_id={}, action={:?}", work_id, action);
        let endpoint = format!("/nemo/v2/works/{}/collection", work_id);
        let builder = self
            .client
            .build_request(
                action.to_http_method(HttpMethod::Post, HttpMethod::Delete),
                &endpoint,
                None,
            )
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 点赞或取消点赞作品
    pub fn toggle_like(&self, work_id: i32, action: ToggleAction) -> MewResult<bool> {
        debug!("切换点赞状态: work_id={}, action={:?}", work_id, action);
        let endpoint = format!("/nemo/v2/works/{}/like", work_id);
        let builder = self
            .client
            .build_request(
                action.to_http_method(HttpMethod::Post, HttpMethod::Delete),
                &endpoint,
                None,
            )
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 再创作作品
    pub fn fork_work(&self, work_id: i32) -> MewResult<bool> {
        debug!("再创作作品: work_id={}", work_id);
        let endpoint = format!("/nemo/v2/works/{}/fork", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 分享作品
    pub fn share_work(&self, work_id: i32) -> MewResult<bool> {
        debug!("分享作品: work_id={}", work_id);
        let endpoint = format!("/nemo/v2/works/{}/share", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 举报作品
    pub fn report_work(&self, work_id: i32, describe: &str, reason: &str) -> MewResult<bool> {
        debug!("举报作品: work_id={}, reason={}", work_id, reason);
        let data = json!({
            "work_id": work_id,
            "report_reason": reason,
            "report_describe": describe,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/nemo/v2/report/work", None)
            .with_payload(data);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 重命名作品
    pub fn update_work_name(
        &self,
        work_id: i32,
        name: &str,
        work_type: Option<i32>,
        is_check_name: bool,
    ) -> MewResult<bool> {
        debug!("重命名作品: work_id={}, name={}", work_id, name);
        let timestamp = current_timestamp_13();
        let endpoint = format!("/work/works/{}/rename", work_id);
        let mut builder = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("is_check_name", is_check_name.to_string())
            .with_param("name", name);
        if let Some(wt) = work_type {
            builder = builder.with_param("work_type", wt.to_string());
        }
        self.check_status(builder, HTTPStatus::Ok)
    }
}

impl Default for BaseWorkOperations {
    fn default() -> Self {
        Self::new()
    }
}

// 评论操作类

/// 作品评论操作接口
pub struct CommentOperations {
    client: &'static CodeMaoClient,
}

impl CommentOperations {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 添加作品评论
    pub fn create_work_comment(
        &self,
        work_id: i32,
        comment: &str,
        emoji: Option<&str>,
        mode: ResponseMode,
    ) -> MewResult<Value> {
        debug!("添加作品评论: work_id={}", work_id);
        let endpoint = format!("/creation-tools/v1/works/{}/comment", work_id);
        let payload = json!({
            "content": comment,
            "emoji_content": emoji.unwrap_or(""),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(payload);
        self.send_maybe_parse(builder, mode, HTTPStatus::Created)
    }

    /// 回复作品评论
    pub fn create_comment_reply(
        &self,
        comment: &str,
        work_id: i32,
        comment_id: i32,
        parent_id: Option<i32>,
        mode: ResponseMode,
    ) -> MewResult<Value> {
        debug!(
            "回复评论: work_id={}, comment_id={}, parent_id={:?}",
            work_id, comment_id, parent_id
        );
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}/reply",
            work_id, comment_id
        );
        let data = json!({
            "parent_id": parent_id.unwrap_or(0),
            "content": comment,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(data);
        self.send_maybe_parse(builder, mode, HTTPStatus::Created)
    }

    /// 删除作品评论
    pub fn delete_comment(&self, work_id: i32, comment_id: i32) -> MewResult<bool> {
        debug!("删除评论: work_id={}, comment_id={}", work_id, comment_id);
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}",
            work_id, comment_id
        );
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, None);
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 置顶或取消置顶评论
    pub fn toggle_comment_pin(
        &self,
        action: ToggleAction,
        work_id: i32,
        comment_id: i32,
    ) -> MewResult<bool> {
        debug!(
            "切换评论置顶: action={:?}, work_id={}, comment_id={}",
            action, work_id, comment_id
        );
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}/top",
            work_id, comment_id
        );
        let builder = self
            .client
            .build_request(
                action.to_http_method(HttpMethod::Put, HttpMethod::Delete),
                &endpoint,
                None,
            )
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 点赞或取消点赞评论
    pub fn toggle_comment_like(
        &self,
        work_id: i32,
        comment_id: i32,
        action: ToggleAction,
    ) -> MewResult<bool> {
        debug!(
            "切换评论点赞: work_id={}, comment_id={}, action={:?}",
            work_id, comment_id, action
        );
        let endpoint = format!(
            "/creation-tools/v1/works/{}/comment/{}/liked",
            work_id, comment_id
        );
        let builder = self
            .client
            .build_request(
                action.to_http_method(HttpMethod::Post, HttpMethod::Delete),
                &endpoint,
                None,
            )
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Created)
    }

    /// 举报作品评论
    pub fn report_comment(&self, work_id: i32, comment_id: i32, reason: &str) -> MewResult<bool> {
        debug!(
            "举报评论: work_id={}, comment_id={}, reason={}",
            work_id, comment_id, reason
        );
        let endpoint = format!("/creation-tools/v1/works/{}/comment/report", work_id);
        let data = json!({
            "comment_id": comment_id,
            "report_reason": reason,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, None)
            .with_payload(data);
        self.check_status(builder, HTTPStatus::Ok)
    }
}

impl Default for CommentOperations {
    fn default() -> Self {
        Self::new()
    }
}

// KITTEN 作品管理类

/// 创建 Kitten 作品的参数
pub struct CreateKittenWorkArgs<'a> {
    pub name: &'a str,
    pub work_url: &'a str,
    pub preview: &'a str,
    pub version: &'a str,
    pub orientation: Option<i32>,
    pub sample_id: Option<&'a str>,
    pub work_source_label: Option<i32>,
    pub save_type: Option<i32>,
}

/// 发布 Kitten 作品的参数
pub struct PublishKittenWorkArgs<'a> {
    pub work_id: i32,
    pub name: &'a str,
    pub description: &'a str,
    pub operation: &'a str,
    pub labels: Vec<Value>,
    pub cover_url: &'a str,
    pub bcmc_url: &'a str,
    pub work_url: &'a str,
    pub fork_enable: i32,
    pub if_default_cover: i32,
    pub version: &'a str,
    pub cover_type: Option<i32>,
    pub user_labels: Option<Vec<Value>>,
}

/// Kitten 作品管理接口(创建,发布,删除,回收站等)
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

    /// 创建 Kitten 作品
    pub fn create_kitten_work(&self, args: CreateKittenWorkArgs<'_>) -> MewResult<Value> {
        debug!(
            "创建Kitten作品: name={}, version={}",
            args.name, args.version
        );
        let payload = json!({
            "name": args.name,
            "work_url": args.work_url,
            "preview": args.preview,
            "orientation": args.orientation.unwrap_or(1),
            "sample_id": args.sample_id.unwrap_or(""),
            "version": args.version,
            "work_source_label": args.work_source_label.unwrap_or(1),
            "save_type": args.save_type.unwrap_or(2),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/kitten/r2/work", Some(BaseKey::Creation))
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发布 Kitten 作品
    pub fn publish_kitten_work(&self, args: PublishKittenWorkArgs<'_>) -> MewResult<bool> {
        debug!(
            "发布Kitten作品: work_id={}, name={}",
            args.work_id, args.name
        );
        let endpoint = format!("/kitten/r2/work/{}/publish", args.work_id);
        let payload = json!({
            "name": args.name,
            "description": args.description,
            "operation": args.operation,
            "labels": args.labels,
            "cover_url": args.cover_url,
            "bcmc_url": args.bcmc_url,
            "work_url": args.work_url,
            "fork_enable": args.fork_enable,
            "if_default_cover": args.if_default_cover,
            "version": args.version,
            "cover_type": args.cover_type.unwrap_or(1),
            "user_labels": args.user_labels.unwrap_or_default(),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation))
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 删除未发布的 Kitten 作品草稿
    pub fn delete_kitten_draft(&self, work_id: i32) -> MewResult<bool> {
        debug!("删除Kitten草稿: work_id={}", work_id);
        let endpoint = format!("/kitten/common/work/{}/temporarily", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 取消发布作品
    pub fn unpublish_work(&self, work_id: i32) -> MewResult<bool> {
        debug!("取消发布作品: work_id={}", work_id);
        let endpoint = format!("/tiger/work/{}/unpublish", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Patch, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 通过 Web 端取消发布作品
    pub fn unpublish_work_web(&self, work_id: i32) -> MewResult<bool> {
        debug!("Web端取消发布作品: work_id={}", work_id);
        let endpoint = format!("/web/works/r2/unpublish/{}", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, None)
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 清空 Kitten 作品回收站
    pub fn empty_kitten_trash(&self) -> MewResult<bool> {
        debug!("清空Kitten回收站");
        let builder = self.client.build_request(
            HttpMethod::Delete,
            "/work/user/works/permanently",
            Some(BaseKey::Creation),
        );
        self.check_status(builder, HTTPStatus::NoContent)
    }

    /// 翻译 Kitten 作品
    pub fn translate_kitten_work(&self, data: Value) -> MewResult<Value> {
        debug!("翻译Kitten作品");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/kitten/work/translate",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }
}

impl Default for KittenWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// NEKO(Kitten N)作品管理类

/// 创建 KN 作品的参数
pub struct CreateKnWorkArgs<'a> {
    pub name: &'a str,
    pub work_url: &'a str,
    pub preview_url: &'a str,
    pub bcm_version: &'a str,
    pub save_type: Option<i32>,
    pub stage_type: Option<i32>,
    pub n_blocks: Option<i32>,
    pub n_roles: Option<i32>,
    pub n_scenes: Option<i32>,
    pub pic_need_check_file_url: Option<&'a str>,
}

/// 发布 KN 作品的参数
pub struct PublishKnWorkArgs<'a> {
    pub work_id: i32,
    pub name: &'a str,
    pub preview_url: &'a str,
    pub description: &'a str,
    pub operation: &'a str,
    pub fork_enable: i32,
    pub if_default_cover: i32,
    pub bcmc_url: &'a str,
    pub work_url: &'a str,
    pub bcm_version: &'a str,
    pub cover_url: Option<&'a str>,
}

/// KN 作品管理接口
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

    /// 创建 KN 作品
    pub fn create_kn_work(&self, args: CreateKnWorkArgs<'_>) -> MewResult<Value> {
        debug!("创建KN作品: name={}", args.name);
        let payload = json!({
            "bcm_version": args.bcm_version,
            "save_type": args.save_type.unwrap_or(2),
            "name": args.name,
            "work_url": args.work_url,
            "preview_url": args.preview_url,
            "stage_type": args.stage_type.unwrap_or(2),
            "n_blocks": args.n_blocks.unwrap_or(0),
            "n_roles": args.n_roles.unwrap_or(2),
            "n_scenes": args.n_scenes.unwrap_or(1),
            "pic_need_check_file_url": args.pic_need_check_file_url.unwrap_or(""),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/neko/works", Some(BaseKey::Creation))
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 发布 KN 作品
    pub fn publish_kn_work(&self, args: PublishKnWorkArgs<'_>) -> MewResult<bool> {
        debug!("发布KN作品: work_id={}, name={}", args.work_id, args.name);
        let endpoint = format!("/neko/community/work/publish/{}", args.work_id);
        let payload = json!({
            "name": args.name,
            "preview_url": args.preview_url,
            "description": args.description,
            "operation": args.operation,
            "fork_enable": args.fork_enable,
            "if_default_cover": args.if_default_cover,
            "bcmc_url": args.bcmc_url,
            "work_url": args.work_url,
            "bcm_version": args.bcm_version,
            "cover_url": args.cover_url.unwrap_or(""),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, Some(BaseKey::Creation))
            .with_payload(payload);
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 删除未发布的 KN 作品草稿
    pub fn delete_kn_draft(&self, work_id: i32, force: i32) -> MewResult<bool> {
        debug!("删除KN草稿: work_id={}, force={}", work_id, force);
        let timestamp = current_timestamp_13();
        let endpoint = format!("/neko/works/{}", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("force", force.to_string());
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 取消发布 KN 作品
    pub fn unpublish_kn_work(&self, work_id: i32) -> MewResult<bool> {
        debug!("取消发布KN作品: work_id={}", work_id);
        let endpoint = format!("/neko/community/work/unpublish/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 清空 KN 作品回收站
    pub fn empty_kn_trash(&self) -> MewResult<bool> {
        debug!("清空KN回收站");
        let builder = self.client.build_request(
            HttpMethod::Delete,
            "/neko/works/permanently",
            Some(BaseKey::Creation),
        );
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 恢复 KN 回收站作品
    pub fn recover_kn_trash(&self, work_id: i32) -> MewResult<bool> {
        debug!("恢复KN回收站作品: work_id={}", work_id);
        let endpoint = format!("/neko/works/{}/recover", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Patch, &endpoint, Some(BaseKey::Creation));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 保存教师作品
    pub fn save_teacher_work(&self, data: Value) -> MewResult<Value> {
        debug!("保存教师作品");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/works/teacher",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 复制作品
    pub fn copy_work(&self, work_id: i32) -> MewResult<Value> {
        debug!("复制作品: work_id={}", work_id);
        let data = json!({ "work_id": work_id });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/works/copy",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 作品图片故障排查
    pub fn troubleshoot_work_pics(&self, work_id: i32) -> MewResult<Value> {
        debug!("作品图片故障排查: work_id={}", work_id);
        let endpoint = format!("/neko/works/pic-troubleshoot/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }
}

impl Default for NekoWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// WOOD(海龟编辑器)作品管理类

/// 创建海龟编辑器作品的参数
pub struct CreateWoodProjectArgs<'a> {
    pub work_name: Option<&'a str>,
    pub language_type: Option<i32>,
    pub run_mode: Option<i32>,
    pub files: Option<Vec<Value>>,
    pub preview_code: Option<&'a str>,
    pub preview_url: Option<&'a str>,
    pub is_turn_on_debug: Option<bool>,
    pub editor_mode: Option<&'a str>,
    pub update_time: Option<i32>,
}

/// 海龟编辑器作品管理接口
pub struct WoodWorkManager {
    client: &'static CodeMaoClient,
}

impl WoodWorkManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取海龟编辑器项目信息
    pub fn fetch_wood_project(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取海龟编辑器项目: work_id={}", work_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/wood/project", Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string());
        self.send_and_parse(builder)
    }

    /// 创建海龟编辑器作品
    pub fn create_wood_project(&self, args: CreateWoodProjectArgs<'_>) -> MewResult<Value> {
        debug!("创建海龟编辑器作品: name={:?}", args.work_name);
        let payload = json!({
            "work_name": args.work_name.unwrap_or("新的作品"),
            "language_type": args.language_type.unwrap_or(3),
            "run_mode": args.run_mode.unwrap_or(0),
            "update_time": args.update_time.unwrap_or(0),
            "addition": {
                "readonly_paths": [],
                "locking_file_lines": {},
                "isTurnOnDebug": args.is_turn_on_debug.unwrap_or(true),
                "editorMode": args.editor_mode.unwrap_or("code"),
            },
            "files": args.files.unwrap_or_default(),
            "preview_url": args.preview_url.unwrap_or(""),
            "preview_code": args.preview_code.unwrap_or(""),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/wood/project", Some(BaseKey::Creation))
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 删除海龟编辑器草稿
    pub fn delete_wood_draft(&self, work_id: i32) -> MewResult<bool> {
        debug!("删除海龟编辑器草稿: work_id={}", work_id);
        let endpoint = format!("/wood/project/{}/temporarily", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 搜索用户的 Wood 作品
    pub fn search_user_wood_projects(
        &self,
        query: Option<&str>,
        page: Option<i32>,
        limit: Option<i32>,
        language_type: Option<i32>,
    ) -> MewResult<Value> {
        debug!("搜索Wood作品: query={:?}", query);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/wood/user/project/search",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("query", query.unwrap_or(""))
            .with_param("page", page.unwrap_or(1).to_string())
            .with_param("limit", limit.unwrap_or(15).to_string())
            .with_param("language_type", language_type.unwrap_or(0).to_string());
        self.send_and_parse(builder)
    }

    /// 在海龟编辑器作品中创建文件
    pub fn create_wood_file(
        &self,
        work_id: i32,
        file_name: Option<&str>,
        source_code: Option<&str>,
        file_type: Option<i32>,
        is_open: bool,
    ) -> MewResult<Value> {
        debug!(
            "创建海龟编辑器文件: work_id={}, file_name={:?}",
            work_id, file_name
        );
        // 先获取现有项目
        let project = self.fetch_wood_project(work_id)?;
        let mut files = project
            .get("files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

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
        self.create_wood_project(CreateWoodProjectArgs {
            work_name: project.get("work_name").and_then(Value::as_str),
            language_type: project
                .get("language_type")
                .and_then(Value::as_i64)
                .map(|v| i32::try_from(v).unwrap_or(0)),
            run_mode: project
                .get("run_mode")
                .and_then(Value::as_i64)
                .map(|v| i32::try_from(v).unwrap_or(0)),
            files: Some(files),
            preview_code: project.get("preview_code").and_then(Value::as_str),
            preview_url: project.get("preview_url").and_then(Value::as_str),
            is_turn_on_debug: project
                .get("addition")
                .and_then(|v| v.get("isTurnOnDebug"))
                .and_then(Value::as_bool),
            editor_mode: project
                .get("addition")
                .and_then(|v| v.get("editorMode"))
                .and_then(Value::as_str),
            update_time: project
                .get("update_time")
                .and_then(Value::as_i64)
                .map(|v| i32::try_from(v).unwrap_or(0)),
        })
    }
}

impl Default for WoodWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// COCO(Coconut)平台管理类

/// Coco 平台管理接口
pub struct CocoWorkManager {
    client: &'static CodeMaoClient,
}

impl CocoWorkManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取 Coco 平台的主要课程列表
    pub fn fetch_coco_primary_courses(&self) -> MewResult<Value> {
        debug!("获取Coco主要课程");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/coconut/primary-course/list",
            Some(BaseKey::Creation),
        );
        self.send_and_parse(builder)
    }

    /// 获取 Coco 的自定义控件列表分页迭代器
    pub fn fetch_custom_widgets_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取Coco自定义控件迭代器");
        let timestamp = current_timestamp_13();
        self.client
            .build_paginated("/coconut/web/widget/list")
            .with_iter_param("TIME", timestamp.to_string())
            .with_page_size(100)
            .with_total_key("data.total")
            .with_data_key("data.items")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("page_size")
            .with_offset_key("current_page")
            .with_base_key(BaseKey::Creation)
            .with_limit(limit.unwrap_or(100))
    }

    /// 获取 Coco 的示范教程列表
    pub fn fetch_demo_courses(&self) -> MewResult<Value> {
        debug!("获取Coco示范教程");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/coconut/sample/list",
            Some(BaseKey::Creation),
        );
        self.send_and_parse(builder)
    }

    /// 获取 Coco 的白名单作品链接
    pub fn fetch_whitelisted_works(&self) -> MewResult<Value> {
        debug!("获取Coco白名单作品");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "https://static.bcmcdn.com/coco/whitelist.json",
            None,
        );
        self.send_and_parse(builder)
    }

    /// 获取 Coco 的 web 控件
    pub fn fetch_web_widget(&self, page: Option<i32>, page_size: Option<i32>) -> MewResult<Value> {
        debug!("获取Coco web控件: page={:?}", page);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/coconut/web/user/widget/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("current_page", page.unwrap_or(1).to_string())
            .with_param("page_size", page_size.unwrap_or(100).to_string());
        self.send_and_parse(builder)
    }

    /// 更新 Coco 作品
    pub fn update_coco_work(
        &self,
        work_id: i32,
        work_name: &str,
        bcm_url: &str,
        preview_url: &str,
        archive_version: Option<&str>,
        save_type: Option<i32>,
    ) -> MewResult<Value> {
        debug!("更新Coco作品: work_id={}, name={}", work_id, work_name);
        let data = json!({
            "id": work_id,
            "name": work_name,
            "preview_url": preview_url,
            "bcm_url": bcm_url,
            "archive_version": archive_version.unwrap_or("0.1.0"),
            "save_type": save_type.unwrap_or(1),
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Put,
                "/coconut/web/work",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 发布 Coco 作品
    pub fn publish_coco_work(
        &self,
        work_id: i32,
        work_name: &str,
        bcmc_url: &str,
        cover_url: &str,
        description: &str,
        operation: &str,
    ) -> MewResult<Value> {
        debug!("发布Coco作品: work_id={}, name={}", work_id, work_name);
        let endpoint = format!("/coconut/web/work/{}/publish", work_id);
        let data = json!({
            "name": work_name,
            "description": description,
            "operation": operation,
            "cover_url": cover_url,
            "bcmc_url": bcmc_url,
            "player_url": format!("https://coco.codemao.cn/editor/player/{}?channel=community", work_id),
        });
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation))
            .with_payload(data);
        self.send_and_parse(builder)
    }
}

impl Default for CocoWorkManager {
    fn default() -> Self {
        Self::new()
    }
}

// 协作功能管理类

/// 作品协作功能接口
pub struct CollaborationManager {
    client: &'static CodeMaoClient,
}

impl CollaborationManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取或删除 Kitten 协作邀请码
    pub fn fetch_kitten_collaboration_code(
        &self,
        work_id: i32,
        method: HttpMethod,
    ) -> MewResult<Value> {
        debug!(
            "获取Kitten协作邀请码: work_id={}, method={:?}",
            work_id, method
        );
        let endpoint = format!("/coll/kitten/collaborator/code/{}", work_id);
        let builder = self
            .client
            .build_request(method, &endpoint, Some(BaseKey::Collaboration));
        self.send_and_parse(builder)
    }

    /// 获取 Coco 协作邀请码
    pub fn fetch_coco_collaboration_code(
        &self,
        work_id: i32,
        permission: CollabPermission,
    ) -> MewResult<Value> {
        debug!(
            "获取Coco协作邀请码: work_id={}, permission={:?}",
            work_id, permission
        );
        let timestamp = current_timestamp_13();
        let endpoint = format!("/coll/coco/collaborator/code/{}", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_param("edit_permission", permission.as_code().to_string());
        self.send_and_parse(builder)
    }

    /// 获取协作者列表分页迭代器
    pub fn fetch_collaborators_gen(
        &self,
        work_type: CollabWorkType,
        work_id: i32,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!(
            "获取协作者迭代器: work_type={:?}, work_id={}",
            work_type, work_id
        );
        let timestamp = current_timestamp_13();
        let endpoint = format!("/coll/{}/collaborator/{}", work_type.as_str(), work_id);
        self.client
            .build_paginated(&endpoint)
            .with_base_key(BaseKey::Collaboration)
            .with_iter_param("TIME", timestamp.to_string())
            .with_page_size(100)
            .with_total_key("data.total")
            .with_data_key("data.items")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("page_size")
            .with_offset_key("current_page")
            .with_limit(limit.unwrap_or(100))
    }

    /// 获取协作状态
    pub fn fetch_collaboration_status(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取协作状态: work_id={}", work_id);
        let endpoint = format!("/collaboration/user/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取协作用户
    pub fn fetch_collaboration_user(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取协作用户: work_id={}", work_id);
        let endpoint = format!("/collaboration/user/edited/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 启用 Kitten/Coco 作品协作功能
    pub fn enable_collaboration(&self, work_id: i32, work_type: CollabWorkType) -> MewResult<bool> {
        debug!("启用协作: work_id={}, type={:?}", work_id, work_type);
        let endpoint = format!("/coll/{}/{}", work_type.as_str(), work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Post, &endpoint, Some(BaseKey::Collaboration))
            .with_payload(json!({}));
        self.check_status(builder, HTTPStatus::Ok)
    }

    /// 获取协作的 Coco 作品分页迭代器
    pub fn fetch_collaboration_coco_works_gen(&self, limit: Option<usize>) -> PaginatedIter {
        debug!("获取协作Coco作品迭代器");
        let timestamp = current_timestamp_13();
        self.client
            .build_paginated("/coll/coco/coll_works")
            .with_base_key(BaseKey::Collaboration)
            .with_iter_param("TIME", timestamp.to_string())
            .with_page_size(40)
            .with_total_key("data.total")
            .with_data_key("data.items")
            .with_pagination_method(PaginationMethod::Page)
            .with_amount_key("page_size")
            .with_offset_key("current_page")
            .with_limit(limit.unwrap_or(40))
    }
}

impl Default for CollaborationManager {
    fn default() -> Self {
        Self::new()
    }
}

// AI 服务类

/// AI 绘画等服务接口
pub struct AIServices {
    client: &'static CodeMaoClient,
}

impl AIServices {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取文生图提示词
    pub fn fetch_text2img_prompt(&self) -> MewResult<Value> {
        debug!("获取文生图提示词");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/neko/text2img/prompt",
            Some(BaseKey::Creation),
        );
        self.send_and_parse(builder)
    }

    /// 获取 AI 绘画模板
    pub fn fetch_ai_painting_templates(&self, template_type: &str) -> MewResult<Value> {
        debug!("获取AI绘画模板: type={}", template_type);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/ai-painting/templates",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", template_type);
        self.send_and_parse(builder)
    }

    /// AI 绘画匹配
    pub fn match_ai_painting(&self, data: Value) -> MewResult<Value> {
        debug!("AI绘画匹配");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/ai-painting/match",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 添加到灵感池
    pub fn add_to_inspiration_pool(
        &self,
        img_url: &str,
        prompt: &str,
        style: &str,
        img_type: &str,
        generation_type: &str,
    ) -> MewResult<Value> {
        debug!("添加到灵感池: prompt={}", prompt);
        let data = json!({
            "img_url": img_url,
            "prompt": prompt,
            "style": style,
            "img_type": img_type,
            "generation_type": generation_type,
        });
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/inspiration-pool",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }
}

impl Default for AIServices {
    fn default() -> Self {
        Self::new()
    }
}

// 教学计划管理类

/// 教学计划管理接口
pub struct TeachingPlanManager {
    client: &'static CodeMaoClient,
}

impl TeachingPlanManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 保存团队作品(教学计划)
    pub fn save_team_work(&self, data: Value) -> MewResult<Value> {
        debug!("保存团队作品");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/teaching-plan/save/team/work",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 获取教学计划操作日志
    pub fn fetch_teaching_plan_logs(
        &self,
        work_id: i32,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取教学计划操作日志: work_id={}", work_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/teaching-plan/list/opr/log",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string())
            .with_page(limit, offset, DEFAULT_LIMIT);
        self.send_and_parse(builder)
    }

    /// 添加教学计划操作日志
    pub fn add_teaching_plan_log(&self, data: Value) -> MewResult<Value> {
        debug!("添加教学计划操作日志");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/teaching-plan/add/opr/log",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 获取作品编辑状态
    pub fn fetch_work_editing_status(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品编辑状态: work_id={}", work_id);
        let endpoint = format!("/neko/teaching-plan/work/editing-status/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 设置作品编辑状态
    pub fn set_work_editing_status(&self, data: Value) -> MewResult<Value> {
        debug!("设置作品编辑状态");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/teaching-plan/set/work/editing-status",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 更新课程进度
    pub fn update_course_progress(&self, data: Value) -> MewResult<Value> {
        debug!("更新课程进度");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/course/user/progress",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 提交课程作品
    pub fn submit_course_work(&self, data: Value) -> MewResult<Value> {
        debug!("提交课程作品");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/course/user/course-work",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 保存教师课程邀请链接
    pub fn save_teacher_course_invite_url(&self, data: Value) -> MewResult<Value> {
        debug!("保存教师课程邀请链接");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/works/save-teacher-course-invite-url",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }
}

impl Default for TeachingPlanManager {
    fn default() -> Self {
        Self::new()
    }
}

// 图像分类管理类

/// 图像分类管理接口
pub struct ImageClassifyManager {
    client: &'static CodeMaoClient,
}

impl ImageClassifyManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取图像分类列表
    pub fn fetch_image_classify_list(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取图像分类列表: limit={:?}, offset={:?}", limit, offset);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/image-classify/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_page(limit, offset, DEFAULT_LIMIT);
        self.send_and_parse(builder)
    }

    /// 提交图像分类
    pub fn submit_image_classify(&self, data: Value) -> MewResult<Value> {
        debug!("提交图像分类");
        let builder = self
            .client
            .build_request(
                HttpMethod::Post,
                "/neko/image-classify",
                Some(BaseKey::Creation),
            )
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 更新图像分类
    pub fn update_image_classify(&self, classify_id: &str, data: Value) -> MewResult<Value> {
        debug!("更新图像分类: classify_id={}", classify_id);
        let endpoint = format!("/neko/image-classify/{}", classify_id);
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation))
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 删除图像分类
    pub fn delete_image_classify(&self, classify_id: &str) -> MewResult<Value> {
        debug!("删除图像分类: classify_id={}", classify_id);
        let endpoint = format!("/neko/image-classify/{}", classify_id);
        let builder =
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }
}

impl Default for ImageClassifyManager {
    fn default() -> Self {
        Self::new()
    }
}

// 包管理类

/// 包管理接口
pub struct PackageManager {
    client: &'static CodeMaoClient,
}

impl PackageManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取包列表
    pub fn fetch_package_list(
        &self,
        package_type: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "获取包列表: type={}, limit={:?}, offset={:?}",
            package_type, limit, offset
        );
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/package/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", package_type)
            .with_page(limit, offset, DEFAULT_LIMIT);
        self.send_and_parse(builder)
    }

    /// 创建包
    pub fn create_package(&self, data: Value) -> MewResult<Value> {
        debug!("创建包");
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/neko/package", Some(BaseKey::Creation))
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 更新包信息
    pub fn update_package(
        &self,
        package_id: &str,
        name: &str,
        description: &str,
    ) -> MewResult<Value> {
        debug!("更新包: package_id={}, name={}", package_id, name);
        let endpoint = format!("/neko/package/{}", package_id);
        let data = json!({
            "name": name,
            "description": description,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Put, &endpoint, Some(BaseKey::Creation))
            .with_payload(data);
        self.send_and_parse(builder)
    }

    /// 删除包
    pub fn delete_package(&self, package_id: &str) -> MewResult<Value> {
        debug!("删除包: package_id={}", package_id);
        let endpoint = format!("/neko/package/{}", package_id);
        let builder =
            self.client
                .build_request(HttpMethod::Delete, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }
}

impl Default for PackageManager {
    fn default() -> Self {
        Self::new()
    }
}

// 示例管理类

/// 示例管理接口
pub struct SampleManager {
    client: &'static CodeMaoClient,
}

impl SampleManager {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取示例列表
    pub fn fetch_sample_list(&self, subject_id: &str) -> MewResult<Value> {
        debug!("获取示例列表: subject_id={}", subject_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/sample/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("subject_id", subject_id);
        self.send_and_parse(builder)
    }
}

impl Default for SampleManager {
    fn default() -> Self {
        Self::new()
    }
}

// 作品数据获取类

/// 作品数据查询接口(详情,评论,源代码,推荐,搜索等)
pub struct WorkDataFetcher {
    client: &'static CodeMaoClient,
}

impl WorkDataFetcher {
    pub fn new() -> Self {
        Self {
            client: CodeMaoClient::global(),
        }
    }

    /// 获取作品详细信息
    pub fn fetch_work_details(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品详情: work_id={}", work_id);
        let endpoint = format!("/creation-tools/v1/works/{}", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取 Kitten 作品详细信息
    pub fn fetch_kitten_work_details(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Kitten作品详情: work_id={}", work_id);
        let endpoint = format!("/kitten/work/detail/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取 KN 作品详细信息
    pub fn fetch_kn_work_details(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取KN作品详情: work_id={}", work_id);
        let endpoint = format!("/neko/works/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取 Coco 作品信息
    pub fn fetch_coco_work_info(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Coco作品信息: work_id={}", work_id);
        let endpoint = format!("/coconut/web/work/{}/info", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取 KN 作品发布状态
    pub fn fetch_kn_publish_status(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取KN作品发布状态: work_id={}", work_id);
        let endpoint = format!("/neko/community/work/detail/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取 KN 作品详情
    pub fn fetch_kn_work_detail(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取KN作品详情: work_id={}", work_id);
        let endpoint = format!("/neko/community/player/published-work-detail/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取玩家作品详情
    pub fn fetch_player_work_detail(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取玩家作品详情: work_id={}", work_id);
        let endpoint = format!("/neko/works/player/work-detail/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 通过课程代码获取作品
    pub fn fetch_work_by_course_code(&self, course_code: &str) -> MewResult<Value> {
        debug!("通过课程代码获取作品: course_code={}", course_code);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/works/get-player-by-course-code",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("course_code", course_code);
        self.send_and_parse(builder)
    }

    /// 获取作品状态
    pub fn fetch_work_status(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品状态: work_id={}", work_id);
        let endpoint = format!("/neko/works/status/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取作品参加的活动信息
    pub fn fetch_work_activity(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品活动信息: work_id={}", work_id);
        let endpoint = format!("/web/works/activity/info/{}", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 检查用户操作状态
    pub fn check_user_operation_status(&self, work_id: i32) -> MewResult<Value> {
        debug!("检查用户操作状态: work_id={}", work_id);
        let endpoint = format!("/neko/community/check-user-opr-work-status/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    // 评论相关

    /// 获取作品评论分页迭代器
    pub fn fetch_work_comments_gen(&self, work_id: i32, limit: Option<usize>) -> PaginatedIter {
        debug!("获取作品评论迭代器: work_id={}", work_id);
        let endpoint = format!("/creation-tools/v1/works/{}/comments", work_id);
        self.client
            .build_paginated(&endpoint)
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(15)
            .with_limit(limit.unwrap_or(15))
            .with_total_key("page_total")
    }

    // 源代码

    /// 获取作品源代码
    pub fn fetch_work_source_code(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品源代码: work_id={}", work_id);
        let endpoint = format!("/creation-tools/v1/works/{}/source/public", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取 Kitten 作品源代码
    pub fn fetch_kitten_source_code(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Kitten源代码: work_id={}", work_id);
        let endpoint = format!("/kitten/work/ide/load/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取游玩端 Kitten 作品代码
    pub fn fetch_kitten_player_code(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Kitten播放器代码: work_id={}", work_id);
        let endpoint = format!("/kitten/r2/work/player/load/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取 Coco 作品源代码
    pub fn fetch_coco_source_code(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Coco源代码: work_id={}", work_id);
        let endpoint = format!("/coconut/web/work/{}/content", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取游玩端 Coco 作品代码
    pub fn fetch_coco_player_code(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Coco播放器代码: work_id={}", work_id);
        let endpoint = format!("/coconut/web/work/{}/load", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 获取游玩端 Wood 作品代码
    pub fn fetch_wood_player_code(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Wood播放器代码: work_id={}", work_id);
        let timestamp = current_timestamp_13();
        let endpoint = format!("/wood/work/{}/publish", work_id);
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation))
            .with_param("TIME", timestamp.to_string())
            .with_param("channel_type", "0");
        self.send_and_parse(builder)
    }

    /// 获取 KN 作品历史版本
    pub fn fetch_kn_work_versions(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取KN作品历史版本: work_id={}", work_id);
        let endpoint = format!("/neko/works/archive/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    // 作品列表和推荐

    /// 获取 Web 端相关作品推荐
    pub fn fetch_web_recommendations(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Web端推荐: work_id={}", work_id);
        let endpoint = format!("/nemo/v2/works/web/{}/recommended", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取 Nemo 端相关作品推荐
    pub fn fetch_nemo_recommendations(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Nemo端推荐: work_id={}", work_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/nemo/v3/work-details/recommended/list",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取 Nemo 播放器详情
    pub fn fetch_nemo_player_detail(
        &self,
        work_type: Option<i32>,
        url: Option<&str>,
    ) -> MewResult<Value> {
        debug!("获取Nemo播放器详情: type={:?}", work_type);
        let mut builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/player/detail", None);
        if let Some(t) = work_type {
            builder = builder.with_param("type", t.to_string());
        }
        if let Some(u) = url {
            builder = builder.with_param("url", u);
        }
        self.send_and_parse(builder)
    }

    /// 绑定七牛上传业务
    pub fn bind_qiniu_upload_business(
        &self,
        business_id: &str,
        url_list: Vec<String>,
    ) -> MewResult<Value> {
        debug!("绑定七牛上传业务: business_id={}", business_id);
        let payload = json!({
            "business_id": business_id,
            "url_list": url_list,
        });
        let builder = self
            .client
            .build_request(HttpMethod::Post, "/nemo/qiniu/upload/business/bind", None)
            .with_payload(payload);
        self.send_and_parse(builder)
    }

    /// 获取 Web 端最新作品
    pub fn fetch_new_works_web(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
        origin: bool,
    ) -> MewResult<Value> {
        debug!("获取Web端最新作品: limit={:?}, origin={}", limit, origin);
        let timestamp = current_timestamp_13();
        let mut builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/pc/discover/newest-work",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_page(limit, offset, DEFAULT_PAGE_SIZE);
        if origin {
            builder = builder.with_param("work_origin_type", "ORIGINAL_WORK");
        }
        self.send_and_parse(builder)
    }

    /// 获取 Web 端主题作品
    pub fn fetch_themed_works_web(
        &self,
        limit: i32,
        offset: Option<i32>,
        subject_id: Option<i32>,
    ) -> MewResult<Value> {
        debug!(
            "获取Web端主题作品: limit={}, subject_id={:?}",
            limit, subject_id
        );
        let timestamp = current_timestamp_13();
        let mut builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/pc/discover/subject-work",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("limit", limit.to_string())
            .with_param("offset", offset.unwrap_or(0).to_string());
        if let Some(sid) = subject_id {
            builder = builder.with_param("subject_id", sid.to_string());
        }
        self.send_and_parse(builder)
    }

    /// 获取 Nemo 端发现页作品
    pub fn fetch_nemo_discover(&self) -> MewResult<Value> {
        debug!("获取Nemo端发现页作品");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/creation-tools/v1/home/discover", None);
        self.send_and_parse(builder)
    }

    /// 获取 Nemo 端最新作品
    pub fn fetch_new_works_nemo(
        &self,
        types: NemoWorkType,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取Nemo端最新作品: type={:?}, limit={:?}", types, limit);
        let timestamp = current_timestamp_13();
        let endpoint = format!("/nemo/v3/newest/work/{}/list", types.as_str());
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_page(limit, offset, DEFAULT_PAGE_SIZE);
        self.send_and_parse(builder)
    }

    /// 获取动态作品
    pub fn fetch_activity_feed(&self, limit: Option<i32>, offset: Option<i32>) -> MewResult<Value> {
        debug!("获取动态作品: limit={:?}", limit);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v3/work/dynamic", None)
            .with_param("TIME", timestamp.to_string())
            .with_page(limit, offset, DEFAULT_PAGE_SIZE);
        self.send_and_parse(builder)
    }

    /// 获取动态推荐用户
    pub fn fetch_recommended_users(&self) -> MewResult<Value> {
        debug!("获取推荐用户");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/nemo/v3/dynamic/focus/user/recommend",
            None,
        );
        self.send_and_parse(builder)
    }

    // 主题相关

    /// 获取随机作品主题 ID 列表
    pub fn fetch_random_subjects(&self) -> MewResult<Value> {
        debug!("获取随机主题");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/nemo/v3/work-subject/random", None);
        self.send_and_parse(builder)
    }

    /// 获取主题详细信息
    pub fn fetch_subject_details(&self, ids: i32) -> MewResult<Value> {
        debug!("获取主题详情: ids={}", ids);
        let endpoint = format!("/nemo/v3/work-subject/{}/info", ids);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取主题下作品
    pub fn fetch_subject_works(
        &self,
        ids: i32,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取主题作品: ids={}", ids);
        let timestamp = current_timestamp_13();
        let endpoint = format!("/nemo/v3/work-subject/{}/works", ids);
        let builder = self
            .client
            .build_request(HttpMethod::Get, &endpoint, None)
            .with_param("TIME", timestamp.to_string())
            .with_page(limit, offset, DEFAULT_PAGE_SIZE);
        self.send_and_parse(builder)
    }

    /// 获取所有主题作品
    pub fn fetch_all_subject_works(
        &self,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取所有主题作品");
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v3/work-subject/home", None)
            .with_param("TIME", timestamp.to_string())
            .with_page(limit, offset, DEFAULT_PAGE_SIZE);
        self.send_and_parse(builder)
    }

    // 作品谱系

    /// 获取 Web 端作品谱系
    pub fn fetch_work_lineage_web(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Web端作品谱系: work_id={}", work_id);
        let endpoint = format!("/tiger/work/tree/{}", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取 Nemo 端作品谱系
    pub fn fetch_work_lineage_nemo(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取Nemo端作品谱系: work_id={}", work_id);
        let endpoint = format!("/nemo/v2/works/root/{}", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    // 回收站

    /// 获取 Kitten 回收站作品分页迭代器
    pub fn fetch_kitten_trash_gen(
        &self,
        version: KittenVersion,
        work_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取Kitten回收站迭代器: version={:?}", version);
        self.client
            .build_paginated("/tiger/work/recycle/list")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(30)
            .with_limit(limit.unwrap_or(30))
            .with_iter_param("version_no", version.as_str())
            .with_iter_param("work_status", work_status.unwrap_or("CYCLED"))
            .with_base_key(BaseKey::Creation)
    }

    /// 获取海龟编辑器回收站作品分页迭代器
    pub fn fetch_wood_trash_gen(
        &self,
        language_type: Option<i32>,
        work_status: Option<&str>,
        published_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取海龟编辑器回收站迭代器");
        self.client
            .build_paginated("/wood/comm/work/list")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(30)
            .with_limit(limit.unwrap_or(30))
            .with_iter_param("language_type", language_type.unwrap_or(0).to_string())
            .with_iter_param("work_status", work_status.unwrap_or("CYCLED"))
            .with_iter_param("published_status", published_status.unwrap_or("undefined"))
            .with_base_key(BaseKey::Creation)
    }

    /// 获取代码岛回收站作品分页迭代器
    pub fn fetch_box_trash_gen(
        &self,
        work_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取代码岛回收站迭代器");
        self.client
            .build_paginated("/box/v2/work/list")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(30)
            .with_limit(limit.unwrap_or(30))
            .with_iter_param("work_status", work_status.unwrap_or("CYCLED"))
            .with_base_key(BaseKey::Creation)
    }

    /// 获取小说回收站分页迭代器
    pub fn fetch_fiction_trash_gen(
        &self,
        fiction_status: Option<&str>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取小说回收站迭代器");
        self.client
            .build_paginated("/web/fanfic/my/new")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(30)
            .with_limit(limit.unwrap_or(30))
            .with_iter_param("fiction_status", fiction_status.unwrap_or("CYCLED"))
    }

    /// 获取 KN 回收站作品分页迭代器
    pub fn fetch_kn_trash_gen(
        &self,
        name: Option<&str>,
        work_business_classify: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("获取KN回收站迭代器");
        self.client
            .build_paginated("/neko/works/v2/list/user")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(15)
            .with_limit(limit.unwrap_or(15))
            .with_response_amount_key("page_size")
            .with_iter_param("name", name.unwrap_or(""))
            .with_iter_param("status", "-99")
            .with_iter_param(
                "work_business_classify",
                work_business_classify.unwrap_or(1).to_string(),
            )
            .with_base_key(BaseKey::Creation)
    }

    // 搜索

    /// 搜索 KN 作品分页迭代器
    pub fn search_kn_works_gen(
        &self,
        name: &str,
        status: Option<i32>,
        work_business_classify: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("搜索KN作品: name={}", name);
        self.client
            .build_paginated("/neko/works/v2/list/user")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(15)
            .with_limit(limit.unwrap_or(15))
            .with_response_amount_key("page_size")
            .with_iter_param("name", name)
            .with_iter_param("status", status.unwrap_or(1).to_string())
            .with_iter_param(
                "work_business_classify",
                work_business_classify.unwrap_or(1).to_string(),
            )
            .with_base_key(BaseKey::Creation)
    }

    /// 搜索已发布 KN 作品分页迭代器
    pub fn search_published_kn_works_gen(
        &self,
        name: &str,
        work_business_classify: Option<i32>,
        limit: Option<usize>,
    ) -> PaginatedIter {
        debug!("搜索已发布KN作品: name={}", name);
        self.client
            .build_paginated("/neko/works/list/user/published")
            .with_iter_param("TIME", current_timestamp_13().to_string())
            .with_page_size(15)
            .with_limit(limit.unwrap_or(15))
            .with_response_amount_key("page_size")
            .with_iter_param("name", name)
            .with_iter_param(
                "work_business_classify",
                work_business_classify.unwrap_or(1).to_string(),
            )
            .with_base_key(BaseKey::Creation)
    }

    /// 通过名称搜索作品(Web 端)
    pub fn search_works_by_name_web(
        &self,
        name: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("Web端搜索作品: name={}", name);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/community/work/name/search", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("query", name)
            .with_page(limit, offset, DEFAULT_LIMIT);
        self.send_and_parse(builder)
    }

    /// 通过名称搜索作品(Nemo 端)
    pub fn search_works_by_name_nemo(
        &self,
        name: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("Nemo端搜索作品: name={}", name);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/nemo/v2/work/name/search", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("key", name)
            .with_page(limit, offset, DEFAULT_LIMIT);
        self.send_and_parse(builder)
    }

    // 标签和元数据

    /// 获取作品元数据
    pub fn fetch_work_metadata(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品元数据: work_id={}", work_id);
        let endpoint = format!("/api/work/info/{}", work_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    /// 获取作品标签
    pub fn fetch_work_tags(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品标签: work_id={}", work_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/creation-tools/v1/work-details/work-labels",
                None,
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("work_id", work_id.to_string());
        self.send_and_parse(builder)
    }

    /// 获取所有 Kitten 作品标签
    pub fn fetch_kitten_tags(&self) -> MewResult<Value> {
        debug!("获取Kitten标签");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/kitten/work/labels",
            Some(BaseKey::Creation),
        );
        self.send_and_parse(builder)
    }

    /// 获取 Kitten 默认封面
    pub fn fetch_kitten_default_covers(&self) -> MewResult<Value> {
        debug!("获取Kitten默认封面");
        let builder = self.client.build_request(
            HttpMethod::Get,
            "/kitten/work/cover/defaultCovers",
            Some(BaseKey::Creation),
        );
        self.send_and_parse(builder)
    }

    /// 获取作品最近使用的封面
    pub fn fetch_recent_covers(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取作品最近封面: work_id={}", work_id);
        let endpoint = format!("/kitten/work/cover/{}/recentCovers", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::Creation));
        self.send_and_parse(builder)
    }

    /// 验证作品名称是否可用
    pub fn validate_work_name(&self, name: &str, work_id: i32) -> MewResult<Value> {
        debug!("验证作品名称: name={}, work_id={}", name, work_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/tiger/work/checkname", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("name", name)
            .with_param("work_id", work_id.to_string());
        self.send_and_parse(builder)
    }

    // 作者相关

    /// 获取作者作品集
    pub fn fetch_author_portfolio(&self, user_id: i32) -> MewResult<Value> {
        debug!("获取作者作品集: user_id={}", user_id);
        let endpoint = format!("/web/works/users/{}", user_id);
        let builder = self.client.build_request(HttpMethod::Get, &endpoint, None);
        self.send_and_parse(builder)
    }

    // 其他

    /// 根据喵口令获取作品数据
    pub fn fetch_work_by_miao_code(&self, token: &str) -> MewResult<Value> {
        debug!("根据喵口令获取作品: token={}", token);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(HttpMethod::Get, "/tiger/nemo/miao-codes", None)
            .with_param("TIME", timestamp.to_string())
            .with_param("token", token);
        self.send_and_parse(builder)
    }

    /// 获取 KN 作品变量列表
    pub fn fetch_kn_variables(&self, work_id: i32) -> MewResult<Value> {
        debug!("获取KN变量: work_id={}", work_id);
        let endpoint = format!("/neko/cv/list/variables/{}", work_id);
        let builder =
            self.client
                .build_request(HttpMethod::Get, &endpoint, Some(BaseKey::SocketCV));
        self.send_and_parse(builder)
    }

    /// 获取积木或角色资源包
    pub fn fetch_resource_pack(
        &self,
        types: ResourcePackType,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取资源包: type={:?}", types);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/package/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", types.as_value().to_string())
            .with_page(limit, offset, NEKO_PACKAGE_PAGE_SIZE);
        self.send_and_parse(builder)
    }

    /// 获取素材分类
    pub fn fetch_material_categories(&self, material_type: &str) -> MewResult<Value> {
        debug!("获取素材分类: type={}", material_type);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/material/categories",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("type", material_type);
        self.send_and_parse(builder)
    }

    /// 获取素材根分类
    pub fn fetch_material_categories_root(&self) -> MewResult<Value> {
        debug!("获取素材根分类");
        let builder =
            self.client
                .build_request(HttpMethod::Get, "/web/materials/categories/root", None);
        self.send_and_parse(builder)
    }

    /// 获取素材列表
    pub fn fetch_material_list(
        &self,
        second_id: &str,
        limit: Option<i32>,
        offset: Option<i32>,
    ) -> MewResult<Value> {
        debug!("获取素材列表: second_id={}", second_id);
        let timestamp = current_timestamp_13();
        let builder = self
            .client
            .build_request(
                HttpMethod::Get,
                "/neko/material/list",
                Some(BaseKey::Creation),
            )
            .with_param("TIME", timestamp.to_string())
            .with_param("second_id", second_id)
            .with_page(limit, offset, DEFAULT_LIMIT);
        self.send_and_parse(builder)
    }
}

impl Default for WorkDataFetcher {
    fn default() -> Self {
        Self::new()
    }
}

// 共享请求辅助(ClientAccess)

impl ClientAccess for BaseWorkOperations {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for CommentOperations {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for KittenWorkManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for NekoWorkManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for WoodWorkManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for CocoWorkManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for CollaborationManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for AIServices {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for TeachingPlanManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for ImageClassifyManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for PackageManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for SampleManager {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}

impl ClientAccess for WorkDataFetcher {
    fn client(&self) -> &CodeMaoClient {
        self.client
    }
}
