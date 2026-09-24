//! KN 侧积木 adapter(前端 + 后端)。
//!
//! 对应官方 webpack module 41888(`temp/ref/mod41888.pretty.js`)里 `kittenBcmToNekoBcmUtils`
//! 收尾的三段(`docs/20-kitten-kn-work-conversion-plan.md` §3.2):
//!
//! - `HC` 的另一半(78421-78440):根积木按类型一分为二——`procedures_2_defnoreturn` 摘出去当
//!   程序集定义,其余留在实体的 `nekoBlockJsonList`;[`split_procedures`]
//! - `zC(entity, proceduresDict)`(78481-78576):每个定义 → `{id,name,type,params,nekoBlockJsonList}`
//!   条目。参数表 = 合成 `Label`(新 UUID)+ 按 `PARAMS<n>` 序号排序的 `String` 形参;`inputs.STACK`
//!   搬进 `statements.STACK`;定义体里每个 `procedures_2_parameter` 引用补上
//!   `<mutation id="<形参 id>">` + `is_output`;整棵树套上 `<arg …>` mutation 且
//!   `fields.NAME` 换成条目 id;含返回值(`ZC` 78406)时**再多产出一条 `ROUND` 条目**
//!   (`VC` 78441 补默认 `VALUE` 输入 + `WC` 78473 从 `NORMAL` 主体里剥掉 `VALUE`);
//!   [`split_procedures`]
//! - `KC(entity, proceduresDict)`(78577-78638):实体里 `procedures_2_callnoreturn` /
//!   `procedures_2_callreturn` 的 `fields.NAME`(源侧是程序集**名**)换成目标**id**,
//!   重建 `mutation def_id/name/type` + 每个参数 `<arg content>`;`String` 形参另换
//!   `math_number` 影子,并把老槽位 `ARG<i-1>` 的输入**复制**(官方是复制不是搬移,两边都留)
//!   到形参 id 槽;同名程序集找不到时原样保留。[`rewrite_calls`]
//!
//! 两条纪律:
//!
//! 1. `GC`(官方 `zC`/`KC` 内部还会再套一层坐标/特例改写)在
//!    [`mapping::translate_kitten_to_kn`](super::mapping::translate_kitten_to_kn) 里已经对整棵树
//!    做过,这里**绝不重复施加**(否则横屏坐标会被除两次 1.3);因此管线顺序是
//!    `kitten::parse_block_data_json` → `mapping::translate_kitten_to_kn` → `neko::split_procedures`
//!    → `neko::rewrite_calls`(实体树)。
//! 2. 上游的 `id` 生成走 [`IdSource`]:合成 `Label` 形参、`KC` 的影子、`VC` 的默认输入都要
//!    **现铸** UUID(官方 `BC()`),确定性模式下才可逐字节对齐。
//!
//! ## 反向(KN → Kitten4,官方无此方向,本库自建)
//!
//! - [`parse_kn_entity`]:`nekoBlockJsonList` 数组 → 中核树(空/缺失 → 空树);
//! - [`parse_kn_procedures`]:`proceduresDict` → [`ProcedureEntry`](形参/定义体原样);
//! - [`unrewrite_calls`]:`KC` 的逆 —— `fields.NAME`(程序集 id)换回**名字**,重建 Kitten4 的
//!   `<mutation name def_id>` + `procedures_2_parameter_shadow` 与 `ARG<j>` 影子,并把 `KC`
//!   复制到形参 id 槽位上的输入搬回 `ARG<j>`;
//! - [`def_root_from_entry`]:`zC` 的逆 —— 形参回到 `PARAMS<j>` 输入(`procedures_2_stable_parameter`
//!   子块,id 沿用形参 id)、`mutation` 回 `<arg name="PARAMS<j>">`、`deletable/editable` 回 `true`。
//!
//! ## 与官方的刻意差异 / 近似
//!
//! - **id 重铸不进报告**:合成 `Label`/影子/默认输入的 id 都是新铸的、且没有对应的"旧 id"被丢,
//!   逐个记 [`TranslateWarning::RemintedId`](super::report::TranslateWarning::RemintedId) 只会淹没报告;
//!   确定性模式([`IdSource::new(true)`](super::ids::IdSource::new))下它们本来就稳定可对齐。
//! - **静默覆盖改成有据可查**:官方把定义积木的 `statements` 整体换成 `{STACK}`、把 `fields`
//!   整体换成 `{NAME: id}`,多出来的槽位会被无声丢掉;本实现照做但逐条记
//!   [`TranslateWarning::DroppedField`](super::report::TranslateWarning::DroppedField)。
//! - **参数名对不上**:官方只 `console.warn("找不到名为 X 的参数")` 然后跳过;本实现同样跳过,
//!   但记 `DroppedField`(`procedures.<定义名>.param.<形参名>`),便于事后定位。
//! - `PARAMS<n>` 序号:官方 `parseInt(key.replace("PARAMS",""), 10)`,取不到数字时是 `NaN`
//!   (排序结果由引擎决定);本实现把"取不到数字"记作 `0`。
//! - 参数引用的 mutation 补全:官方那段遍历**隔层**(见 [`annotate_param_refs`] 的说明),真机产物里
//!   因此有没补 mutation 的形参引用;本实现"遍历到就补",是官方结果的**超集**。
//! - **`shield` 在编码端补齐**:官方 `jC.parseBlock` 给每个节点写 `shield: !!t.shield`(Kitten4
//!   源里没这个键 → 恒 `false`),而 [`BlockJson`] 的 `shield` 是 `skip_serializing_if = "is_false"`;
//!   不补齐的话产物里会少一个官方必写的键([`fill_shield`],真机产物里 439 个节点全带)。
//! - `mutation` / `<arg>` 里的名字、字段一律**不做 XML 转义**(官方也不做),逐字拼接。
//! - **键顺序**:`BlockJson` 按结构体字段序、`serde_json::Map`(未开 `preserve_order`)按字典序,
//!   与官方的"插入顺序"不同 —— 值等价(见 §3.2 的对齐实验),逐字节不等价。

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::core::convert::shared::{DecompilerError, Result};

