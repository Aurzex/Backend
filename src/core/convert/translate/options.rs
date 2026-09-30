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
    /// 实现见 [`super::pipeline::convert_kitten4_document`]:每个实体用自己的临时 id,串行阶段按
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
    /// 折算只改并行度,不碰产物。直接调用 [`super::translate_value`] / [`super::translate_file`] 的
    /// 单文档入口不做折算(调用方自己要的并发,由 [`super::pipeline::workers`] 兜住"不超核数")。
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
