use super::{BlockBehavior, BlockContext, BlockDecompilerCore};
use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, IdGenerator, Result, ValueExt,
};
use log::error;
use serde_json::{Value, json};
use std::fmt::Write as _;

// 反编译器上下文
// 反编译器上下文

// 积木反编译器 trait 与具体实现
pub(crate) trait BlockDecompiler<'a>: Send + Sync {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value>;
}

pub(crate) struct DefaultBlockDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
}

impl<'a> DefaultBlockDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
        }
    }
}

impl<'a> BlockDecompiler<'a> for DefaultBlockDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        self.core.decompile(context)
    }
}

pub(crate) struct IfBlockDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> IfBlockDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        let conditions_count = compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map_or(0, std::vec::Vec::len);
        let behavior = BlockBehavior::If { conditions_count };
        let core = BlockDecompilerCore::new(compiled, behavior);
        Self { core, compiled }
    }
}

impl<'a> BlockDecompiler<'a> for IfBlockDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let children = self
            .compiled
            .get("child_block")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DecompilerError::Decompile("child_block不存在".to_string()))?;
        let conditions_len = self
            .compiled
            .get("conditions")
            .and_then(|v| v.as_array())
            .map_or(0, std::vec::Vec::len);

        // 根据方案8.1 修正 else 属性的判断
        let has_else = children.len() > conditions_len
            && !children.last().is_none_or(serde_json::Value::is_null);

        if let Some(obj) = block_value.as_object_mut() {
            let mut shadows_mut = obj.get_mut("shadows").and_then(|s| s.as_object_mut());
            if let Some(shadows) = shadows_mut.as_mut() {
                if has_else {
                    // 编辑版:有 else 时 shadows 同时含 ELSE_TEXT 与 ELSE
                    shadows.insert("ELSE_TEXT".to_string(), json!(""));
                    shadows.insert("ELSE".to_string(), json!(""));
                } else {
                    shadows.insert("EXTRA_ADD_ELSE".to_string(), json!(""));
                }
            }
            // 编辑版:有 else 时 mutation 标记 else="1",无 else 时为空字符串
            if has_else {
                let mutation =
                    r#"<mutation xmlns="http://www.w3.org/1999/xhtml" else="1"></mutation>"#
                        .to_string();
                obj.insert("mutation".to_string(), Value::String(mutation));
            }
        }
        Ok(block_value)
    }
}

pub(crate) struct TextJoinDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> TextJoinDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for TextJoinDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let param_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map_or(0, serde_json::Map::len);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, param_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct AskAndChooseDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> AskAndChooseDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for AskAndChooseDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let item_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map_or(0, serde_json::Map::len);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, item_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct SetEntityShowHideDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> SetEntityShowHideDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for SetEntityShowHideDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let need_text = self
            .compiled
            .get("need_text")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let time_block_id = self
            .compiled
            .get("time")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mutation = format!(
            r#"<mutation need_text="{}" time="{}"></mutation>"#,
            need_text, time_block_id
        );
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct TextSelectChangeableDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> TextSelectChangeableDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for TextSelectChangeableDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let item_count = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .map_or(0, serde_json::Map::len);
        let mutation = format!(r#"<mutation items="{}"></mutation>"#, item_count);
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(mutation));
        }
        Ok(block_value)
    }
}

