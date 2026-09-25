//! 反向转化(KN → Kitten4)与往返测试。

use super::assembly::*;
use super::*;

mod reverse_tests_inner {
    //! 反向(KN → Kitten4)单测:类型/字段/槽位反演、`KC`/`zC` 的逆、邻接表往返、
    //! 以及「KN → Kitten4 → KN 类型多重集守恒」(docs/rounds/20 §7 Phase 4 验收)。

    use super::assembly::*;
    use super::*;
    use crate::core::convert::translate::model::{BlockJson, BlockTree};
    use crate::core::convert::translate::{mapping, model};
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    fn reverse(block: Value, landscape: bool) -> (BlockJson, TranslateReport) {
        let mut tree = BlockTree::new(vec![BlockJson::from_value(&block).expect("块 JSON")]);
        let mut ids = model::IdSource::new(true);
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        mapping::translate_kn_to_kitten(&mut tree, landscape, &mut report);
        (tree.roots.remove(0), report)
    }

    fn field<'a>(node: &'a BlockJson, name: &str) -> Option<&'a str> {
        node.fields.get(name).and_then(Value::as_str)
    }

    fn slots<V>(map: &BTreeMap<String, V>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }

    #[test]
    fn type_map_keeps_identity_inverts_renames_and_reports_gaps() {
        // 恒等:`LC` 里名字相同的条目原样保留
        let (node, report) = reverse(json!({"type": "repeat_forever", "id": "a"}), false);
        assert_eq!(node.kind, "repeat_forever");
        assert!(report.warnings().is_empty());

        // 改名:`set_sprite_style ← set_costume`、`bump_into ← bump`、`text ← get_split_options`
        for (kn, kitten) in [
            ("set_sprite_style", "set_costume"),
            ("bump_into", "bump"),
            ("text", "get_split_options"),
            ("math_function", "math_single"),
            ("get_play_audio", "get_audios"),
            ("variables_get", "variables_get"),
            ("coordinate_of_sprite", "coordinate_of_sprite"),
            ("logic_compare", "logic_compare"),
        ] {
            let (node, _) = reverse(json!({ "type": kn, "id": "b" }), false);
            assert_eq!(node.kind, kitten, "{kn} 应反演成 {kitten}");
        }

        // 歧义(多个 Kitten 原类型):保留 KN 名 + 报告
        let (node, report) = reverse(json!({"type": "change_variables", "id": "c"}), false);
        assert_eq!(node.kind, "change_variables");
        assert!(
            report.warnings().iter().any(|w| matches!(
                w,
                TranslateWarning::AmbiguousType { kind, .. } if kind == "change_variables"
            )),
            "歧义类型必须报告:{:#?}",
            report.warnings()
        );

        // KN 原生、Kitten 侧无来源:保留 + UnmappedBlock
        let (node, report) = reverse(json!({"type": "temporary_list", "id": "d"}), false);
        assert_eq!(node.kind, "temporary_list");
        assert_eq!(
            report.warnings(),
            [TranslateWarning::UnmappedBlock {
                kind: "temporary_list".into()
            }]
        );

        // `calculate` 是 KN 原生块,但正向会把它降级 → 反向不可逆,必须报告
        let (node, report) = reverse(json!({"type": "calculate", "id": "e"}), false);
        assert_eq!(node.kind, "calculate");
        assert_eq!(
            report.warnings(),
            [TranslateWarning::UnmappedBlock {
                kind: "calculate".into()
            }]
        );
    }

    #[test]
    fn placeholder_inverts_back_via_mutation_title() {
        // `RC` 是"中文标题 → 原 Kitten 类型"的可逆表:占位积木按 mutation 正文还原
        let (node, report) = reverse(
            json!({
                "type": "bcm_translator_text_execution_block",
                "id": "a",
                "disabled": true,
                "shadows": { "TITLE_HEAD": "" },
                "mutation": "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"0\">未命名模型: 添加训练数据 特征1{value} 到{value1}</mutation>"
            }),
            false,
        );
        assert_eq!(node.kind, "ai_lab_add_data");
        assert_eq!(node.mutation, None, "正向会重新生成 mutation");
        assert!(node.shadows.is_empty(), "TITLE_HEAD 是正向注入的");
        assert!(report.warnings().is_empty(), "可逆的降级不该报损失");

        // 标题对不上(或不在表里)→ 保留占位积木 + 报告
        let (node, report) = reverse(
            json!({
                "type": "bcm_translator_text_execution_block",
                "id": "b",
                "mutation": "<mutation items=\"0\">不存在的标题</mutation>"
            }),
            false,
        );
        assert_eq!(node.kind, "bcm_translator_text_execution_block");
        assert_eq!(
            report.warnings(),
            [TranslateWarning::UnmappedBlock {
                kind: "bcm_translator_text_execution_block".into()
            }]
        );
    }

    #[test]
    fn inverts_fields_slots_and_type_level_specials() {
        // math_arithmetic:`fields.type`(取值被映射过)→ `fields.OP`
        let (node, _) = reverse(
            json!({"type": "math_arithmetic", "id": "a", "fields": {"type": "multiply"}}),
            false,
        );
        assert_eq!(node.kind, "math_arithmetic");
        assert_eq!(field(&node, "OP"), Some("MULTIPLY"));

        // set_sprite_style:`style_id` 槽位 → `index`(Kitten 侧槽位名)
        let shadow = "<shadow xmlns=\"http://www.w3.org/1999/xhtml\" type=\"get_styles\" id=\"s\" visible=\"visible\"><field name=\"index\">3</field></shadow>";
        let (node, _) = reverse(
            json!({"type": "set_sprite_style", "id": "b", "shadows": {"style_id": shadow}}),
            false,
        );
        assert_eq!(node.kind, "set_costume");
        assert_eq!(slots(&node.shadows), vec!["index"]);
        assert!(
            node.shadows["index"].contains("type=\"get_current_costume\""),
            "影子内的类型同样要反演:{}",
            node.shadows["index"]
        );

        // variables_set:`variable` → `VAR`
        let (node, _) = reverse(
            json!({"type": "variables_set", "id": "c", "fields": {"variable": "v1"}}),
            false,
        );
        assert_eq!(field(&node, "VAR"), Some("v1"));

        // coordinate_of_sprite → get_3 + attribute(反向 APPEARANCE_ATTRIBUTE),`sprite` 取值回 __self
        let (node, _) = reverse(
            json!({"type": "coordinate_of_sprite", "id": "d", "fields": {"coordinate": "y", "sprite": "--self"}}),
            false,
        );
        assert_eq!(node.kind, "get_3");
        assert_eq!(field(&node, "attribute"), Some("1"));
        assert_eq!(field(&node, "sprite"), Some("__self"));
        assert!(!node.fields.contains_key("coordinate"), "派生字段要清掉");

        // self_set_position_x → self_set_position + coordinary
        let (node, _) = reverse(json!({"type": "self_set_position_x", "id": "e"}), false);
        assert_eq!(node.kind, "self_set_position");
        assert_eq!(field(&node, "coordinary"), Some("x"));

        // stop → terminate(`scope` 是正向注入的)
        let (node, _) = reverse(
            json!({"type": "stop", "id": "f", "fields": {"scope": "0"}}),
            false,
        );
        assert_eq!(node.kind, "terminate");
        assert!(node.fields.is_empty());

        // self_appear + value=disappear → self_disappear
        let (node, _) = reverse(
            json!({"type": "self_appear", "id": "g", "fields": {"value": "disappear"}}),
            false,
        );
        assert_eq!(node.kind, "self_disappear");
        let (node, _) = reverse(
            json!({"type": "self_appear", "id": "h", "fields": {"value": "appear"}}),
            false,
        );
        assert_eq!(node.kind, "self_appear");
        assert!(node.fields.is_empty());

        // text_join 的 `ADD0` → `VALUE`
        let (node, _) = reverse(
            json!({"type": "text_join", "id": "i", "inputs": {"ADD0": {"type": "text", "id": "j"}}}),
            false,
        );
        assert_eq!(node.kind, "text_join");
        assert_eq!(slots(&node.inputs), vec!["VALUE"]);

        // logic_negate:`logic` → `BOOL`
        let (node, _) = reverse(
            json!({"type": "logic_negate", "id": "k", "inputs": {"logic": {"type": "logic_boolean", "id": "l"}}}),
            false,
        );
        assert_eq!(slots(&node.inputs), vec!["BOOL"]);

        // text_select → text_select_changeable:`items` 加一
        let (node, _) = reverse(
            json!({"type": "text_select", "id": "m", "mutation": "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"2\"></mutation>"}),
            false,
        );
        assert_eq!(node.kind, "text_select_changeable");
        assert_eq!(
            node.mutation.as_deref(),
            Some("<mutation xmlns=\"http://www.w3.org/1999/xhtml\" items=\"3\"></mutation>")
        );
    }

    #[test]
    fn folds_pure_list_get_and_unwraps_gc_wrappers() {
        // 列表积木:`inputs.list` 的 pure_list_get 影子折回 `fields.list`(Kitten 侧是 `VAR`)
        let (node, _) = reverse(
            json!({
                "type": "list_append", "id": "a",
                "inputs": { "list": { "type": "pure_list_get", "id": "s", "is_shadow": true, "is_output": true, "fields": { "list": "list-1" } } },
                "shadows": { "list": "<shadow type=\"pure_list_get\"/>" }
            }),
            false,
        );
        assert_eq!(node.kind, "list_append");
        assert_eq!(field(&node, "VAR"), Some("list-1"));
        assert!(node.inputs.is_empty() && node.shadows.is_empty());

        // 横屏坐标壳:`self_move_to` 的 x 被 `math_arithmetic divide 1.3` 包住 → 拆回内层
        let (node, _) = reverse(
            json!({
                "type": "self_move_to", "id": "b",
                "inputs": { "x": {
                    "type": "math_arithmetic", "id": "w", "parent_id": "b",
                    "fields": { "type": "divide" },
                    "inputs": {
                        "A": { "type": "math_number", "id": "n1", "is_shadow": true, "fields": { "NUM": "3" } },
                        "B": { "type": "math_number", "id": "n2", "is_shadow": true, "fields": { "NUM": "1.3" } }
                    }
                } }
            }),
            true,
        );
        assert_eq!(node.inputs["x"].kind, "math_number");
        assert_eq!(field(&node.inputs["x"], "NUM"), Some("3"));

        // 竖屏不拆(正向竖屏也不包)
        let wrapper = json!({
            "type": "self_move_to", "id": "c",
            "inputs": { "x": {
                "type": "math_arithmetic", "id": "w2",
                "fields": { "type": "divide" },
                "inputs": {
                    "A": { "type": "math_number", "id": "n3", "fields": { "NUM": "3" } },
                    "B": { "type": "math_number", "id": "n4", "fields": { "NUM": "1.3" } }
                }
            } }
        });
        let (node, _) = reverse(wrapper, false);
        assert_eq!(node.inputs["x"].kind, "math_arithmetic");

        // `set_camera_alpha` 的 `100 - x` 壳
        let (node, _) = reverse(
            json!({
                "type": "set_camera_alpha", "id": "d",
                "inputs": { "camera_alpha": {
                    "type": "math_arithmetic", "id": "w3",
                    "fields": { "type": "minus" },
                    "inputs": {
                        "A": { "type": "math_number", "id": "n5", "fields": { "NUM": "100" } },
                        "B": { "type": "variables_get", "id": "v1", "fields": { "variable": "var-1" } }
                    }
                } }
            }),
            false,
        );
        assert_eq!(
            slots(&node.inputs),
            vec!["opcity"],
            "`camera_alpha` 槽位在 Kitten 侧叫 `opcity`"
        );
        assert_eq!(node.inputs["opcity"].kind, "variables_get");
        assert_eq!(field(&node.inputs["opcity"], "VAR"), Some("var-1"));
    }

    #[test]
    fn kc_inverse_restores_name_mutation_and_arg_slots() {
        // 正向 `KC` 的产物:fields.NAME = 程序集 id、mutation def_id/name/type + <arg>、
        // 每个 String 形参在 id 槽位复制了输入 + 挂 math_number 影子
        let dict = json!({ "proceduresDict": { "proc-1": {
            "id": "proc-1", "name": "非线性移动", "type": "NORMAL",
            "params": [
                { "id": "label-1", "type": "Label", "name": "非线性移动" },
                { "id": "px", "type": "String", "name": "X" },
                { "id": "pspeed", "type": "String", "name": "Speed" }
            ],
            "nekoBlockJsonList": [{ "type": "procedures_2_defnoreturn", "id": "proc-1", "fields": { "NAME": "proc-1" } }]
        } } });
        let procedures = model::parse_kn_procedures(&dict).expect("解析程序集");
        assert_eq!(procedures.len(), 1);
        assert_eq!(procedures[0].params.len(), 3);

        let mut tree = BlockTree::new(vec![BlockJson::from_value(&json!({
            "type": "procedures_2_callnoreturn",
            "id": "call-1",
            // `pspeed` 是"新版 KN 编辑器把 Custom/List 形参值内联在按形参 id 命名的字段里"那种形态
            "fields": { "NAME": "proc-1", "pspeed": "5" },
            "mutation": "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" def_id=\"proc-1\" name=\"proc-1\" type=\"NORMAL\"><arg id=\"label-1\" content=\"非线性移动\" type=\"Label\"></arg><arg id=\"px\" content=\"X\" type=\"String\"></arg><arg id=\"pspeed\" content=\"Speed\" type=\"String\"></arg></mutation>",
            "shadows": { "NAME": "", "px": "<shadow type=\"math_number\"/>", "pspeed": "<shadow type=\"math_number\"/>" },
            "inputs": {
                "ARG0": { "type": "math_number", "id": "a0", "fields": { "NUM": "160" } },
                "px": { "type": "math_number", "id": "a0", "fields": { "NUM": "160" } }
            }
        })).expect("调用点")]);
        let mut ids = model::IdSource::new(true);
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        model::unrewrite_calls(&mut tree, &procedures, &mut ids, &mut report);

        let call = &tree.roots[0];
        assert_eq!(
            field(call, "NAME"),
            Some("非线性移动"),
            "id 必须换回程序集名"
        );
        assert_eq!(
            slots(&call.inputs),
            vec!["ARG0"],
            "只有连了线的实参有输入:{}",
            call.inputs.len()
        );
        assert_eq!(slots(&call.inputs), vec!["ARG0"], "只有连了线的实参有输入");
        assert_eq!(call.inputs["ARG0"].id.as_deref(), Some("a0"));
        assert_eq!(slots(&call.shadows), vec!["ARG0", "ARG1", "NAME"]);
        assert!(call.shadows["ARG0"].contains("type=\"default_value\""));
        assert_eq!(call.shadows["NAME"], "");
        // 内联取值进 mutation 的 `value` 与对应 `ARG<j>` 影子(不是丢掉 + 报损)
        assert!(
            call.shadows["ARG1"].contains(">5</field>"),
            "{}",
            call.shadows["ARG1"]
        );
        assert_eq!(
            call.mutation.as_deref(),
            Some(concat!(
                "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" name=\"非线性移动\" def_id=\"proc-1\">",
                "<procedures_2_parameter_shadow name=\"X\" value=\"0\"></procedures_2_parameter_shadow>",
                "<procedures_2_parameter_shadow name=\"Speed\" value=\"5\"></procedures_2_parameter_shadow></mutation>"
            ))
        );
        assert!(
            report.warnings().is_empty(),
            "内联取值可还原,不该报损:{:#?}",
            report.warnings()
        );
    }

    #[test]
    fn zc_inverse_rebuilds_definition_root() {
        let dict = json!({ "proceduresDict": { "proc-1": {
            "id": "proc-1", "name": "非线性移动", "type": "NORMAL",
            "params": [
                { "id": "label-1", "type": "Label", "name": "非线性移动" },
                { "id": "px", "type": "String", "name": "X" }
            ],
            "nekoBlockJsonList": [{
                "type": "procedures_2_defnoreturn", "id": "proc-1",
                "fields": { "NAME": "proc-1" }, "deletable": false, "editable": false,
                "mutation": "<mutation xmlns=\"http://www.w3.org/1999/xhtml\"><arg id=\"label-1\" name=\"非线性移动\" type=\"Label\"></arg><arg id=\"px\" name=\"X\" type=\"String\"></arg></mutation>",
                "shadows": { "PROCEDURES_2_DEFNORETURN_MUTATOR": "", "PROCEDURES_2_DEFNORETURN_DEFINE": "", "STACK": "" },
                "inputs": { "PARAMS1": { "type": "procedures_2_stable_parameter", "id": "px", "fields": { "param_name": "X", "param_default_value": "" } } },
                "statements": { "STACK": { "type": "self_move_to", "id": "move" } }
            }]
        } } });
        let mut procedures = model::parse_kn_procedures(&dict).expect("解析程序集");
        let mut ids = model::IdSource::new(true);
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        for entry in &mut procedures {
            mapping::translate_kn_to_kitten(&mut entry.tree, false, &mut report);
        }
        let root =
            model::def_root_from_entry(&procedures[0], &mut ids, &mut report).expect("定义根");

        assert_eq!(root.kind, "procedures_2_defnoreturn");
        assert_eq!(field(&root, "NAME"), Some("非线性移动"));
        assert_eq!(
            slots(&root.inputs),
            vec!["PARAMS0"],
            "形参槽位回到 Kitten4 的 PARAMS<j>"
        );
        assert_eq!(root.inputs["PARAMS0"].kind, "procedures_2_stable_parameter");
        assert_eq!(
            root.inputs["PARAMS0"].id.as_deref(),
            Some("px"),
            "形参 id 沿用(往返要靠它对齐)"
        );
        assert_eq!(field(&root.inputs["PARAMS0"], "param_name"), Some("X"));
        assert_eq!(slots(&root.statements), vec!["STACK"]);
        assert_eq!(root.extra.get("deletable"), Some(&json!(true)));
        assert_eq!(root.extra.get("editable"), Some(&json!(true)));
        assert_eq!(
            root.mutation.as_deref(),
            Some(
                "<mutation xmlns=\"http://www.w3.org/1999/xhtml\"><arg name=\"PARAMS0\"></arg></mutation>"
            )
        );
        assert!(root.shadows.contains_key("PROCEDURES_2_DEFNORETURN_DEFINE"));
        assert!(root.shadows["PARAMS0"].contains("type=\"math_number\""));
    }

    // ---------------------------------------------------------------- 邻接表往返

    fn real_bcmkn() -> Option<Value> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("download/compile/HEX Editor_317683843.bcmkn");
        if !path.exists() {
            return None;
        }
        serde_json::from_str(&std::fs::read_to_string(&path).expect("读 .bcmkn")).ok()
    }

    fn kn_entities(doc: &Value) -> Vec<(String, bool, Value)> {
        let mut out = Vec::new();
        for (key, dict, is_scene) in [
            ("scenes", "scenesDict", true),
            ("actors", "actorsDict", false),
        ] {
            let Some(map) = doc
                .get(key)
                .and_then(Value::as_object)
                .and_then(|outer| outer.get(dict))
                .and_then(Value::as_object)
            else {
                continue;
            };
            for (id, entity) in map {
                out.push((id.clone(), is_scene, entity.clone()));
            }
        }
        out
    }

    /// `build_block_data_json` → `parse_block_data_json` 必须找回同一棵树(真实 3.7 MB 作品的每个实体)
    #[test]
    fn block_data_json_round_trips_real_bcmkn() {
        let Some(doc) = real_bcmkn() else {
            eprintln!("跳过:缺少真作品样例");
            return;
        };
        let mut ids = model::IdSource::new(false);
        let mut entities = 0usize;
        for (id, is_scene, entity) in kn_entities(&doc) {
            let tree = model::parse_kn_entity(&entity["nekoBlockJsonList"]).expect("解析实体");
            if tree.roots.is_empty() {
                continue;
            }
            let bdj = model::build_block_data_json(&tree, &mut ids).expect("编码邻接表");
            assert!(bdj["blocks"].is_object() && bdj["connections"].is_object());
            assert_eq!(
                bdj["blocks"].as_object().map(|m| m.len()),
                bdj["connections"].as_object().map(|m| m.len()),
                "每个积木都要有一条连接表条目(叶子的值是空对象)"
            );
            assert_eq!(bdj["comments"], json!({}));
            let back = model::parse_block_data_json(&bdj).expect("再解析").tree;
            // `blocks` 是 id 字典(serde_json 的 Map 按 key 排序),根的顺序会变成 id 序 ——
            // Kitten4 的根块各自带 `location`,顺序不影响语义,故按 id 比较集合。
            let mut left: Vec<_> = tree.roots.clone();
            let mut right = back.roots.clone();
            let key = |node: &BlockJson| node.id.clone().unwrap_or_default();
            left.sort_by_key(key);
            right.sort_by_key(key);
            for node in left.iter_mut().chain(right.iter_mut()) {
                normalize_empty_mutation(node);
            }
            let diff =
                left.iter()
                    .zip(right.iter())
                    .enumerate()
                    .find_map(|(index, (mine, theirs))| {
                        first_tree_diff(mine, theirs, &format!("roots[{index}]"))
                    });
            assert!(
                diff.is_none() && left.len() == right.len(),
                "{id}({}) 邻接表往返后树不一致:{:?}(根数 {} vs {})",
                if is_scene { "场景" } else { "角色" },
                diff,
                left.len(),
                right.len()
            );
            entities += 1;
        }
        assert!(entities > 0, "样例里应至少有一个带积木的实体");
    }

    /// 每个根积木都要有独立坐标(`XmlBlockWriter` 的 +220 约定),不重叠
    #[test]
    fn roots_get_spread_locations() {
        let tree = BlockTree::new(vec![
            BlockJson::from_value(&json!({ "type": "repeat_forever", "id": "r1" })).unwrap(),
            BlockJson::from_value(&json!({ "type": "repeat_forever", "id": "r2" })).unwrap(),
            BlockJson::from_value(
                &json!({ "type": "repeat_forever", "id": "r3", "location": [7, 9] }),
            )
            .unwrap(),
        ]);
        let mut ids = model::IdSource::new(true);
        let bdj = model::build_block_data_json(&tree, &mut ids).expect("编码");
        assert_eq!(bdj["blocks"]["r1"]["location"], json!([0, 80]));
        assert_eq!(bdj["blocks"]["r2"]["location"], json!([0, 300]));
        assert_eq!(
            bdj["blocks"]["r3"]["location"],
            json!([7, 9]),
            "已有坐标不覆盖"
        );
        assert_eq!(bdj["blocks"]["r1"]["parent_id"], Value::Null);
        assert_eq!(bdj["blocks"]["r1"]["collapsed"], json!(false));
        assert_eq!(bdj["blocks"]["r1"]["mutation"], json!(""));
    }

    /// 重复 id(菱形展开)在 `blocks` 字典里必须现铸新 id,否则互相覆盖
    #[test]
    fn duplicate_ids_are_reminted() {
        let shared = json!({ "type": "text", "id": "shared", "fields": { "TEXT": "x" } });
        let tree = BlockTree::new(vec![
            BlockJson::from_value(&json!({
                "type": "procedures_2_callreturn",
                "id": "call",
                "inputs": { "ARG0": shared.clone(), "ARG1": shared.clone() }
            }))
            .unwrap(),
        ]);
        let mut ids = model::IdSource::new(true);
        let bdj = model::build_block_data_json(&tree, &mut ids).expect("编码");
        let blocks = bdj["blocks"].as_object().expect("blocks");
        assert_eq!(
            blocks.len(),
            3,
            "重复 id 必须重铸成两块:{:?}",
            blocks.keys().collect::<Vec<_>>()
        );
        let call_links = bdj["connections"]["call"].as_object().expect("连接表");
        assert_eq!(call_links.len(), 2, "两个槽位各自连到重铸后的新 id");
        let connected: Vec<&str> = call_links.keys().map(String::as_str).collect();
        assert!(connected.iter().all(|id| blocks.contains_key(*id)));
        let mut names: Vec<&str> = call_links
            .values()
            .filter_map(|link| link["input_name"].as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, vec!["ARG0", "ARG1"]);
    }

    /// Kitten4 给每个块都写 `mutation`(没有就是空串),而 KN 侧可能缺这个键 ——
    /// 比较前把 `Some("")` 归一成 `None`(语义等价,正向只对非空 mutation 动手);
    /// 真实 KN 文件还会把根块的 `parent_id` 写成空串,同样归一。
    fn normalize_empty_mutation(node: &mut BlockJson) {
        if node.mutation.as_deref() == Some("") {
            node.mutation = None;
        }
        if node.parent_id.as_deref() == Some("") {
            node.parent_id = None;
        }
        for child in node.inputs.values_mut() {
            normalize_empty_mutation(child);
        }
        for child in node.statements.values_mut() {
            normalize_empty_mutation(child);
        }
        if let Some(next) = node.next.as_deref_mut() {
            normalize_empty_mutation(next);
        }
    }

    /// 逐节点找第一处差异(邻接表往返失败时用;比整棵树的 `Debug` 输出可读)
    fn first_tree_diff(left: &BlockJson, right: &BlockJson, path: &str) -> Option<String> {
        if left.kind != right.kind {
            return Some(format!("{path}: 类型 {} vs {}", left.kind, right.kind));
        }
        if left.fields != right.fields {
            return Some(format!(
                "{path}({}): fields {:?} vs {:?}",
                left.kind, left.fields, right.fields
            ));
        }
        if left.shadows != right.shadows {
            return Some(format!(
                "{path}({}): shadows {:?} vs {:?}",
                left.kind, left.shadows, right.shadows
            ));
        }
        if left.mutation != right.mutation {
            return Some(format!(
                "{path}({}): mutation {:?} vs {:?}",
                left.kind, left.mutation, right.mutation
            ));
        }
        if left.is_shadow != right.is_shadow || left.is_output != right.is_output {
            return Some(format!(
                "{path}({}): is_shadow/is_output {} {} vs {} {}",
                left.kind, left.is_shadow, left.is_output, right.is_shadow, right.is_output
            ));
        }
        if left.parent_id != right.parent_id {
            return Some(format!(
                "{path}({}): parent_id {:?} vs {:?}",
                left.kind, left.parent_id, right.parent_id
            ));
        }
        for (index, (slot, mine)) in left.inputs.iter().enumerate() {
            match right.inputs.get(slot) {
                Some(theirs) => {
                    if let Some(diff) =
                        first_tree_diff(mine, theirs, &format!("{path}.inputs.{slot}"))
                    {
                        return Some(diff);
                    }
                }
                None => return Some(format!("{path}({}): 右侧缺输入槽 {slot}", left.kind)),
            }
            let _ = index;
        }
        for slot in right.inputs.keys() {
            if !left.inputs.contains_key(slot) {
                return Some(format!("{path}({}): 左侧缺输入槽 {slot}", left.kind));
            }
        }
        for (slot, mine) in &left.statements {
            match right.statements.get(slot) {
                Some(theirs) => {
                    if let Some(diff) =
                        first_tree_diff(mine, theirs, &format!("{path}.statements.{slot}"))
                    {
                        return Some(diff);
                    }
                }
                None => return Some(format!("{path}({}): 右侧缺语句槽 {slot}", left.kind)),
            }
        }
        for slot in right.statements.keys() {
            if !left.statements.contains_key(slot) {
                return Some(format!("{path}({}): 左侧缺语句槽 {slot}", left.kind));
            }
        }
        match (left.next.as_deref(), right.next.as_deref()) {
            (Some(mine), Some(theirs)) => first_tree_diff(mine, theirs, &format!("{path}.next")),
            (None, None) => None,
            (Some(mine), None) => Some(format!(
                "{path}({}): 左侧多一个 next({})",
                left.kind, mine.kind
            )),
            (None, Some(theirs)) => Some(format!(
                "{path}({}): 右侧多一个 next({})",
                left.kind, theirs.kind
            )),
        }
    }

    // ---------------------------------------------------------------- 端到端 + 往返

    /// KN 文档的积木类型频次(实体 + 程序集,`KC` 的复制语义**不**归一)
    fn census_of_kn(doc: &Value) -> BTreeMap<String, usize> {
        let mut out = census_entities_with(doc, false);
        for tree in procedure_trees(doc) {
            accumulate(&tree, &mut out);
        }
        out
    }

    /// 实体侧的类型频次;`normalize_calls` 会先跑 [`model::unrewrite_calls`] 抵消正向 `KC` 的复制语义
    fn census_entities_with(doc: &Value, normalize_calls: bool) -> BTreeMap<String, usize> {
        let mut out = BTreeMap::new();
        let mut ids = model::IdSource::new(true);
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let targets = if normalize_calls {
            model::call_targets(
                &model::parse_kn_procedures(doc.get("procedures").unwrap_or(&Value::Null))
                    .unwrap_or_default(),
            )
        } else {
            Vec::new()
        };
        for (_id, _is_scene, entity) in kn_entities(doc) {
            let mut tree = model::parse_kn_entity(&entity["nekoBlockJsonList"]).unwrap_or_default();
            if normalize_calls {
                model::unrewrite_calls(&mut tree, &targets, &mut ids, &mut report);
            }
            accumulate(&tree, &mut out);
        }
        out
    }

    /// 程序集侧:按**定义积木 id** 聚合的定义体类型频次。
    ///
    /// `known` 是参考文档(通常是转换前的 KN)的定义 id 集合,用于把转换后文档里的
    /// `NORMAL`/`ROUND` 双条目对回同一个定义:正向 `zC` 给 `NORMAL` 沿用定义积木 id、
    /// 给 `ROUND` 铸新 id(定义 id 落在 `fields.NAME`),同一 key 只保留节点更多的那条
    /// (即没被剥掉 `VALUE` 的完整定义体)。
    fn def_census(
        doc: &Value,
        known: &std::collections::BTreeSet<String>,
    ) -> BTreeMap<String, BTreeMap<String, usize>> {
        let mut out: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        let mut sizes: BTreeMap<String, usize> = BTreeMap::new();
        for entry in model::parse_kn_procedures(doc.get("procedures").unwrap_or(&Value::Null))
            .unwrap_or_default()
        {
            let Some(root) = entry.tree.roots.first() else {
                continue;
            };
            let body_name = root
                .fields
                .get("NAME")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let key = if known.contains(&entry.id) {
                entry.id.clone()
            } else if known.contains(body_name) {
                body_name.to_string()
            } else if root.id.as_deref() == Some(entry.id.as_str()) || body_name.is_empty() {
                // `NORMAL` 条目沿用定义积木 id;`fields.NAME` 为空时也只能退到 id
                entry.id.clone()
            } else {
                body_name.to_string()
            };
            let count = entry.tree.count();
            if sizes.get(&key).is_some_and(|size| *size >= count) {
                continue;
            }
            sizes.insert(key.clone(), count);
            let mut census = BTreeMap::new();
            accumulate(&entry.tree, &mut census);
            out.insert(key, census);
        }
        out
    }

    fn procedure_trees(doc: &Value) -> Vec<BlockTree> {
        model::parse_kn_procedures(doc.get("procedures").unwrap_or(&Value::Null))
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.tree)
            .collect()
    }

    fn accumulate(tree: &BlockTree, out: &mut BTreeMap<String, usize>) {
        for (kind, count) in tree.count_types() {
            if !kind.is_empty() {
                *out.entry(kind).or_default() += count;
            }
        }
    }

    fn census_diff(
        before: &BTreeMap<String, usize>,
        after: &BTreeMap<String, usize>,
    ) -> Vec<String> {
        let mut diffs = Vec::new();
        for kind in before
            .keys()
            .chain(after.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            let a = before.get(kind).copied().unwrap_or(0);
            let b = after.get(kind).copied().unwrap_or(0);
            if a != b {
                diffs.push(format!("{kind}: {a} -> {b}"));
            }
        }
        diffs
    }

    /// 真实 `.bcmkn`(3.7 MB,0.27.1 转换器产物)的 KN → Kitten4 → KN:
    /// 类型多重集差异必须**逐条落在文档化的 allow-list 里**。
    ///
    /// 两条腿之间还夹着一次正向(`KC`/`zC`/`GC`),所以差异里既有反向也没做错、纯属正向行为的部分:
    ///
    /// 1. **实体侧**:正向 `GC` 会给横屏坐标输入包一层 `math_arithmetic divide 1.3`(+1 算术块 +1 数字块);
    ///    0.27.1 作品里这类输入本来没被包过,于是每个这样的输入都多出这两个积木。断言口径:
    ///    差异只允许出现在 `math_arithmetic`/`math_number`,且两者增量必须相等(一次包装各加一个),
    ///    并且 `KC` 的复制语义用 `unrewrite_calls` 归一后再比(否则每个实参子树会被数两遍)。
    // ---------------------------------------------------------------- 真作品:纯程序集库

    /// 两份真作品(本地 `download/compile/`,gitignored;来源为平台上传的 `.bcmkn`):
    /// 它们**没有角色**,积木几乎全在 `proceduresDict` 里(52 / 29 条定义)——
    /// 正好覆盖"定义根挂到宿主实体"的**场景分支**(`assembly.rs`:没有角色就挂第一个场景)。
    const PROCEDURE_LIBRARIES: &[&str] = &[
        "download/compile/FjSwU2iKY6bLe6fexZJTvsX3X7AI.bcmkn",
        "download/compile/FjDQB0v0geuG4y9BaTVyi_WcZ1jc.bcmkn",
    ];

    fn procedure_libraries() -> Vec<(String, Value)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        PROCEDURE_LIBRARIES
            .iter()
            .filter_map(|rel| {
                let path = root.join(rel);
                if !path.exists() {
                    return None;
                }
                let text = std::fs::read_to_string(&path).expect("读 .bcmkn");
                Some((
                    (*rel).to_string(),
                    serde_json::from_str(&text).expect("JSON"),
                ))
            })
            .collect()
    }

    fn defs_of(doc: &Value) -> usize {
        doc["procedures"]["proceduresDict"]
            .as_object()
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// 纯程序集库:反向必须成功、定义根要落到场景上、两次转换逐字节一致
    #[test]
    fn procedure_library_reverses_to_kitten4_and_is_deterministic() {
        let libs = procedure_libraries();
        if libs.is_empty() {
            eprintln!("跳过:缺少真作品样例(纯程序集库)");
            return;
        }
        let options = TranslateOptions::new().deterministic_ids(true);

        for (label, source) in &libs {
            assert!(defs_of(source) > 0, "{label}:样例应有程序集定义");
            assert!(
                source["actors"]["actorsDict"]
                    .as_object()
                    .map(|m| !m.is_empty())
                    .unwrap_or(false),
                "{label}:样例应有角色(定义根会挂到第一个角色上)"
            );

            let mut report = TranslateReport::new(
                crate::core::convert::EditorType::Neko,
                TargetEditor::Kitten4,
            );
            let k4 = convert_kn_document(source, &options, &mut report).expect("反向");

            let scenes_in = source["scenes"]["scenesDict"]
                .as_object()
                .map(|m| m.len())
                .unwrap_or(0);
            let scenes_out = k4["theatre"]["scenes"]
                .as_object()
                .map(|m| m.len())
                .unwrap_or(0);
            assert_eq!(scenes_in, scenes_out, "{label}:场景数必须守恒");

            // 定义根积木落在宿主实体的 `block_data_json` 里(没有角色 ⇒ 第一个场景)
            let landed = k4["theatre"]
                .to_string()
                .contains("procedures_2_defnoreturn");
            assert!(landed, "{label}:定义根积木必须出现在 Kitten4 产物里");
            assert!(
                !report.warnings().iter().any(|w| matches!(
                    w,
                    TranslateWarning::DroppedProperty { path } if path.contains("没有实体可挂载")
                )),
                "{label}:有场景就不该报「作品没有实体可挂载定义积木」"
            );

            // 确定性:同一输入两次转换逐字节一致 + 告警逐条同序
            let mut report2 = TranslateReport::new(
                crate::core::convert::EditorType::Neko,
                TargetEditor::Kitten4,
            );
            let k4b = convert_kn_document(source, &options, &mut report2).expect("反向(第二遍)");
            assert_eq!(
                k4.to_string(),
                k4b.to_string(),
                "{label}:同一输入的两次反向转换必须逐字节一致"
            );
            assert_eq!(
                report.warnings(),
                report2.warnings(),
                "{label}:告警必须逐条同序"
            );

            // 往返:KN → Kitten4 → KN。实体侧只允许横屏包装(math_arithmetic + math_number 成对);
            // 定义体按定义积木 id 对齐后必须守恒,`calculate` 是已知的 1:1 降级
            let mut back_report = TranslateReport::new(
                crate::core::convert::EditorType::Kitten4,
                TargetEditor::KittenN,
            );
            let mut k4_again: Value = serde_json::from_str(&k4.to_string()).expect("复刻");
            let kn2 = convert_kitten4_document(&mut k4_again, &options, &mut back_report)
                .expect("再次正向");

            let before = census_entities_with(source, true);
            let after = census_entities_with(&kn2, true);
            let entity_diffs = census_diff(&before, &after);
            // 允许的差异(与 `real_bcmkn_round_trip_multiset_diff_is_documented` 同一口径):
            // ① 横屏坐标包装(math_arithmetic + math_number 成对);
            // ② KN 原生 `calculate` 在正向被降级成文本占位积木(1:1)
            // ③ 已知保真缺口(本测试抓到,未修):inline `pure_list_get` 影子在往返里丢失,
            //    实体侧与定义体侧都出现(见 `docs/rounds/28`)
            let allowed_entity = [
                "math_arithmetic:",
                "math_number:",
                "calculate:",
                "bcm_translator_text_return_value_block:",
                "pure_list_get:",
            ];
            assert!(
                entity_diffs
                    .iter()
                    .all(|diff| allowed_entity.iter().any(|allow| diff.starts_with(allow))),
                "{label}:实体侧出现未文档化的类型差异:\n{}\n(反向报告 {:#?})",
                entity_diffs.join("\n"),
                report.counts()
            );
            let delta = |kind: &str| -> i64 {
                after.get(kind).copied().unwrap_or(0) as i64
                    - before.get(kind).copied().unwrap_or(0) as i64
            };
            assert_eq!(
                delta("calculate"),
                -delta("bcm_translator_text_return_value_block"),
                "{label}:`calculate` 与占位积木必须 1:1 互换"
            );
            assert_eq!(
                delta("math_arithmetic"),
                delta("math_number"),
                "{label}:横屏包装必须成对"
            );

            let before_defs = def_census(source, &std::collections::BTreeSet::new());
            let known: std::collections::BTreeSet<String> = before_defs.keys().cloned().collect();
            let after_defs = def_census(&kn2, &known);
            // 定义 id 必须**不丢**;允许 `after` 多出 —— 正向 `zC` 会把带返回值的定义拆成
            // `NORMAL` + `ROUND` 两条(0.16.2 行为,见 `real_bcmkn_round_trip_multiset_diff_is_documented`)
            let missing: Vec<&String> = before_defs
                .keys()
                .filter(|id| !after_defs.contains_key(*id))
                .collect();
            assert!(
                missing.is_empty(),
                "{label}:往返后定义 id 丢失:{missing:?}(反向报告 {:#?})",
                report.counts()
            );
            // 定义体内同样会出现横屏包装成对增加(与实体侧同一机制),外加 `calculate` 的 1:1 降级。
            //
            // **已知保真缺口(本测试抓到,未修)**:定义体里的 `list_append` 若带
            // `<shadow type="pure_list_get" inline="true">`,往返一圈后该影子不再出现,
            // 同时少掉一个 `procedures_2_callreturn`(这两条差必须成对看:是同一个调用点
            // 的输入影子在反向 `fold_pure_list_get` 与正向云列表特例之间被改写)。
            // 目前只在"纯程序集库"这类作品上出现;修它需要先弄清官方在**定义体**里对
            // inline `pure_list_get` 的处理(实体侧有现成对照),故先按 allow-list 记录、守住不恶化。
            let allowed = [
                "calculate:",
                "bcm_translator_text_return_value_block:",
                "math_arithmetic:",
                "math_number:",
                "pure_list_get:",
                "procedures_2_callreturn:",
            ];
            let mut affected = 0usize;
            let mut deficit = 0i64;
            for (id, before) in &before_defs {
                let after = &after_defs[id];
                let diffs: Vec<String> = census_diff(before, after)
                    .into_iter()
                    .filter(|diff| !allowed.iter().any(|allow| diff.starts_with(allow)))
                    .collect();
                if diffs.is_empty() {
                    continue;
                }
                affected += 1;
                let lost: i64 = before.values().map(|v| *v as i64).sum();
                let kept: i64 = after.values().map(|v| *v as i64).sum();
                deficit += (lost - kept).max(0);
                eprintln!(
                    "[保真缺口] {label} 定义 {id}: {} (净减 {})",
                    diffs.join("; "),
                    lost - kept
                );
            }
            // **预算断言(只许变小)**:今天 `Node VM v3` 是 6 个定义 / 净减 133 块,
            // `now` 是 0 / 0。变大就是回退 —— 这批缺口本身**已知未修**,
            // 根因、证据与后续研究步骤见 `docs/rounds/28-convert-reverse-fidelity-gaps.md`。
            if deficit > 0 {
                // 定位用:反向报告的分类分布(缺口若来自"某个不可映射的容器吞掉子树",这里能看出来)
                let top: Vec<String> = report
                    .counts()
                    .into_iter()
                    .map(|(cat, subjects)| {
                        format!("{cat}×{} ({})", subjects.values().sum::<usize>(), subjects.keys().take(4).cloned().collect::<Vec<_>>().join(","))
                    })
                    .collect();
                eprintln!("[反向报告] {label}: {}", top.join(" | "));
            }
            assert!(
                affected <= 6 && deficit <= 133,
                "{label}:反向保真缺口扩大(受影响定义 {affected}/{}，净减块 {deficit};基线 6 / 133)",
                before_defs.len()
            );
        }
    }

    /// 2. **程序集侧**:正向 `zC` 会把带返回值的定义拆成 `NORMAL` + `ROUND` 两条(0.16.2 行为),
    ///    于是条数变多、同一个定义体会出现两份(一份剥掉 `VALUE`)。断言口径:按**定义积木 id** 对齐,
    ///    每个定义的积木类型多重集必须守恒,唯一允许的差是 `calculate`(下表)。
    /// 3. **allow-list(不可逆项)**:`calculate` 是 KN 原生积木,`LC` 把它降级成
    ///    `bcm_translator_text_return_value_block`,所以反向只能保留 `calculate`(并报 `UnmappedBlock`),
    ///    再正向回来就是占位积木 —— 一次 1:1 的类型替换。
    #[test]
    fn real_bcmkn_round_trip_multiset_diff_is_documented() {
        let Some(source) = real_bcmkn() else {
            eprintln!("跳过:缺少真作品样例");
            return;
        };
        let options = TranslateOptions::new().deterministic_ids(true);
        let mut reverse_report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let mut k4 = convert_kn_document(&source, &options, &mut reverse_report).expect("反向");
        let mut back_report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let kn2 = convert_kitten4_document(&mut k4, &options, &mut back_report).expect("再次正向");

        // ---- 1) 实体侧
        let before_entities = census_entities_with(&source, true);
        let after_entities = census_entities_with(&kn2, true);
        let entity_diffs = census_diff(&before_entities, &after_entities);
        let delta = |kind: &str| -> i64 {
            after_entities.get(kind).copied().unwrap_or(0) as i64
                - before_entities.get(kind).copied().unwrap_or(0) as i64
        };
        assert!(
            entity_diffs.iter().all(
                |diff| diff.starts_with("math_arithmetic:") || diff.starts_with("math_number:")
            ),
            "实体侧出现未文档化的类型差异:\n{}\n(反向报告 {:#?})",
            entity_diffs.join("\n"),
            reverse_report.counts()
        );
        assert_eq!(
            delta("math_arithmetic"),
            delta("math_number"),
            "横屏坐标包装必须成对出现(每次 +1 算术块 +1 数字块):{entity_diffs:?}"
        );
        assert!(
            delta("math_arithmetic") >= 0,
            "包装只会增加积木:{entity_diffs:?}"
        );

        // ---- 2) 程序集侧(按定义积木 id 对齐)
        let before_defs = def_census(&source, &std::collections::BTreeSet::new());
        let known: std::collections::BTreeSet<String> = before_defs.keys().cloned().collect();
        let after_defs = def_census(&kn2, &known);
        assert_eq!(
            before_defs
                .keys()
                .collect::<std::collections::BTreeSet<_>>(),
            after_defs.keys().collect::<std::collections::BTreeSet<_>>(),
            "定义体必须按 id 一一对上(反向用 `fields.NAME` 回填定义名,id 是从 Kitten4 定义根整段搬回来的)"
        );
        // 唯一允许的差异:KN 原生 `calculate` 换成正向的降级占位积木(1:1)
        let allowed = ["calculate:", "bcm_translator_text_return_value_block:"];
        for (id, before) in &before_defs {
            let after = &after_defs[id];
            let diffs: Vec<String> = census_diff(before, after)
                .into_iter()
                .filter(|diff| !allowed.iter().any(|allow| diff.starts_with(allow)))
                .collect();
            assert!(
                diffs.is_empty(),
                "定义 {id} 的积木类型不守恒:{}\n(前 {before:?}\n后 {after:?})",
                diffs.join("; ")
            );
        }
        let calculate_swaps: i64 = before_defs
            .values()
            .map(|census| census.get("calculate").copied().unwrap_or(0) as i64)
            .sum();
        let placeholders_after: i64 = after_defs
            .values()
            .map(|census| {
                census
                    .get("bcm_translator_text_return_value_block")
                    .copied()
                    .unwrap_or(0) as i64
            })
            .sum();
        assert_eq!(
            calculate_swaps, placeholders_after,
            "allow-list 必须正好是 1:1 的类型替换"
        );
        assert!(
            calculate_swaps > 0,
            "样例里应含 KN 原生 `calculate`(反向的不可逆样本)"
        );
    }

    /// KN → Kitten4 → KN:积木类型多重集必须守恒(allow-list 见测试内注释)
    #[test]
    fn kn_kitten4_kn_round_trip_preserves_type_multiset() {
        // 输入用**我们自己正向产物的等价物**:真实 `.bcm4` → KN。
        // 这样往返两侧的编码约定(形参槽位、mutation 形态)完全一致,守恒性才有意义。
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("download/compile/raw/几何对战-联机.bcm4");
        if !path.exists() {
            eprintln!("跳过:缺少真作品样例 {}", path.display());
            return;
        }
        let mut source: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读作品")).expect("JSON");
        let options = TranslateOptions::new().deterministic_ids(true);

        let mut forward_report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let kn =
            convert_kitten4_document(&mut source, &options, &mut forward_report).expect("正向");
        let before = census_of_kn(&kn);
        assert!(before.values().sum::<usize>() > 300, "样例应含数百个积木");

        let mut reverse_report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let mut k4 = convert_kn_document(&kn, &options, &mut reverse_report).expect("反向");
        let mut back_report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let kn2 = convert_kitten4_document(&mut k4, &options, &mut back_report).expect("再次正向");
        let after = census_of_kn(&kn2);

        // 允许的差异(逐条有因):
        // - `math_number` 与 `default_value`/`text`/`get_split_options` 的对应关系是 `LC` 的单值反查结果,
        //   这类"同名不同层"的块在往返里换的是名字,不是数量;数量差必须为 0,故这里只允许**空** allow-list。
        let allowed: &[(&str, &str)] = &[];
        let diffs: Vec<String> = census_diff(&before, &after)
            .into_iter()
            .filter(|diff| {
                !allowed
                    .iter()
                    .any(|(kind, _)| diff.starts_with(&format!("{kind}:")))
            })
            .collect();
        assert!(
            diffs.is_empty(),
            "KN→Kitten4→KN 类型多重集不守恒:\n{}\n(反向报告 {:#?})",
            diffs.join("\n"),
            reverse_report.counts()
        );
        let _ = reverse_report;
    }

    /// 真实 `.bcmkn`(3.7 MB)的端到端:文档硬结构 + 报告里必须有"KN 原生块"的显式告警
    #[test]
    fn real_bcmkn_reverses_to_kitten4_document() {
        let Some(source) = real_bcmkn() else {
            eprintln!("跳过:缺少真作品样例");
            return;
        };
        let options = TranslateOptions::new().deterministic_ids(true);
        let mut report = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let doc = convert_kn_document(&source, &options, &mut report).expect("反向");

        assert_eq!(doc["work_type"], json!("KITTEN"));
        assert_eq!(doc["version"], json!(25));
        assert_eq!(doc["type"], json!(1));
        assert_eq!(doc["project_name"], json!("HEX Editor"));
        // 900×562(横屏)按源画布原样给 Kitten4,`landscape` 判定(宽>高)因此保持一致
        assert_eq!(doc["size"], json!({ "width": 900, "height": 562 }));
        for key in ["scenes", "actors", "styles", "scenes_order"] {
            assert!(doc["theatre"].get(key).is_some(), "theatre.{key} 缺失");
        }
        let actors = doc["theatre"]["actors"].as_object().expect("actors");
        let scenes = doc["theatre"]["scenes"].as_object().expect("scenes");
        assert_eq!(actors.len(), 6);
        assert_eq!(scenes.len(), 1);
        for actor in actors.values() {
            assert!(
                actor.get("block_data_json").is_some(),
                "实体必须有 block_data_json"
            );
            assert!(actor.get("nekoBlockJsonList").is_none(), "KN 键必须清掉");
            let bdj = &actor["block_data_json"];
            assert_eq!(
                bdj["blocks"].as_object().map(|m| m.len()),
                bdj["connections"].as_object().map(|m| m.len())
            );
        }
        assert!(
            scenes
                .values()
                .all(|scene| scene["name"] == json!("Background"))
        );
        // 程序集定义根积木挂到了第一个角色上(`procedures_2_defnoreturn` 是 Kitten4 的实体内根块)
        let defs: usize = actors
            .values()
            .map(|actor| {
                actor["block_data_json"]["blocks"]
                    .as_object()
                    .map(|blocks| {
                        blocks
                            .values()
                            .filter(|block| block["type"] == json!("procedures_2_defnoreturn"))
                            .count()
                    })
                    .unwrap_or(0)
            })
            .sum();
        assert_eq!(defs, 16, "16 条程序集应各产出一个定义根积木");

        // KN 原生块必须逐类型报告,而不是静默丢
        let unmapped: Vec<&str> = report
            .warnings()
            .iter()
            .filter_map(|warning| match warning {
                TranslateWarning::UnmappedBlock { kind } => Some(kind.as_str()),
                _ => None,
            })
            .collect();
        for kind in [
            "temporary_list",
            "console_log",
            "check_key",
            "traverse_number",
            "script_variables",
        ] {
            assert!(
                unmapped.contains(&kind),
                "缺 {kind} 的未映射告警:{unmapped:?}"
            );
        }
        assert!(report.is_lossy());
        // 报告口径:该样例里反向是 1:1 (`0.27.1` 作品既没有 `pure_list_get` 影子节点,
        // 横屏坐标也没被包过,所以没有可折叠/可解包的东西),故两个计数相等
        assert_eq!(
            report.blocks_total,
            report.blocks_converted,
            "该样例不应有折叠/解包:{:#?}",
            report.counts()
        );
        assert!(report.blocks_total > 1500);
        // 确定性:同一输入两次转换必须一致
        let mut again = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let doc2 = convert_kn_document(&source, &options, &mut again).expect("反向2");
        assert_eq!(doc, doc2, "确定性 id 模式下两次转换必须一致");
    }

    /// `translate_file(input, TargetEditor::Kitten4)` 必须真能读 `.bcmkn` 并落盘 `.bcm4`
    /// **实体级并行的前置守门**(docs/rounds/25 §8):多实体 + 程序集定义,不依赖 `download/` 样本。
    ///
    /// 覆盖两处跨实体结构:① 反向 `def_root_from_entry` 把程序集定义根**挂到宿主实体**上
    /// (`assembly.rs:878-886`);② 正向把定义从实体树里 `split_procedures` 抽走、再
    /// `rewrite_calls` 把调用点写回各实体(`neko.rs`)。
    ///
    /// 断言:重复转换**逐字节一致** + 告警**逐条同序** —— 这是"一旦并行就必须保住"的两条不变量。
    #[test]
    fn multi_entity_with_procedures_is_deterministic() {
        let doc = json!({
            "stageSize": { "width": 562, "height": 900 },
            "actors": {
                "actorsDict": {
                    "host": {
                        "x": 0,
                        "y": 0,
                        "nekoBlockJsonList": [
                            { "type": "repeat_forever", "id": "r1" },
                            { "type": "hide", "id": "r2" }
                        ]
                    },
                    "other": {
                        "x": 10,
                        "y": 0,
                        "nekoBlockJsonList": [ { "type": "hide", "id": "h1" } ]
                    }
                }
            },
            "scenes": { "scenesDict": {} },
            "procedures": {
                "proceduresDict": {
                    "def1": {
                        "name": "走两步",
                        "params": [],
                        "nekoBlockJsonList": [ { "type": "hide", "id": "d1" } ]
                    }
                }
            }
        });

        let options = TranslateOptions::new().deterministic_ids(true);
        let mut report_a = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let mut report_b = TranslateReport::new(
            crate::core::convert::EditorType::Neko,
            TargetEditor::Kitten4,
        );
        let first = convert_kn_document(&doc, &options, &mut report_a).expect("反向转换");
        let second = convert_kn_document(&doc, &options, &mut report_b).expect("反向转换(第二遍)");

        assert_eq!(
            first.to_string(),
            second.to_string(),
            "同一输入的两次反向转换必须逐字节一致"
        );
        assert_eq!(
            report_a.warnings(),
            report_b.warnings(),
            "告警必须逐条同序(顺序是可观察的公开面)"
        );
        assert_eq!(report_a.counts(), report_b.counts());

        // 定义根确实挂到了宿主实体上(跨实体结构存在)
        let actors = first["theatre"]["actors"].to_string();
        assert!(
            actors.contains("procedures_2_defnoreturn") || actors.contains("procedures"),
            "程序集定义根应挂到宿主实体的积木里,实际:{actors}"
        );

        // 闭环:反向产物再走一次正向(定义被抽走 → 调用点重写 → 装配)
        let mut k4_again = serde_json::from_str::<Value>(&first.to_string()).expect("复刻源");
        let mut k4 = first;
        let mut forward_report = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let kn = convert_kitten4_document(&mut k4, &options, &mut forward_report)
            .expect("反向产物再正向");
        // KN 文档的角色/场景在**顶层**(`actors.actorsDict` / `scenes.scenesDict`),
        // 与反向产物的 Kitten4 形态(`theatre.*`)不同
        assert!(
            kn["actors"]["actorsDict"].is_object(),
            "正向产物应有角色字典:{kn}"
        );
        assert!(
            kn["scenes"]["scenesDict"].is_object(),
            "正向产物应有场景字典"
        );
        assert!(
            kn["procedures"]["proceduresDict"].is_object(),
            "源里的程序集定义应被抽成 proceduresDict"
        );

        // 正向同样要可重复(定义/调用点改写涉及全局程序集表,最容易被并行打散)
        let mut forward_again = TranslateReport::new(
            crate::core::convert::EditorType::Kitten4,
            TargetEditor::KittenN,
        );
        let kn2 =
            convert_kitten4_document(&mut k4_again, &options, &mut forward_again).expect("再正向");
        assert_eq!(kn.to_string(), kn2.to_string(), "正向也要逐字节可重复");
        assert_eq!(forward_report.warnings(), forward_again.warnings());
    }

    #[test]
    fn translate_file_writes_bcm4_from_bcmkn() {
        let Some(path) = ({
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("download/compile/HEX Editor_317683843.bcmkn");
            path.exists().then_some(path)
        }) else {
            eprintln!("跳过:缺少真作品样例");
            return;
        };
        // 写系统临时目录:不往仓库里落盘(仓库 temp/ 是要清理干净的工作区);
        // 目录每次运行唯一,避免并行/重跑时互相看到对方的产物
        let dir = super::unique_test_dir("reverse");
        std::fs::create_dir_all(&dir).expect("建目录");
        let options = TranslateOptions::new()
            .deterministic_ids(true)
            .output_dir(&dir);
        let outcome = translate_file(&path, TargetEditor::Kitten4, options).expect("转换");
        assert_eq!(outcome.target, TargetEditor::Kitten4);
        assert!(outcome.output.exists(), "产物没落盘:{:?}", outcome.output);
        assert_eq!(
            outcome.output.extension().and_then(|e| e.to_str()),
            Some("bcm4")
        );
        let text = std::fs::read_to_string(&outcome.output).expect("读产物");
        let doc: Value = serde_json::from_str(&text).expect("产物必须是 JSON");
        assert_eq!(doc["work_type"], json!("KITTEN"));
        assert!(doc["theatre"]["actors"].is_object());
        std::fs::write(dir.join("report-reverse.md"), outcome.report.to_markdown())
            .expect("写报告");

        // `strict` 模式:反向是**有损**的(KN 原生块/歧义类型都进报告),必须被挡下
        let strict = TranslateOptions::new()
            .deterministic_ids(true)
            .output_dir(&dir)
            .strict(true);
        let error =
            translate_file(&path, TargetEditor::Kitten4, strict).expect_err("strict 应失败");
        assert!(matches!(error, TranslateError::Lossy { .. }), "{error:?}");
    }
}
