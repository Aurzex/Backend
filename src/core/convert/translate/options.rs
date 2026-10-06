//! 转化选项、结果与错误(从 `translate/mod.rs` 拆出)。
//!
//! 公开路径不变:`translate/mod.rs` 用 `pub use` 把这些名字再导出,外部
//! (`translate::TranslateOptions` 等)与拆分前逐字一致。

use super::report::TranslateReport;
use crate::core::convert::shared::EditorType;

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
    ///
    /// 可见性是 `core::convert`:它比 `pub(crate)` 小、比 `pub(super)`(= `translate`)大 ——
    /// 这一小撮方法只被**域门面**(`convert/mod.rs`)与域内共用,故精确到「域」这一层。
    pub(in crate::core::convert) fn as_editor(self) -> EditorType {
        match self {
            TargetEditor::KittenN => EditorType::Neko,
            TargetEditor::Kitten4 => EditorType::Kitten4,
        }
    }

    /// 产物文件名中的目标标识
    pub(super) fn file_slug(self) -> &'static str {
        match self {
            TargetEditor::KittenN => "kn",
            TargetEditor::Kitten4 => "kitten4",
        }
    }

    /// 产物扩展名(不含点)
    pub(super) fn file_extension(self) -> &'static str {
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

/// 实体级并发的取值(默认 [`EntityConcurrency::Auto`])
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntityConcurrency {
    /// 自动:作品**够大**才开(阈值与实测夹逼见 [`super::pipeline::entity_workers`])。
    /// `cap` 是批量入口折算出的核数上限(`None` = 未折算,由可用核数兜)
    Auto { cap: Option<usize> },
    /// 固定线程数(≥1;显式给 1 即强制串行)
    Fixed(usize),
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
    entity_concurrency: EntityConcurrency,
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
            // 默认**自动**(够大才开):小作品上并行的收益测不出来,而它要付"临时 id 记账"的分配
            // (NEMO 侧实测 +13.9%,见 `../rounds/48`)⇒ 让阈值而不是调用方来兜这件事
            entity_concurrency: EntityConcurrency::Auto { cap: None },
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
    ///
    /// ⚠️ **已知偏差(已文档化,不再改)**:上传给平台的那份"源文件"是**反编译重建的编辑版**,
    /// 不是源作品的原始字节 —— 原始 `.bcm*` 的字节在反编译阶段就被解开了,本库手上只有重建版。
    /// 因此平台侧的"保留原件"打开的是重建版;可读、可再转换等语义不受影响。
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

    // 取值器可见性分两档:域门面(`convert/mod.rs`)要读的那几个收到 `core::convert`
    // (比 `pub(crate)` 小、比 `pub(super)` 大),只在本子树内用的保持 `pub(super)`(= `translate`)。
    pub(in crate::core::convert) fn source_version_ref(&self) -> Option<&str> {
        self.source_version.as_deref()
    }

    pub(super) fn output_dir_ref(&self) -> Option<&std::path::Path> {
        self.output_dir.as_deref()
    }

    pub(in crate::core::convert) fn upload_enabled(&self) -> bool {
        self.upload
    }

    pub(super) fn is_strict(&self) -> bool {
        self.strict
    }

    pub(in crate::core::convert) fn keeps_source(&self) -> bool {
        self.keep_source
    }

    pub(super) fn orientation(&self) -> StageOrientation {
        self.stage
    }

    pub(super) fn ids_deterministic(&self) -> bool {
        self.deterministic_ids
    }

    /// 批量转化的并发数(≥1,默认 1;与 `DecompileOptions::batch_concurrency` 同义)
    pub fn batch_concurrency(mut self, n: usize) -> Self {
        self.batch_concurrency = n.max(1);
        self
    }

    /// 实体级并发(正向与 NEMO;默认 **自动**,见下):单个作品文档内按实体/程序集并行(方案 25 S3a)。
    ///
    /// **取值**:`n = 0` 与 `n = 1` 都是**强制串行**(等价);`n ≥ 2` 是固定线程数;不调用本方法
    /// (默认)则是**自动** —— 作品够大才开,阈值与实测夹逼见
    /// [`super::pipeline::entity_workers`](含 `rounds/51` 的交叉点读数)。
    ///
    /// 实现见 [`super::pipeline::convert_kitten4_document`](Kitten4 方向)与
    /// [`super::nemo::convert_nemo_document`](NEMO 方向,同构的四阶段):每个实体用自己的临时 id,
    /// 串行阶段按项序的账本兑现最终 id,所以
    ///
    /// - `deterministic_ids(true)`(基准/回归测试口径)下,产物与并发 1 **逐字节相同**
    ///   (也有 `tests/convert_bench.rs` 的 1 vs N 同 SHA256 门);
    /// - 非确定性模式下 id 本来就是随机的,承诺降为"同样**合法且唯一**",值不再保证等于并发 1
    ///   (与串行实现本身也不可复现同理);
    /// - 反向(KN → Kitten4)暂不支持本选项,取值被忽略。
    ///
    /// 代价:NEMO 方向的记录法使分配次数 +13.9%(`docs/rounds/48` §4,"自动"模式下这笔代价只落在
    /// 够大的作品上);批量入口 [`translate_works`](crate::core::convert::translate_works) 会按
    /// 作品级并发与可用核数**折算**上限(见 [`TranslateOptions::fold_entity_concurrency`]),
    /// 避免"作品级 × 实体级"两级超订。
    pub fn entity_concurrency(mut self, n: usize) -> Self {
        self.entity_concurrency = EntityConcurrency::Fixed(n.max(1));
        self
    }

    pub(in crate::core::convert) fn concurrency(&self) -> usize {
        self.batch_concurrency.max(1)
    }

    /// 实体级并发的取值(管线在拿到工作项后据此解析真实线程数)
    pub(super) fn entity_concurrency_plan(&self) -> EntityConcurrency {
        self.entity_concurrency
    }

    /// 按**作品级并发**与**可用核数**折算实体级并发的**核数预算**(方案 25 §7 阻塞 #6;批量入口调用)
    ///
    /// 两级并发相乘会超订(作品级 `b` × 实体级 `e` 个翻译线程),所以批量入口把每作品
    /// 分到的核数 `可用核数 / 有效作品并发`(向下取整、至少 1)当作实体级的核数上限:
    ///
    /// - 固定值:`e' = clamp(min(e, 可用核数 / b), 1, ∞)`;
    /// - 自动:只压低核数预算,不动"够不够大"的判断(`Auto { cap: Some(可用核数 / b) }`)——
    ///   阈值按作品大小在管线里判,与作品级并发无关。
    ///
    /// 折算只改并行度,不碰产物。直接调用 [`super::translate_value`] / [`super::translate_file`] 的
    /// 单文档入口不做折算(那边由可用核数兜住"不超核数")。
    pub(in crate::core::convert) fn fold_entity_concurrency(
        mut self,
        works: usize,
        available: usize,
    ) -> Self {
        let batch = self.concurrency().min(works.max(1));
        let share = (available.max(1) / batch).max(1);
        self.entity_concurrency = match self.entity_concurrency {
            EntityConcurrency::Fixed(n) => EntityConcurrency::Fixed(n.min(share).max(1)),
            EntityConcurrency::Auto { cap } => EntityConcurrency::Auto {
                cap: Some(cap.map_or(share, |cap| cap.min(share)).max(1)),
            },
        };
        self
    }
}

impl Default for TranslateOptions {
    fn default() -> Self {
        Self::new()
    }
}

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
    #[error("转换域错误: {0}")]
    Convert(#[from] crate::core::convert::ConvertError),
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
