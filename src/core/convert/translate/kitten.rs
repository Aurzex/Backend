//! Kitten 侧 adapter(Kitten2/3/4)。
//!
//! 本文件两半都在:
//!
//! - **前端**:Kitten4 编辑版 `block_data_json = {blocks, connections, comments}` 的**邻接表**
//!   → [`BlockTree`](中核树)([`parse_block_data_json`]);
//! - **后端**(反向,Phase 4):中核树 → 邻接表([`build_block_data_json`]),重建
//!   `blocks`/`connections`/`parent_id`/`location`(根积木按 80 + 220·i 排开,与
//!   `decompile` 侧 `XmlBlockWriter` 的约定一致)。
//!
//! 与官方实现(`kittenBcmToNekoBcmUtils` 里的 `jC.parseBlock`,`mod41888.pretty.js:77578`)对齐的语义:
//!
//! - `blocks` 是 `id → block` 字典(积木本体);
//! - `connections[parent][child] = {type: "next"|"input", input_type?: "value"|"statement", input_name?}`;
//! - **根积木** = 从未作为子键出现过的 id;
//! - `input` + `input_type:"statement"` 进 `statements` 槽,其余 `input` 进 `inputs` 槽;
//! - `next` 连接语义上与 `input_name` 无关,挂到 `next`;
//! - shadow / 字段 / mutation 原样搬运(`shadows` 是 XML 字符串);
//! - 同一子积木可以出现在多个父之下的**菱形**结构里,官方是逐边重新展开,我们照做;
//!   只有**环**会致命,遇环直接报错(官方会栈溢出)。

use crate::core::convert::shared::{DecompilerError, Result};
use serde_json::{Map, Value, json};
use std::collections::HashSet;

use super::model::{BlockJson, BlockTree, type_name};
use super::model::IdSource;

/// 一个实体的解析结果
#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedEntity {
    pub tree: BlockTree,
}

/// 解析 Kitten4 编辑版的 `block_data_json`
pub(crate) fn parse_block_data_json(block_data_json: &Value) -> Result<ParsedEntity> {
    let bdj = block_data_json
        .as_object()
        .ok_or_else(|| DecompilerError::TypeMismatch {
            expected: "object(block_data_json)".into(),
            actual: type_name(block_data_json).into(),
        })?;

    // 旧作品可能把 block_data_json 存成 JSON 字符串(编辑器容错),这里也接受
    if let Some(blocks) = bdj.get("blocks").and_then(Value::as_object) {
        return parse_parts(blocks, bdj.get("connections"));
    }
    if let Some(text) = bdj.get("blocks").and_then(Value::as_str) {
        let inner: Value = serde_json::from_str(text).map_err(DecompilerError::from)?;
        let inner_obj = inner
            .as_object()
            .ok_or_else(|| DecompilerError::TypeMismatch {
                expected: "object(block_data_json.blocks 字符串内容)".into(),
                actual: type_name(&inner).into(),
            })?;
        let inner_blocks = inner_obj
            .get("blocks")
            .and_then(Value::as_object)
            .ok_or_else(|| DecompilerError::MissingField {
                field: "block_data_json.blocks.blocks".into(),
            })?;
        return parse_parts(inner_blocks, inner_obj.get("connections"));
    }

    Err(DecompilerError::MissingField {
        field: "block_data_json.blocks".into(),
    })
}

fn parse_parts(
    blocks: &Map<String, Value>,
    connections: Option<&Value>,
) -> Result<ParsedEntity> {
    let connections = connections
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // 子键集合(判定根):出现的 id 一律不是根
    let mut child_ids: HashSet<&str> = HashSet::new();
    for entry in connections.values() {
        if let Some(children) = entry.as_object() {
            child_ids.extend(children.keys().map(String::as_str));
        }
    }

    // 根:按 id 排序保证确定性(serde_json 的 Map 默认 key 有序)
    let mut roots = Vec::new();
    for (id, block) in blocks {
        if child_ids.contains(id.as_str()) {
            continue;
        }
        let mut ancestors = Vec::new();
        roots.push(build_node(id, block, blocks, &connections, &mut ancestors)?);
    }

    // 有积木却没有任何根 = 整张图是环(每个 id 都当过别人的子节点),
    // 官方实现会直接栈溢出,这里明确报错而不是静默产出空树。
    if roots.is_empty() && !blocks.is_empty() {
        return Err(DecompilerError::Decompile(format!(
            "Kitten 积木图没有根节点({} 个积木互相成环)",
            blocks.len()
        )));
    }

    Ok(ParsedEntity {
        tree: BlockTree::new(roots),
    })
}

