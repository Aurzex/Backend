use log::debug;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

// 错误定义
#[derive(Error, Debug)]
pub enum FileError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
}

// 路径配置(可自定义根目录)
/// 文件路径管理器,可基于自定义根目录构建所有子目录
#[derive(Debug, Clone)]
pub struct PathConfig {
    root: PathBuf,
}

impl Default for PathConfig {
    /// 默认使用当前工作目录作为根目录
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl PathConfig {
    /// 基于指定根目录创建路径配置
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 获取全局默认路径配置(基于当前目录)
    pub fn global() -> &'static Self {
        static INSTANCE: std::sync::OnceLock<PathConfig> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(PathConfig::default)
    }

    /// 缓存目录
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// 数据目录
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// 下载目录
    pub fn download_dir(&self) -> PathBuf {
        self.root.join("download")
    }

    /// 验证码图片路径
    pub fn captcha_file_path(&self) -> PathBuf {
        self.cache_dir().join("captcha.jpg")
    }

    /// 编译文件路径
    pub fn compile_file_path(&self) -> PathBuf {
        self.download_dir().join("compile")
    }

    /// 转化文件路径(编辑器间互相转化的输出目录)
    pub fn convert_file_path(&self) -> PathBuf {
        self.download_dir().join("convert")
    }

    /// 密码文件路径
    pub fn password_file_path(&self) -> PathBuf {
        self.data_dir().join("password.txt")
    }
}

pub struct CodeMaoFile;

impl CodeMaoFile {
    /// 写入字节数组
    pub fn write_bytes(path: &Path, data: &[u8]) -> Result<(), FileError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        debug!("写入二进制文件: {:?} ({} 字节)", path, data.len());
        fs::write(path, data)?;
        Ok(())
    }
}

/// 将 JSON 值转为 i64(数字直接取,字符串尝试解析)
pub fn value_to_i64(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        _ => None,
    }
}
