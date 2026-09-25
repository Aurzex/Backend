//! 编辑器间互相转化:Kitten4 `.bcm4` ⇄ KittenN `.bcmkn`,以及 **NEMO → KittenN**(单向,
//! 方案见 `docs/rounds/27-nemo-to-kn-conversion-plan.md`)。
//!
//! 管线(正向与官方编辑器一致,见 `docs/rounds/20-kitten-kn-work-conversion-plan.md` §3.2/§3.3/§6.2;
//! 反向是本项目自建,见同一文档 §4):
//!
//! ```text
//! 源文件 JSON ──前端(pure 图↔树)──▶ BlockTree(中核)
//!                                    │
//!                          语义映射(mapping:改名/特例/降级)
//!                                    │
//! 目标文件 JSON ◀──后端(编码 + 工程化收尾)── BlockTree
//! ```
//!
//! | 方向 | 前端 | 语义 | 后端 | 装配 |
//! | --- | --- | --- | --- | --- |
//! | Kitten4 → KN | [`model::parse_block_data_json`] | [`mapping::translate_kitten_to_kn`] | [`model::split_procedures`]/[`model::rewrite_calls`]/[`model::tree_to_json`] | [`assembly::build_document`] |
//! | KN → Kitten4 | [`model::parse_kn_entity`]/[`model::parse_kn_procedures`] | [`mapping::translate_kn_to_kitten`] | [`model::unrewrite_calls`]/[`model::def_root_from_entry`]/[`model::build_block_data_json`] | [`build_kitten4_document`] |
//! | NEMO → KN | [`nemo::prepare_blocks_xml`](`nemo_xml` 解析 + 版本迁移 + 9 个前置改写) | [`nemo_mapping::translate_nemo_to_kn`](官方把映射折进解析,见该模块文档) | 不需要(程序集在解析器内就位) | [`nemo::convert_nemo_document`] |
//!
//! 三条路的**不变量**相同:产物是能过官方 `validateBcm` 的 `.bcmkn`;有损之处一律进
//! [`TranslateReport`]。NEMO 侧只有 `bcm_version` 这个额外输入(老作品要迁移,`docs/rounds/27` §9.3),
//! 走 [`TranslateOptions::source_version`]。
//!
//! 分层纪律:
//!
//! - `kitten` / `neko` 只做**编码转换**(邻接表 ↔ 树 ↔ 目标字段),不做语义决策;
//! - 一切"这个积木映射到谁""要不要降级"都问 [`mapping`];
//! - 所有有损之处进 [`TranslateReport`],不静默吞。
//!
//! 反向的**近似**(逐条都在报告里可见,详见 [`mapping`] 与 [`build_kitten4_document`] 的注释):
//!
//! - KN 原生、Kitten 侧无来源的类型(`KN_TYPES − LC 值域`,以及 `calculate` 这类正向会降级的块)
//!   保留原类型名 + `UnmappedBlock` 告警 —— 不丢弃积木,所以往返的类型多重集仍然守恒;
//! - 一个 KN 类型有多个 Kitten 原类型时(云列表/本地列表、`start_on_click`/`on_running_group_activated` 等)
//!   保留 KN 名 + `DroppedProperty` 告警;
//! - KN 的舞台坐标只有两种画布,反向按源 `stageSize` 原样给 Kitten4 的 `size`
//!   (`landscape = width > height` 因此保持一致),变量坐标按正向公式的逆换算;
//! - 程序集定义积木统一挂到第一个角色名下(KN 的 `proceduresDict` 没有实体归属信息);
//! - 角色 ↔ 造型关联、云变量与本地变量的区分在 KN 里已丢失,按 `currentStyleId` / 全部并入
//!   `variables` 还原并报告。

use crate::core::convert::shared::EditorType;
use std::collections::BTreeMap;

use std::fmt::Write as _;

pub(crate) mod assembly;
pub(crate) mod mapping;
pub(crate) mod model;
pub(crate) mod nemo;
pub(crate) mod nemo_mapping;
#[cfg(test)]
mod nemo_tests;
#[cfg(test)]
mod reverse_tests;
pub(crate) mod tables_gen;

/// 目标编辑器
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetEditor {
    /// KittenN(编辑器类型 `NEKO`,文件 `.bcmkn`)
    KittenN,
    /// Kitten4(编辑器类型 `KITTEN4`,文件 `.bcm4`)
    Kitten4,
}

impl TargetEditor {
    /// 该目标对应的编辑器类型 —— **域内唯一的 `TargetEditor → EditorType` 转换点**,
    /// 消除散落的 `match`(见 `docs/rounds/31` §3 A4)
    pub(crate) fn as_editor(self) -> EditorType {
        match self {
            TargetEditor::KittenN => EditorType::Neko,
            TargetEditor::Kitten4 => EditorType::Kitten4,
        }
    }

    /// 产物文件名中的目标标识
    pub(crate) fn file_slug(self) -> &'static str {
        match self {
            TargetEditor::KittenN => "kn",
            TargetEditor::Kitten4 => "kitten4",
        }
    }

    /// 产物扩展名(不含点)
    pub(crate) fn file_extension(self) -> &'static str {
        match self {
            TargetEditor::KittenN => "bcmkn",
            TargetEditor::Kitten4 => "bcm4",
        }
    }
}

/// 舞台朝向(KN 只有两种画布尺寸;Kitten 侧要挑一个)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageOrientation {
    /// 按源作品的画布长宽比自动选
    Auto,
    /// 562×900
    Portrait,
    /// 900×562
    Landscape,
}

/// 转化选项(构建器风格,与 `DecompileOptions` 一致)
#[derive(Debug, Clone)]
pub struct TranslateOptions {
    output_dir: Option<std::path::PathBuf>,
    upload: bool,
    strict: bool,
    keep_source: bool,
    stage: StageOrientation,
    deterministic_ids: bool,
    batch_concurrency: usize,
    entity_concurrency: usize,
    /// 源文档的 `bcm_version`(只 NEMO 方向用:版本迁移 `< 0.9.4` QC / `< 0.15.0` YC)
    source_version: Option<String>,
}

