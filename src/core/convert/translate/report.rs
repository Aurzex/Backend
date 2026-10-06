//! 转化报告:把"哪些积木没映射上、降级了、字段丢了"逐类计数,不静默吞掉。
//!
//! 从 `translate/mod.rs` 拆出(报告层是横切面,原先五个兄弟模块都要向上依赖门面才能引用它)。

use super::options::TargetEditor;
use crate::core::convert::shared::EditorType;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// 一类告警
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TranslateWarning {
    /// 目标编辑器**没有对应积木**的类型。
    ///
    /// ⚠️ **行为已两次变更**:早期是"保留 KN 原类型名 + 告警、不丢积木",但编辑器遇到不认识的
    /// 名字会**整份工作区加载失败**(`rounds/34` §4nonies)⇒ 改成**剔除/清空**;
    /// 而"剔除"会让积木**真的消失**(`rounds/38` 用 id 口径定案:某件作品丢了 4 个可达块)
    /// ⇒ 现在写出阶段把它**就地改成「未收录积木」标记**
    /// (`incompatible_block` / `incompatible_output_block`,见 `assembly.rs::mark_unknown_blocks`)。
    /// ⇒ **这类告警意味着"块还在画布上,但内容恢复不出来"** —— 看得见的损失,不是静默少块。
    /// 判定"哪些类型属此类"见 `mapping.rs` 顶部注释。
    ///
    /// ⚠️ **计数走结构字段,不走文案(rounds/39 §W4 起)**:此前 `kind` 里拼
    /// `(Kitten4 编辑器不认识,已改成未收录积木 N 块)` / `(…已清空 N 条影子)`,而"标记量预算门"
    /// (`reverse_tests::marker_counts`)只能**反解中文**才拿到读数 —— 文案改一个空格(更别说改词)
    /// 读数就**静默变 0**、`MARKER_BUDGET` 变成"永远通过"。现在拆成 `kind`(**纯类型名**)+
    /// 下面两个字段,读数直接读字段。**"别把协议塞进给人看的文案"这条教训仍然有效**:
    /// 要动字段就先动 `marker_counts` 与 `MARKER_BUDGET` 基线,别在 `kind` 里复活计数后缀。
    UnmappedBlock {
        /// 类型名(**纯**:不再拼 `(…已改成未收录积木 N 块)` 这类后缀)
        kind: String,
        /// 该类型被就地改成「未收录积木」标记的**块数**(写出阶段 `mark_unknown_blocks` 填;
        /// 只报"这个类型认不出"的发射点填 `0`)
        marked: u64,
        /// 该类型在 `shadows` XML 里被清空的**影子条数**(影子是槽位默认值,不换标记块,只清空)
        cleared_shadows: u64,
    },
    /// 映射到文本占位积木(`bcm_translator_text_*`),原文进 mutation
    DegradedToText { kind: String },
    /// 结构上无法表达的字段被丢弃(`path` 形如 `actors.<id>.rotation_type`)
    DroppedField { path: String },
    /// **反向的类型歧义**:一个 KN 类型在官方正向表里对应**多个** Kitten 原类型
    /// (如 `change_variables` ← `change_variables` | `change_cloud_variable`)。
    /// 语义等价、积木不丢,但 Kitten 原类型名不可恢复 ⇒ 仍计入有损。
    /// `chosen` 为 `None` 表示保留 KN 名(它本身就是 Kitten 侧用过的名字)。
    AmbiguousType {
        kind: String,
        candidates: Vec<String>,
        chosen: Option<String>,
    },
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
            TranslateWarning::UnmappedBlock { .. } => "未映射积木(已改成未收录积木 / 清空影子)",
            TranslateWarning::DegradedToText { .. } => "降级为文本占位积木",
            TranslateWarning::DroppedField { .. } => "丢弃字段",
            TranslateWarning::AmbiguousType { .. } => "类型歧义(原类型有多个)",
            TranslateWarning::DroppedProperty { .. } => "丢弃实体属性",
            TranslateWarning::RemintedId { .. } => "重新生成 id",
            TranslateWarning::ReuploadedOnImport { .. } => "官方重传资源(非损失)",
        }
    }

    fn subject(&self) -> &str {
        match self {
            TranslateWarning::UnmappedBlock { kind, .. }
            | TranslateWarning::DegradedToText { kind }
            | TranslateWarning::AmbiguousType { kind, .. } => kind,
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
    /// 正向与 NEMO 方向 = `min(entity_concurrency, 工作项数, 可用核数)`(见
    /// [`super::options::TranslateOptions::entity_concurrency`]);反向暂不支持实体级并行,恒为 1。
    /// 这是"并行真的发生了"的可观测证据:基准/单测用它挡住"并发对照在单核上退化成串行"
    /// 这种空门(方案 25 §9)。
    pub entity_workers: usize,
    warnings: Vec<TranslateWarning>,
}

impl TranslateReport {
    pub(super) fn new(from: EditorType, to: TargetEditor) -> Self {
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

    pub(super) fn warn(&mut self, w: TranslateWarning) {
        self.warnings.push(w);
    }

    /// 取出全部告警(实体级并行按项收集局部报告后,再按项序并入全局报告;见 [`super::pipeline::merge_report`])
    pub(super) fn take_warnings(&mut self) -> Vec<TranslateWarning> {
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
