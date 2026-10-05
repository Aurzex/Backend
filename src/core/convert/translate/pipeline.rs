//! 文档级编排:正向(Kitten4 → KN)与反向(KN → Kitten4)的"拆工作项 → 语义映射 →
//! 兑现 id → 装配",以及两条路共用的工作项并行调度与临时 id 改写。
//!
//! 原先正向编排在 `translate/mod.rs`、反向编排与并行机/remint 在 `assembly.rs`
//! (后者还排在测试之后);拆出来让两边同层可对照,`assembly.rs` 回归单一职责。

use super::assembly::{self, KnEntity};
use super::mapping;
use super::model::{self, BlockJson, BlockTree, ProcedureEntry, TEMP_ID_PREFIX, is_temp_id_char};
use super::options::{StageOrientation, TargetEditor, TranslateError, TranslateOptions};
use super::report::{TranslateReport, TranslateWarning};
use super::tables_gen;
use super::xml;
use crate::core::convert::shared::XHTML;
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

/// 一个正向工作项(= 一个实体):从源文档取出的积木树 + 装配要用的元数据
struct ForwardItem {
    /// 源实体 id(产物字典的键)
    id: String,
    /// 所在容器(`scenes` / `actors`),用于把积木树原样放回源文档
    container: &'static str,
    /// 场景还是角色(决定装配走 `scene_entry` 还是 `actor_entry`)
    is_scene: bool,
    /// 源实体元数据(`block_data_json` 已取走;装配侧本来也会删它)
    source: Option<serde_json::Map<String, serde_json::Value>>,
    /// 源积木树(`block_data_json`),转换结束后原样放回源文档(仅 `Value` 路径有内容可放;
    /// 原文路径下源文档里本就没有该字段,见 [`BlockData`])
    block_data_json: Option<BlockData>,
    /// 装箱权重:只影响并行均衡,不进产物。`Value` 路径 = 源 `blocks` 条数;
    /// 原文路径 = 原文**字节数**(同向且免解析,见 [`collect_forward_items`])
    weight: usize,
}

/// 一个实体待解析的 `block_data_json`
///
/// - `Raw`:源侧骨架路径(`translate_file`)—— 保留原文,解析时**直接反序列化成强类型树**,
///   不建源 `Value` 中间树(读数见 `../rounds/47-data-layer-rewrite-plan.md` Step 5);
/// - `Value`:公开面 `translate_value(Value)` 路径 —— 调用方给的就是已解析的 `Value`,没有原文。
enum BlockData {
    Raw(Box<serde_json::value::RawValue>),
    Value(serde_json::Value),
}

/// 阶段 1 的产出:本项的树、抽出的程序集、铸造账本与**局部**报告
struct ForwardParsed {
    tree: model::BlockTree,
    procedures: Vec<model::ProcedureEntry>,
    /// 本项铸造账本(`(临时 id, 形态)`,顺序 = 铸造顺序)
    log: Vec<(String, model::MintKind)>,
    report: TranslateReport,
}

/// 阶段 2 的产出:重写调用点之后的树 + 影子 id 的铸造账本 + 局部报告
struct ForwardRewritten {
    tree: model::BlockTree,
    log: Vec<(String, model::MintKind)>,
    report: TranslateReport,
}

/// 阶段 0(串行):按官方顺序拆出工作项,并取走每项的 `block_data_json`
///
/// 顺序 = `scenes` → `actors`,各自按 id 排序 —— 与旧实现的 `map.iter_mut()` 完全同一顺序
/// (serde_json 的 `Map` 默认有序)。**这是唯一接触源文档的步骤**,后续阶段只碰工作项自己的数据。
fn collect_forward_items(
    source: &mut serde_json::Value,
    raw_blocks: &mut Option<
        std::collections::BTreeMap<(String, String), Box<serde_json::value::RawValue>>,
    >,
) -> std::result::Result<Vec<ForwardItem>, TranslateError> {
    use serde_json::Value;
    let theatre = source
        .get_mut("theatre")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| TranslateError::InvalidArgument("源作品没有 theatre".into()))?;
    let mut items = Vec::new();
    for (is_scene, container) in [(true, "scenes"), (false, "actors")] {
        let Some(map) = theatre.get_mut(container).and_then(Value::as_object_mut) else {
            continue;
        };
        for (id, entity) in map.iter_mut() {
            let Some(entity) = entity.as_object_mut() else {
                continue;
            };
            // 取走 `block_data_json`:它只服务解析,装配侧本来就会删掉它
            // (assembly `actor_entry` 里那句 `remove` 保留作兜底)
            let block_data_json = match raw_blocks {
                // 源侧骨架路径:源文档里没有该字段,原文在旁表里按 (容器, id) 取
                Some(map) => map
                    .remove(&(container.to_string(), id.clone()))
                    .map(BlockData::Raw),
                None => entity.remove("block_data_json").map(BlockData::Value),
            };
            let weight = match &block_data_json {
                Some(BlockData::Value(bdj)) => bdj
                    .get("blocks")
                    .and_then(Value::as_object)
                    .map_or(0, serde_json::Map::len),
                // 原文路径:权重只用于并行装箱(不进产物),取原文字节数 —— 与"块数"同向且免解析
                Some(BlockData::Raw(raw)) => raw.get().len(),
                None => 0,
            };
            items.push(ForwardItem {
                id: id.clone(),
                container,
                is_scene,
                source: Some(entity.clone()),
                block_data_json,
                weight,
            });
        }
    }
    Ok(items)
}

/// 把取走的 `block_data_json` 放回源文档(维持"转换不改源文档"的约定:可复用、可重复转换)
fn restore_forward_items(source: &mut serde_json::Value, items: &mut [ForwardItem]) {
    use serde_json::Value;
    let Some(theatre) = source.get_mut("theatre").and_then(Value::as_object_mut) else {
        return;
    };
    for item in items.iter_mut() {
        // 原文路径没有可放回的东西:源文档里本来就没有该字段
        let Some(BlockData::Value(block_data_json)) = item.block_data_json.take() else {
            continue;
        };
        let Some(entity) = theatre
            .get_mut(item.container)
            .and_then(Value::as_object_mut)
            .and_then(|map| map.get_mut(&item.id))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        entity.insert("block_data_json".into(), block_data_json);
    }
}

/// 旧口径的 `block_data_json` → 积木树(内联对象影子先在**副本**上改写成影子 XML,源文档不动)
fn parse_value_block_data(
    value: &serde_json::Value,
) -> std::result::Result<model::BlockTree, TranslateError> {
    match normalize_object_shadows(value) {
        Some(owned) => Ok(model::parse_block_data_json(&owned)?),
        None => Ok(model::parse_block_data_json(value)?),
    }
}

/// 阶段 1(单个工作项,项内自足):解析 → 语义映射 → 抽程序集
///
/// 铸造走**临时 id**(`IdSource::recording`),账本交给串行阶段兑现最终 id。
fn parse_forward_item(
    index: usize,
    block_data: Option<&BlockData>,
    landscape: bool,
) -> std::result::Result<ForwardParsed, TranslateError> {
    let mut ids = model::IdSource::recording(index);
    let mut local = TranslateReport::new(
        crate::core::convert::EditorType::Kitten4,
        TargetEditor::KittenN,
    );
    let mut tree = match block_data {
        // 快速通道:原文直接反序列化成强类型树(不建源 `Value` 中间树)
        Some(BlockData::Raw(raw)) => match model::parse_block_data_json_typed(raw) {
            Ok(tree) => tree,
            // 旧形态(字符串化的 `blocks`、内联对象影子:写不进 `BTreeMap<String, String>` 的
            // `shadows`)在这里失败 ⇒ 物化成 `Value` 走下面同一条老路径,行为逐字一致。
            Err(_) => {
                let value: serde_json::Value = serde_json::from_str(raw.get())?;
                parse_value_block_data(&value)?
            }
        },
        Some(BlockData::Value(value)) => parse_value_block_data(value)?,
        None => model::BlockTree::default(),
    };
    local.blocks_total += tree.count(); // 源文件里的积木数(映射前)
    mapping::translate_kitten_to_kn(&mut tree, landscape, &mut ids, &mut local);
    let (kept, procedures) = model::split_procedures(tree, &mut ids, &mut local);
    Ok(ForwardParsed {
        tree: kept,
        procedures,
        log: ids.into_log(),
        report: local,
    })
}