impl TranslateOptions {
    pub fn new() -> Self {
        TranslateOptions {
            output_dir: None,
            upload: false,
            strict: false,
            keep_source: true,
            stage: StageOrientation::Auto,
            deterministic_ids: false,
            batch_concurrency: 1,
            entity_concurrency: 1,
            source_version: None,
        }
    }

    /// 输出目录;`None` 时用 `PathConfig` 的编译目录
    pub fn output_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.output_dir = Some(dir.into());
        self
    }

    /// 转换后上传并新建作品(默认关;开=替用户发布,需明确授权)
    pub fn upload(mut self, on: bool) -> Self {
        self.upload = on;
        self
    }

    /// 遇到不可逆损失直接失败(默认关:降级 + 报告)
    pub fn strict(mut self, on: bool) -> Self {
        self.strict = on;
        self
    }

    /// 正向时保留源作品文件引用(对齐官方 `source` 字段行为,默认开)
    pub fn keep_source(mut self, on: bool) -> Self {
        self.keep_source = on;
        self
    }

    /// 目标画布朝向
    pub fn stage(mut self, stage: StageOrientation) -> Self {
        self.stage = stage;
        self
    }

    /// 确定性 id(测试/基准对齐用;默认关)
    pub fn deterministic_ids(mut self, on: bool) -> Self {
        self.deterministic_ids = on;
        self
    }

    /// 源作品文件的 `bcm_version`(NEMO 版本迁移用;`None` = 不迁移,与官方"没传版本"一致)
    ///
    /// NEMO 的编辑版文档里**没有** `bcm_version`(只有 `app_version`),版本来自作品元信息
    /// (`source_info.bcm_version`);域门面会把反编译产物里的版本自动带上。
    pub fn source_version(mut self, version: impl Into<String>) -> Self {
        let version = version.into();
        self.source_version = Some(version);
        self
    }

    pub(crate) fn source_version_ref(&self) -> Option<&str> {
        self.source_version.as_deref()
    }

    pub(crate) fn output_dir_ref(&self) -> Option<&std::path::Path> {
        self.output_dir.as_deref()
    }

    pub(crate) fn upload_enabled(&self) -> bool {
        self.upload
    }

    pub(crate) fn is_strict(&self) -> bool {
        self.strict
    }

    pub(crate) fn keeps_source(&self) -> bool {
        self.keep_source
    }

    pub(crate) fn orientation(&self) -> StageOrientation {
        self.stage
    }

    pub(crate) fn ids_deterministic(&self) -> bool {
        self.deterministic_ids
    }

    /// 批量转化的并发数(≥1,默认 1;与 `DecompileOptions::batch_concurrency` 同义)
    pub fn batch_concurrency(mut self, n: usize) -> Self {
        self.batch_concurrency = n.max(1);
        self
    }

    /// 实体级并发(正向;≥1,默认 1):单个作品文档内按实体/程序集并行(方案 25 S3a)。
    ///
    /// 实现见 [`convert_kitten4_document`]:每个实体用自己的临时 id,串行阶段按
    /// 「阶段 1 全项 → 阶段 2 全项」的账本兑现最终 id,所以
    ///
    /// - `deterministic_ids(true)`(基准/回归测试口径)下,产物与并发 1 **逐字节相同**
    ///   (也有 `tests/convert_bench.rs` 的 1 vs N 同 SHA256 门);
    /// - 非确定性模式下 id 本来就是随机的,承诺降为"同样**合法且唯一**",值不再保证等于并发 1
    ///   (与串行实现本身也不可复现同理);
    /// - 反向(KN → Kitten4)暂不支持本选项,取值被忽略。
    ///
    /// 批量入口 [`translate_works`](crate::core::convert::translate_works) 会把它按
    /// 作品级并发与可用核数**折算**(见 [`TranslateOptions::fold_entity_concurrency`]),
    /// 避免"作品级 × 实体级"两级超订。
    pub fn entity_concurrency(mut self, n: usize) -> Self {
        self.entity_concurrency = n.max(1);
        self
    }

    pub(crate) fn concurrency(&self) -> usize {
        self.batch_concurrency.max(1)
    }

    /// 本次文档转换用几个实体级工作线程(≥1)
    pub(crate) fn entity_workers(&self) -> usize {
        self.entity_concurrency.max(1)
    }

    /// 按**作品级并发**与**可用核数**折算实体级并发(方案 25 §7 阻塞 #6;批量入口调用)
    ///
    /// 两级并发相乘会超订(作品级 `b` × 实体级 `e` 个翻译线程),所以批量入口把每作品
    /// 分到的核数 `可用核数 / 有效作品并发`(向下取整、至少 1)作为实体级并发上限:
    /// `e' = clamp(min(e, 可用核数 / b), 1, ∞)`。
    ///
    /// 折算只改并行度,不碰产物。直接调用 [`translate_value`] / [`translate_file`] 的
    /// 单文档入口不做折算(调用方自己要的并发,由 [`assembly::workers`] 兜住"不超核数")。
    pub(crate) fn fold_entity_concurrency(mut self, works: usize, available: usize) -> Self {
        let batch = self.concurrency().min(works.max(1));
        let share = (available.max(1) / batch).max(1);
        self.entity_concurrency = self.entity_concurrency.min(share).max(1);
        self
    }
}

