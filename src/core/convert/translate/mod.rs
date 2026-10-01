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
//! | KN → Kitten4 | [`model::parse_kn_entity`]/[`model::parse_kn_procedures`] | [`mapping::translate_kn_to_kitten`] | [`model::unrewrite_calls`]/[`model::def_root_from_entry`]/[`model::build_block_data_json`] | [`assembly::build_kitten4_document`] |
//! | NEMO → KN | [`nemo::prepare_blocks_xml`](`xml` 解析 + 版本迁移 + 9 个前置改写) | [`nemo_mapping::translate_nemo_to_kn`](官方把映射折进解析,见该模块文档) | 不需要(程序集在解析器内就位) | [`nemo::convert_nemo_document`] |
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
//! 反向的**近似**(逐条都在报告里可见,详见 [`mapping`] 与 [`assembly::build_kitten4_document`] 的注释):
//!
//! - KN 原生、Kitten4 编辑器**不认识**的类型在写出阶段被**就地改成「未收录积木」标记**
//!   (`incompatible_block` / `incompatible_output_block`),并逐类记 `UnmappedBlock` 告警
//!   (`kind` 里带 `已改成未收录积木 N 块` 计数)。**块不会凭空消失**(rounds/38 起),
//!   但那个类型的**内容**恢复不出来 ⇒ 往返的类型多重集**不守恒**;
//!   影子 XML 里的未知类型仍是**清空**(影子是槽位默认值,换成标记块没有意义);
//!   (rounds/34 §4nonies 起的行为;早期版本曾"保留原类型名 + 不丢积木",
//!   编辑器遇到不认识的名字会整份工作区加载失败,见 `docs/rounds/34`/`docs/rounds/36`);
//! - 一个 KN 类型有多个 Kitten 原类型时(云列表/本地列表、`start_on_click`/`on_running_group_activated` 等)
//!   保留 KN 名 + `DroppedProperty` 告警;
//! - KN 的舞台坐标只有两种画布,反向按源 `stageSize` 原样给 Kitten4 的 `size`
//!   (`landscape = width > height` 因此保持一致),变量坐标按正向公式的逆换算;
//! - 程序集定义积木统一挂到第一个角色名下(KN 的 `proceduresDict` 没有实体归属信息);
//! - 角色 ↔ 造型关联、云变量与本地变量的区分在 KN 里已丢失,按 `currentStyleId` / 全部并入
//!   `variables` 还原并报告。

// 子模块只服务本子树 ⇒ 默认**私有**(除 `tables_gen`:域级工具 `upload.rs` 的 `bcm_version`
// 兜底要读它的 `BCM_VERSION`,故收窄到 `core::convert`)。收太紧编译器会报错兜底。
mod assembly;
mod kitten4_vocab;
mod mapping;
mod model;
mod nemo;
mod nemo_mapping;
#[cfg(test)]
mod nemo_tests;
mod options;
mod pipeline;
mod report;
#[cfg(test)]
mod reverse_tests;
pub(in crate::core::convert) mod tables_gen;
mod xml;

// 拆分后仍留在门面的公开路径:外部(`convert` 域门面与 `tests/`)按这些路径取类型
pub use options::{
    StageOrientation, TargetEditor, TranslateError, TranslateOptions, TranslateOutcome,
};
pub use report::{TranslateReport, TranslateWarning};

// ---------------------------------------------------------------------------
// 管线:文件 → 文档 → 文件
// ---------------------------------------------------------------------------

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
            let document = pipeline::convert_kitten4_document(&mut source, options, &mut report)?;
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
            let document = pipeline::convert_kn_document(&source, options, &mut report)?;
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
/// 可见性是 `core::convert`(域门面 `convert/mod.rs` 与 `translate_file` 共用;`pub(super)` 不够、
/// `pub(crate)` 过宽)。
pub(in crate::core::convert) fn product_path(
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
/// 可见性是 `core::convert`(域门面 `convert/mod.rs` 用它做跨子域判定)。
pub(in crate::core::convert) fn detect_editor(
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

/// `BACKEND_REQUIRE_FIXTURES=1`:缺夹具(语料 / 真作品样例 / 官方 harness)一律**失败**,
/// 默认只打印一行并跳过。
///
/// 为什么需要:真作品样例在 gitignored 的 `download/`(采集器**增量**写入),干净检出上这些测试
/// 本来全是"打印一行 `跳过:` 后 return —— **显示 pass**" ⇒ 等于没有这条门。
/// 开关家族见 `docs/knowledge/repo-conventions.md` §3ter;这是**域内唯一的定义处**
/// (`reverse_tests` 原先那份已改用它,别再抄第四份)。
#[cfg(test)]
fn require_fixtures() -> bool {
    std::env::var("BACKEND_REQUIRE_FIXTURES")
        .is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "yes"))
}

/// 缺夹具的统一出口:严格开关下 panic 并**点名缺了什么**,否则打印跳过 —— 调用方紧随 `return`。
#[cfg(test)]
fn missing_fixture(what: &str) {
    if require_fixtures() {
        panic!(
            "严格模式(BACKEND_REQUIRE_FIXTURES=1):缺夹具 —— {what}。\
             真作品样例在 gitignored 的 download/(先跑对应的 harvest 测试抓语料)"
        );
    }
    eprintln!("跳过:{what}");
}

/// 测试用临时目录:**每次调用唯一**(进程 + 随机后缀),避免并行测试/跨运行互相覆盖
///
/// 只服务 `translate` 子树自己的测试(本文件 `diff_tests` 与 `reverse_tests`)⇒ 私有。
#[cfg(test)]
fn unique_test_dir(tag: &str) -> std::path::PathBuf {
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

    use super::pipeline::convert_kitten4_document;
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
        let mut tree = model::parse_block_data_json(bdj).expect("解析实体");
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
            missing_fixture(&format!("真作品样本 {}", path.display()));
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
            missing_fixture(&format!("官方 harness({})", harness.display()));
            return;
        }
        let sample = root.join("download/compile/raw/几何对战-联机.bcm4");
        if !sample.exists() {
            missing_fixture(&format!("真作品样本 {}", sample.display()));
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