/// Kitten4 编辑版 → KN 编辑版(纯文档级管线,与官方两步对齐)
///
/// ## 实体级并行(方案 25 S3a)
///
/// 处理顺序决定产物里新铸 id 的**值**(id = 第几次铸造的纯函数)与告警顺序,所以并行
/// 不能让每个线程各自铸 id,而是分段(实现见下文的 [`IdRemap`] 段):
///
/// 1. **阶段 0(串行)**:按官方顺序(`scenes` → `actors`)拆工作项并取走 `block_data_json`;
/// 2. **阶段 1(并行,项内自足)**:解析 + 语义映射 + 抽程序集 —— 每项一个
///    [`model::IdSource::recording`],产出临时 id 并记账,告警进局部报告;
/// 3. 全局 `procedures` 按项序拼接后 → **阶段 2(并行)**:调用点重写
///    (它会把程序集的形参 id 复制进实体树,并当作 `inputs` 的键 —— 由阶段 3 一并改写);
/// 4. **阶段 3(串行)**:按「阶段 1 全项 → 阶段 2 全项」拼账本,用一个串行 `IdSource`
///    兑现最终 id(与旧实现"先所有实体 parse/mapping/split,再所有实体 rewrite_calls"
///    的铸造序列逐次对应),再把临时 id 的值 / 键 / mutation·shadow XML 一并改写;
///    程序集条目与告警串同表改写;
/// 5. **阶段 4(并行)**:改写完的树按今天同一入口编码(`model::tree_to_json`)。
///
/// `entity_concurrency = 1`(默认)时三个阶段都在当前线程按项序跑,但仍然走同一条
/// 临时 id 路径 —— 因此"默认产物与今天逐字节一致"由基准的 SHA256 基线直接守住
/// (`tests/convert_bench.rs` 的两个正向样本)。id 值的逐字节一致只在
/// `deterministic_ids(true)` 下承诺(非确定性模式只承诺合法 + 唯一,见
/// [`TranslateOptions::entity_concurrency`])。
pub(super) fn convert_kitten4_document(
    // `&mut`:装配阶段用不到源实体的 `block_data_json`,而它是整棵积木树(源文档里
    // 最大的字段)。取走它再克隆实体对象,省掉"每个实体复制一份完整积木树"的
    // 文档级白拷贝(方案 23 P0-3)。
    source: &mut serde_json::Value,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    convert_kitten4_document_impl(source, None, options, report)
}

/// 与 [`convert_kitten4_document`] 同一条管线,但 `block_data_json` 由**原文**提供
///
/// 源侧骨架路径(`translate::parse_source_skeleton`)已经把每个实体的 `block_data_json`
/// 原样摘出来、没有建成 `Value`,所以这里按 (容器, id) 交回;源文档里本就没有该字段,
/// 也就不存在"放回"一步(读数与设计见 `../rounds/47-data-layer-rewrite-plan.md` Step 5)。
pub(super) fn convert_kitten4_document_raw(
    source: &mut serde_json::Value,
    block_data: std::collections::BTreeMap<(String, String), Box<serde_json::value::RawValue>>,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    convert_kitten4_document_impl(source, Some(block_data), options, report)
}

fn convert_kitten4_document_impl(
    source: &mut serde_json::Value,
    mut raw_blocks: Option<
        std::collections::BTreeMap<(String, String), Box<serde_json::value::RawValue>>,
    >,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    use serde_json::Value;
    let started = std::time::Instant::now();

    // 官方 GN 第一行就读 `size`;Kitten2/3(`.bcm` + blocksXML)没有它 —— 给明确错误,
    // 而不是像官方那样抛 TypeError(见 docs/rounds/20 §1/§11.1)。
    //
    // 但 **`size` 不是可靠判据**:第三十三轮实测有 Kitten4 作品(`捕鱼达人_259694808`,
    // 文件就是 `.bcm4`)顶层只有 `width`/`height`,旧判据一票否决、把它误判成 Kitten2/3。
    // 改用与装配**同一套回退**(`assembly::source_stage_size`: `size.*` → 顶层
    // `width`/`height` → 官方默认 562×900),只有三者都缺才认定是 Kitten2/3。
    let src_map = source
        .as_object()
        .ok_or_else(|| TranslateError::InvalidArgument("源作品不是 JSON 对象".into()))?;
    if !["size", "width", "height"]
        .iter()
        .any(|key| src_map.contains_key(*key))
    {
        return Err(TranslateError::InvalidArgument(
            "源作品没有 size/width/height:这看起来是 Kitten2/3(.bcm/blocksXML)作品,本库暂不支持该方向"
                .into(),
        ));
    }
    let (src_w, src_h) = assembly::source_stage_size(src_map);
    let landscape = src_w > src_h;

    // Kitten4 的真正特征:实体带 `block_data_json`。Kitten2/3 的积木在 `blocksXML` 里
    // (没有 `block_data_json`)—— 那种喂进来只会得到"零积木的空产物",所以明确报错。
    // (画布字段不能当判据:`捕鱼达人_259694808` 是 Kitten4 却没有 `size`;`春风得意_324995084.bcm`
    // 是 Kitten3 却有 `width`/`height` —— 第三十三轮实测。)
    let entities: Vec<&Value> = ["actors", "scenes"]
        .iter()
        .filter_map(|container| source["theatre"][container].as_object())
        .flat_map(|map| map.values())
        .collect();
    if !entities.is_empty()
        && !match &raw_blocks {
            // 原文路径:源文档里已经没有 `block_data_json`,以旁表为准
            Some(map) => !map.is_empty(),
            None => entities
                .iter()
                .any(|entity| entity.get("block_data_json").is_some()),
        }
    {
        return Err(TranslateError::InvalidArgument(
            "源作品的实体里没有 block_data_json:这看起来是 Kitten2/3(.bcm + blocksXML)作品,本库暂不支持该方向"
                .into(),
        ));
    }

    // 影子形态:本库吃 XML 字符串(`shadows: {槽: "<shadow …>"}`),但平台上有些作品的
    // 影子是**内联对象**(`shadows: {槽: {type, fields, …}}`,第三十三轮实测 `A28社区-开幕_174408420`)。
    // 这一类现在**能转换**了 —— 对象在解析前就地改写成平台同款影子 XML,见
    // [`normalize_object_shadows`](它在 [`parse_forward_item`] 里按项做,不改源文档)。
    let mut items = collect_forward_items(source, &mut raw_blocks)?;
    let weights: Vec<usize> = items.iter().map(|item| item.weight).collect();
    let workers = workers(options.entity_workers(), items.len());
    let deterministic = options.ids_deterministic();
    // 可观测事实:本次转换真的开了几个实体级线程(供基准/单测挡空门,见 `TranslateReport`)
    report.entity_workers = workers;

    let outcome = (|| -> std::result::Result<Value, TranslateError> {
        // ── 阶段 1(并行):解析 + 语义映射 + 抽程序集(官方:scenes.forEach → actors.forEach → zC)
        //
        // 只**借用**源积木树:`block_data_json` 之后要放回源文档。`.collect::<Result<…>>()`
        // 让"首个错误按项序冒泡"与串行一致。
        let block_data: Vec<Option<&BlockData>> = items
            .iter()
            .map(|item| item.block_data_json.as_ref())
            .collect();
        let parsed = run_items(block_data, &weights, workers, |index, block_data| {
            parse_forward_item(index, block_data, landscape)
        })
        .into_iter()
        .collect::<std::result::Result<Vec<ForwardParsed>, TranslateError>>()?;

        let mut trees: Vec<model::BlockTree> = Vec::with_capacity(parsed.len());
        let mut procedures: Vec<model::ProcedureEntry> = Vec::new();
        let mut logs_first: Vec<Vec<(String, model::MintKind)>> = Vec::with_capacity(parsed.len());
        let mut reports_first: Vec<TranslateReport> = Vec::with_capacity(parsed.len());
        for item in parsed {
            trees.push(item.tree);
            procedures.extend(item.procedures);
            logs_first.push(item.log);
            reports_first.push(item.report);
        }

        // ── 阶段 2(并行):程序集调用点重写(官方 KC:所有实体共用同一张 proceduresDict)
        //
        // 记账槽位必须与阶段 1 错开(临时 id 的唯一性靠槽位):阶段 1 用 `[0, 项数)`,
        // 阶段 2 用 `[项数, 2·项数)`。
        let phase = items.len();
        let rewritten = run_items(trees, &weights, workers, |index, mut tree| {
            let mut ids = model::IdSource::recording(phase + index);
            let mut local = TranslateReport::new(
                crate::core::convert::EditorType::Kitten4,
                TargetEditor::KittenN,
            );
            model::rewrite_calls(&mut tree, &procedures, &mut ids, &mut local);
            ForwardRewritten {
                tree,
                log: ids.into_log(),
                report: local,
            }
        });
        let mut trees: Vec<model::BlockTree> = Vec::with_capacity(rewritten.len());
        let mut logs_second: Vec<Vec<(String, model::MintKind)>> =
            Vec::with_capacity(rewritten.len());
        let mut reports_second: Vec<TranslateReport> = Vec::with_capacity(rewritten.len());
        for item in rewritten {
            trees.push(item.tree);
            logs_second.push(item.log);
            reports_second.push(item.report);
        }

        // ── 阶段 3(串行):临时 id → 最终 id
        //
        // 全局铸造顺序 = 「阶段 1:按项序」++「阶段 2:按项序」,与旧实现的
        // 「先所有实体 parse/mapping/split_procedures,再所有实体 rewrite_calls」逐次对应。
        let mut mints = IdRemap::new();
        {
            let mut ids = model::IdSource::new(deterministic);
            for log in logs_first.iter().chain(logs_second.iter()) {
                for (temp, kind) in log {
                    let previous = mints.insert(temp.clone(), kind.mint(&mut ids));
                    // 临时 id 撞车 = 记录模式给了两个不同的铸造点同一个名字(槽位没错开),
                    // 表会被后来的覆盖 → 产物 id 静默错位。调试构建直接抓。
                    debug_assert!(
                        previous.is_none(),
                        "临时 id 冲突:{temp:?}(记账槽位在同一份文档里必须唯一)"
                    );
                }
            }
        }

        // 程序集条目(条目 id / 形参 id / 定义体积木)同表改写:装配端的
        // `procedures_to_json` 才能写出最终 id;节点数在这里顺带数出来。
        let mut unmatched = 0usize;
        let mut procedures_nodes = 0usize;
        for entry in &mut procedures {
            let (nodes, missed) = remap_entry(&mints, entry);
            procedures_nodes += nodes;
            unmatched += missed;
        }

        // ── 阶段 4(并行):改写实体树 + 编码(编码入口与旧实现同一个)
        let encoded = run_items(trees, &weights, workers, |_, mut tree| {
            let (nodes, missed) = remap_tree(&mints, &mut tree);
            let blocks = model::tree_to_json(&tree)?;
            Ok::<_, TranslateError>((blocks, nodes, missed))
        });

        // ── 装配
        // 计数口径:total = 源文件里的积木数;converted = 产物里的节点数
        // (含被搬进 proceduresDict 的定义;影子实体化会让 converted ≥ total,与官方统计一致)
        let mut entities = Vec::with_capacity(items.len());
        let mut converted = procedures_nodes;
        for (item, encoded) in items.iter_mut().zip(encoded) {
            let (blocks, nodes, missed) = encoded?;
            unmatched += missed;
            converted += nodes;
            entities.push(assembly::ConvertedEntity {
                // 源实体 id 也是"放回源文档"的键,所以只克隆(短串);元数据按值搬走
                source_id: item.id.clone(),
                is_scene: item.is_scene,
                blocks,
                source: item.source.take().unwrap_or_default(),
            });
        }

        // 告警/计数:按「阶段 1 全项 → 阶段 2 全项」逐项并入 —— 与串行 push 顺序逐条相同;
        // 告警串里的临时 id(`rewrite_calls` 会写进 `DroppedField.path`)同表换算。
        for local in reports_first {
            unmatched += merge_report(report, local, &mints);
        }
        for local in reports_second {
            unmatched += merge_report(report, local, &mints);
        }

        // 便宜的兜底:改写后产物里不得残留哨兵(真实 id 不含控制字符 ⇒ 哨兵只可能来自
        // 本方案的临时 id)。只在调试构建断言,不改变发布行为的开销。
        debug_assert_eq!(
            unmatched, 0,
            "产物里残留了 {unmatched} 处临时 id 哨兵:实体级并行的 id 改写漏了字段"
        );

        // 确定性模式(=对齐/回归测试用)把时钟也钉死,保证两次转换逐字节一致
        report.blocks_converted = converted;
        report.elapsed_ms = started.elapsed().as_millis();
        let now_ms = if deterministic {
            0
        } else {
            assembly::current_epoch_ms()
        };
        assembly::build_document(source, entities, &procedures, now_ms, report)
            .map_err(TranslateError::from)
    })();

    // 无论成败都把 `block_data_json` 放回源文档(转换不改源文档)
    restore_forward_items(source, &mut items);
    outcome
}

