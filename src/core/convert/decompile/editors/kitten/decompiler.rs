use crate::core::convert::decompile::{
    blocks::{BlockContext, BlockDecompilerFactory},
    shadow::ShadowBuilder,
};
use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, EditorType, IdGenerator, Result, WorkInfo,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(crate) struct KittenDecompiler;

impl KittenDecompiler {
    /// 从 work 的 theatre 中移出角色信息(所有权转移,避免整角色深克隆);
    /// 缺失时回退为默认角色 JSON
    pub(crate) fn take_actor_info(work: &mut Value, actor_id: &str) -> Value {
        if let Some(theatre) = work.get_mut("theatre").and_then(|v| v.as_object_mut()) {
            if let Some(actors) = theatre.get_mut("actors").and_then(|v| v.as_object_mut())
                && let Some(actor) = actors.remove(actor_id)
            {
                return actor;
            }
            if let Some(scenes) = theatre.get_mut("scenes").and_then(|v| v.as_object_mut())
                && let Some(scene) = scenes.remove(actor_id)
            {
                return scene;
            }
        }
        // 按字符截断而非字节:actor_id.len() 为字节数,直接切片可能切断多字节字符导致 panic
        let short_id: String = actor_id.chars().take(8).collect();
        json!({
            "direction": 90,
            "draggable": false,
            "id": actor_id,
            "name": format!("未知角色_{}", short_id),
            "rotation_style": "all around",
            "size": 100,
            "type": "sprite",
            "visible": true,
            "x": 0,
            "y": 0,
        })
    }

    /// 收集被 next_block/child_block/conditions/params 引用的块 ID(角色/场景共享)
    fn collect_referenced_ids(blocks: &serde_json::Map<String, Value>) -> HashSet<String> {
        let mut referenced_ids: HashSet<String> = HashSet::new();
        for (_, block) in blocks {
            if let Some(next) = block.get("next_block") {
                if let Some(id) = next.as_str() {
                    referenced_ids.insert(id.to_string());
                } else if let Some(obj) = next.as_object()
                    && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                {
                    referenced_ids.insert(id.to_string());
                }
            }
            if let Some(children) = block.get("child_block").and_then(|v| v.as_array()) {
                for child in children {
                    if let Some(id) = child.as_str() {
                        referenced_ids.insert(id.to_string());
                    } else if let Some(obj) = child.as_object()
                        && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                    {
                        referenced_ids.insert(id.to_string());
                    }
                }
            }
            if let Some(conditions) = block.get("conditions").and_then(|v| v.as_array()) {
                for cond in conditions {
                    if let Some(id) = cond.as_str() {
                        referenced_ids.insert(id.to_string());
                    } else if let Some(obj) = cond.as_object()
                        && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                    {
                        referenced_ids.insert(id.to_string());
                    }
                }
            }
            if let Some(params) = block.get("params").and_then(|v| v.as_object()) {
                for (_, param_value) in params {
                    if let Some(obj) = param_value.as_object()
                        && let Some(id) = obj.get("id").and_then(|v| v.as_str())
                    {
                        referenced_ids.insert(id.to_string());
                    }
                }
            }
        }
        referenced_ids
    }

    /// 反编译根块(未被引用的块)并插入 context(角色/场景共享)
    fn decompile_root_blocks(
        blocks: &serde_json::Map<String, Value>,
        factory: &BlockDecompilerFactory,
        context: &mut BlockContext,
    ) -> Result<()> {
        let referenced_ids = Self::collect_referenced_ids(blocks);
        for (id, block_data) in blocks {
            if !referenced_ids.contains(id) {
                // 根块之间增加垂直间距,避免自动布局后挤在一起
                context.layout_row += 50.0;
                let mut decompiler = factory.create(block_data);
                // 重新插入补充后的块(If/FunctionDef 等会修改 block_value)
                let block_value = decompiler.decompile(context)?;
                if let Some(bid) = block_value.get("id").and_then(|v| v.as_str()) {
                    context.blocks.insert(bid.to_string(), block_value);
                }
            }
        }
        Ok(())
    }

