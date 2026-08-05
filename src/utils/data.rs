use log::debug;
use serde_json::{Value, to_string_pretty};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ==================== 错误定义 ====================
#[derive(Error, Debug)]
pub enum FileError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 序列化错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("类型错误: {0}")]
    TypeError(String),
    #[error("未知错误: {0}")]
    Other(String),
}

// ==================== 路径配置（可自定义根目录） ====================
/// 文件路径管理器，可基于自定义根目录构建所有子目录。
///
/// # 示例
/// ```no_run
/// let paths = PathConfig::with_root("/var/myapp");
/// let data_dir = paths.data_dir();
/// ```
#[derive(Debug, Clone)]
pub struct PathConfig {
    root: PathBuf,
}

impl Default for PathConfig {
    /// 默认使用当前工作目录作为根目录。
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
}

impl PathConfig {
    /// 基于指定根目录创建路径配置。
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 获取全局默认路径配置（基于当前目录）。
    pub fn global() -> &'static Self {
        static INSTANCE: std::sync::OnceLock<PathConfig> = std::sync::OnceLock::new();
        INSTANCE.get_or_init(PathConfig::default)
    }

    /// 缓存目录。
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// 数据目录。
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// 下载目录。
    pub fn download_dir(&self) -> PathBuf {
        self.root.join("download")
    }

    /// 验证码图片路径。
    pub fn captcha_file_path(&self) -> PathBuf {
        self.cache_dir().join("captcha.jpg")
    }

    /// 编译文件路径。
    pub fn compile_file_path(&self) -> PathBuf {
        self.download_dir().join("compile")
    }

    /// 小说文件路径。
    pub fn fiction_file_path(&self) -> PathBuf {
        self.download_dir().join("fiction")
    }

    /// Token 文件路径。
    pub fn token_file_path(&self) -> PathBuf {
        self.data_dir().join("token.txt")
    }

    /// 密码文件路径。
    pub fn password_file_path(&self) -> PathBuf {
        self.data_dir().join("password.txt")
    }

    /// 确保所有需要的目录存在。
    pub fn ensure_directories(&self) -> Result<(), FileError> {
        debug!("创建必要的目录结构 (root: {:?})", self.root);
        fs::create_dir_all(self.cache_dir())?;
        fs::create_dir_all(self.data_dir())?;
        fs::create_dir_all(self.download_dir())?;
        Ok(())
    }
}

// ==================== 文件内容类型 ====================
/// 表示要写入文件的不同内容形式。
pub enum FileContent {
    /// 普通文本。
    Text(String),
    /// 二进制数据。
    Bytes(Vec<u8>),
    /// JSON 值（会格式化为美化文本）。
    Json(Value),
    /// 多行文本，会以换行符连接。
    Lines(Vec<String>),
}

// ==================== 文件写入工具 ====================
/// 封装了基于 `FileContent` 的安全文件写入操作。
///
/// 所有写入方法都会自动创建父目录，并记录相应日志。
pub struct CodeMaoFile;

impl CodeMaoFile {
    /// 将 `FileContent` 写入到指定路径，根据内容类型自动选择写入模式。
    ///
    /// 此方法为统一入口，内部委托给具体类型方法。
    pub fn file_write(path: &Path, content: &FileContent) -> Result<(), FileError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        match content {
            FileContent::Text(s) => Self::write_text(path, s),
            FileContent::Bytes(b) => Self::write_bytes(path, b),
            FileContent::Json(obj) => Self::write_json(path, obj),
            FileContent::Lines(lines) => Self::write_lines(path, lines),
        }
    }

    /// 写入普通文本字符串。
    pub fn write_text(path: &Path, text: &str) -> Result<(), FileError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        debug!("写入文本文件: {:?} ({} 字符)", path, text.len());
        fs::write(path, text)?;
        Ok(())
    }

    /// 写入字节数组。
    pub fn write_bytes(path: &Path, data: &[u8]) -> Result<(), FileError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        debug!("写入二进制文件: {:?} ({} 字节)", path, data.len());
        fs::write(path, data)?;
        Ok(())
    }

    /// 将 JSON 值序列化为美化的字符串后写入。
    pub fn write_json(path: &Path, value: &Value) -> Result<(), FileError> {
        let json_str = to_string_pretty(value)?;
        Self::write_text(path, &json_str)
    }

    /// 将字符串数组以换行符连接后写入文本文件。
    pub fn write_lines(path: &Path, lines: &[String]) -> Result<(), FileError> {
        let content = lines.join("\n");
        Self::write_text(path, &content)
    }
}
