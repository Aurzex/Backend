//! `model` 的单元测试(2026-11-06 自 `model.rs` 外移,rounds/57)。
//!
//! 归位理由:`model.rs` 总行数曾超过 3000 且测试内联,`streamed_writer_tests` 又夹在代码中间;
//! `../knowledge/repo-conventions.md` §5 要求测试放文件末尾、总行超 3000 时外移。

#[cfg(test)]
mod streamed_writer_tests {
    //! 流式块写出与 `to_value` 路径的**逐字节**等价门(`rounds/47` §4 Step 2/3 的常驻证据)

    use super::super::*;

    /// 覆盖:`extra` 撞名与嵌套、`shield` 真/假、三槽嵌套、转义与非 ASCII、空槽
    fn sample_tree() -> BlockTree {
        let mut tree = parse_kn_entity(&serde_json::json!([
            {
                "type": "controls_if",
                "id": "b1",
                "location": { "x": 1, "y": -2.5 },
                "fields": { "CONDITION": "x", "NOTES": "引号\"反斜杠\\换行\n制表\t中文🙂" },
                "inputs": {
                    "CONDITION": { "type": "math_number", "id": "b2", "shield": true, "fields": { "NUM": 45 } }
                },
                "statements": {
                    "DO": { "type": "self_move_to", "extra": [1, { "深": true }],
                            "next": { "type": "self_go_forward", "disabled": true } },
                    "ELSE": { "type": "self_go_forward", "is_output": true }
                },
                "shadows": { "DO": "<shadow type=\"text\">x</shadow>", "EMPTY": "" },
                "parent_id": "p1",
                "field_constraints": { "min": 0, "max": 10, "float": 1.5 }
            },
            { "type": "unknown_editor_specific", "custom": null, "nested": { "a": [1, 2, 3] } }
        ]))
        .expect("解析样例树");
        // 程序化造出"`extra` 与已知键撞名":解析路径下不可能出现(JSON 不允许重复键)
        tree.roots[0]
            .extra
            .insert("id".to_string(), serde_json::json!("override"));
        tree
    }

    #[test]
    fn streamed_block_tree_equals_value_path() {
        let tree = sample_tree();
        // 两条 `shield` 口径都要与各自的 `to_value` 路径逐字节相同
        for policy in [ShieldPolicy::Always, ShieldPolicy::OnlyWhenTrue] {
            let mut streamed = Vec::new();
            write_block_tree(&tree, policy, &mut streamed).expect("流式写出");
            let expected = serde_json::to_vec(&Value::Array(
                tree_to_value(&tree, policy).expect("to_value"),
            ))
            .expect("序列化 to_value 路径");
            assert_eq!(
                String::from_utf8_lossy(&streamed),
                String::from_utf8_lossy(&expected),
                "流式写出必须与 to_value + to_writer 逐字节相同"
            );
            // 顺带钉住"`extra` 撞名时 extra 胜"这条口径(否则两条路径会在这类树上分叉)
            assert!(String::from_utf8_lossy(&streamed).contains("\"id\":\"override\""));
        }
    }

    /// 空树与最小块:数组外壳不能少;`shield` 键按口径补(正向恒写、NEMO 假值不写)
    #[test]
    fn streamed_block_tree_handles_empty_and_minimal() {
        let empty = BlockTree::default();
        let mut streamed = Vec::new();
        write_block_tree(&empty, ShieldPolicy::Always, &mut streamed).expect("空树");
        assert_eq!(streamed, b"[]");

        let minimal = parse_kn_entity(&serde_json::json!([{ "type": "x" }])).expect("解析");
        let mut streamed = Vec::new();
        write_block_tree(&minimal, ShieldPolicy::Always, &mut streamed).expect("最小树");
        assert_eq!(
            String::from_utf8_lossy(&streamed),
            r#"[{"shield":false,"type":"x"}]"#
        );

        let mut streamed = Vec::new();
        write_block_tree(&minimal, ShieldPolicy::OnlyWhenTrue, &mut streamed).expect("最小树");
        assert_eq!(
            String::from_utf8_lossy(&streamed),
            r#"[{"type":"x"}]"#,
            "NEMO 口径下假 `shield` 不写键"
        );

        let shielded =
            parse_kn_entity(&serde_json::json!([{ "type": "x", "shield": true }])).expect("解析");
        let mut streamed = Vec::new();
        write_block_tree(&shielded, ShieldPolicy::OnlyWhenTrue, &mut streamed).expect("最小树");
        assert_eq!(
            String::from_utf8_lossy(&streamed),
            r#"[{"shield":true,"type":"x"}]"#,
            "NEMO 口径下真 `shield` 照写"
        );
    }

