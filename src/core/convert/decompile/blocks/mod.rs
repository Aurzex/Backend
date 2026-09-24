use super::shadow::ShadowBuilder;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) mod core;
pub(crate) mod special;

pub(crate) use core::BlockDecompilerCore;
pub(crate) use special::{BlockDecompiler, BlockDecompilerFactory, create_block_decompiler};

// 积木行为
pub(crate) trait BlockDecompilerBehavior: Send + Sync {
    fn get_child_input_name(&self, index: usize, conditions_count: usize) -> String;
}

#[derive(Clone)]
pub(crate) enum BlockBehavior {
    Default,
    If { conditions_count: usize },
    FunctionBody,
}

impl BlockDecompilerBehavior for BlockBehavior {
    fn get_child_input_name(&self, index: usize, _conditions_count: usize) -> String {
        match self {
            BlockBehavior::Default => "DO".to_string(),
            BlockBehavior::If { conditions_count } => {
                if index < *conditions_count {
                    // 编辑版插槽名为 DO0/DO1/...(无空格)
                    format!("DO{}", index)
                } else {
                    // 编辑版 else 分支插槽名为 ELSE(无编号)
                    "ELSE".to_string()
                }
            }
            // 函数定义块的函数体插槽名为 STACK
            BlockBehavior::FunctionBody => "STACK".to_string(),
        }
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
