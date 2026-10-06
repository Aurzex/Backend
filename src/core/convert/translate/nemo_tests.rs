//! NEMO → KN 的测试:自造小文档(不依赖 `download/`)+ 真作品门(需要 `temp/harness`)
//!
//! 真作品那部分与仓库其它真机测试同约定:夹具(`temp/harness/`,不入库)不在时**跳过**。
//! 门有三道:**官方 `validateBcm`**、**语义 diff(与官方产物)在 allow-list 内**、**块数守恒**。

use serde_json::{Value, json};

use crate::core::convert::EditorType;
use crate::core::convert::translate::{TargetEditor, TranslateOptions, translate_value};

/// 造一份最小 NEMO 编辑版:一个演员、一个场景、一个造型、一个全局变量
fn nemo_document(actor_xml: &str, scene_xml: &str) -> Value {
    json!({
        "app_version": "2.3.0",
        "project_name": "测试作品",
        "actors": {
            "actors_dict": {
                "actor-1": {
                    "id": "actor-1",
                    "name": "角色1",
                    "x": -70,
                    "y": 12,
                    "rotation": 0,
                    "scale": 70.9,
                    "visible": true,
                    "locked": false,
                    "current_style_id": "style-1",
                    "styles": ["style-1", "style-2"],
                    "scene_id": "scene-1",
                    "blocksXML": actor_xml,
                }
            },
            "current_actor": "actor-1"
        },
        "scenes": {
            "current_scene": "scene-1",
            "scenes_order": ["scene-1"],
            "scenes_dict": {
                "scene-1": {
                    "id": "scene-1",
                    "name": "场景1",
                    "actors": ["actor-1"],
                    "styles": ["style-1"],
                    "current_style_id": "",
                    "visible": true,
                    "blocksXML": scene_xml,
                }
            }
        },
        "styles": {
            "styles_dict": {
                "style-1": {
                    "id": "style-1",
                    "name": "造型1",
                    "texture": "res/drawable/b_1.png",
                    "center_point": { "x": 1, "y": 2 }
                },
                "style-2": {
                    "id": "style-2",
                    "name": "造型2",
                    "url": "https://static.codemao.cn/nemo/22/a.webp"
                }
            }
        },
        "audios": { "sounds": {} },
        "variable": {
            "variable_dict": {
                "var-1": {
                    "id": "var-1",
                    "name": "分数",
                    "type": "private",
                    "value": 7,
                    "visible": false,
                    "is_global": true,
                    "position": { "x": -100, "y": 20 },
                    "scale": 1
                }
            }
        },
        "broadcast": { "broadcast_dict": {} },
        "procedures": {},
        "split_options": { "options_dict": {} },
        "block_count": { "all_block_count": 0, "visible_block_count": 0 }
    })
}

fn convert(document: Value, version: Option<&str>) -> Value {
    let mut options = TranslateOptions::new().deterministic_ids(true);
    if let Some(version) = version {
        options = options.source_version(version);
    }
    translate_value(document, TargetEditor::KittenN, &options)
        .expect("NEMO → KN 转化")
        .document
}

/// 取演员的积木列表
fn actor_blocks(document: &Value) -> &Vec<Value> {
    document
        .pointer("/actors/actorsDict/actor-1/nekoBlockJsonList")
        .and_then(Value::as_array)
        .expect("演员积木列表")
}

