//! 真机集成测试:作品 → 另一种编辑器作品文件的转化(可选上传并建草稿)
//!
//! 与 `live_features.rs` 同约定:
//! - 配置(作品 ID / 账号)全部来自 `data/test-config.json`(gitignored),缺失即跳过;
//! - 每个测试自行登录拿 token,不依赖全局身份槽,可并行;
//! - 输出写系统临时目录,不污染仓库。
//!
//! 两类测试:
//! - **离线转化**(默认跑):反编译真作品 → 转化 → 落盘 + 报告断言,不写平台;
//! - **上传建草稿**(`#[ignore]`,需显式 `--ignored`):会把产物上传并在账号下建一份
//!   **草稿**作品(未发布),名称带「转化自检…(可删)」便于清理。这是平台写操作,
//!   默认不跑,避免 CI / 例行跑测试时留垃圾。
//!
//! ```bash
//! cargo test --test convert_live                         # 离线转化
//! cargo test --test convert_live -- --ignored            # 含上传建草稿(会写平台)
//! ```

use std::path::PathBuf;

use backend::api::auth::LoginBuilder;
use backend::core::convert::translate::{
    TargetEditor, TranslateOptions, TranslateReport, translate_file,
};
// 跨子域编排(反编译取编辑版 + 转化 + 可选上传)在域门面
use backend::core::convert::WorkId;
use backend::core::convert::decompile::{CodemaoDecompiler, DecompileOptions};
use backend::core::convert::translate_work;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct WorkEntry {
    id: i64,
    /// KITTEN4 / NEKO / ...
    #[serde(default)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct AccountEntry {
    account: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct TestConfig {
    #[serde(default)]
    works: Vec<WorkEntry>,
    #[serde(default)]
    accounts: Vec<AccountEntry>,
}

/// 严格模式:设 `BACKEND_REQUIRE_LIVE=1` 时,"配置缺失 / 作品缺失 / 登录失败"一律失败,
/// 而不是静默 pass(默认不设 = 仓库既有约定:缺配置即跳过)
fn require_live() -> bool {
    matches!(
        std::env::var("BACKEND_REQUIRE_LIVE").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

/// 统一的"跳过或失败"出口:返回 `true` 表示调用方应 `return`
fn skip_or_fail(what: &str) -> bool {
    if require_live() {
        panic!("[live] {what};BACKEND_REQUIRE_LIVE=1 时不允许静默跳过");
    }
    eprintln!("[live] 跳过:{what}");
    true
}

fn load_config() -> Option<TestConfig> {
    let path = std::env::var("BACKEND_TEST_CONFIG")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/test-config.json"));
    if !path.exists() {
        let _ = skip_or_fail(&format!("未找到测试配置 {path:?}"));
        return None;
    }
    let text = std::fs::read_to_string(&path).expect("读配置");
    match serde_json::from_str(&text) {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            let _ = skip_or_fail(&format!("解析测试配置失败: {e}"));
            None
        }
    }
}

fn login(cfg: &TestConfig) -> Option<String> {
    let Some(entry) = cfg.accounts.first() else {
        let _ = skip_or_fail("配置里没有 accounts");
        return None;
    };
    match LoginBuilder::new()
        .identity(&entry.account)
        .password(&entry.password)
        .execute()
    {
        Ok(r) if r.success => Some(r.token),
        Ok(r) => {
            if require_live() {
                panic!(
                    "[live] 登录失败:{};BACKEND_REQUIRE_LIVE=1 时视为失败",
                    r.message
                );
            }
            eprintln!("[convert_live] 登录失败:{}", r.message);
            None
        }
        Err(e) => {
            if require_live() {
                panic!("[live] 登录请求失败:{e}");
            }
            eprintln!("[convert_live] 登录请求失败:{e}");
            None
        }
    }
}

/// 工作目录:系统临时目录下的独立子目录(不污染仓库)
fn work_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("backend-convert-live-{tag}"));
    std::fs::create_dir_all(&dir).expect("建目录");
    dir
}

fn first_work(cfg: &TestConfig, kind: &str) -> Option<i64> {
    let found = cfg
        .works
        .iter()
        .find(|w| w.kind.eq_ignore_ascii_case(kind))
        .map(|w| w.id);
    if found.is_none() {
        let _ = skip_or_fail(&format!("配置里没有 kind={kind} 的作品"));
    }
    found
}

