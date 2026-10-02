//! 开发者工具(不进 release 产物的公开面,但随 crate 源码提交):
//! 从 `temp/tables/*.json`(由 KittenN 编辑器 bundle 逆向导出)生成
//! `src/core/convert/translate/tables_gen.rs`。
//!
//! 用法:
//!
//! ```bash
//! cargo run --bin gen_translate_tables -- temp/tables src/core/convert/translate/tables_gen.rs
//! ```
//!
//! 输入 JSON 的顶层形如 `{ "$provenance": {...}, "data": {...} }`;只读 `data`。
//! 升级编辑器 bundle 后重跑本生成器即可(表内容变化会被 git diff 看见)。

use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

fn main() {
    match run() {
        Ok(summary) => println!("{summary}"),
        Err(message) => {
            eprintln!("生成失败:{message}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<String, String> {
    let mut args = std::env::args().skip(1);
    let tables_dir = args.next().unwrap_or_else(|| "temp/tables".to_string());
    let out_path = args
        .next()
        .unwrap_or_else(|| "src/core/convert/translate/tables_gen.rs".to_string());

    let kitten_to_kn = read_map(&tables_dir, "kittenToKn.json")?;
    let mutation_text = read_map(&tables_dir, "kittenMutationText.json")?;
    let shadow_dict = read_nested(&tables_dir, "shadowDict.json")?;
    let chinese_names = read_map(&tables_dir, "chineseNameDict.json")?;
    let config = read_map(&tables_dir, "config.json")?;
    let provenance = read_provenance(&tables_dir, "kittenToKn.json")?;

    let mut plain_text = BTreeMap::new();
    let mut select_text: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (k, v) in &mutation_text {
        match v {
            Value::String(s) => {
                plain_text.insert(k.clone(), s.clone());
            }
            Value::Object(o) => {
                let mut inner = BTreeMap::new();
                for (ik, iv) in o {
                    if let Some(s) = iv.as_str() {
                        inner.insert(ik.clone(), s.to_string());
                    }
                }
                select_text.insert(k.clone(), inner);
            }
            _ => {}
        }
    }

    // 中文名反查(类型 → 中文 token),降级标题兜底用
    let mut zh_by_type: BTreeMap<String, String> = BTreeMap::new();
    for (zh, t) in &chinese_names {
        if let Some(t) = t.as_str() {
            zh_by_type
                .entry(t.to_string())
                .or_insert_with(|| zh.clone());
        }
    }

    let landscape = config
        .get("stage_size")
        .and_then(|s| s.get("landscape"))
        .cloned()
        .unwrap_or_default();
    let portrait = config
        .get("stage_size")
        .and_then(|s| s.get("portrait"))
        .cloned()
        .unwrap_or_default();
    let bcm_version = config
        .get("bcm_version")
        .and_then(Value::as_str)
        .unwrap_or("0.0.0");

    let mut out = String::new();
    let _ = writeln!(out, "//! **本文件由生成器产出,不要手改。**");
    let _ = writeln!(out, "//!");
    let _ = writeln!(
        out,
        "//! 生成命令:`cargo run --bin gen_translate_tables -- temp/tables src/core/convert/translate/tables_gen.rs`"
    );
    let _ = writeln!(out, "//! 手工补充放 `mapping.rs`,不受重跑影响。");
    let _ = writeln!(out, "//!");
    let _ = writeln!(out, "//! 数据来源:{}", provenance);
    let _ = writeln!(out, "//! KittenN 版本常量 `bcm_version` = {bcm_version:?}");
    let _ = writeln!(out);
    let _ = writeln!(out, "/// Kitten → KN 积木类型映射(`LC`,恒等条目也在表里)");
    let _ = writeln!(out, "pub(crate) const KITTEN_TO_KN: &[(&str, &str)] = &[");
    for (k, v) in &kitten_to_kn {
        if let Some(v) = v.as_str() {
            let _ = writeln!(out, "    ({:?}, {:?}),", k, v);
        }
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(
        out,
        "/// 降级占位积木的中文标题(`RC` 的纯字符串部分,按官方原样保留 `{{字段}}`)"
    );
    let _ = writeln!(
        out,
        "pub(crate) const KITTEN_MUTATION_TEXT: &[(&str, &str)] = &["
    );
    for (k, v) in &plain_text {
        let _ = writeln!(out, "    ({:?}, {:?}),", k, v);
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(
        out,
        "/// `RC` 里由官方 handler 按字段选词的部分(我们按选择器近似,见 docs/rounds/20 已知偏差)"
    );
    let _ = writeln!(
        out,
        "pub(crate) const KITTEN_MUTATION_TEXT_SELECT: &[(&str, &[(&str, &str)])] = &["
    );
    for (k, inner) in &select_text {
        let pairs: Vec<String> = inner
            .iter()
            .map(|(ik, iv)| format!("({:?}, {:?})", ik, iv))
            .collect();
        let _ = writeln!(out, "    ({:?}, &[{}]),", k, pairs.join(", "));
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(out, "/// 每个块的默认 shadow XML 覆盖表(`cy`)");
    // 该表当前只被 `mapping::SHADOW_XML_INDEX`(仅 `#[cfg(test)]` 用)读 ⇒ 生成物里也标 cfg(test),
    // 两处同步(手改生成物 + 生成器,见 rounds/40 R2;本机没有 temp/tables,无法重跑生成器核对)。
    let _ = writeln!(out, "#[cfg(test)]");
    let _ = writeln!(
        out,
        "pub(crate) const SHADOW_XML: &[(&str, &[(&str, &str)])] = &["
    );
    for (k, inner) in &shadow_dict {
        let pairs: Vec<String> = inner
            .iter()
            .map(|(ik, iv)| format!("({:?}, {:?})", ik, iv.as_str().unwrap_or_default()))
            .collect();
        let _ = writeln!(out, "    ({:?}, &[{}]),", k, pairs.join(", "));
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(out, "/// 中文名反查(类型 → 中文 token),占位积木标题兜底");
    let _ = writeln!(
        out,
        "pub(crate) const ZH_NAME_BY_TYPE: &[(&str, &str)] = &["
    );
    for (t, zh) in &zh_by_type {
        let _ = writeln!(out, "    ({:?}, {:?}),", t, zh);
    }
    let _ = writeln!(out, "];\n");

    let _ = writeln!(out, "/// KN 里的四种「文本占位积木」");
    let _ = writeln!(
        out,
        "pub(crate) const TEXT_PLACEHOLDER_BLOCKS: [&str; 4] = [\n    \"bcm_translator_text_execution_block\",\n    \"bcm_translator_text_event_block\",\n    \"bcm_translator_text_return_value_block\",\n    \"bcm_translator_text_return_boolean_block\",\n];\n"
    );

    let _ = writeln!(out, "/// 目标格式版本常量(`qC.bcm_version`)");
    let _ = writeln!(out, "pub(crate) const BCM_VERSION: &str = {bcm_version:?};");
    let _ = writeln!(out, "/// 竖屏画布(562×900)");
    let _ = writeln!(
        out,
        "pub(crate) const STAGE_PORTRAIT: (f64, f64) = ({:?}, {:?});",
        portrait
            .get("width")
            .and_then(Value::as_f64)
            .unwrap_or(562.0),
        portrait
            .get("height")
            .and_then(Value::as_f64)
            .unwrap_or(900.0)
    );
    let _ = writeln!(out, "/// 横屏画布(900×562)");
    let _ = writeln!(
        out,
        "pub(crate) const STAGE_LANDSCAPE: (f64, f64) = ({:?}, {:?});",
        landscape
            .get("width")
            .and_then(Value::as_f64)
            .unwrap_or(900.0),
        landscape
            .get("height")
            .and_then(Value::as_f64)
            .unwrap_or(562.0)
    );

    let out_path = Path::new(&out_path);
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建输出目录 {} 失败:{e}", parent.display()))?;
    }
    std::fs::write(out_path, out).map_err(|e| format!("写 {} 失败:{e}", out_path.display()))?;
    Ok(format!(
        "已生成 {}:LC {} 条 / RC 文本 {} 条 + 选择表 {} 条 / shadow {} 条",
        out_path.display(),
        kitten_to_kn.len(),
        plain_text.len(),
        select_text.len(),
        shadow_dict.len(),
    ))
}

fn load(tables_dir: &str, file: &str) -> Result<Value, String> {
    let path = Path::new(tables_dir).join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "读不到 {}:{e}(先用逆向工具导出表格,见 docs/rounds/20 附录 C)",
            path.display()
        )
    })?;
    serde_json::from_str(&text).map_err(|e| format!("{} 不是合法 JSON:{e}", path.display()))
}

fn data(tables_dir: &str, file: &str) -> Result<Value, String> {
    let v = load(tables_dir, file)?;
    Ok(v.get("data").cloned().unwrap_or(v))
}

fn read_map(tables_dir: &str, file: &str) -> Result<BTreeMap<String, Value>, String> {
    match data(tables_dir, file)? {
        Value::Object(o) => Ok(o.into_iter().collect()),
        other => Err(format!("{file}: 期望对象,得到 {other}")),
    }
}

fn read_nested(
    tables_dir: &str,
    file: &str,
) -> Result<BTreeMap<String, BTreeMap<String, Value>>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in read_map(tables_dir, file)? {
        let inner = match v {
            Value::Object(o) => o.into_iter().collect(),
            other => return Err(format!("{file}: {k} 期望对象,得到 {other}")),
        };
        out.insert(k, inner);
    }
    Ok(out)
}

fn read_provenance(tables_dir: &str, file: &str) -> Result<String, String> {
    let v = load(tables_dir, file)?;
    let p = v.get("$provenance").cloned().unwrap_or(Value::Null);
    let url = p
        .get("source")
        .and_then(|s| s.get("url"))
        .and_then(Value::as_str);
    let sha = p
        .get("source")
        .and_then(|s| s.get("sha256"))
        .and_then(Value::as_str);
    Ok(match (url, sha) {
        (Some(u), Some(s)) => format!("{u} (sha256 {s})"),
        _ => "未知(provenance 缺失)".to_string(),
    })
}
