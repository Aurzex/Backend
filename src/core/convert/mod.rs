//! 作品文件转换域:读写作品文件的唯一边界。
//!
//! 两个子域共用一套地基:
//!
//! - [`decompile`](反编译):作品(编译版/传输态)→ 编辑版 JSON / 源码目录树。
//!   门面 + 引擎 + 各编辑器实现都在子域内。
//! - [`translate`](互相转化):一种编辑器的作品文件 ⇄ 另一种(如 Kitten `.bcm4` ⇄ KittenN `.bcmkn`)。
//!   方案见 `docs/20-kitten-kn-work-conversion-plan.md`。
//!
//! 域内约定:
//!
//! - `shared` 是两子域共用的地基(错误 / 模型 / 配置 / 加密 / 文件 / HTTP / 抓取),不对外暴露;
//! - 子域之间不互相依赖,跨子域编排写在本文(域门面)里;
//! - 域外只从 `decompile` / `translate` 两个公开面取东西,跨子域类型从本文件取。

pub mod decompile;
pub(crate) mod shared;
pub mod translate;

// 跨子域类型:域内两处都要用,只在这里留一条公开路径
pub use crate::core::convert::shared::{DecompilerError, EditorType, WorkId};

use crate::api::work::{
    CreateKittenWorkArgs, CreateKnWorkArgs, KittenWorkManager, NekoWorkManager,
};
use crate::core::convert::decompile::{CodemaoDecompiler, DecompileOptions};
use crate::core::convert::translate::{
    TargetEditor, TranslateError, TranslateOptions, TranslateOutcome, set_source_reference,
    translate_file,
};
use crate::utils::filedata::{PathConfig, value_to_i64};
use crate::utils::requests::{MewError, UploadChannel};
use serde_json::Value;

/// 作品 id → 目标编辑器作品文件(反编译取编辑版,再转化;可选上传并新建草稿作品)
///
/// 这是**跨子域编排**:`decompile` 负责把线上作品取成编辑版 JSON,`translate` 负责重排;
/// 两个子域彼此不依赖,组合写在这里(docs/20 §6.1 依赖规则)。
pub fn translate_work(
    work_id: WorkId,
    target: TargetEditor,
    options: TranslateOptions,
) -> Result<TranslateOutcome, TranslateError> {
    let staging = staging_dir(work_id);
    std::fs::create_dir_all(&staging)?;

    let result = translate_work_in(work_id, target, options, &staging);

    // 收尾:只清本作品自己的中间产物(失败路径也清,不留垃圾;
    // 编辑版原文由调用方按需另行反编译,产物落在 `staging` 之外)
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// `translate_work` 的中间产物目录:每个作品**每次调用**一个独立子目录。
///
/// `translate_works` 会并发跑同一批次,共用一个目录时先完成的线程会把其它线程
/// 正在读的源文件一起删掉;随机后缀兜住"同一作品重跑"的情况。
fn staging_dir(work_id: WorkId) -> std::path::PathBuf {
    PathConfig::global()
        .convert_file_path()
        .join("staging")
        .join(format!("{work_id}-{:08x}", fastrand::u32(..)))
}

/// `translate_work` 的主体:在 `staging` 里取编辑版 → 转化 → 可选上传
fn translate_work_in(
    work_id: WorkId,
    target: TargetEditor,
    options: TranslateOptions,
    staging: &std::path::Path,
) -> Result<TranslateOutcome, TranslateError> {
    // 1. 取编辑版:反编译(Kitten 会重建成 block_data_json;NEKO 会解密成明文 KN 文档)
    let source_path = CodemaoDecompiler::global()
        .decompile_with_options(work_id, DecompileOptions::new().output_dir(staging))
        .map_err(TranslateError::Decompiler)?;

    // 2. 转化
    let mut outcome = translate_file(&source_path, target, options.clone())?;

    // 2.5 可选:把源作品文件引用写进产物(仅 Kitten → KN,官方「保留原件」行为)
    //
    // 官方把原始 Kitten 文件重新上传、URL 挂到 KN 的 `source`;失败不影响转化结果,
    // 只记日志(产品语义:源引用是附加信息,不该让转化整体失败)。
    if options.keeps_source()
        && target == TargetEditor::KittenN
        && matches!(outcome.report.from, EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4)
    {
        match attach_source_reference(&outcome, &source_path) {
            Ok(()) => log::debug!("已写入源作品引用:{:?}", outcome.output),
            Err(error) => log::warn!("写入源作品引用失败(产物照常输出):{error}"),
        }
    }

    // 3. 可选:上传产物并新建草稿作品(默认关;开=替用户在平台落一份草稿)
    let work_id_created = if options.upload_enabled() {
        Some(create_draft_work(&outcome, work_id, target)?)
    } else {
        None
    };

    Ok(TranslateOutcome {
        output: outcome.output,
        // `translate_file` 只写文件、不建作品,所以 `outcome.work_id` 恒为 None
        work_id: work_id_created,
        target: outcome.target,
        report: outcome.report,
    })
}

/// 批量转化(顺序与输入一致;并发模型与 `CodemaoDecompiler::decompile_batch` 一致:
/// 按并发数分块、块内 `thread::scope` 并发、块间顺序收集 —— 不引入锁)
pub fn translate_works(
    work_ids: &[WorkId],
    target: TargetEditor,
    options: TranslateOptions,
) -> Vec<Result<TranslateOutcome, TranslateError>> {
    if work_ids.is_empty() {
        return Vec::new();
    }
    let concurrency = options.concurrency().max(1);
    if concurrency == 1 || work_ids.len() <= 1 {
        return work_ids
            .iter()
            .map(|&id| translate_work(id, target, options.clone()))
            .collect();
    }
    let options_ref = &options;
    let mut results = Vec::with_capacity(work_ids.len());
    for chunk in work_ids.chunks(concurrency) {
        let chunk_results: Vec<Result<TranslateOutcome, TranslateError>> =
            std::thread::scope(|scope| {
                let handles: Vec<_> = chunk
                    .iter()
                    .map(|&id| scope.spawn(move || translate_work(id, target, options_ref.clone())))
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| {
                        handle.join().unwrap_or_else(|_| {
                            Err(TranslateError::InvalidArgument("转化线程异常".to_string()))
                        })
                    })
                    .collect()
            });
        results.extend(chunk_results);
    }
    results
}