pub(crate) struct FunctionDefDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> FunctionDefDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            // 函数体 child_block 使用 STACK 插槽
            core: BlockDecompilerCore::new(compiled, BlockBehavior::FunctionBody),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for FunctionDefDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let procedure_name = self.compiled.get_str_or("procedure_name", "");
        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let block = block_value
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("block_value不是对象".to_string()))?;

        if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
            // 编辑版 defnoreturn shadows 键集合:DEFINE / PARAMS0..n / MUTATOR / STACK
            shadows.insert("PROCEDURES_2_DEFNORETURN_DEFINE".to_string(), json!(""));
            shadows.insert("PROCEDURES_2_DEFNORETURN_MUTATOR".to_string(), json!(""));
            shadows.insert("STACK".to_string(), json!(""));
            for i in 0..params.len() {
                // 每个参数插槽配一个 math_number 占位 shadow(编辑版同款)
                let shadow_value = context.shadow_builder.create("math_number", None, None);
                shadows.insert(format!("PARAMS{}", i), shadow_value);
            }
        }

        let mut mutation_args = String::with_capacity(params.len() * 32);
        let parent_id = block
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
            .to_string();

        for (i, (param_name, _)) in params.iter().enumerate() {
            // 编辑版插槽名为 PARAMS0/PARAMS1/...(无空格)
            let input_name = format!("PARAMS{}", i);
            let _ = write!(mutation_args, r#"<arg name="{}"></arg>"#, input_name);

            // 生成稳定的参数块(编辑版 is_shadow=false,可编辑)
            let param_block_id = context.shadow_builder.id_generator.generate(20);
            let param_block = json!({
                "id": param_block_id,
                "type": "procedures_2_stable_parameter",
                "is_shadow": false,
                "is_output": true,
                "fields": {
                    "param_name": param_name,
                    "param_default_value": ""
                },
                "location": [0, 0],
                "collapsed": false,
                "disabled": false,
                "parent_id": parent_id,
                "deletable": true,
                "movable": true,
                "editable": true,
                "visible": "visible",
                "comment": null,
                "mutation": "",
                "shadows": {},
                "field_constraints": {},
                "field_extra_attr": {}
            });
            context.blocks.insert(param_block_id.clone(), param_block);
            context.insert_connection(
                &parent_id,
                &param_block_id,
                json!({
                    "type": "input",
                    "input_type": "value",
                    "input_name": input_name
                }),
            );
        }

        // 编辑版 mutation:<mutation xmlns="..."><arg name="PARAMS0"></arg>...</mutation>
        let mutation = format!(
            r#"<mutation xmlns="http://www.w3.org/1999/xhtml">{}</mutation>"#,
            mutation_args
        );
        block.insert("mutation".to_string(), Value::String(mutation));

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| DecompilerError::Decompile("fields对象不存在".to_string()))?;
        fields.insert(
            "NAME".to_string(),
            Value::String(procedure_name.to_string()),
        );
        Ok(block_value)
    }
}

pub(crate) struct FunctionCallDecompiler<'a> {
    core: BlockDecompilerCore<'a>,
    compiled: &'a Value,
}

impl<'a> FunctionCallDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self {
            core: BlockDecompilerCore::new(compiled, BlockBehavior::Default),
            compiled,
        }
    }
}

impl<'a> BlockDecompiler<'a> for FunctionCallDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.core.decompile(context)?;
        let procedure_name = self.compiled.get_str_or("procedure_name", "");

        let (def_id, disabled) = if let Some(func) = context.functions.get(procedure_name) {
            let id = func.get("id").and_then(|v| v.as_str()).unwrap_or("");
            (id.to_string(), false)
        } else {
            error!("调用未定义的函数: {},将禁用该积木", procedure_name);
            (String::new(), true)
        };

        let params = self
            .compiled
            .get("params")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let block = block_value
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("block_value不是对象".to_string()))?;

        block.insert("disabled".to_string(), Value::Bool(disabled));

        let mut mutation = String::from(r#"<mutation xmlns="http://www.w3.org/1999/xhtml""#);
        let _ = write!(mutation, r#" name="{}""#, procedure_name);
        let _ = write!(mutation, r#" def_id="{}""#, def_id);
        mutation.push('>');
        for (param_name, _) in &params {
            let _ = write!(
                mutation,
                r#"<procedures_2_parameter_shadow name="{}" value="0"></procedures_2_parameter_shadow>"#,
                param_name
            );
        }
        mutation.push_str("</mutation>");
        block.insert("mutation".to_string(), Value::String(mutation));

        if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
            shadows.insert("NAME".to_string(), json!(""));
        }

        let parent_id = block
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("当前块缺少 id".to_string()))?
            .to_string();

        for (param_index, (_param_name, param_value)) in params.iter().enumerate() {
            // 编辑版插槽名为 ARG0/ARG1/...(无空格)
            let input_name = format!("ARG{}", param_index);
            if param_value.is_object() {
                let mut param_decompiler =
                    BlockDecompilerCore::new(param_value, BlockBehavior::Default);
                let param_block = param_decompiler.decompile(context)?;
                let param_id = param_block
                    .get("id")
                    .ok_or_else(|| {
                        DecompilerError::InvalidResponse("param_block缺少id".to_string())
                    })?
                    .as_str()
                    .ok_or_else(|| {
                        DecompilerError::InvalidResponse("param_block id不是字符串".to_string())
                    })?
                    .to_string();
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
                        "input_name": input_name
                    }),
                );
                if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
                    let shadow_value =
                        context
                            .shadow_builder
                            .create("default_value", Some(param_id), None);
                    shadows.insert(input_name, shadow_value);
                }
            } else if let Some(shadows) = block.get_mut("shadows").and_then(|s| s.as_object_mut()) {
                let shadow_value = context.shadow_builder.create("default_value", None, None);
                shadows.insert(input_name, shadow_value);
            }
        }

        let fields = block
            .get_mut("fields")
            .and_then(|v| v.as_object_mut())
            .ok_or_else(|| DecompilerError::Decompile("fields对象不存在".to_string()))?;
        fields.insert(
            "NAME".to_string(),
            Value::String(procedure_name.to_string()),
        );
        Ok(block_value)
    }
}