/// 递归把一个积木及其子图变成节点
fn build_node(
    id: &str,
    block: &Value,
    blocks: &Map<String, Value>,
    connections: &Map<String, Value>,
    ancestors: &mut Vec<String>,
) -> Result<BlockJson> {
    if ancestors.iter().any(|a| a == id) {
        return Err(DecompilerError::Decompile(format!(
            "Kitten 积木图存在环:节点 {id} 重复出现在自身祖先链上"
        )));
    }
    ancestors.push(id.to_string());

    let mut node = BlockJson::from_value(block)?;
    if node.id.is_none() {
        node.id = Some(id.to_string());
    }

    if let Some(children) = connections.get(id).and_then(Value::as_object) {
        for (child_id, link) in children {
            let child_block = blocks.get(child_id).ok_or_else(|| {
                DecompilerError::Decompile(format!("积木 {id} 的连接指向不存在的子积木 {child_id}"))
            })?;
            let child = build_node(child_id, child_block, blocks, connections, ancestors)?;
            match link.get("type").and_then(Value::as_str).unwrap_or("next") {
                "next" => node.next = Some(Box::new(child)),
                "input" => {
                    let slot = link
                        .get("input_name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if slot.is_empty() {
                        return Err(DecompilerError::Decompile(format!(
                            "积木 {id} → {child_id} 的 input 连接缺少 input_name"
                        )));
                    }
                    if link.get("input_type").and_then(Value::as_str) == Some("statement") {
                        node.statements.insert(slot, child);
                    } else {
                        node.inputs.insert(slot, child);
                    }
                }
                other => {
                    return Err(DecompilerError::Decompile(format!(
                        "积木 {id} → {child_id} 的连接类型未知:{other}"
                    )));
                }
            }
        }
    }

    ancestors.pop();
    Ok(node)
}

// ---------------------------------------------------------------- 后端(反向,Phase 4)

/// 根积木的初始纵坐标(`XmlBlockWriter` 的约定:首根 80、每根 +220,互不重叠)
const ROOT_LAYOUT_Y: i64 = 80;
const ROOT_LAYOUT_STEP: i64 = 220;

/// 中核树 → Kitten4 编辑版 `block_data_json = {blocks, connections, comments}`
///
/// - `blocks` 是 `id → 积木` 字典(子节点**不**内联,全部平铺);
/// - `connections[parent][child] = {type:"next"}` 或 `{type:"input", input_type, input_name}`,
///   每个积木都有一条(叶子是空对象 `{}`,与真实 `.bcm4` 一致);
/// - 根积木按 `ROOT_LAYOUT_Y + 220*i` 排开;子积木的 `parent_id` 指向父积木(根为 `null`);
/// - `next` 子节点的 `parent_id` 指向链上前一个积木(Kitten4 实测如此);
/// - 缺 id / id 重复(菱形展开的同一积木被两个槽位引用)时现铸新 id —— `blocks` 是 id 字典,
///   不重铸就会互相覆盖(正向产物里这类重复 id 有 49 个)。
pub(crate) fn build_block_data_json(tree: &BlockTree, ids: &mut IdSource) -> Result<Value> {
    let mut blocks: Map<String, Value> = Map::new();
    let mut connections: Map<String, Value> = Map::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (index, root) in tree.roots.iter().enumerate() {
        let mut node = root.clone();
        if node.location.is_none() {
            node.location = Some(json!([0, ROOT_LAYOUT_Y + ROOT_LAYOUT_STEP * index as i64]));
        }
        encode_block(
            &mut node,
            None,
            &mut blocks,
            &mut connections,
            &mut seen,
            ids,
        )?;
    }
    Ok(json!({
        "blocks": Value::Object(blocks),
        "connections": Value::Object(connections),
        "comments": Value::Object(Map::new()),
    }))
}

