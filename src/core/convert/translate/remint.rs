//! 实体级并行(方案 25 S3a)的两件工具:**临时 id 的改写** + **工作项并行调度**。
//!
//! 正向管线的处理顺序决定了产物里新铸 id 的**值**(id = 第几次铸造的纯函数)与
//! 告警顺序,所以并行不能让每个线程各自铸 id。做法(方案 25 §3):
//!
//! 1. 每个工作项(实体)用 [`IdSource::recording`](super::model::IdSource::recording)
//!    产出**临时 id**(`\u{1}prov:{槽位}:{序}:{形态}`),并把"本次铸造是什么形态"
//!    记进账本(槽位在同一份文档里两两不同 ⇒ 临时 id 全局唯一);
//! 2. 串行阶段按「阶段 1 全项 → 阶段 2 全项」拼账本,用**一个**串行 `IdSource` 兑现
//!    最终 id —— 确定性模式下与"今天串行实现"逐个相同;
//! 3. 本模块负责改写:临时 id 会落在**值**(节点 `id` / `fields` / `parent_id`)与
//!    **mutation / shadow XML 字符串**里;而 `rewrite_calls` 还会把形参 id 写成
//!    `inputs` / `shadows` 的 **BTreeMap 键** —— 今天那些形参 id 来自源积木,但"键也过一遍
//!    改写"是必须保留的能力(一旦某个形参 id 本身是现铸 id —— Label 形参就是现铸 uuid ——
//!    漏掉键就会让产物里悄悄留下哨兵);三种位置都要覆盖,改写后由 `debug_assert` 兜底。
//!
//! 改写是**单遍扫描**:按哨兵前缀定位 token 再查表,代价 O(产物字符串总长),
//! 而不是"每个字符串 × 每个临时 id"(后者在 10 MB 级作品上是数千万次子串搜索)。

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};

use super::model::{BlockJson, BlockTree, TEMP_ID_PREFIX, is_temp_id_char};
use super::neko::ProcedureEntry;
use super::{TranslateReport, TranslateWarning};

/// 临时 id → 最终 id 的改写表
pub(crate) type IdRemap = HashMap<String, String>;

// ---------------------------------------------------------------- 并行调度

/// 实际线程数:≤ 请求并发、≤ 工作项数、≤ 可用核数
///
/// 作品级(批量)与实体级是**两级**并发,预算折算在
/// [`TranslateOptions::fold_entity_concurrency`](super::TranslateOptions::fold_entity_concurrency)
/// 里做(方案 25 §7 阻塞 #6);这里只负责"别为 3 个实体开 32 个线程、别超出核数"。
pub(crate) fn workers(requested: usize, items: usize) -> usize {
    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    requested.max(1).min(items.max(1)).min(available.max(1))
}