use super::blockjson::{BlockJson, BlockTree};
use super::ids::IdSource;
use super::mapping::math_number_shadow;
use super::report::{TranslateReport, TranslateWarning};

/// 官方 mutation 的命名空间(xhtml)
const XHTML: &str = "http://www.w3.org/1999/xhtml";
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
        let child = BlockJson {
            kind: "math_number".into(),
            id: Some(ids.uuid()),
            is_shadow: true,
            fields: BTreeMap::from([("NUM".to_string(), Value::String("0".into()))]),
            field_constraints: Some(
                json!({ "NUM": { "min": null, "max": null, "precision": 0, "mod": null } }),
            ),
            is_output: true,
            parent_id: node.id.clone(),
            shield: false,
            ..Default::default()
        };
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
    let values = match list {
        Value::Array(items) => items.clone(),
        // 少数链路把该字段存成 JSON 字符串(与 Kitten 侧 `block_data_json` 的容错一致)
        Value::String(text) if !text.trim().is_empty() => serde_json::from_str::<Value>(text)
            .map_err(DecompilerError::from)?
            .as_array()
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut roots = Vec::new();
    for value in &values {
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
        if let Some(id) = attr_value(mutation, attr)
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

/// `mutation` 里某个属性的值(与 `mapping.rs` 的同名工具同语义)
fn attr_value<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let end = xml.find('>')?;
    let tag = &xml[..end];
    let mut from = 0;
    loop {
        let at = from + tag[from..].find(attr)?;
        let name_end = at + attr.len();
        if (at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace())
            && tag.as_bytes().get(name_end) == Some(&b'=')
            && tag.as_bytes().get(name_end + 1) == Some(&b'"')
        {
            let start = name_end + 2;
            return Some(&tag[start..start + tag[start..].find('"')?]);
        }
        from = name_end;
    }
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

#[cfg(test)]
mod tests {
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
