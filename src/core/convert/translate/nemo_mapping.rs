//! NEMO → KN 的语义映射(官方 `nemoBcmToNekoBcmUtils` 内层类 `hI` 的忠实移植)。
//!
//! 官方 bundle:`temp/web/main-vendors.9b801394.js` 模块 41888,类 `hI` 位于偏移
//! ~5960458–5999640;表(`sI` 类型映射、`getMappedName` 内联槽位表、`specialFieldValueMap`、
//! `SHADOW_FIELD_NAME_MAP`、`oI` 占位标题)转录在 [`super::tables_gen_nemo`]。
//! 研究结论见 `docs/27-nemo-to-kn-conversion-plan.md` §9。
//!
//! ## 为什么"前端"和"映射"在同一个模块里
//!
//! Kitten 侧是"前端(`kitten::parse_block_data_json` 出纯图)→ 映射(`mapping::translate_kitten_to_kn`
//! 改语义)"两段;NEMO 侧官方**没有**这段分层:一趟 DOM 遍历里边走边查表改名/改槽/合成默认值/降级占位
//! —— 槽位名要看子积木的**映射后**类型、影子字段名要用**映射后**类型去查 `SHADOW_FIELD_NAME_MAP`,
//! 拆成两段就必然把同一个状态机复制一遍。所以本模块就是 NEMO 的"前端 + 映射"一体件,入口是
//! [`translate_nemo_to_kn`](与 Kitten 侧 `mapping::translate_kitten_to_kn` 同层同义):输入是
//! **已解析、已过前置改写**的积木 XML 节点(解析/序列化在 [`super::nemo_xml`]),输出 KN 的
//! [`BlockTree`];文档级管线(骨架、版本迁移、资源 url、变量/舞台归一)在 [`super::nemo`]。
//!
//! ## 语义要点(对应 docs/27 §9.2/§9.3)
//!
//! - **槽位覆盖语义**:`<value name="A"><shadow …/><block …/></value>` → `inputs["A"]` = 那个块、
//!   `shadows["A"]` = 影子**重新序列化**的 `<shadow …/>`。`<empty>` 与 `<shadow>` 走不同分支(官方如此):
//!   前者 `inputs["A"]` 是 `logic_empty` 节点 + `shadows["A"]` 是 `<empty …/>`;后者 `inputs["A"]` 是影子
//!   **实体化**出来的输入节点 + `shadows["A"]` 是 `<shadow …/>`。
//! - **取值驱动的类型**(不能平铺成"类型→类型"表):`appearance_of_sprite`/`coordinate_of_sprite` 按
//!   `attribute` 取值 0/1/2/3/5 拆成 `coordinate_of_sprite`/`style_of_sprite`/`appearance_of_sprite`;
//!   `self_stress_animation` 按 `appear` 取值拆出 `self_appear_animation`;解析**程序集**时没有
//!   `currentActor`,`get_styles` 的字段名退成 `NUM`、影子类型退成 `math_number`。
//! - **KN 侧合成默认值**:见 [`Mapper::handle_special_block_types`]、`parse_fields` 里的
//!   `mouse_down.sprite` / `add_width_height_scale.increase` / `self_change_effect.increase`、
//!   [`Mapper::handle_broadcast_field`]、[`update_block_with_param`]。
//! - **未映射块 → 占位积木**(与官方一致):手机传感器/硬件/AI 等类型在 `sI` 里就被映射成
//!   `bcm_translator_text_*`,mutation 里带中文标题([`NEMO_MUTATION_TEXT`]),`disabled = true`;
//!   同时记一条 [`TranslateWarning::DegradedToText`]。
//! - **id**:官方 `replaceIdsWithUUID` 先把 `block`/`shadow`/`statement`/`value` 的 id 全重铸成随机
//!   uuid(`empty` **不**重铸),解析期再按需铸新 id;我们统一走 [`IdSource`]
//!   (`deterministic_ids` 下产物稳定,便于回归)。
//!
//! ## 与官方的已知偏差(逐条:都在报告或注释里可见)
//!
//! - 官方 `procedures_2_return_value` 的 `VALUE` 影子把 `cI`(**函数本身**,漏了调用)拼进 `id`,
//!   产物里会是一段函数源码;这里改铸真 id(官方 bug,不复制)。
//! - 官方在 `<block type="mobile__text">` 上用**字符串替换**插 `<mutation items="1">`;对自闭合写法
//!   会插成兄弟节点(随后被空类型过滤丢掉)。这里按"插成第一个子节点"实现,自闭合写法下行为不同
//!   (真实作品里 `mobile__text` 不会自闭合)。
//! - `fields` 值在官方是 `undefined` 时,`JSON.stringify` 会丢掉该键(`get_styles` 造型下标越界、
//!   非数字时就会这样);这里同样**丢弃该键**,不落 `null`。

use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use serde_json::{Map, Value, json};

use super::TranslateReport;
use super::TranslateWarning;
use super::model::{BlockJson, BlockTree, IdSource};
use super::nemo_xml::XmlNode;
use super::tables_gen_nemo::{
    NEMO_INPUT_NAME_MAP, NEMO_MUTATION_TEXT, NEMO_SHADOW_FIELD_NAMES, NEMO_SPECIAL_FIELD_VALUES,
    NEMO_TO_KN, NemoMutationText,
};

/// 官方 `lI`:影子 / 变异 XML 的命名空间
pub(crate) const XHTML: &str = "http://www.w3.org/1999/xhtml";

/// 程序集类型:普通(官方 `iI.NORMAL`)
pub(crate) const PROCEDURE_NORMAL: &str = "NORMAL";
/// 程序集类型:带返回值(官方 `iI.ROUND`)
pub(crate) const PROCEDURE_ROUND: &str = "ROUND";

// ---------------------------------------------------------------------------
// 解析上下文(官方 `hI` 实例上的可变状态)
// ---------------------------------------------------------------------------

/// 一个演员/场景:官方 `parseBlocksXML(xml, 实体对象)` 把实体对象挂到 `currentActor`,
/// 解析期只用到 `id` 与 `styles`。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct NemoEntity {
    pub id: String,
    /// 实体拥有的造型 id 数组;`None` 对应 JS 的 `undefined`(缺失)——与"空数组"语义不同
    /// (`[]` 在 JS 里是 truthy,会走"按 1 基下标取造型"的失败分支)。
    pub styles: Option<Vec<String>>,
}

/// 程序集形参(官方 `createParams` 的 `{id,type,name,parent_id,parent_type}`)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NemoParam {
    pub id: String,
    pub name: String,
    /// `Label`(程序集名本身)或 `String`(带实参的形参)
    pub kind: String,
    pub parent_id: String,
    /// 所属程序集的类型(`NORMAL` / `ROUND`,或源文档里的原值)
    pub parent_type: String,
}

/// 一个程序集条目(官方 `proceduresDict` 的 value)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NemoProcedure {
    pub id: String,
    pub name: String,
    /// `NORMAL` / `ROUND`
    pub kind: String,
    pub params: Vec<NemoParam>,
}

/// `translate_nemo_to_kn` 的第二参:官方 `parseBlocksXML(xml, t)` 里的 `t`
///
/// 官方靠 `t[0].parent_type` 是不是 `NORMAL`/`ROUND` 区分"形参数组"与"实体对象"
/// (实体对象下 `t[0]` 是 `undefined`)。
pub(crate) enum NemoSubject<'a> {
    /// 演员或场景
    Entity(&'a NemoEntity),
    /// 程序集形参(第一个元素是 `Label`)
    Params(&'a [NemoParam]),
}

/// 解析上下文:官方 `hI` 实例上**跨调用存活**的状态(同一实例依次解析程序集 → 演员 → 场景)。
///
/// `current_actor` / `current_params` 官方**只置不清**,解析演员时 `current_params` 仍指着最后一个
/// 程序集的形参;这里照抄(只有 `procedures_2_*` 分支会读到,真实作品里不会在演员里出现)。
#[derive(Debug, Default)]
pub(crate) struct NemoParseContext {
    /// 程序集字典(键 = [`NemoProcedure::id`])
    pub procedures: BTreeMap<String, NemoProcedure>,
    /// 广播 id → 名称(官方 `nemoBcm.broadcast.broadcast_dict` 的 `name`)
    pub broadcast_names: BTreeMap<String, String>,
    /// `broadcast_dict` 是否非空:官方 `getBroadcastMessage` 靠它决定"查不到给 `?`"还是"原样返回"
    pub has_broadcasts: bool,
    pub current_actor: Option<NemoEntity>,
    pub current_params: Option<Vec<NemoParam>>,
}

// ---------------------------------------------------------------------------
// 查表索引(与 `mapping.rs` 同口径:线性 `iter().find` → `LazyLock<HashMap>`,首个命中优先)
// ---------------------------------------------------------------------------

static NEMO_TO_KN_INDEX: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::with_capacity(NEMO_TO_KN.len());
    for (nemo, kn) in NEMO_TO_KN {
        map.entry(*nemo).or_insert(*kn);
    }
    map
});

static INPUT_NAME_MAP_INDEX: LazyLock<HashMap<&'static str, HashMap<&'static str, &'static str>>> =
    LazyLock::new(|| {
        let mut map: HashMap<&'static str, HashMap<&'static str, &'static str>> =
            HashMap::with_capacity(NEMO_INPUT_NAME_MAP.len());
        for (kind, slots) in NEMO_INPUT_NAME_MAP {
            let entry = map.entry(*kind).or_default();
            for (slot, mapped) in *slots {
                entry.entry(*slot).or_insert(*mapped);
            }
        }
        map
    });

