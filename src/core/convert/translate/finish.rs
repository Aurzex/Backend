//! KN 文档装配:官方 `De`(webpack module 68602,`temp/ref/handler_de.pretty.js`)的移植。
//!
//! 输入是流水线的两半:实体的**源字段**(`ConvertedEntity::source`,阶段 1 之后仍带
//! `x/y/scale/lock/current_style_id/workspace_offset/…` 的实体对象)+ 已转换好的
//! `nekoBlockJsonList`(`ConvertedEntity::blocks`)+ 程序集条目([`ProcedureEntry`]);
//! 输出是能过官方 `validateBcm` 的 `.bcmkn` 对象(顶层键与真机 `.bcmkn` 一致)。
//!
//! 官方锚点(行号 = `temp/ref/handler_de.pretty.js`,即官方 module 68602):
//!
//! - 画布/坐标:`N = size.width > size.height`(241);变量坐标换算 `O`(352-381)按
//!   `{ x:(x+w/2)*W/w, y:(h/2-y)*H/h }` 把源舞台坐标折算到目标舞台 → [`stage_position`]
//! - 角色 `mapValues(theatre.actors, …)`(247-272)→ [`actor_entry`]:删 `block_data_json`/
//!   `user_change_r_c`/`editable_in_tuition_mode`;名字净化 `jJ`(250)、空名兜底 `"1"`(`yK` 251)、
//!   唯一化 `mq`(252);`current_style_id→currentStyleId`(256);横屏 `position = {x*10/13,…}`
//!   并删 `x`/`y`(257-261);`lock→locked`(262);无 `theatre.groups` 时 `rotation` 取反(263);
//!   `scale` 取整(264);`workspaceScrollXy` 兜底 `{100,50}`(265)
//! - 场景 `mapValues(theatre.scenes, …)`(275-298)→ [`scene_entry`]:`name` 恒「背景」+
//!   `screenName` 唯一化(276-279);`actorIds` = 无 `groups` 时的 `scene.actors`,再加
//!   `group_order` 展开的 `groups[gid].actors`,随后删 `group_order`(280-289);
//!   `currentStyleId`(292);`workspaceScrollXy`(293)
//! - `scenes.sortList`/`currentSceneId`(300-301);音频 `ye(n.audio, …)`(304-343,`sortList`/
//!   `currentAudioId`);造型 `stylesDict`(351,上传逻辑 20-210);变量 `B`(383-535,普通变量
//!   400-430 → [`local_variable`]、云变量 432-470 → [`cloud_variable`]);`stageSize`(547)、
//!   `projectName`(548)
//!
//! ## 离线近似(刻意差异)
//!
//! 1. **造型/音频不再上传**:官方对造型 `fetch → blob`(横屏还按 `10/13` 重采样)再
//!    `ServiceApi.uploadUserFiles` 换新 URL,音频同理。本库不联网、不上传,**源 URL 原样保留**,
//!    并把"官方会重传"的条目记 [`TranslateWarning::DroppedProperty`](判据:造型取
//!    `cdn_url ?? url` 后不是 `https://`,音频没有 `cdn_url`)。官方 `gU(url)`(判断是否站内资源)
//!    未反编译,portrait 分支"已是站内资源就跳过 fetch"这半边只能按 `https://` 近似;横屏分支
//!    官方对**所有**造型都重采样上传(连 `https://` 也换),这里不额外报。
//! 2. **`toolMode`/`isHideStage` 不移植**:官方从活编辑器实例读
//!    (`f.BcmInstance.getBcm().toolMode`),转换器没有编辑器状态;真机 `.bcmkn` 里它们由编辑器
//!    保存时补,`validateBcm` 不要求。
//! 3. **`blockedActors` 不写**:官方只在模板自带该键时才写(`t.blockedActors && …`),我们的
//!    模板没有(角色 `lock→locked` 仍然生效)。
//! 4. **`source` 字段不在这里**:官方装配完另把原始 `.bcm4` 上传、把 URL 写到 `w.source`(§3.1),
//!    属于编排层的"保留原件"选项,不属于 `De`。
//! 5. 缺 `theatre.scenes_order` 时官方会写出 `undefined`(产物非法);本实现退化为实体输入顺序。
//! 6. 名字净化按官方正则的**字符集语义**(白名单外字符 + `<>&."` 一律删)→ [`sanitize`]/
//!    [`truncate_width`](>255 的字符算 2 宽,上限 40 = 官方 `o.dT`);JS `\s` 与
//!    `char::is_whitespace` 的空白集合略有差异(只用于"整体是否空白")。
//! 7. 变量/云变量的 `position` 一律按官方公式重算;源侧没有的键(`current_entity`/`create_time`
//!    缺失等)按官方 `undefined` 语义**不落键**(`createTime` 有 `Date.now()` 兜底,必落)。
//! 8. 畸形数字(`rotation`/`scale`/坐标不是有限数)官方会算出 `NaN`(JSON 里成 `null`),
//!    本实现按 0 处理或跳过改键,产物更稳。

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use crate::core::convert::shared::{DecompilerError, Result};

use super::model::type_name;
use super::mapping::truthy;
use super::neko::{ProcedureEntry, procedures_to_json};
use super::{ TranslateReport, TranslateWarning};
use super::tables_gen::{BCM_VERSION, STAGE_LANDSCAPE, STAGE_PORTRAIT};

/// 官方 `A.W$`:`workspaceScrollXy` 的兜底值(CDN 模板里恰好是 `{100,30}`,**别**把模板值当兜底)
const DEFAULT_WORKSPACE_SCROLL: (f64, f64) = (100.0, 50.0);
/// 场景实体名恒为「背景」(官方 78653)
const SCENE_NAME: &str = "背景";
/// 净化后为空的名字的兜底(官方 `yK` 判空后写 `"1"`)
const FALLBACK_NAME: &str = "1";
/// 名字显示宽度上限(官方 `mq` 里的 `o.dT = 40`)
const NAME_MAX_WIDTH: usize = 40;
/// `project_name` 缺省(官方 `|| "空白作品"`)
const DEFAULT_PROJECT_NAME: &str = "空白作品";
/// 官方横屏下实体坐标的缩放系数(把 Kitten 的 1.3 倍放大还原)
const LANDSCAPE_POSITION_SCALE: f64 = 10.0 / 13.0;
/// 变量样式图标(官方 `EVariableStyle` 的字符串值)
const VAR_STYLE_DEFAULT: &str = "default";
const VAR_STYLE_TEXT: &str = "text";
const VAR_STYLE_MEDAL: &str = "icon_medal";
const VAR_STYLE_HEART: &str = "icon_heart";
const VAR_STYLE_HOURGLASS: &str = "icon_hourglass";
const VAR_STYLE_COIN: &str = "icon_coin";

