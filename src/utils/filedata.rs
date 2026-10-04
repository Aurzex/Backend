use crate::utils::requests::MewResult;
use log::debug;
use std::fs;
use std::path::{Path, PathBuf};

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
    /// 写入字节数组(失败经 `MewError::Io` 上报,与其他层同一口径)
    pub fn write_bytes(path: &Path, data: &[u8]) -> MewResult<()> {
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

// 通用值/文本工具(自 `core::registry` 归位:与举报类型无关,举报引擎只是消费方)

/// 将 JSON 值转为字符串(字符串直取,数字转文本,其余空串)
pub(crate) fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// 将时间戳转换为字符串表示
pub(crate) fn timestamp_to_string(ts: &serde_json::Value) -> String {
    if let Some(secs) = ts.as_i64()
        && secs > 0
    {
        // 原实现先做 UNIX_EPOCH+Duration 再换算回秒数,结果恒等于 secs,属无意义换算
        return format!("{}", secs);
    }
    ts.to_string()
}

/// 去除 JSON 内嵌 HTML 的常见标记,转成可读纯文本
pub(crate) fn html_to_text(html: &str) -> String {
    html.replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n")
        .replace("<p>", "")
        .replace("</p>", "\n")
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// 字节数转人类可读大小(KB/MB 两位小数)
pub(crate) fn bytes_to_human(size_bytes: u64) -> String {
    if size_bytes >= 1024 * 1024 {
        format!("{:.2} MB", size_bytes as f64 / 1024.0 / 1024.0)
    } else {
        format!("{:.2} KB", size_bytes as f64 / 1024.0)
    }
}
