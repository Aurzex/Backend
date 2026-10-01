use super::report::{TranslateReport, TranslateWarning};
use super::xml::{math_number_shadow, xml_attr_value};
use crate::core::convert::shared::XHTML;
use crate::core::convert::shared::{DecompilerError, IdGenerator, Result};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

// 来自 src/core/convert/translate/model.rs
// 中核数据模型:积木节点/树(两种编辑器归一到它)+ id 生成。
// 设计要点(见 `docs/rounds/20-kitten-kn-work-conversion-plan.md` §6.2):
// - **不做语义 IR**:节点就是编辑器自己的 JSON 形状(KN 的 `nekoBlockJsonList` 元素),
// 只是把"树 vs 邻接表"的差异交给 adapter;
// - **保真**:本编辑器特有的键(`collapsed`/`movable`/`field_extra_attr`/`inline`/`visible`…)
// 一律进 [`BlockJson::extra`],往返不丢;
// - **顺序**:`inputs`/`statements`/`fields`/`shadows` 用 `BTreeMap`(键名排序),
// 输出稳定、可 diff;积木树本身的先后由 `next` 与槽位表达,不依赖 map 顺序。

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

/// `math_number` 子积木
pub(crate) fn math_number_node(id: String, num: &str, parent_id: Option<String>) -> BlockJson {
    BlockJson {
        kind: "math_number".to_string(),
        id: Some(id),
        is_shadow: true,
        fields: BTreeMap::from([(String::from("NUM"), Value::String(num.to_string()))]),
        field_constraints: Some(
            json!({"NUM": {"min": null, "max": null, "precision": 0, "mod": null}}),
        ),
        is_output: true,
        parent_id,
        ..Default::default()
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
mod model_tests {
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
    ///
    /// ⚠️ **冻结协议(rounds/37 §1 0.3)**:确定性模式下产物里的 id 是"第几次铸造"的纯函数
    /// (`{counter:012x}`)⇒ **铸造顺序就是产物的一部分**:任何调整铸造次数/次序的重构
    /// (含合并两次插入、延迟到需要时才铸)都会整体偏移 id,**必须先解释再接受 SHA 变化**。
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

// 来自 src/core/convert/translate/kitten.rs
// Kitten 侧 adapter(Kitten2/3/4)。
// 本文件两半都在:
// - **前端**:Kitten4 编辑版 `block_data_json = {blocks, connections, comments}` 的**邻接表**
// → [`BlockTree`](中核树)([`parse_block_data_json`]);
// - **后端**(反向,Phase 4):中核树 → 邻接表([`build_block_data_json`]),重建
// `blocks`/`connections`/`parent_id`/`location`(根积木按 80 + 220·i 排开,与
// `decompile` 侧 `XmlBlockWriter` 的约定一致)。
// 与官方实现(`kittenBcmToNekoBcmUtils` 里的 `jC.parseBlock`,`mod41888.pretty.js:77578`)对齐的语义:
// - `blocks` 是 `id → block` 字典(积木本体);
// - `connections[parent][child] = {type: "next"|"input", input_type?: "value"|"statement", input_name?}`;
// - **根积木** = 从未作为子键出现过的 id;
// - `input` + `input_type:"statement"` 进 `statements` 槽,其余 `input` 进 `inputs` 槽;
// - `next` 连接语义上与 `input_name` 无关,挂到 `next`;
// - shadow / 字段 / mutation 原样搬运(`shadows` 是 XML 字符串);
// - 同一子积木可以出现在多个父之下的**菱形**结构里,官方是逐边重新展开,我们照做;
// 只有**环**会致命,遇环直接报错(官方会栈溢出)。

/// 解析 Kitten4 编辑版的 `block_data_json`
pub(crate) fn parse_block_data_json(block_data_json: &Value) -> Result<BlockTree> {
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

fn parse_parts(blocks: &Map<String, Value>, connections: Option<&Value>) -> Result<BlockTree> {
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

    Ok(BlockTree::new(roots))
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
mod kitten_tests {
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
        assert_eq!(parsed.roots.len(), 1);
        let root = &parsed.roots[0];
        assert_eq!(root.kind, "start_on_click");
        assert_eq!(root.next.as_ref().unwrap().kind, "repeat_forever");
        assert_eq!(
            root.next.as_ref().unwrap().next.as_ref().unwrap().kind,
            "self_appear"
        );
        assert_eq!(parsed.count(), 3);
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
        let root = &parsed.roots[0];
        assert_eq!(
            root.inputs.get("times").map(|b| b.kind.as_str()),
            Some("math_number")
        );
        assert_eq!(
            root.statements.get("DO").map(|b| b.kind.as_str()),
            Some("self_go_forward")
        );
        assert_eq!(parsed.roots.len(), 1);
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
        let node = &parsed.roots[0];
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
        assert_eq!(parsed.roots.len(), 1);
        assert_eq!(parsed.roots[0].kind, "start_on_click");
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
        assert_eq!(empty.roots.len(), 0);
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
        assert_eq!(parsed.roots.len(), 1);
        assert_eq!(parsed.count(), 4, "s 在两个槽下各展开一份");
    }
}

// 来自 src/core/convert/translate/neko.rs
// KN 侧积木 adapter(前端 + 后端)。
// 对应官方 webpack module 41888(`temp/ref/mod41888.pretty.js`)里 `kittenBcmToNekoBcmUtils`
// 收尾的三段(`docs/rounds/20-kitten-kn-work-conversion-plan.md` §3.2):
// - `HC` 的另一半(78421-78440):根积木按类型一分为二——`procedures_2_defnoreturn` 摘出去当
// 程序集定义,其余留在实体的 `nekoBlockJsonList`;[`split_procedures`]
// - `zC(entity, proceduresDict)`(78481-78576):每个定义 → `{id,name,type,params,nekoBlockJsonList}`
// 条目。参数表 = 合成 `Label`(新 UUID)+ 按 `PARAMS<n>` 序号排序的 `String` 形参;`inputs.STACK`
// 搬进 `statements.STACK`;定义体里每个 `procedures_2_parameter` 引用补上
// `<mutation id="<形参 id>">` + `is_output`;整棵树套上 `<arg …>` mutation 且
// `fields.NAME` 换成条目 id;含返回值(`ZC` 78406)时**再多产出一条 `ROUND` 条目**
// (`VC` 78441 补默认 `VALUE` 输入 + `WC` 78473 从 `NORMAL` 主体里剥掉 `VALUE`);
// [`split_procedures`]
// - `KC(entity, proceduresDict)`(78577-78638):实体里 `procedures_2_callnoreturn` /
// `procedures_2_callreturn` 的 `fields.NAME`(源侧是程序集**名**)换成目标**id**,
// 重建 `mutation def_id/name/type` + 每个参数 `<arg content>`;`String` 形参另换
// `math_number` 影子,并把老槽位 `ARG<i-1>` 的输入**复制**(官方是复制不是搬移,两边都留)
// 到形参 id 槽;同名程序集找不到时原样保留。[`rewrite_calls`]
// 两条纪律:
// 1. `GC`(官方 `zC`/`KC` 内部还会再套一层坐标/特例改写)在
// [`mapping::translate_kitten_to_kn`](super::mapping::translate_kitten_to_kn) 里已经对整棵树
// 做过,这里**绝不重复施加**(否则横屏坐标会被除两次 1.3);因此管线顺序是
// `model::parse_block_data_json` → `mapping::translate_kitten_to_kn` → `model::split_procedures`
// → `model::rewrite_calls`(实体树)。
// 2. 上游的 `id` 生成走 [`IdSource`]:合成 `Label` 形参、`KC` 的影子、`VC` 的默认输入都要
// **现铸** UUID(官方 `BC()`),确定性模式下才可逐字节对齐。
// ## 反向(KN → Kitten4,官方无此方向,本库自建)
// - [`parse_kn_entity`]:`nekoBlockJsonList` 数组 → 中核树(空/缺失 → 空树);
// - [`parse_kn_procedures`]:`proceduresDict` → [`ProcedureEntry`](形参/定义体原样);
// - [`unrewrite_calls`]:`KC` 的逆 —— `fields.NAME`(程序集 id)换回**名字**,重建 Kitten4 的
// `<mutation name def_id>` + `procedures_2_parameter_shadow` 与 `ARG<j>` 影子,并把 `KC`
// 复制到形参 id 槽位上的输入搬回 `ARG<j>`;
// - [`def_root_from_entry`]:`zC` 的逆 —— 形参回到 `PARAMS<j>` 输入(`procedures_2_stable_parameter`
// 子块,id 沿用形参 id)、`mutation` 回 `<arg name="PARAMS<j>">`、`deletable/editable` 回 `true`。
// ## 与官方的刻意差异 / 近似
// - **id 重铸不进报告**:合成 `Label`/影子/默认输入的 id 都是新铸的、且没有对应的"旧 id"被丢,
// 逐个记 [`TranslateWarning::RemintedId`](super::TranslateWarning::RemintedId) 只会淹没报告;
// 确定性模式([`IdSource::new(true)`](IdSource::new))下它们本来就稳定可对齐。
// - **静默覆盖改成有据可查**:官方把定义积木的 `statements` 整体换成 `{STACK}`、把 `fields`
// 整体换成 `{NAME: id}`,多出来的槽位会被无声丢掉;本实现照做但逐条记
// [`TranslateWarning::DroppedField`](super::TranslateWarning::DroppedField)。
// - **参数名对不上**:官方只 `console.warn("找不到名为 X 的参数")` 然后跳过;本实现同样跳过,
// 但记 `DroppedField`(`procedures.<定义名>.param.<形参名>`),便于事后定位。
// - `PARAMS<n>` 序号:官方 `parseInt(key.replace("PARAMS",""), 10)`,取不到数字时是 `NaN`
// (排序结果由引擎决定);本实现把"取不到数字"记作 `0`。
// - 参数引用的 mutation 补全:官方那段遍历**隔层**(见 [`annotate_param_refs`] 的说明),真机产物里
// 因此有没补 mutation 的形参引用;本实现"遍历到就补",是官方结果的**超集**。
// - **`shield` 在编码端补齐**:官方 `jC.parseBlock` 给每个节点写 `shield: !!t.shield`(Kitten4
// 源里没这个键 → 恒 `false`),而 [`BlockJson`] 的 `shield` 是 `skip_serializing_if = "is_false"`;
// 不补齐的话产物里会少一个官方必写的键([`fill_shield`],真机产物里 439 个节点全带)。
// - `mutation` / `<arg>` 里的名字、字段一律**不做 XML 转义**(官方也不做),逐字拼接。
// - **键顺序**:`BlockJson` 按结构体字段序、`serde_json::Map`(未开 `preserve_order`)按字典序,
// 与官方的"插入顺序"不同 —— 值等价(见 §3.2 的对齐实验),逐字节不等价。

/// 程序集定义根积木(`HC` 拆出来的那一类)
const DEF_ROOT: &str = "procedures_2_defnoreturn";
/// 源侧形参声明积木(定义里 `inputs.PARAMS<n>`)
const STABLE_PARAM: &str = "procedures_2_stable_parameter";
/// 源侧形参引用积木(定义体里用到参数的地方)
const PARAM_REF: &str = "procedures_2_parameter";
/// 返回值积木(带 `VALUE` 输入即"有返回值")
const RETURN_VALUE: &str = "procedures_2_return_value";
/// 调用积木(无返回值 / 有返回值)
const CALL_TYPES: [&str; 2] = ["procedures_2_callnoreturn", "procedures_2_callreturn"];
/// 程序集类型(官方 `MC` 枚举)
const KIND_NORMAL: &str = "NORMAL";
const KIND_ROUND: &str = "ROUND";
/// 形参类型(官方 `IParamType` 枚举,这里只用到两个)
const PARAM_LABEL: &str = "Label";
const PARAM_STRING: &str = "String";

/// 定义体里逐节点遍历时的合成标签/字段名
const FIELD_NAME: &str = "NAME";
const FIELD_PARAM_NAME: &str = "param_name";
/// 定义体里存放积木链的那个输入槽
const SLOT_STACK: &str = "STACK";
/// `PARAMS<n>` 前缀
const PARAMS_PREFIX: &str = "PARAMS";

/// 官方 `VC`(78441)给返回值积木补的默认 `VALUE` 影子 XML(逐字节照搬,含换行与缩进)
const VALUE_SHADOW_XML: &str = "<shadow type=\"math_number\">\n        <field name=\"TEXT\" constraints=\"-Infinity,Infinity,0,\" allow_text=\"true\">0</field>\n      </shadow>";

/// 一个程序集形参(官方 `params[]` 元素)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcedureParam {
    /// 目标侧形参 id(合成 Label 是新铸 UUID;String 沿用源侧形参积木 id)
    pub id: String,
    /// 形参类型:`Label`(程序集名本身)/ `String`
    pub kind: String,
    /// 形参名(中文)
    pub name: String,
}

/// 一条 `proceduresDict` 条目
#[derive(Debug, Clone)]
pub(crate) struct ProcedureEntry {
    /// 条目 id(`NORMAL` = 源定义积木 id;`ROUND` = 现铸 UUID)
    pub id: String,
    /// 程序集名(源侧 `fields.NAME`)
    pub name: String,
    /// 条目类型:`NORMAL` / `ROUND`
    pub kind: String,
    /// 形参表(第一个恒为合成的 `Label`)
    pub params: Vec<ProcedureParam>,
    /// 定义体(`nekoBlockJsonList`,单根)
    pub tree: BlockTree,
}

// ---------------------------------------------------------------- 程序集拆分(zC + HC)

/// 把实体积木树里的 `procedures_2_defnoreturn` 根摘成程序集条目(`zC`),其余根原样留在树里。
///
/// 官方顺序:先处理**所有**场景再处理所有角色(`GN`,82821-82830),所以调用方按
/// `scenes → actors` 顺序喂进来,`procedures` 数组的顺序才与官方一致;`rewrite_calls`
/// 按数组顺序线性查找同名程序集,顺序错了两条同名程序集就会串。
pub(crate) fn split_procedures(
    tree: BlockTree,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) -> (BlockTree, Vec<ProcedureEntry>) {
    let mut rest = Vec::new();
    let mut procedures = Vec::new();
    for root in tree.roots {
        if root.kind == DEF_ROOT {
            split_one(root, ids, report, &mut procedures);
        } else {
            rest.push(root);
        }
    }
    (BlockTree::new(rest), procedures)
}

/// 单个定义 → 1 或 2 条条目(官方 `zC`,78481-78576)
fn split_one(
    mut def: BlockJson,
    ids: &mut IdSource,
    report: &mut TranslateReport,
    out: &mut Vec<ProcedureEntry>,
) {
    // a = e.fields?.NAME || ""
    let name = field_text(&def.fields, FIELD_NAME);
    let def_id = def.id.clone().unwrap_or_default();

    // o = [{id: BC(), type: "Label", name: a}]
    let mut params = vec![ProcedureParam {
        id: ids.uuid(),
        kind: PARAM_LABEL.into(),
        name: name.clone(),
    }];

    if !def.inputs.is_empty() {
        collect_params(&def.inputs, &mut params);
        // inputs.STACK → statements.STACK(官方整体替换 statements)
        if let Some(stack) = def.inputs.remove(SLOT_STACK) {
            for slot in def.statements.keys() {
                if slot != SLOT_STACK {
                    report.warn(TranslateWarning::DroppedField {
                        path: format!("{DEF_ROOT}.{name}.statements.{slot}"),
                    });
                }
            }
            def.statements = BTreeMap::from([(SLOT_STACK.to_string(), stack)]);
        }
    }

    // 定义体里的形参引用补 mutation id
    if let Some(stack) = def.statements.get_mut(SLOT_STACK) {
        annotate_param_refs(stack, &params, &name, report);
    }

    // l = params.map(p => '<arg …></arg>').join("")
    let arg_xml = params
        .iter()
        .map(|p| {
            format!(
                "<arg id=\"{}\" name=\"{}\" type=\"{}\"></arg>",
                p.id, p.name, p.kind
            )
        })
        .collect::<String>();

    // p(NORMAL 的返回条目用)= 未套 u 之前的那棵树
    let bare = def.clone();
    // u = {...e, mutation, fields:{NAME: e.id}, deletable:false, editable:false}
    for key in def.fields.keys() {
        if key != FIELD_NAME {
            report.warn(TranslateWarning::DroppedField {
                path: format!("{DEF_ROOT}.{name}.fields.{key}"),
            });
        }
    }
    def.mutation = Some(format!("<mutation xmlns=\"{XHTML}\">{arg_xml}</mutation>"));
    def.fields = BTreeMap::from([(FIELD_NAME.to_string(), Value::String(def_id.clone()))]);
    def.extra.insert("deletable".into(), Value::Bool(false));
    def.extra.insert("editable".into(), Value::Bool(false));

    if has_return_value(&def) {
        // NORMAL:克隆 e(WC 剥掉所有 return_value 的 VALUE 输入/影子)
        let mut normal = bare;
        strip_return_values(&mut normal);
        out.push(ProcedureEntry {
            id: normal.id.clone().unwrap_or_else(|| def_id.clone()),
            name: name.clone(),
            kind: KIND_NORMAL.into(),
            params: params.clone(),
            tree: BlockTree::new(vec![normal]),
        });

        // ROUND:u + 新 id,并把每个 return_value 的 VALUE 补上
        let mut round = def;
        round.id = Some(ids.uuid());
        inject_return_values(&mut round, ids);
        let round_id = round.id.clone().unwrap_or_default();
        out.push(ProcedureEntry {
            id: round_id,
            name,
            kind: KIND_ROUND.into(),
            params,
            tree: BlockTree::new(vec![round]),
        });
    } else {
        out.push(ProcedureEntry {
            id: def_id,
            name,
            kind: KIND_NORMAL.into(),
            params,
            tree: BlockTree::new(vec![def]),
        });
    }
}

/// `inputs` 里 `PARAMS<n>` 的形参声明 → `params[]`(官方 filter/sort/forEach,78488-78506)
fn collect_params(inputs: &BTreeMap<String, BlockJson>, params: &mut Vec<ProcedureParam>) {
    let mut slots: Vec<(i64, &BlockJson)> = inputs
        .iter()
        .filter(|(slot, node)| slot.starts_with(PARAMS_PREFIX) && node.kind == STABLE_PARAM)
        .map(|(slot, node)| (params_index(slot), node))
        .collect();
    slots.sort_by_key(|(index, _)| *index);

    for (_, node) in slots {
        // if (n.fields?.param_name) o.push({id: n.id, type:"String", name: n.fields.param_name})
        let Some(param_name) = node
            .fields
            .get(FIELD_PARAM_NAME)
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        params.push(ProcedureParam {
            id: node.id.clone().unwrap_or_default(),
            kind: PARAM_STRING.into(),
            name: param_name.to_string(),
        });
    }
}

/// 定义体(`statements.STACK` 子树)里给形参引用补 mutation(官方 78510-78535)。
///
/// ⚠️ 与官方的刻意差异(**超集**):官方回调里对子节点调用的是 `e`,而 `e` 只处理**孙节点**,
/// 于是"补 mutation"是**隔层**的(相位 1/3/5…),实测真机产物里确实存在没补上 mutation 的
/// `procedures_2_parameter`;而且官方是在 `GC` **之前**走这趟遍历,我们的 `GC` 在
/// [`mapping`](super::mapping) 里先跑,层数早已被坐标包装改写,复刻那份相位没有意义。
/// 本实现"遍历到就补"(凡是出现在 `inputs` 槽里的形参引用都补),结果是官方产物的**超集**,
/// 与 `mapping.rs` 的"官方丢掉的 `is_output`/`field_constraints` 我们保"口径一致。
fn annotate_param_refs(
    node: &mut BlockJson,
    params: &[ProcedureParam],
    proc: &str,
    report: &mut TranslateReport,
) {
    for child in node.inputs.values_mut() {
        if child.kind == PARAM_REF {
            let param_name = child
                .fields
                .get(FIELD_PARAM_NAME)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let hit = params
                .iter()
                .find(|p| p.name == param_name)
                .filter(|p| !p.id.is_empty());
            match hit {
                Some(param) => {
                    child.mutation = Some(format!(
                        "<mutation xmlns=\"{XHTML}\" id=\"{}\"></mutation>",
                        param.id
                    ));
                    child.is_output = true;
                }
                None => {
                    report.warn(TranslateWarning::DroppedField {
                        path: format!("{DEF_ROOT}.{proc}.param.{param_name}"),
                    });
                }
            }
        }
        annotate_param_refs(child, params, proc, report);
    }
    for child in node.statements.values_mut() {
        annotate_param_refs(child, params, proc, report);
    }
    if let Some(next) = node.next.as_deref_mut() {
        annotate_param_refs(next, params, proc, report);
    }
}

/// `ZC`(78406):子树里是否存在带 `VALUE` 输入的 `procedures_2_return_value`
fn has_return_value(node: &BlockJson) -> bool {
    if node.kind == RETURN_VALUE && node.inputs.contains_key("VALUE") {
        return true;
    }
    node.inputs.values().any(has_return_value)
        || node.statements.values().any(has_return_value)
        || node.next.as_deref().map(has_return_value).unwrap_or(false)
}

/// `WC`(78473):从所有 `procedures_2_return_value` 上剥掉 `VALUE` 输入与影子
fn strip_return_values(node: &mut BlockJson) {
    if node.kind == RETURN_VALUE {
        node.inputs.remove("VALUE");
        node.shadows.remove("VALUE");
    }
    for child in node.inputs.values_mut() {
        strip_return_values(child);
    }
    for child in node.statements.values_mut() {
        strip_return_values(child);
    }
    if let Some(next) = node.next.as_deref_mut() {
        strip_return_values(next);
    }
}

/// `VC`(78441):给每个缺 `VALUE` 的 `procedures_2_return_value` 补默认 `math_number` + 影子
fn inject_return_values(node: &mut BlockJson, ids: &mut IdSource) {
    if node.kind == RETURN_VALUE && !node.inputs.contains_key("VALUE") {
        // 官方 `VC` 的字面量(没有 location;`shield: false` 由编码端补)
        let child = math_number_node(ids.uuid(), "0", node.id.clone());
        node.inputs.insert("VALUE".into(), child);
        node.shadows
            .insert("VALUE".into(), VALUE_SHADOW_XML.to_string());
    }
    for child in node.inputs.values_mut() {
        inject_return_values(child, ids);
    }
    for child in node.statements.values_mut() {
        inject_return_values(child, ids);
    }
    if let Some(next) = node.next.as_deref_mut() {
        inject_return_values(next, ids);
    }
}

// ---------------------------------------------------------------- 调用点重写(KC)

/// 把实体树里的程序集调用点重写成"按 id 调用"(官方 `KC`,78577)。
///
/// 官方只对**实体**的 `nekoBlockJsonList` 调用 `KC`(程序集定义体不经此函数),
/// 且对每个节点先做 `GC`——`GC` 已由 [`mapping`](super::mapping) 施加,这里不再重复。
pub(crate) fn rewrite_calls(
    tree: &mut BlockTree,
    procedures: &[ProcedureEntry],
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    for root in &mut tree.roots {
        rewrite_node(root, procedures, ids, report);
    }
}

/// 单节点:命中调用积木则重写,然后递归所有子节点(官方 `KC` 内层 `e`,78579-78633)
fn rewrite_node(
    node: &mut BlockJson,
    procedures: &[ProcedureEntry],
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    if CALL_TYPES.contains(&node.kind.as_str()) {
        let callee_name = node
            .fields
            .get(FIELD_NAME)
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(callee) = callee_name
            .as_deref()
            .and_then(|n| procedures.iter().find(|p| p.name == n))
        {
            rewrite_call(node, callee, ids, report);
        }
    }
    for child in node.inputs.values_mut() {
        rewrite_node(child, procedures, ids, report);
    }
    for child in node.statements.values_mut() {
        rewrite_node(child, procedures, ids, report);
    }
    if let Some(next) = node.next.as_deref_mut() {
        rewrite_node(next, procedures, ids, report);
    }
}

/// 单个调用点:`fields.NAME` → 程序集 id,mutation 重建,`String` 形参换影子 + 复制 `ARG<i-1>`
fn rewrite_call(
    node: &mut BlockJson,
    callee: &ProcedureEntry,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    let old_fields = std::mem::take(&mut node.fields);
    for key in old_fields.keys() {
        if key != FIELD_NAME {
            report.warn(TranslateWarning::DroppedField {
                path: format!(
                    "procedures_2_call.{} -> {}.fields.{key}",
                    callee.name, callee.id
                ),
            });
        }
    }
    node.fields = BTreeMap::from([(FIELD_NAME.to_string(), Value::String(callee.id.clone()))]);
    // 官方整体替换 shadows(老影子 XML 丢弃),只留 NAME 占位 + 各 String 形参影子
    node.shadows = BTreeMap::from([(FIELD_NAME.to_string(), String::new())]);

    let mut mutation = format!(
        "<mutation xmlns=\"{XHTML}\" def_id=\"{id}\" name=\"{id}\" type=\"{kind}\">",
        id = callee.id,
        kind = callee.kind
    );
    for (index, param) in callee.params.iter().enumerate() {
        mutation.push_str(&format!(
            "<arg id=\"{}\" content=\"{}\" type=\"{}\"></arg>",
            param.id, param.name, param.kind
        ));
        if param.kind == PARAM_STRING {
            // 官方无条件写影子(即使对应输入不存在),影子 id 现铸
            node.shadows.insert(
                param.id.clone(),
                format!(
                    "<shadow xmlns=\"{XHTML}\" type=\"math_number\" id=\"{}\" visible=\"visible\"><field constraints=\"-Infinity,Infinity,0,\" allow_text=\"true\" name=\"NUM\">0</field></shadow>",
                    ids.uuid()
                ),
            );
            // ARG(t-1),t 是形参下标(0 号是 Label)→ String 形参抽头
            let source_slot = format!("ARG{}", index as i64 - 1);
            if let Some(input) = node.inputs.get(&source_slot).cloned() {
                node.inputs.insert(param.id.clone(), input);
            }
        }
    }
    mutation.push_str("</mutation>");
    node.mutation = Some(mutation);
}

// ---------------------------------------------------------------- 反向:KN → Kitten4(自建)

/// KN 的 `nekoBlockJsonList`(数组)→ 中核树。空/缺失 → 空树(实体可以没有积木)。
pub(crate) fn parse_kn_entity(list: &Value) -> Result<BlockTree> {
    // 数组形态(绝大多数)直接借用,**不再整表克隆**;只有字符串形态才需要持有一份解析结果
    let owned: Vec<Value>;
    let values: &[Value] = match list {
        Value::Array(items) => items,
        // 少数链路把该字段存成 JSON 字符串(与 Kitten 侧 `block_data_json` 的容错一致)
        Value::String(text) if !text.trim().is_empty() => {
            // 取出数组本体(移动),不再 `as_array().cloned()` 白拷一份整表
            let parsed: Value = serde_json::from_str(text).map_err(DecompilerError::from)?;
            owned = match parsed {
                Value::Array(items) => items,
                _ => Vec::new(),
            };
            &owned
        }
        _ => &[],
    };
    let mut roots = Vec::new();
    for value in values {
        let node = BlockJson::from_value(value)?;
        // 官方 `HC` 过滤掉 `type === ""` 的垃圾节点
        if node.kind.is_empty() {
            continue;
        }
        roots.push(node);
    }
    Ok(BlockTree::new(roots))
}

/// `procedures.proceduresDict`(或裸字典)→ 程序集条目(字段与 KN 完全一致)
pub(crate) fn parse_kn_procedures(dict: &Value) -> Result<Vec<ProcedureEntry>> {
    let dict = match dict {
        Value::Object(map) => match map.get("proceduresDict") {
            Some(inner) => inner,
            None => dict,
        },
        _ => return Ok(Vec::new()),
    };
    let Some(map) = dict.as_object() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(map.len());
    for (key, entry) in map {
        let Some(entry) = entry.as_object() else {
            continue;
        };
        let params = entry
            .get("params")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|param| {
                        Some(ProcedureParam {
                            id: param.get("id").and_then(Value::as_str)?.to_string(),
                            kind: param
                                .get("type")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                            name: param
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.push(ProcedureEntry {
            id: entry
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| key.clone()),
            name: entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            kind: entry
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or(KIND_NORMAL)
                .to_string(),
            params,
            tree: parse_kn_entity(entry.get("nekoBlockJsonList").unwrap_or(&Value::Null))?,
        });
    }
    Ok(out)
}

/// 只带元信息的程序集条目(反向调用点重写用:只需 id/name/params,不克隆定义体)
pub(crate) fn call_targets(procedures: &[ProcedureEntry]) -> Vec<ProcedureEntry> {
    procedures
        .iter()
        .map(|entry| ProcedureEntry {
            id: entry.id.clone(),
            name: entry.name.clone(),
            kind: entry.kind.clone(),
            params: entry.params.clone(),
            tree: BlockTree::default(),
        })
        .collect()
}

/// 调用点重写(`KC`)的**逆**:`fields.NAME`(程序集 id)换回程序集名,重建 Kitten 编辑器的 mutation 与
/// `ARG<j>` 影子,并把 `rewrite_calls` 复制到形参 id 槽位上的输入搬回 `ARG<j>`。
///
/// 官方只对**实体**的 `nekoBlockJsonList` 跑 `KC`;我们这里实体与程序集体都跑(程序集体里的调用点
/// 同样需要还原,否则 Kitten 编辑器看到的是拿不到引用的 UUID)。
pub(crate) fn unrewrite_calls(
    tree: &mut BlockTree,
    procedures: &[ProcedureEntry],
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    for root in &mut tree.roots {
        unrewrite_node(root, procedures, ids, report);
    }
}

fn unrewrite_node(
    node: &mut BlockJson,
    procedures: &[ProcedureEntry],
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    if CALL_TYPES.contains(&node.kind.as_str())
        && let Some(callee) = resolve_callee(node, procedures)
    {
        unrewrite_call(node, callee, ids, report);
    }
    for child in node.inputs.values_mut() {
        unrewrite_node(child, procedures, ids, report);
    }
    for child in node.statements.values_mut() {
        unrewrite_node(child, procedures, ids, report);
    }
    if let Some(next) = node.next.as_deref_mut() {
        unrewrite_node(next, procedures, ids, report);
    }
}

/// 从调用点解析目标程序集:`fields.NAME` 是 `KC` 写入的 id;退化时看 mutation 的 `def_id`/`name`
fn resolve_callee<'a>(
    node: &BlockJson,
    procedures: &'a [ProcedureEntry],
) -> Option<&'a ProcedureEntry> {
    let name = node.fields.get(FIELD_NAME);
    if let Some(id) = name.and_then(Value::as_str)
        && let Some(entry) = procedures.iter().find(|p| p.id == id)
    {
        return Some(entry);
    }
    if let Some(id) = name.and_then(Value::as_str)
        && let Some(entry) = procedures.iter().find(|p| p.name == id)
    {
        return Some(entry);
    }
    let mutation = node.mutation.as_deref()?;
    for attr in ["def_id", "name"] {
        if let Some(id) = xml_attr_value(mutation, attr)
            && let Some(entry) = procedures.iter().find(|p| p.id == id || p.name == id)
        {
            return Some(entry);
        }
    }
    None
}

fn unrewrite_call(
    node: &mut BlockJson,
    callee: &ProcedureEntry,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    // KN 的新版编辑器会把 `Custom`/`List` 形参的值以"形参 id → 字符串"的字段内联在调用点上,
    // Kitten4 没有这种形态:优先把它写进 mutation 的 `value` 与 `ARG<j>` 影子(P1 复现 4 处)。
    let mut fields = std::mem::take(&mut node.fields);
    fields.remove(FIELD_NAME);

    let mut mutation = format!(
        "<mutation xmlns=\"{XHTML}\" name=\"{}\" def_id=\"{}\">",
        callee.name, callee.id
    );
    let mut shadows: BTreeMap<String, String> = BTreeMap::new();
    let mut inputs = std::mem::take(&mut node.inputs);
    let mut rebounds = BTreeMap::new();
    for (index, param) in callee
        .params
        .iter()
        .filter(|p| p.kind != PARAM_LABEL)
        .enumerate()
    {
        let slot = format!("ARG{index}");
        // 正向 `KC` 把 `ARG<i-1>` **复制**到形参 id 槽位;反向优先取形参槽位(它才是权威值)
        let child = inputs.remove(&param.id).or_else(|| inputs.remove(&slot));
        if let Some(child) = child {
            rebounds.insert(slot.clone(), child);
        }
        let inline = fields
            .remove(&param.id)
            .and_then(|value| match value {
                Value::String(text) if !text.is_empty() => Some(text),
                Value::Number(number) => Some(number.to_string()),
                _ => None,
            })
            .unwrap_or_else(|| String::from("0"));
        mutation.push_str(&format!(
            "<procedures_2_parameter_shadow name=\"{}\" value=\"{}\"></procedures_2_parameter_shadow>",
            param.name, inline
        ));
        shadows.insert(slot, default_value_shadow(&ids.short(), &inline));
    }
    mutation.push_str("</mutation>");
    node.mutation = Some(mutation);

    // 老 `ARG<j>` 槽位如果还留着(没被形参槽位顶替),原样并回去;其余输入(理论上不该有)也不丢
    for (slot, child) in inputs {
        rebounds.entry(slot).or_insert(child);
    }
    node.inputs = rebounds;

    for key in fields.keys() {
        report.warn(TranslateWarning::DroppedField {
            path: format!("procedures_2_call.{}.fields.{key}", callee.name),
        });
    }
    node.fields = BTreeMap::from([(FIELD_NAME.to_string(), Value::String(callee.name.clone()))]);

    node.shadows = BTreeMap::from([(FIELD_NAME.to_string(), String::new())]);
    node.shadows.extend(shadows);
}

/// 程序集条目 → Kitten4 的定义根积木(`zC` 的逆):形参回到 `PARAMS<j>` 输入 + `stable_parameter`,
/// 定义体回到 `statements.STACK`(正向把 `inputs.STACK` 搬去了 `statements.STACK`,这里保持)。
pub(crate) fn def_root_from_entry(
    entry: &ProcedureEntry,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) -> Result<BlockJson> {
    let mut body = entry.tree.roots.first().cloned().unwrap_or_default();
    body.kind = DEF_ROOT.to_string();
    if body.id.is_none() {
        body.id = Some(entry.id.clone());
    }
    if !body.statements.contains_key(SLOT_STACK)
        && let Some(stack) = body.inputs.remove(SLOT_STACK)
    {
        body.statements.insert(SLOT_STACK.to_string(), stack);
    }

    let mut inputs = BTreeMap::new();
    let mut mutation = format!("<mutation xmlns=\"{XHTML}\">");
    for (index, param) in entry
        .params
        .iter()
        .filter(|p| p.kind != PARAM_LABEL)
        .enumerate()
    {
        // Kitten4 的定义块形参只有 `String` 形态;`Custom`/`List` 等只能按它还原(记进报告)
        if param.kind != PARAM_STRING {
            report.warn(TranslateWarning::DroppedField {
                path: format!(
                    "procedures.{}.param.{}(type={})",
                    entry.name, param.name, param.kind
                ),
            });
        }
        let stable = BlockJson {
            kind: STABLE_PARAM.to_string(),
            id: Some(param.id.clone()),
            is_output: true,
            parent_id: body.id.clone(),
            fields: BTreeMap::from([
                (
                    FIELD_PARAM_NAME.to_string(),
                    Value::String(param.name.clone()),
                ),
                (
                    String::from("param_default_value"),
                    Value::String(String::new()),
                ),
            ]),
            ..Default::default()
        };

        let slot = format!("{PARAMS_PREFIX}{index}");
        body.shadows
            .insert(slot.clone(), math_number_shadow(&ids.short(), "0"));
        mutation.push_str(&format!("<arg name=\"{slot}\"></arg>"));
        inputs.insert(slot, stable);
    }
    mutation.push_str("</mutation>");

    body.inputs = inputs;
    body.fields = BTreeMap::from([(FIELD_NAME.to_string(), Value::String(entry.name.clone()))]);
    body.mutation = Some(mutation);
    // 正向 `zC` 注入的 `deletable/editable=false` 逆回去(Kitten4 的定义块默认可删可编辑)
    body.extra
        .insert(String::from("deletable"), Value::Bool(true));
    body.extra
        .insert(String::from("editable"), Value::Bool(true));
    body.shadows.insert(
        String::from("PROCEDURES_2_DEFNORETURN_DEFINE"),
        String::new(),
    );
    body.shadows.insert(
        String::from("PROCEDURES_2_DEFNORETURN_MUTATOR"),
        String::new(),
    );
    body.shadows.insert(SLOT_STACK.to_string(), String::new());
    Ok(body)
}

/// Kitten4 的 `default_value` 影子(调用点未连接的实参),逐字对齐真实 `.bcm4`
fn default_value_shadow(id: &str, value: &str) -> String {
    format!(
        "<shadow xmlns=\"{XHTML}\" type=\"default_value\" id=\"{id}\" visible=\"visible\"><field has_been_edited=\"false\" name=\"TEXT\">{value}</field></shadow>"
    )
}

// ---------------------------------------------------------------- 编码

/// 实体/程序集的积木树 → `nekoBlockJsonList` 数组
///
/// 编码前会补齐官方一定会写的 `shield` 键(见 [`fill_shield`])。
pub(crate) fn tree_to_json(tree: &BlockTree) -> Result<Vec<Value>> {
    tree.roots
        .iter()
        .map(|root| {
            let mut value = BlockJson::to_value(root)?;
            fill_shield(&mut value);
            Ok(value)
        })
        .collect()
}

/// 补齐 `shield` 键:官方 `jC.parseBlock` 给**每个**节点写 `shield: !!t.shield`
/// (Kitten4 源里根本没有这个键,所以恒为 `false`),而 [`BlockJson`] 的 `shield` 字段是
/// `skip_serializing_if = "is_false"` —— 不补这一下,产物里就少了官方必写的 `shield`。
/// `shield: true` 由字段自己带出来,不会被覆盖;其余布尔键(`is_shadow`/`is_output`/`disabled`)
/// 官方同样只在真值时写,与 `BlockJson` 的行为一致,不补。
fn fill_shield(node: &mut Value) {
    let Some(obj) = node.as_object_mut() else {
        return;
    };
    obj.entry("shield".to_string())
        .or_insert(Value::Bool(false));
    for slot in ["inputs", "statements"] {
        if let Some(map) = obj.get_mut(slot).and_then(Value::as_object_mut) {
            for child in map.values_mut() {
                fill_shield(child);
            }
        }
    }
    if let Some(next) = obj.get_mut("next") {
        fill_shield(next);
    }
}

/// 程序集条目 → `proceduresDict`(调用方再包一层 `{"proceduresDict": …}`)
pub(crate) fn procedures_to_json(procedures: &[ProcedureEntry]) -> Result<Map<String, Value>> {
    let mut dict = Map::new();
    for entry in procedures {
        let params = entry
            .params
            .iter()
            .map(|p| json!({ "id": p.id, "type": p.kind, "name": p.name }))
            .collect::<Vec<_>>();
        let body = json!({
            "id": entry.id,
            "name": entry.name,
            "type": entry.kind,
            "params": params,
            "nekoBlockJsonList": tree_to_json(&entry.tree)?,
        });
        dict.insert(entry.id.clone(), body);
    }
    Ok(dict)
}

// ---------------------------------------------------------------- 小工具

/// JS 真值语义下的字符串字段(`e.fields?.NAME || ""`)
fn field_text(fields: &BTreeMap<String, Value>, key: &str) -> String {
    match fields.get(key) {
        Some(Value::String(s)) if !s.is_empty() => s.clone(),
        Some(Value::Number(n)) if n.as_f64().map(|v| v != 0.0).unwrap_or(false) => n.to_string(),
        _ => String::new(),
    }
}

/// 官方 `parseInt(slot.replace("PARAMS",""), 10)`:取前缀后的十进制数字前缀,取不到记 0
fn params_index(slot: &str) -> i64 {
    slot.trim_start_matches(PARAMS_PREFIX)
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 查表索引

/// 由 `&[(键, 值)]` 表建哈希索引(`mapping.rs`/`nemo_mapping.rs` 的 `LazyLock` 索引共用)。
///
/// 一律 `entry().or_insert()`:**首个命中优先** —— 与原来的 `iter().find(...)` 逐键等价
/// (表里真有重复键时也取第一个)。表本身是 `&'static` 数据,索引只存它的拷贝。
pub(super) fn flat_index<V: Copy>(pairs: &'static [(&'static str, V)]) -> HashMap<&'static str, V> {
    let mut index = HashMap::with_capacity(pairs.len());
    for (key, value) in pairs {
        index.entry(*key).or_insert(*value);
    }
    index
}

/// 两级表 `&[(键, &[(子键, 值)])]` 的嵌套索引(外层 → 内层),两级同样**首个命中优先**
pub(super) fn nested_index<V: Copy>(
    pairs: &'static [(&'static str, &'static [(&'static str, V)])],
) -> HashMap<&'static str, HashMap<&'static str, V>> {
    let mut index: HashMap<&'static str, HashMap<&'static str, V>> =
        HashMap::with_capacity(pairs.len());
    for (key, entries) in pairs {
        let slots: &mut HashMap<&'static str, V> = index.entry(*key).or_default();
        for (slot, value) in *entries {
            slots.entry(*slot).or_insert(*value);
        }
    }
    index
}