static SPECIAL_FIELD_VALUES_INDEX: LazyLock<
    HashMap<&'static str, HashMap<&'static str, &'static str>>,
> = LazyLock::new(|| {
    let mut map: HashMap<&'static str, HashMap<&'static str, &'static str>> =
        HashMap::with_capacity(NEMO_SPECIAL_FIELD_VALUES.len());
    for (field, entries) in NEMO_SPECIAL_FIELD_VALUES {
        let entry = map.entry(*field).or_default();
        for (text, mapped) in *entries {
            entry.entry(*text).or_insert(*mapped);
        }
    }
    map
});

static SHADOW_FIELD_NAMES_INDEX: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::with_capacity(NEMO_SHADOW_FIELD_NAMES.len());
    for (kind, field) in NEMO_SHADOW_FIELD_NAMES {
        map.entry(*kind).or_insert(*field);
    }
    map
});

static MUTATION_TEXT_INDEX: LazyLock<HashMap<&'static str, &'static NemoMutationText>> =
    LazyLock::new(|| {
        let mut map = HashMap::with_capacity(NEMO_MUTATION_TEXT.len());
        for (kind, text) in NEMO_MUTATION_TEXT {
            map.entry(*kind).or_insert(text);
        }
        map
    });

/// 官方 `mapType`(`sI[e]||e`):表里没有就返回原值
pub(crate) fn map_type(raw: &str) -> &str {
    NEMO_TO_KN_INDEX.get(raw).copied().unwrap_or(raw)
}

// ---------------------------------------------------------------------------
// 官方谓词表(isXxxBlock)
// ---------------------------------------------------------------------------

/// 官方 `isLogicCompareBlock`:这些类型的覆盖块走 [`Mapper::parse_logic_compare`]
/// (即它自己的 `<value>` 子槽要展开成 `inputs`/`shadows`)。
#[rustfmt::skip]
const LOGIC_COMPARE_BLOCKS: &[&str] = &[
    "logic_compare", "logic_operation", "math_number_property", "math_modulo", "divisible_by",
    "math_arithmetic", "random_num", "text_select", "text_length", "math_round", "list_index_of",
    "math_trig", "math_function", "get_orientation", "get_mouse_info", "get_stage_info",
    "logic_negate", "text_contain", "list_is_exist", "list_length", "list_item",
    "get_clone_index_property",
];

/// 官方 `isMultiParameterBlock`:变异里要写 `items="<槽位数>"`
#[rustfmt::skip]
const MULTI_PARAMETER_BLOCKS: &[&str] = &["text_join", "text_split"];

/// 官方 `isSpecialBlockType`:影子**实体化**成输入节点时,这些类型保留自身类型且 `is_shadow=false`
#[rustfmt::skip]
const SPECIAL_BLOCK_TYPES: &[&str] = &[
    "get_answer", "get_choice_and_index", "variables_get", "list_length", "list_item",
    "get_voice_volume", "get_time", "microbit_temperature", "microbit_light_level",
    "microbit_sound_level", "microbit_magnetometer", "microbit_accelerometer", "microbit_math_map",
    "microbit_pin_analog_read", "microbit_compass_heading", "microbit_rotation",
    "microbit_get_volume", "microbit_pin_digital_read", "distance_to", "random_num",
    "set_sprite_style", "self_listen", "appearance_of_sprite", "timer", "list_get", "math_single",
    "text_length", "math_round", "text_join", "text_split", "list_index_of",
    "coordinate_of_sprite", "bcm_translator_text_return_value_block",
    "bcm_translator_text_return_boolean_block", "get_current_clone_index",
    "get_clone_index_property", "get_clone_num", "out_of_boundary", "logic_operation",
    "logic_boolean", "bump_into_color", "bump_into", "user_id_get", "username_get",
];

/// KN 的四种"文本占位积木"(降级产物)
#[rustfmt::skip]
const TEXT_PLACEHOLDERS: &[&str] = &[
    "bcm_translator_text_execution_block",
    "bcm_translator_text_event_block",
    "bcm_translator_text_return_value_block",
    "bcm_translator_text_return_boolean_block",
];

pub(crate) fn is_logic_compare_block(kind: &str) -> bool {
    LOGIC_COMPARE_BLOCKS.contains(&kind)
}

pub(crate) fn is_multi_parameter_block(kind: &str) -> bool {
    MULTI_PARAMETER_BLOCKS.contains(&kind)
}

pub(crate) fn is_procedure_block(kind: &str) -> bool {
    kind == "procedures_2_callreturn" || kind == "procedures_2_parameter"
}

fn is_special_block_type(kind: &str) -> bool {
    SPECIAL_BLOCK_TYPES.contains(&kind)
}

fn is_text_placeholder(kind: &str) -> bool {
    TEXT_PLACEHOLDERS.contains(&kind)
}

fn is_procedure_call_block(kind: &str) -> bool {
    kind == "procedures_2_callnoreturn" || kind == "procedures_2_callreturn"
}

/// 官方 `getMappedName` 的"槽名全大写"规则:`split_text` / `text_to_split` / `^(add|if|choice)\d+$`
fn uppercase_slot(slot: &str) -> bool {
    if slot == "split_text" || slot == "text_to_split" {
        return true;
    }
    ["add", "if", "choice"].iter().any(|prefix| {
        slot.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// 官方 `getAFieldName`
fn a_field_name(kind: &str) -> &'static str {
    if kind == "math_modulo" {
        return "divisor";
    }
    if [
        "random_num",
        "divisible_by",
        "math_arithmetic",
        "math_arithmetic_power",
        "logic_compare",
        "text_contain",
        "text_char_at",
    ]
    .contains(&kind)
    {
        return "A";
    }
    "a"
}

/// 官方 `getBFieldName`
fn b_field_name(kind: &str) -> &'static str {
    if kind == "math_modulo" {
        return "dividend";
    }
    if [
        "random_num",
        "divisible_by",
        "math_arithmetic",
        "math_arithmetic_power",
        "logic_compare",
        "text_contain",
    ]
    .contains(&kind)
    {
        return "B";
    }
    "b"
}

/// 官方 `getVarFieldName`
fn var_field_name(kind: &str) -> &'static str {
    if ["show_hide_list", "pure_list_get", "list_get"].contains(&kind) {
        "list"
    } else {
        "variable"
    }
}

/// 官方 `getOpFieldName`
fn op_field_name(kind: &str) -> &'static str {
    if [
        "math_trig_common",
        "math_trig_arc",
        "math_trig",
        "math_arithmetic",
        "math_arithmetic_power",
        "logic_operation",
        "math_round",
        "math_function",
    ]
    .contains(&kind)
    {
        return "type";
    }
    if kind == "logic_compare" {
        return "OP";
    }
    "time"
}

/// 官方 `getSpecialFieldNameMap(knType)[lower]`
///
/// 原表有 9 条**按类型分流**的条目(值不是常量),所以写成函数而不是常量表。
fn special_field_name(kind: &str, lower: &str) -> Option<&'static str> {
    match lower {
        "beats" => Some(if kind == "microbit_beats_shadow" {
            "NUM"
        } else {
            "beats"
        }),
        "note" => Some(if kind == "microbit_beats_shadow" {
            "NUM"
        } else {
            "note"
        }),
        "func" => Some("show_hide"),
        "method" => Some(if kind == "show_hide_variables" {
            "show_hide"
        } else {
            "method"
        }),
        "sprite1" => Some("sprite"),
        "sprite2" => Some("sprite1"),
        "visible_status" => Some("showHide"),
        "timer_action" => Some("type"),
        "info" => Some("type"),
        "position" => Some("type"),
        "axis" => Some("target"),
        "type" => Some(
            if kind == "replace_list_item" || kind == "delete_list_item" || kind == "list_item" {
                "item"
            } else {
                "type"
            },
        ),
        "audio" => Some("audio_id"),
        "style_change_direction" => Some("prev_next"),
        "property" => Some("type"),
        "target" => Some(if kind == "self_move_specify" {
            "sprite"
        } else {
            "target"
        }),
        "valname" => Some("variable"),
        "action" => Some(if kind == "self_stress_animation" {
            "animation"
        } else {
            "action"
        }),
        "index" => Some(if kind == "get_screens" {
            "screen_id"
        } else {
            "index"
        }),
        "transition" => Some(if kind == "set_screen_transition" {
            "type"
        } else {
            "transition"
        }),
        "align" => Some(if kind == "stamp" { "align" } else { "ALIGN" }),
        // `VAR` 那条在官方表里,但 `getProcessedFieldName` 的 `var` 分支永远先命中;
        // `list_get`/`pure_list_get` 由 `var_field_name` 给出 `list`,这里不再重复。
        "var" => Some("VAR"),
        _ => None,
    }
}

/// 官方 `getMappedName(knType, lowerSlot)`:槽位改名
fn mapped_slot_name(kind: &str, lower_slot: &str) -> String {
    if uppercase_slot(lower_slot) {
        return lower_slot.to_uppercase();
    }
    if let Some(mapped) = INPUT_NAME_MAP_INDEX
        .get(kind)
        .and_then(|slots| slots.get(lower_slot))
    {
        return (*mapped).to_string();
    }
    match lower_slot {
        "n" | "val" => "value".to_string(),
        "a" => a_field_name(kind).to_string(),
        "b" => b_field_name(kind).to_string(),
        other => other.to_string(),
    }
}