/// 一个已转换完成的实体(角色/场景)
#[derive(Debug, Clone)]
pub(crate) struct ConvertedEntity {
    /// 源实体 id(同时也是目标字典的键)
    pub source_id: String,
    /// 场景还是角色(决定走 `scene_entry` 还是 `actor_entry`)
    pub is_scene: bool,
    /// 已转换好的 `nekoBlockJsonList`
    pub blocks: Vec<Value>,
    /// 阶段 1 之后的源实体对象(含 `x/y/scale/lock/current_style_id/workspace_offset/…`)
    pub source: Map<String, Value>,
}

/// 装配 KN 作品文档(官方 `De`)
///
/// `source` 是项目根 JSON(Kitten4 编辑版,或已经过 `mapping`/`neko` 处理的阶段 1 产物——
/// 两种输入都能吃:`broadcasts` 已包成 `{broadcastsDict}` 时原样透传,否则只包一层)。
pub(crate) fn build_document(
    source: &Value,
    entities: Vec<ConvertedEntity>,
    procedures: &[ProcedureEntry],
    now_ms: u128,
    report: &mut TranslateReport,
) -> Result<Value> {
    let src = source
        .as_object()
        .ok_or_else(|| DecompilerError::TypeMismatch {
            expected: "object(Kitten 作品 JSON)".into(),
            actual: type_name(source).into(),
        })?;
    let theatre = src.get("theatre").and_then(Value::as_object);

    // 源画布:官方 `p = size?.width ?? width ?? 562` / `g = size?.height ?? height ?? 900`
    let (src_w, src_h) = source_stage_size(src);
    let landscape = src_w > src_h;
    let groups = theatre
        .and_then(|t| t.get("groups"))
        .and_then(Value::as_object);

    let mut actor_used: Vec<String> = Vec::new();
    let mut scene_used: Vec<String> = Vec::new();
    let mut actors = Map::new();
    let mut scenes = Map::new();
    for entity in entities {
        let mut value = entity.source.clone();
        value.remove("block_data_json");
        value.insert("nekoBlockJsonList".into(), Value::Array(entity.blocks));
        if entity.is_scene {
            scene_entry(&mut value, groups, &mut scene_used);
            scenes.insert(entity.source_id, Value::Object(value));
        } else {
            actor_entry(&mut value, groups, &mut actor_used, landscape);
            actors.insert(entity.source_id, Value::Object(value));
        }
    }

    // scenes.sortList / currentSceneId(官方 78693)
    let scenes_order: Vec<Value> = match theatre
        .and_then(|t| t.get("scenes_order"))
        .and_then(Value::as_array)
    {
        Some(order) => order.clone(),
        None => scenes.keys().map(|k| Value::String(k.clone())).collect(),
    };
    let current_scene = scenes_order
        .first()
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));

    let mut doc = Map::new();
    doc.insert("projectName".into(), json!(project_name(src)));
    doc.insert(
        "scenes".into(),
        json!({ "scenesDict": scenes, "currentSceneId": current_scene, "sortList": scenes_order }),
    );
    doc.insert(
        "styles".into(),
        json!({ "stylesDict": build_styles(theatre, landscape, report) }),
    );
    doc.insert(
        "variables".into(),
        json!({ "variablesDict": build_variables(src, src_w, src_h, landscape, now_ms) }),
    );
    doc.insert("broadcasts".into(), build_broadcasts(src));
    doc.insert("actors".into(), json!({ "actorsDict": actors }));
    // audios:字典 + `sortList`(官方 `audio_order` 为真值时照抄,否则按遍历顺序)+ 首个 currentAudioId
    let (audios_dict, audios_traversed) = build_audios(src, report);
    let audio_order = match src.get("audio_order") {
        Some(Value::Array(order)) => order.clone(),
        _ => audios_traversed,
    };
    let current_audio = audio_order
        .first()
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    doc.insert(
        "audios".into(),
        json!({ "audiosDict": audios_dict, "sortList": audio_order, "currentAudioId": current_audio }),
    );
    doc.insert(
        "procedures".into(),
        json!({ "proceduresDict": procedures_to_json(procedures)? }),
    );
    doc.insert("stageSize".into(), stage_size(landscape));
    doc.insert("version".into(), json!(BCM_VERSION));
    doc.insert("toolType".into(), json!("KN"));
    doc.insert("previewUrl".into(), json!(""));
    doc.insert("resourceZip".into(), json!(""));
    doc.insert("guideUrl".into(), json!(""));
    doc.insert("textToBlock".into(), json!([]));
    doc.insert("aiImageUrls".into(), json!([]));
    doc.insert(
        "hidden_toolbox".into(),
        json!({ "toolbox": [], "blocks": [] }),
    );
    doc.insert("courseMaterials".into(), json!([]));
    Ok(Value::Object(doc))
}

// ---------------------------------------------------------------- 实体(官方 78620-78680)

/// 角色条目(官方 247-272 `mapValues(theatre.actors, …)`)
fn actor_entry(
    value: &mut Map<String, Value>,
    groups: Option<&Map<String, Value>>,
    used: &mut Vec<String>,
    landscape: bool,
) {
    value.remove("user_change_r_c");
    value.remove("editable_in_tuition_mode");

    // 名字:净化 → 空名兜底 → 唯一化
    let mut name = sanitize(&text(value.get("name")));
    if is_bad_name(&name) {
        name = FALLBACK_NAME.to_string();
    }
    let name = uniquify(used, &name, NAME_MAX_WIDTH);
    used.push(name.clone());
    value.insert("name".into(), Value::String(name));

    if truthy(value.get("current_style_id"))
        && let Some(style) = value.remove("current_style_id")
    {
        value.insert("currentStyleId".into(), style);
    }

    // 两个坐标都在时才换算(官方 257-261 `isNil(x) || isNil(y) || (…)`)
    if !is_nil(value.get("x")) && !is_nil(value.get("y")) {
        let x = number(value.get("x"));
        let y = number(value.get("y"));
        let position = if landscape {
            json!({ "x": num(x * LANDSCAPE_POSITION_SCALE), "y": num(y * LANDSCAPE_POSITION_SCALE) })
        } else {
            json!({ "x": num(x), "y": num(y) })
        };
        value.remove("x");
        value.remove("y");
        value.insert("position".into(), position);
    }

    // 官方 262 只在 `lock` 为真值时换成 `locked`;假值原样留在产物里(不额外造 `locked:false`)
    if truthy(value.get("lock"))
        && let Some(lock) = value.remove("lock")
    {
        value.insert("locked".into(), lock);
    }

    // 没有分组表时旋转取反(官方 263)
    if groups.is_none() {
        let rotation = number(value.get("rotation"));
        if rotation != 0.0 && rotation.is_finite() {
            value.insert("rotation".into(), num(-rotation));
        }
    }

    if !is_nil(value.get("scale")) {
        value.insert("scale".into(), num(number(value.get("scale")).floor()));
    }

    value.insert("workspaceScrollXy".into(), workspace_scroll(value));
}

