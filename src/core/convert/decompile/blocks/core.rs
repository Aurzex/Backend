use super::{BlockBehavior, BlockContext, BlockDecompilerBehavior, create_block_decompiler};
use crate::core::convert::shared::{DecompilerError, Result, ValueExt};
use serde_json::{Value, json};
use std::collections::HashMap;

// 积木反编译核心

pub(crate) struct BlockDecompilerCore<'a> {
    compiled: &'a Value,
    behavior: BlockBehavior,
}

impl<'a> BlockDecompilerCore<'a> {
    pub(crate) fn new(compiled: &'a Value, behavior: BlockBehavior) -> Self {
        Self { compiled, behavior }
    }

    pub(crate) fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let config = &context.shadow_builder.config;
        let id = self.compiled.get_str_or("id", "");
        let block_type = self.compiled.get_str_or("type", "");
        let is_shadow = config.shadow_types.contains(block_type);
        // 编辑版 is_output 与编译版 output_type 严格对应(0→false,2→true)
        let output_type = self.compiled.get_i64_or_default("output_type", 0);
        let is_output = is_shadow || output_type > 0;

        let location = self.compiled.get_array_opt("location").map_or_else(
            || {
                // 编译版无 location:按树形自动布局,避免全部重叠在 [0,0]
                let loc = json!([context.layout_col, context.layout_row]);
                context.layout_row += 70.0;
                loc
            },
            |arr| Value::Array(arr.clone()),
        );

        let mut block_value = json!({
            "id": id,
            "type": block_type,
            "location": location,
            "is_shadow": is_shadow,
            "is_output": is_output,
            "collapsed": false,
            "disabled": false,
            "parent_id": null,
            "deletable": true,
            "movable": true,
            "editable": true,
            "visible": "visible",
            "fields": {},
            "field_constraints": {},
            "field_extra_attr": {},
            "comment": self.compiled.get("comment").cloned().unwrap_or(Value::Null),
            "mutation": "",
        });

        let mut shadows: HashMap<String, Value> = HashMap::new();

        self.process_next(context, &mut block_value)?;
        self.process_children(context, &mut shadows, &mut block_value)?;
        self.process_conditions(context, &mut shadows, &mut block_value)?;
        self.process_params(context, &mut shadows, &mut block_value)?;

        if let Some(obj) = block_value.as_object_mut() {
            let shadows_map: serde_json::Map<String, Value> = shadows.into_iter().collect();
            obj.insert("shadows".to_string(), Value::Object(shadows_map));
        }