/// 官方 `getProcessedFieldName`
fn processed_field_name(raw: &str, kind: &str) -> String {
    let lower = raw.to_lowercase();
    if lower == "var" {
        return var_field_name(kind).to_string();
    }
    if lower == "op" {
        return op_field_name(kind).to_string();
    }
    if lower == "options" && kind == "set_top_bottom_layer" {
        return "layer".to_string();
    }
    if lower == "index" && kind == "get_styles" {
        return "style_id".to_string();
    }
    if lower == "mouse_event_type" && kind == "mouse_down" {
        return "type".to_string();
    }
    if (raw == "NAME" && kind == "procedures_2_parameter")
        || (lower == "text" && kind == "math_number")
    {
        return "param_name".to_string();
    }
    if lower == "type" && kind == "self_stress_animation" {
        return "appear".to_string();
    }
    special_field_name(kind, &lower)
        .map(str::to_string)
        .unwrap_or_else(|| raw.to_string())
}

/// 官方 `processAppearanceAttribute`
fn process_appearance_attribute(value: &str) -> String {
    match value {
        "0" => "x",
        "1" => "y",
        "2" => "style_of_sprite",
        "3" => "direction",
        "5" => "scale",
        _ => value,
    }
    .to_string()
}

/// 官方 `processCloneIndexAttribute`
fn process_clone_index_attribute(value: &str) -> String {
    match value {
        "0" => "x",
        "1" => "y",
        "2" => "style_index",
        "3" => "direction",
        "5" => "scale",
        _ => value,
    }
    .to_string()
}

/// 官方 `transformShadowFieldName`
fn shadow_field_name<'a>(kind: &str, field: &'a str) -> &'a str {
    if let Some(mapped) = SHADOW_FIELD_NAMES_INDEX.get(kind) {
        return mapped;
    }
    for (needle, mapped) in NEMO_SHADOW_FIELD_NAMES {
        if field.contains(needle) {
            return mapped;
        }
    }
    field
}

// ---------------------------------------------------------------------------
// XML 小工具(与官方 DOM 用法一一对应)
// ---------------------------------------------------------------------------

/// 官方 `e.getAttribute("x")||"0"` + `parseInt(…, 10)`;解析不出数字 → `null`(JS 的 `NaN`)
fn parse_int(node: &XmlNode, name: &str) -> Value {
    match parse_int_prefix(node.attr(name).unwrap_or("0")) {
        Some(value) => Value::from(value),
        None => Value::Null,
    }
}

/// `parseInt(s, 10)` 的前缀解析(允许前导空白、正负号)
fn parse_int_prefix(text: &str) -> Option<i64> {
    let trimmed = text.trim_start();
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|value| value * sign)
}

/// 官方 `getArgIndex`(`/arg(\d+)/`)
fn arg_index(slot: &str) -> Option<usize> {
    let at = slot.find("arg")?;
    let rest = &slot[at + 3..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    rest[..end].parse::<usize>().ok()
}

/// 官方 `a[Number(t)-1]`(1 基下标;越界 / 非数字 → `undefined`)
fn style_at(styles: &[String], value: &str) -> Option<String> {
    let index = value.trim().parse::<i64>().ok()?;
    styles.get(usize::try_from(index - 1).ok()?).cloned()
}

/// 官方 `e.querySelector(':scope field[name="x"]')`
fn field_text(el: &XmlNode, name: &str) -> Option<String> {
    el.children_of("field")
        .find(|field| field.attr("name") == Some(name))
        .map(XmlNode::text_content)
}

/// 官方 `Array.from(n.querySelectorAll(tag))`(后代,文档序)
fn collect_descendants<'a>(node: &'a XmlNode, tag: &str) -> Vec<&'a XmlNode> {
    let mut out = Vec::new();
    let mut stack: Vec<&XmlNode> = node.children.iter().rev().collect();
    while let Some(current) = stack.pop() {
        if current.tag == tag {
            out.push(current);
        }
        for child in current.children.iter().rev() {
            stack.push(child);
        }
    }
    out
}

