//! Kitten4 文档装配(反向):KN 文档 → Kitten4 编辑版 JSON。
//!
//! 与正向的 `finish.rs` 对称:那里把 Kitten 实体装配成 KN 文档,这里把 KN 实体装配回 Kitten4。
//! 官方没有反向实现(编辑器只做 K→KN),所以本文件是**自建**的逆映射,规则见
//! `docs/20-kitten-kn-work-conversion-plan.md` §4;所有无法回填的字段都进报告。

use super::blockjson::BlockTree;
use super::finish::{num, project_name_at};
use super::{TranslateReport, TranslateWarning};
use super::{
    StageOrientation, TranslateError, TranslateOptions, blockjson, ids, kitten, mapping, neko,
    tables_gen,
};
use crate::core::convert::shared::{DecompilerError, Result};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------------------
// 反向:KN `.bcmkn` → Kitten4 `.bcm4`(本项目自建;官方无此方向,见 docs/20 §4)
// ---------------------------------------------------------------------------

/// Kitten4 编辑版常量(取自真实作品 `download/compile/raw/几何对战-联机.bcm4`,
/// `version=25` / `application_version=4.11.20` / `type=1` / `work_type="KITTEN"`)
const KITTEN4_VERSION: i64 = 25;
const KITTEN4_APPLICATION_VERSION: &str = "4.11.20";
/// Kitten4 里场景实体的固定名(显示名在 `screen_name`,与正向 `finish::SCENE_NAME` 对称)
const KITTEN4_SCENE_NAME: &str = "Background";
/// 正向横屏把角色坐标乘 `10/13`(finish.rs `LANDSCAPE_POSITION_SCALE`),反向除回去
const LANDSCAPE_POSITION_BACK_SCALE: f64 = 13.0 / 10.0;
/// KN 变量样式图标 → Kitten 主题(finish.rs `theme_style` 的逆;`default` → `common`)
fn theme_of_style(style: &str) -> &'static str {
    match style {
        "icon_medal" => "score",
        "icon_heart" => "HP",
        "icon_hourglass" => "clock",
        "icon_coin" => "coin",
        "text" => "pure",
        _ => "common",
    }
}

