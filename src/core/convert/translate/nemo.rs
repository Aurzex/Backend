use super::TranslateError;
use super::TranslateOptions;
use super::TranslateReport;
use super::TranslateWarning;
use super::model::IdSource;
use super::nemo_mapping::NEMO_BCM_VERSION;
use super::nemo_mapping::{
    NemoEntity, NemoParseContext, NemoSubject, PROCEDURE_NORMAL, nemo_parse_procedures,
    translate_nemo_to_kn,
};
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

/// 源 XML 元素计数:`block` / `shadow` / `empty`(官方研究的统计口径)
pub(crate) fn count_source_elements(nodes: &[XmlNode]) -> usize {
    let mut total = 0;
    let mut stack: Vec<&XmlNode> = nodes.iter().collect();
    while let Some(node) = stack.pop() {
        if matches!(node.tag.as_str(), "block" | "shadow" | "empty") {
            total += 1;
        }
        for child in &node.children {
            stack.push(child);
        }
    }
    total
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

// 来自 src/core/convert/translate/nemo_xml.rs
// NEMO 编辑版积木 XML 的**最小 DOM**。
// NEMO(Scratch 风格编辑器)把积木存成一段 `text/xml` 字符串,官方前端用浏览器的
// `DOMParser.parseFromString(xml, "text/xml")` 解析、`XMLSerializer.serializeToString`
// 回写,再用 `getAttribute`/`setAttribute`/`removeAttribute`/`textContent` 做改写。
// 官方 `blockly` 侧的惯例是给一段积木 XML 套一层 `<root>` 再解析(包装根
// `<variables></variables>` 那种也是同一用法),所以这里同时提供
// [`parse`](整份文档)与 [`parse_fragment`](单根包装取直接子元素)。
// Rust 侧没有 DOM,本文件把同一套语义**最小可用**地移植过来:
// | 浏览器 | 本文件 |
// | --- | --- |
// | `DOMParser.parseFromString(xml, "text/xml")` | [`parse`] |
// | `parseFromString("<root>" + blocksXML + "</root>", …)` 取 `root.childNodes` | [`parse_fragment`] |
// | `XMLSerializer.serializeToString(node)` | [`XmlNode::serialize`] |
// | `getAttribute` / `setAttribute` / `removeAttribute` | [`XmlNode::attr`] / [`XmlNode::set_attr`] / [`XmlNode::remove_attr`] |
// | `textContent` | [`XmlNode::text_content`] |
// | `querySelector` / `getElementsByTagName` 的"首个/全部子元素"用法 | [`XmlNode::child`] / [`XmlNode::child_mut`] / [`XmlNode::children_of`] |
// 对齐浏览器的几个语义点:
// - 属性**按出现顺序**存放:浏览器命名属性表不保证顺序,但序列化顺序即插入顺序,
// 所以 `set_attr` 同名**替换值并保持原位置**、异名**追加到末尾**,与 `setAttribute` 一致;
// - 序列化转义与 `XMLSerializer` 一致:属性值转义 `&` `"` `<` `>`,文本转义 `&` `<` `>`;
// - 空元素(无子元素且无文本)序列化成自闭合 `<tag/>`,哪怕原文写的是 `<tag></tag>`
// (浏览器同样如此);文本哪怕只是空白,也不算空元素;
// - 未声明的属性返回 `None`,与 `getAttribute` 返回 `null` 对应:空白串 ≠ 缺失;
// - 未知实体(`&nbsp;`)直接报错,与 `text/xml` 下的 `DOMParser` 一致
// (`text/html` 会容错,我们不学它);
// - 畸形输入(未闭合 / 开闭不匹配 / 属性缺引号 / 意外字符)一律返回
// [`DecompilerError::Decompile`],带行、列与字节偏移,不 panic、不产出半截结果。
// 已知取舍(与浏览器**不**完全等价的地方,调用方需要知道):
// - **不做命名空间处理**:`xmlns` / `xmlns:xxx` 只是普通属性,按字符串读写
// (积木 XML 里的命名空间只是装饰,没有前缀解析/`localName` 语义);
// - **文本只保留"两段式"**:DOM 里文本节点与元素节点交错,这里把元素之间的直接文本
// 按文档顺序拼进父节点的 [`XmlNode::text`],序列化时统一排在子元素**之后**。
// 因此对"子元素之间夹着文本"的输入,`serialize` 的输出与输入不逐字节相同,但
// `parse(serialize)` 与 `parse` 的结果相同(不动点),反复读写稳定。
// Scratch 积木 XML 的文本只出现在叶子元素里(field/mutation),真实数据不受影响;
// - **宽松处**:重复属性名按出现顺序各留一项(浏览器会当畸形文档报错);注释里含 `--`、
// 文本里含 `]]>` 等次要良构约束不校验;
// - **严格处**:根元素之外出现文本、无匹配的结束标签或非法 `<!` 开头标记一律报错(与
// `text/xml` 下的 `DOMParser` 一致);[`parse`] 允许顶层有多个元素(取第一个,编辑器
// 会把 `<variables></variables>` 与各根积木并排存),而 [`parse_fragment`] 要求唯一根包装;
// - **嵌套上限**:元素嵌套超过 `MAX_DEPTH` 层直接报错(真实作品只有几十层)。
// 解析/序列化/取文本都是**迭代**实现(显式栈),不依赖调用方线程的栈大小;
// 唯一与嵌套深度相关的递归是 `XmlNode` 自身的析构,上限就是为了兜住它。

/// 元素嵌套上限:超过即报错。
///
/// 真实作品的积木嵌套只有几十层,这里给足余量;设上限的唯一目的是兜住
/// `XmlNode` 递归析构(`Drop`)与畸形输入的内存占用。
const MAX_DEPTH: usize = 4096;

/// 一个 XML 元素(属性按出现顺序存,子节点按文档顺序存)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct XmlNode {
    /// 元素名(Scratch 积木 XML 一律小写:`block`/`shadow`/`value`/`field`/…)
    pub tag: String,
    /// 属性:按**出现顺序**存放(同名重复的按宽松策略各留一项)
    pub attrs: Vec<(String, String)>,
    /// 子元素:按文档顺序存放
    pub children: Vec<XmlNode>,
    /// 本元素的**直接**文本(不聚合子节点;解析时保留原样,转义已解码)
    pub text: String,
}