    /// **常驻门**(`rounds/47` §4 Step 4):反向(`block_data_json` 邻接表)流式写出与
    /// `encoded_to_value` + `to_writer` 逐字节相同
    ///
    /// 覆盖:`KITTEN4_DEFAULTS` 补缺省、`parent_id` 强制(根写 `null`)、`extra` 撞缺省键、
    /// 重复 id 的现铸(两次编码各用同一确定性 `IdSource`)、嵌套 `inputs`/`statements`/`next`、
    /// 转义与非 ASCII。
    #[test]
    fn encoded_blocks_writer_equals_value_path() {
        let tree = parse_kn_entity(&serde_json::json!([
            {
                "type": "on_running_group_activated",
                "id": "same",
                "fields": { "NAME": "跳跃\"引号\\换行\n中文🙂" },
                "inputs": { "CONDITION": { "type": "math_number", "id": "n1", "fields": { "NUM": 45 } } },
                "statements": { "DO": { "type": "self_move_to", "id": "same",
                                        "next": { "type": "self_go_forward", "shield": true } } },
                "deletable": false, "collapsed": true, "comment": "保真"
            },
            { "type": "no_id_block", "extra": [1, { "深": true }] }
        ]))
        .expect("解析样例树");

        let expected = serde_json::to_vec(
            &encoded_to_value(
                encode_block_data_json(&tree, &mut IdSource::new(true)).expect("编码"),
            )
            .expect("物化"),
        )
        .expect("序列化");
        let mut streamed = Vec::new();
        write_encoded_blocks(
            &encode_block_data_json(&tree, &mut IdSource::new(true)).expect("编码"),
            &mut streamed,
        )
        .expect("流式写出");
        assert_eq!(
            String::from_utf8_lossy(&streamed),
            String::from_utf8_lossy(&expected),
            "反向邻接表的流式写出必须与 encoded_to_value + to_writer 逐字节相同"
        );
    }
}

#[cfg(test)]
mod neko_tests {
    use super::super::*;
    use crate::core::convert::shared::EditorType;

    use crate::core::convert::translate::TargetEditor;

    fn ids() -> IdSource {
        IdSource::new(true)
    }

    fn report() -> TranslateReport {
        TranslateReport::new(EditorType::Kitten4, TargetEditor::KittenN)
    }

    fn node(value: Value) -> BlockJson {
        BlockJson::from_value(&value).expect("block json")
    }

    /// 一个带两个 String 形参 + 参数引用 + 链式体的程序集定义
    fn def_tree() -> BlockTree {
        BlockTree::new(vec![node(json!({
            "type": "procedures_2_defnoreturn",
            "id": "procDef",
            "location": [0, 80],
            "shield": false,
            "fields": { "NAME": "非线性移动" },
            "shadows": { "PARAMS1": "<shadow type=\"math_number\"/>", "PARAMS2": "<shadow type=\"math_number\"/>" },
            "inputs": {
                "PARAMS2": { "type": "procedures_2_stable_parameter", "id": "pSpeed", "fields": { "param_name": "Speed" } },
                "PARAMS1": { "type": "procedures_2_stable_parameter", "id": "pX", "fields": { "param_name": "X" } },
                "STACK": {
                    "type": "self_move_to", "id": "move",
                    "inputs": {
                        "x": { "type": "procedures_2_parameter", "id": "useX", "fields": { "param_name": "X" }, "is_output": true },
                        "y": { "type": "procedures_2_parameter", "id": "useUnknown", "fields": { "param_name": "不存在" }, "is_output": true }
                    }
                }
            }
        }))])
    }

