//! 抓**平台原始编辑格式**语料(rounds/33 §2 卡住的那一块,rounds/37 §11.3 #9 落地)。
//!
//! 背景:平台只对外给"编译态"(`player/load`),编辑格式只存在于**编辑器读写的那份文件**里。
//! 编辑器的 bundle 里写着它的读端点(见 `docs/rounds/37` §12):
//!
//! ```text
//! GET {creation_api}/kitten/work/ide/load/{work_id}   →  { name, ide_type, source_urls: [.bcm4…], … }
//! ```
//!
//! `source_urls` 就是该作品的**历史版本**(编辑器自己写出去的文件),也就是我们要的正向语料。
//!
//! 用法:
//! ```text
//! cargo test --test convert_edit_harvest -- --ignored --nocapture          # 默认:从 download/compile/*.bcm4 的文件名里取 id
//! WORK_IDS=215246857,174408420 cargo test --test convert_edit_harvest -- --ignored --nocapture
//! ```
//!
//! 产物落在 `download/compile/k4edit/<id>-<版本序号>.bcm4`(gitignored ⇒ 不进版本库;
//! 正向扫描器 `k4_corpus_round_trip_sweep` 会自动吃这个目录)。
//!
//! 注意:该端点按**权限**给源文件 —— 只有我们有权读的作品才有 `source_urls`(拿不到时打印提醒并跳过)。

use std::path::PathBuf;

use backend::prelude::*;
use backend::utils::requests::BaseKey;

#[test]
#[ignore = "需要网络 + 账号:抓平台原始编辑格式(`/kitten/work/ide/load`)到 download/compile/k4edit/"]
fn harvest_edit_format() {
    let cfg_path = std::env::var("BACKEND_TEST_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/test-config.json")
        });
    let cfg: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg_path).expect("读测试配置"))
            .expect("配置 JSON");
    let account = cfg["accounts"][0]["account"]
        .as_str()
        .expect("account")
        .to_string();
    let password = cfg["accounts"][0]["password"]
        .as_str()
        .expect("password")
        .to_string();
    let login = backend::api::auth::LoginBuilder::new()
        .identity(&account)
        .password(&password)
        .execute()
        .expect("登录调用失败");
    assert!(login.success, "登录未成功:{:?}", login.message);

    let ids: Vec<String> = match std::env::var("WORK_IDS") {
        Ok(list) => list
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => work_ids_from_corpus(),
    };
    if ids.is_empty() {
        eprintln!(
            "跳过:没有可用的作品 id(先跑 `cargo test --test convert_corpus_harvest -- --ignored`)"
        );
        return;
    }

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("download/compile/k4edit");
    std::fs::create_dir_all(&out_dir).expect("建 download/compile/k4edit");
    let client = CodeMaoClient::global().clone();

    let mut total = 0usize;
    let mut denied = Vec::new();
    for id in &ids {
        // 编辑器 bundle 里的读端点(creation_api 是运行时注入的,本库的 BaseKey::Creation 就是它)
        let meta = match client
            .build_request(
                HttpMethod::Get,
                &format!("/kitten/work/ide/load/{id}"),
                Some(BaseKey::Creation),
            )
            .send()
            .and_then(|response| client.response_to_json(response))
        {
            Ok(value) => value,
            Err(error) => {
                denied.push(format!("{id}({error})"));
                continue;
            }
        };
        let urls: Vec<&str> = meta
            .get("source_urls")
            .and_then(|v| v.as_array())
            .map(|list| list.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        let name = meta
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("(无名)");
        if urls.is_empty() {
            denied.push(format!("{id}({name}:无 source_urls)"));
            continue;
        }
        let mut saved = 0usize;
        for (index, url) in urls.iter().enumerate() {
            let dst = out_dir.join(format!("{id}-{index}.bcm4"));
            let Ok(bytes) = fetch_bytes(&client, url) else {
                continue;
            };
            if std::fs::write(&dst, &bytes).is_ok() {
                saved += 1;
            }
        }
        total += saved;
        println!(
            "[编辑格式] {id}({name}):{saved}/{} 个版本 -> {}",
            urls.len(),
            out_dir.display()
        );
    }
    println!(
        "[编辑格式] 合计落盘 {total} 份;k4edit 目录现有 {} 份",
        count_bcm4(&out_dir)
    );
    if !denied.is_empty() {
        println!(
            "[编辑格式] 跳过 {} 件(无权限或无源文件):{denied:#?}",
            denied.len()
        );
    }
}

/// 从既有语料(`download/compile/*.bcm4` 的文件名 `_<id>.bcm4`)里取作品 id
fn work_ids_from_corpus() -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("download/compile");
    let mut ids: Vec<String> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return ids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".bcm4") {
            continue;
        }
        if let Some(rest) = name.strip_suffix(".bcm4")
            && let Some((_, id)) = rest.rsplit_once('_')
            && id.len() >= 6
            && id.chars().all(|c| c.is_ascii_digit())
        {
            ids.push(id.to_string());
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

fn count_bcm4(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("bcm4"))
                .count()
        })
        .unwrap_or(0)
}

/// 下载一个 CDN 文件(编辑器写出去的作品文件;无需鉴权)
///
/// 走本库自己的请求器:`build_url` 对**绝对 URL** 直接放行(`requests.rs:662`),
/// 于是同一套超时/日志/身份槽都能复用。
fn fetch_bytes(client: &CodeMaoClient, url: &str) -> MewResult<Vec<u8>> {
    let response = client.build_request(HttpMethod::Get, url, None).send()?;
    client.response_to_binary(response)
}