/// 反编译 → 转成 KN 文件(离线,不写平台)
#[test]
fn kitten4_work_to_kn_file() {
    let Some(cfg) = load_config() else { return };
    let Some(token) = login(&cfg) else { return };
    let Some(work_id) = first_work(&cfg, "KITTEN4") else {
        eprintln!("[convert_live] 配置里没有 KITTEN4 作品,跳过");
        return;
    };

    // 1. 反编译出编辑版(写临时目录)
    let dir = work_dir("fwd");
    let source = CodemaoDecompiler::global()
        .decompile_with_options(
            WorkId::new(work_id),
            DecompileOptions::new().output_dir(&dir),
        )
        .expect("反编译");
    assert!(source.exists(), "反编译产物不存在:{source:?}");

    // 2. 转化(离线)
    let out = translate_file(
        &source,
        TargetEditor::KittenN,
        TranslateOptions::new()
            .output_dir(&dir)
            .deterministic_ids(true),
    )
    .expect("转化");
    assert!(out.output.exists(), "转化产物不存在:{:?}", out.output);
    assert!(out.report.blocks_total > 0, "应统计到源积木");
    assert!(
        !has_unmapped(&out.report),
        "该作品出现未映射积木:{:?}",
        out.report.warnings()
    );

    // 3. 产物必须是合法 KN 文档(四个必填结构 + actorIds 自洽)
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out.output).expect("读产物")).expect("JSON");
    for key in ["actors", "scenes", "styles", "stageSize"] {
        assert!(doc.get(key).is_some(), "产物缺必填结构 {key}");
    }
    let actors = doc["actors"]["actorsDict"].as_object().expect("actorsDict");
    for scene in doc["scenes"]["scenesDict"]
        .as_object()
        .expect("scenesDict")
        .values()
    {
        for aid in scene["actorIds"].as_array().cloned().unwrap_or_default() {
            let aid = aid.as_str().unwrap_or_default();
            assert!(actors.contains_key(aid), "场景引用了不存在的角色 {aid}");
        }
    }
    let _ = token;
    eprintln!(
        "[convert_live] {work_id} → KN 完成:{:?}(积木 {} → {})",
        out.output, out.report.blocks_total, out.report.blocks_converted
    );
}

