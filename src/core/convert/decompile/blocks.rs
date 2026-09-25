//! 积木反编译:编译版积木树 → 编辑版 `block_data_json`。
//!
//! 三件事在同一文件里(原 `blocks/{mod,core,special}.rs`,合并原因:它们是同一个状态机):
//!
//! - **插槽命名**(`child_input_name`)与**根块判定**(`referenced_ids`):规则唯一来源,
//!   与 Kitten2/3 的 blocksXML 序列化共用;
//! - **骨架**(`BlockDecompilerCore`):`next`/`child_block`/`conditions`/`params` 的递归展开、
//!   自动布局、连接表与 `parent_id` 维护;
//! - **专用反编译器**(`If`/`FunctionDef`/`FunctionCall`/`TextJoin`/`Mutation` 等)与
//!   按编译版 `type` 分派的 `create_block_decompiler`。

use crate::core::convert::shared::ShadowBuilder;
use crate::core::convert::shared::{DecompilerError, Result, ValueExt};
use log::error;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;

/// 编译版 `child_block` → 编辑版语句插槽名。
///
/// 唯一的规则来源:反编译重建(本模块)与 Kitten2/3 的 blocksXML 序列化
/// ([`crate::core::convert::decompile::editors::kitten::XmlBlockWriter`])共用,
/// 避免两处各写一份后漂移。
pub(crate) fn child_input_name(block_type: &str, index: usize, conditions_count: usize) -> String {
    match block_type {
        "controls_if" | "controls_if_no_else" => {
            if index < conditions_count {
                // 编辑版插槽名为 DO0/DO1/...(无空格)
                format!("DO{}", index)
            } else {
                // 编辑版 else 分支插槽名为 ELSE(无编号)
                "ELSE".to_string()
            }
        }
        // 函数定义块的函数体插槽名为 STACK
        "procedures_2_defnoreturn" => "STACK".to_string(),
        _ => "DO".to_string(),
    }
}

// 积木上下文
#[derive(Clone)]
pub(crate) struct BlockContext {
    pub(crate) actor_data: Value,
    pub(crate) functions: Arc<HashMap<String, Value>>,
    pub(crate) variable_map: Arc<HashMap<String, String>>, // UUID -> 变量名
    pub(crate) shadow_builder: ShadowBuilder,
    pub(crate) blocks: HashMap<String, Value>,
    pub(crate) connections: HashMap<String, HashMap<String, Value>>,
    // 布局游标:编译版无 location 时按树形自动排列积木,避免恢复产物全部重叠在 [0,0]
    pub(crate) layout_col: f64,
    pub(crate) layout_row: f64,
}

impl BlockContext {
    pub(crate) fn new(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
        variable_map: Arc<HashMap<String, String>>,
    ) -> Self {
        Self {
            actor_data,
            functions,
            variable_map,
            shadow_builder,
            blocks: HashMap::new(),
            connections: HashMap::new(),
            layout_col: 0.0,
            layout_row: 0.0,
        }
    }

    pub(crate) fn with_capacity(
        actor_data: Value,
        functions: Arc<HashMap<String, Value>>,
        shadow_builder: ShadowBuilder,
        variable_map: Arc<HashMap<String, String>>,
        blocks_cap: usize,
        connections_cap: usize,
    ) -> Self {
        Self {
            actor_data,
            functions,
            variable_map,
            shadow_builder,
            blocks: HashMap::with_capacity(blocks_cap),
            connections: HashMap::with_capacity(connections_cap),
            layout_col: 0.0,
            layout_row: 0.0,
        }
    }

    pub(crate) fn insert_connection(
        &mut self,
        source_id: &str,
        target_id: &str,
        connection_info: Value,
    ) {
        self.connections
            .entry(source_id.to_string())
            .or_default()
            .insert(target_id.to_string(), connection_info);
    }
}

/// 编译版块表里「被引用过」的块 id(`next_block`/`child_block`/`conditions`/`params`)。
///
/// 编译版的引用**恒为内联对象**(Kitten2/3/4 的 `compiled_block_map` 实测 2 236 处采样
/// 全是对象,见 `docs/21-convert-domain-consolidation-plan.md` §7-1);字符串 id 只出现在
/// **编辑版** `block_data_json` 的 `connections`。真出现字符串说明编译格式漂移了 ——
/// 这里显式报错,而不是静默漏掉引用(那会把子块当成根块、产物多出一堆散块)。
pub(crate) fn referenced_ids(blocks: &serde_json::Map<String, Value>) -> Result<HashSet<String>> {
    fn id_of(value: &Value) -> Option<&str> {
        value.get("id").and_then(Value::as_str)
    }
    fn reject_string(block_id: &str, field: &str) -> DecompilerError {
        DecompilerError::InvalidResponse(format!(
            "块 {block_id} 的 {field} 是字符串 id:编译版引用恒为内联对象(见 docs/21 §7-1)"
        ))
    }

    let mut referenced: HashSet<String> = HashSet::new();
    for (block_id, block) in blocks {
        if let Some(next) = block.get("next_block") {
            if next.is_string() {
                return Err(reject_string(block_id, "next_block"));
            }
            if let Some(id) = id_of(next) {
                referenced.insert(id.to_string());
            }
        }
        for field in ["child_block", "conditions"] {
            let Some(items) = block.get(field).and_then(Value::as_array) else {
                continue;
            };
            for item in items {
                if item.is_string() {
                    return Err(reject_string(block_id, field));
                }
                if let Some(id) = id_of(item) {
                    referenced.insert(id.to_string());
                }
            }
        }
        // `params` 的值允许是标量(非引用一律忽略),只有内联对象才算引用
        if let Some(params) = block.get("params").and_then(Value::as_object) {
            for param_value in params.values() {
                if let Some(id) = id_of(param_value) {
                    referenced.insert(id.to_string());
                }
            }
        }
    }
    Ok(referenced)
}

// ===========================================================================
// 反编译骨架(原 blocks/core.rs)
// ===========================================================================
// 积木反编译核心

pub(crate) struct BlockDecompilerCore<'a> {
    compiled: &'a Value,
}

impl<'a> BlockDecompilerCore<'a> {
    pub(crate) fn new(compiled: &'a Value) -> Self {
        Self { compiled }
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
                    let input_name = child_input_name(
                        self.compiled.get_str_or("type", ""),
                        i,
                        conditions_count,
                    );
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

// ===========================================================================
// 专用积木反编译器与分派(原 blocks/special.rs)
// ===========================================================================
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
            core: BlockDecompilerCore::new(compiled),
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
        let core = BlockDecompilerCore::new(compiled);
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
            core: BlockDecompilerCore::new(compiled),
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
            core: BlockDecompilerCore::new(compiled),
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
            core: BlockDecompilerCore::new(compiled),
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
            core: BlockDecompilerCore::new(compiled),
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
            core: BlockDecompilerCore::new(compiled),
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
            core: BlockDecompilerCore::new(compiled),
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
                    BlockDecompilerCore::new(param_value);
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
