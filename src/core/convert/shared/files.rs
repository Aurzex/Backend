use super::config::DecompilerConfig;
use super::error::Result;
use serde_json::{Value, to_string};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// 文件服务
#[derive(Clone)]
pub(crate) struct FileService {
    config: Arc<DecompilerConfig>,
}

impl FileService {
    pub(crate) fn new(config: Arc<DecompilerConfig>) -> Self {
        Self { config }
    }

    pub(crate) fn safe_filename(name: &str, work_id: i64, extension: &str) -> String {
        let safe_name: String = name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
            .collect();
        let safe_name = safe_name.trim();
        let name_part = if safe_name.is_empty() {
            format!("work_{}", work_id)
        } else {
            safe_name.to_string()
        };
        let ext = if !extension.is_empty() && !extension.starts_with('.') {
            format!(".{}", extension)
        } else {
            extension.to_string()
        };
        format!("{}_{}{}", name_part, work_id, ext)
    }

    pub(crate) fn ensure_dir(path: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(path)?;
        Ok(path.to_path_buf())
    }

    pub(crate) fn write_json(path: &Path, data: &Value) -> Result<()> {
        let json_str = to_string(data)?;
        std::fs::write(path, json_str)?;
        Ok(())
    }

    pub(crate) fn write_binary(path: &Path, data: &[u8]) -> Result<()> {
        std::fs::write(path, data)?;
        Ok(())
    }
}

// 新 ID 生成器(方案一风格)
#[derive(Clone)]
pub(crate) struct IdGenerator {
    chars: Vec<char>,
}

impl IdGenerator {
    pub(crate) fn new() -> Self {
        let chars = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .chars()
            .collect();
        Self { chars }
    }

    pub(crate) fn generate(&self, length: usize) -> String {
        (0..length)
            .map(|_| {
                let idx = fastrand::usize(0..self.chars.len());
                self.chars[idx]
            })
            .collect()
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}
