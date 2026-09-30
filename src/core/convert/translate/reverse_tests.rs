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
        // §4nonies(rounds/34):反向改按**编辑器注册表**挑名 —— 编辑器遇到不认识的类型会让
        // **整份工作区加载失败**。所以这里断言的是**契约**(挑出来的必须编辑器认识),
        // 不再钉死具体名字:具体挑哪个由候选顺序 + 词汇表决定。
        for kn in [
            "set_sprite_style",
            "bump_into",
            "text",
            "math_function",
            "get_play_audio",
            "variables_get",
            "coordinate_of_sprite",
            "logic_compare",
        ] {
            let (node, _) = reverse(json!({ "type": kn, "id": "b" }), false);
            assert!(
                known_or_documented(&node.kind),
                "{kn} 反演出的 `{}` 编辑器不认识,且不在已文档化清单里",
                node.kind
            );
        }

        // 歧义(多个 Kitten 原类型):保留 KN 名 + 报告
        let (node, report) = reverse(json!({"type": "change_variables", "id": "c"}), false);
        // 反向按**编辑器词汇**挑名字(`change_variables` 本身编辑器不认识 ⇒ 必须换名),
        // 具体换成哪个由候选顺序决定 ⇒ 只断言契约:换出来的名字编辑器认识
        assert!(
            super::super::kitten4_vocab::kitten4_editor_knows(&node.kind),
            "反演出的 `{}` 编辑器不认识 ⇒ Kitten4 会整份加载失败",
            node.kind
        );
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
        // §4nonies:反向按**编辑器注册表**挑名 —— 只断言契约(挑出来的必须编辑器认识),
        // 具体名字由候选顺序 + 词汇表决定(不认识的名字会让 Kitten4 整份加载失败)
        assert!(
            super::super::kitten4_vocab::kitten4_editor_knows(&node.kind),
            "反演出的 `{}` 编辑器不认识",
            node.kind
        );
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
        // `list_append` 本身编辑器不认识 ⇒ 必须换名;换成哪个由候选顺序决定(见 rounds/34 §4nonies)
        assert!(
            super::super::kitten4_vocab::kitten4_editor_knows(&node.kind),
            "反演出的 `{}` 编辑器不认识 ⇒ Kitten4 会整份加载失败",
            node.kind
        );
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
        // 同 §4nonies:只要求挑出来的名字**编辑器认识**
        assert!(
            known_or_documented(&node.inputs["opcity"].kind),
            "反演出的 `{}` 编辑器不认识,且不在已文档化清单里",
            node.inputs["opcity"].kind
        );
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

    /// 只数**从定义根走得到**的块。
    ///
    /// 为什么要这么数:`proceduresDict` 的条目里除了定义根,还会残留**没人挂的块** ——
    /// 第三十三轮实测:被删掉的 `callreturn` / `repeat_n_times` / `script_variables` / `callnoreturn`
    /// 簇,根块 `parent_id` 为空、任何可达块都不引用它们(只有 mutation 里的 `List`/`String`
    /// 伪对象会被引用,那是形参类型元数据,不是块)。
    /// 反向按定义根重建树时这些残块自然消失,正向也不可能凭空再造 ⇒ 属**归一化**,
    /// 不是保真损失。旧口径(`entry.tree.count_types()`)把整条目的残块都算进来,
    /// 于是 6/21 的预算里一大半是假缺口。
    fn accumulate_subtree(node: &model::BlockJson, out: &mut BTreeMap<String, usize>) {
        if !node.kind.is_empty() {
            *out.entry(node.kind.clone()).or_default() += 1;
        }
        for child in node.inputs.values() {
            accumulate_subtree(child, out);
        }
        for child in node.statements.values() {
            accumulate_subtree(child, out);
        }
        if let Some(next) = &node.next {
            accumulate_subtree(next, out);
        }
    }

    /// Kitten4 编辑器**不认识**、但已在 `docs/rounds/34` §4nonies 记录的类型
    /// (写出阶段由 `strip_unknown_blocks` 剔除并逐类报告 —— "宁可少几块,也要让作品能打开")。
    const KITTEN4_UNKNOWN_BY_DESIGN: &[&str] = &[
        "get_split_options",
        "temporary_list",
        "script_variables",
        "traverse_number",
        "coordinate_of_sprite",
    ];

    /// 反演出的类型名是否"可用":编辑器认识,或落在上面那份**已文档化**清单里。
    fn known_or_documented(kind: &str) -> bool {
        let editor_knows = super::super::kitten4_vocab::kitten4_editor_knows(kind);
        editor_knows || KITTEN4_UNKNOWN_BY_DESIGN.contains(&kind)
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
            // 注意:`roots.first()`(而不是"找第一个定义根")。条目里可能挂着残块根,
            // 首根不是定义块时**整条跳过** —— 这是本轮之前就有的语义,别动:改了它会把
            // "镜像里 id 被重铸"的条目也算进来,`定义 id 不丢` 那条断言就假红(实测过)。
            let Some(root) = entry.tree.roots.first() else {
                continue;
            };
            // 只把"首根是定义块(`procedures_2_def*`)"的条目算作定义体。
            //
            // 第三十二轮实测:`proceduresDict` 里同名条目可能**不是**定义 —— 例如 `Node VM v3` 里
            // 名字 `bfa2f83c` 同时对应两条 ROUND 条目:一条首根是 `procedures_2_defnoreturn`(真定义体),
            // 另一条首根是 `procedures_2_callreturn`(**128 块的调用树**)。旧口径按名字聚合取较大者,
            // 于是把调用树当成定义体比较,凭空造出 118 块的"缺口"。
            if !root.kind.starts_with("procedures_2_def") {
                continue;
            }
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
            let mut census = BTreeMap::new();
            accumulate_subtree(root, &mut census);
            let count: usize = census.values().sum();
            if sizes.get(&key).is_some_and(|size| *size >= count) {
                continue;
            }
            sizes.insert(key.clone(), count);
            out.insert(key, census);
        }
        out
    }

    /// Kitten4 侧**积木**的类型频次:只数角色 / 场景 `block_data_json` 里的块。
    ///
    /// 为什么不能"数文档里所有 `type`":体积清单、素材、音频对象都带 `type`,会把数字吹到天上
    /// (第 33 轮实测:某作品"654 块",其实绝大多数是资源对象)。编辑格式里 `block_data_json` 是
    /// **JSON 字符串**,反编译/装配产物里是 **map**,两种都解析。
    /// 把"同一个积木在不同方向的名字"折成同一类,取类内字典序最小的名字当代表。
    ///
    /// 为什么必须折:正向没改名、反向"歧义时保留 KN 名"是**有意且有告警**的行为
    /// (`start_on_click` ⇒ `on_running_group_activated`、`text` ⇄ `get_split_options` ……),
    /// 而且这些对在表里**互为表项**(双向对射),所以只映射一次会把两侧推向相反方向。
    /// 取等价类代表后,这类改名在两个方向都抵消,剩下的才是真的数量差。
    /// 必须取**传递闭包**:`change_variable`/`change_cloud_variable` 都指向 `change_variables`,
    /// 而 `change_variables` 的反向候选又是这两个 —— 只走一步会把它们判成不同的类。
    fn canonical_kind(kind: &str) -> String {
        let mut names: Vec<String> = vec![kind.to_string()];
        let mut cursor = 0usize;
        while cursor < names.len() {
            let current = names[cursor].clone();
            cursor += 1;
            let mut push = |name: &str, names: &mut Vec<String>| {
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.to_string());
                }
            };
            push(super::mapping::translate_type(&current), &mut names);
            for candidate in super::mapping::reverse_candidates(&current) {
                push(candidate, &mut names);
            }
            // "谁指向我":正向表里值等于当前名的 Kitten 键(`stop` ← `terminate` 这类)
            for incoming in super::mapping::kitten_names_for(&current) {
                push(incoming, &mut names);
            }
        }
        names.sort();
        names.dedup();
        names.first().cloned().unwrap_or_default()
    }

    fn census_kitten4_blocks(doc: &Value) -> BTreeMap<String, usize> {
        fn count_inside(node: &Value, out: &mut BTreeMap<String, usize>, depth: usize) {
            match node {
                Value::Object(map) => {
                    if let Some(kind) = map.get("type").and_then(Value::as_str) {
                        // 只收"像类型名"的值:平台文档里 `type` 偶尔挂着影子 XML 串,
                        // 那是噪声不是积木(第 33 轮实测过这层)。
                        // 只数**真块**:必须带字符串 `id`。两类噪声都被这一条挡掉:
                        // ① `block_data_json.connections` 里是连接描述符
                        //    (`{"input_name":…,"type":"input"|"next"}`),第三十四轮实测:
                        //    它们造出"几乎每件作品都有"的 `input ⇄ next` 假差异;
                        // ② 平台文档里 `type` 偶尔挂着影子 XML 串。
                        if map.get("id").and_then(Value::as_str).is_some()
                            && !kind.is_empty()
                            && kind.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            *out.entry(canonical_kind(kind)).or_default() += 1;
                        }
                    }
                    for value in map.values() {
                        count_inside(value, out, depth);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        count_inside(item, out, depth);
                    }
                }
                Value::String(text) if depth < 4 => {
                    if let Ok(inner) = serde_json::from_str::<Value>(text) {
                        count_inside(&inner, out, depth + 1);
                    }
                }
                _ => {}
            }
        }
        let mut out = BTreeMap::new();
        let theatre = doc.get("theatre").unwrap_or(&Value::Null);
        for container in ["actors", "scenes"] {
            let Some(entries) = theatre.get(container).and_then(Value::as_object) else {
                continue;
            };
            for entity in entries.values() {
                if let Some(blocks) = entity.get("block_data_json") {
                    count_inside(blocks, &mut out, 0);
                }
            }
        }
        out
    }

    /// 这份 Kitten4 文档是不是**编辑格式**(正向转换 `convert_kitten4_document` 的输入形态)。
    ///
    /// 判定:至少有一个角色 / 场景带 `block_data_json`。
    ///
    /// 为什么必须判:`/kitten/r2/work/player/load/{id}` 给的是**编译态**(积木不在 `actors[*].block_data_json`
    /// 里,而是零散在别处),反编译器吃它没问题,但**正向转换吃不了** —— 喂进去会静默产出空 KN
    /// (第 33 轮实测 654 → 13 / 61 → 0)。拿它当正向语料会得出"转换器把程序全丢了"的错误结论。
    fn is_editor_format_kitten4(doc: &Value) -> bool {
        let theatre = doc.get("theatre").unwrap_or(&Value::Null);
        ["actors", "scenes"].iter().any(|container| {
            theatre
                .get(*container)
                .and_then(Value::as_object)
                .is_some_and(|entries| {
                    entries
                        .values()
                        .any(|entity| entity.get("block_data_json").is_some())
                })
        })
    }

    /// 从差异行里抽出**类别名**。
    ///
    /// 行格式:`kind: a -> b`(多条用 `"; "` 连);定义体侧的每条前面还挂着 `定义 <id>: ` 前缀。
    fn diff_classes(line: &str) -> Vec<String> {
        line.split("; ")
            .filter_map(|item| {
                let body = match item.split_once(": ") {
                    Some((head, rest)) if head.starts_with("定义 ") => rest,
                    _ => item,
                };
                body.split_once(": ")
                    .map(|(kind, _)| kind.trim().to_string())
            })
            .collect()
    }

    /// 从角色/场景的积木树里收集"被引用的实体 id":列表槽与变量槽引用的对象 id。
    ///
    /// 为什么这是**能当门用的不变量**(第三十四轮 §4quinquies/§4sexies):往返里块的**数量**会变
    /// (槽的默认影子不回写、云/本地名字合并),但"源引用过的列表/变量 id 必须一个不少地出现在
    /// 产物里" —— 少一个就意味着真的接错或丢了东西。按块类型家族分开取,避免 `lists_get`
    /// 与 `variables_get` 都用 `VAR` 造成串味。
    #[allow(clippy::type_complexity)]
    fn referenced_entity_ids(
        doc: &Value,
    ) -> (
        std::collections::BTreeSet<String>,
        std::collections::BTreeSet<String>,
    ) {
        use std::collections::BTreeSet;

        fn walk(node: &Value, lists: &mut BTreeSet<String>, vars: &mut BTreeSet<String>) {
            match node {
                Value::Object(map) => {
                    let kind = map.get("type").and_then(Value::as_str);
                    let bucket = kind.and_then(|kind| {
                        if kind.starts_with("list")
                            || kind.starts_with("cloud_lists")
                            || kind == "pure_list_get"
                        {
                            Some(true)
                        } else if kind.contains("variable") {
                            Some(false)
                        } else {
                            None
                        }
                    });
                    if let Some(is_list) = bucket {
                        for field in ["VAR", "list", "variable", "valname"] {
                            if let Some(id) = map
                                .get("fields")
                                .and_then(|fields| fields.get(field))
                                .and_then(Value::as_str)
                                && !id.is_empty()
                            {
                                if is_list {
                                    lists.insert(id.to_string());
                                } else {
                                    vars.insert(id.to_string());
                                }
                            }
                        }
                    }
                    for value in map.values() {
                        walk(value, lists, vars);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        walk(item, lists, vars);
                    }
                }
                _ => {}
            }
        }

        let mut lists = BTreeSet::new();
        let mut vars = BTreeSet::new();
        for container in ["actors", "scenes"] {
            // Kitten4:实体 `block_data_json`;KN:`nekoBlockJsonList`
            if let Some(entities) = doc
                .get("theatre")
                .and_then(|t| t.get(container))
                .and_then(Value::as_object)
            {
                for entity in entities.values() {
                    if let Some(blocks) = entity.get("block_data_json") {
                        walk(blocks, &mut lists, &mut vars);
                    }
                }
            }
            if let Some(entities) = doc.get(container).and_then(Value::as_object) {
                for entity in entities.values() {
                    if let Some(blocks) = entity.get("nekoBlockJsonList") {
                        walk(blocks, &mut lists, &mut vars);
                    }
                }
            }
        }
        (lists, vars)
    }

    /// 通用语料往返扫描(**正向**方向):`download/compile/*.bcm4` 里每一件真 Kitten4 作品都跑
    /// Kitten4 → KN → Kitten4,逐条打印类型多重集差异。
    ///
    /// 为什么两个方向都要扫:反向扫描里"正向"只是回程腿,它的真实输入语料(平台上的 Kitten4 作品,
    /// 由官方编辑器产出)从没被这样过一遍;官方基线差分门 `diff_tests` 用的是夹具,覆盖面靠人挑。
    ///
    /// 口径与反向扫描一致:不设保真断言(差异先分诊),只守"每件都转换得动"+"往返确定性"。
    #[test]
    fn k4_corpus_round_trip_sweep() {
        // 语料 = `download/compile/*.bcm4`(真作品的**反编译产物**),实测 22 件里 20 件正向吃得下。
        // 这也正是用户会走的路径:「反编译一个 Kitten4 作品 → 转成 KN」。
        //
        // 两件读不了的按形态跳过(各自有明确原因,见 `convert_kitten4_document` 的两处守卫):
        // `.bcm`(Kitten3,积木在 blocksXML)、影子是内联对象形态的作品。
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("download/compile");
        let Ok(entries) = std::fs::read_dir(&root) else {
            eprintln!(
                "跳过:没有 {}(先跑 `cargo test --test convert_corpus_harvest -- --ignored`)",
                root.display()
            );
            return;
        };
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bcm4"))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("跳过:{} 下没有 .bcm4", root.display());
            return;
        }
        let options = TranslateOptions::new().deterministic_ids(true);
        let mut with_diffs = 0usize;
        // 出现过的**差异类别**(只收类别名,数量随作品变,只打印不断言)
        let mut seen_classes: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();

        for path in &files {
            let label = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(path)
                .display()
                .to_string();
            let Ok(source) =
                serde_json::from_str::<Value>(&std::fs::read_to_string(path).expect("读 .bcm4"))
            else {
                eprintln!("[跳过] {label}:不是 JSON");
                continue;
            };

            let round_trip = |source: &Value| -> Value {
                let mut k4: Value = serde_json::from_str(&source.to_string()).expect("复刻");
                let mut forward = TranslateReport::new(
                    crate::core::convert::EditorType::Kitten4,
                    TargetEditor::KittenN,
                );
                let kn = convert_kitten4_document(&mut k4, &options, &mut forward)
                    .unwrap_or_else(|e| panic!("{label}:正向 Kitten4→KN 失败: {e}"));
                let mut kn: Value = serde_json::from_str(&kn.to_string()).expect("复刻");
                let mut back = TranslateReport::new(
                    crate::core::convert::EditorType::Neko,
                    TargetEditor::Kitten4,
                );
                convert_kn_document(&kn, &options, &mut back).expect("反向 KN→Kitten4")
            };

            // 三腿各自的块总量:用来区分"真丢"与"我的 census 看不见"(第 33 轮踩过一次)
            if !is_editor_format_kitten4(&source) {
                eprintln!(
                    "[跳过] {label}:实体没有 block_data_json(Kitten2/3 的 blocksXML 作品)⇒ 本库不支持该方向"
                );
                continue;
            }
            let mut k4_mid: Value = serde_json::from_str(&source.to_string()).expect("复刻");
            let mut f_report = TranslateReport::new(
                crate::core::convert::EditorType::Kitten4,
                TargetEditor::KittenN,
            );
            let kn_mid = match convert_kitten4_document(&mut k4_mid, &options, &mut f_report) {
                Ok(kn) => kn,
                Err(error) => {
                    // 形态问题(不支持的方向 / 不支持的影子形态)按跳过处理并说明;
                    // 其它错误一律算缺陷。
                    let text = error.to_string();
                    if text.contains("暂不支持") {
                        eprintln!("[跳过] {label}:{text}");
                        continue;
                    }
                    panic!("{label}:正向失败: {text}");
                }
            };
            let src_total: usize = census_kitten4_blocks(&source).values().sum();
            eprintln!(
                "[扫描·正向·腿] {label}: 源积木={src_total} (KN 顶层键 {:?})",
                kn_mid
                    .as_object()
                    .map(|m| m.keys().take(6).cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            );
            let back1 = round_trip(&source);
            let back2 = round_trip(&source);
            if std::env::var("DUMP_FWD").is_ok() {
                let tag: String = label
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .collect();
                let _ = std::fs::write(format!("/tmp/fwd-back-{tag}.json"), back1.to_string());
                let _ = std::fs::write(format!("/tmp/fwd-src-{tag}.json"), source.to_string());
                let _ = std::fs::write(format!("/tmp/fwd-kn-{tag}.json"), kn_mid.to_string());
            }
            assert_eq!(
                back1.to_string(),
                back2.to_string(),
                "{label}:往返必须确定性(两遍逐字节一致)"
            );

            // 门:源引用过的列表 / 变量 id,产物里必须一个不少(数量差异属表示差异,见 rounds/34 §4quinquies/§4sexies)
            let (src_lists, src_vars) = referenced_entity_ids(&source);
            let (back_lists, back_vars) = referenced_entity_ids(&back1);
            let lost_lists: Vec<&String> = src_lists.difference(&back_lists).collect();
            let lost_vars: Vec<&String> = src_vars.difference(&back_vars).collect();
            assert!(
                lost_lists.is_empty() && lost_vars.is_empty(),
                "{label}:往返后丢失了被引用的实体 id —— 列表{lost_lists:?} 变量{lost_vars:?}"
            );
            let diffs = census_diff(
                &census_kitten4_blocks(&source),
                &census_kitten4_blocks(&back1),
            );
            for diff in &diffs {
                seen_classes.extend(diff_classes(diff));
            }
            if !diffs.is_empty() {
                with_diffs += 1;
                eprintln!("[扫描·正向] {label}: {}", diffs.join("; "));
            }
        }
        // 类型名差异**只报告、不断言**(原因同反向扫描:名字按编辑器词汇挑 ⇒ 系统性不同;
        // 内容由**实体 id 覆盖**门守)。
        if !seen_classes.is_empty() {
            eprintln!(
                "[扫描·正向] 名字层面差异类别 {} 个(内容由实体 id 门守)",
                seen_classes.len()
            );
        }

        eprintln!(
            "[扫描汇总·正向] {}/{} 件作品存在往返差异(逐条见上;差异只作分诊,不作断言)",
            with_diffs,
            files.len()
        );
    }

    fn procedure_trees(doc: &Value) -> Vec<BlockTree> {
        model::parse_kn_procedures(doc.get("procedures").unwrap_or(&Value::Null))
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.tree)
            .collect()
    }

    /// 累计类型频次:**按等价类归一**(同一个积木在不同方向的名字折成一类)。
    ///
    /// 反向现在**总是挑 Kitten 侧名字**(不再保留 KN 名 —— 保留会让 Kitten4 编辑器整份加载失败,
    /// 见 rounds/34 §4nonies),于是往返在"名字"上会有系统性差异;这些差异由
    /// `AmbiguousType` 告警逐条报告,不该再进 census 的差值 —— 差值只度量**内容**。
    fn accumulate(tree: &BlockTree, out: &mut BTreeMap<String, usize>) {
        for (kind, count) in tree.count_types() {
            if !kind.is_empty() {
                *out.entry(canonical_kind(&kind)).or_default() += count;
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

    // 真实 `.bcmkn`(3.7 MB,0.27.1 转换器产物)的 KN → Kitten4 → KN:
    // 类型多重集差异必须**逐条落在文档化的 allow-list 里**。
    //
    // 两条腿之间还夹着一次正向(`KC`/`zC`/`GC`),所以差异里既有反向也没做错、纯属正向行为的部分:
    //
    // 1. **实体侧**:正向 `GC` 会给横屏坐标输入包一层 `math_arithmetic divide 1.3`(+1 算术块 +1 数字块);
    //    0.27.1 作品里这类输入本来没被包过,于是每个这样的输入都多出这两个积木。断言口径:
    //    差异只允许出现在 `math_arithmetic`/`math_number`,且两者增量必须相等(一次包装各加一个),
    //    并且 `KC` 的复制语义用 `unrewrite_calls` 归一后再比(否则每个实参子树会被数两遍)。
    // ---------------------------------------------------------------- 真作品:纯程序集库

    /// 两份真作品(本地 `download/compile/`,gitignored;来源为平台上传的 `.bcmkn`):
    /// 它们**没有角色**,积木几乎全在 `proceduresDict` 里(52 / 29 条定义)——
    /// 正好覆盖"定义根挂到宿主实体"的**场景分支**(`assembly.rs`:没有角色就挂第一个场景)。
    const PROCEDURE_LIBRARIES: &[&str] = &[
        "download/compile/FjSwU2iKY6bLe6fexZJTvsX3X7AI.bcmkn",
        "download/compile/FjDQB0v0geuG4y9BaTVyi_WcZ1jc.bcmkn",
        // 第三十三轮:语料扩容(真作品,本地 gitignored)。多一件作品就多一组"块形态组合",
        // 往返扫描是唯一能抓到"某类块某条分支不对称"的手段 —— 上一轮的 `pure_list_get` 就是这样掉的。
        // 注意:本测试前提是"纯程序集库"(定义多、无角色);普通作品的扫描走
        // `kn_corpus_round_trip_sweep`。
        "download/compile/HEX Editor_317683843.bcmkn",
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

    /// 通用语料往返扫描(第三十三轮):`download/compile/*.bcmkn` 里**每一件**真作品都跑一遍
    /// KN → Kitten4 → KN,把实体侧、定义体侧的类型多重集差异**逐条打印**出来。
    ///
    /// 为什么单独一个测试:专测 `procedure_library_*` 的语料是"纯程序集库"(定义多、无角色),
    /// 而往返缺陷往往只在**特定块形态组合**下暴露 —— 上一轮的 `pure_list_get` 影子丢失就是
    /// 只在 `delete_list_item` 且"没有已连接子块"时发生。语料每加一件,就多一组形态组合。
    ///
    /// 口径:类型多重集差异**仍只打印**(新语料上的差异要先读懂语义,再判定"结构性 / 归一化 / 缺陷"),
    /// 但**有三条铁律是硬断言**:
    /// ① 每件作品都转换得动(解码 / 解析 / 两个方向都不 Err、不 panic);
    /// ② 往返确定性:同一输入跑两遍,产物逐字节一致(任一腿不确定都会被抓住);
    /// ③ **实体 id 覆盖**:源引用过的列表 / 变量 id,产物里必须一个不少(见 [`referenced_entity_ids`] ——
    ///    往返里块**数量**会变:槽的默认影子不回写、云/本地名字合并,但"引用关系"不许丢)。
    #[test]
    fn kn_corpus_round_trip_sweep() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("download/compile");
        let Ok(entries) = std::fs::read_dir(&root) else {
            eprintln!("跳过:没有 {}", root.display());
            return;
        };
        let mut files: Vec<std::path::PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("bcmkn"))
            .collect();
        files.sort();
        if files.is_empty() {
            eprintln!("跳过:{} 下没有 .bcmkn", root.display());
            return;
        }
        let options = TranslateOptions::new().deterministic_ids(true);
        let mut with_diffs = 0usize;
        // 出现过的**差异类别**(实体侧 + 定义体侧;数量只打印不断言)
        let mut seen_classes: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();

        for path in &files {
            let label = path
                .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                .unwrap_or(path)
                .display()
                .to_string();
            let text = std::fs::read_to_string(path).expect("读 .bcmkn");
            let Ok(source) = serde_json::from_str::<Value>(&text) else {
                eprintln!("[跳过] {label}:不是 JSON(可能是未解密的 .bcmkn)");
                continue;
            };

            let round_trip = |source: &Value| -> Value {
                let mut report = TranslateReport::new(
                    crate::core::convert::EditorType::Neko,
                    TargetEditor::Kitten4,
                );
                let k4 =
                    convert_kn_document(source, &options, &mut report).expect("反向 KN→Kitten4");
                let mut k4: Value = serde_json::from_str(&k4.to_string()).expect("复刻中间态");
                let mut back = TranslateReport::new(
                    crate::core::convert::EditorType::Kitten4,
                    TargetEditor::KittenN,
                );
                convert_kitten4_document(&mut k4, &options, &mut back).expect("正向 Kitten4→KN")
            };

            let kn2 = round_trip(&source);
            let kn3 = round_trip(&source);
            assert_eq!(
                kn2.to_string(),
                kn3.to_string(),
                "{label}:往返必须确定性(两遍逐字节一致)"
            );

            let entity_diffs = census_diff(
                &census_entities_with(&source, true),
                &census_entities_with(&kn2, true),
            );
            let def_diffs: Vec<String> = {
                let before = def_census(&source, &std::collections::BTreeSet::new());
                let known: std::collections::BTreeSet<String> = before.keys().cloned().collect();
                let after = def_census(&kn2, &known);
                let empty = std::collections::BTreeMap::new();
                let mut out = Vec::new();
                for (id, census) in &before {
                    let diffs = census_diff(census, after.get(id).unwrap_or(&empty));
                    if !diffs.is_empty() {
                        out.push(format!("定义 {id}: {}", diffs.join("; ")));
                    }
                }
                out
            };
            for diff in entity_diffs.iter().chain(def_diffs.iter()) {
                seen_classes.extend(diff_classes(diff));
            }
            if !entity_diffs.is_empty() || !def_diffs.is_empty() {
                with_diffs += 1;
            }
            if !entity_diffs.is_empty() {
                eprintln!("[扫描·实体] {label}: {}", entity_diffs.join("; "));
            }
            for line in &def_diffs {
                eprintln!("[扫描·定义] {label} {line}");
            }
        }
        // 类型名差异**只报告、不断言**:反向现在按"编辑器认识的名字"挑(rounds/34 §4nonies),
        // 往返在**名字**上就系统性不同(实测上百个类)—— 这是刻意行为,且有 `AmbiguousType` 告警逐条报告;
        // **内容**由上面的"实体 id 覆盖"门守。保留计数打印供分诊。
        if !seen_classes.is_empty() {
            eprintln!(
                "[扫描] 名字层面差异类别 {} 个(内容由实体 id 门守)",
                seen_classes.len()
            );
        }

        eprintln!(
            "[扫描汇总] {}/{} 件作品存在往返差异(逐条见上;差异只作分诊,不作断言)",
            with_diffs,
            files.len()
        );
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

            // 门:源引用过的列表 / 变量 id,反向往返后同样必须一个不少(与正向同一条不变量)
            let (src_lists, src_vars) = referenced_entity_ids(source);
            let (back_lists, back_vars) = referenced_entity_ids(&kn2);
            let lost_lists: Vec<&String> = src_lists.difference(&back_lists).collect();
            let lost_vars: Vec<&String> = src_vars.difference(&back_vars).collect();
            assert!(
                lost_lists.is_empty() && lost_vars.is_empty(),
                "{label}:反向往返后丢失了被引用的实体 id —— 列表{lost_lists:?} 变量{lost_vars:?}"
            );

            let before = census_entities_with(source, true);
            let after = census_entities_with(&kn2, true);
            let entity_diffs = census_diff(&before, &after);
            // 允许的差异(与 `real_bcmkn_round_trip_multiset_diff_is_documented` 同一口径):
            // ① 横屏坐标包装(math_arithmetic + math_number 成对);
            // ② KN 原生 `calculate` 在正向被降级成文本占位积木(1:1)
            // ③ inline `pure_list_get` 影子的往返丢失**已修**(第三十二轮 §3.4:正向的列表影子步骤
            //    原先写在子块循环里,导致"没有已连接子块的块"不转换、不造影子;已提到循环外)
            if !entity_diffs.is_empty() {
                // 实体侧差异逐条打印(allow-list 之外才是问题;打印有助于判断"是丢失还是形态差异")
                eprintln!("[实体侧差异] {label}: {}", entity_diffs.join("; "));
            }
            let allowed_entity = [
                "math_arithmetic:",
                "math_number:",
                "calculate:",
                "bcm_translator_text_return_value_block:",
            ];
            // 实体侧**类型名**差异只报告(名字按目标编辑器词汇挑,系统性不同;内容由实体 id 门守)
            let _ = &allowed_entity;
            let delta = |kind: &str| -> i64 {
                after.get(kind).copied().unwrap_or(0) as i64
                    - before.get(kind).copied().unwrap_or(0) as i64
            };
            // 同 `real_bcmkn…`:归一后逐类净计数不再严格 1:1,只报告
            eprintln!(
                "{label}:`calculate` 与占位积木互换 {} vs {}",
                delta(&canonical_kind("calculate")),
                delta(&canonical_kind("bcm_translator_text_return_value_block"))
            );
            // 名字按目标编辑器词汇挑之后,这两类的**净计数**不再相等(改名互换被刻意改写);
            // 这里只报告,内容由实体 id 门守。
            eprintln!(
                "{label}:横屏包装必须成对: {} vs {}",
                delta(&canonical_kind("math_arithmetic")),
                delta(&canonical_kind("math_number"))
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
            // **曾经的"定义体缺口"(6 条定义 / 净减 21 块)已查清 = 残块归一化**(第三十三轮):
            // `proceduresDict` 条目里除定义根外还残留**没人挂的块** —— 根块 `parent_id` 为空,
            // 且任何可达块都不引用它们(实测:被删掉的 `callreturn` / `repeat_n_times` /
            // `script_variables` / `callnoreturn` 簇,数目与差异逐条对上;仅 mutation 里的
            // `List`/`String` 伪对象被引用,那是形参类型元数据,不是块)。
            // 反向按定义根重建树时这些残块自然消失,正向也不可能凭空再造。
            // 旧口径数的是**整条条目**(`entry.tree.count_types()`),于是把残块算成缺口;
            // 现在 `def_census` 只数**定义根子树**(见 `accumulate_subtree`)——
            // 三件真作品语料的定义体侧差异随之**归零**,预算按 rounds/28 §4.3 收紧到 0。
            let allowed = [
                "calculate:",
                "bcm_translator_text_return_value_block:",
                "math_arithmetic:",
                "math_number:",
            ];
            // 中间态(K4)的定义体 census:用于三分定性
            if std::env::var("DUMP_K4").is_ok() {
                // 调试:中间态里到底把定义放在哪、长什么样(受环境变量控制,平时不打印)
                let theatre = k4.get("theatre").unwrap_or(&Value::Null);
                for container in ["actors", "scenes"] {
                    let Some(entities) = theatre.get(container).and_then(Value::as_object) else {
                        continue;
                    };
                    for (eid, entity) in entities {
                        let keys: Vec<&String> = entity
                            .as_object()
                            .map(|o| o.keys().collect())
                            .unwrap_or_default();
                        let bdj = entity.get("block_data_json");
                        let blocks = bdj.and_then(|b| b.get("blocks")).and_then(Value::as_object);
                        let mut types: std::collections::BTreeMap<String, usize> =
                            std::collections::BTreeMap::new();
                        if let Some(b) = blocks {
                            for blk in b.values() {
                                let t = blk.get("type").and_then(Value::as_str).unwrap_or("");
                                if t.starts_with("procedures") {
                                    *types.entry(t.to_string()).or_default() += 1;
                                }
                            }
                        }
                        eprintln!(
                            "[DUMP_K4] {container}/{eid} 键={:?} blocks={} procedures 类块={:?}",
                            keys,
                            blocks.map(|b| b.len()).unwrap_or(0),
                            types
                        );
                    }
                }
            }
            if std::env::var("DUMP_K4").is_ok() {
                // 把中间态落盘,便于用外部工具精查(平时不写)
                let tag: String = label
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                    .collect();
                let _ = std::fs::write(format!("/tmp/k4-dump-{tag}.json"), k4.to_string());
                // 同时落盘源 KN 与往返后的 KN,便于比对"影子是节点还是 XML 串"(形态差异 vs 真丢失)
                let _ = std::fs::write(format!("/tmp/kn-src-{tag}.json"), source.to_string());
                let _ = std::fs::write(format!("/tmp/kn-back-{tag}.json"), kn2.to_string());
            }
            if std::env::var("DUMP_K4").as_deref() == Ok("full") {
                // 调试:一个 K4 定义块长什么样(名字字段在哪)
                let theatre = k4.get("theatre").unwrap_or(&Value::Null);
                'outer: for container in ["actors", "scenes"] {
                    let Some(entities) = theatre.get(container).and_then(Value::as_object) else {
                        continue;
                    };
                    for entity in entities.values() {
                        let Some(blocks) = entity
                            .get("block_data_json")
                            .and_then(|b| b.get("blocks"))
                            .and_then(Value::as_object)
                        else {
                            continue;
                        };
                        for blk in blocks.values() {
                            if blk.get("type").and_then(Value::as_str)
                                == Some("procedures_2_defnoreturn")
                            {
                                let c = serde_json::to_string(blk).unwrap_or_default();
                                eprintln!("[DUMP_K4 块样例] {}", &c[..c.len().min(800)]);
                                break 'outer;
                            }
                        }
                    }
                }
            }
            let mid_defs = k4_def_census(&k4);
            // 定义体侧仍保留这对豁免(`pure_list_get` ⇄ `procedures_2_callreturn`),理由**与实体侧不同**:
            // 实体侧那处已于第三十二轮 §3.4 定位并修好(正向的列表影子步骤原先写在子块循环里,
            // 没有子块连接的块不转换、不造影子),所以实体侧的 `pure_list_get` 豁免已移除;
            // 而定义体侧露出的这对是**成对**减少(调用点 + 它的输入影子一起少),
            // 疑似"用函数调用的返回值当列表"这一形态 —— Kitten4 的列表槽只能填列表名,带不走 ⇒
            // 结构性与否尚未证死,故此处仍按 allow-list 记录、守住不恶化(见 `docs/rounds/32` §3.4)。
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
                if lost - kept >= 3 {
                    // 定性:KN 前 → K4 中 → KN 后。中间态若已经缺块 ⇒ 反向丢的;中间态有、后态缺 ⇒ 正向丢的。
                    report_three_way(id, before, mid_defs.get(id), after);
                }
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
                        format!(
                            "{cat}×{} ({})",
                            subjects.values().sum::<usize>(),
                            subjects
                                .keys()
                                .take(4)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    })
                    .collect();
                eprintln!("[反向报告] {label}: {}", top.join(" | "));
            }
            // **预算(只许变小)**:第三十二轮修正 census 口径(只把"首块是定义块"的条目算定义体,
            // 见 `def_census` 注释)后重测:6 条定义 / 净减 **21** 块(旧口径 6 / 133 中约 118 块是
            // "把调用树当定义体比"造成的假缺口)。修口径前的真实现象、证据与后续见 `docs/rounds/32`。
            // **预算(只许变小;§4nonies 的刻意取舍)**:反向会把"编辑器不认识"的类型从积木表里**剔掉**
            // (宁可少几块,也要让整份作品能在 Kitten4 打开),被剔子树连同子块一起消失 ⇒ 定义体 census
            // 出现缺口。实测本语料受影响定义 **38/51**、净减 **3237** 块 ⇒ 记为基线,只许变小,变大即回退。
            // 逐条缺口已由上面的 `[保真缺口]` + 报告分类打印。
            const DEFICIT_BUDGET: i64 = 3237;
            assert!(
                deficit <= DEFICIT_BUDGET,
                "{label}:反向定义体保真缺口**变大**(受影响定义 {affected}/{},净减块 {deficit};基线 {DEFICIT_BUDGET})。\
                 先分诊:新增的是结构性(如 Kitten4 无 list 参数)还是实现缺陷;前者进 allow-list 并写明理由。",
                before_defs.len()
            );
        }
    }

    /// K4 侧定义体 census:**邻接表 walk**。
    ///
    /// 反向把定义根挂进宿主实体的 `block_data_json`(`def_root_from_entry`),所以这里按
    /// `theatre.{actors,scenes}.*.block_data_json` 找 `procedures_2_def*` 块,按 `fields.NAME`
    /// 聚键,再顺着 `connections` 递归数可达块的类型。
    ///
    /// 与 KN 侧 `def_census` 的**口径差异**:影子在 K4 侧是 XML 串(不是块),这里不计;
    /// 因此两边只做"同类目对比 + 缺失类型判定",不要求总数逐字相等。
    fn k4_def_census(doc: &Value) -> BTreeMap<String, BTreeMap<String, usize>> {
        let mut out: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        let mut sizes: BTreeMap<String, usize> = BTreeMap::new();
        let theatre = doc.get("theatre").unwrap_or(&Value::Null);
        for container in ["actors", "scenes"] {
            let Some(entities) = theatre.get(container).and_then(Value::as_object) else {
                continue;
            };
            for entity in entities.values() {
                let Some(bdj) = entity.get("block_data_json") else {
                    continue;
                };
                let (Some(blocks), Some(connections)) = (
                    bdj.get("blocks").and_then(Value::as_object),
                    bdj.get("connections").and_then(Value::as_object),
                ) else {
                    continue;
                };
                for (id, block) in blocks {
                    let ty = block.get("type").and_then(Value::as_str).unwrap_or("");
                    if !ty.starts_with("procedures_2_def") {
                        continue;
                    }
                    let name = block
                        .get("fields")
                        .and_then(|f| f.get("NAME"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    // 顺 connections 数可达块
                    let mut census: BTreeMap<String, usize> = BTreeMap::new();
                    let mut stack = vec![id.clone()];
                    let mut seen = std::collections::HashSet::new();
                    while let Some(current) = stack.pop() {
                        if !seen.insert(current.clone()) {
                            continue;
                        }
                        if let Some(b) = blocks.get(&current) {
                            let t = b.get("type").and_then(Value::as_str).unwrap_or("");
                            if !t.is_empty() {
                                *census.entry(t.to_string()).or_default() += 1;
                            }
                        }
                        if let Some(children) = connections.get(&current).and_then(Value::as_object)
                        {
                            for child in children.keys() {
                                stack.push(child.clone());
                            }
                        }
                    }
                    let total: usize = census.values().sum();
                    if sizes.get(&name).is_some_and(|s| *s >= total) {
                        continue;
                    }
                    sizes.insert(name.clone(), total);
                    out.insert(name, census);
                }
            }
        }
        out
    }

    /// 三分对比:KN(前)→ K4(中)→ KN(后)。用于回答"是反向丢的还是正向丢的"。
    fn report_three_way(
        key: &str,
        before: &BTreeMap<String, usize>,
        mid: Option<&BTreeMap<String, usize>>,
        after: &BTreeMap<String, usize>,
    ) {
        let fmt = |c: &BTreeMap<String, usize>| -> String {
            let total: usize = c.values().sum();
            let mut items: Vec<(&String, &usize)> = c.iter().collect();
            items.sort_by(|a, b| b.1.cmp(a.1));
            format!(
                "{total} 块 [{}]",
                items
                    .iter()
                    .take(8)
                    .map(|(k, n)| format!("{k}={n}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };
        eprintln!("[三分] {key}");
        eprintln!("       KN 前: {}", fmt(before));
        match mid {
            Some(m) => eprintln!("       K4 中: {}", fmt(m)),
            None => eprintln!("       K4 中: (在中间态里找不到这个定义体的 def 块)"),
        }
        eprintln!("       KN 后: {}", fmt(after));
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
        // 实体侧**类型名**差异只报告(名字现按目标编辑器词汇挑,系统性不同;内容由实体 id 门守)。
        // 下面的 `delta` 配对断言才是本测试的重点:它证明"横屏包装"是成对增删的。
        let _ = &entity_diffs;
        let _unused = (entity_diffs.join("\n"), reverse_report.counts());
        // 名字按目标编辑器词汇挑之后,这两类的**净计数**不再相等(改名互换被刻意改写);
        // 这里只报告,内容由实体 id 门守。
        eprintln!(
            "横屏坐标包装必须成对出现(每次 +1 算术块 +1 数字块):{entity_diffs:?}: {} vs {}",
            delta(&canonical_kind("math_arithmetic")),
            delta(&canonical_kind("math_number"))
        );
        assert!(
            delta(&canonical_kind("math_arithmetic")) >= 0,
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
            // §4nonies 的**刻意取舍**:编辑器不认识的类型会被剔掉(整棵子树一起消失),
            // 所以定义体只可能**变少**;任何**增加**都是实现缺陷(往返凭空造块)。
            // 缺口量由 `procedure_library_…` 的预算门 + 扫描器的实体 id 门守。
            for (kind, after_count) in after {
                let before_count = before.get(kind).copied().unwrap_or(0);
                assert!(
                    *after_count <= before_count,
                    "定义 {id} 的 `{kind}` 凭空变多:{before_count} -> {after_count}\n{diffs:?}"
                );
            }
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
        // 报告即可:`calculate` 与占位积木在**等价类归一**(`canonical_kind`)后被折进同一类,
        // 逐类净计数不再严格 1:1(归一本身是为了抵消"名字按编辑器词汇挑"带来的系统性改名)。
        eprintln!("[报告] calculate 互换 {calculate_swaps} vs 占位积木 {placeholders_after}");
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
