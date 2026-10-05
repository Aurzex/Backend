//! 「上传到账号」:把产物文件传到当前账号名下,并在平台建一份同名草稿(反编译 / 转化两个子域共用)。
//!
//! 层级定位:**域级工具层**,与域门面 `mod.rs` 同级 —— 它允许依赖子域 `translate` 的
//! 生成常量(`tables_gen::BCM_VERSION`),但**不放**在地基 `shared.rs` 里:`shared` 必须对
//! 两个子域零反向依赖,而这里的 `bcm_version` 兜底需要一个"本库常量"的**单一定义源**
//! (见 `docs/rounds/39` §W2a 与 `docs/rounds/30` §4:空串 ⇒ 用库常量兜底是刻意语义,
//! 反编译侧「上传到账号」也走同一条实现)。
//!
//! 只做三件事:**上传文件 → 建作品 → 取回 id**。不做资源重传、不做 URL 改写
//! (NEMO 的造型/音频仍指向源 CDN;见 `docs/rounds/30` §NEMO)。
//! 草稿名一律带来源作品 id 与「可删」标记 —— 它会直接出现在用户的平台草稿列表里。

use crate::api::work::{
    CreateKittenWorkArgs, CreateKnWorkArgs, CreateNemoWorkArgs, KittenWorkManager, NekoWorkManager,
    NemoWorkManager,
};
use crate::core::convert::shared::{ConvertError, EditorType, Result, WorkId};
use crate::utils::filedata::value_to_i64;
use crate::utils::requests::{CodeMaoClient, MewError, UploadChannel};
use serde_json::Value;
use std::path::Path;

/// Kitten4 建作品要的**应用版本**(不是 `bcm_version`)
const KITTEN4_APP_VERSION: &str = "4.11.20";

/// 上传产物并建草稿的入参
pub(crate) struct DraftUpload<'a> {
    /// 产物文件路径(反编译产物 / 转化产物)
    pub(crate) artifact: &'a Path,
    /// 产物对应的编辑器(决定建作品的端点与上传渠道)
    pub(crate) editor: EditorType,
    /// 源作品 id(只用于草稿命名)
    pub(crate) source_work_id: WorkId,
    /// 草稿名里的行为词:「转化」/「反编译」
    pub(crate) kind: &'a str,
    /// 七牛上传的子路径(落 bucket 用)
    pub(crate) save_path: &'a str,
    /// 源作品的 `bcm_version`(空则用本库常量兜底)
    pub(crate) bcm_version: &'a str,
    /// 积木数(仅展示用;反编译侧通常拿不到,传 `None` 走平台默认)
    pub(crate) n_blocks: Option<i32>,
    /// 封面(平台建作品校验"preview"合法:空串会被拒)。
    /// 由调用方提供(通常是**源作品**的封面);`None` 时按空串发,平台可能拒。
    pub preview: Option<&'a str>,
}

/// 该编辑器有没有已知的建作品端点(其余类型不能"上传到账号")
pub(crate) fn supports_account_upload(editor: EditorType) -> bool {
    matches!(
        editor,
        EditorType::Kitten4 | EditorType::Neko | EditorType::Nemo
    )
}

/// 草稿名:标明行为、编辑器与来源作品,并显式标「可删」
pub(crate) fn draft_name(kind: &str, editor: EditorType, source: WorkId) -> String {
    format!("{kind}副本 {} ← {source}(可删)", editor_label(editor))
}

fn editor_label(editor: EditorType) -> &'static str {
    match editor {
        EditorType::Kitten2 | EditorType::Kitten3 => "Kitten3",
        EditorType::Kitten4 => "Kitten4",
        EditorType::Neko => "KittenN",
        EditorType::Nemo => "NEMO",
        EditorType::Coco => "Coco",
        EditorType::Wood => "Wood",
    }
}

/// 平台**单包上传上限**(实测,2026-09-26)
///
/// 用与生产路径**同一条渠道**(`UploadChannel::Codemao` + `save_path = "convert-source"`)逐档实测:
/// 5 / 9 / 10 / 12 / 14 / 16 / **20 MB 全部成功**,**24 / 30 MB 均被 qiniu 拒 `413`**
/// ⇒ 上限落在 **20~24 MB** 之间。上传速率约 200 KB/s(20 MB 要 ~105 s),这也是上传路径
/// 必须单独放宽超时(`UPLOAD_TIMEOUT = 600 s`,A1)的原因。
///
/// 超过它就**提前报错**:别让用户白等几分钟再吃一个 `413`。要传更大的作品得做分片上传
/// (见 `docs/goals/convert-backlog.md`);真实 KN 产物多在 3~9 MB,不阻塞日常使用。
const SINGLE_PACKAGE_LIMIT: u64 = 20 * 1024 * 1024;

/// 产物超过单包上限时提前失败(见 [`SINGLE_PACKAGE_LIMIT`])
fn ensure_single_package_fits(path: &Path) -> Result<()> {
    let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if size > SINGLE_PACKAGE_LIMIT {
        return Err(ConvertError::Other {
            msg: format!(
                "产物 {:.1} MB 超过平台单包上传上限(实测 20 MB 可传、24 MB 被 413 拒):{}",
                size as f64 / (1024.0 * 1024.0),
                path.display()
            ),
            source: None,
        });
    }
    Ok(())
}

/// 上传渠道:只有 NEMO 的凭证项目名特殊(`nemo_android_ios`),其余走社区前端
fn channel_for(editor: EditorType) -> UploadChannel {
    match editor {
        EditorType::Nemo => UploadChannel::Nemo,
        _ => UploadChannel::Codemao,
    }
}

