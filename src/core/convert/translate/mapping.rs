//! Kitten 侧积木 → KN 端语义映射(官方 `kittenBcmToNekoBcmUtils` 的忠实移植)。
//!
//! 对应 webpack module 41888(`temp/ref/mod41888.pretty.js`)的 `jC` 与 `GC`,按官方顺序做两段:
//!
//! 1. **逐积木的前端改写**(`jC.parseBlock` 内层 `i()`,77578-77770):行内类型特例
//!    (`self_set_position`/`self_change_coordinate`/`self_glide_coordinate` 按 `fields.coordinary`
//!    拆 `_x`/`_y`;`self_disappear`→`self_appear`;`terminate`→`stop`;`get_3`+`fields.attribute`→
//!    `processAppearanceAttribute`(77313)拆 `coordinate|style|appearance|effect_of_sprite`;
//!    `self_appear`→`fields.value="appear"`;`shadow_text`→`items="1"` 变异 + `MUTATE_BUTTON`)
//!    → `LC` 改名(`translateBlockType` 77860)→ 降级占位积木的变异文本
//!    (`createMutationForBlockType` 78303)→ 字段改名/取值映射(`mapFieldName` 77801、
//!    `mapFieldValue` 77790、`specialFieldValueMap` 77313-77485)→ 影子槽改名 + 影子 XML 改写
//!    (`getMappedName` 77497、`transformShadowXml` 77830)→ `text_select_changeable` 的 `items-1`
//!    修补 → 子连接路由(值输入/语句槽/`next`)。
//! 2. **后置特例**(`GC`,78639-78800):`shadow_number` 拆包或降级、`appearance_of_sprite`+UUID 属性
//!    变 `variables_get`、横屏坐标除以 1.3、`set_camera_alpha` 变 `100 - x`、`list_append` 插首项
//!    降级、循环体内音频积木置灰。
//!
//! 文件下半部是**反向**(KN → Kitten4,[`translate_kn_to_kitten`]):`LC`/字段表/槽位表/影子 XML 的
//! 逐条反转、`GC` 两个算术壳的拆解、列表积木 `pure_list_get` 的折叠、占位积木按 mutation 标题
//! 还原原类型;不可逆处一律进 [`TranslateReport`](见该节的分节说明)。
//!
//! ## 与官方的刻意差异(见 docs/20 §6.2「逃生舱」)
//!
//! - 官方为每个节点**新建**对象(`c`,只带 `type/id/location/shield/is_shadow/mutation`),渲染性键
//!   (`collapsed`/`deletable`/`movable`/`editable`/`visible`/`comment`/`field_extra_attr`)与
//!   `is_output`/`field_constraints` 因此被丢掉;本实现**原地改写**,这些键与 [`BlockJson::extra`] 保真。
//! - `parent_id` 仍按官方语义:值输入/语句槽子节点 = 父 id;`next` 子节点与根节点不带 `parent_id`。
//! - 官方对返值/返布尔占位积木(`h` 列表)**不接任何输入连接**,那些子积木在官方产物里会直接消失;
//!   本实现把它们留在原地(不丢数据),槽名也不改。
//! - 占位积木的中文标题:纯字符串项(`RC`)逐字照搬(官方对无 handler 的类型就是这么发的,`{value}`
//!   同样不填);由官方 31 个 `handle*` 拼词的型号改取「选择器字段 → `RC` 子串」近似,并记
//!   [`TranslateWarning::DegradedToText`](清单见 [`HANDLER_BUILT_TEXTS`]/[`SELECT_SPEC`]);
//!   需要工程级上下文(积木索引、`theatre.groups`)的 `handleCalculate`/`handleAutoPlayer*`/
//!   `handleSetEntityShowHide` 等一律退化为表内文本。
//! - 影子 XML / mutation 的改写用字符串级手术(官方走 `DOMParser`+`XMLSerializer`):对良构输入
//!   序列化字节等价(实测 jsdom 往返不变),畸形输入原样保留而不是产出 `parsererror` 文本。

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;

use serde_json::{Value, json};

use super::blockjson::{BlockJson, BlockTree};
use super::ids::IdSource;
use super::report::{TranslateReport, TranslateWarning};
use super::tables_gen::{
    KITTEN_MUTATION_TEXT, KITTEN_MUTATION_TEXT_SELECT, KITTEN_TO_KN, SHADOW_XML,
    TEXT_PLACEHOLDER_BLOCKS, ZH_NAME_BY_TYPE,
};

#[rustfmt::skip]
const PLACEHOLDERS_STATEMENT: &[&str] = &["bcm_translator_text_execution_block", "bcm_translator_text_event_block"];
#[rustfmt::skip]
const PLACEHOLDERS_OUTPUT: &[&str] = &["bcm_translator_text_return_value_block", "bcm_translator_text_return_boolean_block"];
#[rustfmt::skip]
const NEXT_ROUTE_TYPES: &[&str] = &["on_keydown", "sprite_on_tap", "on_swipe", "bcm_translator_text_event_block"];
#[rustfmt::skip]
const LOOP_DO_TYPES: &[&str] = &["repeat_forever", "start_as_a_mirror", "self_listen", "repeat_n_times", "repeat_forever_until"];
/// 官方路由时把 `fields.list` 合成 `inputs.list`(`pure_list_get` 影子)的列表积木
#[rustfmt::skip]
const LIST_INPUT_TYPES: &[&str] = &["list_append", "list_insert_value", "delete_list_item", "replace_list_item", "list_index_of", "list_is_exist", "show_hide_list"];
/// `GC` 横屏坐标包装覆盖的类型(`self_move_to`/`self_glide_to` 包 `x`/`y`,其余包 `value`/`steps`)
#[rustfmt::skip]
const LANDSCAPE_WRAP_TYPES: &[&str] = &["self_go_forward", "self_move_to", "self_set_position_x", "self_set_position_y", "self_glide_coordinate_x", "self_glide_coordinate_y", "self_glide_to", "self_change_coordinate_x", "self_change_coordinate_y"];

const SHADOW_TEXT_MUTATION: &str =
    "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"1\"></mutation>";
pub(crate) const XHTML: &str = "http://www.w3.org/1999/xhtml";

/// 官方 `createMutationForBlockType` 有专门 `handle*` 的 31 型(标题由字段拼词,我们只近似)
#[rustfmt::skip]
const HANDLER_BUILT_TEXTS: &[&str] = &[
    "self_flip", "set_entity_show_hide", "self_ask_listen", "self_ask_record", "change_volume_or_rate_2", "set_volume_or_rate_2",
    "set_layer_with_pen", "set_pen_path", "set_fill_style", "check_screen", "enable_voice_detection", "clone", "calculate",
    "set_camera_status", "set_camera_opcity", "get_camera_data", "get_glasses", "auto_player_record_score_and_delete_actor",
    "auto_player_use_model_supervised", "auto_player_train_save_restart", "auto_player_use_model_unsupervised",
    "auto_player_init_new_model", "check_running_device", "wood_block_get", "wood_block_set", "play_video", "play_video_until_end",
    "continue_pause_video", "show_hide_video", "is_ranking_show_hide", "show_ranking",
];

/// 近似标题的「选择器字段 → 缺省子串」(取自官方各 `handle*` 读的字段与 fallback)
#[rustfmt::skip]
const SELECT_SPEC: &[(&str, &str, &str)] = &[
    ("self_flip", "options", "0"), ("set_entity_show_hide", "type", "show"), ("self_ask_listen", "lang", "zh-hans"),
    ("change_volume_or_rate_2", "audio_key", "volume"), ("set_volume_or_rate_2", "audio_key", "volume"),
    ("set_layer_with_pen", "position", "above"), ("set_pen_path", "point", "start_point"), ("check_screen", "is_stay", "false"),
    ("enable_voice_detection", "state", "open"), ("set_camera_status", "status", "true"), ("get_camera_data", "action_type", "motion"),
    ("get_glasses", "glasses", "sun"), ("is_ranking_show_hide", "VAL", "show"), ("continue_pause_video", "option", "continue"),
    ("show_hide_video", "display", "show"), ("check_running_device", "device", "computer"), ("logic_boolean", "BOOL", "TRUE"),
];

/// `processAppearanceAttribute`(77313):`get_3` 的 `attribute` → (字段名, 字段值)
#[rustfmt::skip]
const APPEARANCE_ATTRIBUTE: &[(&str, (&str, &str))] = &[
    ("0", ("coordinate", "x")), ("1", ("coordinate", "y")), ("2", ("style", "style_of_sprite")), ("3", ("appearance", "direction")),
    ("5", ("appearance", "scale")), ("7", ("effect", "0")), ("8", ("effect", "1")), ("9", ("effect", "2")), ("10", ("effect", "3")),
    ("11", ("effect", "4")), ("12", ("effect", "5")), ("13", ("effect", "6")), ("14", ("effect", "?")),
    ("16", ("appearance", "width")), ("17", ("appearance", "height")),
];

/// `specialFieldValueMap`(77313-77485;21 组 110 条,逐条照抄):字段取值 → 目标取值
#[rustfmt::skip]
const SPECIAL_FIELD_VALUES: &[(&str, &[(&str, &str)])] = &[
    ("sprite", &[("__self", "--self"), ("__mouse", "--mouse"), ("__pointer", "--mouse"), ("__random", "--random")]),
    ("sprite1", &[("__self", "--self"), ("__mouse", "--mouse"), ("__pointer", "--mouse"), ("__random", "--random"), ("__edge", "--edge"), ("__edge_top", "--edge_top"), ("__edge_bottom", "--edge_bottom"), ("__edge_left", "--edge_left"), ("__edge_right", "--edge_right")]),
    ("sprite2", &[("__self", "--self"), ("__mouse", "--mouse"), ("__pointer", "--mouse"), ("__random", "--random"), ("__edge", "--edge"), ("__edge_top", "--edge_top"), ("__edge_bottom", "--edge_bottom"), ("__edge_left", "--edge_left"), ("__edge_right", "--edge_right")]),
    ("type", &[("say", "talk"), ("select_index", "index"), ("select_content", "content"), ("MULTIPLY", "multiply"), ("POWER", "power"), ("MINUS", "minus"), ("DIVIDE", "divide"), ("ADD", "add"), ("ROUND", "round"), ("ROUNDDOWN", "round_down"), ("ROUNDUP", "round_up"), ("AND", "and"), ("OR", "or")]),
    ("sound_id", &[("all", "all_audio")]),
    ("target", &[("X", "x"), ("Y", "y"), ("Z", "z"), ("__pointer", "--mouse"), ("__random", "--random")]),
    ("prev_next", &[("previous", "prev"), ("next", "next")]),
    ("layer", &[("peak", "peak"), ("bottom", "bottom"), ("next_level", "next_layer"), ("previous_level", "prev_layer")]),
    ("screen_id", &[("__previous_scene", "prev"), ("__next_scene", "next")]),
    ("scene", &[("__previous_scene", "prev"), ("__next_scene", "next")]),
    ("align", &[("LEFT", "right"), ("CENTER", "center"), ("RIGHT", "left")]),
    ("language", &[("english", "en"), ("chinese", "zh"), ("classical_chinese", "wyw"), ("french", "fra"), ("spanish", "spa"), ("japanese", "jp")]),
    ("attribute", &[("0", "x"), ("1", "y"), ("2", "style_index"), ("3", "direction"), ("4", "?"), ("5", "scale"), ("7", "0"), ("8", "1"), ("9", "2"), ("10", "3"), ("11", "4"), ("12", "5"), ("13", "6"), ("14", "?"), ("16", "width"), ("17", "height")]),
    ("axis", &[("X", "x"), ("Y", "y"), ("Z", "z")]),
    ("op", &[("week_num", "weekday_num"), ("week", "weekday")]),
    ("TYPE", &[("first", "any"), ("nth", "any")]),
    ("OP", &[("AND", "and"), ("OR", "or"), ("ROOT", "0"), ("ABS", "1"), ("NEG", "2"), ("LN", "3"), ("LOG10", "4"), ("EXP", "5"), ("POW10", "6"), ("SIN", "sin"), ("COS", "cos"), ("TAN", "tan"), ("ASIN", "asin"), ("ACOS", "acos"), ("ATAN", "atan"), ("ADD", "add"), ("MINUS", "minus"), ("MULTIPLY", "multiply"), ("DIVIDE", "divide"), ("POWER", "power"), ("ROUND", "round"), ("ROUNDDOWN", "round_down"), ("ROUNDUP", "round_up")]),
    ("BOOL", &[("TRUE", "true"), ("FALSE", "false")]),
    ("status", &[("true", "turn_on"), ("false", "turn_off")]),
    ("actor", &[("__self", "--self")]),
    ("lang", &[("zh-hans", "zh")]),
];