    #[test]
    fn splits_defs_out_of_the_entity_tree() {
        let mut tree = def_tree();
        tree.roots.push(node(
            json!({ "type": "on_running_group_activated", "id": "hat" }),
        ));
        let mut ids = ids();
        let mut report = report();

        let (rest, procedures) = split_procedures(tree, &mut ids, &mut report);

        assert_eq!(rest.roots.len(), 1);
        assert_eq!(rest.roots[0].kind, "on_running_group_activated");
        assert_eq!(procedures.len(), 1);
        let entry = &procedures[0];
        assert_eq!(entry.kind, "NORMAL");
        assert_eq!(entry.id, "procDef");
        assert_eq!(entry.name, "非线性移动");

        // params:合成的 Label 是 UUID,两个 String 按 PARAMS 序号排(X 在 Speed 前)
        assert_eq!(entry.params.len(), 3);
        assert_eq!(entry.params[0].kind, "Label");
        assert_eq!(entry.params[0].name, "非线性移动");
        assert!(
            entry.params[0].id.contains('-'),
            "Label 形参 id 应是 UUID 形态:{:?}",
            entry.params[0].id
        );
        assert_eq!(
            entry
                .params
                .iter()
                .map(|p| (p.kind.as_str(), p.name.as_str(), p.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Label", "非线性移动", entry.params[0].id.as_str()),
                ("String", "X", "pX"),
                ("String", "Speed", "pSpeed")
            ]
        );

        // STACK 已搬进 statements,inputs 里只剩 PARAMS*
        let body = &entry.tree.roots[0];
        assert_eq!(
            body.statements.get("STACK").map(|b| b.kind.as_str()),
            Some("self_move_to")
        );
        assert_eq!(
            body.inputs.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["PARAMS1", "PARAMS2"]
        );
        // fields.NAME 换成条目 id,deletable/editable 落 extra(输出仍是顶层键)
        assert_eq!(body.fields.get("NAME"), Some(&json!("procDef")));
        assert_eq!(body.extra.get("deletable"), Some(&json!(false)));
        assert_eq!(body.extra.get("editable"), Some(&json!(false)));
        assert_eq!(
            body.mutation.as_deref(),
            Some(
                "<mutation xmlns=\"http://www.w3.org/1999/xhtml\"><arg id=\"00000000-0000-4000-8000-000000000001\" name=\"非线性移动\" type=\"Label\"></arg><arg id=\"pX\" name=\"X\" type=\"String\"></arg><arg id=\"pSpeed\" name=\"Speed\" type=\"String\"></arg></mutation>"
            )
        );

        // 参数引用补上 mutation id / is_output;名字对不上的引用只记告警、不动
        let stack = body.statements.get("STACK").expect("STACK");
        let used = stack.inputs.get("x").expect("x");
        assert_eq!(
            used.mutation.as_deref(),
            Some("<mutation xmlns=\"http://www.w3.org/1999/xhtml\" id=\"pX\"></mutation>")
        );
        assert!(used.is_output);
        let unknown = stack.inputs.get("y").expect("y");
        assert_eq!(unknown.mutation, None);
        assert_eq!(report.warnings().len(), 1);
        assert_eq!(
            report.warnings()[0],
            TranslateWarning::DroppedField {
                path: "procedures_2_defnoreturn.非线性移动.param.不存在".into()
            }
        );
    }

    #[test]
    fn return_value_def_yields_normal_and_round_entries() {
        // STACK = 一个循环体里带 VALUE 的 return_value,后面还挂一个缺 VALUE 的 return_value
        // (同时覆盖 `VC` 的"已有则留、缺则补"两条分支)
        let tree = BlockTree::new(vec![node(json!({
            "type": "procedures_2_defnoreturn",
            "id": "procDef",
            "fields": { "NAME": "取数" },
            "inputs": {
                "STACK": {
                    "type": "repeat_forever_until", "id": "loop",
                    "statements": { "DO": {
                        "type": "procedures_2_return_value", "id": "ret",
                        "inputs": { "VALUE": { "type": "math_number", "id": "num", "is_shadow": true, "fields": { "TEXT": "7" } } },
                        "shadows": { "VALUE": "<shadow type=\"math_number\"/>" }
                    } },
                    "next": { "type": "procedures_2_return_value", "id": "ret2" }
                }
            }
        }))]);
        let mut ids = ids();
        let mut report = report();

        let (_rest, procedures) = split_procedures(tree, &mut ids, &mut report);
        assert_eq!(procedures.len(), 2);
        let normal = &procedures[0];
        let round = &procedures[1];
        assert_eq!(
            (normal.kind.as_str(), normal.id.as_str()),
            ("NORMAL", "procDef")
        );
        assert_eq!(round.kind, "ROUND");
        assert_ne!(round.id, "procDef", "ROUND 条目要现铸新 id");
        assert_eq!(normal.params, round.params);
        assert_eq!(normal.params.len(), 1, "无 PARAMS* 时只剩合成 Label");

        // NORMAL:`WC` 把所有 return_value 的 VALUE 输入与影子都剥掉
        let normal_stack = normal.tree.roots[0].statements.get("STACK").expect("STACK");
        let normal_ret = normal_stack.statements.get("DO").expect("DO");
        assert!(!normal_ret.inputs.contains_key("VALUE"));
        assert_eq!(normal_ret.shadows.get("VALUE"), None);
        assert!(
            !normal_stack
                .next
                .as_deref()
                .expect("普通 return_value 的 STACK 后应接下一块")
                .inputs
                .contains_key("VALUE")
        );

        // ROUND:body 带 `u` 层替换(id 是新 UUID,但 `fields.NAME` 按官方仍是**原定义 id**),
        // 已有 VALUE 原样保留,缺的补默认 math_number + 影子
        let round_body = &round.tree.roots[0];
        assert_eq!(round_body.id.as_deref(), Some(round.id.as_str()));
        assert_eq!(round_body.fields.get("NAME"), Some(&json!("procDef")));
        let round_stack = round_body.statements.get("STACK").expect("STACK");
        let kept = round_stack
            .statements
            .get("DO")
            .expect("DO")
            .inputs
            .get("VALUE")
            .expect("原 VALUE");
        assert_eq!(kept.id.as_deref(), Some("num"));
        assert_eq!(
            kept.fields.get("TEXT"),
            Some(&json!("7")),
            "已有 VALUE 不被改写"
        );
        let injected = round_stack
            .next
            .as_deref()
            .expect("ROUND 的 STACK 后应接下一块")
            .inputs
            .get("VALUE")
            .expect("补出来的 VALUE");
        assert_eq!(injected.kind, "math_number");
        assert!(injected.is_shadow && injected.is_output);
        assert_eq!(injected.parent_id.as_deref(), Some("ret2"));
        assert_eq!(injected.fields.get("NUM"), Some(&json!("0")));
        assert!(injected.field_constraints.is_some());
        assert_eq!(
            round_stack
                .next
                .as_deref()
                .expect("ROUND 的 STACK 后应接下一块")
                .shadows
                .get("VALUE")
                .map(String::as_str),
            Some(VALUE_SHADOW_XML)
        );
    }

    #[test]
    fn rewrites_call_sites_and_leaves_unknown_calls_untouched() {
        let mut ids = ids();
        let mut report = report();
        let procedures = split_procedures(def_tree(), &mut ids, &mut report).1;

        let mut tree = BlockTree::new(vec![node(json!({
            "type": "procedures_2_callnoreturn",
            "id": "call1",
            "fields": { "NAME": "非线性移动" },
            "shadows": { "NAME": "<shadow type=\"text\"/>" },
            "inputs": {
                "ARG0": { "type": "math_number", "id": "a0", "fields": { "TEXT": "160" } },
                "ARG1": { "type": "math_number", "id": "a1", "fields": { "TEXT": "-260" } }
            }
        }))]);
        let unknown = node(json!({
            "type": "procedures_2_callreturn",
            "id": "call2",
            "fields": { "NAME": "不存在的程序集" },
            "mutation": "<mutation/>"
        }));
        tree.roots[0].next = Some(Box::new(unknown));

        rewrite_calls(&mut tree, &procedures, &mut ids, &mut report);

        let call = &tree.roots[0];
        assert_eq!(call.fields.get("NAME"), Some(&json!("procDef")));
        // 影子整体换成 NAME 占位 + 两个 String 形参影子(id 现铸)
        assert_eq!(call.shadows.get("NAME").map(String::as_str), Some(""));
        assert_eq!(call.shadows.len(), 3);
        assert!(call.shadows["pX"].contains("visible=\"visible\""));
        assert!(call.shadows["pX"].contains("name=\"NUM\">0</field>"));
        // ARG<i-1> 复制到形参槽(老槽位保留,官方是复制不是搬移)
        assert_eq!(
            call.inputs.get("ARG0").map(|b| b.id.as_deref()),
            Some(Some("a0"))
        );
        assert_eq!(
            call.inputs.get("pX").map(|b| b.id.as_deref()),
            Some(Some("a0"))
        );
        assert_eq!(
            call.inputs.get("pSpeed").map(|b| b.id.as_deref()),
            Some(Some("a1"))
        );
        assert!(
            !call.inputs.contains_key("ARG2"),
            "ARG<i-1> 只映射到存在的槽位"
        );
        assert_eq!(
            call.mutation.as_deref(),
            Some(concat!(
                "<mutation xmlns=\"http://www.w3.org/1999/xhtml\" def_id=\"procDef\" name=\"procDef\" type=\"NORMAL\">",
                "<arg id=\"00000000-0000-4000-8000-000000000001\" content=\"非线性移动\" type=\"Label\"></arg>",
                "<arg id=\"pX\" content=\"X\" type=\"String\"></arg>",
                "<arg id=\"pSpeed\" content=\"Speed\" type=\"String\"></arg></mutation>"
            ))
        );

        // 找不到同名程序集 → 原样保留
        let untouched = tree.roots[0].next.as_deref().expect("next");
        assert_eq!(untouched.fields.get("NAME"), Some(&json!("不存在的程序集")));
        assert_eq!(untouched.mutation.as_deref(), Some("<mutation/>"));
    }

    #[test]
    fn encodes_trees_and_procedure_dict() {
        let mut ids = ids();
        let mut report = report();
        let (rest, procedures) = split_procedures(def_tree(), &mut ids, &mut report);

        let json_tree = tree_to_value(&rest, ShieldPolicy::Always).expect("rest 树");
        assert!(json_tree.is_empty(), "定义被摘走后实体树为空");

        let dict = procedures_to_json(&procedures).expect("条目编码");
        assert_eq!(
            dict.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["procDef"]
        );
        let entry = dict.get("procDef").expect("条目");
        assert_eq!(entry["id"], json!("procDef"));
        assert_eq!(entry["name"], json!("非线性移动"));
        assert_eq!(entry["type"], json!("NORMAL"));
        assert_eq!(entry["params"][0]["type"], json!("Label"));
        assert_eq!(
            entry["params"][1],
            json!({ "id": "pX", "name": "X", "type": "String" })
        );
        assert_eq!(entry["nekoBlockJsonList"].as_array().map(Vec::len), Some(1));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["type"],
            json!("procedures_2_defnoreturn")
        );
        assert_eq!(entry["nekoBlockJsonList"][0]["deletable"], json!(false));
        assert_eq!(entry["nekoBlockJsonList"][0]["editable"], json!(false));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["fields"]["NAME"],
            json!("procDef")
        );
        // 官方给每个节点写 `shield`(源里没这个键时是 false),编码端要补齐
        assert_eq!(entry["nekoBlockJsonList"][0]["shield"], json!(false));
        assert_eq!(
            entry["nekoBlockJsonList"][0]["statements"]["STACK"]["inputs"]["x"]["shield"],
            json!(false),
            "嵌套节点同样补"
        );
        assert_eq!(
            entry["nekoBlockJsonList"][0]["statements"]["STACK"]["type"],
            json!("self_move_to")
        );
        // 往返:编码产物能再解析回节点(保真)
        let reparsed = BlockJson::from_value(&entry["nekoBlockJsonList"][0]).expect("往返");
        assert_eq!(reparsed, procedures[0].tree.roots[0]);
    }

    #[test]
    fn params_index_follows_parse_int_prefix() {
        assert_eq!(params_index("PARAMS0"), 0);
        assert_eq!(params_index("PARAMS12"), 12);
        assert_eq!(params_index("PARAMS07"), 7);
        assert_eq!(params_index("PARAMS"), 0, "取不到数字按 0 记(官方是 NaN)");
        assert_eq!(params_index("PARAMSX"), 0);
    }
}