/// 场景条目(官方 275-298 `mapValues(theatre.scenes, …)`)
fn scene_entry(
    value: &mut Map<String, Value>,
    groups: Option<&Map<String, Value>>,
    used: &mut Vec<String>,
) {
    // screenName 唯一化(作用域是"场景列表",与角色名互不影响)
    let screen_name = uniquify(used, &text(value.get("screen_name")), NAME_MAX_WIDTH);
    used.push(screen_name.clone());
    value.insert("screenName".into(), Value::String(screen_name));
    value.insert("name".into(), Value::String(SCENE_NAME.to_string()));

    let mut actor_ids: Vec<Value> = Vec::new();
    if groups.is_none()
        && let Some(list) = value
            .get("actors")
            .and_then(Value::as_array)
            .filter(|a| !a.is_empty())
    {
        actor_ids = list.clone();
    }
    // 官方 280-289:先照抄 `scene.actors`(无 groups 时),再按 `group_order` 追加;先克隆出来免得借用撞上 `remove`
    let group_order = value.get("group_order").and_then(Value::as_array).cloned();
    if let Some(order) = group_order.filter(|order| !order.is_empty()) {
        for group_id in order.iter().filter_map(Value::as_str) {
            let group_actors = groups
                .and_then(|g| g.get(group_id))
                .and_then(Value::as_object)
                .and_then(|g| g.get("actors"))
                .and_then(Value::as_array);
            if let Some(list) = group_actors {
                actor_ids.extend(list.iter().cloned());
            }
        }
        value.remove("group_order");
    }
    value.insert("actorIds".into(), Value::Array(actor_ids));

    if truthy(value.get("current_style_id"))
        && let Some(style) = value.remove("current_style_id")
    {
        value.insert("currentStyleId".into(), style);
    }
    value.insert("workspaceScrollXy".into(), workspace_scroll(value));
}

// ---------------------------------------------------------------- 资源字典(官方 78690-78730)

/// 造型字典(官方 351,上传逻辑 20-210):归一 `centerPoint`,URL 原样保留,必要时报"官方会重传"
fn build_styles(
    theatre: Option<&Map<String, Value>>,
    landscape: bool,
    report: &mut TranslateReport,
) -> Value {
    let mut out = Map::new();
    let Some(styles) = theatre
        .and_then(|t| t.get("styles"))
        .and_then(Value::as_object)
    else {
        return Value::Object(out);
    };
    for (id, style) in styles {
        let mut entry = style.as_object().cloned().unwrap_or_default();
        // center_point / rotate_center → centerPoint(官方顺序:后者覆盖前者)
        if let Some(point) = entry.remove("center_point") {
            entry.insert("centerPoint".into(), point);
        }
        if let Some(point) = entry.remove("rotate_center") {
            entry.insert("centerPoint".into(), point);
        }
        // 官方在没有 url(或竖屏有 cdn_url)时把 cdn_url 写进 url;横屏分支不动 url
        let has_url = truthy(entry.get("url"));
        let cdn = entry.get("cdn_url").filter(|v| truthy(Some(*v))).cloned();
        if (!has_url || !landscape)
            && let Some(cdn) = cdn
        {
            entry.insert("url".into(), cdn);
        }
        let effective = entry.get("url").and_then(Value::as_str).unwrap_or_default();
        if !effective.is_empty() && !effective.starts_with("https://") {
            report.warn(TranslateWarning::ReuploadedOnImport {
                path: "theatre.styles[*].url(官方会重新上传并替换;我们保留源 url/cdn_url)".into(),
            });
        }
        out.insert(id.clone(), Value::Object(entry));
    }
    Value::Object(out)
}

/// 音频字典(官方 304-343):返回(字典, 遍历顺序——`sortList` 的兜底)
fn build_audios(
    src: &Map<String, Value>,
    report: &mut TranslateReport,
) -> (Map<String, Value>, Vec<Value>) {
    let audios = dict_at(src, "audio").or_else(|| {
        src.get("theatre")
            .and_then(Value::as_object)
            .and_then(|t| dict_at(t, "audio"))
    });
    let mut dict = Map::new();
    let mut traversed: Vec<Value> = Vec::new();
    if let Some(audios) = audios {
        for (id, audio) in audios {
            let mut entry = audio.as_object().cloned().unwrap_or_default();
            match entry.get("cdn_url").filter(|v| truthy(Some(*v))).cloned() {
                Some(cdn) => {
                    entry.insert("url".into(), cdn);
                }
                None => report.warn(TranslateWarning::ReuploadedOnImport {
                    path: "audio[*].url(官方会重新上传;我们保留源键)".into(),
                }),
            }
            traversed.push(Value::String(id.clone()));
            dict.insert(id.clone(), Value::Object(entry));
        }
    }
    (dict, traversed)
}

/// 变量字典(官方 383-535):`variables` + `cloud_variables`
fn build_variables(
    src: &Map<String, Value>,
    src_w: f64,
    src_h: f64,
    landscape: bool,
    now_ms: u128,
) -> Value {
    let mut dict = Map::new();
    for (key, entry) in variable_entries(src, "variables") {
        let Some(var) = entry.as_object() else {
            continue;
        };
        dict.insert(
            key,
            Value::Object(local_variable(var, src_w, src_h, landscape, now_ms)),
        );
    }
    // 云变量合并(官方 `{...n, ...cloud}`:键冲突时云变量覆盖)
    for (key, entry) in variable_entries(src, "cloud_variables") {
        let Some(var) = entry.as_object() else {
            continue;
        };
        dict.insert(
            key,
            Value::Object(cloud_variable(var, src_w, src_h, landscape, now_ms)),
        );
    }
    Value::Object(dict)
}

