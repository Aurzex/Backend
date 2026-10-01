//! 反编译子域的作品模型与抓取契约:作品元信息(建作品要的 `bcm_version`、封面)、
//! 各家抓取器的原始数据形态,以及 `WorkFetcher` 接口。
//!
//! 原先躺在地基 `shared.rs`(合并前是 `shared/model.rs` 的作品信息/抓取契约段),
//! 但消费者只有 `decompile` ⇒ 回归子域(见 `docs/rounds/39` §W2b)。

use super::config::DecompilerConfig;
use crate::core::convert::shared::{EditorType, Result, ValueExt, WorkId};
use serde_json::Value;
use std::sync::Arc;

/// 作品信息
#[derive(Debug, Clone)]
pub(crate) struct WorkInfo {
    pub(crate) id: WorkId,
    pub(crate) name: String,
    pub(crate) work_type: EditorType,
    pub(crate) user_id: i64,
    /// 源作品的 `bcm_version`(作品详情接口给的元信息;建作品时要原样带上,空值由调用方兜底)
    pub(crate) bcm_version: String,
    /// 源作品的封面 URL(详情接口的 `preview`)。
    ///
    /// **建作品要用**:平台对 `preview` 做合法性校验,空串会被拒(`参数preview封面非法`),
    /// 所以"上传到账号"这条路上要么给源作品封面、要么给平台认的封面地址。
    pub(crate) preview: Option<String>,
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
            preview: data
                .get("preview")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string),
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