/// 把 `block_data_json` 里的**内联对象形态影子**改写成影子 XML;没有这种影子时返回 `None`。
///
/// 平台上有作品的影子不是 XML 字符串而是**对象**(`shadows: {槽: {type, id, visible,
/// editable, fields}}`;第三十三轮实测 `A28社区-开幕_174408420`,全语料仅此一件),而本库
/// 内部一律吃 XML 字符串(`model::BlockJson::shadows` 是 `BTreeMap<String, String>`)⇒
/// 直接喂进去只会在深层反序列化时报 `invalid type: map, expected a string`。
///
/// ## 目标形态的依据(都是平台自己的东西,不是自创写法)
///
/// **同一件作品的平台原件**:`download/compile/k4edit/174408420-*.bcm4`(`ide/load` 拿到的
/// 编辑器亲手写出的源文件)里同一批影子的写法,与对象逐槽比过 800 对(按影子 id 配对):
///
/// - 影子元素:`<shadow xmlns="http://www.w3.org/1999/xhtml" type="{type}" id="{id}"
///   visible="{visible}">…</shadow>`(属性集与对象键一一对应;`editable=false` 的占位影子
///   平台写的是 `<empty … editable="false">`,见下);
/// - 字段:`<field {fields 里除 name/text 外的键=属性} name="{fields.name}">{fields.text}</field>`
///   —— 对象把"字段名/字段文本"放在 `name`/`text` 两个键上,其余键(`constraints`/
///   `allow_text`/`has_been_edited`…)是**字段元素的属性**;
/// - 字段文本为空 → 自闭合 `<field … name="X"/>`(与平台一致)。
///
/// 这套写法同时是**本管线自己合成影子时的形态**(`xml::math_number_shadow`、
/// `nemo_mapping::render_shadow_xml` 都是"真实字段名 + 字段属性"),与 KN 侧语料
/// (`download/compile/*.bcmkn`)一致 ⇒ 产物里不会混进第二种影子方言。
///
/// 对象表达不了的**渲染属性**(`inline`/`deletable`:平台侧由块定义/实例状态决定)会丢。
/// 另外平台在**字符串**形态里对"空槽"写 `""`,而仅凭对象分不出该写 `""` 还是 `<empty>` ⇒
/// 统一按 `editable=false` 写平台的 `<empty … editable="false">`(保住 id 与 `editable`
/// 两个事实;`""` 里没有 id ⇒ 占位影子的 id 会整批丢。量法与读数见 [`object_shadow_xml`])。
///
/// 只返回**副本**:调用方(见 [`parse_forward_item`])拿它去解析,源文档一字不动。
fn normalize_object_shadows(block_data_json: &Value) -> Option<Value> {
    let bdj = block_data_json.as_object()?;
    match bdj.get("blocks") {
        // 编辑格式:`blocks` 就是 id → 积木对象的字典(积木树靠 `connections` 表达,不在嵌套里)
        Some(Value::Object(blocks)) => {
            if !blocks.values().any(has_object_shadow) {
                return None; // 绝大多数作品走这里:一次廉价探测,零拷贝
            }
            let mut copy = bdj.clone();
            let Some(Value::Object(copied)) = copy.get_mut("blocks") else {
                return None;
            };
            for block in copied.values_mut() {
                normalize_block_shadows(block);
            }
            Some(Value::Object(copy))
        }
        // 旧/另一形态:`blocks` 是一段 JSON 字符串,里面再套一层 `{blocks, connections}`
        Some(Value::String(text)) => {
            let inner: Value = serde_json::from_str(text).ok()?;
            let normalized = normalize_object_shadows(&inner)?;
            let mut copy = bdj.clone();
            copy.insert(
                "blocks".into(),
                Value::String(serde_json::to_string(&normalized).ok()?),
            );
            Some(Value::Object(copy))
        }
        _ => None,
    }
}

/// 这个积木对象的 `shadows` 里有没有对象形态的影子(只探测,不改)
fn has_object_shadow(block: &Value) -> bool {
    block
        .get("shadows")
        .and_then(Value::as_object)
        .is_some_and(|shadows| shadows.values().any(Value::is_object))
}

/// 一个积木对象的 `shadows` 槽:对象形态的就地改写成 XML 字符串
fn normalize_block_shadows(block: &mut Value) {
    let Some(shadows) = block.get_mut("shadows").and_then(Value::as_object_mut) else {
        return;
    };
    for value in shadows.values_mut() {
        let Some(rendered) = value.as_object().and_then(object_shadow_xml) else {
            continue; // 字符串形态(以及别的怪东西)原样保留
        };
        *value = Value::String(rendered);
    }
}