/// 写一个积木(含其整棵子树),返回它最终落盘的 id
fn encode_block(
    node: &mut BlockJson,
    parent_id: Option<&str>,
    blocks: &mut Map<String, Value>,
    connections: &mut Map<String, Value>,
    seen: &mut HashSet<String>,
    ids: &mut IdSource,
) -> Result<String> {
    let id = match node.id.clone() {
        Some(id) if !id.is_empty() && seen.insert(id.clone()) => id,
        _ => {
            let fresh = ids.short();
            seen.insert(fresh.clone());
            fresh
        }
    };
    node.parent_id = parent_id.map(str::to_string);

    // 子节点先编码(id 才知道),同时把连接表攒出来
    let mut links = Map::new();
    for slot in ["inputs", "statements"] {
        let is_statement = slot == "statements";
        let children = if is_statement {
            std::mem::take(&mut node.statements)
        } else {
            std::mem::take(&mut node.inputs)
        };
        for (name, mut child) in children {
            let child_id = encode_block(&mut child, Some(&id), blocks, connections, seen, ids)?;
            links.insert(
                child_id,
                json!({
                    "type": "input",
                    "input_type": if is_statement { "statement" } else { "value" },
                    "input_name": name,
                }),
            );
        }
    }
    if let Some(mut next) = node.next.take() {
        let child_id = encode_block(&mut next, Some(&id), blocks, connections, seen, ids)?;
        links.insert(child_id, json!({ "type": "next" }));
    }

    if node.mutation.is_none() {
        node.mutation = Some(String::new());
    }
    if node.field_constraints.is_none() {
        node.field_constraints = Some(json!({}));
    }
    let mut value = node.to_value()?;
    if let Some(object) = value.as_object_mut() {
        for (key, default) in KITTEN4_DEFAULTS {
            object
                .entry((*key).to_string())
                .or_insert_with(|| (*default)());
        }
        object.insert(
            String::from("parent_id"),
            parent_id.map_or(Value::Null, |parent| Value::String(parent.to_string())),
        );
    }
    blocks.insert(id.clone(), value);
    connections.insert(id.clone(), Value::Object(links));
    Ok(id)
}