/// 上传源作品文件,并把返回的 URL 写进产物顶层 `source`(见 [`set_source_reference`])
///
/// 偏差记录:官方上传的是**原始** Kitten 文件字节,我们只有反编译重建的编辑版
/// (`staging` 里的那一份),故上传它(`docs/21` §4-10)。
fn attach_source_reference(
    outcome: &TranslateOutcome,
    source_path: &std::path::Path,
) -> Result<(), TranslateError> {
    let client = crate::utils::requests::CodeMaoClient::global().clone();
    let url = client
        .file_uploader()
        .upload(source_path, UploadChannel::Codemao, "convert-source")
        .map_err(TranslateError::Mew)?;
    set_source_reference(&outcome.output, &url)
}

/// 草稿作品名:标注目标编辑器与来源作品 id,并显式标「可删」
/// (会直接展示在用户的平台草稿列表里)
fn draft_name(source_work_id: WorkId, target: TargetEditor) -> String {
    let label = match target {
        TargetEditor::KittenN => "KittenN",
        TargetEditor::Kitten4 => "Kitten4",
    };
    format!("转化副本 {label} ← {source_work_id}(可删)")
}

/// 上传产物并新建同名草稿作品,返回新作品 id
fn create_draft_work(
    outcome: &TranslateOutcome,
    source_work_id: WorkId,
    target: TargetEditor,
) -> Result<i64, TranslateError> {
    let client = crate::utils::requests::CodeMaoClient::global().clone();
    let uploader = client.file_uploader();
    let url = uploader
        .upload(&outcome.output, UploadChannel::Codemao, "convert")
        .map_err(TranslateError::Mew)?;

    let name = draft_name(source_work_id, target);
    let created: Value = match target {
        TargetEditor::KittenN => NekoWorkManager::new()
            .create_kn_work(CreateKnWorkArgs {
                name: &name,
                work_url: &url,
                preview_url: "",
                bcm_version: crate::core::convert::translate::tables_gen::BCM_VERSION,
                save_type: Some(2),
                stage_type: Some(2),
                n_blocks: Some(outcome.report.blocks_converted as i32),
                n_roles: None,
                n_scenes: None,
                pic_need_check_file_url: None,
            })
            .map_err(TranslateError::Mew)?,
        TargetEditor::Kitten4 => KittenWorkManager::new()
            .create_kitten_work(CreateKittenWorkArgs {
                name: &name,
                work_url: &url,
                preview: "",
                version: "4.11.20",
                orientation: None,
                sample_id: None,
                work_source_label: Some(1),
                save_type: Some(2),
            })
            .map_err(TranslateError::Mew)?,
    };

    let id = created
        .get("id")
        .and_then(value_to_i64)
        .or_else(|| {
            created
                .get("data")
                .and_then(|d| d.get("id"))
                .and_then(value_to_i64)
        })
        .ok_or_else(|| {
            TranslateError::Mew(MewError::InvalidArgument(format!(
                "新建作品成功但响应里没有 id:{created}"
            )))
        })?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 草稿名要能看出目标与来源,并标明可删(会展示在平台草稿列表里)
    #[test]
    fn draft_name_marks_target_source_and_deletable() {
        let name = draft_name(WorkId::new(123), TargetEditor::KittenN);
        assert!(name.contains("KittenN"), "{name}");
        assert!(name.contains("123"), "{name}");
        assert!(name.contains("可删"), "{name}");
    }

    /// P0-1 回归:staging 必须是"每作品每次调用"独立目录(曾共用一个目录导致并发互删)
    #[test]
    fn staging_dir_is_unique_per_call_and_scoped_to_work() {
        let a = staging_dir(WorkId::new(7));
        let b = staging_dir(WorkId::new(7));
        assert_ne!(a, b, "同作品两次调用也不能共用目录");
        assert!(a.starts_with(PathConfig::global().convert_file_path().join("staging")));
        assert!(
            a.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("7-")),
            "目录名要能看出来源作品:{a:?}"
        );
    }
}
