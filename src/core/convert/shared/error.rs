use crate::utils::requests::MewError;
use std::error::Error;
use thiserror::Error;

// 错误定义
#[derive(Error, Debug)]
pub enum DecompilerError {
    #[error("外部错误: {0}")]
    Mew(#[from] MewError),
    #[error("加密错误: {0}")]
    Crypto(String),
    #[error("作品解析失败: {0}")]
    Decompile(String),
    #[error("不支持的作品类型: {0}")]
    UnsupportedType(String),
    #[error("无效的响应数据: {0}")]
    InvalidResponse(String),
    #[error("缺少字段: {field}")]
    MissingField { field: String },
    #[error("类型不匹配: 期望 {expected}, 实际 {actual}")]
    TypeMismatch { expected: String, actual: String },
    #[error("{msg}")]
    Other {
        msg: String,
        #[source]
        source: Option<Box<dyn Error + Send + Sync>>,
    },
}

impl From<std::io::Error> for DecompilerError {
    fn from(e: std::io::Error) -> Self {
        DecompilerError::Mew(e.into())
    }
}

impl From<serde_json::Error> for DecompilerError {
    fn from(e: serde_json::Error) -> Self {
        DecompilerError::Mew(e.into())
    }
}

pub(crate) type Result<T> = std::result::Result<T, DecompilerError>;

// 错误上下文扩展
pub(crate) trait ResultExt<T> {
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|e| DecompilerError::Other {
            msg: f(),
            source: Some(Box::new(e)),
        })
    }
}