    /// 反编译函数定义块(procedures_2_defnoreturn)并插入 context(角色/场景共享)
    fn decompile_procedures(
        actor_compiled: &Value,
        factory: &BlockDecompilerFactory,
        context: &mut BlockContext,
    ) -> Result<()> {
        // 函数可能定义在角色/场景(屏幕角色)中,被其它场景/角色调用;
        // 独立于 compiled_block_map,避免其缺失时连带丢失函数定义
        if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
            for (_, func_data) in procedures {
                context.layout_row += 50.0;
                let mut decompiler = factory.create(func_data);
                // 重新插入:FunctionDefDecompiler 补充的 shadows/mutation/NAME 需覆盖 core 版本
                let block_value = decompiler.decompile(context)?;
                if let Some(bid) = block_value.get("id").and_then(|v| v.as_str()) {
                    context.blocks.insert(bid.to_string(), block_value);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn decompile_actor_blocks(
        config: &Arc<DecompilerConfig>,
        id_generator: &IdGenerator,
        actor_compiled: &Value,
        functions: &Arc<HashMap<String, Value>>,
        actor_info: Value,
        variable_map: Arc<HashMap<String, String>>,
        work_type: EditorType,
    ) -> Result<Value> {
        let shadow_builder = ShadowBuilder::new(config.clone(), id_generator.clone(), work_type);
        let compiled_blocks = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object());
        let estimated_blocks = compiled_blocks.map_or(256, |m| m.len() * 10 + 100);
        let functions_arc = Arc::clone(functions);
        // 移动前先提取 actor_info 中已有的注释,供注释回退使用
        let actor_existing_comments = actor_info
            .get("block_data_json")
            .and_then(|b| b.get("comments"))
            .cloned();
        let mut context = BlockContext::with_capacity(
            actor_info,
            functions_arc,
            shadow_builder,
            variable_map,
            estimated_blocks,
            estimated_blocks * 2,
        );

        let factory = BlockDecompilerFactory::new(config.as_ref(), id_generator);

        if let Some(blocks) = compiled_blocks {
            Self::decompile_root_blocks(blocks, &factory, &mut context)?;
        }

        // 生成函数定义块(procedures_2_defnoreturn),否则调用块会因找不到
        // 定义而被 FunctionCallDecompiler 置为 disabled,函数功能丢失
        // 独立于 compiled_block_map,避免其缺失时连带丢失函数定义
        Self::decompile_procedures(actor_compiled, &factory, &mut context)?;

        // 优先使用 compile_result 中的注释;若数据源未提供,则保留 actor_info
        // 中已有的注释,避免反编译覆盖掉输入中已有的注释数据
        let mut comments = actor_compiled
            .get("comments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if comments.as_object().is_none_or(serde_json::Map::is_empty)
            && let Some(existing) = actor_existing_comments
        {
            comments = existing;
        }

        let mut actor_data = context.actor_data;
        if let Some(obj) = actor_data.as_object_mut() {
            obj.insert(
                "block_data_json".to_string(),
                json!({
                    "blocks": context.blocks,
                    "connections": context.connections,
                    "comments": comments,
                }),
            );
        }
        Ok(actor_data)
    }

    pub(crate) fn decompile_scene_blocks(
        config: &Arc<DecompilerConfig>,
        id_generator: &IdGenerator,
        actor_compiled: &Value,
        scene_info: Value,
        work_type: EditorType,
        functions: &Arc<HashMap<String, Value>>,
    ) -> Result<Value> {
        let shadow_builder = ShadowBuilder::new(config.clone(), id_generator.clone(), work_type);
        let compiled_blocks = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object());
        let estimated_blocks = compiled_blocks.map_or(256, |m| m.len() * 10 + 100);
        // 场景同样使用全局函数表:函数可定义在某个场景(屏幕角色)中,
        // 被其它场景/角色调用(如"总移动设置4"定义在背景(3),调用在背景(1)),
        // 否则场景中的调用块会因找不到定义而被禁用
        let mut context = BlockContext::with_capacity(
            json!({}),
            Arc::clone(functions),
            shadow_builder,
            Arc::new(HashMap::new()), // 场景没有变量映射
            estimated_blocks,
            estimated_blocks * 2,
        );

        let factory = BlockDecompilerFactory::new(config.as_ref(), id_generator);

        if let Some(blocks) = compiled_blocks {
            Self::decompile_root_blocks(blocks, &factory, &mut context)?;
        }

        // 生成函数定义块(procedures_2_defnoreturn).函数可能定义在场景
        // (屏幕角色)中(如"总移动设置4"定义在背景(3)),与角色分支一致,
        // 否则场景中定义的函数缺失,调用块会被 FunctionCallDecompiler 禁用
        Self::decompile_procedures(actor_compiled, &factory, &mut context)?;

        // 优先使用 compile_result 中的注释;若数据源未提供,则保留 scene_info
        // 中已有的注释,避免反编译覆盖掉输入中已有的注释数据
        let mut comments = actor_compiled
            .get("comments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if comments.as_object().is_none_or(serde_json::Map::is_empty)
            && let Some(existing) = scene_info
                .get("block_data_json")
                .and_then(|b| b.get("comments"))
        {
            comments = existing.clone();
        }

        let mut scene = scene_info;
        if let Some(obj) = scene.as_object_mut() {
            obj.insert(
                "block_data_json".to_string(),
                json!({
                    "blocks": context.blocks,
                    "connections": context.connections,
                    "comments": comments,
                }),
            );
        }
        Ok(scene)
    }

    pub(crate) fn update_work_info(
        work: &mut Value,
        work_info: &WorkInfo,
        config: &DecompilerConfig,
    ) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;

        let feature_keys = [
            "physics2",
            "cloud_variable",
            "cloud_list",
            "ai_lab",
            "camera",
            "video",
            "midimusic",
        ];
        let mut original_features = serde_json::Map::new();
        for key in &feature_keys {
            if let Some(val) = work_obj.get(*key) {
                original_features.insert((*key).to_string(), val.clone());
            }
        }

        work_obj.insert(
            "hidden_toolbox".to_string(),
            json!({
                "toolbox": [],
                "blocks": [],
            }),
        );
        // Kitten3 编辑版(如春风得意)work_source_label 为 6,且无 sample_id/设备/最后工具箱等字段
        let is_k3 = matches!(
            work_info.work_type,
            EditorType::Kitten2 | EditorType::Kitten3
        );
        work_obj.insert(
            "work_source_label".to_string(),
            json!(if is_k3 { 6 } else { 1 }),
        );
        if is_k3 {
            // Kitten3 编辑版(如春风得意)顶层含 work_business 字段
            work_obj.insert("work_business".to_string(), json!(0));
        }
        if !is_k3 {
            work_obj.insert("sample_id".to_string(), json!(""));
            work_obj.insert("codemao_value".to_string(), json!(work_info.id.to_string()));
            work_obj.insert("device_widget_type".to_string(), Value::Null);
        }
        work_obj.insert("project_name".to_string(), json!(work_info.name));
        work_obj.insert(
            "toolbox_order".to_string(),
            json!(config.toolbox_categories),
        );
        if !is_k3 {
            work_obj.insert(
                "last_toolbox_order".to_string(),
                json!(config.toolbox_categories),
            );
        }

        for (k, v) in original_features {
            work_obj.insert(k, v);
        }
        Ok(())
    }

    pub(crate) fn clean_work_data(work: &mut Value, work_type: EditorType) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;
        let keys_to_remove = ["compile_result", "preview", "author_nickname"];
        for key in &keys_to_remove {
            work_obj.remove(*key);
        }
        // 清理编译版 theatre 的运行时字段:Kitten4 编辑版无这些键,
        // 但 Kitten3 编辑版(如春风得意)保留 current_entity/current_scene/style_collections
        if !matches!(work_type, EditorType::Kitten2 | EditorType::Kitten3)
            && let Some(theatre) = work.get_mut("theatre").and_then(|t| t.as_object_mut())
        {
            for key in ["current_entity", "current_scene", "style_collections"] {
                theatre.remove(key);
            }
        }
        Ok(())
    }

    pub(crate) fn restore_global_fields(
        work: &mut Value,
        restore_fields: &HashMap<&'static str, Value>,
        restore_groups: Option<&Value>,
    ) {
        for (key, val) in restore_fields {
            work[key] = val.clone();
        }

        if let Some(groups) = restore_groups
            && let Some(theatre) = work.get_mut("theatre")
            && let Some(obj) = theatre.as_object_mut()
        {
            obj.insert("groups".to_string(), groups.clone());
        }
    }
}