/// 普通变量条目(官方 400-430)
fn local_variable(
    var: &Map<String, Value>,
    src_w: f64,
    src_h: f64,
    landscape: bool,
    now_ms: u128,
) -> Map<String, Value> {
    let mut out = Map::new();
    for key in ["id", "type", "name", "value", "visible", "scale"] {
        if let Some(value) = var.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    out.insert(
        "position".into(),
        stage_position(var.get("position"), src_w, src_h, landscape),
    );
    if let Some(global) = var.get("is_global") {
        out.insert("isGlobal".into(), global.clone());
    }
    out.insert("createTime".into(), create_time(var, now_ms));
    out.insert(
        "style".into(),
        Value::String(theme_style(var.get("theme")).to_string()),
    );
    if let Some(entity) = var.get("current_entity") {
        out.insert("currentEntityId".into(), entity.clone());
    }
    out
}

/// 云变量条目(官方 432-470):一律 `isGlobal`,初值 `[]`(列表)/`0`(其余)
fn cloud_variable(
    var: &Map<String, Value>,
    src_w: f64,
    src_h: f64,
    landscape: bool,
    now_ms: u128,
) -> Map<String, Value> {
    let src_type = var.get("type").and_then(Value::as_str).unwrap_or_default();
    let kind = match src_type {
        "private" => "any",
        "public_list" => "list",
        other => other,
    };
    let value = if src_type == "public_list" {
        json!([])
    } else {
        json!(0)
    };

    let mut out = Map::new();
    for key in ["id", "name", "visible", "scale"] {
        if let Some(v) = var.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    out.insert("type".into(), Value::String(kind.to_string()));
    out.insert("value".into(), value);
    out.insert(
        "position".into(),
        stage_position(var.get("position"), src_w, src_h, landscape),
    );
    out.insert("isGlobal".into(), json!(true));
    out.insert("createTime".into(), create_time(var, now_ms));
    out.insert("style".into(), Value::String(VAR_STYLE_DEFAULT.to_string()));
    if let Some(entity) = var.get("current_entity") {
        out.insert("currentEntityId".into(), entity.clone());
    }
    out
}

// ---------------------------------------------------------------- 小工具

/// 源舞台尺寸:官方 `size?.width ?? width ?? 562` / `size?.height ?? height ?? 900`
fn source_stage_size(src: &Map<String, Value>) -> (f64, f64) {
    let size = src.get("size").and_then(Value::as_object);
    let pick = |key: &str, fallback: f64| -> f64 {
        size.and_then(|s| s.get(key))
            .or_else(|| src.get(key))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && *v > 0.0)
            .unwrap_or(fallback)
    };
    (
        pick("width", STAGE_PORTRAIT.0),
        pick("height", STAGE_PORTRAIT.1),
    )
}

/// 目标画布(官方 547:横屏 `{900,562}`,竖屏 `{562,900}`)
fn stage_size(landscape: bool) -> Value {
    let (w, h) = if landscape {
        STAGE_LANDSCAPE
    } else {
        STAGE_PORTRAIT
    };
    json!({ "width": num(w), "height": num(h) })
}

/// 官方 `projectName || "空白作品"`:键名随方向不同(正向 `project_name`,反向 `projectName`)
pub(crate) fn project_name_at(src: &Map<String, Value>, key: &str) -> String {
    match src.get(key) {
        Some(Value::String(name)) if !name.is_empty() => name.clone(),
        _ => DEFAULT_PROJECT_NAME.to_string(),
    }
}

/// 正向:`project_name` 字段
fn project_name(src: &Map<String, Value>) -> String {
    project_name_at(src, "project_name")
}

/// `broadcasts`:源是裸字典时包一层 `{broadcastsDict: …}`,已包好则透传(官方阶段 1 包过一次)
fn build_broadcasts(src: &Map<String, Value>) -> Value {
    let raw = src.get("broadcasts").cloned().unwrap_or_else(|| json!({}));
    match raw.as_object() {
        Some(obj) if obj.contains_key("broadcastsDict") => Value::Object(obj.clone()),
        Some(_) => json!({ "broadcastsDict": raw }),
        None => json!({ "broadcastsDict": {} }),
    }
}

/// 变量表:优先顶层(`variables`/`cloud_variables`),退化到 `theatre.*`;数组输入按条目 `id` 做键
fn variable_entries(src: &Map<String, Value>, key: &str) -> Vec<(String, Value)> {
    let value = src.get(key).or_else(|| {
        src.get("theatre")
            .and_then(Value::as_object)
            .and_then(|t| t.get(key))
    });
    match value {
        Some(Value::Object(dict)) => {
            let inner = dict.get(key).and_then(Value::as_object);
            match inner {
                // 已是 `{variablesDict: {...}}` 形态(阶段 1 之后再次装配)
                Some(nested) => nested.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                None => dict.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            }
        }
        Some(Value::Array(list)) => list
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let key = item
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| index.to_string());
                (key, item.clone())
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// 变量坐标换算(官方 352-381 `O` 内联的 `b`):源舞台中心系 → 目标舞台左上原点像素系
fn stage_position(pos: Option<&Value>, src_w: f64, src_h: f64, landscape: bool) -> Value {
    let x = pos
        .and_then(|p| p.get("x"))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);
    let y = pos
        .and_then(|p| p.get("y"))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);
    let (target_w, target_h) = if landscape {
        STAGE_LANDSCAPE
    } else {
        STAGE_PORTRAIT
    };
    json!({
        "x": num((x + src_w / 2.0) * target_w / src_w),
        "y": num((src_h / 2.0 - y) * target_h / src_h),
    })
}

/// `theme` → 变量样式图标(官方 395-411 `switch (e.theme)`)
fn theme_style(theme: Option<&Value>) -> &'static str {
    match theme.and_then(Value::as_str) {
        Some("score") => VAR_STYLE_MEDAL,
        Some("HP") => VAR_STYLE_HEART,
        Some("clock") => VAR_STYLE_HOURGLASS,
        Some("coin") => VAR_STYLE_COIN,
        Some("pure") => VAR_STYLE_TEXT,
        _ => VAR_STYLE_DEFAULT,
    }
}

