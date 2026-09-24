//! 中核积木节点:两个编辑器的积木树都归一到它,再各自编码回目标格式。
//!
//! 设计要点(见 `docs/20-kitten-kn-work-conversion-plan.md` §6.2):
//!
//! - **不做语义 IR**:节点就是编辑器自己的 JSON 形状(KN 的 `nekoBlockJsonList` 元素),
//!   只是把"树 vs 邻接表"的差异交给 adapter;
//! - **保真**:本编辑器特有的键(`collapsed`/`movable`/`field_extra_attr`/`inline`/`visible`…)
//!   一律进 [`BlockJson::extra`],往返不丢;
//! - **顺序**:`inputs`/`statements`/`fields`/`shadows` 用 `BTreeMap`(键名排序),
//!   输出稳定、可 diff;积木树本身的先后由 `next` 与槽位表达,不依赖 map 顺序。

use crate::core::convert::shared::{DecompilerError, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};

/// 一个积木节点(树形)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(crate) struct BlockJson {
    /// 积木类型(编辑器内的 snake_case 名,如 `repeat_n_times` / `on_running_group_activated`)
    #[serde(rename = "type", default, deserialize_with = "de_string")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 工作区坐标(KN 常有;Kitten4 侧叫 `location`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Value>,
    /// 链式后继
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<Box<BlockJson>>,
    /// 值输入槽
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "de_map"
    )]
    pub inputs: BTreeMap<String, BlockJson>,
    /// 语句槽(`DO`/`DO0`/`STACK`/`ELSE`…)
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "de_map"
    )]
    pub statements: BTreeMap<String, BlockJson>,
    /// 字段(下拉/实体引用 id/字面量)
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "de_map"
    )]
    pub fields: BTreeMap<String, Value>,
    /// 每个输入槽的 shadow(XML 字符串,空串表示占位)
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "de_map"
    )]
    pub shadows: BTreeMap<String, String>,
    /// mutation XML
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "de_bool"
    )]
    pub is_shadow: bool,
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "de_bool"
    )]
    pub is_output: bool,
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "de_bool"
    )]
    pub shield: bool,
    #[serde(
        default,
        skip_serializing_if = "is_false",
        deserialize_with = "de_bool"
    )]
    pub disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// 数字输入的约束(KN 用;原样搬运)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_constraints: Option<Value>,
    /// 本编辑器特有键,原样保真
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

// 真作品里这些键可能是显式 `null`(serde 的 `default` 只兜住"缺失",兜不住 null),
// 所以 bool / String / Map 都要走容错解析;否则整份作品会因为一个 null 直接失败。
fn de_bool<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(deserializer)?.unwrap_or(false))
}

fn de_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn de_map<'de, D, V>(deserializer: D) -> std::result::Result<BTreeMap<String, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    V: Deserialize<'de>,
{
    Ok(Option::<BTreeMap<String, V>>::deserialize(deserializer)?.unwrap_or_default())
}

impl BlockJson {
    /// 从 JSON 对象建节点(未知键进 `extra`)
    pub(crate) fn from_value(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| DecompilerError::TypeMismatch {
                expected: "object(block json)".into(),
                actual: type_name(value).into(),
            })?;
        let mut node: BlockJson =
            serde_json::from_value(Value::Object(obj.clone())).map_err(DecompilerError::from)?;
        // serde 的 flatten 已把未知键收进 extra;这里只做类型名兜底
        if node.kind.is_empty() {
            node.kind = obj
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
        Ok(node)
    }

    /// 转回 JSON 对象
    pub(crate) fn to_value(&self) -> Result<Value> {
        serde_json::to_value(self).map_err(DecompilerError::from)
    }

    /// 深度优先遍历(含 next / inputs / statements)
    pub(crate) fn walk<F: FnMut(&BlockJson)>(&self, f: &mut F) {
        f(self);
        for child in self.inputs.values() {
            child.walk(f);
        }
        for child in self.statements.values() {
            child.walk(f);
        }
        if let Some(next) = &self.next {
            next.walk(f);
        }
    }

    /// 可变深度优先遍历
    pub(crate) fn walk_mut<F: FnMut(&mut BlockJson)>(&mut self, f: &mut F) {
        f(self);
        for child in self.inputs.values_mut() {
            child.walk_mut(f);
        }
        for child in self.statements.values_mut() {
            child.walk_mut(f);
        }
        if let Some(next) = &mut self.next {
            next.walk_mut(f);
        }
    }

    /// 节点总数(含自身)
    pub(crate) fn count(&self) -> usize {
        let mut n = 0;
        self.walk(&mut |_| n += 1);
        n
    }

    /// 类型频次统计
    pub(crate) fn count_types(&self, out: &mut BTreeMap<String, usize>) {
        self.walk(&mut |b| *out.entry(b.kind.clone()).or_default() += 1);
    }

    /// 收集所有 id
    pub(crate) fn collect_ids(&self, out: &mut HashSet<String>) {
        self.walk(&mut |b| {
            if let Some(id) = &b.id {
                out.insert(id.clone());
            }
        });
    }

    /// 按槽名取输入(只读)
    pub(crate) fn input(&self, slot: &str) -> Option<&BlockJson> {
        self.inputs.get(slot).or_else(|| self.statements.get(slot))
    }