/// 一个内联对象影子 → 影子 XML(形态依据见 [`normalize_object_shadows`];转义与自闭合复用
/// [`xml::XmlNode::serialize`],与官方 `XMLSerializer` 同口径)
fn object_shadow_xml(shadow: &Map<String, Value>) -> Option<String> {
    let kind = shadow.get("type")?.as_str()?;
    let id = shadow.get("id").and_then(Value::as_str).unwrap_or_default();
    let visible = shadow
        .get("visible")
        .and_then(Value::as_str)
        .unwrap_or("visible");
    let editable = shadow
        .get("editable")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    // 平台在 `editable=false` 的占位影子上写 `<empty … editable="false">`(不是 `<shadow>`)。
    //
    // 也**必须**写出带 id 的元素:`""`(平台**字符串**形态对空槽的写法)里没有 id ⇒ 占位影子的 id 整批丢。
    // 实测(唯一一件对象形态语料 `A28社区-开幕_174408420`,它带 515 个 `logic_empty` 占位对象):
    // 把本分支临时改成 `String::new()` 再跑 `cargo test --lib k4_corpus_round_trip_sweep -- --nocapture`,
    // 该作品的 `[id台账]` 影子列 **603 → 943**(+340);改回 `<empty …>` 仍是 603
    // ⇒ 比 `""` 多保住 **340** 条占位影子 id(其余 515−340=175 条的槽本来就被正向映射重写,两种写法都保不住)。
    let mut node = xml::XmlNode::new(if editable { "shadow" } else { "empty" });
    node.set_attr("xmlns", XHTML);
    node.set_attr("type", kind);
    node.set_attr("id", id);
    node.set_attr("visible", visible);
    if !editable {
        node.set_attr("editable", "false");
        return Some(node.serialize());
    }

    if let Some(fields) = shadow.get("fields").and_then(Value::as_object) {
        let mut field = xml::XmlNode::new("field");
        for (key, value) in fields {
            if key != "name" && key != "text" {
                field.set_attr(key, &shadow_field_text(value));
            }
        }
        field.set_attr(
            "name",
            fields
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        field.text = fields
            .get("text")
            .map_or_else(String::new, shadow_field_text);
        node.children.push(field);
    }
    Some(node.serialize())
}

/// 影子字段值的文本形态(对象里的字段值实测都是字符串;其余值按 JSON 文本兜底)
fn shadow_field_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// KittenN 编辑版 → Kitten4 编辑版(自建反向管线)
///
/// 步骤与正向**对称**:`model::parse_kn_entity`(树)→ `mapping::translate_kn_to_kitten`(语义反演)
/// → `model::unrewrite_calls`(`KC` 的逆)+ `model::def_root_from_entry`(`zC` 的逆)
/// → `model::build_block_data_json`(邻接表)→ Kitten4 文档装配。
///
/// 画布:`StageOrientation::Auto`(默认)把源 `stageSize` 原样当 Kitten4 的 `size`,于是
/// `landscape = width > height` 两侧一致(清屏/坐标换算自洽);显式指定 `Portrait`/`Landscape`
/// 会换掉画布尺寸,坐标也随之按 `canvas / stageSize` 的比例**重新换算**
///(`kitten4_position`,与正向 `stage_position` 互逆),所以搬到另一种画布时位置仍然对应;
/// 只有**横屏壳的判定**(`mapping` 里的 `landscape`)仍按源 `stageSize`,不受本选项影响。
pub(super) fn convert_kn_document(
    source: &serde_json::Value,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    use serde_json::Value;
    let started = std::time::Instant::now();

    let src = source.as_object().ok_or_else(|| {
        TranslateError::InvalidArgument("源作品不是 JSON 对象:无法按 KittenN 作品解析".into())
    })?;
    let (kn_w, kn_h) = assembly::kn_stage_size(src);
    let landscape = kn_w > kn_h;
    let (canvas_w, canvas_h) = match options.orientation() {
        StageOrientation::Portrait => (tables_gen::STAGE_PORTRAIT.0, tables_gen::STAGE_PORTRAIT.1),
        StageOrientation::Landscape => {
            (tables_gen::STAGE_LANDSCAPE.0, tables_gen::STAGE_LANDSCAPE.1)
        }
        StageOrientation::Auto => (kn_w, kn_h),
    };

    let mut ids = model::IdSource::new(options.ids_deterministic());

    // ── 第一遍:解析 + 语义反演(场景在前、角色在后,与正向一致)
    let mut entities: Vec<KnEntity> = Vec::new();
    for (is_scene, container) in [(true, "scenes"), (false, "actors")] {
        let Some(map) = src
            .get(container)
            .and_then(Value::as_object)
            .and_then(|outer| outer.get(if is_scene { "scenesDict" } else { "actorsDict" }))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (id, entity) in map {
            let mut tree =
                model::parse_kn_entity(entity.get("nekoBlockJsonList").unwrap_or(&Value::Null))?;
            report.blocks_total += tree.count();
            mapping::translate_kn_to_kitten(&mut tree, landscape, report);
            // P2(rounds/37):实体对象里最大的键是 `nekoBlockJsonList`(上一步已解析成 `tree`),
            // 而装配侧读 `source` 只取标量/小数组(名字、坐标、造型、`actorIds`…),**从不读它**
            // ⇒ 克隆时跳过,省下整份 KN 积木 JSON 的第二次深拷(9 MB 级作品 ≈10⁵ 个节点)。
            let mut source = serde_json::Map::new();
            if let Some(object) = entity.as_object() {
                for (key, value) in object {
                    if key != "nekoBlockJsonList" {
                        source.insert(key.clone(), value.clone());
                    }
                }
            }
            entities.push(assembly::KnEntity {
                source_id: id.clone(),
                is_scene,
                tree,
                source,
            });
        }
    }

    // ── 程序集(`proceduresDict`):定义体同样要过一遍语义反演
    let mut procedures = model::parse_kn_procedures(src.get("procedures").unwrap_or(&Value::Null))?;
    for entry in &mut procedures {
        report.blocks_total += entry.tree.count();
        mapping::translate_kn_to_kitten(&mut entry.tree, landscape, report);
    }
    let call_targets = model::call_targets(&procedures);
    for entity in &mut entities {
        model::unrewrite_calls(&mut entity.tree, &call_targets, &mut ids, report);
    }
    // 定义体里的调用点同样要还原(正向只重写实体树,定义体里留着 id 的话 Kitten4 会看到拿不到引用的 UUID;
    // 这是**有意的超集**,与 `annotate_param_refs` 的超集口径一致 —— 类型计数不受影响)
    for entry in &mut procedures {
        model::unrewrite_calls(&mut entry.tree, &call_targets, &mut ids, report);
    }

    // 定义根积木挂到第一个角色(没有角色就挂到第一个场景;都没有 → 报告后跳过)
    let host = entities
        .iter()
        .position(|entity| !entity.is_scene)
        .or(if entities.is_empty() { None } else { Some(0) });
    for entry in &procedures {
        let root = model::def_root_from_entry(entry, &mut ids, report)?;
        match host {
            Some(index) => entities[index].tree.roots.push(root),
            None => report.warn(TranslateWarning::DroppedProperty {
                path: format!("procedures.{}(作品没有实体可挂载定义积木)", entry.name),
            }),
        }
    }

    // ── 第二遍:编码 + 装配
    let mut converted = 0usize;
    let mut blocks_by_entity: Vec<(usize, Value)> = Vec::with_capacity(entities.len());
    for (index, entity) in entities.iter().enumerate() {
        // `blocks` 是 id 字典:同一 id 出现两次(正向 `KC` 的复制语义/菱形展开)必须重铸第二个,
        // 否则互相覆盖 —— 逐条记进报告(`RemintedId` 不算有损:内容都在,换的只是 id)
        for id in assembly::duplicate_ids(&entity.tree) {
            report.warn(TranslateWarning::RemintedId { from: id });
        }
        let value = model::build_block_data_json(&entity.tree, &mut ids)?;
        converted += entity.tree.count();
        blocks_by_entity.push((index, value));
    }
    report.blocks_converted = converted;
    report.elapsed_ms = started.elapsed().as_millis();

    Ok(assembly::build_kitten4_document(
        src,
        entities,
        blocks_by_entity,
        assembly::StageSize {
            landscape,
            canvas: (canvas_w, canvas_h),
            kn: (kn_w, kn_h),
        },
        &mut ids,
        report,
    ))
}

// 来自 src/core/convert/translate/remint.rs
// 实体级并行(方案 25 S3a)的两件工具:**临时 id 的改写** + **工作项并行调度**。
// 正向管线的处理顺序决定了产物里新铸 id 的**值**(id = 第几次铸造的纯函数)与
// 告警顺序,所以并行不能让每个线程各自铸 id。做法(方案 25 §3):
// 1. 每个工作项(实体)用 [`IdSource::recording`](super::model::IdSource::recording)
// 产出**临时 id**(`\u{1}prov:{槽位}:{序}:{形态}`),并把"本次铸造是什么形态"
// 记进账本(槽位在同一份文档里两两不同 ⇒ 临时 id 全局唯一);
// 2. 串行阶段按「阶段 1 全项 → 阶段 2 全项」拼账本,用**一个**串行 `IdSource` 兑现
// 最终 id —— 确定性模式下与"今天串行实现"逐个相同;
// 3. 本模块负责改写:临时 id 会落在**值**(节点 `id` / `fields` / `parent_id`)与
// **mutation / shadow XML 字符串**里;而 `rewrite_calls` 还会把形参 id 写成
// `inputs` / `shadows` 的 **BTreeMap 键** —— 今天那些形参 id 来自源积木,但"键也过一遍
// 改写"是必须保留的能力(一旦某个形参 id 本身是现铸 id —— Label 形参就是现铸 uuid ——
// 漏掉键就会让产物里悄悄留下哨兵);三种位置都要覆盖,改写后由 `debug_assert` 兜底。
// 改写是**单遍扫描**:按哨兵前缀定位 token 再查表,代价 O(产物字符串总长),
// 而不是"每个字符串 × 每个临时 id"(后者在 10 MB 级作品上是数千万次子串搜索)。

/// 临时 id → 最终 id 的改写表
pub(super) type IdRemap = HashMap<String, String>;

// ---------------------------------------------------------------- 并行调度

/// 实际线程数:≤ 请求并发、≤ 工作项数、≤ 可用核数
///
/// 作品级(批量)与实体级是**两级**并发,预算折算在
/// [`TranslateOptions::fold_entity_concurrency`](super::TranslateOptions::fold_entity_concurrency)
/// 里做(方案 25 §7 阻塞 #6);这里只负责"别为 3 个实体开 32 个线程、别超出核数"。
pub(super) fn workers(requested: usize, items: usize) -> usize {
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
pub(super) fn run_items<I, T, F>(
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
pub(super) fn remap_tree(map: &IdRemap, tree: &mut BlockTree) -> (usize, usize) {
    let mut nodes = 0;
    let mut unmatched = 0;
    for root in &mut tree.roots {
        remap_node(map, root, &mut nodes, &mut unmatched);
    }
    (nodes, unmatched)
}

/// 改写一条程序集条目(条目 id / 形参 id / 定义体积木);返回 `(节点数, 未命中数)`
pub(super) fn remap_entry(map: &IdRemap, entry: &mut ProcedureEntry) -> (usize, usize) {
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
pub(super) fn merge_report(
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
        TranslateWarning::UnmappedBlock {
            kind,
            marked,
            cleared_shadows,
        } => TranslateWarning::UnmappedBlock {
            kind: remap_owned(map, kind, unmatched),
            marked,
            cleared_shadows,
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
        TranslateWarning::AmbiguousType {
            kind,
            candidates,
            chosen,
        } => TranslateWarning::AmbiguousType {
            kind: remap_owned(map, kind, unmatched),
            candidates: candidates
                .into_iter()
                .map(|c| remap_owned(map, c, unmatched))
                .collect(),
            chosen: chosen.map(|c| remap_owned(map, c, unmatched)),
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
pub(super) fn remap_json(map: &IdRemap, value: &mut Value) -> usize {
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
mod forward_parallel_tests {
    //! 正向实体级并行的守门测试(方案 25 §8):**不依赖 `download/` 样本**(CI 可跑;
    //! 另有一条真作品差分门,样本缺失时自跳过,与仓库其它真机测试同约定)。
    //!
    //! 自造一份 1 场景 + 2 角色的 Kitten4 文档,并让**跨实体**的程序集调用真实发生
    //! (角色 `a1` 定义程序集、角色 `a2` 调用它),因此会同时覆盖:
    //!
    //! - 阶段 1 的铸造(`mapping` 的横屏算术壳与列表影子、程序集 Label 形参、
    //!   返回值定义的 ROUND 条目、`VC` 补默认 VALUE);
    //! - 阶段 2 把程序集 id / 形参 id 复制进实体树的三类位置:值(`fields.NAME`、节点 id)、
    //!   **BTreeMap 的键**(形参 id 成为 `inputs` / `shadows` 的键)、mutation 与 shadow XML 字符串
    //!   (Label 形参是现铸 uuid,它的 id 就写在 mutation 串里)。
    //!
    //! 断言:
    //!
    //! 1. `entity_concurrency = 1` 与 `= 8` 产物**逐字节相同**、告警**逐条同序**;
    //! 2. 两者都与**测试内的串行参考实现**(实体级并行前的管线)逐字节相同
    //!    (真作品口径由 `tests/convert_bench.rs` 的 SHA256 基线守);
    //! 3. 产物里不残留临时 id 哨兵、uuid 计数值无空洞、调用点确实按最终 id 重写;
    //! 4. 转换不改源文档(`block_data_json` 原样放回,可重复转换)。

    use super::*;
    use crate::core::convert::translate::model::{BlockJson, BlockTree, IdSource};
    use serde_json::{Map, Value, json};

    fn node(value: Value) -> BlockJson {
        BlockJson::from_value(&value).expect("自造节点")
    }

    /// 树 → 源文档里的 `block_data_json = {blocks, connections, comments}`
    fn block_data_json(tree: &BlockTree) -> Value {
        model::build_block_data_json(tree, &mut IdSource::new(true)).expect("编码 block_data_json")
    }

    /// 场景:一条横屏会被 `/1.3` 包装的坐标积木(`mapping` 一次铸 5 个 id)
    fn scene_tree() -> BlockTree {
        let mut hat = node(json!({ "type": "start_on_click", "id": "hatS" }));
        hat.next = Some(Box::new(node(json!({
            "type": "self_move_to",
            "id": "sceneMove",
            "inputs": {
                "x": { "type": "math_number", "id": "sx", "is_shadow": true, "fields": { "TEXT": "10" } },
                "y": { "type": "math_number", "id": "sy", "is_shadow": true, "fields": { "TEXT": "20" } }
            }
        }))));
        BlockTree::new(vec![hat])
    }

    /// 角色 A:带形参(含形参引用)的定义 + 带返回值的定义(触发 ROUND 与补默认 VALUE)
    /// + 一条列表积木(`fields.list` 合成 `inputs.list` 影子)
    fn actor_a_tree() -> BlockTree {
        BlockTree::new(vec![
            node(json!({ "type": "start_on_click", "id": "hatA" })),
            node(json!({
                "type": "procedures_2_defnoreturn",
                "id": "defParams",
                "fields": { "NAME": "跨实体调用" },
                "inputs": {
                    "PARAMS0": { "type": "procedures_2_stable_parameter", "id": "p0",
                                 "fields": { "param_name": "X" } },
                    "PARAMS1": { "type": "procedures_2_stable_parameter", "id": "p1",
                                 "fields": { "param_name": "Speed" } }
                },
                "statements": { "STACK": {
                    "type": "self_move_to",
                    "id": "mv",
                    "inputs": {
                        "x": { "type": "procedures_2_parameter", "id": "refX",
                               "fields": { "param_name": "X" } },
                        "y": { "type": "math_number", "id": "mvY", "is_shadow": true,
                               "fields": { "TEXT": "0" } }
                    }
                } }
            })),
            node(json!({
                "type": "procedures_2_defnoreturn",
                "id": "defReturn",
                "fields": { "NAME": "取数" },
                "statements": { "STACK": {
                    "type": "procedures_2_return_value",
                    "id": "ret",
                    "inputs": { "VALUE": { "type": "math_number", "id": "num", "is_shadow": true,
                                           "fields": { "TEXT": "7" } } },
                    "shadows": { "VALUE": "<shadow type=\"math_number\"/>" },
                    "next": { "type": "procedures_2_return_value", "id": "ret2" }
                } }
            })),
            node(json!({
                "type": "list_append",
                "id": "listRoot",
                "fields": { "list": "mylist" },
                "inputs": { "VALUE": { "type": "math_number", "id": "lnum", "is_shadow": true,
                                       "fields": { "TEXT": "1" } } }
            })),
        ])
    }

    /// 角色 B:调用方与 A 的定义同名 —— 阶段 2 会把 A 的临时 id 复制进 B 的树
    fn actor_b_tree() -> BlockTree {
        let mut hat = node(json!({ "type": "start_on_click", "id": "hatB" }));
        hat.next = Some(Box::new(node(json!({
            "type": "procedures_2_callnoreturn",
            "id": "call1",
            // 多带一个字段:`rewrite_calls` 会把它记成 `DroppedField`(路径里带程序集 id),
            // 用来覆盖"告警串也要改写临时 id"的那条路
            "fields": { "NAME": "跨实体调用", "EXTRA": "x" },
            "shadows": { "NAME": "<shadow type=\"text\"/>" },
            "inputs": {
                "ARG0": { "type": "math_number", "id": "argX", "is_shadow": true,
                          "fields": { "TEXT": "160" } },
                "ARG1": { "type": "math_number", "id": "argSpeed", "is_shadow": true,
                          "fields": { "TEXT": "-260" } }
            }
        }))));
        BlockTree::new(vec![hat])
    }

    /// 自造文档(横向:让横屏包装路径真的跑起来)
    fn multi_entity_document() -> Value {
        json!({
            "project_name": "实体级并行自造样本",
            "size": { "width": 900, "height": 562 },
            "theatre": {
                "scenes": {
                    "s0": { "name": "背景", "block_data_json": block_data_json(&scene_tree()) }
                },
                "actors": {
                    "a1": { "name": "甲", "x": 0, "y": 0, "scale": 1, "lock": false,
                            "block_data_json": block_data_json(&actor_a_tree()) },
                    "a2": { "name": "乙", "x": 10, "y": 20, "scale": 2, "lock": true,
                            "block_data_json": block_data_json(&actor_b_tree()) }
                },
                "scenes_order": ["s0"],
                "current_scene_id": "s0"
            },
            "broadcasts": {}
        })
    }

    /// 走**新**管线(实体级并行),返回产物与报告(`pub(super)`:W10 的对象影子测试在隔壁
    /// `object_shadow_tests` 里复用同一个口径)
    pub(super) fn convert(
        source: &mut Value,
        entity_concurrency: usize,
    ) -> (Value, TranslateReport) {
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new()
            .deterministic_ids(true)
            .entity_concurrency(entity_concurrency);
        let document = convert_kitten4_document(source, &options, &mut report).expect("正向转换");
        (document, report)
    }

    /// **实体级并行前的串行管线**(参考实现):逐实体 parse/mapping/split → 逐实体
    /// `rewrite_calls` → 逐实体编码 → 装配,全程**一个**全局 `IdSource`。
    ///
    /// 放在测试里当差分门:并行实现若把铸造顺序、告警顺序或改写范围做错,产物/告警立刻对不上。
    fn reference_serial_document(source: &Value) -> (Value, TranslateReport) {
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let mut ids = IdSource::new(true);
        let landscape = source["size"]["width"].as_f64().unwrap_or(0.0)
            > source["size"]["height"].as_f64().unwrap_or(0.0);
        let theatre = source["theatre"].as_object().expect("theatre");

        let mut parsed: Vec<(String, bool, model::BlockTree, Map<String, Value>)> = Vec::new();
        let mut procedures: Vec<model::ProcedureEntry> = Vec::new();
        for (is_scene, container) in [(true, "scenes"), (false, "actors")] {
            let Some(map) = theatre.get(container).and_then(Value::as_object) else {
                continue;
            };
            for (id, entity) in map {
                let entity = entity.as_object().expect("实体");
                let mut tree = match entity.get("block_data_json") {
                    Some(bdj) => model::parse_block_data_json(bdj).expect("解析实体"),
                    None => model::BlockTree::default(),
                };
                report.blocks_total += tree.count();
                mapping::translate_kitten_to_kn(&mut tree, landscape, &mut ids, &mut report);
                let (kept, mut extracted) = model::split_procedures(tree, &mut ids, &mut report);
                procedures.append(&mut extracted);
                let mut src = entity.clone();
                src.remove("block_data_json");
                parsed.push((id.clone(), is_scene, kept, src));
            }
        }
        for (_, _, tree, _) in parsed.iter_mut() {
            model::rewrite_calls(tree, &procedures, &mut ids, &mut report);
        }
        let mut converted: usize = procedures.iter().map(|p| p.tree.count()).sum();
        let mut entities = Vec::with_capacity(parsed.len());
        for (id, is_scene, tree, src) in parsed {
            let blocks = model::tree_to_json(&tree).expect("编码");
            converted += tree.count();
            entities.push(assembly::ConvertedEntity {
                source_id: id,
                is_scene,
                blocks,
                source: src,
            });
        }
        report.blocks_converted = converted;
        let document =
            assembly::build_document(source, entities, &procedures, 0, &mut report).expect("装配");
        (document, report)
    }

    /// 产物里所有 `00000000-0000-4000-8000-xxxxxxxxxxxx` 的计数(升序去重)
    ///
    /// 要扫**字符串内部**:一部分现铸 id 只出现在 shadow XML 里(`id="…"`)。
    fn minted_counters(document: &Value) -> Vec<u64> {
        const PREFIX: &str = "00000000-0000-4000-8000-";
        fn walk(value: &Value, out: &mut Vec<u64>) {
            match value {
                Value::String(text) => {
                    let mut rest = text.as_str();
                    while let Some(at) = rest.find(PREFIX) {
                        let tail = &rest[at + PREFIX.len()..];
                        if tail.len() >= 12 && tail[..12].bytes().all(|b| b.is_ascii_hexdigit()) {
                            out.push(u64::from_str_radix(&tail[..12], 16).unwrap_or(0));
                        }
                        rest = tail;
                    }
                }
                Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
                Value::Object(object) => object.values().for_each(|item| walk(item, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(document, &mut out);
        out.sort_unstable();
        out.dedup();
        out
    }

    /// 真作品差分门(缺样本即跳过,与仓库其它真机测试同约定):自造文档挡不住"只有真作品
    /// 里才有的铸造点",所以对真实 `.bcm4` 再做一次三方逐字节对照。
    ///
    /// 这条门正是抓出"阶段 1 与阶段 2 记账槽位撞车"的那条(见 `docs/rounds/25` §9.3)。
    #[test]
    fn real_work_matches_serial_reference_when_sample_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("download/compile/几何对战-联机_215246857.bcm4");
        if !path.exists() {
            super::super::missing_fixture(&format!("真作品样本 {}", path.display()));
            return;
        }
        let source: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读作品")).expect("JSON");

        let (serial_doc, serial_report) = convert(&mut source.clone(), 1);
        let (parallel_doc, parallel_report) = convert(&mut source.clone(), 8);
        let (reference_doc, reference_report) = reference_serial_document(&source);
        let available = std::thread::available_parallelism().map_or(1, |n| n.get());
        if available >= 2 {
            assert!(
                parallel_report.entity_workers > 1,
                "真作品多实体 + 请求并发 8 应真的并行(可用核数 {available})"
            );
        }
        let serial_text = serde_json::to_string(&serial_doc).expect("序列化");

        assert_eq!(
            serial_text,
            serde_json::to_string(&parallel_doc).expect("序列化"),
            "真作品:并发 1 与 8 的产物不一致"
        );
        assert_eq!(
            serial_text,
            serde_json::to_string(&reference_doc).expect("序列化"),
            "真作品:与串行参考实现的产物不一致"
        );
        assert_eq!(
            serial_report.warnings(),
            reference_report.warnings(),
            "真作品:与串行参考实现的告警不一致"
        );
        assert_eq!(serial_report.blocks_total, reference_report.blocks_total);
        assert_eq!(
            serial_report.blocks_converted,
            reference_report.blocks_converted
        );
        assert!(
            !serial_text.contains('\u{1}'),
            "真作品产物里残留了临时 id 哨兵"
        );
    }

    #[test]
    fn parallel_and_serial_products_are_identical() {
        let mut source = multi_entity_document();
        let pristine = source.clone();
        let (serial_doc, serial_report) = convert(&mut source, 1);
        assert_eq!(
            source, pristine,
            "转换不得改动源文档(block_data_json 必须原样放回)"
        );

        let (parallel_doc, parallel_report) = convert(&mut pristine.clone(), 8);
        let serial_text = serde_json::to_string(&serial_doc).expect("序列化");
        let parallel_text = serde_json::to_string(&parallel_doc).expect("序列化");

        // 空门守卫:并发跑的那一次必须**真的**开了多线程(否则"1 vs 8 相同"只是串行 vs 串行;
        // `taskset -c 2` 会把 `available_parallelism` 折成 1,那种环境下只报事实不判失败)
        assert_eq!(serial_report.entity_workers, 1, "并发 1 不应开工作线程");
        let available = std::thread::available_parallelism().map_or(1, |n| n.get());
        if available >= 2 {
            assert!(
                parallel_report.entity_workers > 1,
                "3 个工作项 + 请求并发 8 应真的并行(可用核数 {available})"
            );
        }
        assert_eq!(
            parallel_report.entity_workers,
            std::cmp::min(8, std::cmp::min(3, available.max(1))),
            "实际线程数 = min(请求, 工作项数, 可用核数)"
        );

        // ① 并发 1 vs 8:产物逐字节相同、告警逐条同序
        assert_eq!(
            serial_text, parallel_text,
            "并发 1 与 8 的产物必须逐字节相同"
        );
        assert_eq!(
            serial_report.warnings(),
            parallel_report.warnings(),
            "告警必须逐条同序同内容"
        );
        assert_eq!(serial_report.blocks_total, parallel_report.blocks_total);
        assert_eq!(
            serial_report.blocks_converted,
            parallel_report.blocks_converted
        );
        assert!(serial_report.blocks_total > 0, "自造样本应产生积木");
        assert!(
            serial_report.warnings().iter().any(|warning| matches!(
                warning,
                TranslateWarning::DroppedField { path } if path.starts_with("procedures_2_call.")
            )),
            "自造样本应产出调用点字段告警(否则告警改写这条路没验到):{:#?}",
            serial_report.warnings()
        );
        assert!(
            serial_report
                .warnings()
                .iter()
                .all(|warning| !format!("{warning:?}").contains('\u{1}')),
            "告警里不得残留哨兵:{:#?}",
            serial_report.warnings()
        );

        // ② 与"实体级并行前的串行实现"逐字节相同
        let (reference_doc, reference_report) = reference_serial_document(&pristine);
        assert_eq!(
            serde_json::to_string(&reference_doc).expect("序列化"),
            serial_text,
            "与串行参考实现的产物不一致"
        );
        assert_eq!(
            reference_report.warnings(),
            serial_report.warnings(),
            "与串行参考实现的告警顺序不一致"
        );
        assert_eq!(reference_report.blocks_total, serial_report.blocks_total);
        assert_eq!(
            reference_report.blocks_converted,
            serial_report.blocks_converted
        );

        // ③ 无哨兵残留;uuid 计数值无空洞(临时 id 的账本一个不漏地兑现了)
        assert!(!serial_text.contains('\u{1}'), "产物里残留了临时 id 哨兵");
        let counters = minted_counters(&serial_doc);
        assert!(counters.len() >= 10, "样本应铸出足够多的 id:{counters:?}");
        assert_eq!(
            counters,
            (1..=counters.len() as u64).collect::<Vec<_>>(),
            "uuid 计数必须从 1 起连续(有空洞=某次铸造没被兑现)"
        );

        // ④ 跨实体调用点确实按**最终** id 重写:值 / 键 / mutation 三处都换过
        let actors = serial_doc["actors"]["actorsDict"]
            .as_object()
            .expect("actorsDict");
        let a2_blocks = actors["a2"]["nekoBlockJsonList"]
            .as_array()
            .expect("a2 积木");
        let call = &a2_blocks[0]["next"];
        assert_eq!(call["type"], json!("procedures_2_callnoreturn"));
        assert_eq!(
            call["fields"]["NAME"],
            json!("defParams"),
            "调用点 NAME 应换成程序集条目 id"
        );
        let mutation = call["mutation"].as_str().expect("调用点 mutation");
        assert!(mutation.contains("def_id=\"defParams\""), "{mutation}");
        assert!(mutation.contains("type=\"NORMAL\">"), "{mutation}");
        assert!(mutation.contains("content=\"X\""), "{mutation}");
        assert!(
            call["inputs"]
                .as_object()
                .expect("inputs")
                .contains_key("p0"),
            "String 形参 id 应成为 inputs 的键:{:?}",
            call["inputs"]
        );
        let shadows = call["shadows"].as_object().expect("shadows");
        assert!(
            shadows.contains_key("p0")
                && shadows["p0"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("type=\"math_number\""),
            "String 形参 id 应成为 shadows 的键:{shadows:?}"
        );
        assert!(
            !shadows
                .values()
                .any(|xml| xml.as_str().unwrap_or_default().contains('\u{1}')),
            "shadow XML 里残留哨兵:{shadows:?}"
        );

        // `proceduresDict`:两条 NORMAL + 一条 ROUND(ROUND 的 id 是现铸 uuid)
        let procedures = serial_doc["procedures"]["proceduresDict"]
            .as_object()
            .expect("proceduresDict");
        assert!(procedures.contains_key("defParams"));
        assert!(procedures.contains_key("defReturn"));
        let kinds: Vec<&str> = procedures
            .values()
            .map(|entry| entry["type"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            kinds.iter().filter(|kind| **kind == "NORMAL").count(),
            2,
            "{kinds:?}"
        );
        assert_eq!(
            kinds.iter().filter(|kind| **kind == "ROUND").count(),
            1,
            "{kinds:?}"
        );
    }

    /// 失败路径也必须把源文档还原:旧实现在循环中途失败时,只还原了坏实体**之前**的实体
    /// (坏实体自己的 `block_data_json` 留在"已取走"状态)。并行实现是"全部取走 → 全部放回",
    /// 所以失败也要还原全部 —— 这条把它钉住。
    #[test]
    fn error_path_still_restores_source_document() {
        let mut source = multi_entity_document();
        // 让 a2(第三个工作项)的连接指向不存在的积木 → 解析必然失败
        source["theatre"]["actors"]["a2"]["block_data_json"]["connections"]["hatB"] =
            json!({ "不存在的积木": { "type": "next" } });
        let pristine = source.clone();
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let error = convert_kitten4_document(
            &mut source,
            &TranslateOptions::new()
                .deterministic_ids(true)
                .entity_concurrency(4),
            &mut report,
        )
        .expect_err("坏积木图必须报错");
        match &error {
            TranslateError::Convert(inner) => {
                assert!(inner.to_string().contains("不存在的积木"), "{inner}");
            }
            other => panic!("错误类型不符:{other}"),
        }
        assert_eq!(source, pristine, "失败路径同样不得改动源文档");
    }

    /// 两级并发预算折算(方案 25 §7 阻塞 #6):作品级 × 实体级不得超过可用核数
    ///
    /// 有效作品并发 = `min(batch_concurrency, 作品数)`;每作品分到 `可用核数 / 有效作品并发`。
    #[test]
    fn entity_concurrency_is_folded_by_work_and_core_budget() {
        let requested = TranslateOptions::new().entity_concurrency(8);
        assert_eq!(
            requested
                .clone()
                .fold_entity_concurrency(1, 4)
                .entity_workers(),
            4,
            "单作品:可用核数就是实体级上限"
        );
        assert_eq!(
            requested
                .clone()
                .batch_concurrency(2)
                .fold_entity_concurrency(2, 8)
                .entity_workers(),
            4,
            "2 作品并发 × 4 实体线程 = 8 核"
        );
        assert_eq!(
            requested
                .clone()
                .batch_concurrency(8)
                .fold_entity_concurrency(8, 8)
                .entity_workers(),
            1,
            "作品级已占满核数,实体级折成 1"
        );
        assert_eq!(
            requested
                .clone()
                .batch_concurrency(16)
                .fold_entity_concurrency(16, 8)
                .entity_workers(),
            1,
            "作品级超订时也不给实体级名额(share 至少 1)"
        );
        assert_eq!(
            TranslateOptions::new()
                .fold_entity_concurrency(2, 1)
                .entity_workers(),
            1,
            "单核机器:实体级折成串行"
        );
        assert_eq!(
            TranslateOptions::new()
                .entity_concurrency(8)
                .fold_entity_concurrency(1, 64)
                .entity_workers(),
            8,
            "核多用不满时不吃掉用户请求值"
        );
        assert_eq!(
            TranslateOptions::new()
                .batch_concurrency(1)
                .fold_entity_concurrency(8, 64)
                .entity_workers(),
            1,
            "默认 1 不会被折算顶上去"
        );
    }

    /// 扫产物里"像 uuid 的 token"(36 字符、只含十六进制与 `-`)并断言每个都是合法 uuid v4,
    /// 返回个数。
    ///
    /// 非确定性模式下 id 值是随机的,没法比对"值相同",这条替它兜住**形态与数量**:
    /// 形态错(版本/变体位、段长)或数量变(少铸/多铸/漏改)都会立刻失败。
    fn count_and_check_uuid_shapes(document: &Value) -> usize {
        let text = serde_json::to_string(document).expect("序列化");
        let bytes = text.as_bytes();
        let mut at = 0;
        let mut checked = 0;
        while at < bytes.len() {
            if !(bytes[at].is_ascii_hexdigit() || bytes[at] == b'-') {
                at += 1;
                continue;
            }
            let start = at;
            while at < bytes.len() && (bytes[at].is_ascii_hexdigit() || bytes[at] == b'-') {
                at += 1;
            }
            // 源作品里的实体 id 是 base62(含 `g`/`s` 这类非 hex 字符)、长度也不是 36,
            // 所以"整段恰好 36 且含 4 个连字符"就是 uuid 形态的唯一候选
            let token = &text[start..at];
            if token.len() != 36 || token.matches('-').count() != 4 {
                continue;
            }
            let parts: Vec<&str> = token.split('-').collect();
            assert_eq!(
                parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
                vec![8, 4, 4, 4, 12],
                "id 段长不是 uuid v4:{token}"
            );
            assert!(parts[2].starts_with('4'), "id 版本位不是 4:{token}");
            assert!(
                matches!(parts[3].as_bytes().first(), Some(b'8' | b'9' | b'a' | b'b')),
                "id 变体位不对:{token}"
            );
            checked += 1;
        }
        checked
    }

    /// 非确定性模式(默认)只承诺 id **合法 + 唯一**(与串行实现同理,值本身随机):
    /// 这里验"产物里没有哨兵 + 每个 uuid 形态的 id 都合法 + 铸造次数与确定性模式一致"。
    #[test]
    fn parallel_run_keeps_ids_legal_and_unique_without_deterministic_mode() {
        let mut source = multi_entity_document();
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().entity_concurrency(4);
        let document =
            convert_kitten4_document(&mut source, &options, &mut report).expect("正向转换");
        let available = std::thread::available_parallelism().map_or(1, |n| n.get());
        if available >= 2 {
            assert!(report.entity_workers > 1, "请求并发 4 应真的并行");
        }

        let text = serde_json::to_string(&document).expect("序列化");
        assert!(!text.contains('\u{1}'), "非确定性模式同样不得残留临时 id");

        let random_ids = count_and_check_uuid_shapes(&document);
        assert!(
            random_ids >= 10,
            "样本应铸出足够多的 uuid(实际 {random_ids})"
        );

        // 与确定性模式对照:铸造**次数**(= 产物里 uuid 形态 id 的出现次数)必须一致
        // (确定性模式下的 uuid 计数连续性由 `parallel_and_serial_products_are_identical` 守)
        let mut deterministic_source = multi_entity_document();
        let (deterministic_doc, _) = convert(&mut deterministic_source, 4);
        assert_eq!(
            random_ids,
            count_and_check_uuid_shapes(&deterministic_doc),
            "非确定性模式不得改变铸造次数"
        );
    }
}

#[cfg(test)]
mod remint_tests {
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

/// 内联对象形态影子(第三十三轮被拒收的那类作品)的转换测试 —— W10。
#[cfg(test)]
mod object_shadow_tests {
    use super::forward_parallel_tests::convert;
    use super::*;
    use serde_json::json;

    // ---------------------------------------------------------------- 内联对象形态的影子(W10)

    /// 自造一份**内联对象影子**的 Kitten4 文档(竖屏 ⇒ 不触发横屏算术包装):
    /// 一个角色,`self_move_to` 的 `x` 是 `math_number` 对象影子(带 `constraints`/`allow_text`
    /// 两个额外字段键 + 空 `text`),`y` 是 `editable=false` 的 `logic_empty` 占位对象影子。
    fn object_shadow_document() -> Value {
        json!({
            "project_name": "内联对象影子自造样本",
            "size": { "width": 562, "height": 900 },
            "theatre": {
                "scenes": {},
                "actors": {
                    "a1": { "name": "甲", "block_data_json": {
                        "blocks": {
                            "hat": { "type": "start_on_click", "id": "hat" },
                            "move": {
                                "type": "self_move_to", "id": "move",
                                "shadows": {
                                    "x": {
                                        "editable": true, "id": "sx", "type": "math_number",
                                        "visible": "visible",
                                        "fields": {
                                            "allow_text": "true",
                                            "constraints": "-Infinity,Infinity,0,",
                                            "name": "NUM", "text": "10"
                                        }
                                    },
                                    "y": {
                                        "editable": false, "id": "sy", "type": "logic_empty",
                                        "visible": "visible"
                                    }
                                }
                            }
                        },
                        "connections": { "hat": { "move": { "type": "next" } } },
                        "comments": {}
                    } }
                },
                "scenes_order": [],
                "current_scene_id": null
            },
            "broadcasts": {}
        })
    }

    /// 对象影子 → 影子 XML:逐字段对照平台同款写法(`k4edit/174408420-*.bcm4` 的形态),
    /// 且整条路不报任何告警。
    #[test]
    fn object_shadow_is_rendered_as_platform_xml() {
        let mut document = object_shadow_document();
        // 源文档必须原样保留(改写只发生在解析用的副本上)
        let pristine = document.clone();
        let (product, report) = convert(&mut document, 1);
        assert_eq!(document, pristine, "转换不得改动源文档(含对象影子)");
        assert!(
            report.warnings().is_empty(),
            "对象影子应无损改写,不该有告警:{:#?}",
            report.warnings()
        );

        let entities = product["actors"]["actorsDict"]["a1"]["nekoBlockJsonList"]
            .as_array()
            .expect("a1 积木");
        // `self_move_to` 挂在 hat 的 `next` 上 ⇒ 递归找
        fn find_in<'a>(value: &'a Value, want: &str) -> Option<&'a Value> {
            match value {
                Value::Object(map) => {
                    if map.get("type").and_then(Value::as_str) == Some(want) {
                        return Some(value);
                    }
                    map.values().find_map(|child| find_in(child, want))
                }
                Value::Array(items) => items.iter().find_map(|child| find_in(child, want)),
                _ => None,
            }
        }
        let move_block = entities
            .iter()
            .find_map(|block| find_in(block, "self_move_to"))
            .expect("self_move_to 还在");
        let shadows = move_block["shadows"].as_object().expect("shadows");
        assert_eq!(
            shadows["x"],
            json!(
                "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"math_number\" id=\"sx\" \
                 visible=\"visible\"><field allow_text=\"true\" constraints=\"-Infinity,Infinity,0,\" \
                 name=\"NUM\">10</field></shadow>"
            ),
            "math_number 对象影子的形态:字段名取 `name`、文本取 `text`、其余键当字段属性"
        );
        assert_eq!(
            shadows["y"],
            json!(
                "<empty xmlns=\"http://www.w3.org/1999/xhtml\" type=\"logic_empty\" id=\"sy\" \
                 visible=\"visible\" editable=\"false\"/>"
            ),
            "editable=false 的占位影子按平台写成 <empty …>(不是 <shadow>)"
        );
    }

    /// 空 `text` 的字段自闭合(平台也这么写:`<field name=\"TEXT\"/>`),且对象影子里的
    /// **文本转义**走 XMLSerializer 口径。
    #[test]
    fn object_shadow_field_text_is_escaped_and_self_closed() {
        let empty = json!({
            "editable": true, "id": "s1", "type": "text", "visible": "visible",
            "fields": { "name": "TEXT", "text": "" }
        });
        assert_eq!(
            super::object_shadow_xml(empty.as_object().expect("对象")).expect("可改写"),
            "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"text\" id=\"s1\" \
             visible=\"visible\"><field name=\"TEXT\"/></shadow>"
        );
        let escaped = json!({
            "editable": true, "id": "s2", "type": "text", "visible": "visible",
            "fields": { "name": "TEXT", "text": "a<b & c>d\"e" }
        });
        assert_eq!(
            super::object_shadow_xml(escaped.as_object().expect("对象")).expect("可改写"),
            "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"text\" id=\"s2\" \
             visible=\"visible\"><field name=\"TEXT\">a&lt;b &amp; c&gt;d\"e</field></shadow>"
        );
        // 字符串形态的影子一个字节都不动(既有语料的字节基线靠这条)
        assert_eq!(
            normalize_object_shadows(&json!({ "blocks": { "a": {
            "type": "wait", "id": "a",
            "shadows": { "time": "<shadow type=\"math_number\" id=\"t\"/>", "DO": "" }
        } } })),
            None
        );
    }

    /// 真作品(第三十三轮那份被拒收的 `A28社区-开幕_174408420`):现在**能转换**了,
    /// 且不再有"暂不支持"的拒收。
    #[test]
    fn real_object_shadow_work_converts_when_sample_present() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("download/compile/A28社区-开幕_174408420.bcm4");
        if !path.exists() {
            super::super::missing_fixture(&format!("真作品样本 {}", path.display()));
            return;
        }
        let mut source: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读作品")).expect("JSON");
        let (product, report) = convert(&mut source, 1);
        assert!(
            report.blocks_converted > 1000,
            "真作品应搬走大量积木:{}",
            report.blocks_converted
        );
        // 对象影子一件不剩:产物里的 `shadows` 全是字符串
        let mut seen_xml = 0usize;
        let mut objects = 0usize;
        fn walk(node: &Value, seen_xml: &mut usize, objects: &mut usize) {
            match node {
                Value::Object(map) => {
                    if let Some(shadows) = map.get("shadows").and_then(Value::as_object) {
                        for value in shadows.values() {
                            match value {
                                Value::String(_) => *seen_xml += 1,
                                Value::Object(_) => *objects += 1,
                                _ => {}
                            }
                        }
                    }
                    map.values().for_each(|v| walk(v, seen_xml, objects));
                }
                Value::Array(items) => items.iter().for_each(|v| walk(v, seen_xml, objects)),
                _ => {}
            }
        }
        walk(&product, &mut seen_xml, &mut objects);
        assert!(seen_xml > 3000, "产物里应有大量影子 XML:{seen_xml}");
        assert_eq!(objects, 0, "产物里不得残留对象形态的影子");
    }
}
