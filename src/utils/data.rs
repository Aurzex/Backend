use serde_json::{Value, to_string_pretty};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

// 错误定义
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("类型错误: {0}")]
    TypeError(String),
    #[error("未知错误: {0}")]
    Other(String),
}

// 路径配置（仅保留所需路径）
pub struct PathConfig;

impl PathConfig {
    pub fn current_dir() -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    pub fn cache_dir() -> PathBuf {
        Self::current_dir().join("cache")
    }

    pub fn data_dir() -> PathBuf {
        Self::current_dir().join("data")
    }

    pub fn download_dir() -> PathBuf {
        Self::current_dir().join("download")
    }

    pub fn captcha_file_path() -> PathBuf {
        Self::cache_dir().join("captcha.jpg")
    }

    pub fn compile_file_path() -> PathBuf {
        Self::download_dir().join("compile")
    }

    pub fn fiction_file_path() -> PathBuf {
        Self::download_dir().join("fiction")
    }

    pub fn token_file_path() -> PathBuf {
        Self::data_dir().join("token.txt")
    }

    pub fn password_file_path() -> PathBuf {
        Self::data_dir().join("password.txt")
    }

    pub fn ensure_directories() -> Result<(), ConfigError> {
        fs::create_dir_all(Self::cache_dir())?;
        fs::create_dir_all(Self::data_dir())?;
        fs::create_dir_all(Self::download_dir())?;
        Ok(())
    }
}

// 文件内容类型
pub enum FileContent {
    Text(String),
    Bytes(Vec<u8>),
    Json(Value),
    Lines(Vec<String>),
}

// 文件写入工具
pub struct CodeMaoFile;

impl CodeMaoFile {
    pub fn file_write(path: &Path, content: &FileContent, method: &str) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        match content {
            FileContent::Text(s) => {
                if method.contains('b') {
                    return Err(ConfigError::TypeError(format!(
                        "文本内容不能使用二进制模式: {}",
                        method
                    )));
                }
                fs::write(path, s)?;
            }
            FileContent::Bytes(b) => {
                fs::write(path, b)?;
            }
            FileContent::Json(obj) => {
                if method.contains('b') {
                    return Err(ConfigError::TypeError(format!(
                        "JSON内容不能使用二进制模式: {}",
                        method
                    )));
                }
                let json_str = to_string_pretty(obj)?;
                fs::write(path, json_str)?;
            }
            FileContent::Lines(lines) => {
                if method.contains('b') {
                    return Err(ConfigError::TypeError(format!(
                        "文本内容不能使用二进制模式: {}",
                        method
                    )));
                }
                let content = lines.join("\n");
                fs::write(path, content)?;
            }
        }

        Ok(())
    }
}
