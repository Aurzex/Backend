use super::model::IdSource;
use super::nemo_mapping::NEMO_BCM_VERSION;
use super::nemo_mapping::{
    NemoEntity, NemoParseContext, NemoSubject, PROCEDURE_NORMAL, nemo_parse_procedures,
    translate_nemo_to_kn,
};
use super::options::{TranslateError, TranslateOptions};
use super::report::{TranslateReport, TranslateWarning};
use super::xml::{XmlNode, count_source_elements, parse_fragment};
use crate::core::convert::shared::DecompilerError;
use serde_json::{Map, Value, json};

// 来自 src/core/convert/translate/nemo.rs
// NEMO 编辑版 → KN 编辑版(官方 `nemoBcmToNekoBcmUtils`,即 bundle 模块 41888 里的 `gI`)。
// 本文件是 NEMO 方向的三层里的**文档级管线**(骨架、版本迁移、预改写、资源 url、变量/舞台归一),
// 与 Kitten 方向的 [`super::assembly`] 同层:
// | 层 | Kitten4 → KN | NEMO → KN |
// | -- | ------------ | --------- |
// | 前端 | [`super::model::parse_block_data_json`] | [`parse_blocks_xml`](本文件,`<root>` 包装 → 节点) |
// | 语义 | [`super::mapping::translate_kitten_to_kn`] | [`super::nemo_mapping::translate_nemo_to_kn`] |
// | 后端 | [`super::neko`] 三件套 | 不需要:NEMO 的程序集在官方解析器内就位(见 `nemo_mapping`) |
// | 装配 | [`super::assembly::build_document`] | [`convert_nemo_document`](本文件) |
// 官方管线(`gI`,bundle 偏移 ~6000071)顺序,逐步对应:
// 1. **版本迁移**(可选):`bcm_version < 0.9.4` → QC(角色 rotation 取反 + 旧音频块 XML 重写 +
// 变量坐标按舞台中心还原),`< 0.15.0` → YC(`controls_if` 补 `else="1"` 变异);
// 2. 新建 KN 骨架(`scenes/styles/version/stageSize/variables/broadcasts/actors/audios/procedures/previewUrl`);
// 3. `procedure_dict` → `proceduresDict`(见 [`super::nemo_mapping::nemo_parse_procedures`]);
// 4. 逐 actor / scene 解析 `blocksXML`(前置改写 + [`translate_nemo_to_kn`](super::nemo_mapping::translate_nemo_to_kn));
// 5. 场景命名(`屏幕<序号>` / `name=背景`)与排序;
// 6. 音频 url 规则(`https` 前缀 / `ext=mid` 置空);
// 7. 造型 url 规则(`texture` 走 CDN 前缀、`.webp` 追加 `?imageView2/0/format/png`、`center_point`→`centerPoint`);
// 8. broadcast 按场景重组(`{sceneId: [name…]}`);
// 9. 变量:坐标按舞台中心平移(向上取整前的 `x + w/2`、`h/2 - y`)、类型映射(`private`→`any`)、
// 重名去重、`createTime` 兜底;
// 10. `stageSize` 归一(只两种画布)、`timerPosition`(有 `timer` 变量时);
// 11. `extension = toolbox.devices`、`projectName = project_name`。
// **双形态**:官方产物保留源的 snake_case 字段(`current_style_id`、`x/y/rotation/scale`…)并**并行**加
// camelCase(`position`/`currentStyleId`/`nekoBlockJsonList`/`workspaceScrollXy`/`actorIds`/`centerPoint`);
// `blocksXML` 也原样保留 —— **除了**走过版本迁移的作品:那时官方把整份实体 XML 重新序列化后写回,
// 所以产物里的 `blocksXML` 是迁移后的形态(自闭合标签 + 插入的变异),这里同样产出。
// 已知偏差(逐条都能解释,`nemo_tests` 的 allow-list 是同一份口径):
// - **遍历顺序**:官方按 JS 对象的**插入顺序**遍历 `actors_dict`/`scenes_dict`/`variable_dict`/
// `audios.sounds`/`broadcast_dict`;`serde_json` 的 `Map` 是 BTreeMap(**按键排序**)⇒ 我们按 id 排序。
// 可观测差异有三处,都是"数组顺序"或"重名后缀":`audios.sortList`、`broadcasts.broadcastsDict.<场景>`
// 的数组先后,以及**重名变量**的去重后缀(`vI`)。两边的**集合/内容完全相同**;两份真作品里变量名
// 无重复(已核),`audios.sortList` 与 broadcasts 只差先后(已在 allow-list 里逐条记录)。
// 要逐序对齐需要一个保序的输入解析(现用 `serde_json::Value`,顺序在解析时就丢了)。
// - 官方 `QC` 的"旧音频块重写"是**字符串正则**(要求 `<field name="audio">` 紧跟块开标签);
// 这里按 DOM 等价写法实现(块里存在直接子 `field[name=audio]` 时重写),对真实作品等价。

/// 官方 `mI` / `yI`:竖屏画布(缺省与 `stageSize` 的两个取值之一)
const PORTRAIT_WIDTH: f64 = 562.0;
const PORTRAIT_HEIGHT: f64 = 900.0;
/// 官方 `fI` / `aI`:新实体的工作区滚动位置
const WORKSPACE_SCROLL: (i64, i64) = (100, 50);
/// 官方 `JC.stage_size.portrait`:QC 迁移用的编辑器竖屏常量(`qC.stage_size.portrait`)
const QC_PORTRAIT: (f64, f64) = (562.0, 900.0);
/// 官方 `p`:NEMO 资源站前缀(`nemo/22/`)
const NEMO_ASSET_PREFIX: &str = "https://static.codemao.cn/nemo/22/";

