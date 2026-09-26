use super::mapping::truthy;
use super::model::{BlockJson, TEMP_ID_PREFIX, is_temp_id_char};
use super::model::{BlockTree, type_name};
use super::model::{ProcedureEntry, procedures_to_json};
use super::tables_gen::{BCM_VERSION, STAGE_LANDSCAPE, STAGE_PORTRAIT};
use super::{
    StageOrientation, TranslateError, TranslateOptions, TranslateReport, TranslateWarning, mapping,
    model, tables_gen,
};
use crate::core::convert::shared::{DecompilerError, Result};
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

// 来自 src/core/convert/translate/assembly.rs
// KN 文档装配:官方 `De`(webpack module 68602,`temp/ref/handler_de.pretty.js`)的移植。
// 输入是流水线的两半:实体的**源字段**(`ConvertedEntity::source`,阶段 1 之后仍带
// `x/y/scale/lock/current_style_id/workspace_offset/…` 的实体对象)+ 已转换好的
// `nekoBlockJsonList`(`ConvertedEntity::blocks`)+ 程序集条目([`ProcedureEntry`]);
// 输出是能过官方 `validateBcm` 的 `.bcmkn` 对象(顶层键与真机 `.bcmkn` 一致)。
// 官方锚点(行号 = `temp/ref/handler_de.pretty.js`,即官方 module 68602):
// - 画布/坐标:`N = size.width > size.height`(241);变量坐标换算 `O`(352-381)按
// `{ x:(x+w/2)*W/w, y:(h/2-y)*H/h }` 把源舞台坐标折算到目标舞台 → [`stage_position`]
// - 角色 `mapValues(theatre.actors, …)`(247-272)→ [`actor_entry`]:删 `block_data_json`/
// `user_change_r_c`/`editable_in_tuition_mode`;名字净化 `jJ`(250)、空名兜底 `"1"`(`yK` 251)、
// 唯一化 `mq`(252);`current_style_id→currentStyleId`(256);横屏 `position = {x*10/13,…}`
// 并删 `x`/`y`(257-261);`lock→locked`(262);无 `theatre.groups` 时 `rotation` 取反(263);
// `scale` 取整(264);`workspaceScrollXy` 兜底 `{100,50}`(265)
// - 场景 `mapValues(theatre.scenes, …)`(275-298)→ [`scene_entry`]:`name` 恒「背景」+
// `screenName` 唯一化(276-279);`actorIds` = 无 `groups` 时的 `scene.actors`,再加
// `group_order` 展开的 `groups[gid].actors`,随后删 `group_order`(280-289);
// `currentStyleId`(292);`workspaceScrollXy`(293)
// - `scenes.sortList`/`currentSceneId`(300-301);音频 `ye(n.audio, …)`(304-343,`sortList`/
// `currentAudioId`);造型 `stylesDict`(351,上传逻辑 20-210);变量 `B`(383-535,普通变量
// 400-430 → [`local_variable`]、云变量 432-470 → [`cloud_variable`]);`stageSize`(547)、
// `projectName`(548)
// ## 离线近似(刻意差异)
// 1. **造型/音频不再上传**:官方对造型 `fetch → blob`(横屏还按 `10/13` 重采样)再
// `ServiceApi.uploadUserFiles` 换新 URL,音频同理。本库不联网、不上传,**源 URL 原样保留**,
// 并把"官方会重传"的条目记 [`TranslateWarning::DroppedProperty`](判据:造型取
// `cdn_url ?? url` 后不是 `https://`,音频没有 `cdn_url`)。官方 `gU(url)`(判断是否站内资源)
// 未反编译,portrait 分支"已是站内资源就跳过 fetch"这半边只能按 `https://` 近似;横屏分支
// 官方对**所有**造型都重采样上传(连 `https://` 也换),这里不额外报。
// 2. **`toolMode`/`isHideStage` 不移植**:官方从活编辑器实例读
// (`f.BcmInstance.getBcm().toolMode`),转换器没有编辑器状态;真机 `.bcmkn` 里它们由编辑器
// 保存时补,`validateBcm` 不要求。
// 3. **`blockedActors` 不写**:官方只在模板自带该键时才写(`t.blockedActors && …`),我们的
// 模板没有(角色 `lock→locked` 仍然生效)。
// 4. **`source` 字段不在这里**:官方装配完另把原始 `.bcm4` 上传、把 URL 写到 `w.source`(§3.1),
// 属于编排层的"保留原件"选项,不属于 `De`。
// 5. 缺 `theatre.scenes_order` 时官方会写出 `undefined`(产物非法);本实现退化为实体输入顺序。
// 6. 名字净化按官方正则的**字符集语义**(白名单外字符 + `<>&."` 一律删)→ [`sanitize`]/
// [`truncate_width`](>255 的字符算 2 宽,上限 40 = 官方 `o.dT`);JS `\s` 与
// `char::is_whitespace` 的空白集合略有差异(只用于"整体是否空白")。
// 7. 变量/云变量的 `position` 一律按官方公式重算;源侧没有的键(`current_entity`/`create_time`
// 缺失等)按官方 `undefined` 语义**不落键**(`createTime` 有 `Date.now()` 兜底,必落)。
// 8. 畸形数字(`rotation`/`scale`/坐标不是有限数)官方会算出 `NaN`(JSON 里成 `null`),
// 本实现按 0 处理或跳过改键,产物更稳。
// 反向(KN `.bcmkn` → Kitten4 `.bcm4`)在文件后半段:官方没有这个方向,是本项目自建的
// 逆映射(规则见 `docs/rounds/20-kitten-kn-work-conversion-plan.md` §4);两个方向共用
// `num` / `project_name_at` / 坐标与主题表,放在同一文件里便于对照防漂移。

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
pub(super) fn source_stage_size(src: &Map<String, Value>) -> (f64, f64) {
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

/// `theme` → 变量样式图标(官方 395-411 `switch (e.theme)`;见 [`VAR_STYLE_TABLE`])
fn theme_style(theme: Option<&Value>) -> &'static str {
    theme
        .and_then(Value::as_str)
        .and_then(|theme| {
            VAR_STYLE_TABLE
                .iter()
                .find(|(name, _)| *name == theme)
                .map(|(_, style)| *style)
        })
        .unwrap_or(VAR_STYLE_DEFAULT)
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

// ---------------------------------------------------------------------------
// 反向:KN `.bcmkn` → Kitten4 `.bcm4`(本项目自建;官方无此方向,见 docs/rounds/20 §4)
// ---------------------------------------------------------------------------

/// Kitten4 编辑版常量(取自真实作品 `download/compile/raw/几何对战-联机.bcm4`,
/// `version=25` / `application_version=4.11.20` / `type=1` / `work_type="KITTEN"`)
const KITTEN4_VERSION: i64 = 25;
const KITTEN4_APPLICATION_VERSION: &str = "4.11.20";
/// Kitten4 里场景实体的固定名(显示名在 `screen_name`,与正向 `finish::SCENE_NAME` 对称)
const KITTEN4_SCENE_NAME: &str = "Background";
/// 正向横屏把角色坐标乘 `10/13`(finish.rs `LANDSCAPE_POSITION_SCALE`),反向除回去
const LANDSCAPE_POSITION_BACK_SCALE: f64 = 13.0 / 10.0;
/// 变量样式图标 ↔ Kitten 主题:唯一的双向来源(正向官方 `switch (e.theme)` 395-411,反向是它的逆)
///
/// 正向没命中 → `VAR_STYLE_DEFAULT`(`default`);反向没命中(含 `default`)→ `common`。
const VAR_STYLE_TABLE: &[(&str, &str)] = &[
    ("score", VAR_STYLE_MEDAL),
    ("HP", VAR_STYLE_HEART),
    ("clock", VAR_STYLE_HOURGLASS),
    ("coin", VAR_STYLE_COIN),
    ("pure", VAR_STYLE_TEXT),
];

/// KN 变量样式图标 → Kitten 主题(见 [`VAR_STYLE_TABLE`])
fn theme_of_style(style: &str) -> &'static str {
    VAR_STYLE_TABLE
        .iter()
        .find(|(_, mapped)| *mapped == style)
        .map(|(theme, _)| *theme)
        .unwrap_or("common")
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
            entities.push(KnEntity {
                source_id: id.clone(),
                is_scene,
                tree,
                source: entity.as_object().cloned().unwrap_or_default(),
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
        for id in duplicate_ids(&entity.tree) {
            report.warn(TranslateWarning::RemintedId { from: id });
        }
        let value = model::build_block_data_json(&entity.tree, &mut ids)?;
        converted += entity.tree.count();
        blocks_by_entity.push((index, value));
    }
    report.blocks_converted = converted;
    report.elapsed_ms = started.elapsed().as_millis();

    Ok(build_kitten4_document(
        src,
        entities,
        blocks_by_entity,
        landscape,
        (canvas_w, canvas_h),
        (kn_w, kn_h),
        report,
    ))
}

/// 树里出现两次以上的 id(每个重复值报一次)
fn duplicate_ids(tree: &model::BlockTree) -> Vec<String> {
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
    tree: model::BlockTree,
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

/// Kitten4 默认工具箱开关表(照平台 Kitten4 文件抄:18 个开关,只有 `cognitive` 默认开)
fn kitten4_default_toolbox() -> serde_json::Value {
    json!({
        "current_type": "",
        "physics": false,
        "physics2": false,
        "block_ai_classification": false,
        "block_ai_game": false,
        "block_hardware_arduino": false,
        "block_hardware_weeemake": false,
        "block_hardware_microbit": false,
        "cloud_variable": false,
        "cloud_list": false,
        "advanced": false,
        "camera": false,
        "video": false,
        "wood": false,
        "ai_lab": false,
        "midimusic": false,
        "mobile_control": false,
        "cognitive": true,
    })
}

/// Kitten4 工具箱分类顺序(照平台原件抄;`data` 出现两次是平台原样,别"顺手去重")
#[rustfmt::skip]
const KITTEN4_TOOLBOX_ORDER: &[&str] = &[
    "event", "control", "action", "appearance", "audio", "pen", "sensing", "operator", "data", "data",
    "procedure", "mobile_control", "physic", "physics2", "cloud_variable", "cloud_list", "advanced",
    "ai_lab", "ai_game", "cognitive", "camera", "video", "wood", "arduino", "weeemake", "microbit",
    "ai", "midimusic",
];

/// 剔掉 Kitten4 编辑器不认识的积木(连带清理 `connections` 里的父子引用),逐类记报告。
///
/// 背景见 [`crate::core::convert::translate::kitten4_vocab`] 与 `docs/rounds/34` §4nonies:
/// 编辑器遇到未知积木类型会让**整份工作区**加载失败 ⇒ 不剔的后果是"打开什么都看不到"。
fn strip_unknown_blocks(
    blocks: serde_json::Value,
    report: &mut TranslateReport,
) -> serde_json::Value {
    use serde_json::{Map, Value};

    let Some(mut root) = blocks.as_object().cloned() else {
        return blocks;
    };
    let Some(table) = root.get("blocks").and_then(Value::as_object).cloned() else {
        return Value::Object(root);
    };

    let mut kept = Map::new();
    let mut dropped: Map<String, Value> = Map::new();
    for (id, block) in table {
        let unknown = block
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| !super::kitten4_vocab::kitten4_editor_knows(kind));
        if unknown {
            let kind = block
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let count = dropped.get(&kind).and_then(Value::as_u64).unwrap_or(0);
            dropped.insert(kind, json!(count + 1));
        } else {
            kept.insert(id, block);
        }
    }
    if dropped.is_empty() {
        return Value::Object(root);
    }

    // 被剔掉的块不能再出现在任何父子关系里(否则编辑器照样解析失败)
    if let Some(connections) = root.get("connections").and_then(Value::as_object).cloned() {
        let mut rebuilt = Map::new();
        for (parent, children) in connections {
            if !kept.contains_key(&parent) {
                continue;
            }
            let mut kept_children = Map::new();
            if let Some(map) = children.as_object() {
                for (child, slot) in map {
                    if kept.contains_key(child) {
                        kept_children.insert(child.clone(), slot.clone());
                    }
                }
            }
            rebuilt.insert(parent, Value::Object(kept_children));
        }
        root.insert("connections".into(), Value::Object(rebuilt));
    }

    // 影子 XML 里也可能写着编辑器不认识的类型(它是**字符串**,清积木表时扫不到)。
    // 实测某作品 9696 条影子里 174 条如此(`get_split_options` 占 158)⇒ 一并清成空串
    // (库里既有的占位写法),并逐类计入报告。**必须在把 `kept` 交给 root 之前就地改**。
    let mut shadow_fixed: Map<String, Value> = Map::new();
    for block in kept.values_mut() {
        let Some(map) = block.as_object_mut() else {
            continue;
        };
        let Some(shadows) = map.get("shadows").and_then(Value::as_object).cloned() else {
            continue;
        };
        let mut new_shadows = shadows.clone();
        let mut changed = false;
        for (slot, xml) in shadows {
            let Some(text) = xml.as_str() else { continue };
            let Some(start) = text.find("type=\"") else {
                continue;
            };
            let rest = &text[start + 6..];
            let Some(end) = rest.find('"') else { continue };
            let kind = &rest[..end];
            if !super::kitten4_vocab::kitten4_editor_knows(kind) {
                new_shadows.insert(slot.clone(), Value::String(String::new()));
                changed = true;
                let count = shadow_fixed.get(kind).and_then(Value::as_u64).unwrap_or(0);
                shadow_fixed.insert(kind.to_string(), json!(count + 1));
            }
        }
        if changed {
            map.insert("shadows".into(), Value::Object(new_shadows));
        }
    }

    root.insert("blocks".into(), Value::Object(kept));
    for (kind, count) in shadow_fixed {
        report.warn(TranslateWarning::UnmappedBlock {
            kind: format!(
                "{kind}(Kitten4 编辑器不认识,已清空 {} 条影子)",
                count.as_u64().unwrap_or(0)
            ),
        });
    }

    for (kind, count) in dropped {
        report.warn(TranslateWarning::UnmappedBlock {
            kind: format!(
                "{kind}(Kitten4 编辑器不认识,已剔除 {} 块)",
                count.as_u64().unwrap_or(0)
            ),
        });
    }
    Value::Object(root)
}

/// 装配 Kitten4 编辑版文档
fn build_kitten4_document(
    src: &serde_json::Map<String, serde_json::Value>,
    // `entities` / `blocks_by_entity` 都**按值**收:装配阶段要移动实体源对象与
    // 已编码的积木数据,旧实现每个实体各 clone 一次(含整份 `nekoBlockJsonList`)
    // —— 一份文档级别的白拷贝(方案 23 P0-3)
    entities: Vec<KnEntity>,
    blocks_by_entity: Vec<(usize, serde_json::Value)>,
    landscape: bool,
    canvas: (f64, f64),
    kn_stage: (f64, f64),
    report: &mut TranslateReport,
) -> serde_json::Value {
    use serde_json::{Map, Value, json};

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
    // 编码阶段的 `blocks_by_entity` 与 `entities` 同序同长(见调用点)
    for (position, (entity, (index, blocks))) in
        entities.into_iter().zip(blocks_by_entity).enumerate()
    {
        debug_assert_eq!(position, index, "blocks_by_entity 顺序须与 entities 一致");
        // 移动实体源对象与积木数据(都不再 clone)
        let source = entity.source;
        // 产物是给 Kitten4 **编辑器**读的:编辑器不认识的积木会让它**整份工作区加载失败**
        // (实机实测:80 种类型里 20 种不认识 ⇒ 画布一块都不显示)。所以先把不认识的块剔掉,
        // 逐类记进报告 —— 宁可少几块,也要让作品能打开(见 `kitten4_vocab`、rounds/34 §4nonies)。
        let blocks = strip_unknown_blocks(blocks, report);
        if entity.is_scene {
            scenes.insert(
                entity.source_id.clone(),
                Value::Object(kitten4_scene(&source, blocks, report)),
            );
        } else {
            let scene = scene_of_actor
                .get(&entity.source_id)
                .cloned()
                .or_else(|| first_scene.clone());
            actors.insert(
                entity.source_id.clone(),
                Value::Object(kitten4_actor(&source, blocks, scene, landscape, report)),
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

    let (variables, variable_order) = kitten4_variables(src, canvas, kn_stage, report);
    let (audio, audio_order) = kitten4_audio(src);

    let mut doc = Map::new();
    doc.insert("work_type".into(), json!("KITTEN"));
    doc.insert("type".into(), json!(1));
    doc.insert("version".into(), json!(KITTEN4_VERSION));
    doc.insert(
        "application_version".into(),
        json!(KITTEN4_APPLICATION_VERSION),
    );
    doc.insert(
        "project_name".into(),
        json!(project_name_at(src, "projectName")),
    );
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
    // Kitten4 **编辑版必备的平台骨架键**:源(KN 文档)里通常**没有** `toolbox` 这几个,
    // 而平台自己的 Kitten4 文件都有(实测:转换产物比平台原件少 15 个顶层键,而反编译产物
    // 只少 `painter` —— 那条路编辑器能正常读)。缺了它们,编辑器可能读不出积木 ⇒ 兜底补默认值;
    // 源里真有就照搬(仍然优先)。
    doc.insert("hidden_toolbox".into(), json!({ "toolbox": [], "blocks": [] }));
    doc.insert("toolbox".into(), kitten4_default_toolbox());
    doc.insert("toolbox_order".into(), json!(KITTEN4_TOOLBOX_ORDER));
    doc.insert("last_toolbox_order".into(), json!(KITTEN4_TOOLBOX_ORDER));
    for key in [
        "toolbox",
        "toolbox_order",
        "last_toolbox_order",
        "hidden_toolbox",
    ] {
        if let Some(value) = src.get(key) {
            doc.insert(key.to_string(), value.clone());
        }
    }
    // 平台文件里同样存在的其余骨架键(转换路径原来一个都没有)
    doc.insert("ai_lab".into(), json!({}));
    doc.insert("matrix".into(), json!({}));
    doc.insert("models".into(), json!({}));
    doc.insert("midi_order".into(), json!([]));
    doc.insert("midimusic".into(), json!([]));
    doc.insert("is_partial".into(), json!(false));
    doc.insert("sample_id".into(), json!(""));
    doc.insert("device_widget_type".into(), Value::Null);
    doc.insert("hardware_type".into(), json!(""));
    doc.insert("work_source_label".into(), json!(1));
    // 目标作品的 id 由平台在保存/导入时写回,转换阶段无从得知 ⇒ 空串
    doc.insert("codemao_value".into(), json!(""));
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

/// KN 位置(左上原点像素系)→ Kitten 位置(中心原点)。
///
/// **与正向 [`stage_position`] 严格互逆**(同一文件内相邻,便于对照;公式本身不合并 —— 两者
/// 的参数形态与 `num()` 取整口径不同,强行抽象只会掩盖差异):
/// 正向 `x' = (x + src_w/2) * target_w / src_w`、`y' = (src_h/2 - y) * target_h / src_h`,
/// 这里 `x = x' * canvas_w / kn_w - canvas_w/2`、`y = canvas_h/2 - y' * canvas_h / kn_h`。
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

#[cfg(test)]
mod assembly_tests {
    use super::*;
    use crate::core::convert::shared::EditorType;

    use super::super::{TargetEditor, model::IdSource, model::split_procedures};

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

    /// 显式方向换画布尺寸时,坐标按比例重算(不是"只换尺寸不改坐标")
    #[test]
    fn explicit_orientation_rescales_positions() {
        // 源(像素系左上角)→ Kitten4(中心原点):x 减半宽、y 加半高
        let position = json!({ "x": 0, "y": 0 });
        let (x, y) = kitten4_position(&position, (900.0, 562.0), (900.0, 562.0));
        assert_eq!((x, y), (-450.0, 281.0));

        // 换成竖屏画布(562×900):同一个像素点被按 `canvas / stageSize` 比例重新换算
        let (x2, y2) = kitten4_position(&position, (562.0, 900.0), (900.0, 562.0));
        assert_eq!((x2, y2), (-281.0, 450.0));
        assert_ne!(
            (x, y),
            (x2, y2),
            "显式 StageOrientation 必须让坐标跟着换算,而不是只改画布尺寸"
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
pub(crate) type IdRemap = HashMap<String, String>;

// ---------------------------------------------------------------- 并行调度

/// 实际线程数:≤ 请求并发、≤ 工作项数、≤ 可用核数
///
/// 作品级(批量)与实体级是**两级**并发,预算折算在
/// [`TranslateOptions::fold_entity_concurrency`](super::TranslateOptions::fold_entity_concurrency)
/// 里做(方案 25 §7 阻塞 #6);这里只负责"别为 3 个实体开 32 个线程、别超出核数"。
pub(crate) fn workers(requested: usize, items: usize) -> usize {
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
pub(crate) fn run_items<I, T, F>(
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
pub(crate) fn remap_tree(map: &IdRemap, tree: &mut BlockTree) -> (usize, usize) {
    let mut nodes = 0;
    let mut unmatched = 0;
    for root in &mut tree.roots {
        remap_node(map, root, &mut nodes, &mut unmatched);
    }
    (nodes, unmatched)
}

/// 改写一条程序集条目(条目 id / 形参 id / 定义体积木);返回 `(节点数, 未命中数)`
pub(crate) fn remap_entry(map: &IdRemap, entry: &mut ProcedureEntry) -> (usize, usize) {
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
pub(crate) fn merge_report(
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
        TranslateWarning::UnmappedBlock { kind } => TranslateWarning::UnmappedBlock {
            kind: remap_owned(map, kind, unmatched),
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
pub(crate) fn remap_json(map: &IdRemap, value: &mut Value) -> usize {
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
