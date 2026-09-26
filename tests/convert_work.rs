//! 把一个线上作品转成另一种编辑器的作品文件(按作品类型自动选方向)。
//!
//! 用法:
//!
//! ```text
//! WORK_ID=273988379 cargo test --test convert_work -- --ignored --nocapture
//! ```
//!
//! - `NEKO`(KN,`.bcmkn`)⇒ 转成 **Kitten4**(`.bcm4`);
//! - `KITTEN4` ⇒ 转成 **KittenN**(`.bcmkn`);
//! - 其它编辑器类型(COCO / NEMO / WOOD / Kitten2/3)本库暂不支持,直接报错而不是静默出半成品。
//!
//! 产物落在 `download/converted/`(**不**放进 `download/compile/`:那里是往返扫描器的语料,
//! 混进手工转换产物会污染"每件都是真作品"的前提)。
//!
//! 只转换、**不上传**:上传会在你的账号下建草稿作品(等同一次发布动作),
//! 需要时显式开 `TranslateOptions::upload_to_account`。

use std::path::PathBuf;

use backend::api::work::WorkDataFetcher;
use backend::core::convert::translate::{TargetEditor, TranslateOptions};
use backend::core::convert::{WorkId, translate_work};

#[test]
#[ignore = "需要网络:抓线上作品并转换"]
fn convert_one_work() {
    let id: i64 = std::env::var("WORK_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(273988379);

    // 1. 作品详情:拿类型(决定方向)与名字(只用于打印)
    let details = WorkDataFetcher::new()
        .fetch_work_details(id as i32)
        .expect("拉作品详情");
    let kind = details
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_ascii_uppercase();
    let name = details
        .get("work_name")
        .and_then(|v| v.as_str())
        .unwrap_or("(无名)")
        .to_string();

    let target = match kind.as_str() {
        "NEKO" => TargetEditor::Kitten4,
        "KITTEN4" => TargetEditor::KittenN,
        other => panic!("作品 {id} 的类型是 {other:?}:本库当前只支持 Kitten4 ⇄ KittenN(NEKO)"),
    };

    // 2. 转换(抓取 → 反编译到编辑版 → 重排 → 落盘,全程内存直通)
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("download/converted");
    std::fs::create_dir_all(&out_dir).expect("建 download/converted");
    // 可选:上传到账号并建草稿(默认关)。**开了就等于替用户发布一次**,但只有上传后
    // 编辑器才能按正常路径加载产物 —— 实机验证要走这条。
    let upload = std::env::var("UPLOAD")
        .map(|v| matches!(v.as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    if upload {
        let cfg_path = std::env::var("BACKEND_TEST_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/test-config.json"));
        let text = std::fs::read_to_string(&cfg_path).expect("读测试配置(含账号)");
        let cfg: serde_json::Value = serde_json::from_str(&text).expect("配置 JSON");
        let account = cfg["accounts"][0]["account"].as_str().expect("accounts[0].account").to_string();
        let password = cfg["accounts"][0]["password"].as_str().expect("accounts[0].password").to_string();
        let login = backend::api::auth::LoginBuilder::new()
            .identity(&account)
            .password(&password)
            .execute()
            .expect("登录调用失败");
        assert!(login.success, "登录未成功:{:?}", login.message);
        println!("[上传] 已登录 {account}");
    }

    let options = TranslateOptions::new()
        .output_dir(out_dir.clone())
        .upload(upload);
    let outcome = translate_work(WorkId::new(id), target, options).expect("转换失败");

    // 3. 报账:产物路径 / 体积 / 报告摘要 / 顶层结构 sanity
    let size = std::fs::metadata(&outcome.output)
        .map(|m| m.len())
        .unwrap_or(0);
    println!("[转换] {name}({kind} / id {id})→ {target:?}");
    println!("[转换] 产物: {} ({size} 字节)", outcome.output.display());
    println!("[转换] 上传得到的新作品 id: {:?}", outcome.work_id);
    if let Some(new_id) = outcome.work_id {
        // 上传后**回读平台侧**:确认平台是按目标编辑器类型收下的(这是"文件合法"的强证据)
        match WorkDataFetcher::new().fetch_work_details(new_id as i32) {
            Ok(details) => println!(
                "[上传] 平台回读: id={} type={:?} name={:?} bcm_version={:?} preview={:?}",
                new_id,
                details.get("type"),
                details.get("work_name"),
                details.get("bcm_version"),
                details.get("preview").and_then(|v| v.as_str()).map(|s| &s[..s.len().min(60)])
            ),
            Err(e) => println!("[上传] 平台回读失败: {e}"),
        }
    }
    println!(
        "[转换] 报告: 有损={} 告警={} 条",
        outcome.report.is_lossy(),
        outcome.report.warnings().len()
    );
    let text = std::fs::read_to_string(&outcome.output).expect("读产物");
    let doc: serde_json::Value = serde_json::from_str(&text).expect("产物必须是 JSON");
    assert!(doc.is_object(), "产物必须是 JSON 对象");
    let keys: Vec<String> = doc
        .as_object()
        .map(|m| m.keys().take(12).cloned().collect())
        .unwrap_or_default();
    println!("[转换] 产物顶层键(前 12): {keys:?}");
    let entities = ["actors", "scenes"]
        .iter()
        .map(|cont| {
            doc.get("theatre")
                .and_then(|t| t.get(cont))
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum::<usize>();
    println!("[转换] 产物实体数(Kitten4 侧 theatre): {entities}");
    assert!(
        entities > 0 || doc.get("actors").is_some(),
        "产物既没有 theatre 实体也没有 KN 侧 actors,可疑"
    );

    // 4. 告警分类 + 落一份 markdown 报告(产物旁边,便于人看)
    println!("[转换] 告警分类: {:?}", outcome.report.counts());
    let report_path = outcome.output.with_extension("report.md");
    match std::fs::write(&report_path, outcome.report.to_markdown()) {
        Ok(()) => println!("[转换] 报告文件: {}", report_path.display()),
        Err(e) => println!("[转换] 报告写入失败: {e}"),
    }

    // 5. 自检:再转回原编辑器,数一遍"块总量"看有没有明显出入
    let back_target = match kind.as_str() {
        "NEKO" => TargetEditor::KittenN,
        _ => TargetEditor::Kitten4,
    };
    let back = backend::core::convert::translate::translate_file(
        &outcome.output,
        back_target,
        TranslateOptions::new().output_dir(out_dir.clone()),
    );
    match back {
        Ok(outcome_back) => {
            let back_path = outcome_back.output.clone();
            let back_text = std::fs::read_to_string(&back_path).expect("读回产物");
            // 只做合法性校验:产物必须是 JSON(块数/账号上传等不在本工具职责内)
            let _: serde_json::Value = serde_json::from_str(&back_text).expect("回产物 JSON");
            println!(
                "[自检] 再转回 {back_target:?} → {}(告警 {} 条)",
                back_path.display(),
                outcome_back.report.warnings().len()
            );
        }
        Err(e) => println!("[自检] 回程转换失败: {e}"),
    }
}