/// NEMO 编辑版 → KN 编辑版(纯文档级管线)
///
/// `source` 只读:所有迁移/改写都作用在**派生**值上(不改源文档),与 Kitten 路径"转换不改源文档"一致。
pub(crate) fn convert_nemo_document(
    source: &Value,
    options: &TranslateOptions,
    report: &mut TranslateReport,
) -> Result<Value, TranslateError> {
    // 计时(rounds/37 §0.5):基准表的 `core ms` 取自 `report.elapsed_ms`,此前 NEMO 侧从不设它(恒 0)
    let started = std::time::Instant::now();
    let (qc, yc) = migration_flags(options.source_version_ref());
    let deterministic = options.ids_deterministic();
    let now_ms = if deterministic {
        0
    } else {
        super::assembly::current_epoch_ms()
    };
    let mut ids = IdSource::new(deterministic);

    // ── 骨架(官方 `h`)
    let mut document = Map::new();
    document.insert(
        "scenes".to_string(),
        json!({ "scenesDict": {}, "currentSceneId": "", "sortList": [""] }),
    );
    document.insert("styles".to_string(), json!({ "stylesDict": {} }));
    document.insert("version".to_string(), Value::String(String::new()));
    document.insert(
        "stageSize".to_string(),
        json!({ "width": PORTRAIT_WIDTH, "height": PORTRAIT_HEIGHT }),
    );
    document.insert("variables".to_string(), json!({ "variablesDict": {} }));
    document.insert("broadcasts".to_string(), json!({ "broadcastsDict": {} }));
    document.insert("actors".to_string(), json!({ "actorsDict": {} }));
    document.insert(
        "audios".to_string(),
        json!({ "audiosDict": {}, "sortList": [], "currentAudioId": "" }),
    );
    document.insert("procedures".to_string(), json!({ "proceduresDict": {} }));
    document.insert("previewUrl".to_string(), Value::String(String::new()));

    // ── 解析上下文(广播字典:官方 `getBroadcastMessage` 用)
    let mut ctx = NemoParseContext::default();
    if let Some(dict) = source
        .pointer("/broadcast/broadcast_dict")
        .and_then(Value::as_object)
    {
        ctx.has_broadcasts = !dict.is_empty();
        for (id, entry) in dict {
            let name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            ctx.broadcast_names.insert(id.clone(), name);
        }
    }
    let split_option_names = split_option_names(source);

    // ── 程序集(官方第 3 步;必须在演员/场景之前,解析期查表要用)
    if let Some(dict) = source.pointer("/procedures/procedure_dict") {
        let parsed = nemo_parse_procedures(dict, &mut ctx, &mut ids, report);
        let mut procedures = Map::new();
        for procedure in parsed {
            let mut entry = Map::new();
            entry.insert("id".to_string(), Value::String(procedure.id.clone()));
            entry.insert("name".to_string(), Value::String(procedure.name.clone()));
            entry.insert(
                "type".to_string(),
                Value::String(if procedure.kind.is_empty() {
                    PROCEDURE_NORMAL.to_string()
                } else {
                    procedure.kind.clone()
                }),
            );
            entry.insert(
                "params".to_string(),
                Value::Array(
                    procedure
                        .params
                        .iter()
                        .map(|param| {
                            json!({
                                "id": param.id,
                                "type": param.kind,
                                "name": param.name,
                                "parent_id": param.parent_id,
                                "parent_type": param.parent_type,
                            })
                        })
                        .collect(),
                ),
            );
            entry.insert(
                "nekoBlockJsonList".to_string(),
                Value::Array(tree_to_json(&procedure.tree)),
            );
            entry.insert(
                "workspaceScrollXy".to_string(),
                json!({ "x": WORKSPACE_SCROLL.0, "y": WORKSPACE_SCROLL.1 }),
            );
            report.blocks_converted += procedure.tree.count();
            procedures.insert(procedure.key, Value::Object(entry));
        }
        document.insert(
            "procedures".to_string(),
            json!({ "proceduresDict": Value::Object(procedures) }),
        );
    } else {
        document.insert("procedures".to_string(), json!({}));
    }

    // ── 演员(官方第 4 步;先演员后场景)
    let scenes_order = scenes_order(source);
    let mut actors = Map::new();
    if let Some(dict) = source
        .pointer("/actors/actors_dict")
        .and_then(Value::as_object)
    {
        for (id, actor) in dict {
            let Some(actor) = actor.as_object() else {
                continue;
            };
            let entity = NemoEntity {
                id: id.clone(),
                styles: style_ids_value(actor),
            };
            let (tree, migrated_xml) = parse_entity(
                actor,
                NemoSubject::Entity(&entity),
                &mut ctx,
                &mut ids,
                report,
                &split_option_names,
                &scenes_order,
                qc,
                yc,
            );
            let mut entry = actor.clone();
            if qc {
                // 官方 `QC`:角色 rotation 取反(0 / 缺失 / NaN 是 falsy,跳过)
                if let Some(rotation) = actor.get("rotation").filter(|value| truthy_value(value)) {
                    let negated = rotation.as_f64().map(|value| number_value(-value));
                    if let Some(negated) = negated {
                        entry.insert("rotation".to_string(), negated);
                    }
                }
            }
            entry.insert(
                "locked".to_string(),
                Value::Bool(actor.get("locked").map(truthy_value).unwrap_or(false)),
            );
            if let Some(xml) = migrated_xml {
                entry.insert("blocksXML".to_string(), Value::String(xml));
            }
            entry.insert("position".to_string(), position_of(actor));
            entry.insert("scale".to_string(), floored(actor.get("scale")));
            insert_if_present(&mut entry, "currentStyleId", actor.get("current_style_id"));
            entry.insert(
                "nekoBlockJsonList".to_string(),
                Value::Array(tree_to_json(&tree)),
            );
            entry.insert(
                "workspaceScrollXy".to_string(),
                json!({ "x": WORKSPACE_SCROLL.0, "y": WORKSPACE_SCROLL.1 }),
            );
            report.blocks_converted += tree.count();
            actors.insert(id.clone(), Value::Object(entry));
        }
    }
    document.insert(
        "actors".to_string(),
        json!({ "actorsDict": Value::Object(actors) }),
    );

    // ── 场景
    let mut scenes = Map::new();
    if let Some(dict) = source
        .pointer("/scenes/scenes_dict")
        .and_then(Value::as_object)
    {
        for (id, scene) in dict {
            let Some(scene) = scene.as_object() else {
                continue;
            };
            let entity = NemoEntity {
                id: id.clone(),
                styles: style_ids_value(scene),
            };
            let (tree, migrated_xml) = parse_entity(
                scene,
                NemoSubject::Entity(&entity),
                &mut ctx,
                &mut ids,
                report,
                &split_option_names,
                &scenes_order,
                qc,
                yc,
            );
            let mut entry = scene.clone();
            if let Some(xml) = migrated_xml {
                entry.insert("blocksXML".to_string(), Value::String(xml));
            }
            entry.insert("screenName".to_string(), Value::String(String::new()));
            entry.insert("actorIds".to_string(), Value::Array(Vec::new()));
            entry.insert("currentStyleId".to_string(), Value::String(String::new()));
            entry.insert(
                "nekoBlockJsonList".to_string(),
                Value::Array(tree_to_json(&tree)),
            );
            entry.insert(
                "workspaceScrollXy".to_string(),
                json!({ "x": WORKSPACE_SCROLL.0, "y": WORKSPACE_SCROLL.1 }),
            );
            entry.insert("name".to_string(), Value::String("背景".to_string()));
            let index = scenes_order.iter().position(|order| order == id);
            entry.insert(
                "screenName".to_string(),
                Value::String(format!("屏幕{}", index.map_or(0, |at| at + 1))),
            );
            if let Some(Value::Array(list)) = scene.get("actors") {
                let mut reversed = list.clone();
                reversed.reverse();
                entry.insert("actorIds".to_string(), Value::Array(reversed));
            }
            if let Some(style) = scene
                .get("current_style_id")
                .filter(|value| truthy_value(value))
            {
                entry.insert("currentStyleId".to_string(), style.clone());
            }
            report.blocks_converted += tree.count();
            scenes.insert(id.clone(), Value::Object(entry));
        }
    }
    let mut scene_container = Map::new();
    scene_container.insert("scenesDict".to_string(), Value::Object(scenes));
    if !scenes_order.is_empty() {
        scene_container.insert(
            "currentSceneId".to_string(),
            Value::String(scenes_order[0].clone()),
        );
        scene_container.insert(
            "sortList".to_string(),
            Value::Array(
                scenes_order
                    .iter()
                    .map(|id| Value::String(id.clone()))
                    .collect(),
            ),
        );
    }
    document.insert("scenes".to_string(), Value::Object(scene_container));

    // ── 音频(官方第 6 步)
    let mut audio_ids: Vec<Value> = Vec::new();
    let mut audios = Map::new();
    if let Some(dict) = source.pointer("/audios/sounds").and_then(Value::as_object) {
        for (id, sound) in dict {
            let Some(entry) = sound.as_object() else {
                continue;
            };
            let mut item = entry.clone();
            let ext = entry.get("ext").and_then(Value::as_str).unwrap_or_default();
            let url = if ext == "mid" {
                String::new()
            } else {
                let raw = entry.get("url").and_then(Value::as_str).unwrap_or_default();
                if raw.contains("https") {
                    raw.to_string()
                } else {
                    format!("{NEMO_ASSET_PREFIX}{raw}")
                }
            };
            insert_if_present(&mut item, "id", entry.get("id"));
            insert_if_present(&mut item, "name", entry.get("name"));
            item.insert("url".to_string(), Value::String(url));
            if ext != "mid" {
                audio_ids.push(
                    entry
                        .get("id")
                        .cloned()
                        .unwrap_or_else(|| Value::String(id.clone())),
                );
            }
            audios.insert(id.clone(), Value::Object(item));
        }
    }
    let current_audio = audio_ids
        .first()
        .cloned()
        .unwrap_or(Value::String(String::new()));
    document.insert(
        "audios".to_string(),
        json!({
            "audiosDict": Value::Object(audios),
            "sortList": Value::Array(audio_ids),
            "currentAudioId": current_audio,
        }),
    );

    // ── 造型(官方第 7 步)
    let mut styles = Map::new();
    if let Some(dict) = source
        .pointer("/styles/styles_dict")
        .and_then(Value::as_object)
    {
        for (id, style) in dict {
            let Some(entry) = style.as_object() else {
                continue;
            };
            let mut item = entry.clone();
            if let Some(texture) = entry.get("texture").and_then(Value::as_str) {
                item.insert(
                    "url".to_string(),
                    Value::String(format!("{NEMO_ASSET_PREFIX}{texture}")),
                );
            }
            if let Some(url) = item.get("url").and_then(Value::as_str) {
                let fixed =
                    if !url.ends_with(".webp?imageView2/0/format/png") && url.ends_with(".webp") {
                        let trimmed = url.strip_suffix(".webp").unwrap_or(url);
                        format!("{trimmed}.webp?imageView2/0/format/png")
                    } else {
                        url.to_string()
                    };
                item.insert("url".to_string(), Value::String(fixed));
            }
            if let Some(center) = item.remove("center_point") {
                item.insert("centerPoint".to_string(), center);
            }
            styles.insert(id.clone(), Value::Object(item));
        }
    }
    document.insert(
        "styles".to_string(),
        json!({ "stylesDict": Value::Object(styles) }),
    );

    // ── 广播按场景重组(官方第 8 步)
    let mut broadcasts: Map<String, Value> = Map::new();
    if let Some(dict) = source
        .pointer("/broadcast/broadcast_dict")
        .and_then(Value::as_object)
    {
        for entry in dict.values() {
            let scene = entry
                .get("scene")
                .and_then(Value::as_str)
                .unwrap_or("undefined")
                .to_string();
            let name = entry
                .get("name")
                .cloned()
                .unwrap_or_else(|| Value::String("undefined".to_string()));
            broadcasts
                .entry(scene)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("broadcasts 入参是数组")
                .push(name);
        }
    }
    document.insert(
        "broadcasts".to_string(),
        json!({ "broadcastsDict": Value::Object(broadcasts) }),
    );

    // ── 变量(官方第 9 步)
    let stage_width = source.pointer("/stage_size/width").and_then(Value::as_f64);
    let stage_height = source.pointer("/stage_size/height").and_then(Value::as_f64);
    let mut variables = Map::new();
    let mut variable_names: Vec<String> = Vec::new();
    let mut list_names: Vec<String> = Vec::new();
    if let Some(dict) = source
        .pointer("/variable/variable_dict")
        .and_then(Value::as_object)
    {
        for (key, variable) in dict {
            let Some(entry) = variable.as_object() else {
                continue;
            };
            // 官方 `QC`:变量坐标先按**编辑器竖屏常量**(562×900)还原成 NEMO 自身坐标系;
            // 随后主管线再按文档 `stage_size` 重定心(两者在竖屏文档下正好抵消)。
            let has_position = entry.get("position").is_some_and(truthy_value);
            let mut x = entry
                .get("position")
                .and_then(|position| position.get("x"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            let mut y = entry
                .get("position")
                .and_then(|position| position.get("y"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            if qc && has_position {
                x -= QC_PORTRAIT.0 / 2.0;
                y = -y + QC_PORTRAIT.1 / 2.0;
            }
            let position = json!({
                "x": x + stage_width.unwrap_or(PORTRAIT_WIDTH) / 2.0,
                "y": stage_height.unwrap_or(PORTRAIT_HEIGHT) / 2.0 - y,
            });
            let raw_type = entry
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let mut name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if raw_type == "list" {
                name = dedupe_name(&list_names, &name);
                list_names.push(name.clone());
            } else {
                name = dedupe_name(&variable_names, &name);
                variable_names.push(name.clone());
            }
            let mut item = Map::new();
            insert_if_present(&mut item, "id", entry.get("id"));
            item.insert(
                "type".to_string(),
                Value::String(if raw_type == "private" {
                    "any".to_string()
                } else {
                    raw_type.to_string()
                }),
            );
            item.insert("name".to_string(), Value::String(name));
            item.insert(
                "value".to_string(),
                if raw_type == "public" || raw_type == "private" {
                    Value::from(0)
                } else {
                    entry.get("value").cloned().unwrap_or(Value::Null)
                },
            );
            insert_if_present(&mut item, "visible", entry.get("visible"));
            item.insert("position".to_string(), position);
            insert_if_present(&mut item, "isGlobal", entry.get("is_global"));
            let create_time = entry
                .get("create_time")
                .filter(|value| truthy_value(value))
                .cloned()
                .unwrap_or_else(|| Value::from(now_ms as f64));
            item.insert("createTime".to_string(), create_time);
            insert_if_present(&mut item, "scale", entry.get("scale"));
            item.insert("style".to_string(), Value::String("default".to_string()));
            insert_if_present(&mut item, "currentEntityId", entry.get("current_entity"));
            variables.insert(key.clone(), Value::Object(item));
        }
    }
    document.insert(
        "variables".to_string(),
        json!({ "variablesDict": Value::Object(variables) }),
    );

    // ── 舞台尺寸归一(官方第 10 步)
    let width = stage_width.unwrap_or(PORTRAIT_WIDTH);
    let height = stage_height.unwrap_or(PORTRAIT_HEIGHT);
    document.insert(
        "stageSize".to_string(),
        if width > height {
            json!({ "width": PORTRAIT_HEIGHT, "height": PORTRAIT_WIDTH })
        } else {
            json!({ "width": PORTRAIT_WIDTH, "height": PORTRAIT_HEIGHT })
        },
    );

    // ── 计时器位置(有 `timer` 变量时)
    let has_timer = source
        .pointer("/variable/variable_dict/timer/type")
        .and_then(Value::as_str)
        == Some("timer");
    if has_timer {
        document.insert(
            "timerPosition".to_string(),
            json!({ "x": stage_width.unwrap_or(PORTRAIT_WIDTH) - 160.0 - 20.0, "y": 12.0 }),
        );
    }

    // ── 扩展(官方第 11 步)
    if let Some(toolbox) = source.get("toolbox").and_then(Value::as_object) {
        match toolbox.get("devices") {
            Some(Value::Array(devices)) => {
                document.insert("extension".to_string(), Value::Array(devices.clone()));
            }
            Some(Value::String(device)) => {
                document.insert(
                    "extension".to_string(),
                    Value::Array(vec![Value::String(device.clone())]),
                );
            }
            _ => {}
        }
    }

    // `"project_name" in c ? c.project_name : "新的作品"`
    let project_name = source
        .get("project_name")
        .cloned()
        .unwrap_or_else(|| Value::String("新的作品".to_string()));
    document.insert("projectName".to_string(), project_name);

    // ── 数字归一:`JSON.stringify` 把"整数值的浮点"打印成整数(JS 没有 int/float 之分),
    // 官方产物里的 `stageSize`/`timerPosition`/变量坐标因此都是整数形态;这里统一到同一口径。
    let mut document = Value::Object(document);
    normalize_integral_numbers(&mut document);
    report.elapsed_ms = started.elapsed().as_millis();
    Ok(document)
}

/// 解析一个实体的 `blocksXML`(含版本迁移与前置改写)
#[allow(clippy::too_many_arguments)]
fn parse_entity(
    entity: &Map<String, Value>,
    subject: NemoSubject<'_>,
    ctx: &mut NemoParseContext,
    ids: &mut IdSource,
    report: &mut TranslateReport,
    split_options: &Map<String, Value>,
    scenes_order: &[String],
    qc: bool,
    yc: bool,
) -> (super::model::BlockTree, Option<String>) {
    let Some(xml) = entity.get("blocksXML").and_then(Value::as_str) else {
        return (super::model::BlockTree::default(), None);
    };
    let roots = match prepare_blocks_xml(xml, qc, yc, split_options, scenes_order, ids) {
        Ok((roots, source_elements, migrated_xml)) => {
            // 计数口径:源 XML 里的 `block`/`shadow`/`empty` 元素数(与 `temp/harness/tables-output.txt`
            // 的 "XML elements: block=… shadow=… empty=…" 同一口径,前置改写**之前**数)
            report.blocks_total += source_elements;
            return (
                translate_nemo_to_kn(&roots, subject, ctx, ids, report),
                migrated_xml,
            );
        }
        Err(error) => {
            // 官方这里会拿到 `parsererror` 文档并静默产出空树;我们记一条报告后跳过本体。
            report.warn(TranslateWarning::DroppedField {
                path: format!("blocksXML: {error}"),
            });
            return (super::model::BlockTree::default(), None);
        }
    };
}

/// 前置改写:版本迁移(QC/YC)+ 官方 `parseBlocksXML` 里的 9 个 `transform*`
///
/// 官方顺序(逐条对应,改一次就重解析一次;这里在 DOM 上按同序实施):
/// `transformServoShadowType` → `transformMidiOnPlayNoteBlocks` → `transformDialShadowType` →
/// `transformMathArithmeticPower` → `addAlignFieldToStamp` → `addMutationToControlsIf` →
/// `addMutationToMobileText` → `transformLegacyAudioBlock` → `transformSelfSetEffect2Blocks`,
/// 随后每个根节点:`replaceIdsWithUUID` → `replaceSplitOptions` → `replaceSceneIndex`。
fn prepare_blocks_xml(
    xml: &str,
    qc: bool,
    yc: bool,
    split_options: &Map<String, Value>,
    scenes_order: &[String],
    ids: &mut IdSource,
) -> Result<(Vec<XmlNode>, usize, Option<String>), crate::core::convert::shared::DecompilerError> {
    let mut roots = parse_fragment(&format!("<root>{xml}</root>"))?;
    let source_elements = count_source_elements(&roots);
    if qc {
        qc_audio_blocks(&mut roots, ids);
    }
    if yc {
        yc_controls_if_mutation(&mut roots);
    }
    // 官方 `QC`/`YC` 是**字符串级**改写:它们把整份实体 XML 重新序列化(`innerHTML`)后写回
    // `blocksXML`,所以有迁移的作品里 `blocksXML` 不再是原文(而是迁移后的序列化形态)。
    // 这里在迁移步之后同步产出该字符串(后续前置改写不改它,官方也不改)。
    let migrated_xml = (qc || yc).then(|| roots.iter().map(XmlNode::serialize).collect::<String>());
    transform_servo_shadow_type(&mut roots);
    transform_midi_on_play_note_blocks(&mut roots);
    transform_dial_shadow_type(&mut roots);
    transform_math_arithmetic_power(&mut roots);
    add_align_field_to_stamp(&mut roots);
    add_mutation_to_controls_if(&mut roots);
    add_mutation_to_mobile_text(&mut roots);
    transform_legacy_audio_block(&mut roots, ids);
    transform_self_set_effect_2_blocks(&mut roots);
    for root in &mut roots {
        replace_ids_with_uuid(root, ids);
        replace_split_options(root, split_options);
        replace_scene_index(root, scenes_order);
    }
    Ok((roots, source_elements, migrated_xml))
}

// ---------------------------------------------------------------------------
// 版本迁移(官方 `QC` / `YC`,见 docs/rounds/27 §9.3)
// ---------------------------------------------------------------------------

/// 版本号按段比较(官方 `tI`):`a < b`
fn version_lt(a: &str, b: &str) -> Option<bool> {
    let left: Vec<&str> = a.split('.').collect();
    let right: Vec<&str> = b.split('.').collect();
    if left.len() != 3 || right.len() != 3 {
        return None;
    }
    for (l, r) in left.iter().zip(right.iter()) {
        if l != r {
            return Some(l.parse::<i64>().ok()? < r.parse::<i64>().ok()?);
        }
    }
    Some(false)
}

/// 官方 `rI`:把版本归到"迁移阶段键"(`<0.9.4` → `0.9.3`,`<0.15.0` → `0.14.0`,否则原值)
fn version_stage(version: &str) -> Option<String> {
    match version_lt(version, "0.9.4") {
        Some(true) => Some("0.9.3".to_string()),
        Some(false) => match version_lt(version, "0.15.0") {
            Some(true) => Some("0.14.0".to_string()),
            _ => Some(version.to_string()),
        },
        None => None,
    }
}

/// 逐级应用迁移,直到目标版本(`NEMO_BCM_VERSION`);返回 `(要不要 QC, 要不要 YC)`
///
/// 官方:
/// ```text
/// for (r = rI(bcmVersion); tI(r, qC.bcm_version); ) { s = {"0.9.3": QC, "0.14.0": YC}[r];
///   if (!s) break; o = s(doc); doc = o.new_bcm; r = rI(o.new_version) }
/// ```
/// `QC` 的产物版本是 `0.9.4`(再归段 → `0.14.0` → YC),`YC` 的是 `0.15.0`(再归段 → 自身 → 无迁移 → break)。
/// 版本号非法(段数不为 3)时官方会抛异常;这里跳过迁移并记一条警告。
fn migration_flags(version: Option<&str>) -> (bool, bool) {
    let Some(version) = version.filter(|value| !value.is_empty()) else {
        return (false, false);
    };
    let Some(mut stage) = version_stage(version) else {
        log::warn!("bcm_version {version:?} 段数不为 3,跳过 NEMO 版本迁移(官方此处会抛异常)");
        return (false, false);
    };
    let (mut qc, mut yc) = (false, false);
    // 官方 `for (r = rI(v); tI(r, qC.bcm_version); ) { s = {"0.9.3": QC, "0.14.0": YC}[r]; if (!s) break; … }`
    while version_lt(&stage, NEMO_BCM_VERSION) == Some(true) {
        let next_version = match stage.as_str() {
            "0.9.3" => {
                qc = true;
                "0.9.4"
            }
            "0.14.0" => {
                yc = true;
                "0.15.0"
            }
            _ => break,
        };
        match version_stage(next_version) {
            Some(next) => stage = next,
            None => break,
        }
    }
    (qc, yc)
}

/// 官方 `YC`(< 0.15.0):`controls_if` 首子元素不是 `<mutation>` 时插 `<mutation else="1"/>`
fn yc_controls_if_mutation(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block" && node.attr("type") == Some("controls_if") {
            let first_is_mutation = node
                .children
                .first()
                .is_some_and(|child| child.tag == "mutation");
            if !first_is_mutation {
                let mut mutation = XmlNode::new("mutation");
                mutation.set_attr("else", "1");
                node.children.insert(0, mutation);
            }
        }
        yc_controls_if_mutation(&mut node.children);
    }
}

/// 官方 `QC` 的音频块重写(< 0.9.4)
///
/// 三条等价改写(官方是字符串正则):
/// - `audio__stop_all_audios`(可带 `field[name=audio]`,缺省 `__all_sounds`)→
///   `<value name="audio"><shadow type="sound_get_all" …>…</shadow></value>`;
/// - `audio__play_audio` / `audio__play_audio_and_wait`(必须带 `field[name=audio]`)→
///   `<value name="audio"><shadow type="sound_get" …>…</shadow></value>`。
fn qc_audio_blocks(nodes: &mut [XmlNode], ids: &mut IdSource) {
    for node in nodes.iter_mut() {
        if node.tag == "block" {
            let raw = node.attr("type").unwrap_or_default().to_string();
            let shadow_type = match raw.as_str() {
                "audio__stop_all_audios" => Some(("sound_get_all", Some("__all_sounds"))),
                "audio__play_audio" | "audio__play_audio_and_wait" => Some(("sound_get", None)),
                _ => None,
            };
            if let Some((shadow_type, fallback)) = shadow_type {
                let field = node
                    .children
                    .iter()
                    .position(|child| child.tag == "field" && child.attr("name") == Some("audio"))
                    .map(|index| node.children.remove(index));
                if field.is_some() || raw == "audio__stop_all_audios" {
                    let text = field
                        .map(|field| field.text_content())
                        .or_else(|| fallback.map(str::to_string))
                        .unwrap_or_default();
                    let mut value = XmlNode::new("value");
                    value.set_attr("name", "audio");
                    let mut shadow = XmlNode::new("shadow");
                    shadow.set_attr("type", shadow_type);
                    shadow.set_attr("id", &ids.uuid());
                    shadow.set_attr("inline", "true");
                    shadow.set_attr("visible", "visible");
                    let mut field_node = XmlNode::new("field");
                    field_node.set_attr("name", "audio");
                    field_node.text = text;
                    shadow.children.push(field_node);
                    value.children.push(shadow);
                    node.children.insert(0, value);
                }
            }
        }
        qc_audio_blocks(&mut node.children, ids);
    }
}

// ---------------------------------------------------------------------------
// `parseBlocksXML` 的 9 个前置改写
// ---------------------------------------------------------------------------

/// 处理 `microbit_servo_set_angle_360` 的 `angle` 影子:类型换 `math_number_with_servo`、
/// 约束 `-135,135,1,`、数值夹到 `[-135, 135]`
fn transform_servo_shadow_type(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block"
            && node.attr("type") == Some("microbit_servo_set_angle_360")
            && let Some(shadow) = descendant_value_shadow_mut(node, "angle")
            && shadow.attr("type") == Some("math_number")
        {
            shadow.set_attr("type", "math_number_with_servo");
            if let Some(field) = find_named_mut(shadow, "field", "NUM") {
                field.set_attr("constraints", "-135,135,1,");
                let value = parse_int_prefix(&field.text_content()).unwrap_or(0);
                let clamped = value.clamp(-135, 135);
                field.text = clamped.to_string();
            }
        }
        transform_servo_shadow_type(&mut node.children);
    }
}

/// `midi__on_play_note` / `midi__on_play_section` 的 `statement[name=DO]` 降级成 `<next>`
/// (官方把语句槽拆成链式尾)
fn transform_midi_on_play_note_blocks(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block"
            && matches!(
                node.attr("type"),
                Some("midi__on_play_note") | Some("midi__on_play_section")
            )
            && let Some(index) = node
                .children
                .iter()
                .position(|child| child.tag == "statement" && child.attr("name") == Some("DO"))
        {
            let statement = node.children.remove(index);
            let mut next = XmlNode::new("next");
            next.children = statement.children;
            node.children.insert(index, next);
        }
        transform_midi_on_play_note_blocks(&mut node.children);
    }
}

