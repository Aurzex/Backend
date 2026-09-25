//! 作品文件转换域共享地基:错误、JSON 访问、配置(含影子模板大表)、作品模型、文件/ID、
//! 加密、HTTP 与抓取契约。`decompile`(读)与后续 `translate`(写)两个子域共用,不对外暴露。
//!
//! 子域通过本文件的汇总再导出取用(`crate::core::convert::shared::{…}`),不直接引用子模块路径。

pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod infra;
pub(crate) mod model;

// 错误与错误上下文
pub(crate) use error::{Result, ResultExt};
// 配置 + 影子模板 + 影子构建器
pub(crate) use config::{DecompilerConfig, ShadowBuilder, ShadowTemplate};
// 基础设施:加密 / HTTP / 文件 / JSON 取值
pub(crate) use infra::{CodeMaoHttpClient, CryptoService, FileService, HttpClient, ValueExt};
// 模型:编辑器判别、作品信息、抓取契约、id 生成
pub(crate) use model::{IdGenerator, RawWorkData, WorkFetcher, WorkInfo};

// 门面项(`convert/mod.rs` 对外再导出,保持 `pub` 可见性)
pub use error::DecompilerError;
pub use model::WorkId;
// 编辑器枚举:两子域公开面都要用,`EditorType` 对外暴露(见 docs/rounds/20 §6.1 D3)
pub use model::EditorType;
