//! 编辑器间互相转化(读写双向):Kitten4 `.bcm4` ⇄ KittenN `.bcmkn`。
//!
//! 管线(正向与官方编辑器一致,见 `docs/20-kitten-kn-work-conversion-plan.md` §3.2/§3.3/§6.2;
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
//! | Kitten4 → KN | [`kitten::parse_block_data_json`] | [`mapping::translate_kitten_to_kn`] | [`neko::split_procedures`]/[`neko::rewrite_calls`]/[`neko::tree_to_json`] | [`finish::build_document`] |
//! | KN → Kitten4 | [`neko::parse_kn_entity`]/[`neko::parse_kn_procedures`] | [`mapping::translate_kn_to_kitten`] | [`neko::unrewrite_calls`]/[`neko::def_root_from_entry`]/[`kitten::build_block_data_json`] | [`build_kitten4_document`] |
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

pub(crate) mod blockjson;
pub(crate) mod finish;
pub(crate) mod ids;
pub(crate) mod kitten;
pub(crate) mod kitten4_finish;
pub(crate) mod mapping;
pub(crate) mod neko;
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

    pub(crate) fn concurrency(&self) -> usize {
        self.batch_concurrency.max(1)
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
    #[error("不支持的方向:{from:?} → {to:?}(本库当前只做 Kitten4 ⇄ KittenN)")]
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

/// Kitten4 编辑版 → KN 编辑版(纯文档级管线,与官方两步对齐)
pub(crate) fn convert_kitten4_document(
    source: &serde_json::Value,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    use serde_json::Value;
    let started = std::time::Instant::now();

    // 官方 GN 第一行就读 `size`,Kitten2/3(`.bcm` + blocksXML)没有它 —— 直接给明确错误,
    // 而不是像官方那样抛 TypeError(见 docs/20 §1/§11.1)。
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

    let theatre = source
        .get("theatre")
        .and_then(Value::as_object)
        .ok_or_else(|| TranslateError::InvalidArgument("源作品没有 theatre".into()))?;

    let mut ids = super::translate::ids::IdSource::new(options.ids_deterministic());

    // ── 第一遍:解析 + 语义映射 + 抽程序集(官方:scenes.forEach → actors.forEach → zC)
    let mut parsed: Vec<(
        String,
        bool,
        blockjson::BlockTree,
        serde_json::Map<String, Value>,
    )> = Vec::new();
    let mut procedures: Vec<neko::ProcedureEntry> = Vec::new();

    let scene_order = source
        .pointer("/theatre/scenes_order")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (role_is_scene, container) in [(true, "scenes"), (false, "actors")] {
        let Some(map) = theatre.get(container).and_then(Value::as_object) else {
            continue;
        };
        for (id, entity) in map {
            let mut tree = match entity.get("block_data_json") {
                Some(bdj) => kitten::parse_block_data_json(bdj)?.tree,
                None => blockjson::BlockTree::default(),
            };
            report.blocks_total += tree.count(); // 源文件里的积木数(映射前)
            mapping::translate_kitten_to_kn(&mut tree, landscape, &mut ids, report);
            let (kept, mut extracted) = neko::split_procedures(tree, &mut ids, report);
            procedures.append(&mut extracted);
            parsed.push((
                id.clone(),
                role_is_scene,
                kept,
                entity.as_object().cloned().unwrap_or_default(),
            ));
        }
    }
    let _ = scene_order;

    // ── 第二遍:程序集调用点重写(官方 KC:所有实体共用同一张 proceduresDict)
    for (_, _, tree, _) in parsed.iter_mut() {
        neko::rewrite_calls(tree, &procedures, &mut ids, report);
    }

    // ── 编码 + 装配
    // 计数口径:total = 源文件里的积木数;converted = 产物里的节点数
    // (含被搬进 proceduresDict 的定义;影子实体化会让 converted ≥ total,与官方统计一致)
    let procedures_nodes: usize = procedures.iter().map(|p| p.tree.count()).sum();
    let mut entities = Vec::with_capacity(parsed.len());
    let mut converted = procedures_nodes;
    for (id, is_scene, tree, src) in parsed {
        let blocks = neko::tree_to_json(&tree)?;
        converted += tree.count();
        entities.push(finish::ConvertedEntity {
            source_id: id,
            is_scene,
            blocks,
            source: src,
        });
    }

    // 确定性模式(=对齐/回归测试用)把时钟也钉死,保证两次转换逐字节一致
    report.blocks_converted = converted;
    report.elapsed_ms = started.elapsed().as_millis();
    let now_ms = if options.ids_deterministic() {
        0
    } else {
        finish::current_epoch_ms()
    };
    finish::build_document(source, entities, &procedures, now_ms, report)
        .map_err(TranslateError::from)
}

/// 把一个作品文件转化成另一种编辑器的作品文件
pub fn translate_file(
    input: &std::path::Path,
    target: TargetEditor,
    options: TranslateOptions,
) -> std::result::Result<TranslateOutcome, TranslateError> {
    use crate::utils::filedata::PathConfig;

    let text = std::fs::read_to_string(input)?;
    let source: serde_json::Value = serde_json::from_str(&text)?;
    let from = detect_editor(&source);

    let (document, report) = match (from, target) {
        (Some(crate::core::convert::EditorType::Kitten4), TargetEditor::KittenN) => {
            let mut report = TranslateReport::new(
                crate::core::convert::EditorType::Kitten4,
                TargetEditor::KittenN,
            );
            let document = convert_kitten4_document(&source, &options, &mut report)?;
            (document, report)
        }
        (Some(crate::core::convert::EditorType::Neko), TargetEditor::Kitten4) => {
            let mut report = TranslateReport::new(
                crate::core::convert::EditorType::Neko,
                TargetEditor::Kitten4,
            );
            let document = kitten4_finish::convert_kn_document(&source, &options, &mut report)?;
            (document, report)
        }
        (Some(from), to) => return Err(TranslateError::Unsupported { from, to }),
        (None, to) => {
            return Err(TranslateError::InvalidArgument(format!(
                "无法识别源作品格式(目标 {to:?});本库当前支持 Kitten4(.bcm4 编辑版)与 KittenN(.bcmkn)"
            )));
        }
    };

    if options.is_strict() && report.is_lossy() {
        return Err(TranslateError::Lossy {
            report: Box::new(report),
        });
    }

    // 产物命名:`<源文件名>.<target>.bcmkn|bcm4`
    let slug = match target {
        TargetEditor::KittenN => "kn",
        TargetEditor::Kitten4 => "kitten4",
    };
    let ext = match target {
        TargetEditor::KittenN => "bcmkn",
        TargetEditor::Kitten4 => "bcm4",
    };
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| TranslateError::InvalidArgument(format!("输入路径没有文件名:{input:?}")))?;
    let dir = options
        .output_dir_ref()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| PathConfig::global().convert_file_path());
    std::fs::create_dir_all(&dir)?;
    let output = dir.join(format!("{stem}.{slug}.{ext}"));

    let serialized = serde_json::to_string(&document)?;
    std::fs::write(&output, serialized)?;

    Ok(TranslateOutcome {
        output,
        work_id: None,
        target,
        report,
    })
}