/// `self_point_towards` 的 `degrees` 影子换 `math_number_with_dial`
fn transform_dial_shadow_type(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block"
            && node.attr("type") == Some("self_point_towards")
            && let Some(shadow) = descendant_value_shadow_mut(node, "degrees")
            && shadow.attr("type") == Some("math_number")
        {
            shadow.set_attr("type", "math_number_with_dial");
        }
        transform_dial_shadow_type(&mut node.children);
    }
}

/// `math_arithmetic_power` → `math_arithmetic_common` + 首个子 `<value>` 前插 `field[name=OP]=POWER`
fn transform_math_arithmetic_power(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block" && node.attr("type") == Some("math_arithmetic_power") {
            node.set_attr("type", "math_arithmetic_common");
            let mut field = XmlNode::new("field");
            field.set_attr("name", "OP");
            field.text = "POWER".to_string();
            let at = node
                .children
                .iter()
                .position(|child| child.tag == "value")
                .unwrap_or(node.children.len());
            node.children.insert(at, field);
        }
        transform_math_arithmetic_power(&mut node.children);
    }
}

/// `stamp`:补 `field[name=ALIGN]=CENTER`,并保证有 `mutation items="1"`
fn add_align_field_to_stamp(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block" && node.attr("type") == Some("stamp") {
            let has_align = node
                .children
                .iter()
                .any(|child| child.tag == "field" && child.attr("name") == Some("ALIGN"));
            if !has_align {
                let mut field = XmlNode::new("field");
                field.set_attr("name", "ALIGN");
                field.text = "CENTER".to_string();
                match node
                    .children
                    .iter()
                    .position(|child| child.tag == "mutation")
                {
                    Some(at) => node.children.insert(at + 1, field),
                    None => node.children.insert(0, field),
                }
            }
            match node
                .children
                .iter_mut()
                .find(|child| child.tag == "mutation")
            {
                Some(mutation) => mutation.set_attr("items", "1"),
                None => {
                    let mut mutation = XmlNode::new("mutation");
                    mutation.set_attr("items", "1");
                    node.children.insert(0, mutation);
                }
            }
        }
        add_align_field_to_stamp(&mut node.children);
    }
}