/// `getMappedName` 的表(77497-77577;69 组):(原类型, 原槽名) → 目标槽名
#[rustfmt::skip]
const INPUT_NAME_MAP: &[(&str, &[(&str, &str)])] = &[
    ("self_change_effect", &[("steps", "value")]), ("self_change_effect_3", &[("steps", "value")]),
    ("set_sprite_style", &[("index", "style_id")]), ("set_costume", &[("index", "style_id")]),
    ("list_length", &[("VAR", "list")]), ("lists_length", &[("VAR", "list")]), ("cloud_lists_length", &[("VAR", "list")]),
    ("list_index_of", &[("VAR", "list"), ("VALUE", "list_item_value")]), ("lists_index_of", &[("VAR", "list"), ("VALUE", "list_item_value")]),
    ("cloud_lists_index_of", &[("VAR", "list"), ("VALUE", "list_item_value")]), ("list_append", &[("VAR", "list"), ("VALUE", "list_item_value")]),
    ("lists_append", &[("VAR", "list"), ("VALUE", "list_item_value")]), ("cloud_lists_append", &[("VAR", "list"), ("VALUE", "list_item_value")]),
    ("list_insert_value", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "list_item_value")]),
    ("lists_insert_value", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "list_item_value")]),
    ("cloud_lists_insert_value", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "list_item_value")]),
    ("delete_list_item", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "type")]),
    ("lists_delete", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "type")]),
    ("cloud_lists_delete", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "type")]),
    ("replace_list_item", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "list_item_value")]),
    ("lists_replace", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "list_item_value")]),
    ("cloud_lists_replace", &[("VAR", "list"), ("INDEX", "list_index"), ("VALUE", "list_item_value")]),
    ("list_copy", &[("VALUE", "list"), ("TARGET", "target_list")]), ("lists_copy", &[("VALUE", "list"), ("TARGET", "target_list")]),
    ("cloud_lists_copy", &[("VALUE", "list"), ("TARGET", "target_list")]), ("list_item", &[("VAR", "list"), ("INDEX", "list_index")]),
    ("lists_get_value", &[("VAR", "list"), ("INDEX", "list_index")]), ("cloud_lists_get_value", &[("VAR", "list"), ("INDEX", "list_index")]),
    ("list_is_exist", &[("VAR", "list"), ("VALUE", "list_item_value")]), ("lists_is_exist", &[("VAR", "list"), ("VALUE", "list_item_value")]),
    ("cloud_lists_is_exist", &[("VAR", "list"), ("VALUE", "list_item_value")]),
    ("math_modulo", &[("DIVIDEND", "A"), ("DIVISOR", "B")]), ("random_num", &[("a", "A"), ("b", "B")]), ("random", &[("a", "A"), ("b", "B")]),
    ("logic_compare", &[("a", "A"), ("b", "B")]), ("text_contain", &[("TEXT1", "A"), ("TEXT2", "B")]),
    ("switch_to_screen", &[("screen", "screen_id")]), ("self_listen", &[("broadcast", "message")]),
    ("change_variables", &[("n", "value")]), ("change_variable", &[("n", "value")]), ("change_cloud_variable", &[("n", "value")]),
    ("variables_set", &[("VALUE", "value")]), ("cloud_variables_set", &[("VALUE", "value")]),
    ("math_number_property", &[("NUMBER_TO_CHECK", "num")]), ("divisible_by", &[("NUMBER_TO_CHECK", "A"), ("DIVISOR", "B")]),
    ("convert_type", &[("original_value", "text")]), ("text_length", &[("VALUE", "text")]),
    ("text_select_changeable", &[("STRING", "text"), ("NUM0", "start_index"), ("NUM1", "end_index")]),
    ("text_select", &[("STRING", "text"), ("NUM0", "start_index"), ("NUM1", "end_index")]),
    ("math_function", &[("NUM", "num")]), ("math_single", &[("NUM", "num")]), ("math_trig", &[("NUM", "num")]),
    ("math_trig_common", &[("NUM", "num")]), ("math_trig_arc", &[("NUM", "num")]), ("math_round", &[("NUM", "num")]),
    ("text_join", &[("VALUE", "ADD0")]), ("shadow_text", &[("VALUE", "ADD0")]), ("logic_negate", &[("BOOL", "logic")]),
    ("set_camera_opcity", &[("opcity", "camera_alpha")]), ("set_camera_alpha", &[("opcity", "camera_alpha")]),
    ("procedures_2_defnoreturn", &[("PROCEDURES_2_DEFNORETURN_MUTATOR", "PARAMS0")]),
    ("play_audio", &[("audio", "audio_id")]), ("play_audio_2", &[("audio", "audio_id")]),
    ("play_audio_and_wait", &[("audio", "audio_id")]), ("play_audio_and_wait_2", &[("audio", "audio_id")]),
    ("stop_audio", &[("audio", "audio_id")]), ("stop_audio_2", &[("audio", "audio_id")]),
    ("self_set_effect", &[("val", "value")]), ("self_set_effect_2", &[("val", "value")]),
];

/// `mapFieldName` 的表(77801-77827;28 条;`VAR`/`valname`/`OP` 三条按类型分流,见 [`map_field_name`])
#[rustfmt::skip]
const FIELD_NAME_MAP: &[(&str, &str)] = &[
    ("sound_id", "audio_id"), ("target", "sprite"), ("is_show", "show_hide"), ("screen", "screen_id"), ("scene", "screen_id"),
    ("MESSAGE", "message"), ("broadcast", "message"), ("key_event_type", "type"), ("mouse_event_type", "type"), ("sprite2", "sprite1"),
    ("sprite1", "sprite"), ("position", "type"), ("axis", "target"), ("actions", "type"), ("state", "showHide"), ("op", "time"),
    ("info", "type"), ("TYPE", "item"), ("FUNC", "show_hide"), ("PROPERTY", "type"), ("status", "camera_status"), ("actor", "sprite"),
    ("self_prev_next_style", "prev_next"), ("prev_or_next", "prev_next"), ("lang", "type"),
];

// ---------------------------------------------------------------- 公开 API

/// 把一个实体的积木树按官方管线翻成 KN 语义(就地改写 `tree`)。
///
/// `landscape` 对应官方模块级 `UC = size.width > size.height`(横屏时坐标要除以 1.3)。
pub(crate) fn translate_kitten_to_kn(
    tree: &mut BlockTree,
    landscape: bool,
    ids: &mut IdSource,
    report: &mut TranslateReport,
) {
    let mut ctx = Ctx {
        landscape,
        ids,
        report,
    };
    let roots = std::mem::take(&mut tree.roots);
    tree.roots = roots
        .into_iter()
        .map(|root| {
            let mut node = parse_node(root, &mut ctx);
            node.parent_id = None; // 官方根节点不带 parent_id
            gc_deep(node, &mut ctx)
        })
        .collect();
}

// ---------------------------------------------------------------- 正向查表索引
//
// 表是生成的 `&[(k, v)]`,直接 `iter().find` 是 O(表长) 线性扫描,而每积木每影子都会查。
// 这里建 `LazyLock` 索引;一律用 `entry().or_insert()`(**首个命中优先**),
// 与原来的 `iter().find(...)` 逐键等价(表里真有重复键时也保持"取第一个")。
static KITTEN_TO_KN_INDEX: std::sync::LazyLock<HashMap<&'static str, &'static str>> =
    std::sync::LazyLock::new(|| {
        let mut map = HashMap::with_capacity(KITTEN_TO_KN.len());
        for (kitten, kn) in KITTEN_TO_KN {
            map.entry(*kitten).or_insert(*kn);
        }
        map
    });

static SHADOW_XML_INDEX: std::sync::LazyLock<
    HashMap<&'static str, HashMap<&'static str, &'static str>>,
> = std::sync::LazyLock::new(|| {
    let mut map: HashMap<&'static str, HashMap<&'static str, &'static str>> =
        HashMap::with_capacity(SHADOW_XML.len());
    for (kind, slots) in SHADOW_XML {
        let entry = map.entry(*kind).or_default();
        for (slot, xml) in *slots {
            entry.entry(*slot).or_insert(*xml);
        }
    }
    map
});

static FIELD_NAME_MAP_INDEX: std::sync::LazyLock<HashMap<&'static str, &'static str>> =
    std::sync::LazyLock::new(|| {
        let mut map = HashMap::with_capacity(FIELD_NAME_MAP.len());
        for (from, to) in FIELD_NAME_MAP {
            map.entry(*from).or_insert(*to);
        }
        map
    });

static SPECIAL_FIELD_VALUES_INDEX: std::sync::LazyLock<
    HashMap<&'static str, HashMap<&'static str, &'static str>>,
> = std::sync::LazyLock::new(|| {
    let mut map: HashMap<&'static str, HashMap<&'static str, &'static str>> =
        HashMap::with_capacity(SPECIAL_FIELD_VALUES.len());
    for (field, entries) in SPECIAL_FIELD_VALUES {
        let entry = map.entry(*field).or_default();
        for (text, mapped) in *entries {
            entry.entry(*text).or_insert(*mapped);
        }
    }
    map
});

static INPUT_NAME_MAP_INDEX: std::sync::LazyLock<
    HashMap<&'static str, HashMap<&'static str, &'static str>>,
> = std::sync::LazyLock::new(|| {
    let mut map: HashMap<&'static str, HashMap<&'static str, &'static str>> =
        HashMap::with_capacity(INPUT_NAME_MAP.len());
    for (kind, slots) in INPUT_NAME_MAP {
        let entry = map.entry(*kind).or_default();
        for (slot, mapped) in *slots {
            entry.entry(*slot).or_insert(*mapped);
        }
    }
    map
});

static APPEARANCE_ATTRIBUTE_INDEX: std::sync::LazyLock<
    HashMap<&'static str, (&'static str, &'static str)>,
> = std::sync::LazyLock::new(|| {
    let mut map = HashMap::with_capacity(APPEARANCE_ATTRIBUTE.len());
    for (key, mapped) in APPEARANCE_ATTRIBUTE {
        map.entry(*key).or_insert(*mapped);
    }
    map
});

/// `LC` 查表(`translateBlockType` 77860),表里没有就返回原值
fn translate_type(kind: &str) -> &str {
    KITTEN_TO_KN_INDEX.get(kind).copied().unwrap_or(kind)
}

