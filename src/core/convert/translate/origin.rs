//! 「未收录积木」标记的**原内容载荷**(rounds/55)
//!
//! KN 侧编辑器不认识的积木,写出阶段会就地改成平台自己的「未收录积木」标记
//! (`incompatible_block` / `incompatible_output_block`)。标记块的定义是 `args0: []`
//! (线上编辑器 bundle `kitten.fc848ad0…`)——**没有字段能承载**原积木的类型与取值,
//! 此前这一步是**不可恢复**的:块还在画布上,内容(类型/字段/影子/变异)丢了。
//!
//! 这里把原积木的**标量内容**存进平台**原生注释**(`block_data_json.comments` +
//! 积木上的 `comment` 引用;平台原件里用户注释就是这么存的,格式与键名照抄),于是:
//!
//! - 产物仍能被编辑器加载(注释是平台自己的机制);
//! - 损失从"看得见但不可恢复"变成"看得见且可恢复":本模块的 [`restore_marked_blocks`]
//!   在反向(Kitten4 → KN)解析时按注释把原积木**逐字段还回去**;
//! - 注释文本第一行是给人看的类型名,第二行才是机器载荷(哨兵 [`COMMENT_SENTINEL`] 之后
//!   是一行紧凑 JSON)。
//!
//! 载荷只存**标量**:子块不复制(标记块的连接与子块在产物里原样保留),
//! 只额外记一份"KN 槽名 → 写出去的 Kitten4 槽名"的映射,还原时把子块放回原槽。

use super::mapping::reverse_slot_name;
use super::model::BlockJson;
use super::tables_gen::ZH_NAME_BY_TYPE;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// 机器载荷的哨兵。注释文本里第一次出现之后全是载荷 JSON。
///
/// 同时是"这份作品有没有本转换器写的标记注释"的**廉价判据**:源侧骨架路径(Raw)
/// 只在原文里出现它时才多做一次解析(见 `pipeline::comments_of`)。
pub(super) const COMMENT_SENTINEL: &str = "#convert-kn#";

/// 中文名(注释第一行给人看):与占位积木标题同源(`ZH_NAME_BY_TYPE`;手工补充表)
static ZH_NAME_INDEX: LazyLock<BTreeMap<&'static str, &'static str>> =
    LazyLock::new(|| ZH_NAME_BY_TYPE.iter().copied().collect());

/// 一个被改成「未收录积木」标记的 KN 块的原内容
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct KnOrigin {
    /// 原 KN 节点的标量 JSON(不含 `inputs`/`statements`/`next`;含 `type`/`fields`/
    /// `shadows`/`mutation`/`is_shadow`/`is_output`/渲染性 `extra`)
    pub node: Value,
    /// 槽位映射:`(KN 槽名, 写出去的 Kitten4 槽名)`,按 `inputs`/`statements` 原键序
    pub slots: Vec<(String, String)>,
}

impl KnOrigin {
    /// 在反向映射**改写字段/影子/槽名之前**抓快照(`mapping::reverse_node` 的调用点)
    pub(super) fn capture(kn_kind: &str, node: &BlockJson) -> Option<Self> {
        let slots = node
            .inputs
            .keys()
            .chain(node.statements.keys())
            .map(|slot| (slot.clone(), reverse_slot_name(kn_kind, slot)))
            .collect();
        let mut scalar = node.clone();
        // 调用点(`mapping::reverse_node`)已经把 `kind` 取空换成 `kn_kind` ⇒ 快照要显式填回
        scalar.kind = kn_kind.to_string();
        scalar.inputs.clear();
        scalar.statements.clear();
        scalar.next = None;
        scalar.origin = None;
        Some(KnOrigin {
            node: scalar.to_value().ok()?,
            slots,
        })
    }

    /// 写进平台注释的文本(第一行给人看,第二行是机器载荷)
    pub(super) fn comment_text(&self) -> String {
        let kind = self
            .node
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let label = ZH_NAME_INDEX.get(kind).copied().unwrap_or(kind);
        let payload = serde_json::to_string(&self).unwrap_or_else(|_| String::from("{}"));
        format!(
            "未收录积木:{label}({kind})。原积木内容已保存在本注释里,转换回 KittenN 时自动还原。\n\
             {COMMENT_SENTINEL}{payload}"
        )
    }

    /// 从注释文本还原载荷(不是本转换器写的注释 → `None`)
    pub(super) fn from_comment(text: &str) -> Option<Self> {
        let start = text.find(COMMENT_SENTINEL)? + COMMENT_SENTINEL.len();
        serde_json::from_str(&text[start..]).ok()
    }
}

/// 平台注释对象(键名与平台原件一致;`location` 取积木坐标再右移,避免压住积木)
pub(super) fn comment_entry(
    id: &str,
    block_id: &str,
    text: String,
    location: Option<&Value>,
) -> Value {
    let (x, y) = location
        .and_then(Value::as_array)
        .and_then(|array| {
            let x = array.first()?.as_f64()?;
            let y = array.get(1)?.as_f64()?;
            Some((x, y))
        })
        .unwrap_or((0.0, 0.0));
    json!({
        "id": id,
        "parent_id": block_id,
        "text": text,
        "pinned": false,
        "size": [214, 126],
        "auto_layout": true,
        "color_theme": "BLUE",
        "location": [x + 220.0, y],
    })
}