/// KittenN 编辑版 → Kitten4 编辑版(自建反向管线)
///
/// 步骤与正向**对称**:`neko::parse_kn_entity`(树)→ `mapping::translate_kn_to_kitten`(语义反演)
/// → `neko::unrewrite_calls`(`KC` 的逆)+ `neko::def_root_from_entry`(`zC` 的逆)
/// → `kitten::build_block_data_json`(邻接表)→ Kitten4 文档装配。
///
/// 画布:`StageOrientation::Auto`(默认)把源 `stageSize` 原样当 Kitten4 的 `size`,于是
/// `landscape = width > height` 两侧一致(清屏/坐标换算自洽);显式指定 `Portrait`/`Landscape`
/// 会换掉画布尺寸但**不**改坐标与横屏壳的判定,只在你确实要把作品搬到另一种画布时才用。
pub(crate) fn convert_kn_document(
    source: &serde_json::Value,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> std::result::Result<serde_json::Value, TranslateError> {
    use serde_json::{Map, Value};
    let started = std::time::Instant::now();

    let src = source.as_object().ok_or_else(|| {
        TranslateError::InvalidArgument("源作品不是 JSON 对象:无法按 KittenN 作品解析".into())
    })?;
    let (kn_w, kn_h) = kn_stage_size(src);
    let landscape = kn_w > kn_h;
    let (canvas_w, canvas_h) = match options.orientation() {
        StageOrientation::Portrait => (tables_gen::STAGE_PORTRAIT.0, tables_gen::STAGE_PORTRAIT.1),
        StageOrientation::Landscape => {
            (tables_gen::STAGE_LANDSCAPE.0, tables_gen::STAGE_LANDSCAPE.1)
        }
        StageOrientation::Auto => (kn_w, kn_h),
    };

    let mut ids = ids::IdSource::new(options.ids_deterministic());

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
                neko::parse_kn_entity(entity.get("nekoBlockJsonList").unwrap_or(&Value::Null))?;
            report.blocks_total += tree.count();
            mapping::translate_kn_to_kitten(&mut tree, landscape, report);
            entities.push(KnEntity {
                source_id: id.clone(),
                is_scene,
                tree,
                source: entity.as_object().cloned().unwrap_or_default(),
            });
        }
    }

    // ── 程序集(`proceduresDict`):定义体同样要过一遍语义反演
    let mut procedures = neko::parse_kn_procedures(src.get("procedures").unwrap_or(&Value::Null))?;
    for entry in &mut procedures {
        report.blocks_total += entry.tree.count();
        mapping::translate_kn_to_kitten(&mut entry.tree, landscape, report);
    }
    let call_targets = neko::call_targets(&procedures);
    for entity in &mut entities {
        neko::unrewrite_calls(&mut entity.tree, &call_targets, &mut ids, report);
    }
    // 定义体里的调用点同样要还原(正向只重写实体树,定义体里留着 id 的话 Kitten4 会看到拿不到引用的 UUID;
    // 这是**有意的超集**,与 `annotate_param_refs` 的超集口径一致 —— 类型计数不受影响)
    for entry in &mut procedures {
        neko::unrewrite_calls(&mut entry.tree, &call_targets, &mut ids, report);
    }

    // 定义根积木挂到第一个角色(没有角色就挂到第一个场景;都没有 → 报告后跳过)
    let host = entities
        .iter()
        .position(|entity| !entity.is_scene)
        .or(if entities.is_empty() { None } else { Some(0) });
    for entry in &procedures {
        let root = neko::def_root_from_entry(entry, &mut ids, report)?;
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
        for id in duplicate_ids(&entity.tree) {
            report.warn(TranslateWarning::RemintedId { from: id });
        }
        let value = kitten::build_block_data_json(&entity.tree, &mut ids)?;
        converted += entity.tree.count();
        blocks_by_entity.push((index, value));
    }
    report.blocks_converted = converted;
    report.elapsed_ms = started.elapsed().as_millis();

    Ok(build_kitten4_document(
        src,
        &entities,
        &blocks_by_entity,
        landscape,
        (canvas_w, canvas_h),
        (kn_w, kn_h),
        report,
    ))
}

/// 树里出现两次以上的 id(每个重复值报一次)
fn duplicate_ids(tree: &blockjson::BlockTree) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut counted: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut duplicated = Vec::new();
    tree.walk(&mut |node| {
        if let Some(id) = node.id.as_deref().filter(|id| !id.is_empty())
            && !seen.insert(id.to_string())
            && counted.insert(id.to_string())
        {
            duplicated.push(id.to_string());
        }
    });
    duplicated
}

/// 一个已反演完成的实体
struct KnEntity {
    source_id: String,
    is_scene: bool,
    tree: blockjson::BlockTree,
    source: serde_json::Map<String, serde_json::Value>,
}

