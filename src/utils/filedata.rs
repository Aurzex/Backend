use crate::utils::requests::{MewError, MewResult};
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

static GLOBAL: std::sync::OnceLock<PathConfig> = std::sync::OnceLock::new();

impl PathConfig {
    /// 基于指定根目录创建路径配置
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// 设置全局路径根(必须在任何 [`Self::global`] 调用之前)。
    ///
    /// 库内嵌到其它程序时,工作目录可能不可写或与使用者期望不一致;在初始化客户端、
    /// 登录、反编译或转换之前调用本方法,可把验证码、账号文件、下载与转化目录整体
    /// 重定向到指定根。全局根只能设置一次;重复设置返回
    /// [`MewError::InvalidArgument`]。
    pub fn set_global(root: impl Into<PathBuf>) -> MewResult<()> {
        GLOBAL.set(Self::with_root(root)).map_err(|_| {
            MewError::InvalidArgument(
                "PathConfig 全局根已初始化;set_global 必须在任何 PathConfig::global() 调用之前"
                    .into(),
            )
        })
    }

    /// 获取全局默认路径配置(基于当前目录)
    pub fn global() -> &'static Self {
        GLOBAL.get_or_init(PathConfig::default)
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

    /// 产物目录(默认 `<root>/target/convert-out`)。
    ///
    /// **不再写进 `download/`**:那里是语料与夹具目录(只读),产物与夹具同名时会覆盖夹具
    /// (rounds/40 的 `kn-9.4MB` 就是这样被真机门覆盖的)。产物改随构建产物同生命周期;
    /// 需要别的落点时用调用方的 `output_dir` 显式覆盖。
    pub fn output_dir(&self) -> PathBuf {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|dir| {
                if dir.is_absolute() {
                    dir
                } else {
                    self.root.join(dir)
                }
            })
            .unwrap_or_else(|| self.root.join("target"));
        target.join("convert-out")
    }

    /// 编译文件路径(默认 `<output_dir>/compile`)
    pub fn compile_file_path(&self) -> PathBuf {
        self.output_dir().join("compile")
    }

    /// 转化文件路径(编辑器间互相转化的输出目录,默认 `<output_dir>/convert`)
    pub fn convert_file_path(&self) -> PathBuf {
        self.output_dir().join("convert")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 产物目录与语料目录分离(rounds/40 的夹具被覆盖事故的根治项):默认产物落在
    /// `target/convert-out`,不再写进 `download/`。
    #[test]
    fn output_dirs_stay_out_of_the_fixture_root() {
        let root = std::path::PathBuf::from("/tmp/backend-pathcfg");
        let cfg = PathConfig::with_root(root.clone());
        assert_eq!(cfg.output_dir(), root.join("target/convert-out"));
        assert!(cfg.compile_file_path().starts_with(cfg.output_dir()));
        assert!(cfg.convert_file_path().starts_with(cfg.output_dir()));
        assert!(!cfg.compile_file_path().starts_with(cfg.download_dir()));
        assert!(!cfg.convert_file_path().starts_with(cfg.download_dir()));
    }

    #[test]
    fn global_is_stable_and_second_set_fails() {
        let root = PathConfig::global().root.clone();
        assert_eq!(PathConfig::global().root, root, "global 必须稳定");
        assert!(
            PathConfig::set_global("/tmp/backend-other-root").is_err(),
            "全局根已初始化后不得再次设置"
        );
    }
}