/// `create_time || Date.now()`;`now_ms` 由门面传入(确定性模式下传 0,保证两次转换逐字节一致)
fn create_time(var: &Map<String, Value>, now_ms: u128) -> Value {
    match var.get("create_time").filter(|v| truthy(Some(*v))) {
        Some(value) => value.clone(),
        None => num(now_ms as f64),
    }
}

/// 当前毫秒时间戳(非确定性模式的 `Date.now()` 等价物)
pub(crate) fn current_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// `workspaceScrollXy = {x: workspace_offset?.x || 100, y: workspace_offset?.y || 50}`
fn workspace_scroll(value: &Map<String, Value>) -> Value {
    let offset = value.get("workspace_offset").and_then(Value::as_object);
    let pick = |key: &str, fallback: f64| -> Value {
        match offset.and_then(|o| o.get(key)).filter(|v| truthy(Some(*v))) {
            Some(value) => match value.as_f64() {
                Some(n) if n.is_finite() => num(n),
                _ => num(fallback),
            },
            None => num(fallback),
        }
    };
    json!({ "x": pick("x", DEFAULT_WORKSPACE_SCROLL.0), "y": pick("y", DEFAULT_WORKSPACE_SCROLL.1) })
}

/// 官方 `jJ`(module 11937 的 `F`):删掉白名单外的字符 + `<>&."` 六个字符(250 行调用)
fn sanitize(name: &str) -> String {
    name.chars().filter(|c| is_name_char(*c)).collect()
}

/// 官方白名单里额外放行的中文标点(其余按码点区间判断)
const EXTRA_NAME_CHARS: [char; 25] = [
    '！', '￥', '…', '（', '）', '—', '「', '」', '『', '』', '【', '】', '’', '‘', '”', '“', '；',
    '：', '《', '》', '？', '、', '。', '，', '·',
];

/// 官方正则 `/[^\x20-\x7e\u00C0-\u00FF\u4e00-\u9fa5 <中文标点>]|[<>&."]/` 的字符集语义
fn is_name_char(c: char) -> bool {
    if matches!(c, '<' | '>' | '&' | '.' | '"') {
        return false;
    }
    matches!(c as u32, 0x20..=0x7e | 0xc0..=0xff | 0x4e00..=0x9fa5) || EXTRA_NAME_CHARS.contains(&c)
}

/// 官方 `yK`(`G`):空或纯空白
fn is_bad_name(name: &str) -> bool {
    name.is_empty() || name.chars().all(char::is_whitespace)
}

/// 官方 `W` + `o.dT = 40`(module 35486):按显示宽度截断(`char_code > 255` 记 2 宽)
fn truncate_width(text: &str, limit: usize) -> String {
    let mut width = 0usize;
    let mut out = String::new();
    for c in text.chars() {
        width += if (c as u32) > 255 { 2 } else { 1 };
        out.push(c);
        if width >= limit {
            return out;
        }
    }
    text.to_string()
}

/// 官方 `mq`(`J`):净化 + 截断 + 去重;净化后为空 → 空串(调用方自己补 `"1"`)
fn uniquify(used: &[String], base: &str, limit: usize) -> String {
    let cleaned = sanitize(base);
    if cleaned.is_empty() {
        return String::new();
    }
    let candidate = truncate_width(&cleaned, limit);
    if used.contains(&candidate) {
        increment_until_free(used, &candidate)
    } else {
        candidate
    }
}

/// 官方 `X`:尾随数字 +1,仍冲突就继续(`小明`→`小明1`,`小明9`→`小明10`)
fn increment_until_free(used: &[String], base: &str) -> String {
    let mut current = increment_suffix(base);
    while used.contains(&current) {
        current = increment_suffix(&current);
    }
    current
}

/// 官方 `X` 里的 `t.match(/^(.*?)(\d+)?$/)` + 字符串加法
fn increment_suffix(text: &str) -> String {
    let split = text.len() - text.chars().rev().take_while(char::is_ascii_digit).count();
    let (prefix, digits) = text.split_at(split);
    let digits = if digits.is_empty() { "0" } else { digits };
    format!("{prefix}{}", increment_digits(digits))
}

/// 十进制定长字符串 +1(保留前导零:`009` → `010`)
fn increment_digits(digits: &str) -> String {
    let mut out: Vec<char> = Vec::with_capacity(digits.len() + 1);
    let mut carry = 1u32;
    for c in digits.chars().rev() {
        let sum = c.to_digit(10).unwrap_or(0) + carry;
        carry = if sum > 9 { 1 } else { 0 };
        out.push(char::from_digit(sum % 10, 10).unwrap_or('0'));
    }
    if carry > 0 {
        out.push('1');
    }
    out.reverse();
    out.into_iter().collect()
}

/// JS 数字 → JSON:整数值出整数(`45` 而不是 `45.0`),与官方产物逐字节对齐(两个装配方向共用)
pub(crate) fn num(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        json!(value as i64)
    } else {
        json!(value)
    }
}