/// KN 舞台尺寸(官方只有 `562×900` / `900×562` 两种;缺失时按竖屏)
fn kn_stage_size(src: &serde_json::Map<String, serde_json::Value>) -> (f64, f64) {
    let stage = src.get("stageSize").and_then(serde_json::Value::as_object);
    let pick = |key: &str, fallback: f64| -> f64 {
        stage
            .and_then(|s| s.get(key))
            .and_then(serde_json::Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(fallback)
    };
    (
        pick("width", tables_gen::STAGE_PORTRAIT.0),
        pick("height", tables_gen::STAGE_PORTRAIT.1),
    )
}

/// 装配 Kitten4 编辑版文档
fn build_kitten4_document(
    src: &serde_json::Map<String, serde_json::Value>,
    entities: &[KnEntity],
    blocks_by_entity: &[(usize, serde_json::Value)],
    landscape: bool,
    canvas: (f64, f64),
    kn_stage: (f64, f64),
    report: &mut TranslateReport,
) -> serde_json::Value {
    use serde_json::{Map, Value, json};

    let mut block_of: std::collections::BTreeMap<&str, &Value> = std::collections::BTreeMap::new();
    for (index, value) in blocks_by_entity {
        block_of.insert(entities[*index].source_id.as_str(), value);
    }

    // actor → 所属场景:KN 的场景用 `actorIds` 反向指认;找不到就落到第一个场景
    let first_scene = entities
        .iter()
        .find(|e| e.is_scene)
        .map(|e| e.source_id.clone());
    let mut scene_of_actor: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for entity in entities.iter().filter(|e| e.is_scene) {
        for actor in entity
            .source
            .get("actorIds")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            .unwrap_or_default()
        {
            scene_of_actor
                .entry(actor.to_string())
                .or_insert_with(|| entity.source_id.clone());
        }
    }

    let mut actors = Map::new();
    let mut scenes = Map::new();
    for entity in entities {
        let source = entity.source.clone();
        let Some(blocks) = block_of.get(entity.source_id.as_str()) else {
            continue;
        };
        if entity.is_scene {
            scenes.insert(
                entity.source_id.clone(),
                Value::Object(kitten4_scene(&source, (*blocks).clone(), report)),
            );
        } else {
            let scene = scene_of_actor
                .get(&entity.source_id)
                .cloned()
                .or_else(|| first_scene.clone());
            actors.insert(
                entity.source_id.clone(),
                Value::Object(kitten4_actor(
                    &source,
                    (*blocks).clone(),
                    scene,
                    landscape,
                    report,
                )),
            );
        }
    }

    let scenes_order: Vec<Value> = match src
        .get("scenes")
        .and_then(Value::as_object)
        .and_then(|scenes| scenes.get("sortList"))
        .and_then(Value::as_array)
    {
        Some(order) => order.clone(),
        None => scenes
            .keys()
            .map(|key| Value::String(key.clone()))
            .collect(),
    };

    let mut theatre = Map::new();
    theatre.insert("scenes_order".into(), Value::Array(scenes_order));
    theatre.insert("scenes".into(), Value::Object(scenes));
    theatre.insert("actors".into(), Value::Object(actors));
    theatre.insert("styles".into(), Value::Object(kitten4_styles(src, report)));
    theatre.insert("videos".into(), json!({}));
    theatre.insert("groups".into(), json!({}));
    theatre.insert("timer".into(), json!({}));

    let (variables, variable_order) = kitten4_variables(src, landscape, canvas, kn_stage, report);
    let (audio, audio_order) = kitten4_audio(src);

    let mut doc = Map::new();
    doc.insert("work_type".into(), json!("KITTEN"));
    doc.insert("type".into(), json!(1));
    doc.insert("version".into(), json!(KITTEN4_VERSION));
    doc.insert(
        "application_version".into(),
        json!(KITTEN4_APPLICATION_VERSION),
    );
    doc.insert("project_name".into(), json!(project_name_at(src, "projectName")));
    doc.insert(
        "size".into(),
        json!({ "width": num(canvas.0), "height": num(canvas.1) }),
    );
    doc.insert("theatre".into(), Value::Object(theatre));
    doc.insert("variables".into(), Value::Object(variables));
    doc.insert("variable_order".into(), Value::Array(variable_order));
    doc.insert("cloud_variables".into(), json!({}));
    doc.insert("audio".into(), Value::Object(audio));
    doc.insert("audio_order".into(), Value::Array(audio_order));
    doc.insert("broadcasts".into(), kitten4_broadcasts(src, report));
    // 这几个键 Kitten4 也有,源里有就照搬
    for key in ["toolbox", "toolbox_order", "hidden_toolbox"] {
        if let Some(value) = src.get(key) {
            doc.insert(key.to_string(), value.clone());
        }
    }
    // KN 专属的顶层键:明确列出来、逐条记进报告(不静默丢弃)
    for key in [
        "stageSize",
        "projectName",
        "toolType",
        "previewUrl",
        "resourceZip",
        "guideUrl",
        "textToBlock",
        "aiImageUrls",
        "courseMaterials",
        "source",
        "version",
    ] {
        // 已映射(`stageSize`/`projectName`)的不算丢;`version` 一律重写成 Kitten4 的 25
        let unmapped = !matches!(key, "stageSize" | "projectName");
        if unmapped && src.get(key).is_some_and(|value| !value.is_null()) {
            report.warn(TranslateWarning::DroppedProperty {
                path: format!("KN 顶层键 `{key}`(Kitten4 无对应字段)"),
            });
        }
    }
    Value::Object(doc)
}

/// 一个 KN 实体的公共字段(角色/场景共用):名字、造型、工作区滚动、可见性
fn kitten4_entity_common(
    source: &serde_json::Map<String, serde_json::Value>,
    target: &mut serde_json::Map<String, serde_json::Value>,
    report: &mut TranslateReport,
    scope: &str,
) {
    use serde_json::{Value, json};
    if let Some(name) = source.get("name") {
        target.insert("name".into(), name.clone());
    }
    if let Some(current) = source.get("currentStyleId") {
        target.insert("current_style_id".into(), current.clone());
    }
    if let Some(offset) = source.get("workspaceScrollXy") {
        let (x, y) = (
            offset.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            offset.get("y").and_then(Value::as_f64).unwrap_or(0.0),
        );
        target.insert(
            "workspace_offset".into(),
            json!({ "x": num(x), "y": num(y) }),
        );
    }
    target.insert(
        "visible".into(),
        source.get("visible").cloned().unwrap_or(Value::Bool(true)),
    );
    target.insert("draggable".into(), json!(false));
    target.insert("rotation_type".into(), json!(0));
    // KN 实体有、Kitten4 实体没有的键:逐条报告(报告按 `path` 聚合,不会淹)
    for key in ["comments", "deletable", "editable"] {
        if source.get(key).is_some_and(|value| !value.is_null()) {
            report.warn(TranslateWarning::DroppedProperty {
                path: format!("{scope}[*].{key}(Kitten4 实体没有这个键)"),
            });
        }
    }
}

/// KN 角色 → Kitten4 角色条目
fn kitten4_actor(
    source: &serde_json::Map<String, serde_json::Value>,
    blocks: serde_json::Value,
    scene: Option<String>,
    landscape: bool,
    report: &mut TranslateReport,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::{Value, json};
    let mut out = serde_json::Map::new();
    out.insert(
        "id".into(),
        source.get("id").cloned().unwrap_or(Value::Null),
    );
    kitten4_entity_common(source, &mut out, report, "actors");
    // 正向把 `lock` 改叫 `locked`,坐标乘 `10/13`(横屏)
    if let Some(locked) = source.get("locked") {
        out.insert("lock".into(), locked.clone());
    } else if let Some(lock) = source.get("lock") {
        out.insert("lock".into(), lock.clone());
    }
    // 没有 groups 概念可复原:正向在"源无 `theatre.groups`"时取反,这里按同样规则取反回去
    let rotation = source
        .get("rotation")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if rotation != 0.0 && rotation.is_finite() {
        out.insert("rotation".into(), num(-rotation));
        report.warn(TranslateWarning::DroppedProperty {
            path: "actors[*].rotation(KN 没有分组表,按正向的无 groups 分支取反)".into(),
        });
    } else {
        out.insert("rotation".into(), num(rotation));
    }
    let (x, y) = match source.get("position") {
        Some(position) => {
            let factor = if landscape {
                LANDSCAPE_POSITION_BACK_SCALE
            } else {
                1.0
            };
            (
                position.get("x").and_then(Value::as_f64).unwrap_or(0.0) / factor,
                position.get("y").and_then(Value::as_f64).unwrap_or(0.0) / factor,
            )
        }
        None => (
            source.get("x").and_then(Value::as_f64).unwrap_or(0.0),
            source.get("y").and_then(Value::as_f64).unwrap_or(0.0),
        ),
    };
    out.insert("x".into(), num(x));
    out.insert("y".into(), num(y));
    out.insert(
        "scale".into(),
        source.get("scale").cloned().unwrap_or_else(|| json!(100)),
    );
    if let Some(scene) = scene {
        out.insert("scene".into(), Value::String(scene));
    }
    // KN 角色自带 `styles`(与 Kitten4 同名同义);缺失时退到 `currentStyleId`
    let styles = match source.get("styles") {
        Some(Value::Array(styles)) => Value::Array(styles.clone()),
        _ => Value::Array(source.get("currentStyleId").cloned().into_iter().collect()),
    };
    out.insert("styles".into(), styles);
    out.insert("block_data_json".into(), blocks);
    // 正向会删掉这两个键(官方行为),反向只能给保守默认值
    out.insert("user_change_r_c".into(), json!(false));
    out.insert("editable_in_tuition_mode".into(), json!(false));
    out
}

/// KN 场景 → Kitten4 场景条目
fn kitten4_scene(
    source: &serde_json::Map<String, serde_json::Value>,
    blocks: serde_json::Value,
    report: &mut TranslateReport,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::{Value, json};
    let mut out = serde_json::Map::new();
    out.insert(
        "id".into(),
        source.get("id").cloned().unwrap_or(Value::Null),
    );
    kitten4_entity_common(source, &mut out, report, "scenes");
    out.insert("name".into(), json!(KITTEN4_SCENE_NAME));
    if let Some(screen_name) = source.get("screenName") {
        out.insert("screen_name".into(), screen_name.clone());
    }
    // 正向:有 groups 时 actorIds 由 group_order 展开,无 groups 时照抄 `scene.actors`;
    // 反向没有 groups 概念,写回 `actors` 即可(再正向时 `actorIds = scene.actors`)。
    out.insert(
        "actors".into(),
        source.get("actorIds").cloned().unwrap_or_else(|| json!([])),
    );
    out.insert("group_order".into(), json!([]));
    out.insert(
        "styles".into(),
        source.get("styles").cloned().unwrap_or_else(|| json!([])),
    );
    for key in ["x", "y", "scale", "rotation"] {
        out.insert(
            key.into(),
            source.get(key).cloned().unwrap_or_else(|| match key {
                "scale" => json!(100),
                _ => json!(0),
            }),
        );
    }
    out.insert("block_data_json".into(), blocks);
    out
}

/// KN 造型表 → Kitten4 造型表(`centerPoint` 还原成 `rotate_center`,其余保真)
fn kitten4_styles(
    src: &serde_json::Map<String, serde_json::Value>,
    _report: &mut TranslateReport,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::Value;
    let mut out = serde_json::Map::new();
    let styles = src
        .get("styles")
        .and_then(Value::as_object)
        .and_then(|outer| outer.get("stylesDict"))
        .and_then(Value::as_object);
    let Some(styles) = styles else { return out };
    for (id, style) in styles {
        let mut entry = style.as_object().cloned().unwrap_or_default();
        if let Some(point) = entry.remove("centerPoint") {
            entry.insert("rotate_center".into(), point);
        }
        out.insert(id.clone(), Value::Object(entry));
    }
    out
}

/// KN 变量表 → Kitten4 `variables` + `variable_order`(云变量/本地变量在 KN 里已合并,无法还原)
fn kitten4_variables(
    src: &serde_json::Map<String, serde_json::Value>,
    landscape: bool,
    canvas: (f64, f64),
    kn_stage: (f64, f64),
    report: &mut TranslateReport,
) -> (
    serde_json::Map<String, serde_json::Value>,
    Vec<serde_json::Value>,
) {
    use serde_json::{Value, json};
    let mut out = serde_json::Map::new();
    let mut order = Vec::new();
    let dict = src
        .get("variables")
        .and_then(Value::as_object)
        .and_then(|outer| outer.get("variablesDict"))
        .and_then(Value::as_object);
    let Some(dict) = dict else {
        return (out, order);
    };
    for (id, var) in dict {
        let mut entry = serde_json::Map::new();
        entry.insert(
            "id".into(),
            var.get("id").cloned().unwrap_or_else(|| json!(id)),
        );
        for key in ["type", "name", "value", "visible", "scale"] {
            if let Some(value) = var.get(key) {
                entry.insert(key.into(), value.clone());
            }
        }
        if let Some(position) = var.get("position") {
            let (x, y) = kitten4_position(position, canvas, kn_stage);
            entry.insert("position".into(), json!({ "x": num(x), "y": num(y) }));
        }
        entry.insert("offset".into(), json!({ "x": 0, "y": 0 }));
        if let Some(global) = var.get("isGlobal") {
            entry.insert("is_global".into(), global.clone());
        }
        if let Some(create_time) = var.get("createTime") {
            entry.insert("create_time".into(), create_time.clone());
        }
        if let Some(style) = var.get("style").and_then(Value::as_str) {
            entry.insert("theme".into(), json!(theme_of_style(style)));
        }
        if let Some(entity) = var.get("currentEntityId") {
            entry.insert("current_entity".into(), entity.clone());
        }
        out.insert(id.clone(), Value::Object(entry));
        order.push(json!(id));
    }
    let _ = landscape;
    if !out.is_empty() {
        report.warn(TranslateWarning::DroppedProperty {
            path: "variables(云变量与本地变量在 KN 里已合并,反向一律写入 `variables`)".into(),
        });
    }
    (out, order)
}

/// KN 音频表 → Kitten4 `audio` + `audio_order`
fn kitten4_audio(
    src: &serde_json::Map<String, serde_json::Value>,
) -> (
    serde_json::Map<String, serde_json::Value>,
    Vec<serde_json::Value>,
) {
    use serde_json::Value;
    let mut out = serde_json::Map::new();
    let audio = src
        .get("audios")
        .and_then(Value::as_object)
        .and_then(|outer| outer.get("audiosDict"))
        .and_then(Value::as_object);
    let order = src
        .get("audios")
        .and_then(Value::as_object)
        .and_then(|outer| outer.get("sortList"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(audio) = audio {
        for (id, entry) in audio {
            out.insert(id.clone(), entry.clone());
        }
    }
    (out, order)
}

/// KN `broadcasts.broadcastsDict` → Kitten4 裸字典(脏键 `toJSON` 丢掉并报告)
fn kitten4_broadcasts(
    src: &serde_json::Map<String, serde_json::Value>,
    report: &mut TranslateReport,
) -> serde_json::Value {
    use serde_json::Value;
    let raw = src
        .get("broadcasts")
        .and_then(Value::as_object)
        .and_then(|outer| outer.get("broadcastsDict"))
        .cloned();
    match raw {
        Some(Value::Object(mut map)) => {
            if map.remove("toJSON").is_some() {
                report.warn(TranslateWarning::DroppedField {
                    path: "broadcasts.toJSON(脏键,真实 KN 作品里存在)".into(),
                });
            }
            Value::Object(map)
        }
        _ => Value::Object(serde_json::Map::new()),
    }
}

/// KN 位置(左上原点像素系)→ Kitten 位置(中心原点):正向 `finish::stage_position` 的逆
fn kitten4_position(
    position: &serde_json::Value,
    canvas: (f64, f64),
    kn_stage: (f64, f64),
) -> (f64, f64) {
    let x = position
        .get("x")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let y = position
        .get("y")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    let (w, h) = (canvas.0.max(1.0), canvas.1.max(1.0));
    let (sw, sh) = (kn_stage.0.max(1.0), kn_stage.1.max(1.0));
    (x * w / sw - w / 2.0, h / 2.0 - y * h / sh)
}

