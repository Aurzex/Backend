//! 转化报告:把"哪些积木没映射上、降级了、字段丢了"逐类计数,不静默吞掉。

use crate::core::convert::shared::EditorType;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::TargetEditor;

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