/// 是否 KN 的四种「文本占位积木」(降级产物)
fn is_text_placeholder(kind: &str) -> bool {
    TEXT_PLACEHOLDER_BLOCKS.contains(&kind)
}

/// 官方 `cy` 表:某积木某槽位的默认影子 XML
fn shadow_xml(kind: &str, slot: &str) -> Option<&'static str> {
    SHADOW_XML_INDEX.get(kind)?.get(slot).copied()
}

// ---------------------------------------------------------------- 查表

/// JS 属性访问 / `String(v)` / 模板拼接的等价物(缺失 → `undefined`)
fn js_text(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
    }
}

/// JS 对象取键:字符串/数字/布尔都能当键
fn value_key(value: &Value) -> Option<Cow<'_, str>> {
    match value {
        Value::String(s) => Some(Cow::Borrowed(s.as_str())),
        Value::Number(n) => Some(Cow::Owned(n.to_string())),
        Value::Bool(b) => Some(Cow::Borrowed(if *b { "true" } else { "false" })),
        _ => None,
    }
}

/// JS 真值判断(只用在确实照抄了 `if (x)` 的地方)。
///
/// 两个装配方向共用:`None` 等价 `undefined`;`serde_json` 表示不了 `NaN`
/// (`Number::from_f64(NaN)` 返回 `None`),所以无需再单独判 `NaN`。
pub(crate) fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(_) | Value::Object(_)) => true,
    }
}

/// `mapFieldName`(77801):按**原类型**改字段名
fn map_field_name<'a>(block_type: &str, name: &'a str) -> Cow<'a, str> {
    match name {
        "VAR" | "valname" => {
            if block_type.contains("variable") {
                Cow::Borrowed("variable")
            } else {
                Cow::Borrowed("list")
            }
        }
        "OP" => {
            if block_type == "logic_compare" {
                Cow::Borrowed("OP")
            } else {
                Cow::Borrowed("type")
            }
        }
        _ => match FIELD_NAME_MAP_INDEX.get(name) {
            Some(mapped) => Cow::Borrowed(*mapped),
            None => Cow::Borrowed(name),
        },
    }
}

/// `mapFieldValue`(77790):按**原字段名** + 取值查 `specialFieldValueMap`
fn map_field_value(name: &str, value: &Value) -> Value {
    match value_key(value)
        .as_deref()
        .and_then(|k| mapped_field_text(name, k))
    {
        Some(mapped) => Value::String(mapped),
        None => value.clone(),
    }
}

/// 取值的映射结果(未命中 → `None`);影子 XML 的文本改写也用它
fn mapped_field_text(name: &str, text: &str) -> Option<String> {
    SPECIAL_FIELD_VALUES_INDEX
        .get(name)?
        .get(text)
        .map(|v| (*v).to_string())
}

/// `getMappedName`(77497):槽位改名(含 `procedures_2_defnoreturn` 的 `PARAMS{n}` 后移)
fn get_mapped_name<'a>(block_type: &str, input_name: &'a str) -> Cow<'a, str> {
    if block_type == "procedures_2_defnoreturn" && input_name.contains("PARAMS") {
        return match input_name.replacen("PARAMS", "", 1).parse::<i64>() {
            Ok(index) => Cow::Owned(format!("PARAMS{}", index + 1)),
            Err(_) => Cow::Borrowed(input_name),
        };
    }
    match INPUT_NAME_MAP_INDEX.get(block_type).and_then(|slots| slots.get(input_name)) {
        Some(mapped) => Cow::Borrowed(*mapped),
        None => Cow::Borrowed(input_name),
    }
}

/// `processAppearanceAttribute`(77313)
fn process_appearance_attribute(key: &str) -> Option<(&'static str, &'static str)> {
    APPEARANCE_ATTRIBUTE_INDEX.get(key).copied()
}

/// 官方 UUID 正则 `/^[0-9a-f]{8}-…$/i` 的等价判断
fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

// ---------------------------------------------------------------- 降级占位积木的变异文本

fn rc_plain(kind: &str) -> Option<&'static str> {
    KITTEN_MUTATION_TEXT
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, v)| *v)
}

fn select_text(kind: &str, key: &str) -> Option<&'static str> {
    KITTEN_MUTATION_TEXT_SELECT
        .iter()
        .find(|(k, _)| *k == kind)
        .and_then(|(_, entries)| entries.iter().find(|(k, _)| *k == key).map(|(_, v)| *v))
}

/// 官方 `createMutationForBlockType`(78303)的近似:返回 (标题文本, 是否近似)
fn mutation_text(orig: &str, fields: &BTreeMap<String, Value>) -> (String, bool) {
    if let Some((field, default)) = SELECT_SPEC
        .iter()
        .find(|(t, _, _)| *t == orig)
        .map(|(_, f, d)| (*f, *d))
    {
        let key = fields.get(field).and_then(value_key);
        if let Some(text) = select_text(orig, key.as_deref().unwrap_or(default)) {
            return (text.to_string(), true);
        }
    }
    match rc_plain(orig) {
        Some(text) => (text.to_string(), HANDLER_BUILT_TEXTS.contains(&orig)),
        None => (zh_title(orig), true),
    }
}

/// `ZH_NAME_BY_TYPE` 兜底标题
fn zh_title(kind: &str) -> String {
    ZH_NAME_BY_TYPE
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, v)| (*v).to_string())
        .unwrap_or_else(|| kind.to_string())
}

fn mutation_xml(text: &str) -> String {
    format!("<mutation xmlns=\"{XHTML}\" items=\"0\">{text}</mutation>")
}

// ---------------------------------------------------------------- 影子 / mutation 的字符串级 XML 手术

/// 从整段 XML 取开始标签里 `attr="…"` 的值(先按引号感知找标签尾,再取属性)
/// 唯一的 XML 属性读取实现:本模块与 `neko.rs` 的 mutation 改写共用
pub(crate) fn xml_attr_value<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    attr_value(&xml[..start_tag_end(xml)?], attr)
}

/// 开始标签里 `attr="…"` 的值区间(只认双引号属性;属性名前须是空白,避免 `xname=` 误命中)
fn attr_span(tag: &str, attr: &str) -> Option<Range<usize>> {
    let bytes = tag.as_bytes();
    let mut from = 0;
    loop {
        let at = from + tag[from..].find(attr)?;
        let name_end = at + attr.len();
        if (at == 0 || bytes[at - 1].is_ascii_whitespace())
            && bytes.get(name_end) == Some(&b'=')
            && bytes.get(name_end + 1) == Some(&b'"')
        {
            let start = name_end + 2;
            return Some(start..start + tag[start..].find('"')?);
        }
        from = name_end;
    }
}

fn attr_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    attr_span(tag, attr).map(|span| &tag[span])
}

/// 替换开始标签里某个属性的值(其余字节原样保留)
fn set_attr_value(tag: &str, attr: &str, value: &str) -> String {
    match attr_span(tag, attr) {
        Some(span) => format!("{}{}{}", &tag[..span.start], value, &tag[span.end..]),
        None => tag.to_string(),
    }
}