impl XmlNode {
    /// 新建元素(无属性、无子元素、无文本)
    pub(crate) fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            attrs: Vec::new(),
            children: Vec::new(),
            text: String::new(),
        }
    }

    /// 首个同名属性;未声明同名属性时返回 `None`(注意:不存在 ≠ 空串,对应 `getAttribute` 的 `null`)
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 同名则替换值(**保持原位置**),否则追加到末尾(`DOMParser` + `setAttribute` 语义)
    pub(crate) fn set_attr(&mut self, name: &str, value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = value.to_string(),
            None => self.attrs.push((name.to_string(), value.to_string())),
        }
    }

    /// 删除所有同名属性(对应 `removeAttribute`;属性不存在时无操作)
    pub(crate) fn remove_attr(&mut self, name: &str) {
        self.attrs.retain(|(k, _)| k != name);
    }

    /// 首个直接子元素(tag 精确匹配,不做大小写折叠)
    pub(crate) fn child(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// 首个直接子元素(可变)
    pub(crate) fn child_mut(&mut self, tag: &str) -> Option<&mut XmlNode> {
        self.children.iter_mut().find(|c| c.tag == tag)
    }

    /// 所有直接子元素中 tag 匹配的那些(文档顺序)
    pub(crate) fn children_of<'a>(
        &'a self,
        tag: &'a str,
    ) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children.iter().filter(move |c| c.tag.as_str() == tag)
    }

    /// **深度优先**拼接自身文本与所有后代的文本(与 DOM `textContent` 同义)。
    ///
    /// 元素按文档顺序先序展开("两段式"模型下父节点的直接文本无法还原交错位置,
    /// 这里按"自身文本 + 各后代"的顺序拼;对文本只出现在叶子里的积木 XML 结果一致)。
    pub(crate) fn text_content(&self) -> String {
        let mut out = String::new();
        // 显式栈先序遍历:弹一个节点 → 收它的直接文本 → 逆序压入子节点
        let mut stack: Vec<&XmlNode> = Vec::with_capacity(8);
        stack.push(self);
        while let Some(node) = stack.pop() {
            out.push_str(&node.text);
            for child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        out
    }

    /// 与浏览器 `XMLSerializer` 等价的最小实现。
    ///
    /// 布局:`<tag a="1" b="2">子元素序列 + 文本</tag>`;无子元素且无文本 → 自闭合 `<tag/>`。
    /// 迭代实现(显式工作栈),深层嵌套不会栈溢出。
    pub(crate) fn serialize(&self) -> String {
        /// 待完成的工作项(先序:开始标签 → 子元素 → 文本 → 结束标签)
        enum Job<'a> {
            Open(&'a XmlNode),
            Text(&'a str),
            Close(&'a XmlNode),
        }

        let mut out = String::new();
        let mut jobs: Vec<Job<'_>> = Vec::with_capacity(8);
        jobs.push(Job::Open(self));
        while let Some(job) = jobs.pop() {
            match job {
                Job::Open(node) => {
                    out.push('<');
                    out.push_str(&node.tag);
                    for (name, value) in &node.attrs {
                        out.push(' ');
                        out.push_str(name);
                        out.push_str("=\"");
                        push_attr_escaped(&mut out, value);
                        out.push('"');
                    }
                    if node.children.is_empty() && node.text.is_empty() {
                        // 空元素:浏览器也序列化成自闭合
                        out.push_str("/>");
                    } else {
                        out.push('>');
                        // 栈是后进先出:先压结束标签与文本,再逆序压子节点,
                        // 弹出顺序即为"子元素(文档序) → 文本 → 结束标签"
                        jobs.push(Job::Close(node));
                        jobs.push(Job::Text(&node.text));
                        for child in node.children.iter().rev() {
                            jobs.push(Job::Open(child));
                        }
                    }
                }
                Job::Text(text) => push_text_escaped(&mut out, text),
                Job::Close(node) => {
                    out.push_str("</");
                    out.push_str(&node.tag);
                    out.push('>');
                }
            }
        }
        out
    }
}

/// 属性值转义:`&` `"` `<` `>`(与 `XMLSerializer` 一致;单引号不需要转,因为统一用双引号包)
fn push_attr_escaped(out: &mut String, s: &str) {
    // 绝大多数属性值(编辑器 id、类名、数字)无需转义:先做一次扫描快速返回
    if !s.contains(['&', '"', '<', '>', '\r']) {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // 与官方 harness 的 `dom.js#encodeAttr` 一致:回车也走数字实体
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

/// 文本转义:`&` `<` `>`(与 `XMLSerializer` 一致)
fn push_text_escaped(out: &mut String, s: &str) {
    if !s.contains(['&', '<', '>', '\r']) {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // 与官方 harness 的 `dom.js#encodeText` 一致:回车也走数字实体
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

/// 解析一整份文档,返回**第一个元素**
/// (跳过 XML 声明/注释/DOCTYPE/前后空白/末尾其它节点)
pub(crate) fn parse(xml: &str) -> Result<XmlNode, DecompilerError> {
    let mut parser = Parser::new(xml);
    let roots = parser.run()?;
    // 顶层可以有多个元素(编辑器把 `<variables></variables>` 与各根积木并排存),
    // `parse` 只要第一个,与"取第一个元素"的调用方一致
    match roots.into_iter().next() {
        Some(node) => Ok(node),
        None => Err(DecompilerError::Decompile(format!(
            "积木 XML 解析失败:文档里没有任何元素(输入 {} 字节)",
            xml.len()
        ))),
    }
}

/// 解析 `<root>…</root>` 这种**单根包装**,返回该根的直接子元素
/// (官方 `parseFromString("<root>" + blocksXML + "</root>", "text/xml")` 的用法)。
/// 整份文档的根不是唯一元素 → `Err`。
pub(crate) fn parse_fragment(xml: &str) -> Result<Vec<XmlNode>, DecompilerError> {
    let mut parser = Parser::new(xml);
    let roots = parser.run()?;
    let count = roots.len();
    let mut iter = roots.into_iter();
    match (iter.next(), iter.next()) {
        (Some(root), None) => Ok(root.children),
        (first, _) => Err(DecompilerError::Decompile(format!(
            "积木 XML 片段解析失败:期望唯一根包装,实际解析出 {count} 个顶层元素{}",
            match first {
                Some(node) => format!("(首个是 <{}>)", node.tag),
                None => String::new(),
            }
        ))),
    }
}

/// 名称首字符:字母 / `_` / `:`(允许 `:` 只是为了让 `<a:b>` 这类名字别被误判,不做命名空间)
fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':'
}

/// 名称后续字符
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.')
}

/// 解码数字实体(`&#39;` 十进制 / `&#x27;` 十六进制);非法或越界返回 `None`
fn decode_numeric_entity(body: &str) -> Option<char> {
    let digits = body.strip_prefix('#')?;
    let code = match digits
        .strip_prefix('x')
        .or_else(|| digits.strip_prefix('X'))
    {
        // 十六进制:至少一位,且全是十六进制数字
        Some(hex) => {
            if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(hex, 16).ok()?
        }
        // 十进制
        None => {
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            digits.parse::<u32>().ok()?
        }
    };
    // 代理区(U+D800..U+DFFF)与超出 U+10FFFF 的码点取不到字符,按畸形报错
    char::from_u32(code)
}

/// 极简 XML 解析器。
///
/// 迭代实现:用"未闭合元素栈"代替递归下降,栈深即嵌套深度,
/// 因此深层嵌套只吃堆内存,不吃调用栈。
struct Parser<'a> {
    /// 待解析的完整输入(按字节索引推进,所有切片都落在字符边界上)
    src: &'a str,
    /// 当前字节偏移
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// 当前字符(不推进)
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// 取当前字符并推进(按 UTF-8 长度推进,保证不会切在字符中间)
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn starts_with(&self, needle: &str) -> bool {
        self.src[self.pos..].starts_with(needle)
    }

    /// 跳过 XML 空白(空格/Tab/CR/LF)。
    ///
    /// 不能直接用 `char::is_whitespace`:那会把 NBSP 之类的也当空白吞掉,
    /// 而 XML 里 NBSP 是正经的文本内容。
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// 期望当前字符是 `want`(否则按当前位置报错)
    fn expect_char(&mut self, want: char, msg: &str) -> Result<(), DecompilerError> {
        match self.peek() {
            Some(c) if c == want => {
                self.bump();
                Ok(())
            }
            _ => Err(self.err_here(msg)),
        }
    }

    /// 构造带定位信息的解析错误
    fn err_here(&self, msg: impl std::fmt::Display) -> DecompilerError {
        self.err_at(self.pos, msg)
    }

    /// 带行、列与字节偏移的解析错误,便于定位畸形输入
    fn err_at(&self, pos: usize, msg: impl std::fmt::Display) -> DecompilerError {
        let mut line = 1usize;
        let mut col = 1usize;
        for (i, c) in self.src.char_indices() {
            if i >= pos {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        DecompilerError::Decompile(format!(
            "积木 XML 解析失败(第 {line} 行第 {col} 列,字节 {pos}): {msg}"
        ))
    }

    /// 解析一个 XML 名称(元素名/属性名)。
    ///
    /// Scratch 积木 XML 的名称是纯 ASCII(`procedures_2_parameter_shadow` 这种),
    /// 这里放宽到 Unicode 字母,避免对合法文档误报。
    fn parse_name(&mut self) -> Result<String, DecompilerError> {
        let start = self.pos;
        match self.peek() {
            Some(c) if is_name_start(c) => self.pos += c.len_utf8(),
            _ => return Err(self.err_here("期望元素名或属性名")),
        }
        while let Some(c) = self.peek() {
            if is_name_char(c) {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        Ok(self.src[start..self.pos].to_string())
    }

    /// 解析整份文档,返回所有**顶层**元素(调用方决定取第一个还是要求唯一)
    fn run(&mut self) -> Result<Vec<XmlNode>, DecompilerError> {
        let mut roots: Vec<XmlNode> = Vec::new();
        // 未闭合元素栈:栈顶就是当前正在收文本/子元素的元素
        let mut open: Vec<XmlNode> = Vec::new();
        loop {
            if open.is_empty() {
                // 文档级:先跳过空白/注释/处理指令/DOCTYPE;
                // 此处再遇到文本、结束标签、非法 `<!` 都是畸形(DOMParser 同样报错)
                self.skip_document_misc()?;
                if self.eof() {
                    break;
                }
                if self.starts_with("<!DOCTYPE") {
                    self.skip_doctype()?;
                    continue;
                }
                if self.starts_with("<![CDATA[") {
                    return Err(self.err_here("根元素之外不允许出现 CDATA 段"));
                }
                if self.starts_with("<!") {
                    return Err(self.err_here("根元素之外出现非法的 <! 标记"));
                }
                if self.starts_with("</") {
                    return Err(self.err_here("结束标签没有对应的开始标签"));
                }
                if !self.starts_with("<") {
                    return Err(self.err_here("根元素之外不允许出现文本"));
                }
            } else {
                // 元素内容级:这里的空白属于**文本**,必须原样收进父节点,不能跳过
                if self.eof() {
                    let tag = open.last().map(|n| n.tag.as_str()).unwrap_or("");
                    let end = self.src.len();
                    return Err(self.err_at(end, format!("元素 <{tag}> 未闭合(缺少 </{tag}>)")));
                }
                // 结束标签:弹出栈顶并校验配对
                if self.starts_with("</") {
                    let close_pos = self.pos;
                    self.pos += 2;
                    let name = self.parse_name()?;
                    self.skip_ws();
                    self.expect_char('>', "结束标签未以 > 结束")?;
                    let node = match open.pop() {
                        Some(node) => node,
                        None => {
                            return Err(self.err_at(close_pos, "结束标签没有对应的开始标签"));
                        }
                    };
                    if node.tag != name {
                        return Err(self.err_at(
                            close_pos,
                            format!("结束标签 </{name}> 与开始标签 <{}> 不匹配", node.tag),
                        ));
                    }
                    match open.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => roots.push(node),
                    }
                    continue;
                }
                // 注释/PI/CDATA:丢弃或当文本,都不算子元素
                if self.starts_with("<!--") {
                    self.skip_comment()?;
                    continue;
                }
                if self.starts_with("<![CDATA[") {
                    let text = self.parse_cdata()?;
                    if let Some(parent) = open.last_mut() {
                        parent.text.push_str(&text);
                    }
                    continue;
                }
                if self.starts_with("<?") {
                    self.skip_pi()?;
                    continue;
                }
                if self.starts_with("<!") {
                    return Err(self.err_here("元素内容里出现非法的 <! 标记"));
                }
                // 普通文本(含空白)
                if !self.starts_with("<") {
                    let text = self.parse_text()?;
                    if let Some(parent) = open.last_mut() {
                        parent.text.push_str(&text);
                    }
                    continue;
                }
            }
            // 走到这里,pos 一定指向一个开始标签的 '<'
            if open.len() >= MAX_DEPTH {
                return Err(self.err_here(format!("元素嵌套超过 {MAX_DEPTH} 层,拒绝继续解析")));
            }
            let (node, self_closing) = self.parse_open_tag()?;
            if self_closing {
                match open.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => roots.push(node),
                }
            } else {
                open.push(node);
            }
        }
        Ok(roots)
    }

    /// 文档级杂项:空白、注释、`<?…?>` 处理指令(含开头的 `<?xml …?>` 声明)。
    /// 这些都对应 DOM 里的非元素节点,积木 XML 用不到,直接丢弃。
    fn skip_document_misc(&mut self) -> Result<(), DecompilerError> {
        loop {
            self.skip_ws();
            if self.starts_with("<!--") {
                self.skip_comment()?;
                continue;
            }
            if self.starts_with("<?") {
                self.skip_pi()?;
                continue;
            }
            return Ok(());
        }
    }

    /// 解析开始标签(调用时 `pos` 指向 `<`),返回元素与"是否自闭合"
    fn parse_open_tag(&mut self) -> Result<(XmlNode, bool), DecompilerError> {
        let tag_pos = self.pos;
        self.pos += 1; // '<'
        let tag = self.parse_name()?;
        let mut attrs: Vec<(String, String)> = Vec::new();
        loop {
            // 属性之间允许任意空白(含换行/Tab),这一点与 DOMParser 一致
            self.skip_ws();
            match self.peek() {
                None => {
                    return Err(self.err_at(
                        tag_pos,
                        format!("元素 <{tag}> 的属性区未结束(输入提前结束)"),
                    ));
                }
                Some('/') => {
                    // 自闭合:`/` 与 `>` 之间不允许空白(XML 文法如此)
                    self.pos += 1;
                    self.expect_char('>', "<x/ 后面必须紧跟 >")?;
                    return Ok((
                        XmlNode {
                            tag,
                            attrs,
                            children: Vec::new(),
                            text: String::new(),
                        },
                        true,
                    ));
                }
                Some('>') => {
                    self.pos += 1;
                    break;
                }
                Some(c) if is_name_start(c) => {
                    let name = self.parse_name()?;
                    self.skip_ws();
                    self.expect_char('=', &format!("属性 {name} 后面缺少 ="))?;
                    self.skip_ws();
                    let value = self.parse_attr_value(&name)?;
                    // 重复属性名按出现顺序各留一项(浏览器 DOM 会当畸形报错,这里按宽松策略放行)
                    attrs.push((name, value));
                }
                Some(c) => {
                    return Err(self.err_here(format!("元素 <{tag}> 的属性区出现意外字符 {c:?}")));
                }
            }
        }
        Ok((
            XmlNode {
                tag,
                attrs,
                children: Vec::new(),
                text: String::new(),
            },
            false,
        ))
    }

    /// 解析一个属性值(`"…"` 或 `'…'`,两种引号都支持),实体在此解码
    fn parse_attr_value(&mut self, name: &str) -> Result<String, DecompilerError> {
        let quote_pos = self.pos;
        let quote = match self.peek() {
            Some(c @ ('"' | '\'')) => {
                self.pos += 1;
                c
            }
            _ => {
                return Err(self.err_at(
                    quote_pos,
                    format!("属性 {name} 的值缺少引号(要用 \" 或 ' 包起来)"),
                ));
            }
        };
        let mut out = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(
                        self.err_at(quote_pos, format!("属性 {name} 的值缺少结束引号 {quote}"))
                    );
                }
                // 结束引号
                Some(c) if c == quote => {
                    self.pos += 1;
                    return Ok(out);
                }
                // 实体解码
                Some('&') => {
                    let amp = self.pos;
                    self.pos += 1;
                    out.push(self.parse_entity(amp)?);
                }
                // 属性值里的裸 '<' 在 XML 里非法(必须写 &lt;),DOMParser 同样报错
                Some('<') => {
                    return Err(self.err_at(self.pos, format!("属性 {name} 的值里不允许出现裸 <")));
                }
                Some(c) => {
                    self.pos += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }

    /// 收一段字符数据直到下一个 `<` 或输入结束;`&…;` 实体在此解码
    fn parse_text(&mut self) -> Result<String, DecompilerError> {
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some('<') => return Ok(out),
                Some('&') => {
                    let amp = self.pos;
                    self.pos += 1;
                    out.push(self.parse_entity(amp)?);
                }
                Some(c) => {
                    self.pos += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }

    /// 解码一个实体引用(调用时 `pos` 已在 `&` 之后,`amp_pos` 是 `&` 的位置)。
    ///
    /// 只认 `amp`/`lt`/`gt`/`quot`/`apos` 与数字实体;未知实体直接报错,
    /// 与浏览器 `text/xml` 下的 `DOMParser` 一致(它不认 `&nbsp;`,也不容忍裸 `&`)。
    fn parse_entity(&mut self, amp_pos: usize) -> Result<char, DecompilerError> {
        let rest = &self.src[self.pos..];
        let semi = match rest.find(';') {
            Some(i) => i,
            None => return Err(self.err_at(amp_pos, "实体引用缺少结束的 ;")),
        };
        // 真实实体名最长 4 个 ASCII 字符;给数字实体留点余量,
        // 超过就说明是随手写的裸 '&',不必再往后找 ';'
        let body = &rest[..semi];
        if semi > 32 || !body.is_ascii() {
            return Err(self.err_at(amp_pos, format!("非法的实体引用 &{body}…")));
        }
        let ch = match body {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if body.starts_with('#') => match decode_numeric_entity(body) {
                Some(c) => c,
                None => {
                    return Err(self.err_at(amp_pos, format!("非法的数字实体 &{body};")));
                }
            },
            _ => {
                return Err(self.err_at(
                    amp_pos,
                    format!("未知实体引用 &{body};(只认 &amp; &lt; &gt; &quot; &apos; 与数字实体)"),
                ));
            }
        };
        self.pos += semi + 1; // 跳过实体名与 ';'
        Ok(ch)
    }

    /// 跳过注释 `<!-- … -->`(Pascal/Scratch XML 里可能出现;DOM 里是注释节点,我们丢弃)
    fn skip_comment(&mut self) -> Result<(), DecompilerError> {
        let start = self.pos;
        self.pos += 4; // "<!--"
        match self.src[self.pos..].find("-->") {
            Some(i) => {
                self.pos += i + 3;
                Ok(())
            }
            None => Err(self.err_at(start, "注释未闭合(缺少 -->)")),
        }
    }

    /// 跳过处理指令 `<? … ?>`(含文档头的 `<?xml version="1.0" encoding="UTF-8"?>`)
    fn skip_pi(&mut self) -> Result<(), DecompilerError> {
        let start = self.pos;
        self.pos += 2; // "<?"
        match self.src[self.pos..].find("?>") {
            Some(i) => {
                self.pos += i + 2;
                Ok(())
            }
            None => Err(self.err_at(start, "处理指令未闭合(缺少 ?>)")),
        }
    }

    /// 取 CDATA 段内容(`<![CDATA[ … ]]>`):按原文当文本,**不解码**实体
    fn parse_cdata(&mut self) -> Result<String, DecompilerError> {
        let start = self.pos;
        self.pos += 9; // "<![CDATA["
        match self.src[self.pos..].find("]]>") {
            Some(i) => {
                let text = self.src[self.pos..self.pos + i].to_string();
                self.pos += i + 3;
                Ok(text)
            }
            None => Err(self.err_at(start, "CDATA 段未闭合(缺少 ]]>)")),
        }
    }

    /// 跳过 `<!DOCTYPE …>`(含 `[ … ]` 内部子集;内容不做任何解析)
    fn skip_doctype(&mut self) -> Result<(), DecompilerError> {
        let start = self.pos;
        self.pos += 9; // "<!DOCTYPE"
        let mut depth = 0usize;
        loop {
            match self.bump() {
                None => return Err(self.err_at(start, "DOCTYPE 声明未闭合(缺少 >)")),
                // 跳过引号里的字符串(内部子集可能写成 SYSTEM "…" 并含 '>')
                Some(q @ ('"' | '\'')) => loop {
                    match self.bump() {
                        None => return Err(self.err_at(start, "DOCTYPE 声明里的引号未闭合")),
                        Some(c) if c == q => break,
                        Some(_) => {}
                    }
                },
                Some('[') => depth += 1,
                Some(']') => depth = depth.saturating_sub(1),
                // 只有内部子集闭合后的 `>` 才是声明结束
                Some('>') if depth == 0 => return Ok(()),
                Some(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod nemo_xml_tests {
    use super::*;

    /// 断言解析失败且是 `Decompile` 错误,返回错误说明(测试里用,非测试代码不 panic)
    fn parse_err(xml: &str) -> String {
        match parse(xml) {
            Ok(node) => panic!("期望解析失败,却解析出了 <{}>", node.tag),
            Err(DecompilerError::Decompile(msg)) => msg,
            Err(other) => panic!("期望 Decompile 错误,实际 {other:?}"),
        }
    }

    fn attr_vec(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// 真实形状的 NEMO/Scratch 积木 XML:自闭合、单引号属性、注释、CDATA、
    /// 十进制/十六进制数字实体、`&quot;` 都要能正确解析,且 `parse∘serialize` 是不动点。
    #[test]
    fn parses_and_serializes_realistic_block_xml() {
        let src = r##"<?xml version="1.0" encoding="UTF-8"?>
<!-- NEMO 编辑版积木(Scratch 风格 XML) -->
<block type="procedures_call" id='b1' x="295" y="-133" inline="true" visible="visible">
  <mutation proccode="说 &quot;你好&quot;" argumentids="[]"></mutation>
  <value name="NUM">
    <shadow type="math_number" id="s1">
      <field name="NUM">-1.5</field>
    </shadow>
  </value>
  <field name="TEXT"><![CDATA[a < b & c]]></field>
  <field name="AMP">&#39; &#x2F; &apos; &amp;#39;</field>
  <next>
    <block type="data_setvariableto" id="b2"/>
  </next>
</block>"##;
        let doc = parse(src).expect("真实形状的积木 XML 必须能解析");

        assert_eq!(doc.tag, "block");
        // 属性按出现顺序保存(单引号的 id='b1' 与双引号等价)
        assert_eq!(
            doc.attrs,
            attr_vec(&[
                ("type", "procedures_call"),
                ("id", "b1"),
                ("x", "295"),
                ("y", "-133"),
                ("inline", "true"),
                ("visible", "visible"),
            ]),
            "属性应按出现顺序保存,且两种引号都要能解析"
        );
        // `&quot;` 解码
        assert_eq!(
            doc.child("mutation").and_then(|m| m.attr("proccode")),
            Some("说 \"你好\""),
            "属性值里的 &quot; 应解码成双引号"
        );
        // 自闭合 shadow 有子元素
        assert_eq!(
            doc.child("value")
                .and_then(|v| v.child("shadow"))
                .and_then(|s| s.child("field"))
                .map(|f| f.text_content()),
            Some("-1.5".to_string()),
            "嵌套的 field 文本应能取到"
        );
        // 直接子元素查找:同名多个用 children_of,不存在的 tag 返回 None
        assert_eq!(
            doc.children_of("field").count(),
            2,
            "block 下应有 TEXT/AMP 两个 field"
        );
        assert!(
            doc.child("procedures_definition").is_none(),
            "不存在的子元素应返回 None"
        );
        let text_field = doc.child("field").expect("TEXT 是第一个 field 子元素");
        assert_eq!(text_field.attr("name"), Some("TEXT"));
        assert_eq!(text_field.text, "a < b & c", "CDATA 内容按原文保留");
        // 数字实体:十进制 &#39; 与十六进制 &#x2F;;&amp;#39; 是单遍解码后剩下的字面量
        let amp_field = doc
            .children_of("field")
            .find(|f| f.attr("name") == Some("AMP"))
            .expect("应有 name=AMP 的 field");
        assert_eq!(amp_field.text, "' / ' &#39;", "数字实体与 &apos; 都要解码");
        // 自闭合的空元素:无子元素、无文本
        let next_block = doc
            .child("next")
            .and_then(|n| n.child("block"))
            .expect("next 下有 block");
        assert!(next_block.children.is_empty() && next_block.text.is_empty());
        assert_eq!(next_block.attr("type"), Some("data_setvariableto"));

        // 序列化结果对齐 XMLSerializer 的口径
        let xml_text = doc.serialize();
        for needle in [
            r#"<block type="procedures_call" id="b1" x="295" y="-133" inline="true" visible="visible">"#,
            r#"<mutation proccode="说 &quot;你好&quot;" argumentids="[]"/>"#,
            r#"<field name="TEXT">a &lt; b &amp; c</field>"#,
            r#"<field name="AMP">' / ' &amp;#39;</field>"#,
            r#"<block type="data_setvariableto" id="b2"/>"#,
        ] {
            assert!(
                xml_text.contains(needle),
                "序列化结果应包含 {needle},实际:{xml_text}"
            );
        }

        // 不动点:序列化 → 再解析 → 再序列化,结果必须逐字节一致
        // (元素之间的空白作为父节点文本,统一排在子元素之后,位置有变但稳定)
        let again = parse(&xml_text)
            .expect("自家序列化的结果必须能再解析")
            .serialize();
        assert_eq!(xml_text, again, "parse∘serialize 应稳定(不动点)");

        // 规范形状(子元素在前、子元素之间无文本)可以逐字节往返
        let canonical =
            r#"<block type="math_number" id="s1"><field name="NUM">-1.5</field></block>"#;
        assert_eq!(
            parse(canonical).expect("规范形状可解析").serialize(),
            canonical,
            "规范形状应逐字节往返"
        );
    }

    /// 属性读写:`attr` 缺失返回 `None`、`set_attr` 同名替换保持位置、异名追加到末尾、
    /// `remove_attr` 删除(不存在时无操作)。
    #[test]
    fn attr_read_write() {
        let mut node = XmlNode::new("block");
        assert_eq!(
            node.attr("type"),
            None,
            "未声明的属性应返回 None,而不是空串"
        );

        node.set_attr("type", "math_number");
        node.set_attr("id", "b1");
        node.set_attr("visible", "");
        assert_eq!(
            node.attr("visible"),
            Some(""),
            "声明过的空串属性要与缺失区分开"
        );
        assert_eq!(node.attrs.len(), 3, "三个属性各一项");

        // 同名替换:值更新,位置不动
        node.set_attr("type", "text");
        assert_eq!(
            node.attrs,
            attr_vec(&[("type", "text"), ("id", "b1"), ("visible", "")]),
            "同名替换应保持原位置"
        );

        // 异名追加:排到末尾
        node.set_attr("inline", "true");
        assert_eq!(
            node.attrs
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["type", "id", "visible", "inline"],
            "新属性应追加到末尾"
        );

        node.remove_attr("id");
        assert_eq!(node.attr("id"), None, "删除后应返回 None");
        assert_eq!(
            node.attrs
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["type", "visible", "inline"],
            "删除只影响同名属性"
        );
        node.remove_attr("nonexistent");
        assert_eq!(node.attrs.len(), 3, "删除不存在的属性应无操作");

        // 属性值转义对齐 XMLSerializer:& " < > 都转;单引号不转(统一用双引号包)
        node.set_attr("mode", "a&b\"c<d>e'f");
        let xml = node.serialize();
        assert!(
            xml.contains(r#"mode="a&amp;b&quot;c&lt;d&gt;e'f""#),
            "属性值转义应与 XMLSerializer 一致,实际:{xml}"
        );
        assert_eq!(
            parse(&xml).expect("自家序列化结果应可解析").attr("mode"),
            Some("a&b\"c<d>e'f"),
            "转义必须可逆"
        );

        // 只有空白文本的元素不算空元素,不能自闭合
        assert_eq!(
            parse("<block> </block>").expect("空白文本元素").serialize(),
            "<block> </block>",
            "有空白文本时应保留 <tag> </tag> 而不是自闭合"
        );
    }

    /// `<root>` 包装取直接子元素:只返回直接子元素,包装根自己的文本丢掉,
    /// 与官方 `parseFromString("<root>"+blocksXML+"</root>")` 后取 `root.children` 一致。
    #[test]
    fn parse_fragment_returns_direct_children() {
        let children = parse_fragment(
            r#"<root><block type="math_number" id="b1"/><value name="A"><shadow type="math_number" id="s1"/></value></root>"#,
        )
        .expect("单根包装应能解析");
        assert_eq!(children.len(), 2, "应返回 2 个直接子元素");
        assert_eq!(children[0].tag, "block");
        assert_eq!(children[0].attr("type"), Some("math_number"));
        assert_eq!(children[1].tag, "value");
        assert_eq!(children[1].attr("name"), Some("A"));
        assert_eq!(
            children[1].child("shadow").and_then(|s| s.attr("id")),
            Some("s1"),
            "直接子元素的子树应完整保留"
        );

        // 包装根的空白文本不聚合进子元素
        let padded =
            parse_fragment("<root>\n  <block/>\n  <block/>\n</root>").expect("带空白的包装");
        assert_eq!(padded.len(), 2, "包装根的直接子元素仍是 2 个");
        assert_eq!(padded[0].text, "", "空白归包装根,不归子元素");

        // 顶层多根 → 契约要求报错
        assert!(
            parse_fragment("<a/><b/>").is_err(),
            "顶层不是唯一根时 parse_fragment 必须报错"
        );
        assert!(parse_fragment("").is_err(), "空的包装也必须报错");
    }

    /// `text_content` 深度优先聚合:`<field name="NUM">1</field>` → `"1"`;
    /// 含子元素的父节点聚合后代文本;空白与语义相关的前后空格不能被 trim。
    #[test]
    fn text_content_aggregates_descendants() {
        let field = parse(r#"<field name="NUM">1</field>"#).expect("field 解析");
        assert_eq!(field.text_content(), "1", "field 的文本就是 NUM 的值");

        let value = parse(
            r#"<value name="A"><shadow type="math_number" id="s1"><field name="NUM">42</field></shadow></value>"#,
        )
        .expect("value 解析");
        assert_eq!(value.text, "", "value 自己没有直接文本");
        assert_eq!(value.text_content(), "42", "没有直接文本时聚合后代文本");

        // 前后空格必须原样保留(field 值可能带空格)
        let spaced = parse(concat!(
            r#"<block>"#,
            r#"<field name="TEXT">  hi  </field>"#,
            r#"<field name="OTHER">x</field>"#,
            r#"</block>"#,
        ))
        .expect("带空格的 field 解析");
        assert_eq!(
            spaced.child("field").map(|f| f.text_content()),
            Some("  hi  ".to_string()),
            "文本不能 trim"
        );
        assert_eq!(spaced.text_content(), "  hi  x", "按文档顺序先序聚合");
    }

    /// 畸形输入一律 `Err(Decompile)`(不 panic、不产出半截结果)。
    #[test]
    fn malformed_input_errors() {
        // 元素未闭合
        let msg = parse_err(r#"<block><field name="NUM">1</field>"#);
        assert!(msg.contains("未闭合"), "应说明未闭合,实际:{msg}");
        // 开闭标签不匹配
        let msg = parse_err("<block></value>");
        assert!(msg.contains("不匹配"), "应说明不匹配,实际:{msg}");
        // 未知实体(与浏览器 text/xml 一致:&nbsp; 不认)
        let msg = parse_err("<block>&nbsp;</block>");
        assert!(msg.contains("未知实体"), "应说明未知实体,实际:{msg}");
        // 属性值里的未知实体
        assert!(
            parse(r#"<block type="a&nbsp;b"/>"#).is_err(),
            "属性值里的未知实体也要报错"
        );
        // 非法数字实体(代理区取不到字符)
        assert!(
            parse("<block>&#xD800;</block>").is_err(),
            "代理区码点应报错"
        );
        assert!(parse("<block>&#x;</block>").is_err(), "空十六进制应报错");
        assert!(parse("<block>&#;</block>").is_err(), "空十进制应报错");
        // 裸 '&'
        assert!(parse("<block>a & b</block>").is_err(), "裸 & 应报错");
        // 属性值缺引号 / 缺结束引号 / 裸 '<'
        assert!(
            parse("<block type=math_number/>").is_err(),
            "属性值缺引号应报错"
        );
        assert!(
            parse(r#"<block type="math_number/>"#).is_err(),
            "属性值缺结束引号应报错"
        );
        assert!(
            parse(r#"<block type="a<b"/>"#).is_err(),
            "属性值里的裸 < 应报错"
        );
        // 属性区中途结束
        assert!(parse("<block type").is_err(), "属性区未结束应报错");
        assert!(parse("<block type=").is_err(), "缺属性值应报错");
        // 自闭合标签没写完
        assert!(parse("<block/").is_err(), "自闭合缺 > 应报错");
        // 没有元素 / 根之外的裸文本 / 多余的结束标签
        assert!(parse("").is_err(), "空文档应报错");
        assert!(parse("   \n  ").is_err(), "只有空白的文档应报错");
        assert!(parse("hello").is_err(), "根元素之外的文本应报错");
        assert!(parse("</block>").is_err(), "无匹配的结束标签应报错");
        assert!(parse("<block/>tail").is_err(), "根元素之后的裸文本应报错");
        // 注释 / PI / CDATA 未闭合
        assert!(
            parse("<block><!-- oops </block>").is_err(),
            "未闭合注释应报错"
        );
        assert!(
            parse("<block><![CDATA[oops</block>").is_err(),
            "未闭合 CDATA 应报错"
        );
        // 顶层多根对 parse 合法(编辑器把 <variables> 与积木并排存),parse 取第一个
        let multi =
            parse("<variables></variables><block type=\"a\"/>").expect("多顶层元素应可解析");
        assert_eq!(multi.tag, "variables");
    }

    /// 深层嵌套必须能处理(几百层),解析/序列化都是迭代实现,不吃调用栈。
    #[test]
    fn deep_nesting_does_not_overflow_stack() {
        const DEPTH: usize = 300;
        let mut src = String::new();
        for _ in 0..DEPTH {
            src.push_str("<block>");
        }
        src.push_str("deep");
        for _ in 0..DEPTH {
            src.push_str("</block>");
        }
        let node = parse(&src).expect("几百层嵌套必须能解析");
        assert_eq!(node.text_content(), "deep", "深层文本应聚合出来");
        let mut depth = 0usize;
        let mut cursor = &node;
        while let Some(next) = cursor.child("block") {
            depth += 1;
            cursor = next;
        }
        assert_eq!(depth, DEPTH - 1, "嵌套层数应完整保留");
        // 深层树的序列化也是迭代实现
        assert_eq!(
            parse(&node.serialize())
                .expect("深层树的序列化结果可再解析")
                .text_content(),
            "deep",
            "序列化-解析应稳定"
        );
        // 超过上限时是 Err 而不是爆栈
        let mut too_deep = String::new();
        for _ in 0..(MAX_DEPTH + 2) {
            too_deep.push_str("<block>");
        }
        assert!(
            parse(&too_deep).is_err(),
            "超过 MAX_DEPTH 层应报错而不是栈溢出"
        );
    }
}