/// `controls_if` 有 `statement[name=ELSE]` 且没有 `<mutation>` 时补 `<mutation else="1"/>`(插成首子)
fn add_mutation_to_controls_if(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block" && node.attr("type") == Some("controls_if") {
            let has_else = node
                .children
                .iter()
                .any(|child| child.tag == "statement" && child.attr("name") == Some("ELSE"));
            let has_mutation = node.children.iter().any(|child| child.tag == "mutation");
            if has_else && !has_mutation {
                let mut mutation = XmlNode::new("mutation");
                mutation.set_attr("else", "1");
                node.children.insert(0, mutation);
            }
        }
        add_mutation_to_controls_if(&mut node.children);
    }
}

/// 官方 `addMutationToMobileText`(字符串替换):`mobile__text` 补 `<mutation items="1"/>`(插成首子)
fn add_mutation_to_mobile_text(nodes: &mut [XmlNode]) {
    for node in nodes.iter_mut() {
        if node.tag == "block" && node.attr("type") == Some("mobile__text") {
            let mut mutation = XmlNode::new("mutation");
            mutation.set_attr("items", "1");
            node.children.insert(0, mutation);
        }
        add_mutation_to_mobile_text(&mut node.children);
    }
}

/// 官方 `transformLegacyAudioBlock`:把旧写法 `<field name="audio">` 包成
/// `<value name="audio"><shadow type="sound_get" …>`(其它子元素全部丢弃,只留 `next`)
fn transform_legacy_audio_block(nodes: &mut [XmlNode], ids: &mut IdSource) {
    for node in nodes.iter_mut() {
        if node.tag == "block"
            && matches!(
                node.attr("type"),
                Some("audio__play_audio") | Some("audio__play_audio_and_wait")
            )
        {
            let field = node
                .children
                .iter()
                .position(|child| child.tag == "field" && child.attr("name") == Some("audio"));
            if let Some(index) = field {
                let text = node.children[index].text_content();
                let saved_next = node
                    .children
                    .iter()
                    .position(|child| child.tag == "next")
                    .map(|at| node.children.remove(at));
                node.children.clear();
                node.text.clear();
                let mut value = XmlNode::new("value");
                value.set_attr("name", "audio");
                let mut shadow = XmlNode::new("shadow");
                shadow.set_attr("type", "sound_get");
                shadow.set_attr("id", &ids.uuid());
                shadow.set_attr("visible", "visible");
                shadow.set_attr("inline", "true");
                let mut field_node = XmlNode::new("field");
                field_node.set_attr("name", "audio");
                field_node.text = text;
                shadow.children.push(field_node);
                value.children.push(shadow);
                node.children.push(value);
                match saved_next {
                    Some(next) => node.children.push(next),
                    None => {
                        let mut next = XmlNode::new("next");
                        next.set_attr("last_next_in_stack", "true");
                        node.children.push(next);
                    }
                }
                node.set_attr("visible", "visible");
            }
        }
        transform_legacy_audio_block(&mut node.children, ids);
    }
}