/// `<` 到开始标签结束(跳过引号里的 `>`)
fn start_tag_end(xml: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, b) in xml.as_bytes().iter().enumerate().skip(1) {
        match b {
            b'"' => quoted = !quoted,
            b'>' if !quoted => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// `transformShadowXml`(77830):影子 XML 的 `type`、每个 `<field>` 的 `name` 与文本一起过映射表
fn transform_shadow_xml(block_type: &str, xml: &str) -> String {
    if xml.is_empty() {
        return String::new();
    }
    let Some(root_end) = start_tag_end(xml) else {
        return xml.to_string();
    }; // 不像 XML:原样保留(官方这里会产出 parsererror)
    let root = &xml[..root_end];
    let mut out = match attr_value(root, "type") {
        Some(kind) if translate_type(kind) != kind => {
            set_attr_value(root, "type", translate_type(kind))
        }
        _ => root.to_string(),
    };
    let mut rest = &xml[root_end..];
    while let Some(open) = rest.find("<field") {
        let name_end = open + "<field".len();
        if !rest[name_end..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            out.push_str(&rest[..name_end]); // `<fields…` 之类:照抄
            rest = &rest[name_end..];
            continue;
        }
        let (Some(tag_end), Some(close)) = (
            start_tag_end(&rest[open..]).map(|e| open + e),
            rest[name_end..].find("</field>").map(|c| name_end + c),
        ) else {
            break;
        };
        let tag = &rest[open..tag_end];
        out.push_str(&rest[..open]);
        match attr_value(tag, "name") {
            Some(name) => {
                let mapped = map_field_name(block_type, name);
                out.push_str(&if mapped == name {
                    tag.to_string()
                } else {
                    set_attr_value(tag, "name", &mapped)
                });
                let old_text = &rest[tag_end..close];
                out.push_str(
                    &mapped_field_text(name, old_text).unwrap_or_else(|| old_text.to_string()),
                );
            }
            None => out.push_str(tag),
        }
        out.push_str("</field>");
        rest = &rest[close + "</field>".len()..];
    }
    out.push_str(rest);
    out
}

/// 官方 `text_select_changeable` 修补:`items` 属性减一(解析失败则原样保留)
fn decrement_items(xml: &str) -> String {
    match attr_span(xml, "items").and_then(|span| {
        xml[span.clone()]
            .trim()
            .parse::<i64>()
            .ok()
            .map(|n| (span, n))
    }) {
        Some((span, n)) => format!("{}{}{}", &xml[..span.start], n - 1, &xml[span.end..]),
        None => xml.to_string(),
    }
}

/// 官方两个算术包装里共用的 `math_number` 影子串(默认值恒为 `1`)
pub(crate) fn math_number_shadow(id: &str, num: &str) -> String {
    format!(
        "<shadow xmlns=\"{XHTML}\" type=\"math_number\" id=\"{id}\" visible=\"visible\"><field constraints=\"-Infinity,Infinity,0,\" name=\"NUM\">{num}</field></shadow>"
    )
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

/// 官方 `GC` 的两个算术包装:`原值 op 常量`(横屏坐标 `/1.3`;`set_camera_alpha` 是 `100 - x`)
fn wrap_arithmetic(
    ctx: &mut Ctx,
    original: BlockJson,
    op: &str,
    constant: &str,
    original_first: bool,
    location: Option<Value>,
    parent_id: Option<String>,
) -> BlockJson {
    let wrap_id = ctx.ids.uuid();
    let (a_shadow, b_shadow) = (ctx.ids.uuid(), ctx.ids.uuid());
    let (a_id, b_id) = (ctx.ids.uuid(), ctx.ids.uuid());
    let mut original = original;
    original.parent_id = Some(wrap_id.clone()); // 官方给原节点重铸 id 并改挂到包装块
    let (input_a, input_b) = if original_first {
        original.id = Some(a_id);
        (original, math_number_node(b_id, constant, Some(wrap_id.clone())))
    } else {
        original.id = Some(b_id);
        (math_number_node(a_id, constant, Some(wrap_id.clone())), original)
    };
    BlockJson {
        kind: "math_arithmetic".to_string(),
        id: Some(wrap_id.clone()),
        location,
        shadows: BTreeMap::from([
            (String::from("A"), math_number_shadow(&a_shadow, "1")),
            (String::from("B"), math_number_shadow(&b_shadow, "1")),
        ]),
        fields: BTreeMap::from([(String::from("type"), Value::String(op.to_string()))]),
        is_output: true,
        parent_id,
        inputs: BTreeMap::from([(String::from("A"), input_a), (String::from("B"), input_b)]),
        ..Default::default()
    }
}

/// 列表积木的 `pure_list_get` 影子块(官方两处合成的公共形状)
fn pure_list_get(list: Value, parent_id: Option<String>, shadow_id: String) -> BlockJson {
    BlockJson {
        kind: "pure_list_get".to_string(),
        id: Some(shadow_id),
        is_shadow: true,
        is_output: true,
        fields: BTreeMap::from([(String::from("list"), list)]),
        parent_id,
        ..Default::default()
    }
}

fn pure_list_shadow(id: &str, list: &Value) -> String {
    format!(
        "<shadow xmlns=\"{XHTML}\" type=\"pure_list_get\" id=\"{id}\" visible=\"visible\" inline=\"true\"><field name=\"list\">{}</field></shadow>",
        js_text(Some(list))
    )
}

// ---------------------------------------------------------------- 逐积木变换(parseBlock)

struct Ctx<'a> {
    landscape: bool,
    ids: &'a mut IdSource,
    report: &'a mut TranslateReport,
}

/// 官方 `parseBlock` 的节点变换(步骤 1-10)+ 递归
fn parse_node(mut node: BlockJson, ctx: &mut Ctx) -> BlockJson {
    let orig = std::mem::take(&mut node.kind);
    let mut orig_fields = std::mem::take(&mut node.fields);
    let mut orig_shadows = std::mem::take(&mut node.shadows);
    let orig_mutation = node.mutation.take();

    // (1) 行内类型特例
    let coordinary = orig_fields.get("coordinary").and_then(value_key);
    let attribute = orig_fields
        .get("attribute")
        .filter(|v| truthy(Some(v)))
        .and_then(value_key);
    let kind = if matches!(
        orig.as_str(),
        "self_set_position" | "self_change_coordinate" | "self_glide_coordinate"
    ) {
        match coordinary.as_deref().filter(|c| *c == "x" || *c == "y") {
            Some(axis) => format!("{orig}_{axis}"),
            None => translate_type(&orig).to_string(),
        }
    } else if orig == "self_disappear" {
        "self_appear".to_string()
    } else if orig == "terminate" {
        "stop".to_string()
    } else if orig == "get_3" && attribute.is_some() {
        match process_appearance_attribute(attribute.as_deref().unwrap_or_default()) {
            // 官方把新键并进原 fields(`attribute` 一并留下,照抄)
            Some((name, value)) => {
                orig_fields.insert(name.to_string(), Value::String(value.to_string()));
                format!("{name}_of_sprite")
            }
            None => {
                orig_fields.insert(String::from("appearance"), Value::String(String::from("?")));
                translate_type(&orig).to_string()
            }
        }
    } else if orig == "self_appear" {
        orig_fields.insert(String::from("value"), Value::String(String::from("appear")));
        translate_type(&orig).to_string()
    } else if orig == "shadow_text" {
        node.mutation = Some(SHADOW_TEXT_MUTATION.to_string());
        orig_shadows.insert(String::from("MUTATE_BUTTON"), String::new());
        translate_type(&orig).to_string()
    } else {
        translate_type(&orig).to_string()
    };

    // (3) 降级占位积木的变异文本 + 执行/事件型置灰
    if rc_plain(&orig).is_some() || KITTEN_MUTATION_TEXT_SELECT.iter().any(|(k, _)| *k == orig) {
        let (text, approximate) = mutation_text(&orig, &orig_fields);
        node.mutation = Some(mutation_xml(&text));
        if approximate {
            ctx.report
                .warn(TranslateWarning::DegradedToText { kind: orig.clone() });
        }
    }
    let statement_placeholder = PLACEHOLDERS_STATEMENT.contains(&kind.as_str());
    if statement_placeholder {
        node.disabled = true;
        node.shadows = BTreeMap::from([(String::from("TITLE_HEAD"), String::new())]);
    }

    // (4) 其余按原类型的字段注入(落在 c 层:原积木一旦有字段就会被下面的整体重排覆盖——照抄官方)
    let mut fields: BTreeMap<String, Value> = match orig.as_str() {
        "self_disappear" => BTreeMap::from([(
            String::from("value"),
            Value::String(String::from("disappear")),
        )]),
        "self_gradually_show_hide" => BTreeMap::from([(
            String::from("show_hide"),
            Value::String(String::from("hide")),
        )]),
        "terminate" => BTreeMap::from([(String::from("scope"), Value::String(String::from("0")))]),
        "get_face_age" => {
            BTreeMap::from([(String::from("type"), Value::String(String::from("age")))])
        }
        "get_emotion_result" => {
            BTreeMap::from([(String::from("type"), Value::String(String::from("emotion")))])
        }
        "get_face_shape_result" => BTreeMap::from([(
            String::from("type"),
            Value::String(String::from("faceShape")),
        )]),
        _ => BTreeMap::new(),
    };

    // (5) 字段改名 + 取值映射
    if !orig_fields.is_empty() {
        fields.clear();
        for (name, value) in orig_fields {
            fields.insert(
                map_field_name(&orig, &name).into_owned(),
                map_field_value(&name, &value),
            );
        }
        // (5b) 云列表三型:list 字段变成 inputs.list 上的 pure_list_get 影子。
        //
        // **官方就是不对称的,别"修"**:官方 `list` 分支(byte 77699+)同时写
        // `inputs.list` 与 `shadows.list`,而云列表分支(byte 5922654)只写 `inputs.list`
        // 并 `delete c.fields.list`。我们照抄该不对称(见 docs/20 §3.2 的对齐实验)。
        if matches!(
            orig.as_str(),
            "cloud_lists_length" | "cloud_lists_get_value" | "cloud_lists_delete"
        ) && let Some(list) = fields.remove("list")
        {
            let shadow_id = ctx.ids.uuid();
            node.inputs.insert(
                String::from("list"),
                pure_list_get(list, node.id.clone(), shadow_id),
            );
        }
    }
    node.fields = fields;

    // (6) 影子槽改名 + 影子 XML 改写
    if !statement_placeholder && !orig_shadows.is_empty() {
        node.shadows = orig_shadows
            .into_iter()
            .map(|(slot, xml)| {
                (
                    get_mapped_name(&orig, &slot).into_owned(),
                    transform_shadow_xml(&orig, &xml),
                )
            })
            .collect();
    }

    node.kind = kind;
    // (7) `text_select_changeable` 的 items-1(官方读的是**原** mutation)
    if orig == "text_select_changeable"
        && let Some(xml) = orig_mutation.as_deref().filter(|xml| !xml.is_empty())
    {
        node.mutation = Some(decrement_items(xml));
    }

    // (9)+(10) 子连接路由
    route_children(&mut node, ctx);

    // 递归子节点;值输入/语句槽带父 id,`next` 不带(官方语义)
    let parent_id = node.id.clone();
    for child in node.inputs.values_mut() {
        let taken = std::mem::take(child);
        *child = parse_node(taken, ctx);
        child.parent_id = parent_id.clone();
    }
    for child in node.statements.values_mut() {
        let taken = std::mem::take(child);
        *child = parse_node(taken, ctx);
        child.parent_id = parent_id.clone();
    }
    if let Some(next) = node.next.take() {
        let mut parsed = parse_node(*next, ctx);
        parsed.parent_id = None;
        node.next = Some(Box::new(parsed));
    }
    node
}

/// 官方 `parseBlock` 末尾的连接路由(9/10 步):值输入 → 语句槽 / `next` / 留在 `inputs`
fn route_children(node: &mut BlockJson, ctx: &mut Ctx) {
    // 借用而非克隆:`kind` 全程只读,而下面动的都是别的字段(disjoint field borrow)
    let kind = &node.kind;
    if PLACEHOLDERS_OUTPUT.contains(&kind.as_str()) {
        return; // 官方对返值/返布尔占位积木不接任何输入连接(子积木随之消失);本实现留在原地不丢数据
    }
    let inputs = std::mem::take(&mut node.inputs)
        .into_iter()
        .map(|(slot, child)| (true, slot, child));
    let statements = std::mem::take(&mut node.statements)
        .into_iter()
        .map(|(slot, child)| (false, slot, child));

    for (from_value, slot, child) in inputs.chain(statements) {
        // (9) 列表积木:`fields.list` 变 `inputs.list`(官方对每条 input 连接都做一次)
        if from_value
            && LIST_INPUT_TYPES.contains(&kind.as_str())
            && let Some(list) = node.fields.remove("list")
        {
            let shadow_id = ctx.ids.uuid();
            node.shadows
                .insert(String::from("list"), pure_list_shadow(&shadow_id, &list));
            node.inputs.insert(
                String::from("list"),
                pure_list_get(list, node.id.clone(), shadow_id),
            );
        }
        let slot = get_mapped_name(kind, &slot).into_owned();
        if NEXT_ROUTE_TYPES.contains(&kind.as_str()) {
            node.next = Some(Box::new(child));
        } else if (kind.as_str() == "controls_if" || kind.as_str() == "when")
            && (slot.contains("DO") || (kind.as_str() == "controls_if" && slot.contains("ELSE")))
        {
            node.statements.insert(slot, child);
        } else if LOOP_DO_TYPES.contains(&kind.as_str()) && slot == "DO" {
            node.statements.insert(String::from("DO"), child);
        } else if from_value {
            node.inputs.insert(slot, child);
        } else {
            // 官方会把没命中规则的值输入一律塞进 inputs;树里语句槽按源文件 `input_type` 建的,
            // 保持原位(procedures 的 `STACK` 最终仍回语句槽)。
            node.statements.insert(slot, child);
        }
    }
}

// ============================================================================================
// 反向:KN → Kitten4(官方无此方向,本库自建;见 docs/20 §4)
// ============================================================================================
//
// 与正向**逐条对称**:正向做的每一步改名/取值/槽位/影子改写,这里都按表反查回去;
// 正向**新造**的东西(占位积木的 mutation 文本、`stop` 的 `scope`、`self_appear` 的 `value`、
// `get_3` 派生的 `_of_sprite`、`pure_list_get` 影子、横屏算术壳)在这里**拆掉**。
//
// 三条纪律:
//
// 1. **不可逆的必须进报告**:KN 原生而 Kitten 侧无来源的类型(`KN_TYPES − LC 值域` 的 68 类,
//    以及 `calculate` 这类"正向会降级"的 KN 原生块)保留原类型 + [`TranslateWarning::UnmappedBlock`];
//    一个 KN 类型有多个 Kitten 原类型时保留 KN 名(与正向的恒等分支相容)+ `DroppedProperty`;
// 2. **不做语义猜测**:云列表/本地列表(`list_append` ← `lists_append` | `cloud_lists_append`)
//    这类同名歧义一律报出来,不假装知道;
// 3. **保留即无损**:凡是我们"看不懂"的类型都**原样保留类型名**(不丢弃积木),这样
//    KN→Kitten4→KN 的类型多重集不被悄悄改写(见 `mod.rs` 的往返测试)。

/// 反向上下文(只有横屏与报告:反向映射不现铸 id)
struct RevCtx<'a> {
    landscape: bool,
    report: &'a mut TranslateReport,
}

/// 把一棵 KN 语义的积木树就地改写成 Kitten4 语义(自建反向映射)。
pub(crate) fn translate_kn_to_kitten(
    tree: &mut BlockTree,
    landscape: bool,
    report: &mut TranslateReport,
) {
    let mut ctx = RevCtx { landscape, report };
    let roots = std::mem::take(&mut tree.roots);
    tree.roots = roots
        .into_iter()
        .map(|root| reverse_node(root, &mut ctx))
        .collect();
}

/// `LC` 反转:KN 类型 → 可能的 Kitten 原类型(表内顺序)
static REVERSE_TYPES: std::sync::LazyLock<BTreeMap<&'static str, Vec<&'static str>>> =
    std::sync::LazyLock::new(|| {
        let mut out: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
        for (kitten, kn) in KITTEN_TO_KN {
            out.entry(*kn).or_default().push(*kitten);
        }
        out
    });

/// 占位积木的标题 → 原 Kitten 类型(`KITTEN_MUTATION_TEXT` + `_SELECT` 反转;176 条标题)
static REVERSE_PLACEHOLDER_TITLES: std::sync::LazyLock<BTreeMap<&'static str, Vec<&'static str>>> =
    std::sync::LazyLock::new(|| {
        let mut out: BTreeMap<&'static str, Vec<&'static str>> = BTreeMap::new();
        for (kind, text) in KITTEN_MUTATION_TEXT {
            out.entry(*text).or_default().push(*kind);
        }
        for (kind, entries) in KITTEN_MUTATION_TEXT_SELECT {
            for (_, text) in *entries {
                out.entry(*text).or_default().push(*kind);
            }
        }
        out
    });

/// 一个 KN 类型是否是"两个编辑器都可能有的名字"(Kitten 侧出现过这个名字:LC 的键/值、
/// 或正向代码里当 Kitten 侧认得的类型)→ 保留它不算"未映射"。
fn is_kitten_side(kind: &str) -> bool {
    KITTEN_TO_KN.iter().any(|(k, v)| *k == kind || *v == kind)
        || NEXT_ROUTE_TYPES.contains(&kind)
        || LOOP_DO_TYPES.contains(&kind)
        || LIST_INPUT_TYPES.contains(&kind)
        || LANDSCAPE_WRAP_TYPES.contains(&kind)
        || is_text_placeholder(kind)
        || matches!(
            kind,
            "math_number"
                | "self_set_position"
                | "self_change_coordinate"
                | "self_glide_coordinate"
                | "terminate"
                | "get_3"
                | "self_disappear"
        )
}

/// `LC` 里键值不同的类型(正向会改名):保留它们的 KN 名**不能**保证往返回来
fn is_renamed_lc_key(kind: &str) -> bool {
    KITTEN_TO_KN.iter().any(|(k, v)| *k == kind && *v != kind)
}

/// 正向类型预览:校验反演候选确实能再正向映射回同一个 KN 类型(往返安全性的硬保证)
fn forward_type_preview(kitten: &str) -> String {
    if matches!(
        kitten,
        "self_set_position" | "self_change_coordinate" | "self_glide_coordinate"
    ) || matches!(
        kitten,
        "get_3" | "self_appear" | "terminate" | "self_disappear"
    ) {
        // 这些类型的正向结果取决于字段,不参与"候选校验"(它们有自己的专门分支)
        return String::new();
    }
    translate_type(kitten).to_string()
}

/// 反向字段名表(手写补充;仅列出**歧义/与字段名不同**的条目)。
///
/// 每条都能在正向的 `FIELD_NAME_MAP`/行内特例里找到出处;括号里是真实样品
/// (`download/compile/raw/几何对战-联机.bcm4` 跑正向 pass)观察到的原文。
#[rustfmt::skip]
const KN_FIELD_BACK: &[(&str, &str, &str)] = &[
    // Kitten 的 `OP` → KN 的 `type`(math 类;`logic_compare` 除外,见 map_field_name)
    ("math_arithmetic", "type", "OP"), ("math_arithmetic_common", "type", "OP"), ("math_arithmetic_power", "type", "OP"),
    ("logic_operation", "type", "OP"), ("math_function", "type", "OP"), ("math_single", "type", "OP"),
    ("math_round", "type", "OP"), ("math_trig", "type", "OP"), ("math_trig_common", "type", "OP"), ("math_trig_arc", "type", "OP"),
    // Kitten 的 `PROPERTY` → KN 的 `type`(math_number_property)
    ("math_number_property", "type", "PROPERTY"),
    // 事件字段:`mouse_event_type`/`key_event_type` → `type`
    ("mouse_down", "type", "mouse_event_type"), ("on_keydown", "type", "key_event_type"), ("check_key", "type", "key_event_type"),
    // 广播:KN `message` 可能是 Kitten 的 `MESSAGE` 或 `broadcast`
    ("broadcast_input", "message", "MESSAGE"), ("self_broadcast", "message", "MESSAGE"), ("self_broadcast_and_wait", "message", "MESSAGE"),
    ("broadcast", "message", "MESSAGE"),
    // 造型/音频引用:同名但来自不同原字段
    ("get_audios", "audio_id", "sound_id"), ("get_styles", "style_id", "style_id"), ("mirror", "sprite", "sprite"),
    // 碰撞积木的 sprite1/sprite2 → sprite/sprite1
    ("bump", "sprite", "sprite1"), ("bump", "sprite1", "sprite2"),
    // get_3 家族:`sprite` 是原字段,`attribute` 保留(取值另表)
    ("get_3", "sprite", "sprite"),
];

/// KN 字段名 → Kitten 字段名(按**目标 Kitten 类型**消歧)
fn reverse_field_name(kitten: &str, kn_name: &str) -> String {
    if let Some((_, _, back)) = KN_FIELD_BACK
        .iter()
        .find(|(t, n, _)| *t == kitten && *n == kn_name)
    {
        return (*back).to_string();
    }
    // 变量/列表积木的 `VAR`(`map_field_name` 的行内特例:`variable`/`list`/`valname` 都源自 `VAR`)
    match kn_name {
        "variable" | "list" => return "VAR".to_string(),
        "valname" => return "valname".to_string(),
        _ => {}
    }
    // `FIELD_NAME_MAP` 的反转(值 → 键);歧义时优先"本来就是这个名字"
    if map_field_name(kitten, kn_name) == kn_name
        && !FIELD_NAME_MAP.iter().any(|(_, v)| *v == kn_name)
    {
        return kn_name.to_string();
    }
    let mut candidates: Vec<&str> = FIELD_NAME_MAP
        .iter()
        .filter(|(_, v)| *v == kn_name)
        .map(|(k, _)| *k)
        .collect();
    if kn_name == "type" && !candidates.contains(&"OP") {
        candidates.push("OP");
    }
    match candidates.len() {
        0 => kn_name.to_string(),
        1 => candidates[0].to_string(),
        _ => {
            if map_field_name(kitten, kn_name) == kn_name {
                kn_name.to_string()
            } else {
                candidates[0].to_string()
            }
        }
    }
}

/// KN 取值 → Kitten 原取值(按**Kitten 字段名**反查 `SPECIAL_FIELD_VALUES`)
fn reverse_field_value(kitten_field: &str, value: &Value) -> Value {
    match value_key(value)
        .as_deref()
        .and_then(|text| unmapped_field_text(kitten_field, text))
    {
        Some(back) => Value::String(back),
        None => value.clone(),
    }
}

/// 取值表反查:`SPECIAL_FIELD_VALUES[name]` 里映射结果为 `text` 的源值
fn unmapped_field_text(name: &str, text: &str) -> Option<String> {
    let entries = SPECIAL_FIELD_VALUES.iter().find(|(k, _)| *k == name)?.1;
    entries
        .iter()
        .find(|(_, v)| *v == text)
        .map(|(k, _)| (*k).to_string())
}

/// KN 输入/影子槽位名 → Kitten 槽位名(按 `INPUT_NAME_MAP` 反查;`key` 是正向 lookup 用的类型)
fn reverse_slot_name(key: &str, kn_slot: &str) -> String {
    let Some((_, entries)) = INPUT_NAME_MAP.iter().find(|(k, _)| *k == key) else {
        return kn_slot.to_string();
    };
    let hits: Vec<&str> = entries
        .iter()
        .filter(|(_, mapped)| *mapped == kn_slot)
        .map(|(orig, _)| *orig)
        .collect();
    match hits.as_slice() {
        [only] => (*only).to_string(),
        _ => kn_slot.to_string(),
    }
}

/// 影子 XML 的字段级反演:`type` 属性与每个 `<field>` 的名字/取值
fn untransform_shadow_xml(kitten_type: &str, xml: &str) -> String {
    if xml.is_empty() {
        return String::new();
    }
    let Some(root_end) = start_tag_end(xml) else {
        return xml.to_string();
    };
    let root = &xml[..root_end];
    let mut out = match attr_value(root, "type") {
        // `math_number` 是 Kitten4 影子自己的类型(`LC` 的唯一原类型 `default_value` 是**块**形态),
        // 影子 XML 里保持原名
        Some("math_number") => root.to_string(),
        Some(kind) => {
            let back = REVERSE_TYPES
                .get(kind)
                .filter(|candidates| candidates.len() == 1)
                .map(|candidates| candidates[0]);
            match back {
                Some(back) if back != kind => set_attr_value(root, "type", back),
                _ => root.to_string(),
            }
        }
        None => root.to_string(),
    };
    let mut rest = &xml[root_end..];
    while let Some(open) = rest.find("<field") {
        let name_end = open + "<field".len();
        if !rest[name_end..].starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            out.push_str(&rest[..name_end]);
            rest = &rest[name_end..];
            continue;
        }
        let (Some(tag_end), Some(close)) = (
            start_tag_end(&rest[open..]).map(|e| open + e),
            rest[name_end..].find("</field>").map(|c| name_end + c),
        ) else {
            break;
        };
        let tag = &rest[open..tag_end];
        out.push_str(&rest[..open]);
        match attr_value(tag, "name") {
            Some(name) => {
                let back = reverse_field_name(kitten_type, name);
                out.push_str(&if back == name {
                    tag.to_string()
                } else {
                    set_attr_value(tag, "name", &back)
                });
                let old_text = &rest[tag_end..close];
                out.push_str(
                    &unmapped_field_text(&back, old_text).unwrap_or_else(|| old_text.to_string()),
                );
            }
            None => out.push_str(tag),
        }
        out.push_str("</field>");
        rest = &rest[close + "</field>".len()..];
    }
    out.push_str(rest);
    out
}

