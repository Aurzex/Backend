//! 常用类型与 trait 的预导入(prelude)
//!
//! 下游一行 `use backend::prelude::*;` 即可获得客户端、请求样板 trait、
//! 错误类型、身份枚举与分页迭代器等高频使用项,降低 `use` 门槛。
//! 刻意不重导出 13 个业务 Manager(避免 work.rs 与 user.rs 各自
//! `KittenVersion` 同名导致的歧义),按域自行 `use backend::api::…` 即可。

pub use crate::utils::requests::{
    Identity, ClientAccess, CodeMaoClient, HttpMethod, MewRequestBuilder, MewError, MewResult,
    PaginatedIter,
};