#[cfg(test)]
mod block_json_tests {
    use super::super::*;
    use serde_json::json;

    #[test]
    fn value_round_trips_through_block_json() {
        let value = json!({
            "type": "repeat_n_times",
            "id": "b1",
            "location": [334.4444580078125, -12.5],
            "shadows": { "times": "<shadow type=\"math_number\"/>", "DO": "" },
            "fields": { "sprite": "--self" },
            "collapsed": false
        });
        let node = BlockJson::from_value(&value).expect("解析");
        let back = node.to_value().expect("序列化");
        assert_eq!(back["type"], value["type"]);
        assert_eq!(back["location"], value["location"]);
        assert_eq!(back["shadows"], value["shadows"]);
        assert_eq!(back["collapsed"], value["collapsed"], "未知键经 extra 保真");
    }

    /// 依赖 `serde_json` 的 `float_roundtrip` feature:
    /// 关掉它时 `240.88868713378906` 会解析成 240.88868713378903(差 1 ULP),
    /// 于是与官方 JS 产物做逐字节对齐时,`location` 这类浮点会假报差异。
    #[test]
    fn floats_are_parsed_with_correct_rounding() {
        let parsed: f64 = serde_json::from_str("240.88868713378906").expect("解析浮点");
        assert_eq!(parsed, 240.88868713378906_f64);
        assert_eq!(
            serde_json::to_string(&parsed).expect("序列化浮点"),
            "240.88868713378906",
            "往返后必须逐字一致"
        );
    }