/// `text_select_changeable` 的修补逆向:`items` 属性加一
fn increment_items(xml: &str) -> String {
    match attr_span(xml, "items").and_then(|span| {
        xml[span.clone()]
            .trim()
            .parse::<i64>()
            .ok()
            .map(|n| (span, n))
    }) {
        Some((span, n)) => format!("{}{}{}", &xml[..span.start], n + 1, &xml[span.end..]),
        None => xml.to_string(),
    }
}

/// `<mutation …>正文</mutation>` 的正文(占位积木的降级文本就存在这里)
fn mutation_body(xml: &str) -> &str {
    let Some(open) = xml.find('>') else { return "" };
    let rest = &xml[open + 1..];
    match rest.rfind("</mutation>") {
        Some(close) => &rest[..close],
        None => rest,
    }
}

/// 单个节点的反向改写(步骤与正向 `parse_node`/`gc_node` 对称)
fn reverse_node(mut node: BlockJson, ctx: &mut RevCtx) -> BlockJson {
    let kn_kind = std::mem::take(&mut node.kind);
    if kn_kind.is_empty() {
        node.kind = kn_kind;
        return node;
    }

    // (1) 正向 `GC` 的算术壳:横屏坐标 `/1.3`、`set_camera_alpha` 的 `100 - x`
    unwrap_arithmetic_wrappers(&mut node, &kn_kind, ctx.landscape);

    // (2) 列表积木:`inputs.list` 上的 `pure_list_get` 影子折回 `fields.list`(向前是 `VAR`)
    fold_pure_list_get(&mut node, &kn_kind);

    // (3) 类型反演(gc 的 `attribute` 补齐要等字段反演之后再写,否则会被取值表再搬一次)
    let (kitten_kind, get3_attribute) = reverse_kind(&kn_kind, &mut node, ctx);

    // (4) 字段名 + 取值反演
    reverse_fields(&mut node, &kitten_kind);

    // (5) 类型级字段补齐(必须在 (4) 之后)
    if let Some(attribute) = get3_attribute {
        node.fields.insert(
            String::from("attribute"),
            Value::String(attribute.to_string()),
        );
    }
    finalize_kind_fields(&kn_kind, &kitten_kind, &mut node);

    // (6) 影子槽位名 + 影子 XML 反演
    let shadows = std::mem::take(&mut node.shadows);
    for (slot, xml) in shadows {
        let back_slot = reverse_slot_name(&kitten_kind, &slot);
        node.shadows
            .insert(back_slot, untransform_shadow_xml(&kitten_kind, &xml));
    }

    // (7) 输入/语句槽位名反演(正向 `route_children` 的 lookup 键是**KN 类型**)
    let inputs = std::mem::take(&mut node.inputs);
    for (slot, child) in inputs {
        node.inputs
            .insert(reverse_slot_name(&kn_kind, &slot), child);
    }
    let statements = std::mem::take(&mut node.statements);
    for (slot, child) in statements {
        node.statements
            .insert(reverse_slot_name(&kn_kind, &slot), child);
    }

    // (8) mutation 反演(`text_select` → `text_select_changeable` 的 items+1)
    if kitten_kind == "text_select_changeable"
        && let Some(xml) = node.mutation.as_deref().filter(|xml| !xml.is_empty())
    {
        node.mutation = Some(increment_items(xml));
    }

    node.kind = kitten_kind;

    // (8) 递归
    for child in node.inputs.values_mut() {
        let taken = std::mem::take(child);
        *child = reverse_node(taken, ctx);
    }
    for child in node.statements.values_mut() {
        let taken = std::mem::take(child);
        *child = reverse_node(taken, ctx);
    }
    if let Some(next) = node.next.take() {
        node.next = Some(Box::new(reverse_node(*next, ctx)));
    }
    node
}