    /// 按槽名取输入(可写)
    pub(crate) fn input_mut(&mut self, slot: &str) -> Option<&mut BlockJson> {
        if self.inputs.contains_key(slot) {
            self.inputs.get_mut(slot)
        } else {
            self.statements.get_mut(slot)
        }
    }

    /// 是否"空节点"(没有任何内容)
    pub(crate) fn is_empty_node(&self) -> bool {
        self.kind.is_empty()
    }
}

/// 一个实体(角色/场景/程序集)的积木树集合
#[derive(Debug, Clone, Default)]
pub(crate) struct BlockTree {
    /// 根积木(工作区里互不相连的脚本头)
    pub roots: Vec<BlockJson>,
}

impl BlockTree {
    pub(crate) fn new(roots: Vec<BlockJson>) -> Self {
        BlockTree { roots }
    }

    /// 按 `type` 找根
    pub(crate) fn find_root(&self, kind: &str) -> Option<&BlockJson> {
        self.roots.iter().find(|b| b.kind == kind)
    }

    pub(crate) fn count(&self) -> usize {
        self.roots.iter().map(BlockJson::count).sum()
    }

    pub(crate) fn count_types(&self) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        for r in &self.roots {
            r.count_types(&mut out);
        }
        out
    }

    pub(crate) fn walk<F: FnMut(&BlockJson)>(&self, f: &mut F) {
        for r in &self.roots {
            r.walk(f);
        }
    }

    pub(crate) fn walk_mut<F: FnMut(&mut BlockJson)>(&mut self, mut f: F) {
        for r in &mut self.roots {
            r.walk_mut(&mut f);
        }
    }
}

/// JSON 值的类型名(错误信息用)
pub(crate) fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn value_round_trips_through_block_json() {
        let value = json!({
            "type": "repeat_n_times",
            "id": "b1",
            "location": [334.4444580078125, -12.5],
            "shadows": { "times": "<shadow type=\"math_number\"/>", "DO": "" },
            "fields": { "sprite": "--self" },
            "collapsed": false
        });
        let node = BlockJson::from_value(&value).expect("解析");
        let back = node.to_value().expect("序列化");
        assert_eq!(back["type"], value["type"]);
        assert_eq!(back["location"], value["location"]);
        assert_eq!(back["shadows"], value["shadows"]);
        assert_eq!(back["collapsed"], value["collapsed"], "未知键经 extra 保真");
    }

    /// 依赖 `serde_json` 的 `float_roundtrip` feature:
    /// 关掉它时 `240.88868713378906` 会解析成 240.88868713378903(差 1 ULP),
    /// 于是与官方 JS 产物做逐字节对齐时,`location` 这类浮点会假报差异。
    #[test]
    fn floats_are_parsed_with_correct_rounding() {
        let parsed: f64 = serde_json::from_str("240.88868713378906").expect("解析浮点");
        assert_eq!(parsed, 240.88868713378906_f64);
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            "240.88868713378906",
            "往返后必须逐字一致"
        );
    }

    #[test]
    fn tree_utilities_walk_and_count() {
        let root = json!({
            "type": "root", "id": "r",
            "inputs": { "A": { "type": "mid", "id": "m", "inputs": { "A": { "type": "leaf", "id": "l" } } } },
            "next": { "type": "second", "id": "s" }
        });
        let tree = BlockTree::new(vec![BlockJson::from_value(&root).unwrap()]);
        assert_eq!(tree.count(), 4);
        let mut ids = std::collections::HashSet::new();
        tree.walk(&mut |b| {
            if let Some(id) = &b.id {
                ids.insert(id.clone());
            }
        });
        assert_eq!(ids.len(), 4);
        let types = tree.count_types();
        assert_eq!(types.get("mid"), Some(&1));
        assert_eq!(types.get("leaf"), Some(&1));
    }
}

#[cfg(test)]
mod null_tolerance_tests {
    use super::*;
    use serde_json::json;

    /// 真作品里这些键可能是显式 null:不能被一个 null 拖垮整份作品的解析。
    #[test]
    fn explicit_nulls_do_not_break_parsing() {
        let value = json!({
            "type": "repeat_n_times",
            "id": "b1",
            "is_shadow": null,
            "is_output": null,
            "shield": null,
            "disabled": null,
            "mutation": null,
            "location": null,
            "inputs": null,
            "statements": null,
            "fields": null,
            "shadows": null,
            "field_constraints": null,
            "parent_id": null
        });
        let node = BlockJson::from_value(&value).expect("null 容错");
        assert_eq!(node.kind, "repeat_n_times");
        assert!(!node.is_shadow && !node.is_output && !node.shield && !node.disabled);
        assert!(node.inputs.is_empty() && node.statements.is_empty());
        assert!(node.fields.is_empty() && node.shadows.is_empty());
        assert!(node.mutation.is_none() && node.location.is_none());
    }

    #[test]
    fn null_type_becomes_empty_kind() {
        let node = BlockJson::from_value(&json!({ "type": null, "id": "x" })).expect("解析");
        assert_eq!(node.kind, "");
    }
}