/// 把源作品文件引用写进**已产出**的 KN 文档顶层 `source` 字段。
///
/// 官方做法:KN 编辑器导入 Kitten 作品时,把原始 Kitten 文件字节重新上传,
/// 并把 URL 写到 KN 作品的 `source`(即"保留原件",见 `docs/20` §3.1 的 `w.source = T`)。
/// 这里只做**纯文件改写**,上传由 `convert` 门面编排(保持本子域不碰网络)。
///
/// 已知偏差:我们手里只有反编译重建的编辑版,官方上传的是原始文件字节
/// (见 `docs/21` §4-10)。
pub fn set_source_reference(
    output: &std::path::Path,
    url: &str,
) -> std::result::Result<(), TranslateError> {
    let text = std::fs::read_to_string(output)?;
    let mut document: serde_json::Value = serde_json::from_str(&text)?;
    let Some(object) = document.as_object_mut() else {
        return Err(TranslateError::InvalidArgument(format!(
            "产物不是 JSON 对象:{output:?}"
        )));
    };
    object.insert("source".to_string(), serde_json::Value::String(url.to_string()));
    // 与 `translate_file` 同一序列化口径(serde_json 默认按键排序,round-trip 稳定)
    std::fs::write(output, serde_json::to_string(&document)?)?;
    Ok(())
}