/// `self_set_effect_2` / `self_set_effect` 的 `val` 影子数值按 `scope` 夹取
fn transform_self_set_effect_2_blocks(nodes: &mut [XmlNode]) {
    /// 每个 `scope` 的取值范围与是否环绕(官方 `transformSelfSetEffect2Blocks` 的 `t`)
    const RANGES: [(i64, Option<i64>, bool); 7] = [
        (0, Some(360), true),
        (0, Some(100), false),
        (0, Some(100), false),
        (1, None, false),
        (0, Some(100), false),
        (0, Some(100), false),
        (0, Some(100), false),
    ];
    for node in nodes.iter_mut() {
        if node.tag == "block"
            && matches!(
                node.attr("type"),
                Some("self_set_effect_2") | Some("self_set_effect")
            )
        {
            let scope = find_named(node, "field", "scope")
                .map(|field| field.text_content().parse::<f64>().unwrap_or(0.0).trunc())
                .unwrap_or(0.0);
            let range = RANGES
                .get(usize::try_from(scope as i64).unwrap_or(0))
                .copied()
                .unwrap_or(RANGES[0]);
            if let Some(shadow) = descendant_value_shadow_mut(node, "val")
                && shadow.attr("type") == Some("math_number")
                && let Some(field) = find_named_mut(shadow, "field", "NUM")
                && let Ok(value) = field.text_content().trim().parse::<f64>()
            {
                field.text = clamp_effect(value, range).to_string();
            }
        }
        transform_self_set_effect_2_blocks(&mut node.children);
    }
}

