//! 中核数据模型:积木节点/树(两种编辑器归一到它)+ id 生成。
//!
//! 设计要点(见 `docs/rounds/20-kitten-kn-work-conversion-plan.md` §6.2):
//!
//! - **不做语义 IR**:节点就是编辑器自己的 JSON 形状(KN 的 `nekoBlockJsonList` 元素),
//!   只是把"树 vs 邻接表"的差异交给 adapter;
//! - **保真**:本编辑器特有的键(`collapsed`/`movable`/`field_extra_attr`/`inline`/`visible`…)
//!   一律进 [`BlockJson::extra`],往返不丢;
//! - **顺序**:`inputs`/`statements`/`fields`/`shadows` 用 `BTreeMap`(键名排序),
//!   输出稳定、可 diff;积木树本身的先后由 `next` 与槽位表达,不依赖 map 顺序。

use crate::core::convert::shared::{DecompilerError, IdGenerator, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

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
    ///
    /// **借用**反序列化:`serde_json` 为 `&Value` 实现了 `Deserializer`,直接读原树即可。
    /// 旧实现是 `serde_json::from_value(Value::Object(obj.clone()))` —— 每块先深拷贝一份
    /// 整块 JSON(含 `fields`/`inputs`/`shadows`)再消费它;10 万级块时是纯浪费
    /// (方案 23 P0-3)。`kind` 走 `de_string` 容错解析:缺失/显式 null 都折成空串。
    pub(crate) fn from_value(value: &Value) -> Result<Self> {
        if !value.is_object() {
            return Err(DecompilerError::TypeMismatch {
                expected: "object(block json)".into(),
                actual: type_name(value).into(),
            });
        }
        let node: BlockJson =
            serde::Deserialize::deserialize(value).map_err(DecompilerError::from)?;
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

// ===========================================================================
// id 生成:KN 侧实体/程序集用 UUID 形态,影子和块 id 用小写短 id。
//
// 官方编辑器每次运行都现铸 UUID(`crypto.randomUUID`),因此同一作品两次转换的产物
// **不会**逐字节相同(实测:同一输入两次运行有 28 个 id 不同)。为了能跟官方产物做
// 逐字节对齐与往返测试,`IdSource::new(true)`(确定性模式)改用递增计数器生成固定形态的 id。
// ===========================================================================

/// 铸造种类(`uuid` / `short`)。
///
/// 记录模式(见 [`IdSource::recording`])只记"这一项的第几次铸造是什么形态",
/// 最终 id 由串行阶段按**全局铸造序号**兑现 —— 形态是兑现时唯一需要的输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MintKind {
    /// [`IdSource::uuid`]
    Uuid,
    /// [`IdSource::short`]
    Short,
}

impl MintKind {
    /// 临时 id 里的形态标记(单字符,见 [`TEMP_ID_PREFIX`])
    fn tag(self) -> char {
        match self {
            MintKind::Uuid => 'u',
            MintKind::Short => 's',
        }
    }

    /// 用**串行** id 源兑现这次铸造(确定性模式下 = 第 counter 次铸造的纯函数)
    pub(crate) fn mint(self, ids: &mut IdSource) -> String {
        match self {
            MintKind::Uuid => ids.uuid(),
            MintKind::Short => ids.short(),
        }
    }
}

/// 临时 id 的哨兵前缀。
///
/// 真实 id 是 UUID / 十六进制短串,**不含控制字符**,因此带哨兵的串只可能来自
/// [`IdSource::recording`],可以在改写时按前缀无歧义地定位(方案 25 §3 阶段 C)。
pub(crate) const TEMP_ID_PREFIX: char = '\u{1}';

/// 临时 id 体允许的字符(用于单遍改写时确定 token 的右边界)
pub(crate) fn is_temp_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.')
}

/// id 来源(确定性 / 随机 / 记录)
#[derive(Clone)]
pub(crate) struct IdSource {
    deterministic: bool,
    counter: u64,
    chars: IdGenerator,
    /// 记录模式(实体级并行):`Some` 时 [`IdSource::uuid`]/[`IdSource::short`]
    /// 不推进全局计数器,而是产出**临时 id** 并记账
    record: Option<IdRecord>,
}

/// 记录模式的账本
#[derive(Clone)]
struct IdRecord {
    /// 记账槽位(工作项序号,**同一份文档里必须两两不同**)—— 临时 id 靠它保证全局唯一
    slot: usize,
    /// 本槽位的铸造账本:按铸造顺序攒 `(临时 id, 形态)`
    log: Vec<(String, MintKind)>,
}

impl IdSource {
    pub(crate) fn new(deterministic: bool) -> Self {
        IdSource {
            deterministic,
            counter: 0,
            chars: IdGenerator::new(),
            record: None,
        }
    }

