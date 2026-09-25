//! 一次性探测:KN 作品注册表是否也承载 NEMO 作品(只读)。用完即删。

use std::time::{SystemTime, UNIX_EPOCH};

use backend::api::auth::LoginBuilder;
use backend::prelude::*;
use backend::utils::requests::{BaseKey, ClientConfig};
use serde_json::Value;

fn ts13() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .to_string()
}

fn get(client: &CodeMaoClient, path: &str, base: BaseKey, params: &[(&str, String)]) -> Option<Value> {
    let mut b = client.build_request(HttpMethod::Get, path, Some(base));
    for (k, v) in params {
        b = b.with_param(*k, v.clone());
    }
    match b.send() {
        Ok(resp) => match client.response_to_json(resp) {
            Ok(v) => Some(v),
            Err(e) => {
                println!("  解析失败 {path}: {e}");
                None
            }
        },
        Err(e) => {
            println!("  请求失败 {path}: {e}");
            None
        }
    }
}

#[test]
fn tmp_probe_kn_registry() {
    let cfg: Value = serde_json::from_str(
        &std::fs::read_to_string("data/test-config.json").expect("无配置"),
    )
    .unwrap();
    let acc = &cfg["accounts"][0];
    let login = LoginBuilder::new()
        .identity(acc["account"].as_str().unwrap())
        .password(acc["password"].as_str().unwrap())
        .execute()
        .expect("登录请求失败");
    println!("登录: {}", login.success);

    let client = CodeMaoClient::new_with_global_auth(ClientConfig::new());
    client.set_token(Identity::Fluffy, login.token).unwrap();

    for path in ["/neko/works/v2/list/user", "/neko/works/list/user/published"] {
        println!("\n== {path}");
        let t = ts13();
        if let Some(v) = get(
            &client,
            path,
            BaseKey::Creation,
            &[("offset", "0".into()), ("limit", "25".into()), ("TIME", t)],
        ) {
            let items = v
                .get("items")
                .or_else(|| v.get("data").and_then(|d| d.get("items")))
                .and_then(|i| i.as_array())
                .cloned()
                .unwrap_or_default();
            println!("  条目数 {}", items.len());
            for it in items.iter().take(12) {
                println!(
                    "  id={} name={} <bcm_version={}> type={} url={}",
                    it.get("id").unwrap_or(&Value::Null),
                    it.get("name").unwrap_or(&Value::Null),
                    it.get("bcm_version").unwrap_or(&Value::Null),
                    it.get("type").or(it.get("work_type")).unwrap_or(&Value::Null),
                    it.get("work_url")
                        .or(it.get("bcm_url"))
                        .and_then(|u| u.as_str())
                        .unwrap_or("-"),
                );
            }
            if items.is_empty() {
                println!("  原始响应: {}", &v.to_string()[..v.to_string().len().min(600)]);
            }
        }
    }

    println!("\n== 统一作品列表全量");
    let v = get(
        &client,
        "/creation-tools/v1/works/list/user",
        BaseKey::Default,
        &[("offset", "0".into()), ("limit", "100".into())],
    )
    .unwrap();
    let arr = v.as_array().cloned().unwrap_or_default();
    println!("  条数 {}", arr.len());
    for it in arr.iter() {
        let u = it.get("work_url").and_then(|x| x.as_str()).unwrap_or("-");
        println!(
            "  work_id={} type={} name={:?} bcm={:?} url_tail=...{}  preview_tail=...{}",
            it.get("work_id").unwrap_or(&Value::Null),
            it.get("work_type").unwrap_or(&Value::Null),
            it.get("name").unwrap_or(&Value::Null),
            it.get("bcm_version").unwrap_or(&Value::Null),
            u.chars().rev().take(24).collect::<String>().chars().rev().collect::<String>(),
            it.get("preview")
                .and_then(|x| x.as_str())
                .unwrap_or("-")
                .chars()
                .rev()
                .take(28)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>(),
        );
    }
}