    #[test]
    fn tree_utilities_walk_and_count() {
        let root = json!({
            "type": "root", "id": "r",
            "inputs": { "A": { "type": "mid", "id": "m", "inputs": { "A": { "type": "leaf", "id": "l" } } } },
            "next": { "type": "second", "id": "s" }
        });
        let tree = BlockTree::new(vec![
            BlockJson::from_value(&root).expect("构造测试 BlockJson"),
        ]);
        assert_eq!(tree.count(), 4);
        let mut ids = std::collections::HashSet::new();
        tree.walk(&mut |b| {
            if let Some(id) = &b.id {
                ids.insert(id.clone());
            }
        });
        assert_eq!(ids.len(), 4);
        let types = tree.count_types();
        assert_eq!(types.get("mid"), Some(&1));
        assert_eq!(types.get("leaf"), Some(&1));
    }
}

#[cfg(test)]
mod null_tolerance_tests {
    use super::super::*;
    use serde_json::json;

    /// 真作品里这些键可能是显式 null:不能被一个 null 拖垮整份作品的解析。
    #[test]
    fn explicit_nulls_do_not_break_parsing() {
        let value = json!({
            "type": "repeat_n_times",
            "id": "b1",
            "is_shadow": null,
            "is_output": null,
            "shield": null,
            "disabled": null,
            "mutation": null,
            "location": null,
            "inputs": null,
            "statements": null,
            "fields": null,
            "shadows": null,
            "field_constraints": null,
            "parent_id": null
        });
        let node = BlockJson::from_value(&value).expect("null 容错");
        assert_eq!(node.kind, "repeat_n_times");
        assert!(!node.is_shadow && !node.is_output && !node.shield && !node.disabled);
        assert!(node.inputs.is_empty() && node.statements.is_empty());
        assert!(node.fields.is_empty() && node.shadows.is_empty());
        assert!(node.mutation.is_none() && node.location.is_none());
    }

