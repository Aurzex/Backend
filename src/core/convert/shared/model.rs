use super::config::DecompilerConfig;
use super::error::Result;
use super::json::ValueExt;
use serde_json::Value;
use std::sync::Arc;

// 作品类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorType {
    Kitten2,
    Kitten3,
    Kitten4,
    Coco,
    Neko,
    Nemo,
    Wood,
}

impl std::str::FromStr for EditorType {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "KITTEN2" => Ok(EditorType::Kitten2),
            "KITTEN3" => Ok(EditorType::Kitten3),
            // 无后缀的 KITTEN 作品(如 geometry 对战)编辑版使用 XML shadow,对应 Kitten3 格式
            "KITTEN" => Ok(EditorType::Kitten3),
            "KITTEN4" => Ok(EditorType::Kitten4),
            "COCO" => Ok(EditorType::Coco),
            "NEKO" => Ok(EditorType::Neko),
            "NEMO" => Ok(EditorType::Nemo),
            "WOOD" => Ok(EditorType::Wood),
            _ => Err(()),
        }
    }
}

impl EditorType {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            EditorType::Kitten2 => "KITTEN2",
            EditorType::Kitten3 => "KITTEN3",
            EditorType::Kitten4 => "KITTEN4",
            EditorType::Coco => "COCO",
            EditorType::Neko => "NEKO",
            EditorType::Nemo => "NEMO",
            EditorType::Wood => "WOOD",
        }
    }

    pub(crate) fn is_kitten(&self) -> bool {
        matches!(
            self,
            EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4
        )
    }
    pub(crate) fn is_nemo(&self) -> bool {
        matches!(self, EditorType::Nemo)
    }
    pub(crate) fn is_neko(&self) -> bool {
        matches!(self, EditorType::Neko)
    }
    pub(crate) fn is_coco(&self) -> bool {
        matches!(self, EditorType::Coco)
    }
    pub(crate) fn is_wood(&self) -> bool {
        matches!(self, EditorType::Wood)
    }
    pub(crate) fn use_xml_shadow(&self) -> bool {
        // Kitten2/3/4 编辑版(.bcm/.bcm4)的 shadows 均为 XML 字符串
        matches!(
            self,
            EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4
        )
    }
}

/// 作品 ID 新类型:与 user_id/admin_id 等裸 i64 区分,编译期防混用
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkId(i64);

impl WorkId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl From<i64> for WorkId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

impl From<WorkId> for i64 {
    fn from(id: WorkId) -> i64 {
        id.0
    }
}

impl std::fmt::Display for WorkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 作品信息
#[derive(Debug, Clone)]
pub(crate) struct WorkInfo {
    pub(crate) id: WorkId,
    pub(crate) name: String,
    pub(crate) work_type: EditorType,
    pub(crate) version: String,
    pub(crate) user_id: i64,
    pub(crate) preview_url: String,
    pub(crate) application_version: String,
}

impl WorkInfo {
    pub(crate) fn from_api_response(data: &Value) -> Result<Self> {
        let work_type_str = data.get_str_or("type", "NEMO");
        let work_type = work_type_str
            .parse::<EditorType>()
            .unwrap_or(EditorType::Nemo);
        let name = data
            .get("work_name")
            .or_else(|| data.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("未知作品")
            .to_string();
        Ok(Self {
            id: WorkId::new(data.get_i64_or_default("id", 0)),
            name,
            work_type,
            version: data.get_string_or("bcm_version", "0.16.2"),
            user_id: data.get_i64_or_default("user_id", 0),
            preview_url: data.get_string_or("preview", ""),
            application_version: data.get_string_or("application_version", "0.0.0"),
        })
    }

    pub(crate) fn file_extension(&self, config: &Arc<DecompilerConfig>) -> String {
        config
            .file_extensions
            .get(self.work_type.as_str())
            .cloned()
            .unwrap_or(".json".to_string())
    }
}
