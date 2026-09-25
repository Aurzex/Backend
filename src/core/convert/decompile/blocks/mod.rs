use super::shadow::ShadowBuilder;
use crate::core::convert::shared::{DecompilerError, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(crate) mod core;
pub(crate) mod special;

pub(crate) use special::{BlockDecompiler, create_block_decompiler};

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
