//! Kitten(Kitten2/3/4)抓取与反编译。
//!
//! 三件事同一文件(原 `kitten/{mod,decompiler,xml}.rs`):
//! - `KittenFetcher` / `KittenDecompiler` 的 [`WorkDecompiler`] 实现;
//! - 编译版积木树 → 编辑版重建(`decompile_actor_blocks`,`block_data_json`);
//! - [`XmlBlockWriter`]:编译版积木树 → Kitten2/3 的 `blocksXML`(与反编译重建共用
//!   [`crate::core::convert::decompile::blocks::child_input_name`] / `referenced_ids`)。

use crate::core::convert::decompile::{
    DecompileResult, DecompilerContext, WorkDecompiler,
    blocks::{BlockContext, child_input_name, create_block_decompiler, referenced_ids},
    save_json_result,
};
use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, EditorType, HttpClient, IdGenerator, RawWorkData, Result,
    ResultExt, ShadowBuilder, ValueExt, WorkFetcher, WorkInfo,
};
use log::warn;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// KITTEN
pub(crate) struct KittenFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl KittenFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for KittenFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let url = format!(
            "{}/kitten/r2/work/player/load/{}",
            self.config.creation_base_url, work_info.id
        );
        let data = self.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;
        let compiled = self.http_client.get_json(compiled_url, None)?;
        Ok(RawWorkData::Kitten(Arc::new(compiled)))
    }
}