#[cfg(test)]
mod neko_tests {
    use super::*;
    use crate::core::convert::shared::EditorType;

    use super::super::TargetEditor;

    fn ids() -> IdSource {
        IdSource::new(true)
    }

    fn report() -> TranslateReport {
        TranslateReport::new(EditorType::Kitten4, TargetEditor::KittenN)
    }

    fn node(value: Value) -> BlockJson {
        BlockJson::from_value(&value).expect("block json")
    }

    /// 一个带两个 String 形参 + 参数引用 + 链式体的程序集定义
    fn def_tree() -> BlockTree {
        BlockTree::new(vec![node(json!({
            "type": "procedures_2_defnoreturn",
            "id": "procDef",
            "location": [0, 80],
            "shield": false,
            "fields": { "NAME": "非线性移动" },
            "shadows": { "PARAMS1": "<shadow type=\"math_number\"/>", "PARAMS2": "<shadow type=\"math_number\"/>" },
            "inputs": {
                "PARAMS2": { "type": "procedures_2_stable_parameter", "id": "pSpeed", "fields": { "param_name": "Speed" } },
                "PARAMS1": { "type": "procedures_2_stable_parameter", "id": "pX", "fields": { "param_name": "X" } },
                "STACK": {
                    "type": "self_move_to", "id": "move",
                    "inputs": {
                        "x": { "type": "procedures_2_parameter", "id": "useX", "fields": { "param_name": "X" }, "is_output": true },
                        "y": { "type": "procedures_2_parameter", "id": "useUnknown", "fields": { "param_name": "不存在" }, "is_output": true }
                    }
                }
            }
        }))])
    }

