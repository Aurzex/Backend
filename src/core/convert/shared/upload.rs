//! 把产物上传到当前账号,并在平台建一份同名草稿(反编译 / 转化两个子域共用)
//!
//! 只做三件事:**上传文件 → 建作品 → 取回 id**。不做资源重传、不做 URL 改写
//! (NEMO 的造型/音频仍指向源 CDN;见 `docs/rounds/30` §NEMO)。
//!
//! 草稿名一律带来源作品 id 与「可删」标记 —— 它会直接出现在用户的平台草稿列表里。

use std::path::Path;

use serde_json::Value;

use super::error::{DecompilerError, Result};
use super::model::{EditorType, WorkId};
use crate::api::work::{
    CreateKittenWorkArgs, CreateKnWorkArgs, CreateNemoWorkArgs, KittenWorkManager, NekoWorkManager,
    NemoWorkManager,
};
use crate::utils::filedata::value_to_i64;
use crate::utils::requests::{CodeMaoClient, MewError, UploadChannel};

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
    let url = client
        .file_uploader()
        .upload(spec.artifact, channel_for(spec.editor), spec.save_path)?;
    let name = draft_name(spec.kind, spec.editor, spec.source_work_id);
    let bcm_version = if spec.bcm_version.is_empty() {
        crate::core::convert::translate::tables_gen::BCM_VERSION
    } else {
        spec.bcm_version
    };

    let created: Value = match spec.editor {
        EditorType::Neko => NekoWorkManager::new_with_client(client.clone())
            .create_kn_work(CreateKnWorkArgs {
                name: &name,
                work_url: &url,
                preview_url: "",
                bcm_version,
                save_type: Some(2),
                stage_type: Some(2),
                n_blocks: spec.n_blocks,
                n_roles: None,
                n_scenes: None,
                pic_need_check_file_url: None,
            })?,
        EditorType::Kitten4 => KittenWorkManager::new_with_client(client.clone())
            .create_kitten_work(CreateKittenWorkArgs {
                name: &name,
                work_url: &url,
                preview: "",
                version: KITTEN4_APP_VERSION,
                orientation: None,
                sample_id: None,
                work_source_label: Some(1),
                save_type: Some(2),
            })?,
        EditorType::Nemo => NemoWorkManager::new_with_client(client.clone())
            .create_nemo_work(CreateNemoWorkArgs {
                name: &name,
                work_url: &url,
                preview_url: "",
                bcm_version,
                orientation: None,
                n_blocks: spec.n_blocks,
                n_roles: None,
                cloud_variables: None,
                root_ids: None,
                template_type: None,
            })?,
        other => {
            return Err(DecompilerError::Mew(MewError::InvalidArgument(format!(
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
            DecompilerError::Mew(MewError::InvalidArgument(format!(
                "新建作品成功但响应里没有 id:{created}"
            )))
        })
}

#[cfg(test)]
mod tests {
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
    #[test]
    fn nemo_uses_dedicated_upload_channel() {
        assert_eq!(channel_for(EditorType::Nemo), UploadChannel::Nemo);
        assert_eq!(channel_for(EditorType::Neko), UploadChannel::Codemao);
        assert_eq!(channel_for(EditorType::Kitten4), UploadChannel::Codemao);
    }
}
