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
    TargetEditor, TranslateError, TranslateOptions, TranslateOutcome, translate_file,
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
    let staging = PathConfig::global()
        .download_dir()
        .join("convert")
        .join("staging");
    std::fs::create_dir_all(&staging)?;

    // 1. 取编辑版:反编译(Kitten 会重建成 block_data_json;NEKO 会解密成明文 KN 文档)
    let source_path = CodemaoDecompiler::global()
        .decompile_with_options(work_id, DecompileOptions::new().output_dir(&staging))
        .map_err(TranslateError::Decompiler)?;

    // 2. 转化
    let outcome = translate_file(&source_path, target, options.clone())?;

    // 3. 可选:上传产物并新建草稿作品(默认关;开=替用户在平台落一份草稿)
    let work_id_created = if options.upload_enabled() {
        Some(create_draft_work(&outcome, work_id, target)?)
    } else {
        None
    };

    // 4. 清掉中间产物(编辑版原文由调用方按需另行反编译)
    let _ = std::fs::remove_dir_all(&staging);

    Ok(TranslateOutcome {
        output: outcome.output,
        work_id: work_id_created.or(outcome.work_id),
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

    // 名称:沿用源作品名 + 明确标注是转化产物(便于用户识别与删除)
    let name = format!("转化自检 {} (可删)", i64::from(source_work_id));
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