/// 类型反演:返回 (Kitten 侧类型名, `get_3` 需要补的 `attribute` 取值)
fn reverse_kind(
    kn_kind: &str,
    node: &mut BlockJson,
    ctx: &mut RevCtx,
) -> (String, Option<&'static str>) {
    // (a) 文本占位积木:从 mutation 正文反查原 Kitten 类型(`RC` 是"中文标题 → 原类型"的可逆表)
    if is_text_placeholder(kn_kind) {
        return (reverse_placeholder(kn_kind, node, ctx), None);
    }
    // (b) `GC` 的两个类型级特例
    if kn_kind == "stop" {
        // 正向:`terminate` → `stop` + `fields.scope="0"`
        if node.fields.get("scope").and_then(value_key).as_deref() == Some("0") {
            node.fields.remove("scope");
        } else if let Some(scope) = node.fields.get("scope") {
            ctx.report.warn(TranslateWarning::DroppedField {
                path: format!("stop.scope={}", js_text(Some(scope))),
            });
        }
        return ("terminate".to_string(), None);
    }
    if kn_kind == "self_appear" {
        // 正向:`self_disappear` → `self_appear`+`value=disappear`;`self_appear` → `value=appear`
        return match node.fields.get("value").and_then(value_key).as_deref() {
            Some("disappear") => {
                node.fields.remove("value");
                ("self_disappear".to_string(), None)
            }
            Some("appear") => {
                node.fields.remove("value");
                ("self_appear".to_string(), None)
            }
            Some(other) => {
                ctx.report.warn(TranslateWarning::DroppedField {
                    path: format!("self_appear.value={other}"),
                });
                ("self_appear".to_string(), None)
            }
            None => ("self_appear".to_string(), None),
        };
    }
    // (c) 坐标拆分形态:`self_set_position_x` → `self_set_position` + `coordinary`(补齐见 finalize)
    for base in [
        "self_set_position",
        "self_change_coordinate",
        "self_glide_coordinate",
    ] {
        if kn_kind
            .strip_prefix(base)
            .and_then(|rest| rest.strip_prefix('_'))
            .is_some_and(|axis| axis == "x" || axis == "y")
        {
            return (base.to_string(), None);
        }
    }
    // (d) `get_3` 派生形态:`*_of_sprite` → `get_3` + `attribute`
    if let Some(base) = kn_kind.strip_suffix("_of_sprite")
        && let Some(attribute) = reverse_appearance_attribute(base, &node.fields)
    {
        return ("get_3".to_string(), Some(attribute));
    }
    // (e) `shadow_text` 的正向形态(`items="1"` + `MUTATE_BUTTON` 影子)
    if kn_kind == "text_join"
        && (node.mutation.as_deref() == Some(SHADOW_TEXT_MUTATION)
            || node.shadows.contains_key("MUTATE_BUTTON"))
    {
        node.mutation = Some(SHADOW_TEXT_MUTATION.to_string());
        return ("shadow_text".to_string(), None);
    }
    // (e2) `math_number`:`LC` 的唯一原类型是 `default_value`,但**影子块**在 Kitten4 里就是
    // `math_number`(影子 XML 的 `type="math_number"` 要与积木本身一致),故影子保持原名
    if kn_kind == "math_number" && node.is_shadow {
        return ("math_number".to_string(), None);
    }
    // (f) `LC` 反演
    match REVERSE_TYPES.get(kn_kind).map(Vec::as_slice) {
        Some([only]) => ((*only).to_string(), None),
        Some(candidates) => {
            // 歧义:能安全保留 KN 名(它本身就是 Kitten 侧的名字)就保留,否则按"非云优先"挑一个
            if !is_renamed_lc_key(kn_kind) {
                ctx.report.warn(TranslateWarning::DroppedProperty {
                    path: format!(
                        "{kn_kind}(Kitten 原类型有 {} 个:{},已保留 KN 名)",
                        candidates.len(),
                        candidates.join("|")
                    ),
                });
                (kn_kind.to_string(), None)
            } else {
                let chosen = candidates
                    .iter()
                    .find(|c| !c.starts_with("cloud_"))
                    .unwrap_or(&candidates[0]);
                ctx.report.warn(TranslateWarning::DroppedProperty {
                    path: format!(
                        "{kn_kind}(Kitten 原类型有 {} 个:{},已取 {chosen})",
                        candidates.len(),
                        candidates.join("|")
                    ),
                });
                ((*chosen).to_string(), None)
            }
        }
        // 不是 LC 值:可能是 LC 的键(恒等 → 保留;会改名 → 不可逆)、也可能完全在表外
        None => {
            if is_renamed_lc_key(kn_kind) || !is_kitten_side(kn_kind) {
                ctx.report.warn(TranslateWarning::UnmappedBlock {
                    kind: kn_kind.to_string(),
                });
            }
            (kn_kind.to_string(), None)
        }
    }
}

/// 占位积木 → 原 Kitten 类型(`RC` 标题反查 + 正向校验)
fn reverse_placeholder(kn_kind: &str, node: &mut BlockJson, ctx: &mut RevCtx) -> String {
    let title = mutation_body(node.mutation.as_deref().unwrap_or_default()).to_string();
    let hit = REVERSE_PLACEHOLDER_TITLES
        .get(title.as_str())
        .and_then(|candidates| {
            candidates
                .iter()
                .find(|candidate| forward_type_preview(candidate) == kn_kind)
        });
    match hit {
        Some(kitten) => {
            // 正向会重新生成 mutation;影子里的 TITLE_HEAD 也是正向注入的
            node.mutation = None;
            node.shadows.remove("TITLE_HEAD");
            (*kitten).to_string()
        }
        None => {
            ctx.report.warn(TranslateWarning::UnmappedBlock {
                kind: kn_kind.to_string(),
            });
            kn_kind.to_string()
        }
    }
}

/// `processAppearanceAttribute`(77313)的反查:`*_of_sprite` + 字段 → `get_3` 的 `attribute` 取值
fn reverse_appearance_attribute(
    base: &str,
    fields: &BTreeMap<String, Value>,
) -> Option<&'static str> {
    let (field, value) = match base {
        "coordinate" => ("coordinate", fields.get("coordinate")),
        "style" => ("style", fields.get("style")),
        "appearance" => ("appearance", fields.get("appearance")),
        "effect" => ("effect", fields.get("effect")),
        _ => return None,
    };
    let value = value.and_then(value_key)?;
    APPEARANCE_ATTRIBUTE
        .iter()
        .find(|(_, (name, mapped))| *name == field && *mapped == value.as_ref())
        .map(|(key, _)| *key)
}

/// 类型级字段补齐(在字段反演之后跑;这些字段是正向从别的字段派生的,不属于源)
fn finalize_kind_fields(kn_kind: &str, kitten_kind: &str, node: &mut BlockJson) {
    if kitten_kind == "get_3" {
        // `coordinate`/`style`/`appearance`/`effect` 是正向按 `attribute` 派生的,`attribute` 才是源字段
        node.fields.remove("coordinate");
        node.fields.remove("style");
        node.fields.remove("appearance");
        node.fields.remove("effect");
        return;
    }
    if let Some(axis) = [
        "self_set_position",
        "self_change_coordinate",
        "self_glide_coordinate",
    ]
    .iter()
    .find_map(|base| {
        kn_kind
            .strip_prefix(base)
            .and_then(|rest| rest.strip_prefix('_'))
            .filter(|a| *a == "x" || *a == "y")
    }) {
        node.fields
            .entry(String::from("coordinary"))
            .or_insert_with(|| Value::String(axis.to_string()));
    }
}

/// 字段名 + 取值反演
fn reverse_fields(node: &mut BlockJson, kitten_kind: &str) {
    let fields = std::mem::take(&mut node.fields);
    for (name, value) in fields {
        let back_name = reverse_field_name(kitten_kind, &name);
        let back_value = reverse_field_value(&back_name, &value);
        node.fields.insert(back_name, back_value);
    }
}

/// 正向 `GC` 的算术壳拆解(按 KN 形态识别,识别不出就原样留着)
fn unwrap_arithmetic_wrappers(node: &mut BlockJson, kn_kind: &str, landscape: bool) {
    if landscape && LANDSCAPE_WRAP_TYPES.contains(&kn_kind) {
        let slots: &[&str] = if matches!(kn_kind, "self_move_to" | "self_glide_to") {
            &["x", "y"]
        } else {
            &["value", "steps"]
        };
        for slot in slots {
            if let Some(inner) = unwrap_arithmetic(node.inputs.get(*slot), "divide", "1.3", true) {
                node.inputs.insert((*slot).to_string(), inner);
            }
        }
    }
    if kn_kind == "set_camera_alpha"
        && let Some(inner) =
            unwrap_arithmetic(node.inputs.get("camera_alpha"), "minus", "100", false)
    {
        node.inputs.insert(String::from("camera_alpha"), inner);
    }
}