impl Default for TranslateOptions {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 管线:文件 → 文档 → 文件
// ---------------------------------------------------------------------------

/// 转化结果
#[derive(Debug)]
pub struct TranslateOutcome {
    /// 产物路径
    pub output: std::path::PathBuf,
    /// 上传建作品后返回的新作品 id(`upload` 关闭时为 `None`)
    pub work_id: Option<i64>,
    /// 目标编辑器
    pub target: TargetEditor,
    /// 报告(覆盖率/降级/丢弃)
    pub report: TranslateReport,
}

/// 转化错误
#[derive(Debug, thiserror::Error)]
pub enum TranslateError {
    #[error("传输/通用错误: {0}")]
    Mew(#[from] crate::utils::requests::MewError),
    #[error("作品文件解析失败: {0}")]
    Decompiler(#[from] crate::core::convert::DecompilerError),
    #[error("不支持的方向:{from:?} → {to:?}(本库当前做 Kitten4 ⇄ KittenN 与 NEMO → KittenN)")]
    Unsupported {
        from: crate::core::convert::EditorType,
        to: TargetEditor,
    },
    #[error("有损转化被 strict 挡下:失败项见报告")]
    Lossy { report: Box<TranslateReport> },
    #[error("调用方参数非法: {0}")]
    InvalidArgument(String),
}

// 与仓库错误分层一致:传输层错误压进 `Mew`,不新增 Io/Json 变体
impl From<std::io::Error> for TranslateError {
    fn from(error: std::io::Error) -> Self {
        TranslateError::Mew(crate::utils::requests::MewError::from(error))
    }
}

impl From<serde_json::Error> for TranslateError {
    fn from(error: serde_json::Error) -> Self {
        TranslateError::Mew(crate::utils::requests::MewError::from(error))
    }
}

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
    /// 源积木树(`block_data_json`),转换结束后原样放回源文档
    block_data_json: Option<serde_json::Value>,
    /// 装箱权重(源 `blocks` 条数):只影响并行均衡,不进产物
    weight: usize,
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
            let block_data_json = entity.remove("block_data_json");
            let weight = block_data_json
                .as_ref()
                .and_then(|bdj| bdj.get("blocks"))
                .and_then(Value::as_object)
                .map_or(0, serde_json::Map::len);
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
        let Some(block_data_json) = item.block_data_json.take() else {
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

/// 阶段 1(单个工作项,项内自足):解析 → 语义映射 → 抽程序集
///
/// 铸造走**临时 id**(`IdSource::recording`),账本交给串行阶段兑现最终 id。
fn parse_forward_item(
    index: usize,
    block_data_json: Option<&serde_json::Value>,
    landscape: bool,
) -> std::result::Result<ForwardParsed, TranslateError> {
    let mut ids = model::IdSource::recording(index);
    let mut local = TranslateReport::new(
        crate::core::convert::EditorType::Kitten4,
        TargetEditor::KittenN,
    );
    let mut tree = match block_data_json {
        Some(block_data_json) => model::parse_block_data_json(block_data_json)?.tree,
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
/// 不能让每个线程各自铸 id,而是分段(实现见 [`remint`]):
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
pub(crate) fn convert_kitten4_document(
    // `&mut`:装配阶段用不到源实体的 `block_data_json`,而它是整棵积木树(源文档里
    // 最大的字段)。取走它再克隆实体对象,省掉"每个实体复制一份完整积木树"的
    // 文档级白拷贝(方案 23 P0-3)。
    source: &mut serde_json::Value,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    use serde_json::Value;
    let started = std::time::Instant::now();

    // 官方 GN 第一行就读 `size`,Kitten2/3(`.bcm` + blocksXML)没有它 —— 直接给明确错误,
    // 而不是像官方那样抛 TypeError(见 docs/rounds/20 §1/§11.1)。
    let size = source.get("size").ok_or_else(|| {
        TranslateError::InvalidArgument(
            "源作品没有 size 字段:这看起来是 Kitten2/3(.bcm/blocksXML)作品,本库暂不支持该方向"
                .into(),
        )
    })?;
    let landscape = match (
        size.get("width").and_then(Value::as_f64),
        size.get("height").and_then(Value::as_f64),
    ) {
        (Some(w), Some(h)) => w > h,
        _ => false,
    };

    let mut items = collect_forward_items(source)?;
    let weights: Vec<usize> = items.iter().map(|item| item.weight).collect();
    let workers = assembly::workers(options.entity_workers(), items.len());
    let deterministic = options.ids_deterministic();
    // 可观测事实:本次转换真的开了几个实体级线程(供基准/单测挡空门,见 `TranslateReport`)
    report.entity_workers = workers;

    let outcome = (|| -> std::result::Result<Value, TranslateError> {
        // ── 阶段 1(并行):解析 + 语义映射 + 抽程序集(官方:scenes.forEach → actors.forEach → zC)
        //
        // 只**借用**源积木树:`block_data_json` 之后要放回源文档。`.collect::<Result<…>>()`
        // 让"首个错误按项序冒泡"与串行一致。
        let block_data: Vec<Option<&Value>> = items
            .iter()
            .map(|item| item.block_data_json.as_ref())
            .collect();
        let parsed =
            assembly::run_items(block_data, &weights, workers, |index, block_data_json| {
                parse_forward_item(index, block_data_json, landscape)
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
        let rewritten = assembly::run_items(trees, &weights, workers, |index, mut tree| {
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
        let mut mints = assembly::IdRemap::new();
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
            let (nodes, missed) = assembly::remap_entry(&mints, entry);
            procedures_nodes += nodes;
            unmatched += missed;
        }

        // ── 阶段 4(并行):改写实体树 + 编码(编码入口与旧实现同一个)
        let encoded = assembly::run_items(trees, &weights, workers, |_, mut tree| {
            let (nodes, missed) = assembly::remap_tree(&mints, &mut tree);
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
            unmatched += assembly::merge_report(report, local, &mints);
        }
        for local in reports_second {
            unmatched += assembly::merge_report(report, local, &mints);
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

/// 文档级转化结果([`translate_value`] 的产物:文档留在内存,不落盘)
#[derive(Debug)]
pub struct TranslateDocument {
    /// 目标编辑器的编辑版文档
    pub document: serde_json::Value,
    /// 目标编辑器
    pub target: TargetEditor,
    /// 报告(覆盖率/降级/丢弃)
    pub report: TranslateReport,
}

/// 把**内存里的**编辑版文档转化成另一种编辑器(不碰文件系统)
///
/// 与 [`translate_file`] 共用同一条管线,区别只在 IO:`translate_file`
/// = 读盘 → 本函数 → 流式写盘。`translate_work`(域门面)用本入口把
/// 「反编译落盘 → translate 读回」这一段整个省掉(方案 23 P0-2):
/// 10 MB 级作品实测省一次 serialize + 一次 parse(墙钟 10–20%)。
pub fn translate_value(
    mut source: serde_json::Value,
    target: TargetEditor,
    options: &TranslateOptions,
) -> std::result::Result<TranslateDocument, TranslateError> {
    let from = detect_editor(&source);

    let (document, report) = match (from, target) {
        (Some(crate::core::convert::EditorType::Kitten4), TargetEditor::KittenN) => {
            let mut report = TranslateReport::new(
                crate::core::convert::EditorType::Kitten4,
                TargetEditor::KittenN,
            );
            let document = convert_kitten4_document(&mut source, options, &mut report)?;
            (document, report)
        }
        (Some(crate::core::convert::EditorType::Nemo), TargetEditor::KittenN) => {
            let mut report = TranslateReport::new(
                crate::core::convert::EditorType::Nemo,
                TargetEditor::KittenN,
            );
            let document = nemo::convert_nemo_document(&source, options, &mut report)?;
            (document, report)
        }
        (Some(crate::core::convert::EditorType::Neko), TargetEditor::Kitten4) => {
            let mut report = TranslateReport::new(
                crate::core::convert::EditorType::Neko,
                TargetEditor::Kitten4,
            );
            let document = assembly::convert_kn_document(&source, options, &mut report)?;
            (document, report)
        }
        (Some(from), to) => return Err(TranslateError::Unsupported { from, to }),
        (None, to) => {
            return Err(TranslateError::InvalidArgument(format!(
                "无法识别源作品格式(目标 {to:?});本库当前支持 Kitten4(.bcm4 编辑版)、KittenN(.bcmkn)与 NEMO(.bcm 编辑版)"
            )));
        }
    };

    if options.is_strict() && report.is_lossy() {
        return Err(TranslateError::Lossy {
            report: Box::new(report),
        });
    }

    Ok(TranslateDocument {
        document,
        target,
        report,
    })
}

/// 把一个作品文件转化成另一种编辑器的作品文件
pub fn translate_file(
    input: &std::path::Path,
    target: TargetEditor,
    options: TranslateOptions,
) -> std::result::Result<TranslateOutcome, TranslateError> {
    use crate::core::convert::shared::FileService;
    use crate::utils::filedata::PathConfig;

    let text = std::fs::read_to_string(input)?;
    let source: serde_json::Value = serde_json::from_str(&text)?;
    let converted = translate_value(source, target, &options)?;

    let output = product_path(input, target, &options)?;

    // 流式写盘(方案 23 P0-1):与 `to_string` 逐字节相同,省掉整份中间串与一次整块拷贝
    FileService::write_json(&output, &converted.document)?;

    Ok(TranslateOutcome {
        output,
        work_id: None,
        target: converted.target,
        report: converted.report,
    })
}

/// 产物路径口径:`<源文件名主干>.<slug>.<bcmkn|bcm4>`
///
/// `translate_file` 与域门面(内存直通路径)共用同一口径,避免两条入口命名分叉。
pub(crate) fn product_path(
    source_file: &std::path::Path,
    target: TargetEditor,
    options: &TranslateOptions,
) -> std::result::Result<std::path::PathBuf, TranslateError> {
    use crate::utils::filedata::PathConfig;

    let slug = target.file_slug();
    let ext = target.file_extension();
    let stem = source_file
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            TranslateError::InvalidArgument(format!("源文件名没有主干:{source_file:?}"))
        })?;
    let dir = options
        .output_dir_ref()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathConfig::global().convert_file_path());
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{stem}.{slug}.{ext}")))
}

/// 把源作品文件引用写进**内存里**的 KN 文档顶层 `source` 字段(纯函数)
pub fn set_source_reference_in(
    document: &mut serde_json::Value,
    url: &str,
) -> std::result::Result<(), TranslateError> {
    let Some(object) = document.as_object_mut() else {
        return Err(TranslateError::InvalidArgument("产物不是 JSON 对象".into()));
    };
    object.insert(
        "source".to_string(),
        serde_json::Value::String(url.to_string()),
    );
    Ok(())
}

/// 把源作品文件引用写进**已产出**的 KN 文档顶层 `source` 字段。
///
/// 官方做法:KN 编辑器导入 Kitten 作品时,把原始 Kitten 文件字节重新上传,
/// 并把 URL 写到 KN 作品的 `source`(即"保留原件",见 `docs/rounds/20` §3.1 的 `w.source = T`)。
/// 这里只做**纯文件改写**,上传由 `convert` 门面编排(保持本子域不碰网络)。
///
/// 已知偏差:我们手里只有反编译重建的编辑版,官方上传的是原始文件字节
/// (见 `docs/rounds/21` §4-10)。
///
/// 域内编排([`crate::core::convert::translate_work`])走 [`set_source_reference_in`]
/// 直接改内存文档,不读回-写回产物文件(方案 23 P0-2)。
pub fn set_source_reference(
    output: &std::path::Path,
    url: &str,
) -> std::result::Result<(), TranslateError> {
    use crate::core::convert::shared::FileService;

    let text = std::fs::read_to_string(output)?;
    let mut document: serde_json::Value = serde_json::from_str(&text)?;
    set_source_reference_in(&mut document, url).map_err(|error| match error {
        TranslateError::InvalidArgument(_) => {
            TranslateError::InvalidArgument(format!("产物不是 JSON 对象:{output:?}"))
        }
        other => other,
    })?;
    // 与 `translate_file` 同一序列化口径(serde_json 默认按键排序,round-trip 稳定)
    FileService::write_json(output, &document)?;
    Ok(())
}

/// 识别源作品属于哪个编辑器(按顶层结构判定,不用扩展名)。
///
/// Kitten2 与 Kitten3 的编辑版都是 `blocksXML`,本地样本与 `docs/rounds/20` 都没有可靠的
/// 区分标记,而**两者都不支持转化**(编辑器自己会拒绝,见 `docs/rounds/20` §3.1),
/// 故统一按 Kitten3 报;不编造 `size` 之类的判据。
pub(crate) fn detect_editor(
    source: &serde_json::Value,
) -> Option<crate::core::convert::EditorType> {
    use serde_json::Value;
    if source.get("theatre").is_some() {
        let has_xml = source
            .pointer("/theatre/actors")
            .and_then(Value::as_object)
            .map(|m| m.values().any(|a| a.get("blocksXML").is_some()))
            .unwrap_or(false);
        let has_bdj = source
            .pointer("/theatre/actors")
            .and_then(Value::as_object)
            .map(|m| m.values().any(|a| a.get("block_data_json").is_some()))
            .unwrap_or(false);
        if has_xml && !has_bdj {
            return Some(crate::core::convert::EditorType::Kitten3);
        }
        return Some(crate::core::convert::EditorType::Kitten4);
    }
    if source
        .get("actors")
        .and_then(|a| a.get("actors_dict"))
        .is_some()
    {
        return Some(crate::core::convert::EditorType::Nemo);
    }
    if source
        .get("actors")
        .and_then(|a| a.get("actorsDict"))
        .is_some()
    {
        return Some(crate::core::convert::EditorType::Neko);
    }
    None
}

// ===========================================================================
// 转化报告(report):把"哪些积木没映射上、降级了、字段丢了"逐类计数,不静默吞掉
// ===========================================================================

/// 一类告警
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TranslateWarning {
    /// 目标编辑器没有对应积木:**保留 KN 原类型名 + 告警**(不丢弃积木 ⇒ 往返的类型多重集仍守恒,
    /// 但 Kitten4 侧可能不认这个类型名)。判定"哪些类型属此类"见 `mapping.rs` 顶部注释。
    UnmappedBlock { kind: String },
    /// 映射到文本占位积木(`bcm_translator_text_*`),原文进 mutation
    DegradedToText { kind: String },
    /// 结构上无法表达的字段被丢弃(`path` 形如 `actors.<id>.rotation_type`)
    DroppedField { path: String },
    /// 目标格式带不走的实体级属性(如 KN 的变量样式图标)
    DroppedProperty { path: String },
    /// 新铸了 id(影子/参数),便于对齐时忽略
    RemintedId { from: String },
    /// 官方导入时会重新上传资源、产物里的 url 由平台重写(我们保留源 url)——**不是损失**
    ReuploadedOnImport { path: String },
}

impl TranslateWarning {
    /// 告警类别标签(日志与集成测试按类别统计用)
    pub fn category(&self) -> &'static str {
        match self {
            TranslateWarning::UnmappedBlock { .. } => "未映射积木(保留原类型名)",
            TranslateWarning::DegradedToText { .. } => "降级为文本占位积木",
            TranslateWarning::DroppedField { .. } => "丢弃字段",
            TranslateWarning::DroppedProperty { .. } => "丢弃实体属性",
            TranslateWarning::RemintedId { .. } => "重新生成 id",
            TranslateWarning::ReuploadedOnImport { .. } => "官方重传资源(非损失)",
        }
    }

    fn subject(&self) -> &str {
        match self {
            TranslateWarning::UnmappedBlock { kind }
            | TranslateWarning::DegradedToText { kind } => kind,
            TranslateWarning::DroppedField { path }
            | TranslateWarning::DroppedProperty { path } => path,
            TranslateWarning::RemintedId { from } => from,
            TranslateWarning::ReuploadedOnImport { path } => path,
        }
    }
}

/// 转换结果报告
#[derive(Debug, Clone)]
pub struct TranslateReport {
    pub from: EditorType,
    pub to: TargetEditor,
    pub blocks_total: usize,
    pub blocks_converted: usize,
    pub elapsed_ms: u128,
    /// 本次转换**实际**使用的实体级工作线程数(≥1)。
    ///
    /// 正向 = `min(entity_concurrency, 工作项数, 可用核数)`(见
    /// [`TranslateOptions::entity_concurrency`]);反向暂不支持实体级并行,恒为 1。
    /// 这是"并行真的发生了"的可观测证据:基准/单测用它挡住"并发对照在单核上退化成串行"
    /// 这种空门(方案 25 §9)。
    pub entity_workers: usize,
    warnings: Vec<TranslateWarning>,
}

impl TranslateReport {
    pub(crate) fn new(from: EditorType, to: TargetEditor) -> Self {
        TranslateReport {
            from,
            to,
            blocks_total: 0,
            blocks_converted: 0,
            elapsed_ms: 0,
            entity_workers: 1,
            warnings: Vec::new(),
        }
    }

    pub(crate) fn warn(&mut self, w: TranslateWarning) {
        self.warnings.push(w);
    }

    /// 取出全部告警(实体级并行按项收集局部报告后,再按项序并入全局报告;见 [`assembly::merge_report`])
    pub(crate) fn take_warnings(&mut self) -> Vec<TranslateWarning> {
        std::mem::take(&mut self.warnings)
    }

    pub fn warnings(&self) -> &[TranslateWarning] {
        &self.warnings
    }

    /// 是否有"不可逆"损失(未映射/降级/丢弃)。
    ///
    /// 不算损失的:新铸 id(内容都在,只换了 id)、官方导入时重传资源
    /// (产物 url 由平台重写,我们保留源 url)。
    pub fn is_lossy(&self) -> bool {
        self.warnings.iter().any(|w| {
            !matches!(
                w,
                TranslateWarning::RemintedId { .. } | TranslateWarning::ReuploadedOnImport { .. }
            )
        })
    }

    /// 计数(类别 → 主体 → 次数)
    ///
    /// 先按**借用键**聚合,最后只对去重后的类别/主体做一次字符串化:
    /// 告警可达上万条(反向 10 MB 作品 1 817 条),旧实现每条要 3 次 `String`
    /// 分配(`key()` 克隆 + 类别 + 主体),而 `key()` 的结果其实被丢弃
    /// (方案 23 P0-4)。
    pub fn counts(&self) -> BTreeMap<String, BTreeMap<String, usize>> {
        let mut agg: BTreeMap<(&'static str, &str), usize> = BTreeMap::new();
        for w in &self.warnings {
            *agg.entry((w.category(), w.subject())).or_default() += 1;
        }
        let mut out: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for ((category, subject), n) in agg {
            out.entry(category.to_string())
                .or_default()
                .insert(subject.to_string(), n);
        }
        out
    }

    /// 人类可读摘要(markdown)
    pub fn to_markdown(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "## 转换报告");
        let _ = writeln!(s);
        let _ = writeln!(
            s,
            "- 源编辑器:`{:?}` → 目标编辑器:`{:?}`",
            self.from, self.to
        );
        let _ = writeln!(
            s,
            "- 积木:{}(成功转换 {})",
            self.blocks_total, self.blocks_converted
        );
        let _ = writeln!(s, "- 耗时:{} ms", self.elapsed_ms);
        let _ = writeln!(s, "- 有损:{}", if self.is_lossy() { "是" } else { "否" });
        let counts = self.counts();
        if counts.is_empty() {
            let _ = writeln!(s, "\n无告警。");
            return s;
        }
        let _ = writeln!(s, "\n| 类别 | 主体 | 次数 |");
        let _ = writeln!(s, "| --- | --- | --- |");
        for (cat, items) in counts {
            for (subject, n) in items {
                let _ = writeln!(s, "| {cat} | `{subject}` | {n} |");
            }
        }
        s
    }
}

/// 测试用临时目录:**每次调用唯一**(进程 + 随机后缀),避免并行测试/跨运行互相覆盖
#[cfg(test)]
pub(crate) fn unique_test_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "backend-convert-{tag}-{}-{:08x}",
        std::process::id(),
        fastrand::u32(..)
    ))
}

#[cfg(test)]
mod diff_tests {
    //! 与官方产物对齐的差分门(docs/rounds/20 §7 Phase 2 验收)。
    //!
    //! 夹具 `tests/fixtures/translate/geoduel_scene_actor.json` 里是**真实作品**的
    //! 「Kitten4 输入 block_data_json」与「官方 `kittenBcmToNekoBcmUtils` 输出」成对切片
    //! (来自 `temp/baseline/out-kitten4.json`,基线 bundle sha256 见 docs/rounds/20 附录 C)。
    //!
    //! 比较口径(为什么不做整段 JSON diff):
    //! - 官方新建节点(shadow 实体化、`pure_list_get`、`math_arithmetic` 包裹)拿不到稳定 id → 按类型计数比较;
    //! - 官方会丢掉 `is_output`/`field_constraints`/`extra` 键、且 `next` 子节点不带 `parent_id`,
    //!   我们刻意保留(§6.2 逃生舱)→ 只比 `type`/`fields`/槽名集合;
    //! - 官方 UUID 每次运行都变 → 涉及 id 的等价性只在「输入里已存在的 id」上比较。

    use super::*;
    use crate::core::convert::translate::model::BlockJson;
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};

    /// `set_source_reference`:把原件 URL 写进产物顶层,其余键原样保留;非对象产物显式报错
    #[test]
    fn set_source_reference_writes_top_level_source() {
        let dir = std::env::temp_dir().join(format!(
            "backend-convert-source-{}-{}",
            std::process::id(),
            fastrand::u32(..)
        ));
        std::fs::create_dir_all(&dir).expect("建目录");
        let path = dir.join("work.kn.bcmkn");
        std::fs::write(
            &path,
            r#"{"stageSize":{"width":900,"height":562},"projectName":"样例"}"#,
        )
        .expect("写文件");

        set_source_reference(&path, "https://creation.codemao.cn/src.bcm4").expect("写引用");
        let doc: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读文件")).expect("JSON");
        assert_eq!(
            doc["source"],
            serde_json::json!("https://creation.codemao.cn/src.bcm4")
        );
        assert_eq!(
            doc["projectName"],
            serde_json::json!("样例"),
            "其余键原样保留"
        );

        // 非对象产物:显式报错,不静默写出半成品
        let bad = dir.join("bad.json");
        std::fs::write(&bad, "[1,2]").expect("写文件");
        assert!(set_source_reference(&bad, "x").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fixture() -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/translate/geoduel_scene_actor.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读不到夹具 {}: {e}", path.display()));
        serde_json::from_str(&text).expect("夹具 JSON")
    }

    /// 按官方语义跑一遍「单个实体」的管线,返回 (编码后的根数组, 我们抽出的程序集)
    fn run_entity(
        bdj: &Value,
        landscape: bool,
    ) -> (Vec<Value>, Vec<model::ProcedureEntry>, TranslateReport) {
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let mut ids = model::IdSource::new(true); // 确定性 id:对齐测试必需
        let mut tree = model::parse_block_data_json(bdj).expect("解析实体").tree;
        mapping::translate_kitten_to_kn(&mut tree, landscape, &mut ids, &mut report);
        let (kept, procs) = model::split_procedures(tree, &mut ids, &mut report);
        let mut kept = kept;
        model::rewrite_calls(&mut kept, &procs, &mut ids, &mut report);
        let json = model::tree_to_json(&kept).expect("编码");
        (json, procs, report)
    }

    /// 官方 `UC = size.width > size.height`(横屏);夹具顶部带了真实作品的 size
    fn landscape(f: &Value) -> bool {
        let w = f["size"]["width"].as_f64().unwrap_or(0.0);
        let h = f["size"]["height"].as_f64().unwrap_or(0.0);
        w > h
    }

    fn type_census(values: &[Value]) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        fn walk(node: &Value, out: &mut BTreeMap<String, usize>) {
            if let Some(t) = node.get("type").and_then(Value::as_str) {
                *out.entry(t.to_string()).or_default() += 1;
            }
            for key in ["inputs", "statements"] {
                if let Some(map) = node.get(key).and_then(Value::as_object) {
                    for child in map.values() {
                        walk(child, out);
                    }
                }
            }
            if let Some(next) = node.get("next") {
                walk(next, out);
            }
        }
        for v in values {
            walk(v, &mut out);
        }
        out
    }

    /// 收集 输入 block_data_json 里出现过的积木 id
    fn input_ids(bdj: &Value) -> BTreeSet<String> {
        bdj.get("blocks")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// 把树按 id 展平成 map(只保留有 id 的节点)
    fn by_id(values: &[Value]) -> BTreeMap<String, Value> {
        let mut out = BTreeMap::new();
        fn walk(node: &Value, out: &mut BTreeMap<String, Value>) {
            if let Some(id) = node.get("id").and_then(Value::as_str) {
                out.insert(id.to_string(), node.clone());
            }
            for key in ["inputs", "statements"] {
                if let Some(map) = node.get(key).and_then(Value::as_object) {
                    for child in map.values() {
                        walk(child, out);
                    }
                }
            }
            if let Some(next) = node.get("next") {
                walk(next, out);
            }
        }
        for v in values {
            walk(v, &mut out);
        }
        out
    }

    fn assert_type_census_matches(label: &str, ours: &[Value], theirs: &[Value]) {
        let a = type_census(ours);
        let b = type_census(theirs);
        let only_ours: Vec<_> = a
            .iter()
            .filter(|(k, v)| b.get(*k) != Some(*v))
            .map(|(k, v)| format!("{k}: 我们 {v} vs 官方 {:?}", b.get(k)))
            .collect();
        let only_theirs: Vec<_> = b
            .iter()
            .filter(|(k, v)| a.get(*k) != Some(*v))
            .map(|(k, v)| format!("{k}: 官方 {v} vs 我们 {:?}", a.get(k)))
            .collect();
        assert!(
            only_ours.is_empty() && only_theirs.is_empty(),
            "[{label}] 类型计数不一致\n我们多/不等: {only_ours:#?}\n官方多/不等: {only_theirs:#?}"
        );
    }

    #[test]
    fn actor_matches_official_type_census() {
        let f = fixture();
        let actor = &f["actor"];
        let bdj = &actor["block_data_json"];
        let official = actor["official_nekoBlockJsonList"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let (ours, _procs, report) = run_entity(bdj, landscape(&f));
        assert_type_census_matches("actor", &ours, &official);
        // 官方在该角色上没有降级(Phase-0 交叉校验:整个作品 0 个占位积木)
        assert!(
            report
                .warnings()
                .iter()
                .all(|w| !matches!(w, TranslateWarning::DegradedToText { .. })),
            "该角色不应出现降级积木:{:#?}",
            report.warnings()
        );
    }

    #[test]
    fn scene_matches_official_type_census() {
        let f = fixture();
        let scene = &f["scene"];
        let official = scene["official_nekoBlockJsonList"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let (ours, _procs, _report) = run_entity(&scene["block_data_json"], landscape(&f));
        assert_type_census_matches("scene", &ours, &official);
    }

    #[test]
    fn per_id_types_and_fields_match_official() {
        let f = fixture();
        for label in ["scene", "actor"] {
            let entity = &f[label];
            let bdj = &entity["block_data_json"];
            let official = entity["official_nekoBlockJsonList"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let (ours_json, _procs, _report) = run_entity(bdj, landscape(&f));
            let ours_by_id = by_id(&ours_json);
            let theirs_by_id = by_id(&official);
            let known = input_ids(bdj);

            let mut diffs = Vec::new();
            for (id, node) in &theirs_by_id {
                if !known.contains(id) {
                    continue; // 官方新建节点(shadow/包裹块),id 不稳定,交给类型计数
                }
                let Some(mine) = ours_by_id.get(id) else {
                    diffs.push(format!("[{label}] 我们缺节点 {id}({:?})", node["type"]));
                    continue;
                };
                if mine["type"] != node["type"] {
                    diffs.push(format!(
                        "[{label}] {id} 类型不同:我们 {:?} vs 官方 {:?}",
                        mine["type"], node["type"]
                    ));
                    continue;
                }
                let mine_fields = mine.get("fields").cloned().unwrap_or(Value::Null);
                let their_fields = node.get("fields").cloned().unwrap_or(Value::Null);
                if mine_fields != their_fields {
                    diffs.push(format!(
                        "[{label}] {id}({:?}) 字段不同:我们 {mine_fields} vs 官方 {their_fields}",
                        node["type"]
                    ));
                }
                for slot in ["inputs", "statements"] {
                    let a: BTreeSet<_> = mine
                        .get(slot)
                        .and_then(Value::as_object)
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default();
                    let b: BTreeSet<_> = node
                        .get(slot)
                        .and_then(Value::as_object)
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default();
                    // 官方在 h 型(返值/返布尔占位)上会丢连接,我们保留 —— 记为已知差异
                    if a != b && !a.is_superset(&b) {
                        diffs.push(format!(
                            "[{label}] {id} {slot} 槽不同:我们 {a:?} vs 官方 {b:?}"
                        ));
                    }
                }
            }
            assert!(
                diffs.is_empty(),
                "[{label}] 与官方逐块差异 {} 处:\n{}",
                diffs.len(),
                diffs.join("\n")
            );
        }
    }

    #[test]
    fn procedures_match_official_dict() {
        let f = fixture();
        // 官方 stage-1 把所有 procedures_2_defnoreturn 抽进全局 proceduresDict;
        // 我们的夹具里带了完整字典,按 id 对齐(官方 id 在构建程序集时是现铸的 uuid?否:
        // NORMAL 条目沿用定义块 id,ROUND 条目新铸 —— 只比 NORMAL)
        let official = f["official_proceduresDict"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        let mut ours_entries = Vec::new();
        for label in ["scene", "actor"] {
            let (_json, procs, _r) = run_entity(&f[label]["block_data_json"], landscape(&f));
            ours_entries.extend(procs);
        }
        assert!(
            !ours_entries.is_empty(),
            "夹具应至少含一个程序集定义(几何对战:4 个)"
        );
        let mut matched = 0;
        for entry in &ours_entries {
            let Some(theirs) = official.get(&entry.id) else {
                continue;
            };
            matched += 1;
            assert_eq!(theirs["name"], entry.name, "程序集 {} 名字不一致", entry.id);
            assert_eq!(theirs["type"], entry.kind, "程序集 {} 类型不一致", entry.id);
            let their_params = theirs["params"].as_array().cloned().unwrap_or_default();
            assert_eq!(
                their_params.len(),
                entry.params.len(),
                "程序集 {} 形参个数不一致",
                entry.id
            );
            for (mine, their) in entry.params.iter().zip(their_params.iter()) {
                assert_eq!(their["type"], mine.kind, "形参类型不一致");
                assert_eq!(their["name"], mine.name, "形参名不一致");
                if mine.kind == "Label" {
                    // 首个 Label 形参是官方现铸的(BC() → 随机 uuid),两边 id 本来就不等
                    assert_eq!(
                        their["id"].as_str().map(str::len),
                        Some(36),
                        "Label 形参 id 应是 uuid"
                    );
                } else {
                    assert_eq!(their["id"], mine.id, "String 形参应沿用源形参积木 id");
                }
            }
        }
        assert!(matched > 0, "没有任何程序集能与官方字典按 id 对上");
    }

    #[test]
    fn end_to_end_document_passes_structure_checks() {
        // 用真作品的完整输入跑一遍端到端,断言 KN 文档的硬结构(validateBcm 的四个必填)
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("download/compile/raw/几何对战-联机.bcm4");
        if !path.exists() {
            eprintln!("跳过:缺少真作品样本 {}", path.display());
            return;
        }
        let mut source: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读作品")).expect("JSON");
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().deterministic_ids(true);
        let doc = convert_kitten4_document(&mut source, &options, &mut report).expect("转换");

        for key in ["actors", "scenes", "styles", "stageSize"] {
            assert!(doc.get(key).is_some(), "缺必填结构 {key}");
        }
        let actors = doc["actors"]["actorsDict"].as_object().expect("actorsDict");
        let scenes = doc["scenes"]["scenesDict"].as_object().expect("scenesDict");
        assert!(!actors.is_empty() && !scenes.is_empty());
        for (sid, scene) in scenes {
            for aid in scene["actorIds"].as_array().cloned().unwrap_or_default() {
                let aid = aid.as_str().unwrap_or_default();
                assert!(
                    actors.contains_key(aid),
                    "场景 {sid} 的 actorIds 含不存在的角色 {aid}"
                );
            }
        }
        assert_eq!(
            doc["procedures"]["proceduresDict"]
                .as_object()
                .map(|m| m.len()),
            Some(4)
        );
        assert!(
            report
                .warnings()
                .iter()
                .all(|w| !matches!(w, TranslateWarning::UnmappedBlock { .. })),
            "真实样例不应有未映射积木:{:#?}",
            report.warnings()
        );
        // 确定性:同一输入两次转换必须逐字节一致
        let mut report2 = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let doc2 = convert_kitten4_document(&mut source, &options, &mut report2).expect("转换2");
        assert_eq!(doc, doc2, "确定性 id 模式下两次转换必须一致");
    }

    /// 最强门:把真实作品的产物交给**官方 `validateBcm`**(编辑器自己的校验器)判。
    /// 依赖 `temp/harness`(逆向工具,不入库);不存在时跳过,与仓库其它真机测试同约定。
    #[test]
    fn generated_bcmkn_passes_official_validator_when_harness_present() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let harness = root.join("temp/harness/harness.js");
        if !harness.exists() {
            eprintln!("跳过:缺少官方 harness({})", harness.display());
            return;
        }
        let sample = root.join("download/compile/raw/几何对战-联机.bcm4");
        if !sample.exists() {
            eprintln!("跳过:缺少真作品样本 {}", sample.display());
            return;
        }
        let mut source: Value =
            serde_json::from_str(&std::fs::read_to_string(&sample).expect("读作品")).expect("JSON");
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().deterministic_ids(true);
        let doc = convert_kitten4_document(&mut source, &options, &mut report).expect("转换");
        let dir = unique_test_dir("forward");
        std::fs::create_dir_all(&dir).expect("建目录");
        let out = dir.join("geoduel.kn.bcmkn");
        std::fs::write(&out, serde_json::to_string(&doc).expect("序列化")).expect("写产物");
        std::fs::write(dir.join("report.md"), report.to_markdown()).expect("写报告");

        let script = format!(
            "const h=require({:?});const {{validateBcm}}=h.require(87123);             const fs=require('fs');const doc=JSON.parse(fs.readFileSync({:?},'utf8'));             const e=console.error;console.error=()=>{{}};let ok=false;try{{ok=validateBcm(doc);}}catch(_){{ok=false;}}             console.error=e;console.log(ok?'VALID':'INVALID');",
            harness.to_string_lossy(),
            out.to_string_lossy(),
        );
        let output = std::process::Command::new("node")
            .arg("-e")
            .arg(&script)
            .current_dir(root)
            .output();
        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                assert!(
                    stdout.contains("VALID"),
                    "官方 validateBcm 判定我们产出的 .bcmkn 不合法(原始输出:{stdout})"
                );
            }
            Ok(o) => panic!(
                "调用 node 校验失败:{} {}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            ),
            Err(e) => eprintln!("跳过:无法执行 node({e})"),
        }
    }

    /// 手工排障:把产物落到系统临时目录(不参与断言)
    #[allow(dead_code)]
    fn dump_real_work_for_external_validation() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("download/compile/raw/几何对战-联机.bcm4");
        if !path.exists() {
            return;
        }
        let mut source: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().deterministic_ids(true);
        let doc = convert_kitten4_document(&mut source, &options, &mut report).unwrap();
        let dir = unique_test_dir("forward2");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("geoduel.kn.bcmkn"),
            serde_json::to_string(&doc).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("report.md"), report.to_markdown()).unwrap();
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

    /// 走**新**管线(实体级并行),返回产物与报告
    fn convert(source: &mut Value, entity_concurrency: usize) -> (Value, TranslateReport) {
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
                    Some(bdj) => model::parse_block_data_json(bdj).expect("解析实体").tree,
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
            eprintln!("跳过:缺少真作品样本 {}", path.display());
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
            TranslateError::Decompiler(inner) => {
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
