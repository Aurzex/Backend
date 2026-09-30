//! 作品文件转换域:读写作品文件的唯一边界。
//!
//! 两个子域共用一套地基:
//!
//! - [`decompile`](反编译):作品(编译版/传输态)→ 编辑版 JSON / 源码目录树。
//!   门面 + 引擎 + 各编辑器实现都在子域内。
//! - [`translate`](互相转化):一种编辑器的作品文件 ⇄ 另一种(如 Kitten `.bcm4` ⇄ KittenN `.bcmkn`)。
//!   方案见 `docs/rounds/20-kitten-kn-work-conversion-plan.md`。
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

use crate::core::convert::decompile::{CodemaoDecompiler, DecompileOptions, DecompiledArtifact};
use crate::core::convert::shared::{DraftUpload, FileService};
use crate::core::convert::translate::{
    TargetEditor, TranslateError, TranslateOptions, TranslateOutcome, detect_editor, product_path,
    set_source_reference_in, translate_value,
};
use crate::utils::filedata::PathConfig;
use crate::utils::requests::UploadChannel;

/// 作品 id → 目标编辑器作品文件(反编译取编辑版,再转化;可选上传并新建草稿作品)
///
/// 这是**跨子域编排**:`decompile` 负责把线上作品取成编辑版 JSON,`translate` 负责重排;
/// 两个子域彼此不依赖,组合写在这里(docs/rounds/20 §6.1 依赖规则)。
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

/// `translate_work` 的主体:取**内存**编辑版 → 转化 → 可选源引用 → 落盘 → 可选上传
///
/// 方案 23 P0-2:全程内存直通,不再"反编译落盘 → `translate_file` 读回 → 改写产物
/// 再读回-写回":
///
/// 1. 反编译走 [`CodemaoDecompiler::decompile_artifact_with`](Kitten/NEKO 直接给内存文档);
/// 2. 转化走 [`translate_value`];
/// 3. 源作品引用直接改内存文档([`set_source_reference_in`]);只有**要上传源文件**
///    的那一路(`keep_source` 且 Kitten → KN)才把源文档落一次盘,文件名与旧路径逐字一致
///    (官方导入时也重传原件,这一步省不掉,但省掉了随后的读回-改写-再写);
/// 4. 产物只在最后流式写一次。
fn translate_work_in(
    work_id: WorkId,
    target: TargetEditor,
    options: TranslateOptions,
    staging: &std::path::Path,
) -> Result<TranslateOutcome, TranslateError> {
    // 1. 取编辑版(Kitten 会重建成 block_data_json;NEKO 会解密成明文 KN 文档)
    let artifact = CodemaoDecompiler::global()
        // `.save_raw(false)`:那条路上的 raw 只落在 staging,收尾整目录删掉
        // (phase 3 P1,rounds/37)—— 默认 true 会白写一份与源同量级的 JSON(9 MB 级作品 ≈ 数百 ms)。
        .decompile_artifact_with(
            work_id,
            DecompileOptions::new().output_dir(staging).save_raw(false),
        )
        .map_err(TranslateError::Decompiler)?;
    let (source_document, source_file_name, source_version, preview) = match artifact {
        DecompiledArtifact::Document {
            document,
            file_name,
            source_version,
            preview,
        } => (document, file_name, source_version, preview),
        DecompiledArtifact::Path(path) => {
            return Err(TranslateError::InvalidArgument(format!(
                "作品 {work_id} 的产物是资源形态({}):互相转化只支持编辑版文档 \
                 (Kitten4 ⇄ KittenN、NEMO → KittenN);WOOD 请用反编译接口另行处理",
                path.display()
            )));
        }
    };
    // NEMO 老作品要按源版本做迁移(`docs/rounds/27` §9.3);调用方显式给过版本就尊重调用方
    let options = if options.source_version_ref().is_none() && !source_version.is_empty() {
        options.source_version(source_version)
    } else {
        options
    };

    // 2. 需要上传源文件时,先把源文档落一次盘(上传接口吃文件路径)
    // 官方在 Kitten → KN 与 NEMO → KN 两条路上都写产物的 `source`(保留原件)
    let needs_source_upload = options.keeps_source()
        && target == TargetEditor::KittenN
        && matches!(
            detect_editor(&source_document),
            Some(
                EditorType::Kitten2 | EditorType::Kitten3 | EditorType::Kitten4 | EditorType::Nemo
            )
        );
    let source_path = if needs_source_upload {
        let path = staging.join(&source_file_name);
        FileService::write_json(&path, &source_document)?;
        Some(path)
    } else {
        None
    };

    // 3. 转化(内存)
    let converted = translate_value(source_document, target, &options)?;
    let mut document = converted.document;

    // 4. 可选:把源作品文件引用写进产物(仅 Kitten → KN,官方「保留原件」行为)
    //
    // 官方把原始 Kitten 文件重新上传、URL 挂到 KN 的 `source`;失败不影响转化结果,
    // 只记日志(产品语义:源引用是附加信息,不该让转化整体失败)。
    if let Some(source_path) = source_path {
        match upload_source_file(&source_path)
            .and_then(|url| set_source_reference_in(&mut document, &url))
        {
            Ok(()) => log::debug!("已写入源作品引用"),
            Err(error) => log::warn!("写入源作品引用失败(产物照常输出):{error}"),
        }
    }

    // 5. 落盘产物(整份文档只写这一次)
    let output = product_path(std::path::Path::new(&source_file_name), target, &options)?;
    FileService::write_json(&output, &document)?;

    // 6. 可选:上传产物并新建草稿作品(默认关;开=替用户在平台落一份草稿)
    let mut outcome = TranslateOutcome {
        output,
        work_id: None,
        target,
        report: converted.report,
    };
    if options.upload_enabled() {
        outcome.work_id = Some(create_draft_work(&outcome, work_id, target, preview)?);
    }
    Ok(outcome)
}

