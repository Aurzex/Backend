//! 加载门(离线):本库产出的 `.bcmkn` 交官方 `BcmHelpers.validateBcm` 判定。
//!
//! 夹具 `tests/fixtures/convert/load_gate/minimal.bcm4` 是自制的合成作品(由合成 KN 反向生成,
//! 不含第三方内容)。官方编辑器 bundle 不入库:由 `.github/workflows/load-gate.yml`(手动触发)
//! 按固定 SHA256 下载到 `temp/web/`,再以 `BCM_WEB_DIR=temp/web` 跑本测试。
//!
//! 默认 `cargo test` 跳过(缺 bundle 或 node 时打印 `[跳过]`);工作流里设
//! `BACKEND_REQUIRE_LOAD_GATE=1`,缺依赖即失败,不允许"静默绿"。

use backend::core::convert::translate::{TargetEditor, TranslateOptions, translate_value};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn web_dir() -> PathBuf {
    std::env::var_os("BCM_WEB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("tests/web"))
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// 跑官方校验器,返回(进程成功与否, 完整 stdout)。判定行是 `validateBcm -> true/false`;
/// harness 在判 `false` 时以非零退出,所以调用方不能把"非零"当成运行失败。
fn validate(doc: &Value, tag: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("backend-load-gate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let file = dir.join(format!("{tag}.bcmkn"));
    std::fs::write(&file, serde_json::to_vec(doc).expect("序列化")).expect("写产物");

    let output = Command::new("node")
        .arg(repo_root().join("tests/load_gate/harness.js"))
        .arg("--validate")
        .arg(&file)
        .env("BCM_WEB_DIR", web_dir())
        .output()
        .expect("运行 node harness");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stdout.contains("validateBcm ->"),
        "node 没跑出判定(file={}):STDOUT={stdout} STDERR={stderr}",
        file.display()
    );
    let _ = std::fs::remove_dir_all(&dir);
    (output.status.success(), stdout)
}

#[test]
#[ignore = "需官方 bundle(BCM_WEB_DIR);手动工作流 .github/workflows/load-gate.yml 跑"]
fn product_passes_official_loader() {
    let harness = repo_root().join("tests/load_gate/harness.js");
    let ready = node_available() && harness.exists() && web_dir().join("main.js").exists();
    if !ready {
        if std::env::var_os("BACKEND_REQUIRE_LOAD_GATE").is_some() {
            panic!(
                "加载门依赖缺失(node / harness / bundle):web_dir={:?}",
                web_dir()
            );
        }
        eprintln!("[跳过] 加载门需要 node 与官方 bundle(BCM_WEB_DIR)");
        return;
    }

    let fixture = repo_root().join("tests/fixtures/convert/load_gate/minimal.bcm4");
    let source: Value = serde_json::from_str(&std::fs::read_to_string(&fixture).expect("读夹具"))
        .expect("夹具 JSON");
    let translated = translate_value(
        source,
        TargetEditor::KittenN,
        &TranslateOptions::new().deterministic_ids(true),
    )
    .expect("正向转换(Kitten4 -> KN)");
    let doc = &translated.document;

    // validateBcm 的四个结构必填(rounds/20 §11.1)
    for key in ["actors", "scenes", "styles", "stageSize"] {
        assert!(doc.get(key).is_some(), "产物缺顶层 `{key}`:{doc}");
    }

    let (ok, verdict) = validate(doc, "valid");
    assert!(
        ok && verdict.contains("validateBcm -> true"),
        "官方加载门判产物不合法:{verdict}"
    );

    // 负控:去掉 `actors.actorsDict`(必填结构)必须被判 false —— 证明门不是空转。
    let mut broken = doc.clone();
    broken
        .get_mut("actors")
        .and_then(Value::as_object_mut)
        .expect("actors")
        .remove("actorsDict");
    let (ok, verdict) = validate(&broken, "broken");
    assert!(
        !ok && verdict.contains("validateBcm -> false"),
        "负控应当被判不合法:{verdict}"
    );
}