impl WorkDecompiler for KittenDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let work_arc = match raw {
            RawWorkData::Kitten(data) => data,
            _ => {
                return Err(DecompilerError::Decompile(
                    "KittenDecompiler 只能处理 Kitten 数据".into(),
                ));
            }
        };

        // 提取需要恢复的全局字段,仅克隆这 13 个字段而非整份作品 JSON
        // 反编译会重写 work 的 theatre 与各角色积木,但 variables/lists/broadcasts 等
        // 顶层全局字段必须保留原值,故先在 try_unwrap 之前借 work_arc 读出
        // (try_unwrap 会消耗 work_arc,若先解包则无法再借用原始数据)
        // 作品 JSON 的主体是各角色的 blocks/block_data_json,恢复逻辑用不到,
        // 只克隆这些字段可避免数十 MB 的整份深拷贝
        let mut restore_fields: HashMap<&'static str, Value> = HashMap::new();
        let mut restore_groups: Option<Value> = None;
        {
            let original = work_arc.as_ref();
            for key in [
                "variables",
                "lists",
                "broadcasts",
                "audio",
                "matrix",
                "models",
                "physics2",
                "cloud_variable",
                "cloud_list",
                "ai_lab",
                "camera",
                "video",
                "midimusic",
            ] {
                if let Some(val) = original.get(key) {
                    restore_fields.insert(key, val.clone());
                }
            }
            restore_groups = original
                .get("theatre")
                .and_then(|t| t.get("groups"))
                .cloned();
        }
        let mut work = Arc::try_unwrap(work_arc).unwrap_or_else(|arc| (*arc).clone());

        // 编译产物数组是反编译的输入,但最终会被 clean_work_data 从输出中删除,
        // 因此直接 take 移出获得所有权(零拷贝),既作为只读输入又避免整数组深拷贝
        let compile_result = work
            .get_mut("compile_result")
            .and_then(|v| v.as_array_mut())
            .map(std::mem::take)
            .ok_or_else(|| DecompilerError::InvalidResponse("compile_result不存在".to_string()))?;

        // 从全局 variables 构建 UUID -> 变量名映射
        let mut global_variable_map = HashMap::new();
        if let Some(vars) = work.get("variables").and_then(|v| v.as_object()) {
            for (uuid, var_info) in vars {
                if let Some(name) = var_info.get("name").and_then(|v| v.as_str()) {
                    global_variable_map.insert(uuid.clone(), name.to_string());
                }
            }
        }
        // 所有角色/场景共享同一份映射,避免每角色深拷贝
        let global_variable_map = Arc::new(global_variable_map);

        let work_type = context.work_info.work_type;
        // Kitten2/3 编辑版用 blocksXML(Blockly XML 字符串),Kitten4 用 block_data_json
        let use_blocks_xml = matches!(work_type, EditorType::Kitten2 | EditorType::Kitten3);

        // 全局函数表:过程可在一个角色(如 Function)中定义,被其它角色调用,
        // 因此合并所有 compile_result 的 procedures,否则跨角色调用会被禁用
        let mut global_functions: HashMap<String, Value> = HashMap::new();
        for actor_compiled in &compile_result {
            if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
                for (name, func_data) in procedures {
                    global_functions.insert(name.clone(), func_data.clone());
                }
            }
        }
        // 所有角色/场景共享同一份函数表,避免每角色深拷贝
        let global_functions = Arc::new(global_functions);

        // 将 scenes 整表移出 work,处理完再写回
        // 场景反编译需要同时持有 scene 数据(只读)与 theatre 引用(写回),
        // 直接借用会造成 work 的不可变/可变借用冲突,逐场景克隆则浪费整份深拷贝
        // 移出后 scenes 与 work 相互独立,读写无冲突,且 is_scene 判断也复用同一份表
        let had_scenes = work
            .get("theatre")
            .and_then(|t| t.get("scenes"))
            .and_then(|s| s.as_object())
            .is_some();
        let mut scenes = work
            .get_mut("theatre")
            .and_then(|t| t.get_mut("scenes"))
            .and_then(|s| s.as_object_mut())
            .map(std::mem::take)
            .unwrap_or_default();

        for actor_compiled in &compile_result {
            let actor_id = actor_compiled.get_str_or("id", "");

            let is_scene = scenes.contains_key(actor_id);

            if is_scene {
                if use_blocks_xml {
                    // Kitten2/3:生成 blocksXML 字符串
                    let xml = XmlBlockWriter::new(context.config.as_ref())
                        .write_blocks(actor_compiled)
                        .with_context(|| format!("反编译场景 {} 失败", actor_id))?;
                    if let Some(scene) = scenes.get_mut(actor_id).and_then(|v| v.as_object_mut()) {
                        scene.insert("blocksXML".to_string(), Value::String(xml));
                    }
                } else {
                    // remove 移出所有权:场景整表已在循环前从 work 取出(mem::take),
                    // 直接转移所有权避免整场景深克隆
                    let scene_info = scenes.remove(actor_id).ok_or_else(|| {
                        DecompilerError::InvalidResponse(format!("场景 {} 不存在", actor_id))
                    })?;
                    let updated_scene = Self::decompile_scene_blocks(
                        &context.config,
                        &context.id_generator,
                        actor_compiled,
                        scene_info,
                        work_type,
                        &global_functions,
                    )
                    .with_context(|| format!("反编译场景 {} 失败", actor_id))?;
                    scenes.insert(actor_id.to_string(), updated_scene);
                }
            } else {
                if use_blocks_xml {
                    // Kitten2/3:生成 blocksXML 字符串
                    let xml = XmlBlockWriter::new(context.config.as_ref())
                        .write_blocks(actor_compiled)
                        .with_context(|| format!("反编译角色 {} 失败", actor_id))?;
                    if let Some(actors) = work
                        .get_mut("theatre")
                        .and_then(|t| t.get_mut("actors"))
                        .and_then(|a| a.as_object_mut())
                        && let Some(actor) =
                            actors.get_mut(actor_id).and_then(|v| v.as_object_mut())
                    {
                        actor.insert("blocksXML".to_string(), Value::String(xml));
                    }
                } else {
                    let actor_info = Self::take_actor_info(&mut work, actor_id);
                    // 角色也使用全局变量映射
                    let updated_actor = Self::decompile_actor_blocks(
                        &context.config,
                        &context.id_generator,
                        actor_compiled,
                        &global_functions,
                        actor_info,
                        Arc::clone(&global_variable_map),
                        work_type,
                    )
                    .with_context(|| format!("反编译角色 {} 失败", actor_id))?;

                    if let Some(actors) = work
                        .get_mut("theatre")
                        .and_then(|t| t.get_mut("actors"))
                        .and_then(|a| a.as_object_mut())
                    {
                        actors.insert(actor_id.to_string(), updated_actor);
                    }
                }
            }
        }

        // 写回处理后的 scenes
        if had_scenes && let Some(theatre) = work.get_mut("theatre").and_then(|t| t.as_object_mut())
        {
            theatre.insert("scenes".to_string(), Value::Object(scenes));
        }

        Self::update_work_info(&mut work, &context.work_info, context.config.as_ref())?;
        Self::clean_work_data(&mut work, work_type)?;
        Self::restore_global_fields(&mut work, &restore_fields, restore_groups.as_ref());

        Ok(DecompileResult::Json(work))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        let extension = context
            .work_info
            .file_extension(&context.config)
            .trim_start_matches('.')
            .to_owned();
        save_json_result(result, output_dir, context, &extension, "KITTEN")
    }
}

