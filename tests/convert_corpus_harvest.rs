//! 语料采集器:从平台抓真作品 → 反编译落盘到 `download/compile/` →
//! 由 lib 单测 `kn_corpus_round_trip_sweep` 自动纳入往返扫描。
//!
//! 为什么单独一个测试:`download/compile/` 里原有的语料是手工挑的,覆盖面靠人;
//! 而**语料越杂,越容易撞出"某类块的某条分支不对称"** —— 上一轮的 `pure_list_get`
//! 影子缺失,就是只在「`delete_list_item` 且没有已连接子块」时发生。采集器用公开发现流
//! (`/creation-tools/v1/pc/discover/newest-work`,**匿名可读**,不需要账号)按"最新"取,
//! 天然带各种编辑器、各种写法。
//!
//! 发现流的条目只有 `work_id`(没有编辑器类型),所以逐个用作品详情
//! (`/creation-tools/v1/works/{id}`)查 `type` 再筛;筛中的才反编译(省掉 NEMO/WOOD 的慢路径)。
//!
//! 默认 `#[ignore]`:需要网络,并且会往 `download/compile/`(gitignored)写文件。
//!
//! ```text
//! cargo test --test convert_corpus_harvest -- --ignored --nocapture     # 默认 12 件,NEKO+KITTEN4
//! HARVEST_N=30 HARVEST_KINDS=NEKO cargo test --test convert_corpus_harvest -- --ignored --nocapture
//! HARVEST_OFFSET=40 cargo test --test convert_corpus_harvest -- --ignored --nocapture
//! ```
//!
//! 抓完跑扫描器看差异:
//! ```text
//! cargo test --lib kn_corpus_round_trip_sweep -- --nocapture
//! ```

use std::collections::BTreeSet;
use std::path::PathBuf;

use backend::api::work::WorkDataFetcher;
use backend::core::convert::decompile::{DecompileOptions, decompile_work_with};
use serde_json::Value;

/// 递归收集所有 `work_id` / `id`(数字或数字字符串)。
///
/// 不写死字段路径:发现流的返回结构随平台版本变(`items` / `data` / `list` 都可能),
/// 而"作品条目一定带 work_id"是稳定的。
fn collect_work_ids(node: &Value, out: &mut BTreeSet<i64>) {
    match node {
        Value::Object(map) => {
            for key in ["work_id", "id"] {
                if let Some(id) = map.get(key).and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
                }) {
                    out.insert(id);
                }
            }
            for value in map.values() {
                collect_work_ids(value, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_work_ids(item, out);
            }
        }
        _ => {}
    }
}

/// 从作品详情里取编辑器类型(顶层 `type`,或嵌在 `data` 下)
fn editor_type(details: &Value) -> Option<String> {
    fn find(node: &Value) -> Option<String> {
        match node {
            Value::Object(map) => {
                if let Some(t) = map.get("type").and_then(Value::as_str) {
                    return Some(t.to_ascii_uppercase());
                }
                for value in map.values() {
                    if let Some(t) = find(value) {
                        return Some(t);
                    }
                }
                None
            }
            Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(details)
}

#[test]
#[ignore = "需要网络:抓公开作品并反编译落盘"]
fn harvest_corpus() {
    let wanted: BTreeSet<String> = std::env::var("HARVEST_KINDS")
        .unwrap_or_else(|_| "NEKO,KITTEN4".to_string())
        .split(',')
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .collect();
    let want_n: usize = std::env::var("HARVEST_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let offset: i32 = std::env::var("HARVEST_OFFSET")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("download/compile");
    std::fs::create_dir_all(&out_dir).expect("建 download/compile");
    let fetcher = WorkDataFetcher::new();

    // 1. 公开发现流:最新作品(匿名可读,一批 40 条)
    let raw = fetcher
        .fetch_new_works_web(Some(40), Some(offset), false)
        .expect("拉取最新作品流");
    let mut ids = BTreeSet::new();
    collect_work_ids(&raw, &mut ids);
    println!("[采集] 发现流 offset={offset} 拿到 {} 个作品 id", ids.len());
    if ids.is_empty() {
        let head: String = raw.to_string().chars().take(600).collect();
        println!("[采集] 一个 id 都没解析出来,响应前 600 字符 = {head}");
    }

    // 2. 逐个查详情定类型,只留下目标类型
    let mut targets = Vec::new();
    for id in &ids {
        match fetcher.fetch_work_details(*id as i32) {
            Ok(details) => {
                let kind = editor_type(&details).unwrap_or_default();
                if wanted.contains(&kind) {
                    targets.push((*id, kind));
                }
            }
            Err(e) => println!("[采集] {id} 详情失败: {e}"),
        }
        if targets.len() >= want_n {
            break;
        }
    }
    println!("[采集] 命中目标类型 {wanted:?} 的 {}/{} 件", targets.len(), want_n);

    // 3. 逐件反编译落盘(关掉资源下载:语料只需要文档本身,1390 次请求 → 0 次)
    //
    // 另外把 Kitten4 作品的**编辑格式**源码单独存一份到 `download/compile/k4raw/`:
    // 反编译产物(`.bcm4`)是**上传格式**(`block_data_json` 是 map),而正向转换
    // (`convert_kitten4_document`)吃的是**编辑格式**(`block_data_json` 是字符串)——
    // 两者不是一回事,拿错了会得到 `invalid type: map, expected a string`。
    let k4raw_dir = out_dir.join("k4raw");
    let _ = std::fs::create_dir_all(&k4raw_dir);
    let mut ok = 0usize;
    for (id, kind) in &targets {
        let options = DecompileOptions::new()
            .output_dir(out_dir.clone())
            .save_raw(false)
            .skip_resources(true);
        match decompile_work_with((*id).into(), options) {
            Ok(path) => {
                ok += 1;
                println!("[采集] {id} ({kind}) → {}", path.display());
                if kind == "KITTEN4" {
                    match fetcher.fetch_work_source_code(*id as i32) {
                        Ok(source) => {
                            let keys: Vec<String> = source
                                .as_object()
                                .map(|m| m.keys().take(8).cloned().collect())
                                .unwrap_or_default();
                            let dest = k4raw_dir.join(format!("k4-{id}.json"));
                            let _ = std::fs::write(&dest, source.to_string());
                            println!("[采集]   ↳ 编辑格式源码 → {} (顶层键 {keys:?})", dest.display());
                        }
                        Err(e) => println!("[采集]   ↳ 源码拉取失败: {e}"),
                    }
                }
            }
            Err(e) => println!("[采集] {id} ({kind}) 失败: {e}"),
        }
    }
    println!(
        "[采集] 完成 {ok}/{} 件,产物在 {};接着跑:\n  cargo test --lib kn_corpus_round_trip_sweep -- --nocapture",
        targets.len(),
        out_dir.display()
    );
    assert!(ok > 0, "一件都没抓到:检查网络或接口是否变更");
}