/// 批量转化(顺序与输入一致;并发模型与 `CodemaoDecompiler::decompile_batch` 一致:
/// 按并发数分块、块内 `thread::scope` 并发、块间顺序收集 —— 不引入锁)
///
/// 并发是**两级**的:作品级(本函数的 `batch_concurrency`)× 实体级
/// ([`TranslateOptions::entity_concurrency`],单文档内按实体并行,见 `docs/rounds/25`)。
/// 两级直接相乘会把 CPU 超订 `batch × entity` 倍,所以在入口把实体级按作品级与
/// **可用核数**折算一次(方案 25 §7 阻塞 #6):每作品分到的核数
/// `可用核数 / 有效作品并发`(取整、至少 1)就是实体级上限。折算只改并行度,不改产物。
pub fn translate_works(
    work_ids: &[WorkId],
    target: TargetEditor,
    options: TranslateOptions,
) -> Vec<Result<TranslateOutcome, TranslateError>> {
    if work_ids.is_empty() {
        return Vec::new();
    }
    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    let options = options.fold_entity_concurrency(work_ids.len(), available);
    let concurrency = options.concurrency().max(1);
    if concurrency == 1 || work_ids.len() <= 1 {
        return work_ids
            .iter()
            .map(|&id| translate_work(id, target, options.clone()))
            .collect();
    }
    let options_ref = &options;
    shared::batch_map(
        work_ids,
        concurrency,
        |&id| translate_work(id, target, options_ref.clone()),
        || TranslateError::InvalidArgument("转化线程异常".to_string()),
    )
}

/// 上传源作品文件,返回可挂到产物 `source` 的 URL(见 [`set_source_reference_in`])
///
/// 偏差记录:官方上传的是**原始** Kitten 文件字节,我们只有反编译重建的编辑版
/// (即这里落盘的这一份),故上传它(`docs/rounds/21` §4-10)。
fn upload_source_file(source_path: &std::path::Path) -> Result<String, TranslateError> {
    let client = crate::utils::requests::CodeMaoClient::global().clone();
    client
        .file_uploader()
        .upload(source_path, UploadChannel::Codemao, "convert-source")
        .map_err(TranslateError::Mew)
}

/// 上传产物并新建同名草稿作品,返回新作品 id
///
/// 上传 + 建作品 + 取名都在 `shared::upload`(**反编译侧的"上传到账号"走同一份实现**,
/// 见 `docs/rounds/30`);这里只负责把转化结果翻译成那份入参。
fn create_draft_work(
    outcome: &TranslateOutcome,
    source_work_id: WorkId,
    target: TargetEditor,
    preview: Option<String>,
) -> Result<i64, TranslateError> {
    let client = crate::utils::requests::CodeMaoClient::global().clone();
    // 平台建作品要求封面合法(`preview` 空串会被拒:"参数preview封面非法")⇒ 用**源作品的封面**;
    // 取不到就留空(仍可能被拒,但至少不自造非法值)。
    //
    // P10(rounds/37):这个封面由**反编译阶段**随产物一起带出来(`DecompiledArtifact::Document::preview`,
    // 本来就是同一个 `WorkInfo.preview`),原先却在这里**再拉一次作品详情** ——
    // 白付一个 RTT,还引入了对全局客户端的隐式依赖(见 `docs/goals/convert-backlog.md` 2b)。
    let preview = preview.filter(|value| !value.is_empty());

    let spec = DraftUpload {
        artifact: &outcome.output,
        editor: target.as_editor(),
        source_work_id,
        kind: "转化",
        save_path: "convert",
        bcm_version: "",
        n_blocks: Some(outcome.report.blocks_converted as i32),
        preview: preview.as_deref(),
    };
    crate::core::convert::shared::create_draft(&client, &spec).map_err(TranslateError::Decompiler)
}

#[cfg(test)]
mod tests {
    use super::*;

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