/// 识别正向 `wrap_arithmetic` 造出来的壳,返回内层原值
fn unwrap_arithmetic(
    wrapper: Option<&BlockJson>,
    op: &str,
    constant: &str,
    original_first: bool,
) -> Option<BlockJson> {
    let wrapper = wrapper?;
    if wrapper.kind != "math_arithmetic"
        || wrapper.fields.get("type").and_then(value_key).as_deref() != Some(op)
    {
        return None;
    }
    let (original_slot, constant_slot) = if original_first {
        ("A", "B")
    } else {
        ("B", "A")
    };
    let constant_node = wrapper.inputs.get(constant_slot)?;
    if constant_node.kind != "math_number"
        || constant_node
            .fields
            .get("NUM")
            .and_then(value_key)
            .as_deref()
            != Some(constant)
    {
        return None;
    }
    let mut inner = wrapper.inputs.get(original_slot)?.clone();
    if inner.kind == "math_arithmetic" && inner.id.is_some() && inner.id == wrapper.id {
        return None; // 自引用保护
    }
    if inner.location.is_none() {
        inner.location = wrapper.location.clone();
    }
    Some(inner)
}

/// 列表积木:`inputs.list` 上的 `pure_list_get` 影子折回 `fields.list`(Kitten 侧是 `VAR`)
fn fold_pure_list_get(node: &mut BlockJson, kn_kind: &str) {
    let foldable = LIST_INPUT_TYPES.contains(&kn_kind)
        || matches!(
            kn_kind,
            "cloud_lists_length" | "cloud_lists_get_value" | "cloud_lists_delete"
        );
    if !foldable {
        return;
    }
    let Some(list_input) = node.inputs.get("list") else {
        return;
    };
    if list_input.kind != "pure_list_get" || !list_input.is_shadow {
        return;
    }
    let Some(list) = list_input.fields.get("list").cloned() else {
        return;
    };
    node.fields.insert(String::from("list"), list);
    node.inputs.remove("list");
    node.shadows.remove("list");
}

// ---------------------------------------------------------------- 后置特例(GC)

/// 官方 `GC` 的深度遍历:先本节点、再子节点
fn gc_deep(mut node: BlockJson, ctx: &mut Ctx) -> BlockJson {
    node = gc_node(node, ctx);
    for child in node.inputs.values_mut() {
        let taken = std::mem::take(child);
        *child = gc_deep(taken, ctx);
    }
    for child in node.statements.values_mut() {
        let taken = std::mem::take(child);
        *child = gc_deep(taken, ctx);
    }
    if let Some(next) = node.next.take() {
        node.next = Some(Box::new(gc_deep(*next, ctx)));
    }
    node
}

fn gc_node(mut node: BlockJson, ctx: &mut Ctx) -> BlockJson {
    // `shadow_number` 拆包 / 降级
    if node.kind == "shadow_number"
        && let Some(value) = node.inputs.remove("VALUE")
    {
        let num = value.fields.get("NUM").cloned();
        let unwrapped = gc_node(value, ctx);
        if unwrapped.kind != "math_number" || node.parent_id.is_some() {
            let mut out = unwrapped;
            out.location = node.location;
            return out;
        }
        let mutation = mutation_xml(&format!("展示数字{}", js_text(num.as_ref())));
        return BlockJson {
            kind: "bcm_translator_text_return_value_block".to_string(),
            id: node.id,
            location: node.location,
            mutation: Some(mutation),
            shadows: node.shadows,
            ..Default::default()
        };
    }
    // `appearance_of_sprite` + UUID 属性 → 变量回读(有父) / 文本占位(无父)
    if node.kind == "appearance_of_sprite" {
        let attribute = node
            .fields
            .get("attribute")
            .filter(|v| truthy(Some(v)))
            .and_then(value_key)
            .filter(|a| is_uuid(a));
        if let Some(attribute) = attribute {
            let (id, location) = (node.id.take(), node.location.take());
            if node.parent_id.is_some() {
                return BlockJson {
                    kind: "variables_get".to_string(),
                    id,
                    location,
                    mutation: Some(String::new()),
                    fields: BTreeMap::from([(
                        String::from("variable"),
                        Value::String(attribute.into_owned()),
                    )]),
                    parent_id: node.parent_id,
                    ..Default::default()
                };
            }
            let mutation = mutation_xml(&format!("自己的变量{attribute}"));
            return BlockJson {
                kind: "bcm_translator_text_return_value_block".to_string(),
                id,
                location,
                mutation: Some(mutation),
                ..Default::default()
            };
        }
    }
    // 横屏坐标:包一层 `math_arithmetic`(divide 1.3)
    if ctx.landscape && LANDSCAPE_WRAP_TYPES.contains(&node.kind.as_str()) {
        let slots: &[&str] = if matches!(node.kind.as_str(), "self_move_to" | "self_glide_to") {
            &["x", "y"]
        } else {
            &["value", "steps"]
        };
        for slot in slots {
            if let Some(child) = node.inputs.remove(*slot) {
                let wrap =
                    wrap_arithmetic(ctx, child, "divide", "1.3", true, None, node.id.clone());
                node.inputs.insert((*slot).to_string(), wrap);
            }
        }
    }
    // `set_camera_alpha`:包一层 `100 - x`(官方还按原输入坐标偏移包装块)
    if node.kind == "set_camera_alpha"
        && let Some(child) = node.inputs.remove("camera_alpha")
    {
        let location = child
            .location
            .as_ref()
            .and_then(Value::as_array)
            .and_then(|pair| match (pair.first(), pair.get(1)) {
                (Some(x), Some(y)) => x
                    .as_f64()
                    .zip(y.as_f64())
                    .map(|(x, y)| json!([x + 150.0, y + 20.0])),
                _ => None,
            });
        let wrap = wrap_arithmetic(ctx, child, "minus", "100", false, location, node.id.clone());
        node.inputs.insert(String::from("camera_alpha"), wrap);
    }
    // `list_append` 插到首项 → 降级为文本占位(官方只在值积木同时带 `NUM`/`list` 时才动手)
    if node.kind == "list_append"
        && node.fields.get("POS").and_then(value_key).as_deref() == Some("first")
    {
        let item = node.inputs.get("list_item_value");
        let num = item.and_then(|b| b.fields.get("NUM")).cloned();
        let item_list = item.and_then(|b| b.fields.get("list")).cloned();
        if num.is_some() && item_list.is_some() {
            let list = node
                .inputs
                .get("list")
                .and_then(|b| b.fields.get("list"))
                .cloned();
            node.kind = "bcm_translator_text_execution_block".to_string();
            node.disabled = true;
            node.mutation = Some(mutation_xml(&format!(
                "添加  {} 到 {} 首项",
                js_text(num.as_ref()),
                js_text(list.as_ref())
            )));
            node.shadows = BTreeMap::from([(String::from("TITLE_HEAD"), String::new())]);
        }
    }
    // 循环体内(语句链)的音频积木置灰
    if matches!(
        node.kind.as_str(),
        "repeat_forever" | "repeat_n_times" | "repeat_forever_until"
    ) && let Some(body) = node.statements.get_mut("DO")
    {
        disable_audio_in_chain(body);
    }
    node
}

/// 官方 `GC` 里对循环体的置灰:只沿 `statements` 与 `next` 走,不进 `inputs`
fn disable_audio_in_chain(node: &mut BlockJson) {
    if matches!(node.kind.as_str(), "play_words_audio" | "play_audio") {
        node.disabled = true;
    }
    for child in node.statements.values_mut() {
        disable_audio_in_chain(child);
    }
    if let Some(next) = &mut node.next {
        disable_audio_in_chain(next);
    }
}

#[cfg(test)]
mod tests {
    /// P0-3 回归:标签里前一个属性值含 `>` 时,标签尾必须按引号感知找。
    /// 旧实现(neko.rs 的 `xml.find('>')`)会把标签截断在引号内的 `>`,导致后面的属性读不到。
    #[test]
    fn xml_attr_value_is_quote_aware_about_tag_end() {
        let xml = r#"<mutation items="2" def_id="a>b" name="x">"#;
        assert_eq!(xml_attr_value(xml, "name"), Some("x"));
        assert_eq!(xml_attr_value(xml, "def_id"), Some("a>b"));
        assert_eq!(xml_attr_value(xml, "items"), Some("2"));
        assert_eq!(xml_attr_value(xml, "missing"), None);
    }

    use super::*;
    use crate::core::convert::shared::EditorType;
    use crate::core::convert::translate::TargetEditor;
    use serde_json::json;

    /// 把一个积木当根跑整条映射管线
    fn run(block: Value, landscape: bool) -> (BlockJson, TranslateReport) {
        let mut tree = BlockTree::new(vec![BlockJson::from_value(&block).expect("块 JSON")]);
        let mut ids = IdSource::new(true);
        let mut report = TranslateReport::new(EditorType::Kitten4, TargetEditor::KittenN);
        translate_kitten_to_kn(&mut tree, landscape, &mut ids, &mut report);
        (tree.roots.remove(0), report)
    }