    #[test]
    fn splits_defs_out_of_the_entity_tree() {
        let mut tree = def_tree();
        tree.roots.push(node(
            json!({ "type": "on_running_group_activated", "id": "hat" }),
        ));
        let mut ids = ids();
        let mut report = report();

        let (rest, procedures) = split_procedures(tree, &mut ids, &mut report);

        assert_eq!(rest.roots.len(), 1);
        assert_eq!(rest.roots[0].kind, "on_running_group_activated");
        assert_eq!(procedures.len(), 1);
        let entry = &procedures[0];
        assert_eq!(entry.kind, "NORMAL");
        assert_eq!(entry.id, "procDef");
        assert_eq!(entry.name, "非线性移动");

        // params:合成的 Label 是 UUID,两个 String 按 PARAMS 序号排(X 在 Speed 前)
        assert_eq!(entry.params.len(), 3);
        assert_eq!(entry.params[0].kind, "Label");
        assert_eq!(entry.params[0].name, "非线性移动");
        assert!(
            entry.params[0].id.contains('-'),
            "Label 形参 id 应是 UUID 形态:{:?}",
            entry.params[0].id
        );
        assert_eq!(
            entry
                .params
                .iter()
                .map(|p| (p.kind.as_str(), p.name.as_str(), p.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Label", "非线性移动", entry.params[0].id.as_str()),
                ("String", "X", "pX"),
                ("String", "Speed", "pSpeed")
            ]
        );

        // STACK 已搬进 statements,inputs 里只剩 PARAMS*
        let body = &entry.tree.roots[0];
        assert_eq!(
            body.statements.get("STACK").map(|b| b.kind.as_str()),
            Some("self_move_to")
        );
        assert_eq!(
            body.inputs.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["PARAMS1", "PARAMS2"]
        );
        // fields.NAME 换成条目 id,deletable/editable 落 extra(输出仍是顶层键)
        assert_eq!(body.fields.get("NAME"), Some(&json!("procDef")));
        assert_eq!(body.extra.get("deletable"), Some(&json!(false)));
        assert_eq!(body.extra.get("editable"), Some(&json!(false)));
        assert_eq!(
            body.mutation.as_deref(),
            Some(
                "<mutation xmlns=\"http://www.w3.org/1999/xhtml\"><arg id=\"00000000-0000-4000-8000-000000000001\" name=\"非线性移动\" type=\"Label\"></arg><arg id=\"pX\" name=\"X\" type=\"String\"></arg><arg id=\"pSpeed\" name=\"Speed\" type=\"String\"></arg></mutation>"
            )
        );

        // 参数引用补上 mutation id / is_output;名字对不上的引用只记告警、不动
        let stack = body.statements.get("STACK").expect("STACK");
        let used = stack.inputs.get("x").expect("x");
        assert_eq!(
            used.mutation.as_deref(),
            Some("<mutation xmlns=\"http://www.w3.org/1999/xhtml\" id=\"pX\"></mutation>")
        );
        assert!(used.is_output);
        let unknown = stack.inputs.get("y").expect("y");
        assert_eq!(unknown.mutation, None);
        assert_eq!(report.warnings().len(), 1);
        assert_eq!(
            report.warnings()[0],
            TranslateWarning::DroppedField {
                path: "procedures_2_defnoreturn.非线性移动.param.不存在".into()
            }
        );
    }

    #[test]
    fn return_value_def_yields_normal_and_round_entries() {
        // STACK = 一个循环体里带 VALUE 的 return_value,后面还挂一个缺 VALUE 的 return_value
        // (同时覆盖 `VC` 的"已有则留、缺则补"两条分支)
        let tree = BlockTree::new(vec![node(json!({
            "type": "procedures_2_defnoreturn",
            "id": "procDef",
            "fields": { "NAME": "取数" },
            "inputs": {
                "STACK": {
                    "type": "repeat_forever_until", "id": "loop",
                    "statements": { "DO": {
                        "type": "procedures_2_return_value", "id": "ret",
                        "inputs": { "VALUE": { "type": "math_number", "id": "num", "is_shadow": true, "fields": { "TEXT": "7" } } },
                        "shadows": { "VALUE": "<shadow type=\"math_number\"/>" }
                    } },
                    "next": { "type": "procedures_2_return_value", "id": "ret2" }
                }
            }
        }))]);
        let mut ids = ids();
        let mut report = report();

        let (_rest, procedures) = split_procedures(tree, &mut ids, &mut report);
        assert_eq!(procedures.len(), 2);
        let normal = &procedures[0];
        let round = &procedures[1];
        assert_eq!(
            (normal.kind.as_str(), normal.id.as_str()),
            ("NORMAL", "procDef")
        );
        assert_eq!(round.kind, "ROUND");
        assert_ne!(round.id, "procDef", "ROUND 条目要现铸新 id");
        assert_eq!(normal.params, round.params);
        assert_eq!(normal.params.len(), 1, "无 PARAMS* 时只剩合成 Label");

        // NORMAL:`WC` 把所有 return_value 的 VALUE 输入与影子都剥掉
        let normal_stack = normal.tree.roots[0].statements.get("STACK").expect("STACK");
        let normal_ret = normal_stack.statements.get("DO").expect("DO");
        assert!(!normal_ret.inputs.contains_key("VALUE"));
        assert_eq!(normal_ret.shadows.get("VALUE"), None);
        assert!(
            !normal_stack
                .next
                .as_deref()
                .unwrap()
                .inputs
                .contains_key("VALUE")
        );

        // ROUND:body 带 `u` 层替换(id 是新 UUID,但 `fields.NAME` 按官方仍是**原定义 id**),
        // 已有 VALUE 原样保留,缺的补默认 math_number + 影子
        let round_body = &round.tree.roots[0];
        assert_eq!(round_body.id.as_deref(), Some(round.id.as_str()));
        assert_eq!(round_body.fields.get("NAME"), Some(&json!("procDef")));
        let round_stack = round_body.statements.get("STACK").expect("STACK");
        let kept = round_stack
            .statements
            .get("DO")
            .expect("DO")
            .inputs
            .get("VALUE")
            .expect("原 VALUE");
        assert_eq!(kept.id.as_deref(), Some("num"));
        assert_eq!(
            kept.fields.get("TEXT"),
            Some(&json!("7")),
            "已有 VALUE 不被改写"
        );
        let injected = round_stack
            .next
            .as_deref()
            .unwrap()
            .inputs
            .get("VALUE")
            .expect("补出来的 VALUE");
        assert_eq!(injected.kind, "math_number");
        assert!(injected.is_shadow && injected.is_output);
        assert_eq!(injected.parent_id.as_deref(), Some("ret2"));
        assert_eq!(injected.fields.get("NUM"), Some(&json!("0")));
        assert!(injected.field_constraints.is_some());
        assert_eq!(
            round_stack
                .next
                .as_deref()
                .unwrap()
                .shadows
                .get("VALUE")
                .map(String::as_str),
            Some(VALUE_SHADOW_XML)
        );
    }

    #[test]
    fn rewrites_call_sites_and_leaves_unknown_calls_untouched() {
        let mut ids = ids();
        let mut report = report();
        let procedures = split_procedures(def_tree(), &mut ids, &mut report).1;

        let mut tree = BlockTree::new(vec![node(json!({
            "type": "procedures_2_callnoreturn",
            "id": "call1",
            "fields": { "NAME": "非线性移动" },
            "shadows": { "NAME": "<shadow type=\"text\"/>" },
            "inputs": {
                "ARG0": { "type": "math_number", "id": "a0", "fields": { "TEXT": "160" } },
                "ARG1": { "type": "math_number", "id": "a1", "fields": { "TEXT": "-260" } }
            }
        }))]);
        let unknown = node(json!({
            "type": "procedures_2_callreturn",
            "id": "call2",
            "fields": { "NAME": "不存在的程序集" },
            "mutation": "<mutation/>"
        }));
        tree.roots[0].next = Some(Box::new(unknown));

        rewrite_calls(&mut tree, &procedures, &mut ids, &mut report);

        let call = &tree.roots[0];
        assert_eq!(call.fields.get("NAME"), Some(&json!("procDef")));
        // 影子整体换成 NAME 占位 + 两个 String 形参影子(id 现铸)
        assert_eq!(call.shadows.get("NAME").map(String::as_str), Some(""));
        assert_eq!(call.shadows.len(), 3);
        assert!(call.shadows["pX"].contains("visible=\"visible\""));
        assert!(call.shadows["pX"].contains("name=\"NUM\">0</field>"));
        // ARG<i-1> 复制到形参槽(老槽位保留,官方是复制不是搬移)
        assert_eq!(
            call.inputs.get("ARG0").map(|b| b.id.as_deref()),
            Some(Some("a0"))
        );
        assert_eq!(
            call.inputs.get("pX").map(|b| b.id.as_deref()),
            Some(Some("a0"))
        );
        assert_eq!(
            call.inputs.get("pSpeed").map(|b| b.id.as_deref()),
            Some(Some("a1"))
        );
        assert!(
            !call.inputs.contains_key("ARG2"),
            "ARG<i-1> 只映射到存在的槽位"
        );
        assert_eq!(
            call.mutation.as_deref(),
            Some(concat!(
                "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" def_id=\"procDef\" name=\"procDef\" type=\"NORMAL\">",
                "<arg id=\"00000000-0000-4000-8000-000000000001\" content=\"非线性移动\" type=\"Label\"></arg>",
                "<arg id=\"pX\" content=\"X\" type=\"String\"></arg>",
                "<arg id=\"pSpeed\" content=\"Speed\" type=\"String\"></arg></mutation>"
            ))
        );

        // 找不到同名程序集 → 原样保留
        let untouched = tree.roots[0].next.as_deref().expect("next");
        assert_eq!(untouched.fields.get("NAME"), Some(&json!("不存在的程序集")));
        assert_eq!(untouched.mutation.as_deref(), Some("<mutation/>"));
    }

    #[test]
    fn encodes_trees_and_procedure_dict() {
        let mut ids = ids();
        let mut report = report();
        let (rest, procedures) = split_procedures(def_tree(), &mut ids, &mut report);

        let json_tree = tree_to_json(&rest).expect("rest 树");
        assert!(json_tree.is_empty(), "定义被摘走后实体树为空");

        let dict = procedures_to_json(&procedures).expect("条目编码");
        assert_eq!(
            dict.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["procDef"]
        );
        let entry = dict.get("procDef").expect("条目");
        assert_eq!(entry["id"], json!("procDef"));
        assert_eq!(entry["name"], json!("非线性移动"));
        assert_eq!(entry["type"], json!("NORMAL"));
        assert_eq!(entry["params"][0]["type"], json!("Label"));
        assert_eq!(
            entry["params"][1],
            json!({ "id": "pX", "name": "X", "type": "String" })
        );
        assert_eq!(entry["nekoBlockJsonList"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["type"],
            json!("procedures_2_defnoreturn")
        );
        assert_eq!(entry["nekoBlockJsonList"][0]["deletable"], json!(false));
        assert_eq!(entry["nekoBlockJsonList"][0]["editable"], json!(false));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["fields"]["NAME"],
            json!("procDef")
        );
        // 官方给每个节点写 `shield`(源里没这个键时是 false),编码端要补齐
        assert_eq!(entry["nekoBlockJsonList"][0]["shield"], json!(false));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["statements"]["STACK"]["inputs"]["x"]["shield"],
            json!(false),
            "嵌套节点同样补"
        );
        assert_eq!(
            entry["nekoBlockJsonList"][0]["statements"]["STACK"]["type"],
            json!("self_move_to")
        );
        // 往返:编码产物能再解析回节点(保真)
        let reparsed = BlockJson::from_value(&entry["nekoBlockJsonList"][0]).expect("往返");
        assert_eq!(reparsed, procedures[0].tree.roots[0]);
    }

    #[test]
    fn params_index_follows_parse_int_prefix() {
        assert_eq!(params_index("PARAMS0"), 0);
        assert_eq!(params_index("PARAMS12"), 12);
        assert_eq!(params_index("PARAMS07"), 7);
        assert_eq!(params_index("PARAMS"), 0, "取不到数字按 0 记(官方是 NaN)");
        assert_eq!(params_index("PARAMSX"), 0);
    }
}