/// KN 作品 → Kitten4 文件(反向,离线)
#[test]
fn kn_work_to_kitten4_file() {
    let Some(cfg) = load_config() else { return };
    if login(&cfg).is_none() {
        return;
    }
    let Some(work_id) = first_work(&cfg, "NEKO") else {
        eprintln!("[convert_live] 配置里没有 NEKO 作品,跳过");
        return;
    };
    let dir = work_dir("rev");
    let source = CodemaoDecompiler::global()
        .decompile_with_options(
            WorkId::new(work_id),
            DecompileOptions::new().output_dir(&dir),
        )
        .expect("反编译(解密)");
    let out = translate_file(
        &source,
        TargetEditor::Kitten4,
        TranslateOptions::new()
            .output_dir(&dir)
            .deterministic_ids(true),
    )
    .expect("反向转化");
    assert!(out.output.exists());
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out.output).expect("读产物")).expect("JSON");
    // Kitten4 编辑版的关键结构
    assert!(doc.get("theatre").is_some(), "缺 theatre");
    assert!(doc.get("size").is_some(), "缺 size");
    let actors = doc["theatre"]["actors"].as_object().expect("actors");
    assert!(
        actors.values().any(|a| a.get("block_data_json").is_some()),
        "至少一个实体应带 block_data_json"
    );
    // 告警总数没有意义(大量是 id 重铸,不是损失)⇒ 按类型统计,让人一眼看到真实缺口
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for w in out.report.warnings() {
        *kinds.entry(w.category()).or_default() += 1;
    }
    eprintln!(
        "[convert_live] {work_id} → Kitten4 完成:{:?}(有损:{};告警 {} 条:{} )",
        out.output,
        out.report.is_lossy(),
        out.report.warnings().len(),
        kinds
            .iter()
            .map(|(k, n)| format!("{k}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    // 未映射积木按 KN 侧类型名聚合(判断"缺的是哪些能力"的唯一依据)
    let mut unmapped: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for w in out.report.warnings() {
        if let backend::core::convert::translate::TranslateWarning::UnmappedBlock { kind } = w {
            *unmapped.entry(kind.as_str()).or_default() += 1;
        }
    }
    // 丢弃属性按"归一化路径"聚合:实体键会按实体重复上千次,必须先把 id/uuid 抹掉
    fn norm_property(path: &str) -> String {
        if let Some(rest) = path.split("[*].").nth(1) {
            return format!("实体键 {}", rest.split('(').next().unwrap_or(rest));
        }
        if let Some(rest) = path.strip_prefix("KN 顶层键 `") {
            return format!("顶层键 {}", rest.split('`').next().unwrap_or(rest));
        }
        path.split('(').next().unwrap_or(path).to_string()
    }
    let mut dropped: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for w in out.report.warnings() {
        match w {
            backend::core::convert::translate::TranslateWarning::DroppedProperty { path } => {
                *dropped.entry(norm_property(path)).or_default() += 1
            }
            backend::core::convert::translate::TranslateWarning::DroppedField { path } => {
                *dropped.entry(format!("[字段] {path}")).or_default() += 1
            }
            backend::core::convert::translate::TranslateWarning::AmbiguousType { kind, .. } => {
                *dropped.entry(format!("[歧义] {kind}")).or_default() += 1
            }
            _ => {}
        }
    }
    let mut dtop: Vec<_> = dropped.iter().collect();
    dtop.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!(
        "[convert_live] 丢弃/歧义 {} 种 / {} 条,Top20:{}",
        dropped.len(),
        dropped.values().sum::<usize>(),
        dtop.iter()
            .take(20)
            .map(|(k, n)| format!("{k}={n}"))
            .collect::<Vec<_>>()
            .join(" | ")
    );
    let mut top: Vec<_> = unmapped.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!(
        "[convert_live] 未映射积木 {} 种 / {} 个,Top20:{}",
        unmapped.len(),
        unmapped.values().sum::<usize>(),
        top.iter()
            .take(20)
            .map(|(k, n)| format!("{k}={n}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
}

/// 端到端(写平台):作品 → 转化 → 上传 → 建草稿
///
/// 默认 `#[ignore]`:这是平台写操作,会在账号下留草稿(名字形如「转化副本 KittenN ← <作品 id>(可删)」)。
#[test]
#[ignore = "会调用平台写接口(上传 + 建草稿),需显式 --ignored 运行"]
fn translate_work_creates_draft_when_ignored() {
    let Some(cfg) = load_config() else { return };
    if login(&cfg).is_none() {
        return;
    }
    let Some(work_id) = first_work(&cfg, "KITTEN4") else {
        return;
    };
    let out = translate_work(
        WorkId::new(work_id),
        TargetEditor::KittenN,
        TranslateOptions::new().upload(true),
    )
    .expect("作品转化 + 建草稿");
    let created = out.work_id.expect("应返回新建的草稿作品 id");
    assert!(created > 0);

    // 回读:平台必须把它存成 KN 作品,且带上我们写的 bcm_version
    let detail = backend::api::work::WorkDataFetcher::new()
        .fetch_kn_work_details(created as i32)
        .expect("回读草稿详情");
    assert_eq!(
        detail.get("work_id").and_then(serde_json::Value::as_i64),
        Some(created),
        "回读的 work_id 不一致:{detail}"
    );
    assert!(
        detail
            .get("work_url")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|u| !u.is_empty()),
        "平台没给草稿分配 work_url:{detail}"
    );
    assert_eq!(
        detail
            .get("bcm_version")
            .and_then(serde_json::Value::as_str),
        Some("0.16.2"),
        "回读的 bcm_version 与目标格式常量不一致:{detail}"
    );
    eprintln!(
        "[convert_live] 已建草稿作品 id={created}(work_url={:?},可在账号下删除)",
        detail.get("work_url").and_then(serde_json::Value::as_str)
    );
}

fn has_unmapped(report: &TranslateReport) -> bool {
    report.warnings().iter().any(|w| {
        matches!(
            w,
            backend::core::convert::translate::TranslateWarning::UnmappedBlock { .. }
        )
    })
}

/// 端到端(写平台):反编译 → **上传到当前账号** → 建草稿 → 回读 → **删掉自己建的草稿**
///
/// 默认 `#[ignore]`(平台写操作)。与 `translate_work_creates_draft_when_ignored` 的区别:
/// 这里验证的是**反编译侧**的 [`DecompileOptions::upload_to_account`](原样备份/搬家),
/// 并且跑完**自行删除草稿**,不留垃圾。
#[test]
#[ignore = "会调用平台写接口(上传 + 建草稿),需显式 --ignored 运行"]
fn decompile_uploads_backup_draft_and_cleans_up_when_ignored() {
    let Some(cfg) = load_config() else { return };
    if login(&cfg).is_none() {
        return;
    }
    let Some(work_id) = first_work(&cfg, "NEKO") else {
        eprintln!("[convert_live] 配置里没有 NEKO 作品,跳过");
        return;
    };
    let dir = work_dir("backup");
    let outcome = CodemaoDecompiler::global()
        .decompile_outcome(
            WorkId::new(work_id),
            DecompileOptions::new()
                .output_dir(&dir)
                .upload_to_account(true),
        )
        .expect("反编译 + 上传到账号");
    assert!(
        outcome.artifact.exists(),
        "产物应落盘:{:?}",
        outcome.artifact
    );
    assert_eq!(outcome.editor, backend::core::convert::EditorType::Neko);
    let created = outcome.work_id.expect("应返回新建的草稿作品 id");
    assert!(created > 0, "草稿 id 应为正数:{created}");

    // 回读:平台把它存成 KN 作品,并分配了 work_url
    let detail = backend::api::work::WorkDataFetcher::new()
        .fetch_kn_work_details(created as i32)
        .expect("回读草稿详情");
    assert!(
        detail
            .get("work_url")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|u| !u.is_empty()),
        "平台没给草稿分配 work_url:{detail}"
    );

    // 自清理:本用例建的草稿必须删掉(否则会在账号里留垃圾)
    let deleted = backend::api::work::NekoWorkManager::new()
        .delete_kn_draft(created as i32, 2)
        .expect("删除自建草稿");
    assert!(deleted, "自建草稿应删除成功");
    eprintln!(
        "[convert_live] 反编译 {work_id} → 建草稿 {created} → 已自行删除(产物 {:?})",
        outcome.artifact
    );
}
