use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, FileService, HttpClient, IdGenerator, Result, WorkInfo,
};
use std::sync::Arc;

pub(crate) struct DecompilerContext {
    pub(crate) work_info: WorkInfo,
    pub(crate) http_client: Box<dyn HttpClient>,
    pub(crate) file_service: FileService,
    pub(crate) id_generator: IdGenerator,
    pub(crate) config: Arc<DecompilerConfig>,
}

// Context Builder
pub(crate) struct DecompilerContextBuilder {
    work_info: Option<WorkInfo>,
    http_client: Option<Box<dyn HttpClient>>,
    config: Option<Arc<DecompilerConfig>>,
    id_generator: Option<IdGenerator>,
}

impl Default for DecompilerContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DecompilerContextBuilder {
    pub(crate) fn new() -> Self {
        Self {
            work_info: None,
            http_client: None,
            config: None,
            id_generator: None,
        }
    }

    pub(crate) fn work_info(mut self, info: WorkInfo) -> Self {
        self.work_info = Some(info);
        self
    }

    pub(crate) fn http_client(mut self, client: Box<dyn HttpClient>) -> Self {
        self.http_client = Some(client);
        self
    }

    pub(crate) fn config(mut self, config: Arc<DecompilerConfig>) -> Self {
        self.config = Some(config);
        self
    }

    pub(crate) fn id_generator(mut self, generator: IdGenerator) -> Self {
        self.id_generator = Some(generator);
        self
    }

    pub(crate) fn build(self) -> Result<DecompilerContext> {
        let config = self.config.unwrap_or_default();
        Ok(DecompilerContext {
            work_info: self.work_info.ok_or_else(|| DecompilerError::Other {
                msg: "缺少work_info".into(),
                source: None,
            })?,
            http_client: self.http_client.ok_or_else(|| DecompilerError::Other {
                msg: "缺少http_client".into(),
                source: None,
            })?,
            file_service: FileService::new(config.clone()),
            id_generator: self.id_generator.unwrap_or_default(),
            config,
        })
    }
}