/// 官方 `s(e, t)`(环绕 + 夹取)
fn clamp_effect(value: f64, range: (i64, Option<i64>, bool)) -> f64 {
    let (min, max, wrap) = range;
    if wrap && max == Some(360) && (0.0..=360.0).contains(&value) {
        return value;
    }
    if wrap && let Some(max) = max {
        if value < 0.0 {
            let mut value = value;
            let step = max as f64;
            while value < 0.0 {
                value += step;
            }
            return value;
        }
        return value % max as f64;
    }
    if value < min as f64 {
        return min as f64;
    }
    if let Some(max) = max
        && value > max as f64
    {
        return max as f64;
    }
    value
}

/// 官方 `replaceIdsWithUUID`:`block`/`shadow`/`statement`/`value` 换新 uuid(`empty` 不动)
fn replace_ids_with_uuid(node: &mut XmlNode, ids: &mut IdSource) {
    if matches!(
        node.tag.as_str(),
        "block" | "shadow" | "statement" | "value"
    ) {
        let id = ids.uuid();
        node.set_attr("id", &id);
    }
    for child in &mut node.children {
        replace_ids_with_uuid(child, ids);
    }
}

/// 官方 `replaceSplitOptions`:`text_split` 的 `value[name=SPLIT_TEXT]` 里
/// `field[name=option]` 的取值换成 `split_options.options_dict[id].name`(`空格` → 单个空格),
/// 字段名改成 `TEXT`
fn replace_split_options(node: &mut XmlNode, split_options: &Map<String, Value>) {
    if node.tag == "block" && node.attr("type") == Some("text_split") {
        let resolved = find_named(node, "value", "SPLIT_TEXT")
            .and_then(|value| find_named(value, "field", "option"))
            .and_then(|field| {
                let key = field.text_content();
                split_options
                    .get(&key)
                    .and_then(|entry| entry.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        if let Some(name) = resolved
            && let Some(field) = find_named_mut(node, "field", "option")
        {
            field.set_attr("name", "TEXT");
            field.text = if name == "空格" {
                " ".to_string()
            } else {
                name
            };
        }
    }
    for child in &mut node.children {
        replace_split_options(child, split_options);
    }
}

/// 官方 `replaceSceneIndex`:`set_scene_by_index` 的 `value[name=index]` 里
/// `field[name=index]` 的 1 基下标换成 `scenes_order` 里的场景 id
fn replace_scene_index(node: &mut XmlNode, scenes_order: &[String]) {
    if node.tag == "block" && node.attr("type") == Some("set_scene_by_index") {
        let field_value = find_named(node, "value", "index")
            .and_then(|value| find_named(value, "field", "index"))
            .map(XmlNode::text_content)
            .filter(|text| !text.is_empty());
        if let Some(text) = field_value
            && let Some(index) = parse_int_prefix(&text)
        {
            let index = index - 1;
            if index >= 0
                && let Some(scene) = scenes_order.get(index as usize)
                && let Some(field) = find_named_mut(node, "field", "index")
            {
                field.text = scene.clone();
            }
        }
    }
    for child in &mut node.children {
        replace_scene_index(child, scenes_order);
    }
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

/// 先序找第一个 tag 匹配的后代(**不含**自身)
fn find_descendant<'a>(node: &'a XmlNode, tag: &str) -> Option<&'a XmlNode> {
    for child in &node.children {
        if child.tag == tag {
            return Some(child);
        }
        if let Some(found) = find_descendant(child, tag) {
            return Some(found);
        }
    }
    None
}

/// 先序找第一个 tag 匹配的可变后代(**不含**自身;与 `querySelector` 同序)
fn find_descendant_mut<'a>(node: &'a mut XmlNode, tag: &str) -> Option<&'a mut XmlNode> {
    for index in 0..node.children.len() {
        if node.children[index].tag == tag {
            return node.children.get_mut(index);
        }
        // 子树里真有目标才往下走(否则 `&mut` 的借用会跨迭代悬着)
        if has_descendant(&node.children[index], tag) {
            return find_descendant_mut(&mut node.children[index], tag);
        }
    }
    None
}