/// 工作项并行调度:把 `items` 交给最多 `workers` 个线程(按 `weights` 贪心装箱),
/// 按**工作项序号**返回结果。
///
/// - `weights[i]` 只用于均衡(重活优先、每次放进当前最空的线程),不影响任何语义;
/// - 返回的 `Vec` 与 `items` 等长同序:调用方按序号拼装,结果与串行逐项跑一致
///   (包括"首个错误按序号冒泡" —— `Result` 收集在调用方按序号做);
/// - 工作线程 panic 会在此处重新抛出(`thread::scope` 的默认语义)。
pub(crate) fn run_items<I, T, F>(
    items: Vec<I>,
    weights: &[usize],
    workers: usize,
    task: F,
) -> Vec<T>
where
    I: Send,
    T: Send,
    F: Fn(usize, I) -> T + Sync,
{
    let count = items.len();
    debug_assert_eq!(weights.len(), count, "装箱权重必须与工作项一一对应");
    if count == 0 {
        return Vec::new();
    }
    if workers <= 1 || count == 1 {
        return items
            .into_iter()
            .enumerate()
            .map(|(index, item)| task(index, item))
            .collect();
    }

    // 贪心装箱(最长处理时间优先):重活先排,每次投给当前最空的线程
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| weights[b].cmp(&weights[a]).then(a.cmp(&b)));
    let mut bins: Vec<Vec<usize>> = vec![Vec::new(); workers];
    let mut loads = vec![0usize; workers];
    for index in order {
        let (target, _) = loads
            .iter()
            .enumerate()
            .min_by_key(|(worker, load)| (**load, *worker))
            .expect("workers ≥ 1");
        loads[target] += weights[index];
        bins[target].push(index);
    }

    // 把项按序号搬进各自的线程(每项只被取走一次)
    let mut slots: Vec<Option<I>> = items.into_iter().map(Some).collect();
    let batches: Vec<Vec<(usize, I)>> = bins
        .into_iter()
        .map(|mut bin| {
            bin.sort_unstable();
            bin.into_iter()
                .filter_map(|index| slots[index].take().map(|item| (index, item)))
                .collect()
        })
        .collect();

    let task_ref = &task;
    let results: Vec<Vec<(usize, T)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = batches
            .into_iter()
            .map(|batch| {
                scope.spawn(move || {
                    batch
                        .into_iter()
                        .map(|(index, item)| (index, task_ref(index, item)))
                        .collect::<Vec<(usize, T)>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("实体级并行的工作线程 panic"))
            .collect()
    });

    let mut ordered: Vec<Option<T>> = (0..count).map(|_| None).collect();
    for batch in results {
        for (index, value) in batch {
            ordered[index] = Some(value);
        }
    }
    ordered
        .into_iter()
        .map(|value| value.expect("每个工作项都被调度恰好一次"))
        .collect()
}

// ---------------------------------------------------------------- 临时 id 改写

/// 原地改写一个字符串字段;返回未命中数(见 [`remap_text`])
fn remap_string(map: &IdRemap, text: &mut String) -> usize {
    if !text.contains(TEMP_ID_PREFIX) {
        return 0;
    }
    let (rewritten, unmatched) = remap_text(map, text);
    *text = rewritten.into_owned();
    unmatched
}

/// 改写一个**拥有**的字符串(键这一类)
fn remap_owned(map: &IdRemap, text: String, unmatched: &mut usize) -> String {
    if !text.contains(TEMP_ID_PREFIX) {
        return text;
    }
    let (rewritten, missed) = remap_text(map, &text);
    *unmatched += missed;
    rewritten.into_owned()
}