        // 不再在此处插入 blocks:所有调用方(process_next/children/conditions/params、
        // 顶层循环、FunctionCall 参数块)都会将返回值重新插入 context.blocks,
        // 原深克隆 + 哈希插入 + 丢弃每积木重复一次,属纯浪费
        Ok(block_value)
    }

    fn process_next(&self, context: &mut BlockContext, block_value: &mut Value) -> Result<()> {
        if let Some(next_compiled) = self.compiled.get("next_block")
            && !next_compiled.is_null()
        {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            // 树内块也经专用分派,保证 callnoreturn/controls_if 等专用反编译器生效
            let mut decompiler = create_block_decompiler(next_compiled);
            // 下一层链块向右缩进一个层级
            context.layout_col += 220.0;
            let next_block = decompiler.decompile(context)?;
            context.layout_col -= 220.0;
            let next_id = next_block
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("next_block缺少id".to_string()))?
                .to_string();
            context.blocks.insert(next_id.clone(), next_block);
            if let Some(b) = context.blocks.get_mut(&next_id)
                && let Some(o) = b.as_object_mut()
            {
                o.insert("parent_id".to_string(), json!(parent_id));
            }
            context.insert_connection(&parent_id, &next_id, json!({"type": "next"}));
        }
        Ok(())
    }

    fn process_children(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(children) = self.compiled.get("child_block").and_then(|v| v.as_array()) {
            let conditions_count = self
                .compiled
                .get("conditions")
                .and_then(|v| v.as_array())
                .map_or(0, std::vec::Vec::len);

            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (i, child) in children.iter().enumerate() {
                if !child.is_null() {
                    let mut decompiler = create_block_decompiler(child);
                    context.layout_col += 220.0;
                    let child_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let child_id = child_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("child_block缺少id".to_string())
                        })?
                        .to_string();
                    let input_name = self.behavior.get_child_input_name(i, conditions_count);
                    context.blocks.insert(child_id.clone(), child_block);
                    if let Some(b) = context.blocks.get_mut(&child_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &child_id,
                        json!({
                            "type": "input",
                            "input_type": "statement",
                            "input_name": input_name
                        }),
                    );
                    if let std::collections::hash_map::Entry::Vacant(e) = shadows.entry(input_name)
                    {
                        let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                        e.insert(shadow_value);
                    }
                }
            }
        }
        Ok(())
    }

    fn process_conditions(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        if let Some(conditions) = self.compiled.get("conditions").and_then(|v| v.as_array()) {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (i, condition) in conditions.iter().enumerate() {
                let input_name = format!("IF{}", i);
                if condition.is_null() {
                    let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                    shadows.insert(input_name, shadow_value);
                } else {
                    let mut decompiler = create_block_decompiler(condition);
                    context.layout_col += 220.0;
                    let condition_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    let cond_id = condition_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("condition_block缺少id".to_string())
                        })?
                        .to_string();
                    context.blocks.insert(cond_id.clone(), condition_block);
                    if let Some(b) = context.blocks.get_mut(&cond_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &cond_id,
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": input_name
                        }),
                    );
                    let shadow_value = context.shadow_builder.create("logic_empty", None, None);
                    shadows.insert(input_name, shadow_value);
                }
            }
        }
        Ok(())
    }

    fn infer_shadow_type(&self, param_name: &str, value: &Value) -> &'static str {
        match param_name {
            "condition" | "BOOL" => "logic_empty",
            "message" | "MESSAGE" => "broadcast_input",
            "sound_id" | "SOUND" => "get_audios",
            "whole_sound" | "all_sounds" => "get_whole_audios",
            "style_id" | "costume" | "COSTUME" => "get_current_costume",
            "scene" | "SCENE" | "scene_id" => "get_current_scene",
            "list" | "LIST" => "lists_get",
            _ => match value {
                Value::String(_) => "text",
                Value::Bool(_) => "logic_boolean",
                _ => "math_number",
            },
        }
    }

    fn process_params(
        &self,
        context: &mut BlockContext,
        shadows: &mut HashMap<String, Value>,
        block_value: &mut Value,
    ) -> Result<()> {
        let block_type = self.compiled.get_str_or("type", "");
        // 过程定义/调用块的 params(参数名→参数块)由 FunctionDef/FunctionCallDecompiler
        // 单独处理,此处跳过避免双连接与 fields 污染
        if block_type == "procedures_2_defnoreturn"
            || block_type == "procedures_2_callnoreturn"
            || block_type == "procedures_2_callreturn"
        {
            return Ok(());
        }
        if let Some(params) = self.compiled.get_object_opt("params") {
            let parent_id = block_value
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
                .to_string();

            for (name, value) in params {
                if value.is_object() {
                    let mut decompiler = create_block_decompiler(value);
                    context.layout_col += 220.0;
                    let param_block = decompiler.decompile(context)?;
                    context.layout_col -= 220.0;
                    // 类型名较短,转为拥有值以解除对 param_block 的借用,允许随后移动
                    let param_type = param_block
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let param_id = param_block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            DecompilerError::InvalidResponse("param_block缺少id".to_string())
                        })?
                        .to_string();
                    // 先取类型再移动:param_block 的 clone 仅为满足 insert 的取所有权,
                    // 移动后不再需要原值
                    context.blocks.insert(param_id.clone(), param_block);
                    if let Some(b) = context.blocks.get_mut(&param_id)
                        && let Some(o) = b.as_object_mut()
                    {
                        o.insert("parent_id".to_string(), json!(parent_id));
                    }
                    context.insert_connection(
                        &parent_id,
                        &param_id,
                        json!({
                            "type": "input",
                            "input_type": "value",
                            "input_name": name
                        }),
                    );
                    if context
                        .shadow_builder
                        .config
                        .shadow_types
                        .contains(&param_type)
                    {
                        // 编辑版 shadow 模板显示的是类型默认值(如 math_number 的 0),
                        // 与参数块实际值无关,因此不传 text
                        let shadow_value = context.shadow_builder.create(
                            &param_type,
                            Some(param_id.clone()),
                            None,
                        );
                        shadows.insert(name.clone(), shadow_value);
                    } else {
                        let shadow_type = self.infer_shadow_type(name, &Value::Null);
                        let shadow_value = context.shadow_builder.create(shadow_type, None, None);
                        shadows.insert(name.clone(), shadow_value);
                    }
                } else {
                    // 处理基本类型参数(如变量 UUID 引用)
                    // 布尔开关参数(如 bump 的 warp)在编辑版中不呈现
                    // (无 shadow,无 fields),跳过以对齐编辑版格式
                    if value.is_boolean() {
                        continue;
                    }
                    if name == "VAR" {
                        // 编辑版格式:变量引用以 UUID 存入 fields(variables_set/get 均如此),
                        // 且不生成 shadow(编辑版变量块的 shadows 中无 VAR 键)
                        if let Some(fields) = block_value
                            .as_object_mut()
                            .and_then(|v| v.get_mut("fields").and_then(|v| v.as_object_mut()))
                        {
                            fields.insert(name.clone(), value.clone());
                        }
                        continue;
                    }
                    let shadow_type = self.infer_shadow_type(name, value);
                    let num_str;
                    let shadow_text = match value {
                        Value::String(s) => Some(s.as_str()),
                        Value::Number(n) => {
                            num_str = n.to_string();
                            Some(num_str.as_str())
                        }
                        _ => None,
                    };
                    let shadow_value =
                        context
                            .shadow_builder
                            .create(shadow_type, None, shadow_text);
                    shadows.insert(name.clone(), shadow_value);

                    if let Some(fields) = block_value
                        .as_object_mut()
                        .and_then(|v| v.get_mut("fields").and_then(|v| v.as_object_mut()))
                    {
                        fields.insert(name.clone(), value.clone());
                    }
                }
            }
        }
        Ok(())
    }
}