/// 官方 `z.Z`(lodash `isNil`)
fn is_nil(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

/// 取字符串字段(非字符串一律当空串,官方会抛 `TypeError`,这里退化)
fn text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// JS `Number(v)`(`undefined`/`null` → 0)
fn number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// `obj[key]` 当字典用
fn dict_at<'a>(obj: &'a Map<String, Value>, key: &str) -> Option<&'a Map<String, Value>> {
    obj.get(key).and_then(Value::as_object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::convert::shared::EditorType;

    use super::super::{TargetEditor, model::IdSource, neko::split_procedures};

    fn report() -> TranslateReport {
        TranslateReport::new(EditorType::Kitten4, TargetEditor::KittenN)
    }

    fn entity(id: &str, is_scene: bool, source: Value) -> ConvertedEntity {
        ConvertedEntity {
            source_id: id.to_string(),
            is_scene,
            blocks: vec![json!({ "type": "on_running_group_activated", "id": "hat" })],
            source: source.as_object().expect("实体是对象").clone(),
        }
    }

    fn sample_source() -> Value {
        json!({
            "project_name": "几何对战",
            "size": { "width": 960, "height": 720 },
            "theatre": {
                "scenes_order": ["scene-1"],
                "groups": {
                    "g1": { "id": "g1", "actors": ["actor-2"] },
                    "g2": { "id": "g2", "actors": ["actor-3", "actor-1"] }
                },
                "styles": {
                    "style-1": {
                        "id": "style-1", "name": "几何作战",
                        "url": "data:image/png;base64,AAAA",
                        "cdn_url": "https://creation.codemao.cn/x.png",
                        "rotate_center": { "x": 3, "y": 4 }, "pivot": { "x": 0, "y": 0 }
                    }
                }
            },
            "variables": {
                "var-1": {
                    "id": "var-1", "type": "any", "is_global": true, "scale": 1, "theme": "score",
                    "value": 5, "name": "得分", "position": { "x": -480, "y": -300 }, "visible": false
                }
            },
            "cloud_variables": {
                "cloud-1": { "id": "cloud-1", "type": "public_list", "name": "云列表", "visible": true, "scale": 1, "position": { "x": 0, "y": 0 } }
            },
            "broadcasts": { "scene-1": ["初始主页"] },
            "audio": { "audio-1": { "id": "audio-1", "name": "音效", "url": "data:audio/mp3;base64,AAAA", "ext": "mp3" } },
            "audio_order": ["audio-1"]
        })
    }

    fn actor_source(id: &str, name: &str, x: f64, y: f64, lock: Value) -> Value {
        json!({
            "id": id, "name": name, "x": x, "y": y, "scale": 45.7, "rotation": 30, "rotation_type": 0,
            "lock": lock, "draggable": false, "visible": true, "styles": ["style-1"],
            "workspace_offset": { "x": 12, "y": 0 }, "current_style_id": "style-1",
            "block_data_json": { "blocks": {} }, "user_change_r_c": 1, "editable_in_tuition_mode": false
        })
    }

    fn scene_source(id: &str, screen_name: &str, group_order: Value) -> Value {
        json!({
            "id": id, "name": "Background", "screen_name": screen_name, "actors": ["actor-1"],
            "group_order": group_order, "x": 0, "y": 0, "scale": 100, "visible": true,
            "styles": ["style-1"], "current_style_id": "style-1",
            "workspace_offset": { "x": 12, "y": 0 }, "block_data_json": { "blocks": {} }
        })
    }

    #[test]
    fn actor_gets_position_scale_and_style_rename() {
        let entities = vec![entity(
            "actor-1",
            false,
            actor_source("actor-1", "UI", 0.0, -500.0, json!(false)),
        )];
        let mut report = report();
        let doc = build_document(&sample_source(), entities, &[], 0, &mut report).expect("装配");

        let actor = &doc["actors"]["actorsDict"]["actor-1"];
        // 横屏(960>720):坐标乘 10/13,并删掉 x/y
        assert_eq!(
            actor["position"],
            json!({ "x": 0, "y": -384.61538461538464 })
        );
        assert!(actor.get("x").is_none() && actor.get("y").is_none());
        // scale 取整(整数形态),rotation 有待分组表时不动
        assert_eq!(actor["scale"], json!(45));
        assert_eq!(actor["rotation"], json!(30));
        // current_style_id → currentStyleId;block_data_json 系列被删
        assert_eq!(actor["currentStyleId"], json!("style-1"));
        assert!(actor.get("current_style_id").is_none());
        assert!(actor.get("block_data_json").is_none());
        assert!(actor.get("user_change_r_c").is_none());
        assert!(actor.get("editable_in_tuition_mode").is_none());
        // lock 假值原样保留(官方行为:只有真值才换成 locked)
        assert_eq!(actor["lock"], json!(false));
        assert!(actor.get("locked").is_none());
        // workspaceScrollXy:有 offset 用 offset(y=0 是假值 → 兜底 50)
        assert_eq!(actor["workspaceScrollXy"], json!({ "x": 12, "y": 50 }));
        assert_eq!(actor["name"], json!("UI"));
        assert_eq!(
            actor["nekoBlockJsonList"][0]["type"],
            json!("on_running_group_activated")
        );
    }

    #[test]
    fn actor_locked_and_rotation_flip_without_groups() {
        let mut source = sample_source();
        source
            .as_object_mut()
            .expect("obj")
            .get_mut("theatre")
            .expect("theatre")
            .as_object_mut()
            .expect("obj")
            .remove("groups");
        let entities = vec![entity(
            "actor-1",
            false,
            actor_source("actor-1", "UI", 100.0, 200.0, json!(true)),
        )];
        let mut report = report();
        let doc = build_document(&source, entities, &[], 0, &mut report).expect("装配");

        let actor = &doc["actors"]["actorsDict"]["actor-1"];
        assert_eq!(actor["locked"], json!(true), "真值 lock 换成 locked");
        assert!(actor.get("lock").is_none());
        assert_eq!(actor["rotation"], json!(-30), "没有分组表时旋转取反");
        assert_eq!(
            actor["position"],
            json!({ "x": 76.92307692307693, "y": 153.84615384615387 })
        );
    }

    #[test]
    fn scene_expands_actor_ids_from_groups_and_uniquifies_names() {
        let entities = vec![
            entity(
                "scene-1",
                true,
                scene_source("scene-1", "主页", json!(["g1", "g2"])),
            ),
            entity("scene-2", true, scene_source("scene-2", "主页", json!([]))),
        ];
        let mut source = sample_source();
        source["theatre"]["scenes_order"] = json!(["scene-1", "scene-2"]);
        let mut report = report();
        let doc = build_document(&source, entities, &[], 0, &mut report).expect("装配");

        let scene = &doc["scenes"]["scenesDict"]["scene-1"];
        // 有 groups:actorIds 按 group_order 展开(照抄 scene.actors 的那半不生效)
        assert_eq!(scene["actorIds"], json!(["actor-2", "actor-3", "actor-1"]));
        assert!(scene.get("group_order").is_none());
        assert_eq!(scene["name"], json!("背景"));
        assert_eq!(scene["screenName"], json!("主页"));
        assert_eq!(scene["currentStyleId"], json!("style-1"));
        assert_eq!(scene["workspaceScrollXy"], json!({ "x": 12, "y": 50 }));
        // 场景保留 x/y/scale(官方只对角色做坐标换算)
        assert_eq!(scene["x"], json!(0));
        assert!(scene.get("block_data_json").is_none());

        // 第二个场景:同名 → 尾部数字 +1;group_order 为空数组时原样保留
        let second = &doc["scenes"]["scenesDict"]["scene-2"];
        assert_eq!(second["screenName"], json!("主页1"));
        assert_eq!(second["group_order"], json!([]));
        assert_eq!(
            second["actorIds"],
            json!([]),
            "有 groups 时照抄 scene.actors 的分支不生效"
        );

        assert_eq!(doc["scenes"]["sortList"], json!(["scene-1", "scene-2"]));
        assert_eq!(doc["scenes"]["currentSceneId"], json!("scene-1"));

        // 没有 groups 时:actorIds 照抄 `scene.actors`
        let mut no_groups = sample_source();
        no_groups["theatre"]
            .as_object_mut()
            .expect("obj")
            .remove("groups");
        let doc = build_document(
            &no_groups,
            vec![entity(
                "scene-1",
                true,
                scene_source("scene-1", "主页", json!([])),
            )],
            &[],
            0,
            &mut report,
        )
        .expect("装配");
        assert_eq!(
            doc["scenes"]["scenesDict"]["scene-1"]["actorIds"],
            json!(["actor-1"])
        );
    }

    #[test]
    fn name_uniquify_increments_trailing_digits() {
        let entities = vec![
            entity(
                "actor-1",
                false,
                actor_source("actor-1", "小明", 0.0, 0.0, json!(false)),
            ),
            entity(
                "actor-2",
                false,
                actor_source("actor-2", "小明", 0.0, 0.0, json!(false)),
            ),
            entity(
                "actor-3",
                false,
                actor_source("actor-3", "小明1", 0.0, 0.0, json!(false)),
            ),
            entity(
                "actor-4",
                false,
                actor_source("actor-4", "9", 0.0, 0.0, json!(false)),
            ),
            entity(
                "actor-5",
                false,
                actor_source("actor-5", "  ", 0.0, 0.0, json!(false)),
            ),
            entity(
                "actor-6",
                false,
                actor_source("actor-6", "a<b>c\"d", 0.0, 0.0, json!(false)),
            ),
        ];
        let mut report = report();
        let doc = build_document(&sample_source(), entities, &[], 0, &mut report).expect("装配");
        let name = |id: &str| doc["actors"]["actorsDict"][id]["name"].clone();
        assert_eq!(name("actor-1"), json!("小明"));
        assert_eq!(name("actor-2"), json!("小明1"));
        assert_eq!(name("actor-3"), json!("小明2"));
        assert_eq!(name("actor-4"), json!("9"));
        assert_eq!(name("actor-5"), json!("1"), "纯空白名兜底 1");
        assert_eq!(
            name("actor-6"),
            json!("abcd"),
            "非法字符(<,>,.)被删,\" 也是"
        );
    }

    #[test]
    fn variables_map_theme_and_positions() {
        let mut source = sample_source();
        source["variables"] = json!({
            "v-score": { "id": "v-score", "type": "any", "is_global": false, "scale": 1, "theme": "score", "value": 5, "name": "得分", "position": { "x": -480, "y": -300 }, "visible": false },
            "v-hp": { "id": "v-hp", "type": "any", "scale": 2, "theme": "HP", "name": "血", "current_entity": "actor-1" },
            "v-clock": { "id": "v-clock", "type": "any", "theme": "clock", "name": "钟" },
            "v-coin": { "id": "v-coin", "type": "any", "theme": "coin", "name": "币" },
            "v-pure": { "id": "v-pure", "type": "any", "theme": "pure", "name": "字" },
            "v-common": { "id": "v-common", "type": "any", "theme": "common", "name": "常" }
        });
        let mut report = report();
        // 时钟由门面传入:确定性模式传 0,否则传当前毫秒;这里显式传一个固定值验证落键
        let doc = build_document(&source, vec![], &[], 1700000000123, &mut report).expect("装配");
        let vars = &doc["variables"]["variablesDict"];
        assert_eq!(vars["v-score"]["style"], json!("icon_medal"));
        assert_eq!(vars["v-score"]["isGlobal"], json!(false));
        assert_eq!(
            vars["v-score"]["createTime"],
            json!(1700000000123u64),
            "缺 create_time 时用传入的时钟"
        );
        // O():(x + 960/2) * 900/960,(720/2 - y) * 562/720
        assert_eq!(
            vars["v-score"]["position"],
            json!({ "x": 0, "y": 515.1666666666666 })
        );
        assert_eq!(vars["v-score"]["value"], json!(5));
        assert_eq!(vars["v-hp"]["style"], json!("icon_heart"));
        assert_eq!(vars["v-hp"]["currentEntityId"], json!("actor-1"));
        assert!(
            vars["v-hp"].get("value").is_none(),
            "源里没有 value → 不落键(官方 undefined)"
        );
        assert_eq!(vars["v-clock"]["style"], json!("icon_hourglass"));
        assert_eq!(vars["v-coin"]["style"], json!("icon_coin"));
        assert_eq!(vars["v-pure"]["style"], json!("text"));
        assert_eq!(vars["v-common"]["style"], json!("default"));

        // 云变量:public_list → list + 初值 [];isGlobal 恒为 true
        let cloud = &doc["variables"]["variablesDict"]["cloud-1"];
        assert_eq!(cloud["type"], json!("list"));
        assert_eq!(cloud["value"], json!([]));
        assert_eq!(cloud["isGlobal"], json!(true));
        assert_eq!(cloud["style"], json!("default"));

        // private → any + 初值 0;未识别的类型原样透传
        let mut private = sample_source();
        private["cloud_variables"] = json!({
            "c-private": { "id": "c-private", "type": "private", "name": "私有", "position": { "x": 480, "y": 360 } },
            "c-other": { "id": "c-other", "type": "public", "name": "公开", "position": { "x": 0, "y": 0 } }
        });
        let doc = build_document(&private, vec![], &[], 0, &mut report).expect("装配");
        let vars = &doc["variables"]["variablesDict"];
        assert_eq!(vars["c-private"]["type"], json!("any"));
        assert_eq!(vars["c-private"]["value"], json!(0));
        assert_eq!(vars["c-private"]["position"], json!({ "x": 900, "y": 0 }));
        assert_eq!(vars["c-other"]["type"], json!("public"));
        assert_eq!(vars["c-other"]["value"], json!(0));
    }

    #[test]
    fn broadcast_wrap_stage_size_and_document_skeleton() {
        let mut report = report();
        let doc = build_document(&sample_source(), vec![], &[], 0, &mut report).expect("装配");
        assert_eq!(
            doc["broadcasts"],
            json!({ "broadcastsDict": { "scene-1": ["初始主页"] } })
        );
        assert_eq!(doc["stageSize"], json!({ "width": 900, "height": 562 }));
        assert_eq!(doc["projectName"], json!("几何对战"));
        assert_eq!(doc["version"], json!(BCM_VERSION));
        assert_eq!(doc["toolType"], json!("KN"));
        for key in ["previewUrl", "resourceZip", "guideUrl"] {
            assert_eq!(doc[key], json!(""));
        }
        for key in ["textToBlock", "aiImageUrls"] {
            assert_eq!(doc[key], json!([]));
        }
        assert_eq!(
            doc["hidden_toolbox"],
            json!({ "toolbox": [], "blocks": [] })
        );
        assert_eq!(doc["courseMaterials"], json!([]));
        assert_eq!(doc["procedures"]["proceduresDict"], json!({}));
        assert!(
            doc.get("toolMode").is_none() && doc.get("isHideStage").is_none(),
            "活编辑器状态不移植"
        );
        // 程序集条目落进 proceduresDict(经 split_procedures 之后)
        let mut ids = IdSource::new(true);
        let def = json!({
            "type": "procedures_2_defnoreturn", "id": "proc-1", "fields": { "NAME": "跳跃" },
            "inputs": { "STACK": { "type": "self_go_forward", "id": "fwd" } }
        });
        let tree = super::super::model::BlockTree::new(vec![
            super::super::model::BlockJson::from_value(&def).expect("节点"),
        ]);
        let (_, procedures) = split_procedures(tree, &mut ids, &mut report);
        let doc =
            build_document(&sample_source(), vec![], &procedures, 0, &mut report).expect("装配");
        let entry = &doc["procedures"]["proceduresDict"]["proc-1"];
        assert_eq!(entry["name"], json!("跳跃"));
        assert_eq!(entry["type"], json!("NORMAL"));
        assert_eq!(entry["params"][0]["type"], json!("Label"));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["statements"]["STACK"]["type"],
            json!("self_go_forward")
        );
        assert!(doc.get("blockedActors").is_none());

        // 已经包好的 broadcasts 原样透传(阶段 1 之后再装配)
        let mut wrapped = sample_source();
        wrapped["broadcasts"] = json!({ "broadcastsDict": { "scene-1": ["初始主页"] } });
        let doc = build_document(&wrapped, vec![], &[], 0, &mut report).expect("装配");
        assert_eq!(
            doc["broadcasts"],
            json!({ "broadcastsDict": { "scene-1": ["初始主页"] } })
        );

        // 竖屏 + 缺 project_name
        let mut portrait = sample_source();
        portrait["size"] = json!({ "width": 720, "height": 960 });
        portrait
            .as_object_mut()
            .expect("obj")
            .remove("project_name");
        let doc = build_document(&portrait, vec![], &[], 0, &mut report).expect("装配");
        assert_eq!(doc["stageSize"], json!({ "width": 562, "height": 900 }));
        assert_eq!(doc["projectName"], json!("空白作品"));
        // 竖屏造型:url 被 cdn_url 覆盖(官方 portrait 分支),所以不再报"要上传"
        assert_eq!(
            doc["styles"]["stylesDict"]["style-1"]["url"],
            json!("https://creation.codemao.cn/x.png")
        );
    }

    #[test]
    fn styles_and_audio_keep_source_urls_and_report_uploads() {
        let mut report = report();
        let doc = build_document(&sample_source(), vec![], &[], 0, &mut report).expect("装配");
        // 横屏:官方对每个造型都重采样上传 → 源 url 原样保留 + 报 dropped
        let style = &doc["styles"]["stylesDict"]["style-1"];
        assert_eq!(style["url"], json!("data:image/png;base64,AAAA"));
        assert_eq!(style["cdn_url"], json!("https://creation.codemao.cn/x.png"));
        assert_eq!(
            style["centerPoint"],
            json!({ "x": 3, "y": 4 }),
            "rotate_center → centerPoint"
        );
        assert_eq!(style["pivot"], json!({ "x": 0, "y": 0 }), "其余键保真");
        // 「官方会重新上传」按**非损失**类记(C3):产物 url 由平台重写,我们保留源 url
        assert!(report.warnings().iter().any(
            |w| matches!(w, TranslateWarning::ReuploadedOnImport { path } if path.contains("theatre.styles[*]"))
        ));
        assert!(
            !report.is_lossy(),
            "重传资源不算有损(否则 strict 在任何含 data:/非 https 造型的真作品上必失败)"
        );

        // 音频:没有 cdn_url → 官方必然重传,源 url 保真 + 报告;sortList/currentAudioId 取 audio_order
        let audios = &doc["audios"];
        assert_eq!(
            audios["audiosDict"]["audio-1"]["url"],
            json!("data:audio/mp3;base64,AAAA")
        );
        assert_eq!(audios["sortList"], json!(["audio-1"]));
        assert_eq!(audios["currentAudioId"], json!("audio-1"));
        assert!(report.warnings().iter().any(
            |w| matches!(w, TranslateWarning::ReuploadedOnImport { path } if path.contains("audio[*]"))
        ));

        // 没有 audio_order 时 sortList = 遍历顺序,currentAudioId 取首个
        let mut source = sample_source();
        source.as_object_mut().expect("obj").remove("audio_order");
        let doc = build_document(&source, vec![], &[], 0, &mut report).expect("装配");
        assert_eq!(doc["audios"]["sortList"], json!(["audio-1"]));
    }

    #[test]
    fn sanitize_truncates_and_keeps_chinese_punctuation() {
        assert_eq!(sanitize("小明。"), "小明。");
        assert_eq!(sanitize("a.b"), "ab");
        assert_eq!(sanitize("ok!"), "ok!");
        assert_eq!(sanitize("emoji😀x"), "emojix");
        assert_eq!(sanitize("好的·吧"), "好的·吧");
        assert_eq!(truncate_width("中中中", 5), "中中中");
        assert_eq!(truncate_width("abcdef", 3), "abc");
        assert_eq!(truncate_width("中文", 3), "中文");
        assert_eq!(
            truncate_width(&"中".repeat(25), NAME_MAX_WIDTH)
                .chars()
                .count(),
            20,
            "宽字符算 2 宽,上限 40"
        );
        assert!(is_bad_name("   "));
        assert!(!is_bad_name("x"));
        assert_eq!(increment_suffix("小明"), "小明1");
        assert_eq!(increment_suffix("小明9"), "小明10");
        assert_eq!(increment_suffix(""), "1");
        assert_eq!(increment_digits("009"), "010");
    }
}