    #[test]
    fn null_type_becomes_empty_kind() {
        let node = BlockJson::from_value(&json!({ "type": null, "id": "x" })).expect("解析");
        assert_eq!(node.kind, "");
    }
}

#[cfg(test)]
mod id_tests {
    use super::super::*;
    use std::collections::HashSet;

    #[test]
    fn deterministic_ids_are_stable_and_unique() {
        let mut a = IdSource::new(true);
        let mut b = IdSource::new(true);
        let left: Vec<String> = (0..5).map(|_| a.uuid()).collect();
        let right: Vec<String> = (0..5).map(|_| b.uuid()).collect();
        assert_eq!(left, right, "同一序号序列必须一致");
        assert_eq!(left.len(), left.iter().collect::<HashSet<_>>().len());
        assert!(
            left.iter().all(|id| id.len() == 36),
            "UUID 形态:{}",
            left[0]
        );
    }

    #[test]
    fn random_ids_look_like_uuid_v4() {
        let mut src = IdSource::new(false);
        let id = src.uuid();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }

    #[test]
    fn short_ids_have_fixed_length() {
        let mut src = IdSource::new(false);
        assert_eq!(src.short().len(), 22);
        let mut det = IdSource::new(true);
        assert_eq!(det.short().len(), 22);
    }

    /// 记录模式(实体级并行):产出带哨兵的临时 id、按铸造顺序记账、不推进全局计数;
    /// 账本按顺序交给串行源兑现后,结果与"同一序列直接在串行源上铸造"逐个相同。
    #[test]
    fn recording_mode_logs_mints_and_replays_identically() {
        let mut recorded = IdSource::recording(7);
        let temps: Vec<String> = vec![recorded.uuid(), recorded.short(), recorded.uuid()];
        assert!(
            temps.iter().all(|id| id.starts_with(TEMP_ID_PREFIX)),
            "记录模式必须产出临时 id:{temps:?}"
        );
        assert_eq!(
            temps,
            vec![
                format!("{TEMP_ID_PREFIX}prov:7:0:u"),
                format!("{TEMP_ID_PREFIX}prov:7:1:s"),
                format!("{TEMP_ID_PREFIX}prov:7:2:u"),
            ]
        );
        let log = recorded.into_log();
        assert_eq!(log.len(), 3);
        assert_eq!(
            log.iter().map(|(_, kind)| *kind).collect::<Vec<_>>(),
            vec![MintKind::Uuid, MintKind::Short, MintKind::Uuid]
        );

        // 兑现:与"直接串行铸造"逐字节一致(确定性模式下 id 是第几次铸造的纯函数)
        let mut replay = IdSource::new(true);
        let finals: Vec<String> = log.iter().map(|(_, kind)| kind.mint(&mut replay)).collect();
        let mut direct = IdSource::new(true);
        let expected = vec![direct.uuid(), direct.short(), direct.uuid()];
        assert_eq!(finals, expected);
    }
}