fn select_text(map: &[(&str, &str)], key: &str) -> String {
    map.iter()
        .find(|(selector, _)| *selector == key)
        .map(|(_, text)| (*text).to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 一份积木 XML(已解析 + 已过前置改写)→ KN 的 [`BlockTree`]
///
/// `roots` 来自 [`super::nemo_xml::parse_fragment`](`<root>` 包装的直接子元素);
/// `subject` 决定 `currentActor` / `currentProcedure`(官方 `parseBlocksXML` 的第二参)。
pub(crate) fn translate_nemo_to_kn(
    roots: &[XmlNode],
    subject: NemoSubject<'_>,
    ctx: &mut NemoParseContext,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) -> BlockTree {
    let mut mapper = Mapper {
        ctx,
        ids,
        report,
    };
    mapper.select_subject(subject);
    let mut out = Vec::with_capacity(roots.len());
    for root in roots {
        let node = mapper.parse_block(root);
        // 官方 `.filter(e => "" !== e.type)`:没有 `type` 属性的裸节点(例如退化成兄弟的
        // `<mutation>`)产出空类型积木,官方直接丢掉。
        if !node.kind.is_empty() {
            out.push(node);
        }
    }
    BlockTree::new(out)
}

/// 一个已解析的程序集条目(官方 `proceduresDict` 的一条)
#[derive(Debug, Clone)]
pub(crate) struct ParsedProcedure {
    /// `proceduresDict` 的键(与官方一致:骨架键 = 源字典键 / 副本键 = 新铸 id)
    pub key: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub params: Vec<NemoParam>,
    pub tree: BlockTree,
}

/// `nemo_parse_procedures` 内部的源条目(官方 `procedure_dict` 的一条 + 展开出来的副本)
#[derive(Debug, Clone)]
struct SourceProcedure {
    /// `proceduresDict` 的键(骨架键 = 源字典键;副本键 = 新铸 id)
    key: String,
    /// 条目自己的 `id`(官方 `a.id`;源条目缺 `id` 时退成键)
    id: String,
    name: String,
    kind: String,
    blocks_xml: Option<String>,
    /// `params` 里的形参名(官方 `e.params.forEach`)
    param_names: Vec<String>,
}

/// 官方 `parseProcedures`:`procedure_dict` → 程序集条目列表(含返回值副本)
///
/// 官方顺序:**先**建骨架(只带 id/name/type/params)并挂到 `this.proceduresDict`(解析期查表用它),
/// **再**逐个解析 `blocksXML`(此刻 `proceduresDict` 已完整),最后整体替换。
pub(crate) fn nemo_parse_procedures(
    source: &Value,
    ctx: &mut NemoParseContext,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) -> Vec<ParsedProcedure> {
    let Some(dict) = source.as_object() else {
        return Vec::new();
    };

    // ① 源字典顺序收集(官方 `Object.entries(procedure_dict)`)
    let mut sources: Vec<SourceProcedure> = Vec::new();
    for (key, value) in dict {
        let Some(entry) = value.as_object() else {
            continue;
        };
        sources.push(SourceProcedure {
            key: key.clone(),
            id: entry
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_string(),
            name: entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            // 官方 `n[i] = {…, type: a.type || iI.NORMAL}`:源条目没有 `type` 时**就是** NORMAL,
            // 而 `findMatchingProcedure(name, kind)` 拿 NORMAL 去比 —— 所以这里必须归一,
            // 否则程序集调用点找不到定义(NAME 不会换成程序集 id、mutation 不会合成)。
            kind: match entry.get("type").and_then(Value::as_str) {
                Some(kind) if !kind.is_empty() => kind.to_string(),
                _ => PROCEDURE_NORMAL.to_string(),
            },
            blocks_xml: entry
                .get("blocksXML")
                .and_then(Value::as_str)
                .map(str::to_string),
            param_names: entry
                .get("params")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }

    // ② 官方 `preprocessProcedures`:带返回积木的条目额外挂一份 `type=ROUND` 副本(新铸 id,
    //    **原条目保留**),排在原条目前面(官方 `Object.values(e).forEach` 先做这一步)。
    let mut expanded: Vec<SourceProcedure> = Vec::new();
    for entry in &sources {
        if entry.blocks_xml.as_deref().is_some_and(has_return_blocks) {
            let round_id = ids.uuid();
            expanded.push(SourceProcedure {
                key: round_id.clone(),
                id: round_id,
                name: entry.name.clone(),
                kind: PROCEDURE_ROUND.to_string(),
                blocks_xml: entry.blocks_xml.clone(),
                param_names: entry.param_names.clone(),
            });
        }
        expanded.push(entry.clone());
    }

    // ③ 骨架 + 形参铸造(官方 `createParams`),同时把骨架挂进 `ctx.procedures`(解析期查表要用)
    let mut skeleton: BTreeMap<String, NemoProcedure> = BTreeMap::new();
    let mut plan: Vec<(SourceProcedure, Vec<NemoParam>)> = Vec::new();
    for entry in expanded {
        let params = create_params(&entry.name, &entry.kind, &entry.id, &entry.param_names, ids);
        skeleton.insert(
            entry.key.clone(),
            NemoProcedure {
                id: entry.id.clone(),
                name: entry.name.clone(),
                kind: entry.kind.clone(),
                params: params.clone(),
            },
        );
        plan.push((entry, params));
    }
    ctx.procedures = skeleton;

    // ④ 逐个解析 `blocksXML`(官方 `_h(r, …)`)
    let mut out = Vec::with_capacity(plan.len());
    for (entry, params) in plan {
        let mut tree = match &entry.blocks_xml {
            Some(xml) => match super::nemo_xml::parse_fragment(&format!("<root>{xml}</root>")) {
                Ok(roots) => {
                    report.blocks_total += super::nemo::count_source_elements(&roots);
                    translate_nemo_to_kn(&roots, NemoSubject::Params(&params), ctx, ids, report)
                }
                Err(error) => {
                    // 官方这里会得到 `parsererror` 文档并静默产出空列表;我们记一条报告后跳过
                    // 本体(id/name/params 仍保留),不丢整份作品。
                    report.warn(TranslateWarning::DroppedField {
                        path: format!("procedures.{}.blocksXML: {error}", entry.key),
                    });
                    BlockTree::default()
                }
            },
            None => BlockTree::default(),
        };
        update_neko_block_json_list(&mut tree, &params, &entry.id);
        out.push(ParsedProcedure {
            key: entry.key,
            id: entry.id,
            name: entry.name,
            kind: entry.kind,
            params,
            tree,
        });
    }
    out
}

fn has_return_blocks(xml: &str) -> bool {
    super::nemo_xml::parse_fragment(&format!("<root>{xml}</root>"))
        .map(|roots| contains_return_block(&roots))
        .unwrap_or(false)
}

fn contains_return_block(nodes: &[XmlNode]) -> bool {
    nodes.iter().any(|node| {
        (node.tag == "block" && node.attr("type") == Some("procedures_2_return_value"))
            || contains_return_block(&node.children)
    })
}

fn create_params(
    name: &str,
    kind: &str,
    procedure_id: &str,
    param_names: &[String],
    ids: &mut IdSource,
) -> Vec<NemoParam> {
    let parent_type = if kind.is_empty() {
        PROCEDURE_NORMAL.to_string()
    } else {
        kind.to_string()
    };
    let mut params = vec![NemoParam {
        id: ids.uuid(),
        name: name.to_string(),
        kind: "Label".to_string(),
        parent_id: procedure_id.to_string(),
        parent_type: parent_type.clone(),
    }];
    for param in param_names {
        params.push(NemoParam {
            id: ids.uuid(),
            name: param.clone(),
            kind: "String".to_string(),
            parent_id: procedure_id.to_string(),
            parent_type: parent_type.clone(),
        });
    }
    params
}

fn update_neko_block_json_list(tree: &mut BlockTree, params: &[NemoParam], procedure_id: &str) {
    for root in &mut tree.roots {
        if root.kind != "procedures_2_defnoreturn" {
            continue;
        }
        root.shadows.clear();
        root.shadows
            .insert("PROCEDURES_2_DEFNORETURN_DEFINE".to_string(), String::new());
        root.shadows.insert("PARAMS0".to_string(), String::new());
        root.shadows.insert("STACK".to_string(), String::new());
        let name = root
            .fields
            .get("NAME")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(name) = name
            && params.iter().any(|param| param.name == name) {
                update_block_with_param(root, params, procedure_id);
            }
        root.id = Some(procedure_id.to_string());
    }
}

fn update_block_with_param(block: &mut BlockJson, params: &[NemoParam], procedure_id: &str) {
    let mut mutation = format!("<mutation xmlns=\"{XHTML}\">");
    for (index, param) in params.iter().enumerate() {
        let arg_name = param.name.replace(' ', "_");
        mutation.push_str(&format!(
            "<arg id=\"{}\" name=\"{}\" type=\"{}\"></arg>",
            param.id, arg_name, param.kind
        ));
        if index > 0 {
            block.shadows.insert(
                format!("PARAMS{index}"),
                format!(
                    "<shadow xmlns=\"{XHTML}\" type=\"{}\"><field name=\"VALUE\">0</field></shadow>",
                    param.kind
                ),
            );
            let mut fields: BTreeMap<String, Value> = BTreeMap::new();
            fields.insert(
                "param_name".to_string(),
                Value::String(param.name.clone()),
            );
            fields.insert(
                "param_default_value".to_string(),
                Value::String(String::new()),
            );
            block.inputs.insert(
                format!("PARAMS{index}"),
                BlockJson {
                    kind: "procedures_2_stable_parameter".to_string(),
                    id: Some(param.id.clone()),
                    fields,
                    is_output: true,
                    parent_id: Some(procedure_id.to_string()),
                    ..BlockJson::default()
                },
            );
        }
    }
    mutation.push_str("</mutation>");
    block.mutation = Some(mutation);
    block
        .fields
        .insert("NAME".to_string(), Value::String(procedure_id.to_string()));
}

// ---------------------------------------------------------------------------
// 主映射器
// ---------------------------------------------------------------------------

/// 官方 `parseBlock` 的递归状态机
struct Mapper<'a> {
    ctx: &'a mut NemoParseContext,
    ids: &'a mut IdSource,
    report: &'a mut TranslateReport,
}

/// 一个 `<value>` 槽位的产出(官方 `handleValueElement` 家族的返回形状)
#[derive(Default)]
struct ValueResult {
    inputs: BTreeMap<String, BlockJson>,
    shadows: BTreeMap<String, String>,
}

impl Mapper<'_> {
    /// 官方 `parseBlocksXML` 开头:决定这一趟是"实体"还是"程序集形参"
    fn select_subject(&mut self, subject: NemoSubject<'_>) {
        match subject {
            NemoSubject::Entity(entity) => {
                self.ctx.current_actor = Some(entity.clone());
            }
            NemoSubject::Params(params) => {
                let first_ok = params
                    .first()
                    .is_some_and(|param| {
                        param.parent_type == PROCEDURE_NORMAL || param.parent_type == PROCEDURE_ROUND
                    });
                if first_ok {
                    self.ctx.current_params = Some(params.to_vec());
                } else {
                    // 官方怪癖:非 NORMAL/ROUND(`HEXAGONAL` 或源文档原值)会把它当实体用;
                    // 保持同一行为 —— 只置 `current_actor`,于是 `styles` 缺失。
                    self.ctx.current_actor = Some(NemoEntity::default());
                }
            }
        }
    }

    fn uuid(&mut self) -> String {
        self.ids.uuid()
    }

    fn note_degraded(&mut self, raw_type: &str) {
        self.report.warn(TranslateWarning::DegradedToText {
            kind: raw_type.to_string(),
        });
    }

    // ------------------------------------------------------------------ 官方 parseBlock

    /// 官方 `parseBlock`
    fn parse_block(&mut self, el: &XmlNode) -> BlockJson {
        let raw_type = el.attr("type").unwrap_or_default().to_string();
        let mut node = self.initialize_block_json(el);
        if MUTATION_TEXT_INDEX.contains_key(raw_type.as_str()) {
            node.mutation = Some(self.create_mutation_for_block_type(el, &raw_type));
        }
        self.handle_special_block_types(&mut node);
        for child in &el.children {
            self.handle_child_element(child, el, &mut node);
        }
        if node.kind == "bcm_translator_text_execution_block"
            || node.kind == "bcm_translator_text_event_block"
        {
            node.disabled = true;
        }
        if is_text_placeholder(&node.kind) {
            self.note_degraded(&raw_type);
        }
        if node.kind == "procedures_2_return_value" {
            node.mutation =
                Some(format!("<mutation xmlns=\"{XHTML}\" items=\"1\" type=\"ROUND\"></mutation>"));
            let parent_type = self
                .ctx
                .current_params
                .as_ref()
                .and_then(|params| params.first())
                .map(|param| param.parent_type.clone())
                .unwrap_or_default();
            if parent_type == PROCEDURE_NORMAL {
                node.inputs.clear();
                node.shadows.clear();
                node.shadows
                    .insert("PROCEDURES_2_DEFRETURN_RETURN".to_string(), String::new());
            }
            if parent_type == PROCEDURE_ROUND {
                node.shadows
                    .insert("PROCEDURES_2_DEFRETURN_RETURN".to_string(), String::new());
                // 官方此处把 `cI`(**函数本身**,漏了调用)拼进 id,产物里是一段函数源码 ——
                // 那是官方 bug,这里铸真 id(见模块文档"已知偏差")。
                let value_id = self.uuid();
                node.shadows.insert(
                    "VALUE".to_string(),
                    format!(
                        "<shadow xmlns=\"{XHTML}\" type=\"math_number\" id=\"{value_id}\" \
                         visible=\"visible\"><field constraints=\"-Infinity,Infinity,0,\" \
                         name=\"NUM\">0</field></shadow>"
                    ),
                );
            }
        }
        let appear = string_field(&node, "appear");
        if node.kind == "self_stress_animation" && (appear == "disappear" || appear == "appear") {
            node.kind = "self_appear_animation".to_string();
        }
        if node.kind == "appearance_of_sprite" {
            let coordinate = string_field(&node, "coordinate");
            if coordinate == "y" || coordinate == "x" {
                node.kind = "coordinate_of_sprite".to_string();
            } else if !coordinate.is_empty() {
                node.kind = "style_of_sprite".to_string();
            }
        }
        node
    }

    /// 官方 `initializeBlockJson`
    fn initialize_block_json(&mut self, el: &XmlNode) -> BlockJson {
        let mut node = BlockJson {
            kind: map_type(el.attr("type").unwrap_or_default()).to_string(),
            id: Some(
                el.attr("id")
                    .map(str::to_string)
                    .unwrap_or_else(|| self.uuid()),
            ),
            location: Some(json!([parse_int(el, "x"), parse_int(el, "y")])),
            shield: el.attr("shield") == Some("true"),
            ..BlockJson::default()
        };
        node.extra.insert(
            "visible".to_string(),
            Value::Bool(el.attr("visible") == Some("visible")),
        );
        node.extra.insert(
            "inline".to_string(),
            Value::Bool(el.attr("inline") == Some("true")),
        );
        node
    }

    /// 官方 `handleSpecialBlockTypes`(注意:switch 的是**映射后**类型)
    fn handle_special_block_types(&mut self, node: &mut BlockJson) {
        match node.kind.as_str() {
            "self_disappear" => {
                node.kind = "self_appear".to_string();
                node.fields
                    .insert("value".to_string(), Value::String("disappear".to_string()));
            }
            "self_appear" => {
                node.fields
                    .insert("value".to_string(), Value::String("appear".to_string()));
            }
            "self_gradually_disappear" => {
                node.kind = "self_gradually_show_hide".to_string();
                node.fields
                    .insert("show_hide".to_string(), Value::String("hide".to_string()));
            }
            "self_gradually_appear" => {
                node.kind = "self_gradually_show_hide".to_string();
                node.fields
                    .insert("show_hide".to_string(), Value::String("show".to_string()));
            }
            // 官方此支是死代码:`math_arithmetic_power` 先被 `transformMathArithmeticPower`
            // 改写成 `math_arithmetic_common` + `OP=POWER`,映射后类型永远不会是它。照抄保留。
            "math_arithmetic_power" => {
                node.fields
                    .insert("type".to_string(), Value::String("power".to_string()));
            }
            "self_change_coordinate_x"
            | "self_change_coordinate_y"
            | "self_glide_coordinate_x"
            | "self_glide_coordinate_y"
            | "self_change_scale" => {
                node.fields
                    .insert("increase".to_string(), Value::String("increase".to_string()));
            }
            _ => {}
        }
    }

    /// 官方 `createMutationForBlockType`(只对 `oI` 里登记过的原始类型调用)
    fn create_mutation_for_block_type(&mut self, el: &XmlNode, raw_type: &str) -> String {
        let Some(text) = MUTATION_TEXT_INDEX.get(raw_type).copied() else {
            return format!("<mutation xmlns=\"{XHTML}\" items=\"0\"></mutation>");
        };
        let title = match text {
            NemoMutationText::Plain(text) => (*text).to_string(),
            NemoMutationText::Select(map) => {
                let key = match raw_type {
                    "show_ranking" => {
                        field_text(el, "direction").unwrap_or_else(|| "positive".to_string())
                    }
                    "on_phone_tilt" => {
                        field_text(el, "type").unwrap_or_else(|| "default".to_string())
                    }
                    "set_fill_path" => {
                        field_text(el, "point").unwrap_or_else(|| "base".to_string())
                    }
                    "self_flip" => field_text(el, "options").unwrap_or_else(|| "0".to_string()),
                    "logic_boolean" => field_text(el, "BOOL").unwrap_or_else(|| "TRUE".to_string()),
                    _ => String::new(),
                };
                select_text(map, &key)
            }
            NemoMutationText::SelectNested(map) => {
                let key = field_text(el, "options").unwrap_or_else(|| "false".to_string());
                let index = el
                    .child("value")
                    .filter(|value| value.attr("name") == Some("index"))
                    .and_then(|value| value.child("shadow"))
                    .and_then(|shadow| shadow.child("field"))
                    .map(XmlNode::text_content)
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "1".to_string());
                let inner = map
                    .iter()
                    .find(|(selector, _)| *selector == key)
                    .map(|(_, inner)| *inner)
                    .unwrap_or(&[]);
                if index == "__previous_scene" || index == "__next_scene" {
                    select_text(inner, &index)
                } else {
                    select_text(inner, "default").replace("{index}", &index)
                }
            }
        };
        format!("<mutation xmlns=\"{XHTML}\" items=\"0\">{title}</mutation>")
    }

    // ------------------------------------------------------------------ 子元素分发

    /// 官方 `handleChildElement`
    fn handle_child_element(&mut self, el: &XmlNode, parent_el: &XmlNode, parent: &mut BlockJson) {
        match el.tag.as_str() {
            "next" => {
                if let Some(block) = el.child("block") {
                    let mut child = self.parse_block(block);
                    child.parent_id = parent.id.clone();
                    parent.next = Some(Box::new(child));
                }
            }
            "value" => {
                let name = el.attr("name").unwrap_or_default().to_string();
                let result = self.handle_value_element(el, parent_el, parent, &name);
                parent.inputs.extend(result.inputs);
                parent.shadows.extend(result.shadows);
            }
            "statement" => {
                let name = el.attr("name").unwrap_or_default().to_string();
                if let Some(block) = el.child("block") {
                    let mut child = self.parse_block(block);
                    child.parent_id = parent.id.clone();
                    parent.statements.insert(name, child);
                    if parent.kind == "when" && parent.shadows.is_empty() {
                        parent.shadows.insert("DO".to_string(), String::new());
                        parent.shadows.insert("condition".to_string(), String::new());
                    }
                }
            }
            "field" => {
                // 官方对**每个** `<field>` 子元素都重新 `parseFields(父元素)`,然后整体合并;
                // 副作用是"每次都会重铸广播输入的 id",最终 id 来自最后一次 —— 这里同序。
                let fields = self.parse_fields(parent_el);
                parent.fields.extend(node_fields(fields.clone()));
                if let Some(message) = fields.get("message").and_then(Value::as_str) {
                    let message = message.to_string();
                    let parent_id = parent.id.clone().unwrap_or_default();
                    self.handle_broadcast_field(parent, &message, &parent_id);
                }
                self.handle_procedure_fields(parent, &fields);
            }
            "mutation" => {
                // 官方 `e.setAttribute("xmlns", lI)` 后 `serializeToString(e)`:同名替换、异名追加到末尾
                let mut copy = el.clone();
                copy.set_attr("xmlns", XHTML);
                if parent.kind != "bcm_translator_text_execution_block" {
                    parent.mutation = Some(copy.serialize());
                }
                if is_procedure_call_block(&parent.kind) {
                    self.handle_procedure_call_mutation(el, parent);
                }
            }
            _ => {}
        }
    }

    /// 官方 `handleValueElement`
    fn handle_value_element(
        &mut self,
        el: &XmlNode,
        parent_el: &XmlNode,
        parent: &BlockJson,
        name: &str,
    ) -> ValueResult {
        for (index, child) in el.children.iter().enumerate() {
            let next = el.children.get(index + 1);
            match child.tag.as_str() {
                "empty" => return self.handle_empty_element(child, next, parent, name),
                "shadow" => return self.handle_shadow_element(child, next, parent_el, parent, name),
                _ => {}
            }
        }
        ValueResult::default()
    }

    /// 官方 `handleEmptyElement`
    fn handle_empty_element(
        &mut self,
        empty: &XmlNode,
        next: Option<&XmlNode>,
        parent: &BlockJson,
        slot_raw: &str,
    ) -> ValueResult {
        let mapped_empty = map_type(empty.attr("type").unwrap_or_default()).to_string();
        let slot = if parent.kind == "logic_negate" && slot_raw == "BOOL" {
            "logic".to_string()
        } else {
            slot_raw.to_string()
        };
        let Some(next) = next else {
            return self.create_empty_element_result(empty, &mapped_empty, parent, &slot);
        };
        if next.tag != "block" {
            return ValueResult::default();
        }
        let next_kind = map_type(next.attr("type").unwrap_or_default()).to_string();
        let mut result = ValueResult::default();
        if is_logic_compare_block(&next_kind) {
            result.inputs = self.handle_logic_compare_block(next, parent, &slot);
        } else if !next_kind.is_empty() && is_multi_parameter_block(&next_kind) {
            result.inputs = self.handle_multi_parameter_block(next, parent, &slot);
        } else if is_procedure_block(&next_kind) {
            return self.handle_procedure_block(empty, next, &slot);
        } else {
            result.inputs = self.create_nested_block_result(next, parent, &slot);
        }
        let id = empty
            .attr("id")
            .map(str::to_string)
            .unwrap_or_else(|| self.uuid());
        let fields = self.parse_fields(empty);
        result
            .shadows
            .insert(slot, render_shadow_xml(&mapped_empty, &id, &fields));
        result
    }

    /// 官方 `handleShadowElement`
    fn handle_shadow_element(
        &mut self,
        shadow: &XmlNode,
        next: Option<&XmlNode>,
        parent_el: &XmlNode,
        parent: &BlockJson,
        slot_raw: &str,
    ) -> ValueResult {
        let next_kind = next
            .map(|node| map_type(node.attr("type").unwrap_or_default()).to_string())
            .unwrap_or_default();
        let shadow_kind = map_type(shadow.attr("type").unwrap_or_default()).to_string();
        let id = shadow
            .attr("id")
            .map(str::to_string)
            .unwrap_or_else(|| self.uuid());
        let fields = self.parse_fields(shadow);
        let slot = mapped_slot_name(&parent.kind, &slot_raw.to_lowercase());
        let procedure_slot = self.get_procedure_id(parent_el, &parent.kind, &slot);
        let effective = if procedure_slot.is_empty() {
            slot.clone()
        } else {
            procedure_slot
        };
        if let Some(next) = next {
            let mut result = ValueResult::default();
            if is_logic_compare_block(&next_kind) {
                result.inputs = self.handle_logic_block(next, parent, &effective);
            } else if !next_kind.is_empty() && is_multi_parameter_block(&next_kind) {
                result.inputs = self.handle_multi_parameter_block(next, parent, &effective);
            } else {
                if is_procedure_block(&next_kind) {
                    return self.handle_procedure_block(shadow, next, &slot);
                }
                return self.handle_common_block(shadow, Some(next), parent_el, parent, &slot);
            }
            result
                .shadows
                .insert(effective, render_shadow_xml(&shadow_kind, &id, &fields));
            return result;
        }
        self.handle_common_block(shadow, None, parent_el, parent, &slot)
    }

    /// 官方 `handleCommonBlock`:影子实体化成输入节点(+ 还原影子 XML)
    fn handle_common_block(
        &mut self,
        shadow: &XmlNode,
        next: Option<&XmlNode>,
        parent_el: &XmlNode,
        parent: &BlockJson,
        slot: &str,
    ) -> ValueResult {
        let next_kind = next
            .map(|node| map_type(node.attr("type").unwrap_or_default()).to_string())
            .unwrap_or_default();
        let shadow_raw = shadow.attr("type").unwrap_or_default();
        let shadow_kind = map_type(shadow_raw).to_string();
        let shadow_fields = self.parse_fields(shadow);
        let fields = match next {
            Some(node) => self.parse_fields(node),
            None => shadow_fields.clone(),
        };
        let procedure_slot = self.get_procedure_id(parent_el, &parent.kind, slot);
        let mut node = self.create_input_detail(next, parent, &shadow_kind, fields, &next_kind);
        self.adjust_input_type(&mut node);
        if is_text_placeholder(&node.kind) {
            self.note_degraded(shadow_raw);
        }
        let id = shadow
            .attr("id")
            .map(str::to_string)
            .unwrap_or_else(|| self.uuid());
        let xml = render_shadow_xml(&shadow_kind, &id, &shadow_fields);
        let key = if procedure_slot.is_empty() {
            slot.to_string()
        } else {
            procedure_slot
        };
        let mut result = ValueResult::default();
        result.inputs.insert(key.clone(), node);
        result.shadows.insert(key, xml);
        result
    }

    /// 官方 `createInputDetail`
    fn create_input_detail(
        &mut self,
        next: Option<&XmlNode>,
        parent: &BlockJson,
        shadow_kind: &str,
        fields: BTreeMap<String, Value>,
        next_kind: &str,
    ) -> BlockJson {
        let mut mutation = String::new();
        if let Some(node) = next {
            let raw = node.attr("type").unwrap_or_default();
            if MUTATION_TEXT_INDEX.contains_key(raw) {
                mutation = self.create_mutation_for_block_type(node, raw);
            }
        }
        // 官方 `this.currentActor || "get_styles" !== r || (r = "math_number")`:
        // 解析程序集时没有 currentActor,`get_styles` 影子退成 `math_number`。
        let fallback = if self.ctx.current_actor.is_none() && shadow_kind == "get_styles" {
            "math_number"
        } else {
            shadow_kind
        };
        let special = is_special_block_type(next_kind);
        BlockJson {
            kind: if special {
                next_kind.to_string()
            } else {
                fallback.to_string()
            },
            id: Some(
                next.and_then(|node| node.attr("id"))
                    .map(str::to_string)
                    .unwrap_or_else(|| self.uuid()),
            ),
            fields: node_fields(fields),
            is_shadow: !special,
            parent_id: parent.id.clone(),
            mutation: Some(mutation),
            ..BlockJson::default()
        }
    }

    /// 官方 `createNestedBlockResult`
    fn create_nested_block_result(
        &mut self,
        next: &XmlNode,
        parent: &BlockJson,
        slot: &str,
    ) -> BTreeMap<String, BlockJson> {
        let raw = next.attr("type").unwrap_or_default();
        let kind = map_type(raw).to_string();
        let parsed = self.parse_block(next);
        let mutation = if MUTATION_TEXT_INDEX.contains_key(raw) {
            self.create_mutation_for_block_type(next, raw)
        } else {
            String::new()
        };
        if is_text_placeholder(&kind) {
            self.note_degraded(raw);
        }
        let stub = BlockJson {
            kind: kind.clone(),
            id: Some(
                next.attr("id")
                    .map(str::to_string)
                    .unwrap_or_else(|| self.uuid()),
            ),
            is_shadow: next.tag == "shadow" || kind == "logic_empty",
            fields: node_fields(self.parse_fields(next)),
            parent_id: parent.id.clone(),
            mutation: Some(mutation),
            ..BlockJson::default()
        };
        let mut out = BTreeMap::new();
        // 官方:字段为空的 `logic_negate` 走"不要壳、只留子树"的等价物
        if stub.kind == "logic_negate" && stub.fields.is_empty() {
            out.insert(slot.to_string(), parsed);
        } else {
            out.insert(slot.to_string(), stub);
        }
        out
    }

    /// 官方 `createEmptyElementResult`
    fn create_empty_element_result(
        &mut self,
        empty: &XmlNode,
        mapped_empty: &str,
        parent: &BlockJson,
        slot: &str,
    ) -> ValueResult {
        let fields = self.parse_fields(empty);
        let id = empty
            .attr("id")
            .map(str::to_string)
            .unwrap_or_else(|| self.uuid());
        let mut node = BlockJson {
            kind: mapped_empty.to_string(),
            id: Some(id.clone()),
            is_shadow: empty.tag == "shadow" || mapped_empty == "logic_empty",
            fields: node_fields(fields.clone()),
            parent_id: parent.id.clone(),
            ..BlockJson::default()
        };
        self.adjust_input_type(&mut node);
        let mut result = ValueResult::default();
        result
            .shadows
            .insert(slot.to_string(), render_empty_xml(mapped_empty, &id, &fields));
        result.inputs.insert(slot.to_string(), node);
        result
    }

    /// 官方 `adjustInputType`
    fn adjust_input_type(&self, node: &mut BlockJson) {
        let coordinate = string_field(node, "coordinate");
        if coordinate == "y" || coordinate == "x" {
            node.kind = "coordinate_of_sprite".to_string();
        } else if !coordinate.is_empty() {
            node.kind = "style_of_sprite".to_string();
        }
    }

    /// 官方 `parseLogicCompare`:展开一个"值返回块"自己的 `<value>` 子槽
    fn parse_logic_compare(
        &mut self,
        el: &XmlNode,
        parent: &BlockJson,
    ) -> (
        BTreeMap<String, Value>,
        BTreeMap<String, BlockJson>,
        BTreeMap<String, String>,
    ) {
        let fields = self.parse_fields(el);
        let kind = map_type(el.attr("type").unwrap_or_default()).to_string();
        let mut inputs: BTreeMap<String, BlockJson> = BTreeMap::new();
        let mut shadows: BTreeMap<String, String> = BTreeMap::new();
        for (index, value) in el.children.iter().enumerate() {
            if value.tag != "value" {
                continue;
            }
            let name = value.attr("name").unwrap_or_default();
            let empty = value.child("empty");
            let shadow = value.child("shadow");
            let block = value.child("block");
            let slot = mapped_slot_name(&kind, &name.to_lowercase());
            if let Some(empty) = empty
                && shadow.is_none() && block.is_none() {
                    let result = self.handle_empty_element(
                        empty,
                        value.children.get(index + 1),
                        parent,
                        &slot,
                    );
                    inputs.extend(result.inputs);
                    shadows.extend(result.shadows);
                }
            if let Some(shadow) = shadow {
                self.handle_shadow_in_logic_compare(
                    shadow,
                    &mut inputs,
                    &mut shadows,
                    &slot,
                    parent,
                );
            }
            if let Some(block) = block
                && !slot.is_empty() {
                    let node = self.parse_block(block);
                    let node_id = node.id.clone().unwrap_or_default();
                    inputs.insert(slot.clone(), node);
                    let outer = map_type(el.attr("type").unwrap_or_default());
                    // 官方:逻辑运算/取反的槽位被覆盖块占住时,影子槽补一个 `logic_empty`
                    if (outer == "logic_operation" || outer == "logic_negate")
                        && !shadows.contains_key(&slot)
                    {
                        shadows.insert(
                            slot.clone(),
                            render_empty_xml("logic_empty", &node_id, &BTreeMap::new()),
                        );
                    }
                }
        }
        (fields, inputs, shadows)
    }

    /// 官方 `handleShadowInLogicCompare`
    fn handle_shadow_in_logic_compare(
        &mut self,
        shadow: &XmlNode,
        inputs: &mut BTreeMap<String, BlockJson>,
        shadows: &mut BTreeMap<String, String>,
        slot: &str,
        parent: &BlockJson,
    ) {
        if slot.is_empty() {
            return;
        }
        let id = shadow
            .attr("id")
            .map(str::to_string)
            .unwrap_or_else(|| self.uuid());
        let kind = map_type(shadow.attr("type").unwrap_or_default()).to_string();
        let fields = self.parse_fields(shadow);
        shadows.insert(slot.to_string(), render_shadow_xml(&kind, &id, &fields));
        let mut node = shadow_input_node(&kind, &id, fields, parent);
        self.adjust_input_type(&mut node);
        inputs.insert(slot.to_string(), node);
    }

    /// 官方 `handleLogicCompareBlock`(`<empty>` + 覆盖块;**不带** `parent_id`)
    fn handle_logic_compare_block(
        &mut self,
        block: &XmlNode,
        parent: &BlockJson,
        slot: &str,
    ) -> BTreeMap<String, BlockJson> {
        let mut node = self.logic_node(block, parent);
        node.parent_id = None;
        let mut out = BTreeMap::new();
        out.insert(slot.to_string(), node);
        out
    }

    /// 官方 `handleLogicBlock`(`<shadow>` + 覆盖块;带 `parent_id`)
    fn handle_logic_block(
        &mut self,
        block: &XmlNode,
        parent: &BlockJson,
        slot: &str,
    ) -> BTreeMap<String, BlockJson> {
        let mut node = self.logic_node(block, parent);
        node.parent_id = parent.id.clone();
        let mut out = BTreeMap::new();
        out.insert(slot.to_string(), node);
        out
    }

    /// `handleLogicCompareBlock` / `handleLogicBlock` 的公共体
    fn logic_node(&mut self, block: &XmlNode, parent: &BlockJson) -> BlockJson {
        let (fields, inputs, shadows) = self.parse_logic_compare(block, parent);
        let mut node = BlockJson {
            kind: map_type(block.attr("type").unwrap_or_default()).to_string(),
            id: Some(self.uuid()),
            fields: node_fields(fields),
            inputs,
            shadows,
            ..BlockJson::default()
        };
        if parent.kind == "when" {
            node.shadows.clear();
            node.shadows.insert("DO".to_string(), String::new());
            node.shadows.insert("condition".to_string(), String::new());
        }
        if block.attr("type") == Some("math_number_property") {
            let condition_id = self.uuid();
            node.shadows.clear();
            node.shadows.insert(
                "condition".to_string(),
                format!(
                    "<empty xmlns=\"{XHTML}\" type=\"{}\" id=\"{condition_id}\" visible=\"visible\" \
                     editable=\"false\"></empty>",
                    node.kind
                ),
            );
            node.shadows.insert("DO".to_string(), String::new());
        }
        node
    }

    /// 官方 `handleMultiParameterBlock`
    fn handle_multi_parameter_block(
        &mut self,
        block: &XmlNode,
        parent: &BlockJson,
        slot: &str,
    ) -> BTreeMap<String, BlockJson> {
        let (fields, inputs, shadows) = self.parse_logic_compare(block, parent);
        let items = inputs.len();
        let node = BlockJson {
            kind: map_type(block.attr("type").unwrap_or_default()).to_string(),
            id: Some(self.uuid()),
            fields: node_fields(fields),
            inputs,
            shadows,
            parent_id: parent.id.clone(),
            mutation: Some(format!(
                "<mutation xmlns=\"{XHTML}\" items=\"{items}\"></mutation>"
            )),
            ..BlockJson::default()
        };
        let mut out = BTreeMap::new();
        out.insert(slot.to_string(), node);
        out
    }

    /// 官方 `handleProcedureBlock`(槽里是 `<shadow>`/`<empty>`,覆盖块是 `procedures_2_*`)
    fn handle_procedure_block(&mut self, el: &XmlNode, next: &XmlNode, slot: &str) -> ValueResult {
        let kind = map_type(el.attr("type").unwrap_or_default()).to_string();
        let next_kind = map_type(next.attr("type").unwrap_or_default()).to_string();
        let fields = self.parse_fields(el);
        let arg_slot = if next_kind == "procedures_2_callreturn" && slot.contains("arg") {
            self.find_procedure_id(next, PROCEDURE_ROUND, Some(slot))
        } else {
            String::new()
        };
        let id = el.attr("id")
            .map(str::to_string)
            .unwrap_or_else(|| self.uuid());
        let xml = if kind.is_empty() {
            None
        } else {
            Some(render_shadow_xml(&kind, &id, &fields))
        };
        let key = if arg_slot.is_empty() {
            slot.to_string()
        } else {
            arg_slot
        };
        let mut result = ValueResult::default();
        result.inputs.insert(key.clone(), self.parse_block(next));
        if let Some(xml) = xml {
            result.shadows.insert(key, xml);
        }
        result
    }

    // ------------------------------------------------------------------ 字段

    /// 官方 `parseFields`
    fn parse_fields(&mut self, el: &XmlNode) -> BTreeMap<String, Value> {
        let kind = map_type(el.attr("type").unwrap_or_default()).to_string();
        let mut out = BTreeMap::new();
        let mut any = false;
        for field in el.children_of("field") {
            any = true;
            let raw = field.attr("name").unwrap_or_default();
            let mut value = field.text_content();
            let mut name = processed_field_name(raw, &kind);
            if (kind == "appearance_of_sprite" || kind == "coordinate_of_sprite")
                && name == "attribute"
            {
                match value.as_str() {
                    "0" | "1" | "2" => name = "coordinate".to_string(),
                    "3" | "5" => name = "appearance".to_string(),
                    _ => {}
                }
            }
            // 官方此处字段值是 `undefined` 时**键仍然存在**:写进节点后 `JSON.stringify` 会丢掉它,
            // 但 `createShadowXml` 会照键造一个空 `<field name="…"/>`(实测官方产物如此)。
            // 所以这里用 `Value::Null` 占位,交给 `node_fields()` 在进节点时剔除。
            match self.process_field_value(&name, &value, &kind) {
                Some(mapped) => value = mapped,
                None => {
                    out.insert(name, Value::Null);
                    continue;
                }
            }
            if self.ctx.current_actor.is_none() && kind == "get_styles" && name == "style_id" {
                name = "NUM".to_string();
            }
            out.insert(name, Value::String(value));
        }
        if any {
            // 官方在 reduce 里把这三条默认值并到**每一条**字段之后(等效于最后覆盖同名键)
            match kind.as_str() {
                "mouse_down" => {
                    out.insert("sprite".to_string(), Value::String("--screen".to_string()));
                }
                "add_width_height_scale" | "self_change_effect" => {
                    out.insert("increase".to_string(), Value::String("increase".to_string()));
                }
                _ => {}
            }
        }
        out
    }

    /// 官方 `processFieldValue`;`None` = JS 里的 `undefined`(字段键被丢掉)
    fn process_field_value(&self, field: &str, value: &str, kind: &str) -> Option<String> {
        if kind == "self_rotate_around" && field == "sprite" && value == "__self" {
            return Some(match &self.ctx.current_actor {
                Some(actor) if !actor.id.is_empty() => actor.id.clone(),
                _ => value.to_string(),
            });
        }
        if let Some(mapped) = SPECIAL_FIELD_VALUES_INDEX
            .get(field)
            .and_then(|values| values.get(value))
        {
            return Some((*mapped).to_string());
        }
        if ["replace_list_item", "list_item", "delete_list_item"].contains(&kind)
            && field == "item"
            && value == "first"
        {
            return Some("any".to_string());
        }
        if (kind == "appearance_of_sprite" || kind == "coordinate_of_sprite")
            && (field == "coordinate" || field == "appearance")
        {
            return Some(process_appearance_attribute(value));
        }
        if kind == "get_clone_index_property" && field == "attribute" {
            return Some(process_clone_index_attribute(value));
        }
        if kind == "mouse_down" && field == "mouse_event_type" {
            return Some("type".to_string());
        }
        if kind == "get_styles" && field == "style_id" {
            return match self.ctx.current_actor.as_ref().and_then(|actor| actor.styles.as_ref()) {
                Some(styles) => style_at(styles, value),
                None => Some(value.to_string()),
            };
        }
        if kind == "procedures_2_callreturn" && field == "NAME" {
            if let Some(procedure) = self
                .ctx
                .procedures
                .values()
                .find(|procedure| procedure.name == value && procedure.kind == PROCEDURE_ROUND)
            {
                return Some(procedure.id.clone());
            }
            return Some(value.to_string());
        }
        Some(value.to_string())
    }

    /// 官方 `handleBroadcastField`
    fn handle_broadcast_field(&mut self, parent: &mut BlockJson, message: &str, parent_id: &str) {
        let resolved = self.get_broadcast_message(message);
        let input_id = self.uuid();
        let mut fields: BTreeMap<String, Value> = BTreeMap::new();
        fields.insert("message".to_string(), Value::String(resolved.clone()));
        parent.inputs.insert(
            "message".to_string(),
            BlockJson {
                kind: "broadcast_input".to_string(),
                id: Some(input_id),
                is_shadow: true,
                fields,
                is_output: true,
                parent_id: Some(parent_id.to_string()),
                ..BlockJson::default()
            },
        );
        let shadow_id = self.uuid();
        // 官方这段是纯字符串拼接(不过 DOM),**不转义** —— 照抄。
        parent.shadows.insert(
            "message".to_string(),
            format!(
                "<shadow xmlns=\"{XHTML}\" type=\"broadcast_input\" id=\"{shadow_id}\" \
                 visible=\"visible\"><field name=\"message\">{resolved}</field></shadow>"
            ),
        );
        parent.fields.remove("message");
    }

    /// 官方 `getBroadcastMessage`
    fn get_broadcast_message(&self, message: &str) -> String {
        if message == "?" {
            return message.to_string();
        }
        if self.ctx.has_broadcasts {
            return self
                .ctx
                .broadcast_names
                .get(message)
                .cloned()
                .unwrap_or_else(|| "?".to_string());
        }
        message.to_string()
    }

    // ------------------------------------------------------------------ 程序集

    /// 官方 `getProcedureId`(`e.closest("block")` = 当前正在解析的那个块)
    fn get_procedure_id(&self, parent_el: &XmlNode, parent_kind: &str, slot: &str) -> String {
        if !is_procedure_call_block(parent_kind) {
            return String::new();
        }
        let kind = if parent_kind == "procedures_2_callnoreturn" {
            PROCEDURE_NORMAL
        } else {
            PROCEDURE_ROUND
        };
        self.find_procedure_id(parent_el, kind, Some(slot))
    }

    /// 官方 `findProcedureId`
    fn find_procedure_id(&self, block: &XmlNode, kind: &str, slot: Option<&str>) -> String {
        let Some(mutation) = block.child("mutation") else {
            return String::new();
        };
        let Some(name) = mutation.attr("name") else {
            return String::new();
        };
        let shadows = collect_descendants(mutation, "procedures_2_parameter_shadow");
        let target = match slot {
            None => shadows.first().copied(),
            Some(slot) => {
                let Some(index) = arg_index(slot) else {
                    return String::new();
                };
                match shadows.get(index) {
                    Some(node) => Some(*node),
                    None => return String::new(),
                }
            }
        };
        let Some(target) = target else {
            return String::new();
        };
        let Some(param_name) = target.attr("name") else {
            return String::new();
        };
        self.find_procedure_param_id(name, param_name, kind)
    }

    /// 官方 `findProcedureParamId` + `findMatchingProcedureInDict`
    fn find_procedure_param_id(&self, name: &str, param_name: &str, kind: &str) -> String {
        self.ctx
            .procedures
            .values()
            .find(|procedure| procedure.name == name && procedure.kind == kind)
            .and_then(|procedure| {
                procedure
                    .params
                    .iter()
                    .find(|param| param.name == param_name && param.kind == "String")
            })
            .map(|param| param.id.clone())
            .unwrap_or_default()
    }

    /// 官方 `findMatchingProcedure`
    fn find_matching_procedure(&self, name: &str, kind: &str) -> Option<&NemoProcedure> {
        self.ctx
            .procedures
            .values()
            .find(|procedure| procedure.name == name && procedure.kind == kind)
    }

    /// 官方 `handleProcedureFields`
    fn handle_procedure_fields(&mut self, parent: &mut BlockJson, fields: &BTreeMap<String, Value>) {
        if parent.kind == "procedures_2_callnoreturn" || parent.kind == "procedures_2_callreturn" {
            let kind = if parent.kind == "procedures_2_callreturn" {
                PROCEDURE_ROUND
            } else {
                PROCEDURE_NORMAL
            };
            let name = fields
                .get("NAME")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if let Some(id) = self
                .find_matching_procedure(&name, kind)
                .map(|procedure| procedure.id.clone())
            {
                parent.fields.insert("NAME".to_string(), Value::String(id));
            }
        }
        if parent.kind == "procedures_2_parameter" || parent.kind == "procedures_2_parameter_shadow" {
            let procedure_name = self
                .ctx
                .current_params
                .as_ref()
                .and_then(|params| params.first())
                .map(|param| param.name.clone());
            if let Some(procedure_name) = procedure_name {
                self.handle_procedure_parameter_mutation(parent, fields, &procedure_name);
            }
        }
    }

    /// 官方 `handleProcedureParameterMutation`
    fn handle_procedure_parameter_mutation(
        &mut self,
        parent: &mut BlockJson,
        fields: &BTreeMap<String, Value>,
        procedure_name: &str,
    ) {
        let Some(parent_type) = self
            .ctx
            .current_params
            .as_ref()
            .and_then(|params| params.first())
            .map(|param| param.parent_type.clone())
        else {
            return;
        };
        let Some(procedure) = self
            .ctx
            .procedures
            .values()
            .find(|procedure| procedure.name == procedure_name && procedure.kind == parent_type)
        else {
            return;
        };
        let param_name = fields
            .get("param_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let found = procedure
            .params
            .iter()
            .skip(1)
            .find(|param| param.kind == "String" && param.name == param_name)
            .map(|param| param.id.clone());
        if let Some(id) = found {
            parent.mutation = Some(format!("<mutation xmlns=\"{XHTML}\" id=\"{id}\"/>"));
        }
    }

    /// 官方 `handleProcedureCallMutation`
    fn handle_procedure_call_mutation(&mut self, mutation_el: &XmlNode, parent: &mut BlockJson) {
        let name = mutation_el.attr("name").unwrap_or_default().to_string();
        let kind = if parent.kind == "procedures_2_callreturn" {
            PROCEDURE_ROUND
        } else {
            PROCEDURE_NORMAL
        };
        let Some(procedure) = self.find_matching_procedure(&name, kind).cloned() else {
            return;
        };
        parent.mutation = Some(self.create_procedure_call_mutation(&procedure, mutation_el));
        self.create_procedure_shadows(&procedure, parent);
    }

    /// 官方 `createProcedureCallMutation`
    fn create_procedure_call_mutation(
        &self,
        procedure: &NemoProcedure,
        mutation_el: &XmlNode,
    ) -> String {
        let mut out = format!(
            "<mutation xmlns=\"{XHTML}\" def_id=\"{}\" name=\"{}\" type=\"{}\">",
            procedure.id, procedure.id, procedure.kind
        );
        let mut push_arg = |id: &str, content: &str, kind: &str, out: &mut String| {
            out.push_str(&format!(
                "<arg id=\"{id}\" content=\"{content}\" type=\"{kind}\"/>"
            ));
        };
        if self.ctx.current_actor.is_some() {
            for param in &procedure.params {
                push_arg(&param.id, &param.name, &param.kind, &mut out);
            }
        } else {
            if let Some(label) = procedure.params.iter().find(|param| param.kind == "Label") {
                push_arg(
                    &label.id,
                    mutation_el.attr("name").unwrap_or_default(),
                    "Label",
                    &mut out,
                );
            }
            for node in collect_descendants(mutation_el, "procedures_2_parameter_shadow") {
                let name = node.attr("name").unwrap_or_default();
                if let Some(param) = procedure
                    .params
                    .iter()
                    .find(|param| param.name == name && param.kind == "String")
                {
                    push_arg(&param.id, &param.name, &param.kind, &mut out);
                }
            }
        }
        out.push_str("</mutation>");
        out
    }

    /// 官方 `createProcedureShadows`
    fn create_procedure_shadows(&self, procedure: &NemoProcedure, parent: &mut BlockJson) {
        let mut shadows: BTreeMap<String, String> = BTreeMap::new();
        for param in &procedure.params {
            if param.kind == "Label" {
                shadows.insert("NAME".to_string(), String::new());
                shadows.insert(param.id.clone(), String::new());
            }
        }
        if !shadows.is_empty() {
            parent.shadows.extend(shadows);
        }
    }
}

// ---------------------------------------------------------------------------
// 节点构造小工具
// ---------------------------------------------------------------------------

/// 官方 `createShadowInput`
fn shadow_input_node(
    kind: &str,
    id: &str,
    fields: BTreeMap<String, Value>,
    parent: &BlockJson,
) -> BlockJson {
    let mut constraints = Map::new();
    constraints.insert(
        "NUM".to_string(),
        json!({ "min": Value::Null, "max": Value::Null, "precision": 0, "mod": Value::Null }),
    );
    BlockJson {
        kind: kind.to_string(),
        id: Some(id.to_string()),
        is_shadow: true,
        fields: node_fields(fields),
        field_constraints: Some(Value::Object(constraints)),
        is_output: true,
        parent_id: parent.id.clone(),
        ..BlockJson::default()
    }
}

/// 进 KN 节点的 `fields`:剔除 `undefined`(`Value::Null` 占位)—— 官方 `JSON.stringify` 丢键
fn node_fields(fields: BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    fields
        .into_iter()
        .filter(|(_, value)| !value.is_null())
        .collect()
}

/// 取字符串字段(官方 `fields?.x`,缺失/非字符串 → `undefined` → 空串)
fn string_field(node: &BlockJson, name: &str) -> String {
    node.fields
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

// ---------------------------------------------------------------------------
// XML 渲染(官方 `createShadowXml` / `createEmptyXml`)
// ---------------------------------------------------------------------------

/// 官方 `createShadowXml`:字段名过 [`shadow_field_name`],文本按序列化规则转义
fn render_shadow_xml(kind: &str, id: &str, fields: &BTreeMap<String, Value>) -> String {
    let head = format!(
        "<shadow xmlns=\"{XHTML}\" type=\"{}\" id=\"{}\" visible=\"visible\"",
        escape_text(kind),
        escape_text(id)
    );
    if fields.is_empty() {
        return format!("{head}/>");
    }
    let mut out = format!("{head}>");
    for (name, value) in fields {
        out.push_str(&render_field(shadow_field_name(kind, name), value));
    }
    out.push_str("</shadow>");
    out
}

/// 官方 `createEmptyXml`:字段名**不做** `SHADOW_FIELD_NAME_MAP` 变换
fn render_empty_xml(kind: &str, id: &str, fields: &BTreeMap<String, Value>) -> String {
    let head = format!(
        "<empty xmlns=\"{XHTML}\" type=\"{}\" id=\"{}\" visible=\"visible\" editable=\"false\"",
        escape_text(kind),
        escape_text(id)
    );
    if fields.is_empty() {
        return format!("{head}/>");
    }
    let mut out = format!("{head}>");
    for (name, value) in fields {
        out.push_str(&render_field(name, value));
    }
    out.push_str("</empty>");
    out
}

fn render_field(name: &str, value: &Value) -> String {
    let text = match value {
        Value::String(text) => text.clone(),
        // `undefined`(JS) → `textContent = undefined` → 空文本元素
        Value::Null => String::new(),
        other => other.to_string(),
    };
    // 官方经 `textContent` + `XMLSerializer`:空文本不产生子节点 ⇒ 序列化成自闭合
    // (`<field name="TEXT"/>`);空白文本(`" "`)仍是有内容的文本节点。
    if text.is_empty() {
        return format!("<field name=\"{}\"/>", escape_text(name));
    }
    format!(
        "<field name=\"{}\">{}</field>",
        escape_text(name),
        escape_text(&text)
    )
}

/// 官方 XMLSerializer 的转义(属性值与文本共用一份足够安全的最小集合)
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}