pub(crate) struct MutationDecompiler<'a> {
    inner: DefaultBlockDecompiler<'a>,
    mutation: String,
}

impl<'a> MutationDecompiler<'a> {
    pub(crate) fn new(compiled: &'a Value, mutation: String) -> Self {
        Self {
            inner: DefaultBlockDecompiler::new(compiled),
            mutation,
        }
    }
}

impl<'a> BlockDecompiler<'a> for MutationDecompiler<'a> {
    fn decompile(&mut self, context: &mut BlockContext) -> Result<Value> {
        let mut block_value = self.inner.decompile(context)?;
        if let Some(obj) = block_value.as_object_mut() {
            obj.insert("mutation".to_string(), Value::String(self.mutation.clone()));
        }
        Ok(block_value)
    }
}

// 积木反编译器工厂
/// 按块类型分派专用反编译器
/// 树内递归(process_next/children/conditions/params)也使用本函数,
/// 否则嵌套的 procedures_2_callnoreturn / controls_if 等不会走专用反编译器,
/// 导致 NAME/mutation/ARG 参数块/if-else 结构缺失
/// 独立于 BlockDecompilerFactory,避免其 lifetime 绑定 BlockContext
pub(crate) fn create_block_decompiler<'a>(
    compiled: &'a Value,
) -> Box<dyn BlockDecompiler<'a> + 'a> {
    let block_type = compiled.get_str_or("type", "");
    match block_type {
        "controls_if" | "controls_if_no_else" => Box::new(IfBlockDecompiler::new(compiled)),
        "text_join" => Box::new(TextJoinDecompiler::new(compiled)),
        "ask_and_choose" => Box::new(AskAndChooseDecompiler::new(compiled)),
        "set_entity_show_hide" => Box::new(SetEntityShowHideDecompiler::new(compiled)),
        "text_select_changeable" => Box::new(TextSelectChangeableDecompiler::new(compiled)),
        "procedures_2_defnoreturn" => Box::new(FunctionDefDecompiler::new(compiled)),
        "procedures_2_callnoreturn" | "procedures_2_callreturn" => {
            Box::new(FunctionCallDecompiler::new(compiled))
        }
        "procedures_2_return_value" => {
            let item_count = compiled
                .get("params")
                .and_then(|v| v.as_object())
                .map_or(0, serde_json::Map::len);
            let mutation = format!("<mutation items=\"{}\"></mutation>", item_count);
            Box::new(MutationDecompiler::new(compiled, mutation))
        }
        "procedures_2_stable_parameter" | "procedures_2_parameter" => {
            Box::new(DefaultBlockDecompiler::new(compiled))
        }
        _ => Box::new(DefaultBlockDecompiler::new(compiled)),
    }
}

pub(crate) struct BlockDecompilerFactory<'a> {
    config: &'a DecompilerConfig,
    id_generator: &'a IdGenerator,
}

impl<'a> BlockDecompilerFactory<'a> {
    pub(crate) fn new(config: &'a DecompilerConfig, id_generator: &'a IdGenerator) -> Self {
        Self {
            config,
            id_generator,
        }
    }

    pub(crate) fn create(&self, compiled: &'a Value) -> Box<dyn BlockDecompiler<'a> + 'a> {
        create_block_decompiler(compiled)
    }
}
