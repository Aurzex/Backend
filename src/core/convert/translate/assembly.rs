use super::mapping::truthy;
use super::model::{BlockTree, ProcedureEntry, procedures_to_json, type_name};
use super::report::{TranslateReport, TranslateWarning};
use super::tables_gen::{BCM_VERSION, STAGE_LANDSCAPE, STAGE_PORTRAIT};
use super::{mapping, model, tables_gen};
use crate::core::convert::shared::{DecompilerError, Result};
use serde_json::{Map, Value, json};
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
pub(super) struct ConvertedEntity {
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
pub(super) fn build_document(
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
        // P7(rounds/37):`entities` 是按值收的 Vec ⇒ 解构搬走,不再 clone 每个实体对象
        // (与上方 `build_kitten4_document` 的同段改法同一口径)。
        let ConvertedEntity {
            source_id,
            is_scene,
            blocks,
            mut source,
        } = entity;
        let mut value = source;
        value.remove("block_data_json");
        value.insert("nekoBlockJsonList".into(), Value::Array(blocks));
        if is_scene {
            scene_entry(&mut value, groups, &mut scene_used);
            scenes.insert(source_id, Value::Object(value));
        } else {
            actor_entry(&mut value, groups, &mut actor_used, landscape);
            actors.insert(source_id, Value::Object(value));
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
pub(super) fn project_name_at(src: &Map<String, Value>, key: &str) -> String {
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
pub(super) fn current_epoch_ms() -> u128 {
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
    name.chars().filter(|c| is_display_name_char(*c)).collect()
}

/// 官方白名单里额外放行的中文标点(其余按码点区间判断)
const EXTRA_NAME_CHARS: [char; 25] = [
    '！', '￥', '…', '（', '）', '—', '「', '」', '『', '』', '【', '】', '’', '‘', '”', '“', '；',
    '：', '《', '》', '？', '、', '。', '，', '·',
];

/// 官方正则 `/[^\x20-\x7e\u00C0-\u00FF\u4e00-\u9fa5 <中文标点>]|[<>&."]/` 的字符集语义
///
/// 名字里的 `display`:本函数判的是**官方显示名**的可用字符,与 `xml.rs` 的 `is_name_char`
/// (XML 名称字符)语义无关,故不重名。
fn is_display_name_char(c: char) -> bool {
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
pub(super) fn num(value: f64) -> Value {
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

/// 树里出现两次以上的 id(每个重复值报一次)
pub(super) fn duplicate_ids(tree: &model::BlockTree) -> Vec<String> {
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

/// 一个已反演完成的实体(反向编排在 `pipeline`,装配在本文件:字段对 `pub(super)` 可见)
pub(super) struct KnEntity {
    pub(super) source_id: String,
    pub(super) is_scene: bool,
    pub(super) tree: model::BlockTree,
    pub(super) source: serde_json::Map<String, serde_json::Value>,
}

/// KN 舞台尺寸(官方只有 `562×900` / `900×562` 两种;缺失时按竖屏)
pub(super) fn kn_stage_size(src: &serde_json::Map<String, serde_json::Value>) -> (f64, f64) {
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

/// 把 Kitten4 编辑器**不认识**的积木就地改成「未收录积木」标记,逐类记报告。
///
/// 两轮结论叠在一起才定成现在这样:
/// - `docs/rounds/34` §4nonies:编辑器遇到未知积木类型会让**整份工作区加载失败**
///   ⇒ 不能把不认识的名字原样写出去;
/// - `docs/rounds/38`:**但"剔除"会让积木真的消失**(id 口径实测:某件作品丢了 4 个可达块,
///   且 43 个占位映射因没有标题必然走到这一步)。
///
/// ⇒ 改成顶替成平台自己的 `incompatible_block`(语句位)/ `incompatible_output_block`(值位):
/// 两者都在编辑器注册表里、`args0` 为空、bundle 里的 JS 生成器是 `throw Error()`
/// (它本来就是"不可执行"的占位块)。位置与存在都保住:语句位**原地**渲染成「未收录积木」并留在链里;
/// 值位因平台块定义**没有 `output` 连接**而落成孤立块(槽位仍空,但块还在画布上)。
/// "哪个类型没认出来、有多少块"照旧逐类进报告 —— 效果从"悄悄少几块"变成"看得见的损失"。
fn mark_unknown_blocks(
    blocks: serde_json::Value,
    report: &mut TranslateReport,
) -> serde_json::Value {
    use serde_json::{Map, Value};
    use std::collections::{BTreeMap, BTreeSet};

    /// 编辑器认识的「未收录积木」标记(语句位 / 值位)
    const MARKER_STATEMENT: &str = "incompatible_block";
    const MARKER_OUTPUT: &str = "incompatible_output_block";

    let knows = super::kitten4_vocab::kitten4_editor_knows;

    // 就地消费(**rounds/37 P3**):入参本来就按值给到,原先却又 `as_object().cloned()` 整份拷一遍,
    // 再单独拷 `blocks`/`connections` 表、逐块拷 `shadows` —— 一份 9 MB 级文档因此被深拷 3~4 次。
    // 现在:拿走所有权 → 原地改。`serde_json::Map` 是 BTreeMap(按键排序),
    // 重建与插入顺序都不影响产物字节。
    let Value::Object(mut root) = blocks else {
        return blocks;
    };

    // 值槽里的子块要落成"值型"标记:`connections` 里 `input_type == "value"` 的那些;
    // 块自己带 `is_output: true` 也算。两处都不认时当语句块(标记块两种都没有连接时,
    // 编辑器一样会把它画成孤立块,只是形状不同)。
    let value_children: BTreeSet<String> = root
        .get("connections")
        .and_then(Value::as_object)
        .map(|connections| {
            connections
                .values()
                .filter_map(Value::as_object)
                .flat_map(|children| children.iter())
                .filter(|(_, info)| info.get("input_type").and_then(Value::as_str) == Some("value"))
                .map(|(child, _)| child.clone())
                .collect()
        })
        .unwrap_or_default();

    // 计数用 `BTreeMap`(键序稳定 ⇒ 告警顺序稳定)
    let mut marked: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(Value::Object(table)) = root.get_mut("blocks") {
        for (id, block) in table.iter_mut() {
            let Some(map) = block.as_object_mut() else {
                continue;
            };
            let Some(kind) = map.get("type").and_then(Value::as_str).map(str::to_string) else {
                continue;
            };
            if knows(&kind) {
                continue;
            }
            let output = map
                .get("is_output")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || value_children.contains(id);
            *marked.entry(kind).or_insert(0) += 1;
            map.insert(
                String::from("type"),
                Value::String(
                    if output {
                        MARKER_OUTPUT
                    } else {
                        MARKER_STATEMENT
                    }
                    .to_string(),
                ),
            );
            // 标记块 `args0` 为空 ⇒ 字段/影子/变异都不留(形态与平台一致)
            map.remove("fields");
            map.remove("shadows");
            map.remove("mutation");
            map.insert(String::from("is_output"), Value::Bool(output));
        }
    }
    // 块不再被移除 ⇒ 原来那段"重建 `connections` 去掉悬空引用"可以整段删掉:
    // 连接表保持原样,换个类型的块仍挂在原位置(值型标记连不上槽,编辑器会自己画成孤立块)。

    // 影子 XML 里也可能写着编辑器不认识的类型(它是**字符串**,清积木表时扫不到)。
    // 实测某作品 9696 条影子里 174 条如此(`get_split_options` 占 158)⇒ 一并清成空串
    // (库里既有的占位写法),并逐类计入报告。
    // 先探测"这一块到底有没有不认识的影子",没有就整块跳过(原先无条件 `shadows.clone()` 两遍)。
    //
    // 影子**不换标记块**:影子是槽位的默认值,不是用户摆的积木;换成一个"未收录积木"影子没有意义
    // (它没有字段可表达默认值),所以照旧清空 —— 这是 rounds/34 §4quinquies 记的 D2 行为。
    let mut shadow_fixed: BTreeMap<String, u64> = BTreeMap::new();
    if let Some(Value::Object(blocks)) = root.get_mut("blocks") {
        for block in blocks.values_mut() {
            let Some(map) = block.as_object_mut() else {
                continue;
            };
            let needs_fix = map
                .get("shadows")
                .and_then(Value::as_object)
                .is_some_and(|shadows| {
                    shadows
                        .values()
                        .any(|xml| shadow_type(xml).is_some_and(|kind| !knows(kind)))
                });
            if !needs_fix {
                continue;
            }
            let Some(Value::Object(shadows)) = map.remove("shadows") else {
                continue;
            };
            let mut new_shadows = Map::new();
            for (slot, xml) in shadows {
                match shadow_type(&xml) {
                    Some(kind) if !knows(kind) => {
                        *shadow_fixed.entry(kind.to_string()).or_insert(0) += 1;
                        new_shadows.insert(slot, Value::String(String::new()));
                    }
                    _ => {
                        new_shadows.insert(slot, xml);
                    }
                }
            }
            map.insert("shadows".into(), Value::Object(new_shadows));
        }
    }

    for (kind, count) in shadow_fixed {
        report.warn(TranslateWarning::UnmappedBlock {
            kind: format!("{kind}(Kitten4 编辑器不认识,已清空 {count} 条影子)"),
        });
    }
    for (kind, count) in marked {
        report.warn(TranslateWarning::UnmappedBlock {
            kind: format!("{kind}(Kitten4 编辑器不认识,已改成未收录积木 {count} 块)"),
        });
    }
    Value::Object(root)
}

/// 影子 XML 串里的 `type="…"` 取值(没有则 `None`)
fn shadow_type(xml: &serde_json::Value) -> Option<&str> {
    let text = xml.as_str()?;
    let start = text.find("type=\"")? + 6;
    let rest = &text[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

/// 装配阶段需要的舞台口径(打包传参:横竖屏 + 画布尺寸 + KN 舞台尺寸)
pub(super) struct StageSize {
    pub(super) landscape: bool,
    pub(super) canvas: (f64, f64),
    pub(super) kn: (f64, f64),
}

/// 装配 Kitten4 编辑版文档
pub(super) fn build_kitten4_document(
    src: &serde_json::Map<String, serde_json::Value>,
    // `entities` / `blocks_by_entity` 都**按值**收:装配阶段要移动实体源对象与
    // 已编码的积木数据,旧实现每个实体各 clone 一次(含整份 `nekoBlockJsonList`)
    // —— 一份文档级别的白拷贝(方案 23 P0-3)
    entities: Vec<KnEntity>,
    blocks_by_entity: Vec<(usize, serde_json::Value)>,
    stage: StageSize,
    // `theatre.groups` 的组 id 由它现铸(`deterministic_ids` 下稳定,见该段的说明)
    ids: &mut model::IdSource,
    report: &mut TranslateReport,
) -> serde_json::Value {
    use serde_json::{Map, Value, json};

    let StageSize {
        landscape,
        canvas,
        kn: kn_stage,
    } = stage;

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
        // (实机实测:80 种类型里 20 种不认识 ⇒ 画布一块都不显示)。所以写出前把不认识的块
        // 就地改成编辑器认识的「未收录积木」标记(而不是删掉),并逐类记进报告。
        let blocks = mark_unknown_blocks(blocks, report);
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

    // ── `theatre.groups` + 场景 `group_order`:**必须合成**,否则编辑器一个角色都不显示
    //
    // 实机现象(rounds/34 §4nonies):转换产物在 Kitten4 编辑器里作品名/变量能进,但**角色一个都不出现**
    // —— 平台原件的 `theatre.groups` 是"每组一个角色、组上带 `scene` 归属"的表,场景的 `group_order`
    // 列本场景的组 id;我们的写出器原先这两处分别是 `{}` / `[]`(KN 侧确实没有分组概念)。
    // 这里按"一角色一组"合成:组 id 现铸(`IdSource`,`deterministic_ids` 下稳定),
    // 组的 `scene` 指向角色所属场景,`group_order` 按 `actorIds` 顺序列出组 id。
    let mut groups = Map::new();
    let mut group_order_by_scene: Map<String, Value> = Map::new();
    let mut grouped: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (scene_key, scene) in scenes.iter() {
        let mut order: Vec<Value> = Vec::new();
        if let Some(actor_ids) = scene.get("actors").and_then(Value::as_array) {
            for actor_id in actor_ids.iter().filter_map(Value::as_str) {
                order.push(Value::String(synthesize_group(
                    &mut groups,
                    ids,
                    actor_id,
                    scene_key,
                )));
                grouped.insert(actor_id.to_string());
            }
        }
        group_order_by_scene.insert(scene_key.clone(), Value::Array(order));
    }
    // 没被任何场景 `actorIds` 指认的角色(装配阶段兜底挂到了"第一个场景")也要有条组,否则它不显示
    for (actor_id, actor) in actors.iter() {
        if grouped.contains(actor_id) {
            continue;
        }
        let Some(scene_key) = actor
            .get("scene")
            .and_then(Value::as_str)
            .filter(|key| scenes.contains_key(*key))
            .map(str::to_string)
        else {
            continue;
        };
        let group_id = synthesize_group(&mut groups, ids, actor_id, &scene_key);
        if let Some(Value::Array(order)) = group_order_by_scene.get_mut(&scene_key) {
            order.push(Value::String(group_id));
        }
    }
    for (scene_key, order) in group_order_by_scene {
        if let Some(scene) = scenes.get_mut(&scene_key) {
            scene["group_order"] = order;
        }
    }

    let mut theatre = Map::new();
    theatre.insert("scenes_order".into(), Value::Array(scenes_order));
    theatre.insert("scenes".into(), Value::Object(scenes));
    theatre.insert("actors".into(), Value::Object(actors));
    theatre.insert("styles".into(), Value::Object(kitten4_styles(src, report)));
    theatre.insert("videos".into(), json!({}));
    theatre.insert("groups".into(), Value::Object(groups));
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
    doc.insert(
        "hidden_toolbox".into(),
        json!({ "toolbox": [], "blocks": [] }),
    );
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

/// 合成一条 Kitten4 "单角色组"(`theatre.groups` 的条目形态照平台原件,组 id 现铸)
///
/// 编辑器靠这张表枚举"场景里有哪些角色";KN 侧没有分组概念,所以反向按"一角色一组"合成
/// (见 `build_kitten4_document` 里 `theatre.groups` 那段的说明)。
fn synthesize_group(
    groups: &mut serde_json::Map<String, serde_json::Value>,
    ids: &mut model::IdSource,
    actor_id: &str,
    scene_id: &str,
) -> String {
    use serde_json::json;
    let id = ids.uuid();
    groups.insert(
        id.clone(),
        json!({
            "actors": [actor_id],
            "id": id,
            "is_fold": false,
            "is_group": false,
            "name": "",
            "scene": scene_id,
            "visible": true,
        }),
    );
    id
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
    // 这里写的 `actors` 就是正向要照抄的那份。`group_order` 先占位,真正的值由装配阶段按
    // 合成出来的 `theatre.groups` 填(编辑器靠它 + groups 枚举角色,空着就一个角色都不显示)。
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

    /// 编辑器不认识的块**就地改成「未收录积木」标记**,而不是被删掉(rounds/38):
    /// 语句位 → `incompatible_block`;值槽子块(看 `connections` 的 `input_type`)→
    /// `incompatible_output_block`;字段/影子/变异清空;位置与连接保持;逐类计数进报告。
    #[test]
    fn unknown_blocks_become_incompatible_markers() {
        let mut report = report();
        let blocks = json!({
            "blocks": {
                "a": { "id": "a", "type": "temporary_list", "is_output": true,
                       "fields": { "list": "list-1" },
                       "shadows": { "list": "<shadow type=\"text\" id=\"s\"/>" },
                       "mutation": "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"0\"/>",
                       "location": [1.0, 2.0] },
                "b": { "id": "b", "type": "script_variables", "is_output": false, "fields": {} },
                "c": { "id": "c", "type": "text", "is_output": true, "fields": { "TEXT": "hi" } },
                "d": { "id": "d", "type": "list_item", "is_output": false }
            },
            "connections": {
                "c": { "d": { "input_name": "value", "input_type": "value", "type": "input" } }
            }
        });
        let out = mark_unknown_blocks(blocks, &mut report);
        let got = out["blocks"].as_object().expect("blocks");
        // 认得的原样;不认识的按位置换标记
        assert_eq!(got["c"]["type"], "text");
        assert_eq!(got["a"]["type"], "incompatible_output_block", "值型");
        assert_eq!(got["b"]["type"], "incompatible_block", "语句型");
        assert_eq!(
            got["d"]["type"], "incompatible_output_block",
            "自己没标 is_output,但它在值槽里 ⇒ 值型"
        );
        // 位置与连接保持原样(块还在原位,不再有"连接悬空"需要清理)
        assert_eq!(got["a"]["location"], json!([1.0, 2.0]));
        assert_eq!(out["connections"]["c"]["d"]["input_name"], "value");
        // 标记块 `args0` 为空 ⇒ 字段/影子/变异都不留
        for id in ["a", "b", "d"] {
            assert!(got[id].get("fields").is_none(), "{id}: 不留字段");
            assert!(got[id].get("shadows").is_none(), "{id}: 不留影子");
            assert!(got[id].get("mutation").is_none(), "{id}: 不留变异");
        }
        // 逐类计数(报告按类型排序)
        let kinds: Vec<&str> = report
            .warnings()
            .iter()
            .filter_map(|warning| match warning {
                TranslateWarning::UnmappedBlock { kind } => Some(kind.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "list_item(Kitten4 编辑器不认识,已改成未收录积木 1 块)",
                "script_variables(Kitten4 编辑器不认识,已改成未收录积木 1 块)",
                "temporary_list(Kitten4 编辑器不认识,已改成未收录积木 1 块)",
            ]
        );
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
