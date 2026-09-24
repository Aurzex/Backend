use super::context::DecompilerContext;
use crate::core::convert::shared::{DecompilerError, FileService, RawWorkData, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

// 结果类型与 Trait
#[derive(Debug)]
pub(crate) enum DecompileResult {
    Json(Value),
    Path(String),
}

pub(crate) trait WorkDecompiler: Send + Sync {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult>;
    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf>;
}

/// 将 JSON 反编译结果写入输出目录,返回文件路径(供各反编译器共用)
pub(crate) fn save_json_result(
    result: &DecompileResult,
    output_dir: Option<&Path>,
    context: &DecompilerContext,
    extension: &str,
    decompiler_name: &str,
) -> Result<PathBuf> {
    match result {
        DecompileResult::Json(json) => {
            let output_path = output_dir.unwrap_or(&context.config.default_output_dir);
            FileService::ensure_dir(output_path)?;
            let filename = FileService::safe_filename(
                &context.work_info.name,
                context.work_info.id.get(),
                extension,
            );
            let filepath = output_path.join(filename);
            FileService::write_json(&filepath, json)?;
            Ok(filepath)
        }
        _ => Err(DecompilerError::Decompile(format!(
            "{}反编译器应返回JSON",
            decompiler_name
        ))),
    }
}

/// 返回路径型反编译结果(供返回路径的反编译器共用)
pub(crate) fn save_path_result(result: &DecompileResult, decompiler_name: &str) -> Result<PathBuf> {
    match result {
        DecompileResult::Path(path) => Ok(PathBuf::from(path)),
        _ => Err(DecompilerError::Decompile(format!(
            "{}反编译器应返回路径",
            decompiler_name
        ))),
    }
}