    /// 记录模式:产出临时 id(不带全局计数),铸造顺序记进账本
    ///
    /// `slot` 是**记账槽位**,`同一份文档里必须两两不同` —— 临时 id 的全局唯一性完全靠它:
    /// 同一实体在阶段 1(解析/映射/抽程序集)与阶段 2(调用点重写)各要一个记录源,
    /// 两阶段必须用**不同的**槽位(否则同 `slot` + 同 `seq` 会撞成同一个临时 id)。
    ///
    /// 确定性标志在这里无用武之地:临时 id 只是占位符,最终值由串行阶段按账本顺序兑现
    /// (确定性模式下与"今天串行实现"逐字节相同)。
    pub(crate) fn recording(slot: usize) -> Self {
        IdSource {
            deterministic: false,
            counter: 0,
            chars: IdGenerator::new(),
            record: Some(IdRecord {
                slot,
                log: Vec::new(),
            }),
        }
    }

    /// 取出铸造账本(`(临时 id, 形态)`,顺序 = 铸造顺序);非记录模式返回空
    pub(crate) fn into_log(self) -> Vec<(String, MintKind)> {
        self.record.map(|record| record.log).unwrap_or_default()
    }

    /// 记录模式:产出临时 id `"\u{1}prov:{slot}:{seq}:{tag}"` 并记账
    ///
    /// `slot` 必须**同一份文档内唯一**(见 [`IdSource::recording`])。
    fn temp(&mut self, kind: MintKind) -> String {
        let record = self.record.as_mut().expect("临时 id 只在记录模式下铸造");
        let id = format!(
            "{TEMP_ID_PREFIX}prov:{}:{}:{}",
            record.slot,
            record.log.len(),
            kind.tag()
        );
        record.log.push((id.clone(), kind));
        id
    }

    /// UUID v4 形态(实体 / 程序集 / KN 影子块)
    pub(crate) fn uuid(&mut self) -> String {
        if self.record.is_some() {
            return self.temp(MintKind::Uuid);
        }
        self.counter += 1;
        if self.deterministic {
            return format!("00000000-0000-4000-8000-{:012x}", self.counter);
        }
        let hex = |n: usize| -> String {
            (0..n)
                .map(|_| std::char::from_digit(fastrand::u32(0..16), 16).unwrap())
                .collect()
        };
        let variant = ['8', '9', 'a', 'b'][fastrand::usize(0..4)]; // RFC 4122 variant 位固定为 10xx(单字符)
        format!(
            "{}-{}-4{}-{}{}-{}",
            hex(8),
            hex(4),
            hex(3),
            variant,
            hex(3),
            hex(12)
        )
    }

    /// 22 字符短 id(与反编译侧 `IdGenerator` 同风格;KN 里两种形态都见得到)
    pub(crate) fn short(&mut self) -> String {
        if self.record.is_some() {
            return self.temp(MintKind::Short);
        }
        self.counter += 1;
        if self.deterministic {
            return format!("{:0>22}", format!("{:x}", self.counter));
        }
        self.chars.generate(22)
    }
}

#[cfg(test)]
mod id_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn deterministic_ids_are_stable_and_unique() {
        let mut a = IdSource::new(true);
        let mut b = IdSource::new(true);
        let left: Vec<String> = (0..5).map(|_| a.uuid()).collect();
        let right: Vec<String> = (0..5).map(|_| b.uuid()).collect();
        assert_eq!(left, right, "同一序号序列必须一致");
        assert_eq!(left.len(), left.iter().collect::<HashSet<_>>().len());
        assert!(
            left.iter().all(|id| id.len() == 36),
            "UUID 形态:{}",
            left[0]
        );
    }

    #[test]
    fn random_ids_look_like_uuid_v4() {
        let mut src = IdSource::new(false);
        let id = src.uuid();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn short_ids_have_fixed_length() {
        let mut src = IdSource::new(false);
        assert_eq!(src.short().len(), 22);
        let mut det = IdSource::new(true);
        assert_eq!(det.short().len(), 22);
    }

    /// 记录模式(实体级并行):产出带哨兵的临时 id、按铸造顺序记账、不推进全局计数;
    /// 账本按顺序交给串行源兑现后,结果与"同一序列直接在串行源上铸造"逐个相同。
    #[test]
    fn recording_mode_logs_mints_and_replays_identically() {
        let mut recorded = IdSource::recording(7);
        let temps: Vec<String> = vec![recorded.uuid(), recorded.short(), recorded.uuid()];
        assert!(
            temps.iter().all(|id| id.starts_with(TEMP_ID_PREFIX)),
            "记录模式必须产出临时 id:{temps:?}"
        );
        assert_eq!(
            temps,
            vec![
                format!("{TEMP_ID_PREFIX}prov:7:0:u"),
                format!("{TEMP_ID_PREFIX}prov:7:1:s"),
                format!("{TEMP_ID_PREFIX}prov:7:2:u"),
            ]
        );
        let log = recorded.into_log();
        assert_eq!(log.len(), 3);
        assert_eq!(
            log.iter().map(|(_, kind)| *kind).collect::<Vec<_>>(),
            vec![MintKind::Uuid, MintKind::Short, MintKind::Uuid]
        );

        // 兑现:与"直接串行铸造"逐字节一致(确定性模式下 id 是第几次铸造的纯函数)
        let mut replay = IdSource::new(true);
        let finals: Vec<String> = log.iter().map(|(_, kind)| kind.mint(&mut replay)).collect();
        let mut direct = IdSource::new(true);
        let expected = vec![direct.uuid(), direct.short(), direct.uuid()];
        assert_eq!(finals, expected);
    }
}
