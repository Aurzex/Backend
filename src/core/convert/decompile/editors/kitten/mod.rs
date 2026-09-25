use crate::core::convert::decompile::{
    DecompileResult, DecompilerContext, WorkDecompiler, save_json_result,
};
use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, EditorType, HttpClient, RawWorkData, Result, ResultExt,
    ValueExt, WorkFetcher, WorkInfo,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) mod decompiler;
pub(crate) mod xml;

pub(crate) use decompiler::KittenDecompiler;
pub(crate) use xml::XmlBlockWriter;

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