/// 这份块表里的注释(id → 文本);两种 `block_data_json` 形态共用
pub(super) fn comments_of(value: &Value) -> BTreeMap<String, String> {
    value
        .get("comments")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(id, entry)| {
                    entry
                        .get("text")
                        .and_then(Value::as_str)
                        .map(|text| (id.clone(), text.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 一个节点是否是写出阶段的「未收录积木」标记(还原前是 Kitten4 名,还原点在正向映射之后)
fn is_marker(kind: &str) -> bool {
    super::mapping::is_text_placeholder(kind)
        || matches!(kind, "incompatible_block" | "incompatible_output_block")
}

/// 把树里"带原内容注释的标记块"换回原 KN 积木,返回还原块数
pub(super) fn restore_marked_blocks(
    tree: &mut super::model::BlockTree,
    comments: &BTreeMap<String, String>,
) -> usize {
    let mut restored = 0;
    let roots = std::mem::take(&mut tree.roots);
    tree.roots = roots
        .into_iter()
        .map(|root| restore_node(root, comments, &mut restored))
        .collect();
    restored
}

fn restore_node(
    mut node: BlockJson,
    comments: &BTreeMap<String, String>,
    restored: &mut usize,
) -> BlockJson {
    if is_marker(&node.kind)
        && let Some(origin) = node
            .extra
            .get("comment")
            .and_then(Value::as_str)
            .and_then(|id| comments.get(id))
            .and_then(|text| KnOrigin::from_comment(text))
        && let Ok(mut original) = BlockJson::from_value(&origin.node)
    {
        // 子块:产物里仍是"Kitten4 槽名" ⇒ 按载荷里的映射放回 KN 槽名(其余原样保留)
        let mut inputs = std::mem::take(&mut node.inputs);
        let mut statements = std::mem::take(&mut node.statements);
        for (kn_slot, written_slot) in origin.slots {
            if let Some(child) = inputs.remove(&written_slot) {
                original.inputs.insert(kn_slot.clone(), child);
            }
            if let Some(child) = statements.remove(&written_slot) {
                original.statements.insert(kn_slot, child);
            }
        }
        original.inputs.extend(inputs);
        original.statements.extend(statements);
        original.next = node.next.take();
        // id / 坐标 / 父 id 以产物里的为准(往返里它们没变;缺 id 时才用快照的)
        if node.id.is_some() {
            original.id = node.id.take();
        }
        if node.location.is_some() {
            original.location = node.location.take();
        }
        if node.parent_id.is_some() {
            original.parent_id = node.parent_id.take();
        }
        node = original;
        *restored += 1;
    }
    for child in node.inputs.values_mut() {
        let taken = std::mem::take(child);
        *child = restore_node(taken, comments, restored);
    }
    for child in node.statements.values_mut() {
        let taken = std::mem::take(child);
        *child = restore_node(taken, comments, restored);
    }
    if let Some(next) = node.next.take() {
        node.next = Some(Box::new(restore_node(*next, comments, restored)));
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(value: Value) -> BlockJson {
        BlockJson::from_value(&value).expect("读成节点")
    }

    #[test]
    fn comment_round_trips_scalar_content() {
        let source = node(json!({
            "type": "temporary_list",
            "id": "b1",
            "fields": { "list": "list-1" },
            "shadows": { "MUTATE_BUTTON": "" },
            "mutation": "<mutation items=\"2\"><arg name=\"PARAMS0\"/></mutation>",
            "is_output": true,
            "inputs": { "VALUE": { "type": "math_number", "id": "n1" } }
        }));
        let origin = KnOrigin::capture("temporary_list", &source).expect("抓快照");
        assert_eq!(
            origin.node.get("type").and_then(Value::as_str),
            Some("temporary_list")
        );
        assert!(origin.node.get("inputs").is_none(), "子块不进载荷");
        assert_eq!(
            origin.slots,
            vec![("VALUE".to_string(), "VALUE".to_string())]
        );

        let text = origin.comment_text();
        assert!(text.contains("临时列表(temporary_list)"), "{text}");
        let back = KnOrigin::from_comment(&text).expect("读回载荷");
        assert_eq!(back, origin);
    }

    #[test]
    fn restore_puts_subblocks_back_into_kn_slots() {
        // 反向写出后的形态:标记块 + 原内容注释 + 子块已按写出槽名挂好
        let mut tree = super::super::model::BlockTree::new(vec![node(json!({
            "type": "incompatible_output_block",
            "id": "b1",
            "extra_kept": true,
            "comment": "c1",
            "inputs": { "WRITTEN": { "type": "math_number", "id": "n1", "fields": { "NUM": "3" } } }
        }))]);
        let origin = KnOrigin {
            node: json!({
                "type": "temporary_list",
                "id": "b1",
                "fields": { "list": "list-1" },
                "is_output": true
            }),
            slots: vec![("VALUE".to_string(), "WRITTEN".to_string())],
        };
        let comments = BTreeMap::from([("c1".to_string(), origin.comment_text())]);
        assert_eq!(restore_marked_blocks(&mut tree, &comments), 1);
        let restored = &tree.roots[0];
        assert_eq!(restored.kind, "temporary_list");
        assert_eq!(restored.fields.get("list"), Some(&json!("list-1")));
        assert!(restored.is_output);
        assert_eq!(
            restored.inputs.get("VALUE").map(|b| b.id.as_deref()),
            Some(Some("n1")),
            "子块按载荷槽名放回"
        );
        assert!(
            restored.extra.get("comment").is_none(),
            "注释本身是 Kitten4 侧的东西,不进 KN 产物"
        );
        assert!(
            restored.extra.get("extra_kept").is_none(),
            "渲染键来自原 KN 快照"
        );
    }

    #[test]
    fn marker_without_our_comment_is_left_alone() {
        let mut tree = super::super::model::BlockTree::new(vec![node(json!({
            "type": "bcm_translator_text_execution_block",
            "id": "p1",
            "fields": { "a": 1 }
        }))]);
        let comments = BTreeMap::new();
        assert_eq!(restore_marked_blocks(&mut tree, &comments), 0);
        assert_eq!(tree.roots[0].kind, "bcm_translator_text_execution_block");
    }
}