/// 槽位覆盖语义:`<value>` 里同时有 `<shadow>` 与覆盖块时,
/// `inputs[槽]` = 那个块、`shadows[槽]` = 影子**重新序列化**的 XML。
#[test]
fn value_slot_keeps_shadow_xml_and_override_block() {
    let actor_xml = concat!(
        r#"<block type="wait" id="b0" visible="visible" inline="true" x="10" y="20">"#,
        r#"<value name="time">"#,
        r#"<shadow type="math_number" id="s1" visible="visible">"#,
        r#"<field constraints="-Infinity,Infinity,0," name="NUM">3</field></shadow>"#,
        r#"<block type="variables_get" id="b1" visible="visible" inline="true">"#,
        r#"<field name="VAR">var-1</field></block>"#,
        r#"</value></block>"#,
    );
    let document = convert(nemo_document(actor_xml, ""), None);
    let roots = actor_blocks(&document);
    assert_eq!(roots.len(), 1, "一个根积木:{roots:?}");
    let wait = &roots[0];
    assert_eq!(wait["type"], "wait");
    assert_eq!(wait["location"], json!([10, 20]));
    // inputs.time = 覆盖块(variables_get 在官方表里是"特殊类型":保留自身类型且 is_shadow=false;
    // `false` 是序列化时的省略默认值,所以断言"没有 is_shadow"而不是"== false")
    assert_eq!(wait["inputs"]["time"]["type"], "variables_get");
    assert!(
        wait["inputs"]["time"].get("is_shadow").is_none(),
        "覆盖块不是影子:{:?}",
        wait["inputs"]["time"]
    );
    // VAR → variable(variables_get 的字段改名)
    assert_eq!(wait["inputs"]["time"]["fields"]["variable"], "var-1");
    // shadows.time = 影子的 XML 重序列化(影子 id 被重铸 ⇒ 只断言结构)
    let shadow = wait["shadows"]["time"].as_str().expect("影子 XML");
    assert!(
        shadow
            .starts_with(r#"<shadow xmlns="http://www.w3.org/1999/xhtml" type="math_number" id=""#),
        "{shadow}"
    );
    assert!(
        shadow.ends_with(r#"" visible="visible"><field name="NUM">3</field></shadow>"#),
        "{shadow}"
    );
}

/// 双形态:源的 snake_case 字段原样保留(`blocksXML` **逐字节**不变),另外并行给 camelCase
#[test]
fn document_keeps_source_fields_and_adds_camel_case() {
    let actor_xml = r#"<block type="self_appear" id="b0" visible="visible" inline="true"/>"#;
    let document = convert(nemo_document(actor_xml, ""), None);
    let actor = &document["actors"]["actorsDict"]["actor-1"];
    // 原样保留
    assert_eq!(actor["blocksXML"], actor_xml, "blocksXML 原样保留");
    assert_eq!(actor["current_style_id"], "style-1");
    assert_eq!(actor["styles"], json!(["style-1", "style-2"]));
    // 并行新增
    assert_eq!(actor["position"], json!({ "x": -70, "y": 12 }));
    assert_eq!(actor["currentStyleId"], "style-1");
    assert_eq!(actor["workspaceScrollXy"], json!({ "x": 100, "y": 50 }));
    // `scale` 被官方**原地取整**(`Math.floor`),双形态下这一条是"改写后保留 snake_case 名"
    assert_eq!(actor["scale"], 70, "scale 取整(70.9 → 70)");
    // 场景:命名与排序
    let scene = &document["scenes"]["scenesDict"]["scene-1"];
    assert_eq!(scene["screenName"], "屏幕1");
    assert_eq!(scene["name"], "背景");
    assert_eq!(scene["actorIds"], json!(["actor-1"]));
    assert_eq!(document["scenes"]["sortList"], json!(["scene-1"]));
    assert_eq!(document["scenes"]["currentSceneId"], "scene-1");
    // 造型:center_point → centerPoint,texture 走 CDN 前缀,.webp 追加查询串
    let styles = &document["styles"]["stylesDict"];
    assert_eq!(styles["style-1"]["centerPoint"], json!({ "x": 1, "y": 2 }));
    assert!(styles["style-1"].get("center_point").is_none());
    assert_eq!(
        styles["style-1"]["url"],
        "https://static.codemao.cn/nemo/22/res/drawable/b_1.png"
    );
    assert_eq!(
        styles["style-2"]["url"],
        "https://static.codemao.cn/nemo/22/a.webp?imageView2/0/format/png"
    );
    // 变量:private → any、value 归零、坐标按舞台中心平移(无 stage_size ⇒ 竖屏 562×900)
    let variable = &document["variables"]["variablesDict"]["var-1"];
    assert_eq!(variable["type"], "any");
    assert_eq!(variable["value"], 0);
    assert_eq!(variable["style"], "default");
    assert_eq!(variable["position"], json!({ "x": 181, "y": 430 }));
    // 顶层骨架
    assert_eq!(
        document["stageSize"],
        json!({ "width": 562, "height": 900 })
    );
    assert_eq!(document["projectName"], "测试作品");
    assert_eq!(document["version"], "");
    assert_eq!(document["previewUrl"], "");
}

/// 版本迁移(`docs/rounds/27` §9.3):`< 0.15.0` 走 YC(`controls_if` 补 `else="1"` 变异),
/// `< 0.9.4` 走 QC(角色 rotation 取反 + 旧音频块 `<field name="audio">` 包成影子)。
#[test]
fn version_migration_rewrites_legacy_documents() {
    let yc_xml = concat!(
        r#"<block type="controls_if" id="b0" visible="visible" inline="true" x="0" y="0">"#,
        r#"<value name="if0"><empty type="logic_empty" id="e1" visible="visible"/>"#,
        r#"<block type="logic_compare" id="b1" visible="visible" inline="true"><field name="OP">EQ</field>"#,
        r#"<value name="a"><shadow type="math_number" id="s1" visible="visible">"#,
        r#"<field name="NUM">1</field></shadow></value>"#,
        r#"<value name="b"><shadow type="math_number" id="s2" visible="visible">"#,
        r#"<field name="NUM">2</field></shadow></value>"#,
        r#"</block></value>"#,
        r#"<statement name="DO0"><block type="audio__play_audio" id="b2" visible="visible" inline="true">"#,
        r#"<field name="audio">audio-1</field></block></statement></block>"#,
    );

    // YC:0.11.0 < 0.15.0 —— `controls_if` 首子不是 `<mutation>` 就补 `<mutation else="1"/>`
    // (注意:YC 只认 `controls_if`,不改 `controls_if_no_else`;这也是判别它是否真的跑了的关键)
    let yc = convert(nemo_document(yc_xml, ""), Some("0.11.0"));
    let block = &actor_blocks(&yc)[0];
    assert_eq!(block["type"], "controls_if");
    let mutation = block["mutation"].as_str().unwrap_or_default();
    assert!(mutation.contains(r#"else="1""#), "YC 未生效:{mutation:?}");
    // 迁移是**字符串级**改写:`blocksXML` 被重新序列化(自闭合 + 插入的变异),不再是原文
    let migrated = yc["actors"]["actorsDict"]["actor-1"]["blocksXML"]
        .as_str()
        .expect("blocksXML");
    assert!(
        migrated.contains(r#"<mutation else="1"/><value name="if0">"#)
            || migrated.contains(r#"<mutation else="1"/>"#),
        "迁移后的 blocksXML 未包含插入的变异:{migrated}"
    );
    assert!(
        migrated.contains(r#"<next last_next_in_stack="true"/>"#) || !migrated.contains("<next"),
        "迁移后的 blocksXML 应重新序列化(自闭合):{migrated}"
    );

    // 不给版本 ⇒ 不迁移:同一个块不应该有变异(证明上面那条是**版本迁移**带来的,不是恒有行为)
    let plain = convert(nemo_document(yc_xml, ""), None);
    assert!(
        plain_blocks(&plain)[0].get("mutation").is_none(),
        "无 bcm_version 时不该做版本迁移:{:?}",
        plain_blocks(&plain)[0]
    );
    assert_eq!(
        plain["actors"]["actorsDict"]["actor-1"]["blocksXML"], yc_xml,
        "无迁移时 blocksXML 逐字节原样"
    );

    // QC:0.9.0 < 0.9.4 —— rotation 取反 + 变量坐标"先还原再重算"⇒ 与源坐标一致
    let qc = convert(nemo_document(yc_xml, ""), Some("0.9.0"));
    assert_eq!(
        qc["actors"]["actorsDict"]["actor-1"]["rotation"], 0,
        "0 取反后仍是 0"
    );
    let variable = &qc["variables"]["variablesDict"]["var-1"];
    assert_eq!(
        variable["position"],
        json!({ "x": -100, "y": 20 }),
        "QC 的坐标还原与主管线的重定心互相抵消"
    );
    // QC 的旧音频块重写:字段搬进 `value[name=audio]` 的影子,槽名映射成 `audio_id`
    let play = &qc["actors"]["actorsDict"]["actor-1"]["nekoBlockJsonList"][0]["statements"]["DO0"];
    assert_eq!(play["type"], "play_audio");
    assert_eq!(play["inputs"]["audio_id"]["type"], "get_play_audio");
    assert_eq!(play["inputs"]["audio_id"]["fields"]["audio_id"], "audio-1");
    assert!(
        play["shadows"]["audio_id"]
            .as_str()
            .is_some_and(|xml| xml.contains(r#"<field name="audio_id">audio-1</field>"#)),
        "{:?}",
        play["shadows"]["audio_id"]
    );
    // QC 迁移注入节点的 id 落在了"迁移后的整份 XML"快照里,那一处也必须走同一张 id 改写表;
    // 否则临时哨兵(控制字符前缀)会落进产物。这条扫整份产物,任何位置残留都抓。
    assert!(
        !serde_json::to_string(&qc)
            .expect("序列化")
            .contains('\u{1}'),
        "产物里不得残留临时 id 哨兵"
    );
}

/// `actor-1` 的积木列表(测试里多一处复用)
fn plain_blocks(document: &Value) -> &Vec<Value> {
    document
        .pointer("/actors/actorsDict/actor-1/nekoBlockJsonList")
        .and_then(Value::as_array)
        .expect("演员积木列表")
}

/// 未映射块降级成占位积木(官方一致):`bcm_translator_text_*` + `disabled: true` + 报告
#[test]
fn unmapped_blocks_become_placeholders_and_are_reported() {
    let actor_xml = concat!(
        r#"<block type="on_phone_shake" id="b0" visible="visible" inline="true" x="0" y="0">"#,
        r#"<next><block type="show_ranking" id="b1" visible="visible" inline="true">"#,
        r#"<field name="direction">positive</field></block></next></block>"#,
    );
    let document = convert(nemo_document(actor_xml, ""), None);
    let root = &actor_blocks(&document)[0];
    assert_eq!(root["type"], "bcm_translator_text_event_block");
    assert_eq!(root["disabled"], true);
    assert!(
        root["mutation"]
            .as_str()
            .is_some_and(|text| text.contains("当手机被摇晃")),
        "{:?}",
        root["mutation"]
    );
    let next = &root["next"];
    assert_eq!(next["type"], "bcm_translator_text_execution_block");
    assert!(
        next["mutation"]
            .as_str()
            .is_some_and(|text| text.contains("显示正序云排行榜")),
        "{:?}",
        next["mutation"]
    );
}

// ===========================================================================
// 真作品门:两份 NEMO 作品 + 官方产物夹具(需要 `temp/harness/`,不入库 ⇒ 缺席即跳过)
// ===========================================================================

/// 官方产物夹具 + 输入(路径,`bcm_version`),与 `temp/harness/out-*.json` 一一对应
///
/// 与 `reverse_tests` / `pipeline` 那些"缺夹具就打印跳过"的真机测试**不同**:本测试是
/// `#[ignore]`(要显式 `--ignored` 才跑)且缺件走**硬 `assert!`**,本来就没有"静默 pass"的出口
/// ⇒ 不需要 `BACKEND_REQUIRE_FIXTURES`(开关家族见 `docs/knowledge/repo-conventions.md` §3ter);
/// 这里只标注这条差别,不改口径。
const REAL_SAMPLES: &[(&str, &str, &str)] = &[
    (
        "download/compile/蛋仔派对2-奥姆返场新盲盒生存赛重做_194684070/user_works/194684070/194684070.bcm",
        "temp/harness/out-eggparty-194684070.json",
        "0.16.2",
    ),
    (
        // 输入在语料目录 `download/`(R2 前在 `temp/harness/`,已挪位置);官方产物夹具仍留在
        // `temp/harness/`(它是可再生的对照物,不属于"采下来的语料")。
        "download/compile/nemo-103791894.bcm",
        "temp/harness/out-wuxian-103791894.json",
        "0.11.0",
    ),
];

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// 真作品端到端:我们的产物过**官方 `validateBcm`** + 与官方产物**语义 diff 在 allow-list 内**
#[test]
#[ignore = "需要 temp/harness(官方 harness + NEMO 真作品夹具)与 download/ 下的真作品,默认不跑"]
fn nemo_real_samples_match_official_products() {
    let root = repo_root();
    let harness = root.join("temp/harness/harness.js");
    assert!(harness.exists(), "缺少官方 harness:{}", harness.display());
    for (input, fixture, version) in REAL_SAMPLES {
        let input_path = root.join(input);
        let fixture_path = root.join(fixture);
        assert!(
            input_path.exists(),
            "缺少 NEMO 输入:{}",
            input_path.display()
        );
        assert!(
            fixture_path.exists(),
            "缺少官方产物夹具:{}",
            fixture_path.display()
        );

        let text = std::fs::read_to_string(&input_path).expect("读入 NEMO 作品");
        let source: Value = serde_json::from_str(&text).expect("解析 NEMO 作品");
        let options = TranslateOptions::new()
            .deterministic_ids(true)
            .source_version(*version);
        let outcome = translate_value(source, TargetEditor::KittenN, &options).expect("转化");

        let ours = root.join("temp/harness").join(format!(
            "ours-{}.json",
            fixture
                .trim_start_matches("temp/harness/out-")
                .trim_end_matches(".json")
        ));
        std::fs::write(
            &ours,
            serde_json::to_string(&outcome.document).expect("序列化"),
        )
        .expect("写产物");

        // ① 官方 validateBcm
        let stdout = run_node(
            &harness,
            &format!(
                "const h=require({:?});const {{validateBcm}}=h.require(87123);\
                 const fs=require('fs');const doc=JSON.parse(fs.readFileSync({:?},'utf8'));\
                 const e=console.error;console.error=()=>{{}};let ok=false;\
                 try{{ok=validateBcm(doc);}}catch(_){{ok=false;}}console.error=e;\
                 console.log(ok?'VALID':'INVALID');",
                harness.to_string_lossy(),
                ours.to_string_lossy(),
            ),
        );
        // harness 的 Node 侧会先打两行 banner,判定本身是最后一行
        let verdict = stdout
            .lines()
            .map(str::trim)
            .rfind(|line| *line == "VALID" || *line == "INVALID")
            .unwrap_or("NO-VERDICT");
        assert_eq!(
            verdict, "VALID",
            "{input}:官方 validateBcm 判定不合法(原始输出:{stdout})"
        );

        // ② 语义 diff(忽略 id/location/createTime/parent_id 与空/缺省等价,见 allow-list)
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(&fixture_path).expect("读夹具"))
                .expect("解析夹具");
        let mut differences = Vec::new();
        diff_value(&outcome.document, &expected, "", &mut differences);
        let tolerated: Vec<String> = differences
            .iter()
            .filter(|difference| allowed(difference))
            .cloned()
            .collect();
        eprintln!(
            "  {}:官方 validateBcm → {verdict};语义 diff 共 {} 处(全部在 allow-list 内)",
            input,
            differences.len()
        );
        eprintln!("  allow-list 内 {} 处,样例:", tolerated.len());
        for item in tolerated.iter().take(3) {
            eprintln!("    {item}");
        }
        let unexpected: Vec<String> = differences
            .iter()
            .filter(|difference| !allowed(difference))
            .cloned()
            .collect();
        assert!(
            unexpected.is_empty(),
            "{input}:与官方产物的差异超出 allow-list(共 {} 处,前 40 条):\n{}",
            differences.len(),
            unexpected
                .iter()
                .take(40)
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        );

        // ③ 块数守恒(官方统计口径:节点类型非空)
        let ours_nodes = count_nodes(&outcome.document);
        let official_nodes = count_nodes(&expected);
        assert_eq!(
            ours_nodes, official_nodes,
            "{input}:产物块数与官方不一致(我们 {ours_nodes} vs 官方 {official_nodes})"
        );
        eprintln!(
            "  {}:输入块数(源 XML 元素)={}  我们的产物块数={ours_nodes}  官方产物块数={official_nodes}",
            input, outcome.report.blocks_total
        );
    }
}

/// 跑一段 node 脚本(与 `mod.rs` 的 harness 门同一做法)
fn run_node(script_path: &std::path::Path, script: &str) -> String {
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(script)
        .current_dir(script_path.parent().expect("harness 目录"))
        .output()
        .expect("调用 node");
    assert!(
        output.status.success(),
        "node 退出码 {:?}:{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// 语义 diff 里**允许/忽略**的键(逐条给出理由,见报告)
const IGNORED_KEYS: &[&str] = &[
    // 官方每次运行现铸 id(我们确定性模式下也一样是自铸),没有可比性
    "id",
    // 工作区坐标:官方 `parseBlock` 把非根积木的 x/y 记成 [0,0],根的位置是编辑器布局,不是语义
    "location",
    // 变量创建时间:官方用 `Date.now()`
    "createTime",
    // 反向推导出来的父指针(两端都有,但值的来源不同)
    "parent_id",
];

/// allow-list 条目的标记前缀(见 [`allowed`])
const ALLOW_ORDER: &str = "ALLOW-ORDER:";

/// 顺序敏感的数组:官方按**源字典插入顺序**产出,`serde_json` 的 `BTreeMap` 里看不到这个顺序
fn is_order_sensitive(path: &str) -> bool {
    let path = path.trim_start_matches('.');
    path == "audios.sortList" || path.starts_with("broadcasts.broadcastsDict")
}

/// 两个数组的元素顺序是否一致
fn same_order(left: &[Value], right: &[Value]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a == b)
}

/// 允许的差异(allow-list):每条都能解释,且不掩盖"我们做错了什么"。
///
/// **只有一条**:`audios.sortList` 与 `broadcasts.broadcastsDict.<场景>` 的**数组顺序**。
/// 官方按 JS 字典的**插入顺序**(= 源文件里的键顺序)产出这两个数组;`serde_json` 的 `Map`
/// 是 `BTreeMap`(按键排序),源文件顺序在解析后就不可见 ⇒ 我们按 id 排序。
/// 两边的**集合完全相同**(`diff_value` 对这两条路径按集合比较,集合不同照样报差异),
/// 差异只在先后;这也是本层唯一的顺序敏感点(变量重名去重同源,但两份真作品无重名)。
fn allowed(difference: &str) -> bool {
    difference.starts_with(ALLOW_ORDER)
}

/// 普通对象比较(键集合并集,逐键递归;忽略键与"空 = 缺省"等价在这里统一处理)
fn diff_object(
    left: &serde_json::Map<String, Value>,
    right: &serde_json::Map<String, Value>,
    path: &str,
    out: &mut Vec<String>,
) {
    let mut keys: Vec<&String> = left.keys().chain(right.keys()).collect();
    keys.sort();
    keys.dedup();
    for key in keys {
        if IGNORED_KEYS.contains(&key.as_str()) {
            continue;
        }
        // 官方 `currentAudioId = sortList[0]`,是 `audios.sortList` 的直接派生值:
        // 顺序差异已经在 sortList 那条 allow-list 上记过一次,这里不重复报(集合仍严格比)。
        if key == "currentAudioId" && path.trim_start_matches('.') == "audios" {
            continue;
        }
        let child = format!("{path}.{key}");
        match (left.get(key), right.get(key)) {
            (Some(a), Some(b)) => diff_value(a, b, &child, out),
            (Some(a), None) => {
                if !is_empty_default(a) {
                    out.push(format!("{child}: 我们多出 {a}"));
                }
            }
            (None, Some(b)) => {
                if !is_empty_default(b) {
                    out.push(format!("{child}: 我们缺 {b}"));
                }
            }
            (None, None) => {}
        }
    }
}

/// 递归语义 diff(路径形如 `actors.actorsDict.actor-1.nekoBlockJsonList.0.type`)
fn diff_value(ours: &Value, theirs: &Value, path: &str, out: &mut Vec<String>) {
    match (ours, theirs) {
        (Value::Object(left), Value::Object(right)) => {
            // `shadows` 映射里,槽名是字面量(`NAME`/`PARAMS0`/`message`…),而程序集实参槽的键是
            // **新铸的形参 id**(两端都铸,值必然不同)⇒ 字面量键严格比,id 键按**值的多重集**比。
            if path.ends_with(".shadows") {
                diff_shadow_map(left, right, path, out);
                return;
            }
            diff_object(left, right, path, out);
        }
        (Value::Array(left), Value::Array(right)) => {
            // allow-list 条目:这两个数组由官方按**源字典的插入顺序**产出,而 `serde_json::Map`
            // 是按**键排序**的 BTreeMap ⇒ 源顺序解析后不可见,我们只能按 id 排序。
            // 因此这里按**集合**比:集合不同 ⇒ 真差异(照报);只是先后不同 ⇒ 记一条 allow-list 差异。
            if is_order_sensitive(path) {
                let mut ours_sorted: Vec<String> = left.iter().map(Value::to_string).collect();
                let mut theirs_sorted: Vec<String> = right.iter().map(Value::to_string).collect();
                ours_sorted.sort();
                theirs_sorted.sort();
                if ours_sorted != theirs_sorted {
                    out.push(format!(
                        "{path}: 集合不同 ours={ours_sorted:?} official={theirs_sorted:?}"
                    ));
                } else if !same_order(left, right) {
                    out.push(format!(
                        "{ALLOW_ORDER} {path}: 集合相同、先后不同(源字典顺序在 serde_json 里不可见)"
                    ));
                }
                return;
            }
            if left.len() != right.len() {
                out.push(format!(
                    "{path}: 长度不同 ours={} official={}",
                    left.len(),
                    right.len()
                ));
                return;
            }
            for (index, (a, b)) in left.iter().zip(right.iter()).enumerate() {
                diff_value(a, b, &format!("{path}.{index}"), out);
            }
        }
        _ => {
            if !same_scalar(ours, theirs) {
                out.push(format!("{path}: ours={ours} official={theirs}"));
            }
        }
    }
}

/// 标量比较:数字按 `f64` 比(消掉 `10` 与 `10.0` 的表示差);
/// 影子/变异 **XML 串**里的 `id="…"` 归一(官方每次现铸随机 id,见 docs/rounds/27 §9.2),其余严格相等
fn same_scalar(ours: &Value, theirs: &Value) -> bool {
    match (ours, theirs) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::String(a), Value::String(b)) if a.starts_with('<') || b.starts_with('<') => {
            normalize_xml_ids(a) == normalize_xml_ids(b)
        }
        _ => ours == theirs,
    }
}

/// 把 XML 串里的 `id="…"` 换成 `id="*"`(只用于比较,不改产物)
fn normalize_xml_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("id=\"") {
        out.push_str(&rest[..at]);
        out.push_str("id=\"*\"");
        let after = &rest[at + 4..];
        match after.find('"') {
            Some(end) => rest = &after[end + 1..],
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// 看起来像 uuid(我们确定性模式与官方随机 id 都是这个形态)
fn looks_like_id(key: &str) -> bool {
    key.len() == 36
        && key.chars().filter(|c| *c == '-').count() == 4
        && key.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

/// `shadows` 映射:字面量键(槽名)逐键严格比,id 键(`<形参 id>`)按值多重集比
fn diff_shadow_map(
    ours: &serde_json::Map<String, Value>,
    theirs: &serde_json::Map<String, Value>,
    path: &str,
    out: &mut Vec<String>,
) {
    let split = |map: &serde_json::Map<String, Value>| {
        let mut literal = serde_json::Map::new();
        let mut ids: Vec<String> = Vec::new();
        for (key, value) in map {
            if looks_like_id(key) {
                ids.push(normalize_xml_ids(value.as_str().unwrap_or_default()));
            } else {
                literal.insert(key.clone(), value.clone());
            }
        }
        ids.sort();
        (literal, ids)
    };
    let (literal_ours, ids_ours) = split(ours);
    let (literal_theirs, ids_theirs) = split(theirs);
    // 注意:这里用 `diff_object` 而不是 `diff_value` —— 路径仍以 `.shadows` 结尾,
    // 再走一次 `diff_value` 会重新进入本函数(无限递归)。
    diff_object(&literal_ours, &literal_theirs, path, out);
    if ids_ours != ids_theirs {
        out.push(format!(
            "{path}: 形参影子槽不同 ours={ids_ours:?} official={ids_theirs:?}"
        ));
    }
}

/// 空/缺省等价:官方 `JSON.stringify` 会丢掉 `undefined` 属性,而我们的结构体序列化会省略空容器/`false`
///
/// 逐条:布尔 `false`(所有 `ms.flag` 都默认 false)、空串、空数组、空对象、`null`。
fn is_empty_default(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(flag) => !*flag,
        Value::String(text) => text.is_empty(),
        Value::Array(items) => items.is_empty(),
        Value::Object(map) => map.is_empty(),
        Value::Number(_) => false,
    }
}

/// 官方口径的块数(与 `temp/harness/harness.js` 的 `countBlocks` 同序同义)
fn count_nodes(document: &Value) -> usize {
    let mut total = 0;
    for section in ["actors", "scenes", "procedures"] {
        let Some(container) = document.get(section).and_then(Value::as_object) else {
            continue;
        };
        for dict in container.values() {
            let Some(dict) = dict.as_object() else {
                continue;
            };
            for entity in dict.values() {
                if let Some(list) = entity.get("nekoBlockJsonList").and_then(Value::as_array) {
                    for node in list {
                        count_node(node, &mut total);
                    }
                }
            }
        }
    }
    total
}

fn count_node(node: &Value, total: &mut usize) {
    let Some(map) = node.as_object() else {
        return;
    };
    if map.get("type").and_then(Value::as_str).is_some() {
        *total += 1;
    }
    for key in ["inputs", "statements"] {
        if let Some(map) = map.get(key).and_then(Value::as_object) {
            for child in map.values() {
                count_node(child, total);
            }
        }
    }
    if let Some(next) = map.get("next") {
        count_node(next, total);
    }
}

/// 前端元素计数口径:源 XML 里的 `block`/`shadow`/`empty`(与 `temp/harness/tables-output.txt` 一致)
#[test]
fn nemo_source_element_census_matches_research_numbers() {
    // 这条只是把"研究里的输入块数"口径固化在代码里:统计源 XML 元素(不含 `next`/`value`/`field`)
    let actor_xml = concat!(
        r#"<block type="wait" id="b0" visible="visible" inline="true">"#,
        r#"<value name="time"><shadow type="math_number" id="s1" visible="visible">"#,
        r#"<field name="NUM">1</field></shadow></value>"#,
        r#"<next><block type="controls_if" id="b1" visible="visible" inline="true">"#,
        r#"<value name="if0"><empty type="logic_empty" id="e1" visible="visible"/></value>"#,
        r#"<statement name="DO0"><block type="self_appear" id="b2" visible="visible" inline="true"/></statement>"#,
        r#"</block></next></block>"#,
    );
    let parsed = crate::core::convert::translate::xml::parse_fragment(actor_xml).expect("解析");
    let mut blocks = 0;
    let mut shadows = 0;
    let mut empties = 0;
    fn walk(
        nodes: &[crate::core::convert::translate::xml::XmlNode],
        b: &mut usize,
        s: &mut usize,
        e: &mut usize,
    ) {
        for node in nodes {
            match node.tag.as_str() {
                "block" => *b += 1,
                "shadow" => *s += 1,
                "empty" => *e += 1,
                _ => {}
            }
            walk(&node.children, b, s, e);
        }
    }
    walk(&parsed, &mut blocks, &mut shadows, &mut empties);
    assert_eq!((blocks, shadows, empties), (3, 1, 1));
    let _ = EditorType::Nemo;
}

// ===========================================================================
// 反编译侧的内存入口(fp 门面走的就是它:`translate_work` 全程不落盘、不下资源)
// ===========================================================================

/// 不联网的 `HttpClient` 桩:内存入口根本不用网络(用了就是被调用到,直接报错暴露)
#[derive(Clone)]
struct OfflineHttp;

impl crate::core::convert::shared::HttpClient for OfflineHttp {
    fn get_json(
        &self,
        _url: &str,
        _headers: Option<Vec<(String, String)>>,
    ) -> crate::core::convert::shared::Result<Value> {
        Err(crate::core::convert::ConvertError::InvalidResponse(
            "测试桩:内存入口不应联网".into(),
        ))
    }
    fn get_binary(&self, _url: &str) -> crate::core::convert::shared::Result<Vec<u8>> {
        Err(crate::core::convert::ConvertError::InvalidResponse(
            "测试桩:内存入口不应联网".into(),
        ))
    }
    fn get_text(&self, _url: &str) -> crate::core::convert::shared::Result<String> {
        Err(crate::core::convert::ConvertError::InvalidResponse(
            "测试桩:内存入口不应联网".into(),
        ))
    }
    fn box_clone(&self) -> Box<dyn crate::core::convert::shared::HttpClient> {
        Box::new(self.clone())
    }
}

/// NEMO 的内存入口:直接给明文编辑版 + 源版本(`bcm_version` 只存在于作品元信息里)
#[test]
fn nemo_decompiler_offers_in_memory_editable_document() {
    use crate::core::convert::decompile::config::DecompilerConfig;
    use crate::core::convert::decompile::editors::NemoDecompiler;
    use crate::core::convert::decompile::work::{RawWorkData, WorkInfo};
    use crate::core::convert::decompile::{DecompilerContext, WorkDecompiler};
    use crate::core::convert::shared::IdGenerator;
    use std::sync::Arc;

    let config = Arc::new(DecompilerConfig::default());
    let context = DecompilerContext {
        output_dir: None,
        resource_concurrency: 1,
        download_resources: false,
        work_info: WorkInfo {
            id: crate::core::convert::WorkId::new(194684070),
            name: "测试作品".to_string(),
            work_type: EditorType::Nemo,
            user_id: 0,
            bcm_version: "0.16.2".to_string(),
            preview: None,
        },
        http_client: Box::new(OfflineHttp),
        id_generator: IdGenerator::new(),
        config,
    };
    let document = json!({ "actors": { "actors_dict": {} }, "app_version": "2.3.0" });
    let raw = RawWorkData::Nemo(
        Arc::new(document.clone()),
        Arc::new(json!({ "bcm_version": "0.11.0", "name": "测试作品" })),
    );
    let editable = NemoDecompiler
        .editable_document(&raw, &context)
        .expect("内存入口")
        .expect("NEMO 一定给内存编辑版");
    assert_eq!(editable.document, document);
    assert_eq!(editable.source_version, "0.11.0");

    // 非 NEMO 数据:明确报错(而不是给一份空文档)
    let wrong = RawWorkData::Wood(Arc::new(json!({})));
    assert!(NemoDecompiler.editable_document(&wrong, &context).is_err());
}

// ===========================================================================
// 编码失败不再静默丢块(W6②:`filter_map(ok())` 已删)
// ===========================================================================

/// 单根编码失败**进报告**(而不是被吞掉):成功的根照常进产物、失败的根不进产物且必须可见
///
/// 说明:`BlockJson` 的字段(字符串 / `Value` / 子节点)全都可序列化,**正常数据构造不出**
/// `to_value` 失败 ⇒ 这条防御分支直接投喂一个**真实的** `serde_json` 编码错误
/// (非字符串 map 键报的就是它;与 `BlockJson::to_value` 走同一个 `serde_json::to_value`
/// → `ConvertError` 通道),断言生产分支"记报告 + 跳过该根 + 算有损"。
#[test]
fn encode_failure_enters_report_instead_of_being_dropped() {
    use crate::core::convert::shared::ConvertError;
    use crate::core::convert::translate::{TranslateReport, TranslateWarning, nemo};

    // 非字符串 map 键:serde_json 报 `key must be a string`(真实编码错误,非手搓)
    let bad: std::collections::BTreeMap<(i32, i32), i32> =
        std::collections::BTreeMap::from([((1, 2), 3)]);
    let error =
        ConvertError::from(serde_json::to_value(&bad).expect_err("非字符串 map 键必须编码失败"));

    let mut report = TranslateReport::new(EditorType::Nemo, TargetEditor::KittenN);
    let mut roots = Vec::new();
    nemo::push_root(&mut roots, Ok(json!({ "type": "wait" })), &mut report);
    assert_eq!(roots.len(), 1, "成功的根照常进产物");
    assert!(report.warnings().is_empty(), "成功的根不该产生告警");

    nemo::push_root(&mut roots, Err(error), &mut report);
    assert_eq!(roots.len(), 1, "失败的根不进产物(块确实少了)");
    match report.warnings() {
        [TranslateWarning::DroppedField { path }] => {
            assert!(
                path.starts_with("nekoBlockJsonList: "),
                "路径要指认失败的编码面:{path}"
            );
        }
        other => panic!("编码失败必须进报告:{other:?}"),
    }
    assert!(report.is_lossy(), "编码失败 = 丢块 ⇒ 必须计入有损");
    assert_eq!(report.blocks_total, 0);
}

/// 实体级并行的守门门(真作品;缺样本即跳过,与仓库其它夹具测试同约定):
/// NEMO 方向并发 1 与并发 8 必须**逐字节相同**,且并发 8 那一次必须**真的**开了多线程
/// (否则"1 vs 8 相同"只是串行 vs 串行 —— 方案 25 §9 的空门口径)。
#[test]
fn nemo_entity_parallelism_is_byte_identical_and_really_parallel() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "download/compile/蛋仔派对2-奥姆返场新盲盒生存赛重做_194684070/user_works/194684070/194684070.bcm",
    );
    if !path.exists() {
        super::missing_fixture(&format!("真作品样本 {}", path.display()));
        return;
    }
    let text = std::fs::read_to_string(&path).expect("读取样本");
    let source: Value = serde_json::from_str(&text).expect("样本应是明文编辑版 JSON");

    let convert_at = |concurrency: usize| {
        let options = TranslateOptions::new()
            .deterministic_ids(true)
            .entity_concurrency(concurrency)
            .source_version("0.16.2");
        let outcome = translate_value(source.clone(), TargetEditor::KittenN, &options)
            .expect("NEMO → KN 转化");
        (
            serde_json::to_string(&outcome.document).expect("序列化产物"),
            outcome.report,
        )
    };

    let (serial, serial_report) = convert_at(1);
    let (parallel, parallel_report) = convert_at(8);
    assert_eq!(serial_report.entity_workers, 1, "并发 1 不应开工作线程");
    // 该样本有 847 个演员 + 38 个场景,实体数远大于核数 ⇒ "没开线程"只可能是空门
    // (把可用核数折成 1 的环境里只报事实,不判失败)
    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    if available >= 2 {
        assert!(
            parallel_report.entity_workers > 1,
            "请求并发 8 应真的并行(可用核数 {available})"
        );
    }
    assert_eq!(serial, parallel, "NEMO 实体级并行必须与并发 1 逐字节相同");
    assert!(
        !serial.contains('\u{1}'),
        "产物里不得残留临时 id 哨兵(临时 id 方案见 model::TEMP_ID_PREFIX)"
    );
}

/// **常驻字节等价门**(`rounds/49`):文件路径吃的**流式产物**(三处块表不建 `Value`)与内存路径
/// 逐字节相同
///
/// 用真作品样本(847 演员 + 38 场景 + 程序集,三处挂点全覆盖)把三条口径钉在一起:
/// 内存 `Value`、产物物化回 `Value`、产物直接写出的字节。块表编码口径是
/// [`nemo::convert_nemo_document_product`] 的 `ShieldPolicy::OnlyWhenTrue`(不补假 `shield`),
/// 与旧 `nemo::tree_to_json` 一致 —— 这三条若分叉,产物就与历史基线不同。
#[test]
fn nemo_product_path_is_byte_identical_to_value_path() {
    use crate::core::convert::translate::{TranslateReport, nemo};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "download/compile/蛋仔派对2-奥姆返场新盲盒生存赛重做_194684070/user_works/194684070/194684070.bcm",
    );
    if !path.exists() {
        super::missing_fixture(&format!("真作品样本 {}", path.display()));
        return;
    }
    let text = std::fs::read_to_string(&path).expect("读取样本");
    let source: Value = serde_json::from_str(&text).expect("样本应是明文编辑版 JSON");
    let options = TranslateOptions::new()
        .deterministic_ids(true)
        .source_version("0.16.2");

    let mut value_report = TranslateReport::new(EditorType::Nemo, TargetEditor::KittenN);
    let value =
        nemo::convert_nemo_document(&source, &options, &mut value_report).expect("内存路径");
    let expected = serde_json::to_string(&value).expect("序列化内存路径产物");

    let mut product_report = TranslateReport::new(EditorType::Nemo, TargetEditor::KittenN);
    let product = nemo::convert_nemo_document_product(&source, &options, &mut product_report)
        .expect("流式产物");
    let mut streamed = Vec::new();
    product.write_to(&mut streamed).expect("流式写出");
    assert_eq!(
        String::from_utf8_lossy(&streamed),
        expected,
        "流式写出的字节必须与内存路径逐字节相同"
    );
    assert_eq!(
        serde_json::to_string(&product.into_value().expect("物化")).expect("序列化产物"),
        expected,
        "产物物化回 Value 也必须与内存路径逐字节相同"
    );
    // 份量对账:两条路径的块数与告警逐条相同(共用一份装配的直接后果)
    assert_eq!(
        product_report.blocks_converted, value_report.blocks_converted,
        "块数"
    );
    assert_eq!(
        product_report.warnings().len(),
        value_report.warnings().len(),
        "告警条数"
    );
}

/// 流式写出口径钉桩:块表数字归一在**树上**做,必须与"整棵树物化成 `Value` 再归一"等价
///
/// `BlockJson` 的 `Value` 字段共四个(`location`/`fields`/`field_constraints`/`extra`),
/// 这条测试把四个位置 + 三个子树槽位(`inputs`/`statements`/`next`)都放上"整数值的浮点",
/// 逐字段比两条口径的产物。
#[test]
fn tree_normalization_covers_every_value_field() {
    use crate::core::convert::translate::nemo;
    let materialize = |tree: &super::model::BlockTree| {
        Value::Array(
            super::model::tree_to_value(tree, super::model::ShieldPolicy::Always).expect("物化"),
        )
    };
    let tree = super::model::parse_kn_entity(&serde_json::json!([
        {
            "type": "a",
            "id": "n",
            "location": [1.0, 2.5],
            "fields": { "NUM": 3.0, "KEEP": 4.5 },
            "field_constraints": { "min": 5.0 },
            "extra": { "x": 6.0, "深": { "y": 7.0 } },
            "inputs": { "V": { "type": "b", "id": "m", "fields": { "NUM": 8.0 } } },
            "statements": { "DO": { "type": "c", "id": "o", "extra": { "y": 9.0 } } },
            "next": { "type": "d", "id": "p", "fields": { "NUM": 10.0 } }
        }
    ]))
    .expect("解析样例树");

    // 旧口径:整棵树物化成 `Value`,再整份递归归一
    let mut materialized = materialize(&tree);
    let raw = materialized.clone();
    nemo::normalize_integral_numbers(&mut materialized);
    assert_ne!(
        materialized, raw,
        "样例必须真的含'整数值的浮点',否则本测试是空门"
    );

    // 流式口径:在 typed 树上就地归一,再物化
    let mut typed = tree.clone();
    nemo::normalize_tree_numbers(&mut typed);
    let normalized_typed = materialize(&typed);

    assert_eq!(
        normalized_typed, materialized,
        "树上归一必须覆盖整树归一的每个数字位置"
    );
    // 顺带钉住两个方向:整数值的浮点折成整数、真小数不动
    assert_eq!(normalized_typed[0]["fields"]["NUM"], json!(3));
    assert_eq!(normalized_typed[0]["fields"]["KEEP"], json!(4.5));
    assert_eq!(normalized_typed[0]["location"], json!([1, 2.5]));
    assert_eq!(normalized_typed[0]["extra"]["深"]["y"], json!(7));
    assert_eq!(
        normalized_typed[0]["inputs"]["V"]["fields"]["NUM"],
        json!(8)
    );
    assert_eq!(
        normalized_typed[0]["statements"]["DO"]["extra"]["y"],
        json!(9)
    );
    assert_eq!(normalized_typed[0]["next"]["fields"]["NUM"], json!(10));
}