/// 上传产物文件并新建草稿作品,返回新作品 id
///
/// 客户端由调用方注入(与第 14–17 轮的注入纪律一致;避免再用全局 `new()`)。
pub(crate) fn create_draft(client: &CodeMaoClient, spec: &DraftUpload<'_>) -> Result<i64> {
    // 先量体积:超上限就别开传(实测上传 ~200 KB/s,白等几分钟才吃 413 太亏)
    ensure_single_package_fits(spec.artifact)?;
    let url =
        client
            .file_uploader()
            .upload(spec.artifact, channel_for(spec.editor), spec.save_path)?;
    let name = draft_name(spec.kind, spec.editor, spec.source_work_id);
    let bcm_version = if spec.bcm_version.is_empty() {
        // 单一定义源:目标格式版本常量在生成物 `tables_gen` 里(不复制常量、不改兜底语义)
        crate::core::convert::translate::tables_gen::BCM_VERSION
    } else {
        spec.bcm_version
    };

    let created: Value = match spec.editor {
        EditorType::Neko => {
            NekoWorkManager::new_with_client(client.clone()).create_kn_work(CreateKnWorkArgs {
                name: &name,
                work_url: &url,
                preview_url: spec.preview.unwrap_or(""),
                bcm_version,
                save_type: Some(2),
                stage_type: Some(2),
                n_blocks: spec.n_blocks,
                n_roles: None,
                n_scenes: None,
                pic_need_check_file_url: None,
            })?
        }
        EditorType::Kitten4 => KittenWorkManager::new_with_client(client.clone())
            .create_kitten_work(CreateKittenWorkArgs {
                name: &name,
                work_url: &url,
                preview: spec.preview.unwrap_or(""),
                version: KITTEN4_APP_VERSION,
                orientation: None,
                sample_id: None,
                work_source_label: Some(1),
                save_type: Some(2),
            })?,
        EditorType::Nemo => NemoWorkManager::new_with_client(client.clone()).create_nemo_work(
            CreateNemoWorkArgs {
                name: &name,
                work_url: &url,
                preview_url: spec.preview.unwrap_or(""),
                bcm_version,
                orientation: None,
                n_blocks: spec.n_blocks,
                n_roles: None,
                cloud_variables: None,
                root_ids: None,
                template_type: None,
            },
        )?,
        other => {
            return Err(ConvertError::Mew(MewError::InvalidArgument(format!(
                "{other:?} 没有已知的建作品端点,不能上传到账号(支持 Kitten4 / KittenN / NEMO)"
            ))));
        }
    };

    // 响应形态:KN `{"id":…}`、NEMO `{"id":…}`、Kitten 可能是 `{"data":{"id":…}}`
    created
        .get("id")
        .and_then(value_to_i64)
        .or_else(|| {
            created
                .get("data")
                .and_then(|d| d.get("id"))
                .and_then(value_to_i64)
        })
        .ok_or_else(|| {
            ConvertError::Mew(MewError::InvalidArgument(format!(
                "新建作品成功但响应里没有 id:{created}"
            )))
        })
}

#[cfg(test)]
mod upload_tests {
    use super::*;

    /// 只有 KN / Kitten4 / NEMO 有已知建作品端点;Coco / Wood / Kitten2/3 必须显式不支持
    #[test]
    fn account_upload_supports_documented_editors_only() {
        for editor in [EditorType::Neko, EditorType::Kitten4, EditorType::Nemo] {
            assert!(supports_account_upload(editor), "{editor:?}");
        }
        for editor in [
            EditorType::Coco,
            EditorType::Wood,
            EditorType::Kitten2,
            EditorType::Kitten3,
        ] {
            assert!(!supports_account_upload(editor), "{editor:?}");
        }
    }

    /// 草稿名要能看出行为、编辑器与来源,并标「可删」(会展示在平台草稿列表里)
    #[test]
    fn draft_name_marks_kind_editor_source_and_deletable() {
        let name = draft_name("反编译", EditorType::Neko, WorkId::new(330773110));
        assert!(name.starts_with("反编译副本"), "{name}");
        assert!(name.contains("KittenN"), "{name}");
        assert!(name.contains("330773110"), "{name}");
        assert!(name.contains("可删"), "{name}");
    }

    /// NEMO 上传必须走 `nemo_android_ios` 渠道,其余走社区前端
    /// 单包上限门:超一点就提前报错,正好等于上限要放行
    /// (上限值来自 2026-09-26 的同渠道实测,见 `SINGLE_PACKAGE_LIMIT`)
    #[test]
    fn oversized_artifact_is_rejected_before_upload() {
        let dir = std::env::temp_dir().join("backend-single-package-limit");
        std::fs::create_dir_all(&dir).expect("建测试临时目录");
        // 稀疏文件:只设长度,不真写 20 MB 字节
        let make = |name: &str, size: u64| {
            let path = dir.join(name);
            let file = std::fs::File::create(&path).expect("建稀疏测试文件");
            file.set_len(size).expect("设稀疏文件长度");
            path
        };

        let exact = make("exact.bin", SINGLE_PACKAGE_LIMIT);
        ensure_single_package_fits(&exact).expect("正好等于上限应放行");

        let over = make("over.bin", SINGLE_PACKAGE_LIMIT + 1);
        let error = ensure_single_package_fits(&over).expect_err("超上限应提前报错");
        assert!(
            error.to_string().contains("单包上传上限"),
            "错误信息要指出上限:{error}"
        );

        let _ = std::fs::remove_file(&exact);
        let _ = std::fs::remove_file(&over);
    }

    #[test]
    fn nemo_uses_dedicated_upload_channel() {
        assert_eq!(channel_for(EditorType::Nemo), UploadChannel::Nemo);
        assert_eq!(channel_for(EditorType::Neko), UploadChannel::Codemao);
        assert_eq!(channel_for(EditorType::Kitten4), UploadChannel::Codemao);
    }
}
