//! 领域模型:编辑器判别与扩展名表、作品信息、抓取契约、id 生成器。

use super::config::DecompilerConfig;
use super::error::Result;
use super::infra::ValueExt;
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
    pub(crate) user_id: i64,
    /// 源作品的 `bcm_version`(作品详情接口给的元信息;建作品时要原样带上,空值由调用方兜底)
    pub(crate) bcm_version: String,
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
            user_id: data.get_i64_or_default("user_id", 0),
            bcm_version: data.get_str_or("bcm_version", "").to_string(),
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

// ===== 抓取契约(原 fetch.rs)=====

pub(crate) enum RawWorkData {
    Kitten(Arc<Value>),
    NekoEncrypted(String),
    Nemo(Arc<Value>, Arc<Value>),
    Wood(Arc<Value>),
    Coco(Arc<Value>),
}

pub(crate) trait WorkFetcher: Send + Sync {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData>;
}

// ===== id 生成器(原 files.rs 的 IdGenerator)=====

// 新 ID 生成器(方案一风格)
#[derive(Clone)]
pub(crate) struct IdGenerator {
    chars: Vec<char>,
}

impl IdGenerator {
    pub(crate) fn new() -> Self {
        let chars = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .collect();
        Self { chars }
    }

    pub(crate) fn generate(&self, length: usize) -> String {
        (0..length)
            .map(|_| {
                let idx = fastrand::usize(0..self.chars.len());
                self.chars[idx]
            })
            .collect()
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}