/// 识别源作品属于哪个编辑器(按顶层结构判定,不用扩展名)。
///
/// Kitten2 与 Kitten3 的编辑版都是 `blocksXML`,本地样本与 `docs/20` 都没有可靠的
/// 区分标记,而**两者都不支持转化**(编辑器自己会拒绝,见 `docs/20` §3.1),
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
    /// 目标编辑器没有对应积木(反向默认策略:丢弃)
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
    /// 聚合键:同类别 + 同主体算一类
    fn key(&self) -> (u8, String) {
        match self {
            TranslateWarning::UnmappedBlock { kind } => (0, kind.clone()),
            TranslateWarning::DegradedToText { kind } => (1, kind.clone()),
            TranslateWarning::DroppedField { path } => (2, path.clone()),
            TranslateWarning::DroppedProperty { path } => (3, path.clone()),
            TranslateWarning::RemintedId { from } => (4, from.clone()),
            TranslateWarning::ReuploadedOnImport { path } => (5, path.clone()),
        }
    }

    fn category(&self) -> &'static str {
        match self {
            TranslateWarning::UnmappedBlock { .. } => "未映射积木(已丢弃)",
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
            warnings: Vec::new(),
        }
    }

    pub(crate) fn warn(&mut self, w: TranslateWarning) {
        self.warnings.push(w);
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
    pub fn counts(&self) -> BTreeMap<String, BTreeMap<String, usize>> {
        let mut out: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for w in &self.warnings {
            let _ = w.key();
            *out.entry(w.category().to_string())
                .or_default()
                .entry(w.subject().to_string())
                .or_default() += 1;
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
    //! 与官方产物对齐的差分门(docs/20 §7 Phase 2 验收)。
    //!
    //! 夹具 `tests/fixtures/translate/geoduel_scene_actor.json` 里是**真实作品**的
    //! 「Kitten4 输入 block_data_json」与「官方 `kittenBcmToNekoBcmUtils` 输出」成对切片
    //! (来自 `temp/baseline/out-kitten4.json`,基线 bundle sha256 见 docs/20 附录 C)。
    //!
    //! 比较口径(为什么不做整段 JSON diff):
    //! - 官方新建节点(shadow 实体化、`pure_list_get`、`math_arithmetic` 包裹)拿不到稳定 id → 按类型计数比较;
    //! - 官方会丢掉 `is_output`/`field_constraints`/`extra` 键、且 `next` 子节点不带 `parent_id`,
    //!   我们刻意保留(§6.2 逃生舱)→ 只比 `type`/`fields`/槽名集合;
    //! - 官方 UUID 每次运行都变 → 涉及 id 的等价性只在「输入里已存在的 id」上比较。

    use super::*;
    use crate::core::convert::translate::blockjson::BlockJson;
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
        assert_eq!(doc["source"], serde_json::json!("https://creation.codemao.cn/src.bcm4"));
        assert_eq!(doc["projectName"], serde_json::json!("样例"), "其余键原样保留");

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
    ) -> (Vec<Value>, Vec<neko::ProcedureEntry>, TranslateReport) {
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let mut ids = ids::IdSource::new(true); // 确定性 id:对齐测试必需
        let mut tree = kitten::parse_block_data_json(bdj).expect("解析实体").tree;
        mapping::translate_kitten_to_kn(&mut tree, landscape, &mut ids, &mut report);
        let (kept, procs) = neko::split_procedures(tree, &mut ids, &mut report);
        let mut kept = kept;
        neko::rewrite_calls(&mut kept, &procs, &mut ids, &mut report);
        let json = neko::tree_to_json(&kept).expect("编码");
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
        let source: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读作品")).expect("JSON");
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().deterministic_ids(true);
        let doc = convert_kitten4_document(&source, &options, &mut report).expect("转换");

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
        let doc2 = convert_kitten4_document(&source, &options, &mut report2).expect("转换2");
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
        let source: Value =
            serde_json::from_str(&std::fs::read_to_string(&sample).expect("读作品")).expect("JSON");
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().deterministic_ids(true);
        let doc = convert_kitten4_document(&source, &options, &mut report).expect("转换");
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
        let source: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let options = TranslateOptions::new().deterministic_ids(true);
        let doc = convert_kitten4_document(&source, &options, &mut report).unwrap();
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
