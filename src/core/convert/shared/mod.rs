//! 作品文件转换域共享地基:错误、JSON 访问、配置(含影子模板大表)、作品模型、文件/ID、
//! 加密、HTTP 与抓取契约。`decompile`(读)与后续 `translate`(写)两个子域共用,不对外暴露。
//!
//! 子域通过本文件的汇总再导出取用(`crate::core::convert::shared::{…}`),不直接引用子模块路径。

pub(crate) mod config;
pub(crate) mod crypto;
pub(crate) mod error;
pub(crate) mod fetch;
pub(crate) mod files;
pub(crate) mod http;
pub(crate) mod json;
pub(crate) mod model;

// 错误与错误上下文
pub(crate) use error::{Result, ResultExt};
// JSON 访问扩展
pub(crate) use json::ValueExt;
// 配置与影子模板
pub(crate) use config::{DecompilerConfig, ShadowTemplate};
// 作品模型(编辑器判别、扩展名表)
pub(crate) use model::WorkInfo;
// 文件与 ID
pub(crate) use files::{FileService, IdGenerator};
// 加密
pub(crate) use crypto::CryptoService;
// HTTP
pub(crate) use http::{CodeMaoHttpClient, HttpClient};
// 抓取契约
pub(crate) use fetch::{RawWorkData, WorkFetcher};

// 门面项(`convert/mod.rs` 对外再导出,保持 `pub` 可见性)
pub use error::DecompilerError;
pub use model::WorkId;
// 编辑器枚举:两子域公开面都要用,`EditorType` 对外暴露(见 docs/20 §6.1 D3)
pub use model::EditorType;
