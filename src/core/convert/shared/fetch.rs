use super::error::Result;
use super::model::WorkInfo;
use serde_json::Value;
use std::sync::Arc;

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