/// 先序找第一个 `tag[name=…]` 后代(**不含**自身;与 `querySelector('tag[name="x"]')` 同序同义)
fn find_named<'a>(node: &'a XmlNode, tag: &str, name: &str) -> Option<&'a XmlNode> {
    for child in &node.children {
        if child.tag == tag && child.attr("name") == Some(name) {
            return Some(child);
        }
        if let Some(found) = find_named(child, tag, name) {
            return Some(found);
        }
    }
    None
}

/// 同 [`find_named`],返回可变引用
fn find_named_mut<'a>(node: &'a mut XmlNode, tag: &str, name: &str) -> Option<&'a mut XmlNode> {
    for index in 0..node.children.len() {
        if node.children[index].tag == tag && node.children[index].attr("name") == Some(name) {
            return node.children.get_mut(index);
        }
        if has_named(&node.children[index], tag, name) {
            return find_named_mut(&mut node.children[index], tag, name);
        }
    }
    None
}

/// 子树里有没有 `tag[name=…]` 后代
fn has_named(node: &XmlNode, tag: &str, name: &str) -> bool {
    node.children.iter().any(|child| {
        (child.tag == tag && child.attr("name") == Some(name)) || has_named(child, tag, name)
    })
}

/// 子树里有没有 tag 匹配的后代(**不含**自身)
fn has_descendant(node: &XmlNode, tag: &str) -> bool {
    node.children
        .iter()
        .any(|child| child.tag == tag || has_descendant(child, tag))
}