/// 单遍改写一个字符串里的临时 id;返回 `(新串, 未命中数)`
///
/// 未命中 = 串里出现了哨兵却查不到账本。真实 id 不含控制字符,所以哨兵只可能来自
/// 本模块的临时 id 方案 —— 未命中即"改写的账本不全",由调用方汇总后断言。
fn remap_text<'a>(map: &IdRemap, text: &'a str) -> (Cow<'a, str>, usize) {
    if !text.contains(TEMP_ID_PREFIX) {
        return (Cow::Borrowed(text), 0);
    }
    let mut out = String::with_capacity(text.len() + 16);
    let mut unmatched = 0;
    let mut rest = text;
    while let Some(start) = rest.find(TEMP_ID_PREFIX) {
        let (head, tail) = rest.split_at(start);
        out.push_str(head);
        // token 右边界:哨兵之后第一个不属于临时 id 字符集的字符
        // (哨兵是 1 字节,`tail[1..]` 落在字符边界上)
        let end = tail[1..]
            .char_indices()
            .find(|(_, c)| !is_temp_id_char(*c))
            .map_or(tail.len(), |(offset, _)| offset + 1);
        let token = &tail[..end];
        match map.get(token) {
            Some(final_id) => out.push_str(final_id),
            None => {
                unmatched += 1;
                out.push_str(token);
            }
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    (Cow::Owned(out), unmatched)
}

/// 改写一棵实体树;返回 `(节点数, 未命中数)`
///
/// 节点数顺带数出来,顶替旧实现里额外的 `tree.count()` 遍历。
pub(crate) fn remap_tree(map: &IdRemap, tree: &mut BlockTree) -> (usize, usize) {
    let mut nodes = 0;
    let mut unmatched = 0;
    for root in &mut tree.roots {
        remap_node(map, root, &mut nodes, &mut unmatched);
    }
    (nodes, unmatched)
}

/// 改写一条程序集条目(条目 id / 形参 id / 定义体积木);返回 `(节点数, 未命中数)`
pub(crate) fn remap_entry(map: &IdRemap, entry: &mut ProcedureEntry) -> (usize, usize) {
    let mut unmatched = remap_string(map, &mut entry.id);
    for param in &mut entry.params {
        unmatched += remap_string(map, &mut param.id);
    }
    let (nodes, missed) = remap_tree(map, &mut entry.tree);
    (nodes, unmatched + missed)
}

/// 把一项的**局部**报告并入全局报告
///
/// - `blocks_total` 相加(与串行逐项累加等价);
/// - 告警**按项序追加**(与串行逐条 `push` 的顺序逐条相同);
/// - 告警里的临时 id 一并换算:`rewrite_calls` 会把程序集 id 写进 `DroppedField.path`
///   (形如 `procedures_2_call.名字 -> <id>.fields.X`)。
///
/// 返回未命中数(供调用方汇总断言)。
pub(crate) fn merge_report(
    global: &mut TranslateReport,
    mut local: TranslateReport,
    map: &IdRemap,
) -> usize {
    global.blocks_total += local.blocks_total;
    let mut unmatched = 0;
    for warning in local.take_warnings() {
        global.warn(remap_warning(map, warning, &mut unmatched));
    }
    unmatched
}

/// 改写一条告警里的字符串(临时 id 只可能出现在这些路径/主体串里)
fn remap_warning(
    map: &IdRemap,
    warning: TranslateWarning,
    unmatched: &mut usize,
) -> TranslateWarning {
    match warning {
        TranslateWarning::UnmappedBlock { kind } => TranslateWarning::UnmappedBlock {
            kind: remap_owned(map, kind, unmatched),
        },
        TranslateWarning::DegradedToText { kind } => TranslateWarning::DegradedToText {
            kind: remap_owned(map, kind, unmatched),
        },
        TranslateWarning::DroppedField { path } => TranslateWarning::DroppedField {
            path: remap_owned(map, path, unmatched),
        },
        TranslateWarning::DroppedProperty { path } => TranslateWarning::DroppedProperty {
            path: remap_owned(map, path, unmatched),
        },
        TranslateWarning::RemintedId { from } => TranslateWarning::RemintedId {
            from: remap_owned(map, from, unmatched),
        },
        TranslateWarning::ReuploadedOnImport { path } => TranslateWarning::ReuploadedOnImport {
            path: remap_owned(map, path, unmatched),
        },
    }
}

/// 单节点改写:覆盖所有可能携带 id 的字段
///
/// - `id` / `parent_id` / `mutation`(`rewrite_calls` 重建的 `<mutation def_id=…>`);
/// - `fields` / `shadows` / `inputs` / `statements` 的**键与值**
///   (形参 id 会同时当 `inputs` 的键与 `shadows` 的键);
/// - `location` / `field_constraints` / `extra`(未知键)里的任何字符串;
/// - `kind` 也过一遍:块类型名从不铸 id,但这样"没改到的字段"就只剩结构性问题,
///   哨兵兜底(`debug_assert`)才真正覆盖全树。
fn remap_node(map: &IdRemap, node: &mut BlockJson, nodes: &mut usize, unmatched: &mut usize) {
    *nodes += 1;
    *unmatched += remap_string(map, &mut node.kind);
    if let Some(id) = node.id.as_mut() {
        *unmatched += remap_string(map, id);
    }
    if let Some(parent_id) = node.parent_id.as_mut() {
        *unmatched += remap_string(map, parent_id);
    }
    if let Some(mutation) = node.mutation.as_mut() {
        *unmatched += remap_string(map, mutation);
    }
    if let Some(location) = node.location.as_mut() {
        *unmatched += remap_json(map, location);
    }
    if let Some(constraints) = node.field_constraints.as_mut() {
        *unmatched += remap_json(map, constraints);
    }
    *unmatched += remap_value_map(map, &mut node.fields);
    *unmatched += remap_shadow_map(map, &mut node.shadows);
    remap_node_map(map, &mut node.inputs, nodes, unmatched);
    remap_node_map(map, &mut node.statements, nodes, unmatched);
    if let Some(next) = node.next.as_deref_mut() {
        remap_node(map, next, nodes, unmatched);
    }
    *unmatched += remap_object(map, &mut node.extra);
}

/// 改写 `inputs` / `statements`(槽名或形参 id → 子节点):键与子树都改
fn remap_node_map(
    map: &IdRemap,
    children: &mut BTreeMap<String, BlockJson>,
    nodes: &mut usize,
    unmatched: &mut usize,
) {
    if children.keys().any(|key| key.contains(TEMP_ID_PREFIX)) {
        let old = std::mem::take(children);
        for (key, mut child) in old {
            let key = remap_owned(map, key, unmatched);
            remap_node(map, &mut child, nodes, unmatched);
            children.insert(key, child);
        }
    } else {
        for child in children.values_mut() {
            remap_node(map, child, nodes, unmatched);
        }
    }
}

/// 改写 `fields`(字段名 → JSON):键与值都改(值可能是实体/程序集 id)
fn remap_value_map(map: &IdRemap, fields: &mut BTreeMap<String, Value>) -> usize {
    if fields.keys().any(|key| key.contains(TEMP_ID_PREFIX)) {
        let old = std::mem::take(fields);
        let mut unmatched = 0;
        for (key, mut value) in old {
            let key = remap_owned(map, key, &mut unmatched);
            unmatched += remap_json(map, &mut value);
            fields.insert(key, value);
        }
        unmatched
    } else {
        fields
            .values_mut()
            .map(|value| remap_json(map, value))
            .sum()
    }
}

/// 改写 `shadows`(槽名或形参 id → shadow XML):键与 XML 里的 id 都改
fn remap_shadow_map(map: &IdRemap, shadows: &mut BTreeMap<String, String>) -> usize {
    if !shadows
        .iter()
        .any(|(key, xml)| key.contains(TEMP_ID_PREFIX) || xml.contains(TEMP_ID_PREFIX))
    {
        return 0;
    }
    let old = std::mem::take(shadows);
    let mut unmatched = 0;
    for (key, mut xml) in old {
        let key = remap_owned(map, key, &mut unmatched);
        unmatched += remap_string(map, &mut xml);
        shadows.insert(key, xml);
    }
    unmatched
}

/// 递归改写一个 JSON 值(字符串 / 数组 / 对象的**键与值**);返回未命中数
pub(crate) fn remap_json(map: &IdRemap, value: &mut Value) -> usize {
    match value {
        Value::String(text) => remap_string(map, text),
        Value::Array(items) => items.iter_mut().map(|item| remap_json(map, item)).sum(),
        Value::Object(object) => remap_object(map, object),
        _ => 0,
    }
}

/// 递归改写一个 JSON 对象(键与值)
fn remap_object(map: &IdRemap, object: &mut Map<String, Value>) -> usize {
    if object.keys().any(|key| key.contains(TEMP_ID_PREFIX)) {
        let old = std::mem::take(object);
        let mut unmatched = 0;
        for (key, mut value) in old {
            let key = remap_owned(map, key, &mut unmatched);
            unmatched += remap_json(map, &mut value);
            object.insert(key, value);
        }
        unmatched
    } else {
        object
            .values_mut()
            .map(|value| remap_json(map, value))
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map_of(pairs: &[(&str, &str)]) -> IdRemap {
        pairs
            .iter()
            .map(|(temp, final_id)| (temp.to_string(), final_id.to_string()))
            .collect()
    }

    /// 改写覆盖三类位置:值、BTreeMap 的键(含前缀/后缀包裹)、mutation XML 串
    #[test]
    fn remaps_values_keys_and_xml_strings() {
        let map = map_of(&[
            ("\u{1}prov:0:0:u", "uuid-final"),
            ("\u{1}prov:0:1:s", "short-final"),
        ]);

        // 值 + 键 + 字符串内部的 token(右边界由字符集决定)
        let mut text = String::from("\u{1}prov:0:0:u");
        assert_eq!(remap_string(&map, &mut text), 0);
        assert_eq!(text, "uuid-final");

        let mut xml = String::from(
            "<mutation def_id=\"\u{1}prov:0:0:u\"><arg id=\"\u{1}prov:0:1:s\"></arg></mutation>",
        );
        assert_eq!(remap_string(&map, &mut xml), 0);
        assert_eq!(
            xml,
            "<mutation def_id=\"uuid-final\"><arg id=\"short-final\"></arg></mutation>"
        );

        // 键:改写后 BTreeMap 必须按**最终**键重排(否则产物键序与串行不一致)
        let mut children: BTreeMap<String, BlockJson> = BTreeMap::from([
            ("\u{1}prov:0:0:u".to_string(), BlockJson::default()),
            (
                "\u{1}prov:0:1:s".to_string(),
                BlockJson {
                    id: Some("\u{1}prov:0:0:u".into()),
                    mutation: Some("\u{1}prov:0:1:s".into()),
                    ..Default::default()
                },
            ),
        ]);
        let mut nodes = 0;
        let mut unmatched = 0;
        remap_node_map(&map, &mut children, &mut nodes, &mut unmatched);
        assert_eq!((nodes, unmatched), (2, 0));
        assert_eq!(
            children.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["short-final", "uuid-final"]
        );
        assert_eq!(children["short-final"].id.as_deref(), Some("uuid-final"));
        assert_eq!(
            children["short-final"].mutation.as_deref(),
            Some("short-final")
        );
    }

    /// 未命中(哨兵不在账本里)必须被数出来,且原样保留(由调用方断言兜底);
    /// 键上的哨兵同样参与改写与计数
    #[test]
    fn keeps_and_counts_unmatched_sentinels() {
        let map = map_of(&[
            ("\u{1}prov:0:0:u", "uuid-final"),
            ("\u{1}prov:9:9:u", "key-final"),
        ]);
        let mut value = json!({ "\u{1}prov:9:9:u": ["\u{1}prov:0:0:u", "\u{1}prov:9:9:s"] });
        assert_eq!(remap_json(&map, &mut value), 1, "只有 9:9:s 没进账本");
        assert_eq!(
            value,
            json!({ "key-final": ["uuid-final", "\u{1}prov:9:9:s"] })
        );
    }

    /// 串行语义:1 个线程时按序号顺序跑(结果与"逐个跑"完全一致)
    #[test]
    fn single_worker_runs_in_item_order() {
        let items: Vec<usize> = (0..4).collect();
        let result = run_items(items, &[1, 1, 1, 1], 1, |index, item| {
            assert_eq!(index, item);
            item * 2
        });
        assert_eq!(result, vec![0, 2, 4, 6]);
    }

    /// 多线程:结果仍按序号排列(与装箱分配无关)
    #[test]
    fn multi_worker_keeps_item_order() {
        let items: Vec<usize> = (0..17).collect();
        let weights: Vec<usize> = items.iter().map(|i| i % 5).collect();
        let result = run_items(items, &weights, 4, |index, item| {
            assert_eq!(index, item);
            item + 1
        });
        assert_eq!(result, (1..=17).collect::<Vec<_>>());
    }

    /// 线程数夹取:≤ 请求、≤ 项数、≤ 可用核数,且 ≥ 1
    #[test]
    fn workers_are_clamped() {
        let available = std::thread::available_parallelism().map_or(1, |n| n.get());
        assert_eq!(workers(0, 4), 1, "请求 0 也至少 1 个线程");
        assert_eq!(workers(8, 0), 1, "没有工作项时不空转");
        assert_eq!(workers(8, 3), 3.min(available).max(1));
        assert_eq!(workers(8, 1000), available.max(1), "封顶到可用核数");
    }
}