#[cfg(test)]
mod kitten_tests {
    use super::super::*;
    use serde_json::json;

    fn bdj(blocks: Value, connections: Value) -> Value {
        json!({ "blocks": blocks, "connections": connections, "comments": {} })
    }

    #[test]
    fn parses_next_chain_and_roots() {
        let value = bdj(
            json!({
                "a": { "type": "start_on_click", "id": "a", "location": [0, 0] },
                "b": { "type": "repeat_forever", "id": "b", "shadows": { "DO": "" } },
                "c": { "type": "self_appear", "id": "c" }
            }),
            json!({
                "a": { "b": { "type": "next" } },
                "b": { "c": { "type": "next" } },
                "c": {}
            }),
        );
        let parsed = parse_block_data_json(&value).expect("parse");
        assert_eq!(parsed.roots.len(), 1);
        let root = &parsed.roots[0];
        assert_eq!(root.kind, "start_on_click");
        assert_eq!(
            root.next.as_ref().expect("root 应接下一块").kind,
            "repeat_forever"
        );
        assert_eq!(
            root.next
                .as_ref()
                .expect("root 应接下一块")
                .next
                .as_ref()
                .expect("第二块应再接一块")
                .kind,
            "self_appear"
        );
        assert_eq!(parsed.count(), 3);
    }

    #[test]
    fn routes_value_and_statement_inputs() {
        let value = bdj(
            json!({
                "p": { "type": "repeat_n_times", "id": "p" },
                "v": { "type": "math_number", "id": "v", "is_shadow": true, "fields": { "NUM": "10" } },
                "s": { "type": "self_go_forward", "id": "s" }
            }),
            json!({
                "p": {
                    "v": { "type": "input", "input_type": "value", "input_name": "times" },
                    "s": { "type": "input", "input_type": "statement", "input_name": "DO" }
                }
            }),
        );
        let parsed = parse_block_data_json(&value).expect("parse");
        let root = &parsed.roots[0];
        assert_eq!(
            root.inputs.get("times").map(|b| b.kind.as_str()),
            Some("math_number")
        );
        assert_eq!(
            root.statements.get("DO").map(|b| b.kind.as_str()),
            Some("self_go_forward")
        );
        assert_eq!(parsed.roots.len(), 1);
    }

    #[test]
    fn keeps_shadows_fields_and_unknown_keys() {
        let value = bdj(
            json!({
                "b": {
                    "type": "repeat_n_times", "id": "b",
                    "shadows": { "times": "<shadow type=\"math_number\"/>", "DO": "" },
                    "fields": { "sprite": "--self" },
                    "field_constraints": { "NUM": { "min": 1 } },
                    "collapsed": false, "movable": true, "visible": "visible",
                    "mutation": "", "deletable": true
                }
            }),
            json!({ "b": {} }),
        );
        let parsed = parse_block_data_json(&value).expect("parse");
        let node = &parsed.roots[0];
        assert_eq!(
            node.shadows.get("times").map(String::as_str),
            Some("<shadow type=\"math_number\"/>")
        );
        assert_eq!(node.shadows.get("DO").map(String::as_str), Some(""));
        assert_eq!(node.fields.get("sprite"), Some(&json!("--self")));
        assert!(node.field_constraints.is_some());
        assert_eq!(node.extra.get("collapsed"), Some(&json!(false)));
        assert_eq!(node.extra.get("movable"), Some(&json!(true)));
        assert_eq!(node.extra.get("visible"), Some(&json!("visible")));
        assert_eq!(node.mutation.as_deref(), Some(""));
    }

    #[test]
    fn accepts_legacy_stringified_blocks() {
        let inner = json!({
            "blocks": { "a": { "type": "start_on_click", "id": "a" } },
            "connections": { "a": {} }
        });
        let value = json!({ "blocks": inner.to_string(), "connections": {}, "comments": {} });
        let parsed = parse_block_data_json(&value).expect("parse");
        assert_eq!(parsed.roots.len(), 1);
        assert_eq!(parsed.roots[0].kind, "start_on_click");
    }