    fn field<'a>(node: &'a BlockJson, name: &str) -> Option<&'a str> {
        node.fields.get(name).and_then(Value::as_str)
    }

    fn slots<V>(map: &BTreeMap<String, V>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }

    #[test]
    fn lc_rename_hits_and_identity_fallback() {
        assert_eq!(translate_type("bump"), "bump_into");
        assert_eq!(
            translate_type("start_on_click"),
            "on_running_group_activated"
        );
        assert_eq!(translate_type("math_arithmetic"), "math_arithmetic"); // 表里没有 → 原值
        assert!(is_text_placeholder("bcm_translator_text_execution_block"));
        assert!(!is_text_placeholder("math_arithmetic"));
        assert!(
            shadow_xml("set_camera_alpha", "camera_alpha")
                .unwrap()
                .contains("camera_alpha_slider")
        );
        assert_eq!(shadow_xml("math_arithmetic", "A"), None);
        // 槽位表:PARAMS{n} 后移一位,PROCEDURES_2_DEFNORETURN_MUTATOR → PARAMS0
        assert_eq!(
            get_mapped_name("procedures_2_defnoreturn", "PARAMS0"),
            "PARAMS1"
        );
        assert_eq!(
            get_mapped_name(
                "procedures_2_defnoreturn",
                "PROCEDURES_2_DEFNORETURN_MUTATOR"
            ),
            "PARAMS0"
        );
        assert_eq!(get_mapped_name("math_arithmetic", "A"), "A");
    }

    #[test]
    fn placeholder_degrades_with_title_head_and_rc_text() {
        // RC 纯字符串项:逐字照搬,无告警
        let (node, report) = run(json!({"type": "ai_lab_add_data", "id": "a"}), false);
        assert_eq!(node.kind, "bcm_translator_text_execution_block");
        assert!(node.disabled);
        assert_eq!(node.shadows.get("TITLE_HEAD").map(String::as_str), Some(""));
        assert_eq!(
            node.mutation.unwrap(),
            "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"0\">未命名模型: 添加训练数据 特征1{value} 到{value1}</mutation>"
        );
        assert!(report.warnings().is_empty());
        // 官方由 handler 拼词的型号:文字取表 + 记降级
        let (node, report) = run(json!({"type": "clone", "id": "b"}), false);
        assert!(
            node.mutation
                .unwrap()
                .contains("分裂 {sprite_id} 到 x {x} y {y}")
        );
        assert_eq!(
            report.warnings(),
            [TranslateWarning::DegradedToText {
                kind: "clone".into()
            }]
        );
    }

    #[test]
    fn self_disappear_becomes_appear_with_value() {
        let (node, _) = run(json!({"type": "self_disappear", "id": "a"}), false);
        assert_eq!(node.kind, "self_appear");
        assert_eq!(field(&node, "value"), Some("disappear"));
        // 原积木自带字段时官方会整体重排 c.fields,注入被覆盖(照抄)
        let (node, _) = run(
            json!({"type": "self_disappear", "id": "b", "fields": {"scope": "1"}}),
            false,
        );
        assert_eq!(slots(&node.fields), vec!["scope"]);
    }

    #[test]
    fn get_3_attribute_split_and_unknown_fallback() {
        let cases = [
            ("0", "coordinate_of_sprite", "coordinate", "x", "x"),
            ("5", "appearance_of_sprite", "appearance", "scale", "scale"),
            ("7", "effect_of_sprite", "effect", "0", "0"),
            ("16", "appearance_of_sprite", "appearance", "width", "width"),
        ];
        for (attribute, kind, name, value, attr_value) in cases {
            let (node, _) = run(
                json!({"type": "get_3", "id": "a", "fields": {"sprite": "__self", "attribute": attribute}}),
                false,
            );
            assert_eq!(node.kind, kind, "attribute={attribute}");
            assert_eq!(field(&node, name), Some(value), "attribute={attribute}");
            // 官方把新键并进原 fields:`sprite` 留着,`attribute` 自身也过一遍取值映射表
            assert_eq!(field(&node, "sprite"), Some("--self"));
            assert_eq!(
                field(&node, "attribute"),
                Some(attr_value),
                "attribute={attribute}"
            );
        }
        // 表外取值:注入 appearance="?" 并不改类型
        let (node, _) = run(
            json!({"type": "get_3", "id": "c", "fields": {"attribute": "99"}}),
            false,
        );
        assert_eq!(node.kind, "appearance_of_sprite");
        assert_eq!(field(&node, "appearance"), Some("?"));
        assert_eq!(field(&node, "attribute"), Some("99"));
    }

    #[test]
    fn maps_field_names_and_enum_values() {
        let (node, _) = run(
            json!({"type": "math_arithmetic", "id": "a", "fields": {"OP": "MULTIPLY"}}),
            false,
        );
        assert_eq!(field(&node, "type"), Some("multiply"));
        // logic_compare 的 OP 名字保留,取值仍过映射表
        let (node, _) = run(
            json!({"type": "logic_compare", "id": "b", "fields": {"OP": "ADD"}}),
            false,
        );
        assert_eq!(field(&node, "OP"), Some("add"));
        // 变量/列表积木的 VAR 分流
        let (node, _) = run(
            json!({"type": "variables_set", "id": "c", "fields": {"VAR": "v1"}}),
            false,
        );
        assert_eq!(field(&node, "variable"), Some("v1"));
        // set_sprite_style 的 index→style_id 是**槽位**改名(官方 mapFieldName 里没有 index)
        let style_shadow = "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"get_styles\" id=\"s\" visible=\"visible\"><field name=\"index\">3</field></shadow>";
        let (node, _) = run(
            json!({"type": "set_sprite_style", "id": "d", "shadows": {"index": style_shadow}}),
            false,
        );
        assert_eq!(slots(&node.shadows), vec!["style_id"]);
        assert_eq!(node.shadows["style_id"], style_shadow);
        let (node, _) = run(
            json!({"type": "mouse_down", "id": "e", "fields": {"mouse_event_type": "down"}}),
            false,
        );
        assert_eq!(field(&node, "type"), Some("down"));
        // 值映射:sprite 的 __self/__mouse
        let (node, _) = run(
            json!({"type": "self_set_position", "id": "f", "fields": {"coordinary": "x", "sprite": "__mouse"}}),
            false,
        );
        assert_eq!(node.kind, "self_set_position_x");
        assert_eq!(field(&node, "sprite"), Some("--mouse"));
    }

    #[test]
    fn rewrites_shadow_slots_fields_and_values() {
        // 槽名 VALUE → ADD0,影子内类型过 LC,`{text_join}` 的 items=1 变异 + MUTATE_BUTTON
        let xml = "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"math_number\" id=\"s1\" visible=\"visible\"><field name=\"NUM\">0</field></shadow>";
        let (node, _) = run(
            json!({"type": "shadow_text", "id": "a", "shadows": {"VALUE": xml}}),
            false,
        );
        assert_eq!(node.kind, "text_join");
        assert_eq!(node.mutation.as_deref(), Some(SHADOW_TEXT_MUTATION));
        assert_eq!(slots(&node.shadows), vec!["ADD0", "MUTATE_BUTTON"]);
        assert_eq!(node.shadows["ADD0"], xml); // 未命中改名/取值 → 字节不变
        assert_eq!(node.shadows["MUTATE_BUTTON"], "");
        // 影子内字段名 + 取值改写
        let xml = "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"get_audios\" id=\"s2\" visible=\"visible\"><field name=\"sound_id\">?</field><field name=\"OP\">MULTIPLY</field></shadow>";
        let (node, _) = run(
            json!({"type": "play_audio_and_wait_2", "id": "b", "shadows": {"audio": xml}}),
            false,
        );
        assert_eq!(
            node.shadows["audio_id"],
            "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"get_play_audio\" id=\"s2\" visible=\"visible\"><field name=\"audio_id\">?</field><field name=\"type\">multiply</field></shadow>"
        );
    }

    #[test]
    fn routes_children_by_official_type_lists() {
        // repeat_n_times:值输入 DO → statements.DO,且带父 id
        let (node, _) = run(
            json!({"type": "repeat_n_times", "id": "a", "inputs": {"DO": {"type": "wait_until", "id": "b"}}}),
            false,
        );
        assert!(node.inputs.is_empty());
        assert_eq!(slots(&node.statements), vec!["DO"]);
        assert_eq!(node.statements["DO"].parent_id.as_deref(), Some("a"));
        // controls_if:DO0/ELSE 都进 statements
        let (node, _) = run(
            json!({"type": "controls_if", "id": "c", "inputs": {"DO0": {"type": "wait_until", "id": "d"}, "ELSE": {"type": "wait_until", "id": "e"}}}),
            false,
        );
        assert_eq!(slots(&node.statements), vec!["DO0", "ELSE"]);
        // 事件帽的值输入官方挂到 next(且不带 parent_id)
        let (node, _) = run(
            json!({"type": "sprite_on_tap", "id": "f", "inputs": {"DO": {"type": "wait_until", "id": "g"}}}),
            false,
        );
        assert_eq!(
            node.next.as_ref().map(|n| n.kind.as_str()),
            Some("wait_until")
        );
        assert!(node.next.as_ref().unwrap().parent_id.is_none());
        // 普通值输入按槽位表改名后留在 inputs
        let (node, _) = run(
            json!({"type": "logic_compare", "id": "h", "inputs": {"a": {"type": "math_number", "id": "i"}}}),
            false,
        );
        assert_eq!(slots(&node.inputs), vec!["A"]);
        // 官方 h 列表(返值占位)不接输入连接 → 子积木在官方产物里直接消失;本实现留在原地(不丢数据、槽名不改)
        let (node, _) = run(
            json!({"type": "calculate", "id": "j", "inputs": {"a": {"type": "math_number", "id": "k"}}}),
            false,
        );
        assert_eq!(node.kind, "bcm_translator_text_return_value_block");
        assert_eq!(slots(&node.inputs), vec!["a"]);
    }

    #[test]
    fn landscape_wraps_coordinate_inputs_only_when_landscape() {
        let block = json!({"type": "self_move_to", "id": "a", "inputs": {"x": {"type": "math_number", "id": "n1", "fields": {"NUM": "3"}}, "y": {"type": "math_number", "id": "n2", "fields": {"NUM": "4"}}}});
        let (node, _) = run(block.clone(), true);
        for slot in ["x", "y"] {
            let wrap = &node.inputs[slot];
            assert_eq!(wrap.kind, "math_arithmetic");
            assert_eq!(field(wrap, "type"), Some("divide"));
            assert_eq!(wrap.inputs["B"].kind, "math_number");
            assert_eq!(field(&wrap.inputs["B"], "NUM"), Some("1.3"));
            assert_eq!(wrap.inputs["A"].parent_id.as_deref(), wrap.id.as_deref());
            assert_eq!(wrap.inputs["A"].kind, "math_number"); // 原输入被搬进 A(重铸 id)
        }
        let (node, _) = run(block, false);
        assert_eq!(node.inputs["x"].kind, "math_number");
        assert_eq!(field(&node.inputs["x"], "NUM"), Some("3"));
    }

    #[test]
    fn list_append_list_field_becomes_pure_list_get_input() {
        let (node, _) = run(
            json!({"type": "list_append", "id": "a", "fields": {"VAR": "list-1", "POS": "last"}, "inputs": {"VALUE": {"type": "math_number", "id": "n1", "fields": {"NUM": "7"}}}}),
            false,
        );
        assert!(!node.fields.contains_key("list")); // 字段被搬走
        let get = &node.inputs["list"];
        assert_eq!(get.kind, "pure_list_get");
        assert!(get.is_shadow && get.is_output);
        assert_eq!(field(get, "list"), Some("list-1"));
        assert_eq!(get.parent_id.as_deref(), Some("a"));
        assert!(node.shadows["list"].contains("type=\"pure_list_get\""));
        assert!(node.shadows["list"].contains("<field name=\"list\">list-1</field>"));
        assert_eq!(node.inputs["list_item_value"].kind, "math_number"); // VALUE → list_item_value
        assert!(!node.disabled); // POS=last:不触发 GC 的首项降级
    }

    #[test]
    fn decrements_text_select_changeable_items() {
        let (node, _) = run(
            json!({"type": "text_select_changeable", "id": "a", "mutation": "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"3\"></mutation>"}),
            false,
        );
        assert_eq!(node.kind, "text_select");
        assert_eq!(
            node.mutation.as_deref(),
            Some("<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"2\"></mutation>")
        );
    }

    #[test]
    fn gc_unwraps_shadow_number_and_disables_loop_audio() {
        // shadow_number 包着 math_number 且无父 → 降级为文本占位
        let (node, _) = run(
            json!({"type": "shadow_number", "id": "a", "inputs": {"VALUE": {"type": "math_number", "id": "b", "fields": {"NUM": "5"}}}}),
            false,
        );
        assert_eq!(node.kind, "bcm_translator_text_return_value_block");
        assert!(node.mutation.unwrap().contains("展示数字5"));
        // 有父 → 直接换成内层 math_number(位置换成外层的位置);根节点会被清掉 parent_id,故这里当子积木
        let (node, _) = run(
            json!({"type": "variables_set", "id": "p", "fields": {"VAR": "v"}, "inputs": {"VALUE": {"type": "shadow_number", "id": "c", "location": [1, 2], "inputs": {"VALUE": {"type": "math_number", "id": "d", "fields": {"NUM": "5"}}}}}}),
            false,
        );
        let child = &node.inputs["value"];
        assert_eq!(child.kind, "math_number");
        assert_eq!(child.location, Some(json!([1, 2])));
        // 循环体内的音频积木置灰(沿 statements/next)
        let (node, _) = run(
            json!({"type": "repeat_forever", "id": "e", "statements": {"DO": {"type": "play_audio", "id": "f", "next": {"type": "play_words_audio", "id": "g"}}}}),
            false,
        );
        assert!(node.statements["DO"].disabled);
        assert!(node.statements["DO"].next.as_ref().unwrap().disabled);
        // 循环外的音频不动
        let (node, _) = run(json!({"type": "play_audio", "id": "h"}), false);
        assert!(!node.disabled);
    }
}