// ===========================================================================
// 编辑版重建(原 kitten/decompiler.rs)
// ===========================================================================

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

    /// 反编译根块(未被引用的块)并插入 context(角色/场景共享)
    fn decompile_root_blocks(
        blocks: &serde_json::Map<String, Value>,
        context: &mut BlockContext,
    ) -> Result<()> {
        let referenced_ids = referenced_ids(blocks)?;
        for (id, block_data) in blocks {
            if !referenced_ids.contains(id) {
                // 根块之间增加垂直间距,避免自动布局后挤在一起
                context.layout_row += 50.0;
                let mut decompiler = create_block_decompiler(block_data);
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
    fn decompile_procedures(actor_compiled: &Value, context: &mut BlockContext) -> Result<()> {
        // 函数可能定义在角色/场景(屏幕角色)中,被其它场景/角色调用;
        // 独立于 compiled_block_map,避免其缺失时连带丢失函数定义
        if let Some(procedures) = actor_compiled.get("procedures").and_then(|v| v.as_object()) {
            for (_, func_data) in procedures {
                context.layout_row += 50.0;
                let mut decompiler = create_block_decompiler(func_data);
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

        if let Some(blocks) = compiled_blocks {
            Self::decompile_root_blocks(blocks, &mut context)?;
        }

        // 生成函数定义块(procedures_2_defnoreturn),否则调用块会因找不到
        // 定义而被 FunctionCallDecompiler 置为 disabled,函数功能丢失
        // 独立于 compiled_block_map,避免其缺失时连带丢失函数定义
        Self::decompile_procedures(actor_compiled, &mut context)?;

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

        if let Some(blocks) = compiled_blocks {
            Self::decompile_root_blocks(blocks, &mut context)?;
        }

        // 生成函数定义块(procedures_2_defnoreturn).函数可能定义在场景
        // (屏幕角色)中(如"总移动设置4"定义在背景(3)),与角色分支一致,
        // 否则场景中定义的函数缺失,调用块会被 FunctionCallDecompiler 禁用
        Self::decompile_procedures(actor_compiled, &mut context)?;

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

// ===========================================================================
// blocksXML 序列化(原 kitten/xml.rs)
// ===========================================================================

// Kitten2/3 blocksXML 序列化器
/// Kitten2/3 编辑版(如春风得意)以 Blockly XML 字符串(blocksXML)存储积木
/// 与 Kitten4 的 block_data_json(blocks/connections)不同,本组件负责把编译块树
/// 序列化为 Blockly XML,独立成组件便于单独测试与复用
pub(crate) struct XmlBlockWriter<'a> {
    config: &'a DecompilerConfig,
}

impl<'a> XmlBlockWriter<'a> {
    pub(crate) fn new(config: &'a DecompilerConfig) -> Self {
        Self { config }
    }

    /// 生成 actor/场景的 blocksXML(`<variables></variables>` + 各根块)
    pub(crate) fn write_blocks(&self, actor_compiled: &Value) -> Result<String> {
        let mut xml = String::from("<variables></variables>");
        let compiled_blocks = actor_compiled
            .get("compiled_block_map")
            .and_then(|v| v.as_object());
        if let Some(blocks) = compiled_blocks {
            // 收集被引用的块 id,只将顶层根块作为独立 XML 块输出(与反编译重建同一实现)
            let referenced_ids = referenced_ids(blocks)?;
            let mut y = 0.0;
            for (id, block) in blocks {
                if !referenced_ids.contains(id) {
                    xml.push_str(&self.block_xml(block, true, y));
                    y += 220.0;
                }
            }
        }
        Ok(xml)
    }

    /// 将编译块树的单个块渲染为 Blockly XML
    fn block_xml(&self, compiled: &Value, is_root: bool, y: f64) -> String {
        let bt = compiled.get_str_or("type", "");
        let bid = compiled.get_str_or("id", "");
        let mut s = if is_root {
            format!(
                r#"<block type="{}" id="{}" inline="true" visible="visible" x="0" y="{}">"#,
                bt, bid, y
            )
        } else {
            format!(
                r#"<block type="{}" id="{}" inline="true" visible="visible">"#,
                bt, bid
            )
        };

        // fields:params 标量
        let mut field_xml = String::new();
        let mut value_xml = String::new();
        if let Some(params) = compiled.get_object_opt("params") {
            for (k, v) in params {
                if !v.is_object() && !v.is_array() {
                    let _ = write!(field_xml, r#"<field name="{}">"#, k);
                    Self::push_value_text_escaped(&mut field_xml, v);
                    field_xml.push_str("</field>");
                }
            }
            // value 插槽:params 对象
            for (k, v) in params {
                if v.is_object() {
                    let _ = write!(value_xml, r#"<value name="{}">"#, k);
                    value_xml.push_str(&self.value_xml(v));
                    value_xml.push_str("</value>");
                }
            }
        }
        s.push_str(&field_xml);
        // value 插槽先于 statement(编辑版如 self_listen 为 <value>...<statement>)
        s.push_str(&value_xml);

        // conditions → <value name="IF{i}">
        // 借用数组而非克隆:仅需迭代与长度
        let conditions = compiled.get("conditions").and_then(|v| v.as_array());
        let conditions_len = conditions.map_or(0, std::vec::Vec::len);
        if let Some(conditions) = conditions {
            for (i, c) in conditions.iter().enumerate() {
                if c.is_object() {
                    let _ = write!(s, r#"<value name="IF{}">"#, i);
                    s.push_str(&self.value_xml(c));
                    s.push_str("</value>");
                }
            }
        }

        // child_block → <statement name="...">
        if let Some(children) = compiled.get("child_block").and_then(|v| v.as_array()) {
            for (i, c) in children.iter().enumerate() {
                if !c.is_object() {
                    continue;
                }
                // 与反编译重建共用同一套插槽命名规则(blocks::child_input_name)
                let name = child_input_name(bt, i, conditions_len);
                let _ = write!(s, r#"<statement name="{}">"#, name);
                s.push_str(&self.block_xml(c, false, 0.0));
                s.push_str("</statement>");
            }
        }

        // next 链
        if let Some(nb) = compiled.get("next_block")
            && nb.is_object()
        {
            s.push_str("<next>");
            s.push_str(&self.block_xml(nb, false, 0.0));
            s.push_str("</next>");
        }

        s.push_str("</block>");
        s
    }

    /// value 插槽内容:shadow 类型渲染为 `<shadow>`,否则递归为 `<block>`
    fn value_xml(&self, v: &Value) -> String {
        let vt = v.get_str_or("type", "");
        let vid = v.get_str_or("id", "");
        if self.config.shadow_types.contains(vt) {
            let mut s = format!(r#"<shadow type="{}" id="{}" visible="visible">"#, vt, vid);
            if let Some(params) = v.get_object_opt("params") {
                for (k, fv) in params {
                    if !fv.is_object() && !fv.is_array() {
                        let _ = write!(s, r#"<field name="{}">"#, k);
                        Self::push_value_text_escaped(&mut s, fv);
                        s.push_str("</field>");
                    }
                }
            }
            s.push_str("</shadow>");
            s
        } else {
            self.block_xml(v, false, 0.0)
        }
    }

    /// XML 转义:单遍扫描,避免链式 replace 每次全量分配
    /// XML 转义后写入 `out`,避免为字符串字段构造中间 `String`。
    fn push_escaped(out: &mut String, s: &str) {
        if !s.contains(['&', '<', '>', '"', '\'']) {
            out.push_str(s);
            return;
        }
        for c in s.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&apos;"),
                _ => out.push(c),
            }
        }
    }

    /// 将 `Value` 按文本形式转义后写入 `out`;字符串直接借用,数字临时转字符串。
    fn push_value_text_escaped(out: &mut String, v: &Value) {
        match v {
            Value::String(s) => Self::push_escaped(out, s),
            Value::Number(n) => {
                let text = n.to_string();
                Self::push_escaped(out, &text);
            }
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            _ => {}
        }
    }
}