    #[test]
    fn rejects_cycles_dangling_children_and_missing_blocks() {
        // 可达环:r → r2 → r(有根,靠祖先链检测)
        let reachable_cycle = bdj(
            json!({
                "r": { "type": "x", "id": "r" },
                "r2": { "type": "y", "id": "r2" }
            }),
            json!({
                "r": { "r2": { "type": "next" } },
                "r2": { "r": { "type": "next" } }
            }),
        );
        assert!(parse_block_data_json(&reachable_cycle).is_err());

        // 纯环(无根):每个 id 都当过别人的子节点
        let rootless = bdj(
            json!({ "a": { "type": "x", "id": "a" } }),
            json!({ "a": { "a": { "type": "next" } } }),
        );
        assert!(parse_block_data_json(&rootless).is_err());

        let dangling = bdj(
            json!({ "a": { "type": "x", "id": "a" } }),
            json!({ "a": { "gone": { "type": "next" } } }),
        );
        assert!(parse_block_data_json(&dangling).is_err());

        assert!(parse_block_data_json(&json!({ "connections": {} })).is_err());

        // 空 blocks 合法(空实体)
        let empty =
            parse_block_data_json(&json!({ "blocks": {}, "connections": {}, "comments": {} }))
                .expect("空实体解析为空树");
        assert_eq!(empty.roots.len(), 0);
    }

    #[test]
    fn diamond_children_are_expanded_twice() {
        // 官方逐边递归:同一子积木被两个槽引用时,两边各展开一份(不共享节点)
        let diamond = json!({
            "blocks": {
                "r": { "type": "root", "id": "r" },
                "mid": { "type": "mid", "id": "mid" },
                "s": { "type": "shared", "id": "s" }
            },
            "connections": {
                "r": {
                    "mid": { "type": "input", "input_type": "value", "input_name": "A" },
                    "s": { "type": "input", "input_type": "value", "input_name": "B" }
                },
                "mid": { "s": { "type": "input", "input_type": "value", "input_name": "A" } },
                "s": {}
            },
            "comments": {}
        });
        let parsed = parse_block_data_json(&diamond).expect("parse");
        assert_eq!(parsed.roots.len(), 1);
        assert_eq!(parsed.count(), 4, "s 在两个槽下各展开一份");
    }
}

#[cfg(test)]
mod kn_typed_tests {
    //! `parse_kn_entity_typed`(源侧骨架的快速通道)与 [`parse_kn_entity`] 的逐项等价
    //! (`rounds/50`)

    use super::super::*;

    /// 原文与 `Value` 两条入口在三种形态上产出同一棵树:数组、JSON 字符串、`null`
    #[test]
    fn typed_matches_value_entry_for_all_shapes() {
        for text in [
            r#"[{"type": "wait", "id": "n1", "fields": {"DURATION": 1}}, {"type": "", "id": "junk"}]"#,
            r#""[{\"type\": \"wait\", \"id\": \"n1\"}]""#,
            "null",
            "[]",
        ] {
            let value: Value = serde_json::from_str(text).expect("样例应是合法 JSON");
            let raw = serde_json::value::RawValue::from_string(text.to_string()).expect("RawValue");
            let from_value = parse_kn_entity(&value).expect("Value 路径");
            let from_raw = parse_kn_entity_typed(&raw).expect("原文路径");
            assert_eq!(
                from_raw.roots, from_value.roots,
                "两条入口必须产出同一棵树(输入 {text})"
            );
        }
    }

    /// `type == ""` 的垃圾节点按官方 `HC` 过滤(两条入口都过滤)
    #[test]
    fn typed_filters_empty_kinds() {
        let raw = serde_json::value::RawValue::from_string(
            r#"[{"type": ""}, {"type": "wait", "id": "n1"}]"#.to_string(),
        )
        .expect("RawValue");
        let tree = parse_kn_entity_typed(&raw).expect("原文路径");
        assert_eq!(tree.count(), 1);
        assert_eq!(tree.roots[0].kind, "wait");
    }

    /// 形状不合的原文(对象而非数组)失败 ⇒ 调用方在该实体粒度回落 `Value` 口径
    #[test]
    fn typed_rejects_wrong_shape() {
        let raw = serde_json::value::RawValue::from_string(r#"{"blocks": {}}"#.to_string())
            .expect("RawValue");
        assert!(parse_kn_entity_typed(&raw).is_err());
    }
}