/// 先序找第一个 `value[name=…] > shadow`(官方 `querySelector('value[name="x"] > shadow')`;
/// 该 `value` 没有直接 `shadow` 时继续找下一个同名 `value`)
fn find_value_shadow<'a>(node: &'a XmlNode, name: &str) -> Option<&'a XmlNode> {
    for child in &node.children {
        if child.tag == "value"
            && child.attr("name") == Some(name)
            && let Some(shadow) = child.child("shadow")
        {
            return Some(shadow);
        }
        if let Some(found) = find_value_shadow(child, name) {
            return Some(found);
        }
    }
    None
}

/// 同 [`find_value_shadow`],返回可变引用(先序,与 `querySelector` 同序)
fn find_value_shadow_mut<'a>(node: &'a mut XmlNode, name: &str) -> Option<&'a mut XmlNode> {
    for index in 0..node.children.len() {
        if node.children[index].tag == "value"
            && node.children[index].attr("name") == Some(name)
            && node.children[index].child("shadow").is_some()
        {
            return node.children[index].child_mut("shadow");
        }
        if has_value_shadow(&node.children[index], name) {
            return find_value_shadow_mut(&mut node.children[index], name);
        }
    }
    None
}

/// 子树里有没有 `value[name=…] > shadow`
fn has_value_shadow(node: &XmlNode, name: &str) -> bool {
    node.children.iter().any(|child| {
        (child.tag == "value"
            && child.attr("name") == Some(name)
            && child.child("shadow").is_some())
            || has_value_shadow(child, name)
    })
}

/// handler 里的 `descendant_value_shadow_mut` = [`find_value_shadow_mut`] 的别名(可读性)
fn descendant_value_shadow_mut<'a>(node: &'a mut XmlNode, name: &str) -> Option<&'a mut XmlNode> {
    find_value_shadow_mut(node, name)
}

/// 官方 `parseInt` 的前缀解析(复用 `nemo_mapping` 的口径)
fn parse_int_prefix(text: &str) -> Option<i64> {
    let trimmed = text.trim_start();
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|value| value * sign)
}

/// JS 真值(`undefined`/`null`/`false`/`0`/`""`/`NaN` → false)
fn truthy_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|value| value != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `Number.isNaN(v)`(只对数字类型的 NaN 成立,与 lodash `isNaN` 同义)
fn is_nan_number(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_f64)
        .is_some_and(|number| number.is_nan())
}

/// 官方 `Math.floor(e.scale)`;非数字 → `NaN` → JSON `null`
fn floored(value: Option<&Value>) -> Value {
    let Some(number) = value.and_then(Value::as_f64) else {
        return Value::Null;
    };
    number_value(number.floor())
}

/// 官方 `e.position`(`x`/`y` 有一个是 NaN 就整体归零;缺失的键**不出现**)
fn position_of(entity: &Map<String, Value>) -> Value {
    let x = entity.get("x");
    let y = entity.get("y");
    if is_nan_number(x) || is_nan_number(y) {
        return json!({ "x": 0, "y": 0 });
    }
    let mut position = Map::new();
    if let Some(x) = x {
        position.insert("x".to_string(), x.clone());
    }
    if let Some(y) = y {
        position.insert("y".to_string(), y.clone());
    }
    Value::Object(position)
}

/// 递归把"整数值的浮点"折成整数(`f64` 与 `i64` 在 JS 里同一个类型)
pub(crate) fn normalize_integral_numbers(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64()
                && float.fract() == 0.0
                && float.abs() < 9.007_199_254_740_992e15
            {
                *value = Value::from(float as i64);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_integral_numbers(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                normalize_integral_numbers(item);
            }
        }
        _ => {}
    }
}

/// `f64` → JSON 数字(整数就发整数,与 JS `JSON.stringify` 一致)
fn number_value(number: f64) -> Value {
    if !number.is_finite() {
        return Value::Null;
    }
    if number.fract() == 0.0 && number.abs() < 9.007_199_254_740_992e15 {
        Value::from(number as i64)
    } else {
        Value::from(number)
    }
}

/// 只在源里存在时写键(`JSON.stringify` 会丢掉 `undefined`)
fn insert_if_present(target: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        target.insert(key.to_string(), value.clone());
    }
}

/// 官方 `vI`:重名时"尾数 +1"(带进位),仍重名就继续加
fn dedupe_name(existing: &[String], name: &str) -> String {
    if !existing.iter().any(|item| item == name) {
        return name.to_string();
    }
    let mut candidate = name.to_string();
    loop {
        candidate = increment_trailing_digits(&candidate);
        if !existing.iter().any(|item| item == &candidate) {
            return candidate;
        }
    }
}

/// 官方 `r[1] + addWithCarry(r[2]||"0", "1")`
fn increment_trailing_digits(name: &str) -> String {
    let digits_at = name
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(index, _)| index)
        .unwrap_or(name.len());
    let (prefix, digits) = name.split_at(digits_at);
    let digits = if digits.is_empty() { "0" } else { digits };
    format!("{prefix}{}", digits_add_one(digits))
}

/// 十进制字符串 + 1(带进位)
fn digits_add_one(digits: &str) -> String {
    let mut out: Vec<u8> = digits.bytes().collect();
    let mut carry = 1u8;
    for byte in out.iter_mut().rev() {
        let digit = *byte - b'0' + carry;
        if digit > 9 {
            *byte = b'0' + digit - 10;
            carry = 1;
        } else {
            *byte = b'0' + digit;
            carry = 0;
        }
    }
    let mut text = String::from_utf8_lossy(&out).into_owned();
    if carry > 0 {
        text.insert(0, '1');
    }
    text
}

/// `scenes.scenes_order`(字符串数组)
fn scenes_order(source: &Value) -> Vec<String> {
    source
        .pointer("/scenes/scenes_order")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 实体的 `styles`(造型 id 数组);`None` = 缺失(与空数组语义不同,见 `nemo_mapping`)
fn style_ids_value(entity: &Map<String, Value>) -> Option<Vec<String>> {
    entity.get("styles").and_then(Value::as_array).map(|list| {
        list.iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

/// `split_options.options_dict`(选项 id → `{id, name}`)
fn split_option_names(source: &Value) -> Map<String, Value> {
    source
        .pointer("/split_options/options_dict")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

/// [`super::model::BlockTree`] → KN 的 `nekoBlockJsonList` 数组
pub(crate) fn tree_to_json(tree: &super::model::BlockTree) -> Vec<Value> {
    tree.roots
        .iter()
        .filter_map(|root| root.to_value().ok())
        .collect()
}