/// Kitten4 每块都写的键(缺省值);已有值一律不覆盖
type DefaultFactory = fn() -> Value;
#[rustfmt::skip]
const KITTEN4_DEFAULTS: &[(&str, DefaultFactory)] = &[
    ("comment", || Value::Null), ("collapsed", || Value::Bool(false)), ("disabled", || Value::Bool(false)),
    ("deletable", || Value::Bool(true)), ("movable", || Value::Bool(true)), ("editable", || Value::Bool(true)),
    ("visible", || Value::String(String::from("visible"))), ("is_shadow", || Value::Bool(false)),
    ("is_output", || Value::Bool(false)), ("fields", || Value::Object(Map::new())),
    ("shadows", || Value::Object(Map::new())), ("field_extra_attr", || Value::Object(Map::new())),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bdj(blocks: Value, connections: Value) -> Value {
        json!({ "blocks": blocks, "connections": connections, "comments": {} })
    }

    #[test]
    fn parses_next_chain_and_roots() {
        let value = bdj(
            json!({
                "a": { "type": "start_on_click", "id": "a", "location": [0, 0] },
                "b": { "type": "repeat_forever", "id": "b", "shadows": { "DO": "" } },
                "c": { "type": "self_appear", "id": "c" }
            }),
            json!({
                "a": { "b": { "type": "next" } },
                "b": { "c": { "type": "next" } },
                "c": {}
            }),
        );
        let parsed = parse_block_data_json(&value).expect("parse");
        assert_eq!(parsed.tree.roots.len(), 1);
        let root = &parsed.tree.roots[0];
        assert_eq!(root.kind, "start_on_click");
        assert_eq!(root.next.as_ref().unwrap().kind, "repeat_forever");
        assert_eq!(
            root.next.as_ref().unwrap().next.as_ref().unwrap().kind,
            "self_appear"
        );
        assert_eq!(parsed.tree.count(), 3);
    }

    #[test]
    fn routes_value_and_statement_inputs() {
        let value = bdj(
            json!({
                "p": { "type": "repeat_n_times", "id": "p" },
                "v": { "type": "math_number", "id": "v", "is_shadow": true, "fields": { "NUM": "10" } },
                "s": { "type": "self_go_forward", "id": "s" }
            }),
            json!({
                "p": {
                    "v": { "type": "input", "input_type": "value", "input_name": "times" },
                    "s": { "type": "input", "input_type": "statement", "input_name": "DO" }
                }
            }),
        );
        let parsed = parse_block_data_json(&value).expect("parse");
        let root = &parsed.tree.roots[0];
        assert_eq!(
            root.inputs.get("times").map(|b| b.kind.as_str()),
            Some("math_number")
        );
        assert_eq!(
            root.statements.get("DO").map(|b| b.kind.as_str()),
            Some("self_go_forward")
        );
        assert_eq!(parsed.tree.roots.len(), 1);
    }

    #[test]
    fn keeps_shadows_fields_and_unknown_keys() {
        let value = bdj(
            json!({
                "b": {
                    "type": "repeat_n_times", "id": "b",
                    "shadows": { "times": "<shadow type=\"math_number\"/>", "DO": "" },
                    "fields": { "sprite": "--self" },
                    "field_constraints": { "NUM": { "min": 1 } },
                    "collapsed": false, "movable": true, "visible": "visible",
                    "mutation": "", "deletable": true
                }
            }),
            json!({ "b": {} }),
        );
        let parsed = parse_block_data_json(&value).expect("parse");
        let node = &parsed.tree.roots[0];
        assert_eq!(
            node.shadows.get("times").map(String::as_str),
            Some("<shadow type=\"math_number\"/>")
        );
        assert_eq!(node.shadows.get("DO").map(String::as_str), Some(""));
        assert_eq!(node.fields.get("sprite"), Some(&json!("--self")));
        assert!(node.field_constraints.is_some());
        assert_eq!(node.extra.get("collapsed"), Some(&json!(false)));
        assert_eq!(node.extra.get("movable"), Some(&json!(true)));
        assert_eq!(node.extra.get("visible"), Some(&json!("visible")));
        assert_eq!(node.mutation.as_deref(), Some(""));
    }

    #[test]
    fn accepts_legacy_stringified_blocks() {
        let inner = json!({
            "blocks": { "a": { "type": "start_on_click", "id": "a" } },
            "connections": { "a": {} }
        });
        let value = json!({ "blocks": inner.to_string(), "connections": {}, "comments": {} });
        let parsed = parse_block_data_json(&value).expect("parse");
        assert_eq!(parsed.tree.roots.len(), 1);
        assert_eq!(parsed.tree.roots[0].kind, "start_on_click");
    }

    #[test]
    fn rejects_cycles_dangling_children_and_missing_blocks() {
        // 可达环:r → r2 → r(有根,靠祖先链检测)
        let reachable_cycle = bdj(
            json!({
                "r": { "type": "x", "id": "r" },
                "r2": { "type": "y", "id": "r2" }
            }),
            json!({
                "r": { "r2": { "type": "next" } },
                "r2": { "r": { "type": "next" } }
            }),
        );
        assert!(parse_block_data_json(&reachable_cycle).is_err());

        // 纯环(无根):每个 id 都当过别人的子节点
        let rootless = bdj(
            json!({ "a": { "type": "x", "id": "a" } }),
            json!({ "a": { "a": { "type": "next" } } }),
        );
        assert!(parse_block_data_json(&rootless).is_err());

        let dangling = bdj(
            json!({ "a": { "type": "x", "id": "a" } }),
            json!({ "a": { "gone": { "type": "next" } } }),
        );
        assert!(parse_block_data_json(&dangling).is_err());

        assert!(parse_block_data_json(&json!({ "connections": {} })).is_err());

        // 空 blocks 合法(空实体)
        let empty =
            parse_block_data_json(&json!({ "blocks": {}, "connections": {}, "comments": {} }))
                .expect("空实体解析为空树");
        assert_eq!(empty.tree.roots.len(), 0);
    }

    #[test]
    fn diamond_children_are_expanded_twice() {
        // 官方逐边递归:同一子积木被两个槽引用时,两边各展开一份(不共享节点)
        let diamond = json!({
            "blocks": {
                "r": { "type": "root", "id": "r" },
                "mid": { "type": "mid", "id": "mid" },
                "s": { "type": "shared", "id": "s" }
            },
            "connections": {
                "r": {
                    "mid": { "type": "input", "input_type": "value", "input_name": "A" },
                    "s": { "type": "input", "input_type": "value", "input_name": "B" }
                },
                "mid": { "s": { "type": "input", "input_type": "value", "input_name": "A" } },
                "s": {}
            },
            "comments": {}
        });
        let parsed = parse_block_data_json(&diamond).expect("parse");
        assert_eq!(parsed.tree.roots.len(), 1);
        assert_eq!(parsed.tree.count(), 4, "s 在两个槽下各展开一份");
    }
}
