use crate::api::auth::CloudAuthenticator;
use crate::core::convert::decompile::config::DecompilerConfig;
use crate::core::convert::decompile::shadow::ShadowBuilder;
use crate::core::convert::decompile::work::{RawWorkData, WorkFetcher, WorkInfo};
use crate::core::convert::decompile::{
    BlockContext, DecompileResult, DecompilerContext, WorkDecompiler, child_input_name,
    create_block_decompiler, referenced_ids, save_json_result,
};
use crate::core::convert::decompile::{
    EditableDocument, ResourceTask, download_resources_parallel, save_path_result,
};
use crate::core::convert::shared::{CryptoService, FileService, WorkId};
use crate::core::convert::shared::{
    DecompilerError, EditorType, HttpClient, IdGenerator, Result, ResultExt, ValueExt,
};
use log::info;
use log::warn;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// 来自 src/core/convert/decompile/editors/mod.rs

// 各编辑器(作品类型)的抓取器与反编译器
// **现状**:按类型切分的那批文件(`editors/{mod,kitten,nemo,simple,coco…}.rs`)已并为**本文件**
// (见下面各段"来自 …"面包屑);编译版积木 → 编辑版的通用骨架在 `mod.rs`,与类型无关的公共
// 设施(配置/加解密/HTTP/文件/模型)在 `shared`。
//
// - COCO / NEKO / WOOD(原 `simple.rs`,三个轻量编辑器合一处)
//   - `NekoFetcher`: 取作品详情并下载加密内容(`RawWorkData::NekoEncrypted`)
//   - `NekoDecompiler`: BCMKN 解密 → JSON
// - NEMO(原 `nemo.rs`)
//   - `NemoFetcher` / `NemoDecompiler`: 解密与场景重建
//   - `NemoResourceManager`: 素材/封面/用户库落盘
//   - `WoodFetcher` / `WoodDecompiler`: 解密与作品重建
//   - `CocoFetcher` / `CocoDecompiler`: 取源文件并重建场景/角色
// - Kitten(Kitten2/3/4)(原 `kitten.rs`)
//   - `KittenFetcher` 与 `KittenDecompiler` 的 `WorkDecompiler` 实现
//   - 编译版积木树 → 编辑版重建(角色/场景/全局字段)
//   - `XmlBlockWriter`: 编译版积木树 → blocksXML 序列化(Kitten2/3 编辑版)
//
// 门面(`decompile/mod.rs`)只经本文件取用下列十个名字,内部结构可自由调整:
// `CocoDecompiler` / `CocoFetcher`、`KittenDecompiler` / `KittenFetcher`、
// `NekoDecompiler` / `NekoFetcher`、`NemoDecompiler` / `NemoFetcher`、
// `WoodDecompiler` / `WoodFetcher`。

/// Kitten/Coco/Neko 三个 JSON 编辑器共享的 `save_result` 实现(仅编辑器名字面量不同)。
fn save_json_result_for_editor(
    result: &DecompileResult,
    output_dir: Option<&Path>,
    context: &DecompilerContext,
    editor: &str,
) -> Result<PathBuf> {
    // 扩展名与其它编辑器同一来源(与 coco/kitten 一致,不再硬编码)
    let extension = context
        .work_info
        .file_extension(&context.config)
        .trim_start_matches('.')
        .to_owned();
    save_json_result(result, output_dir, context, &extension, editor)
}

// `decompile` 入口的变体校验:取出目标变体,错变体时的错误文案与原各反编译器逐字一致。
impl RawWorkData {
    fn expect_kitten(self) -> Result<Arc<Value>> {
        match self {
            RawWorkData::Kitten(data) => Ok(data),
            _ => Err(DecompilerError::Decompile(
                "KittenDecompiler 只能处理 Kitten 数据".into(),
            )),
        }
    }

    fn expect_nemo(self) -> Result<(Arc<Value>, Arc<Value>)> {
        match self {
            RawWorkData::Nemo(bcm, source_info) => Ok((bcm, source_info)),
            _ => Err(DecompilerError::Decompile(
                "NemoDecompiler 需要 Nemo 数据".into(),
            )),
        }
    }

    fn expect_coco(self) -> Result<Arc<Value>> {
        match self {
            RawWorkData::Coco(data) => Ok(data),
            _ => Err(DecompilerError::Decompile(
                "CocoDecompiler 需要 Coco 数据".into(),
            )),
        }
    }

    fn expect_neko_encrypted(self) -> Result<String> {
        match self {
            RawWorkData::NekoEncrypted(encrypted) => Ok(encrypted),
            _ => Err(DecompilerError::Decompile(
                "NekoDecompiler 需要 NekoEncrypted 数据".into(),
            )),
        }
    }

    fn expect_wood(self) -> Result<Arc<Value>> {
        match self {
            RawWorkData::Wood(data) => Ok(data),
            _ => Err(DecompilerError::Decompile(
                "WoodDecompiler 需要 Wood 数据".into(),
            )),
        }
    }
}

/// 五个 Fetcher 共用的持有结构:HTTP 客户端 + 反编译配置。
struct HttpFetchCtx {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl HttpFetchCtx {
    fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

// 来自 src/core/convert/decompile/editors/kitten.rs
// Kitten(Kitten2/3/4)抓取与反编译。
// 三件事同一文件(原 `kitten/{mod,decompiler,xml}.rs`):
// - `KittenFetcher` / `KittenDecompiler` 的 [`WorkDecompiler`] 实现;
// - 编译版积木树 → 编辑版重建(`decompile_actor_blocks`,`block_data_json`);
// - [`XmlBlockWriter`]:编译版积木树 → Kitten2/3 的 `blocksXML`(与反编译重建共用
// [`crate::core::convert::decompile::child_input_name`] / `referenced_ids`)。

// KITTEN
pub(crate) struct KittenFetcher {
    ctx: HttpFetchCtx,
}

impl KittenFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            ctx: HttpFetchCtx::new(http_client, config),
        }
    }
}

impl WorkFetcher for KittenFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let url = format!(
            "{}/kitten/r2/work/player/load/{}",
            self.ctx.config.creation_base_url, work_info.id
        );
        let data = self.ctx.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;
        let compiled = self.ctx.http_client.get_json(compiled_url, None)?;
        Ok(RawWorkData::Kitten(Arc::new(compiled)))
    }
}

impl WorkDecompiler for KittenDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let work_arc = raw.expect_kitten()?;

        // 提取需要恢复的全局字段,仅克隆这 13 个字段而非整份作品 JSON
        // 反编译会重写 work 的 theatre 与各角色积木,但 variables/lists/broadcasts 等
        // 顶层全局字段必须保留原值,故先在 try_unwrap 之前借 work_arc 读出
        // (try_unwrap 会消耗 work_arc,若先解包则无法再借用原始数据)
        // 作品 JSON 的主体是各角色的 blocks/block_data_json,恢复逻辑用不到,
        // 只克隆这些字段可避免数十 MB 的整份深拷贝
        let mut restore_fields: HashMap<&'static str, Value> = HashMap::new();
        let restore_groups: Option<Value>;
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
        save_json_result_for_editor(result, output_dir, context, "KITTEN")
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

// 来自 src/core/convert/decompile/editors/nemo.rs

// NEMO
/// NEMO / WOOD 两个资源管理器共用的配置(字段逐项相同)。
pub(crate) struct ResourceConfig<'a> {
    pub(crate) http_client: &'a dyn HttpClient,
    pub(crate) work_id: WorkId,
    /// 资源下载并发数(见 [`DecompileOptions::resource_concurrency`])
    pub(crate) resource_concurrency: usize,
    /// 是否下载资源文件(见 [`DecompileOptions::skip_resources`])
    pub(crate) download_resources: bool,
}

pub(crate) struct NemoFetcher {
    ctx: HttpFetchCtx,
}

impl NemoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            ctx: HttpFetchCtx::new(http_client, config),
        }
    }
}

impl WorkFetcher for NemoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let source_url = format!(
            "{}/creation-tools/v1/works/{}/source/public",
            self.ctx.config.base_url, work_info.id
        );
        let source_info = self.ctx.http_client.get_json(&source_url, None)?;

        let bcm_url = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取work_urls".to_string()))?;

        let bcm_data = self.ctx.http_client.get_json(bcm_url, None)?;
        Ok(RawWorkData::Nemo(Arc::new(bcm_data), Arc::new(source_info)))
    }
}

pub(crate) struct NemoDecompiler;

impl NemoDecompiler {
    fn decompile_inner(
        context: &DecompilerContext,
        bcm_data: Arc<Value>,
        source_info: Arc<Value>,
    ) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id.get(), "");
        // 与 `save_result` 同一落点:调用方指定了就写它,否则回退默认目录
        let base_dir = context
            .output_dir
            .as_deref()
            .unwrap_or(&context.config.default_output_dir);
        let work_dir = base_dir.join(folder_name);

        let resource_config = ResourceConfig {
            http_client: &*context.http_client,
            work_id,
            resource_concurrency: context.resource_concurrency,
            download_resources: context.download_resources,
        };
        let mut resource_manager = NemoResourceManager::new(resource_config, work_dir.clone());

        resource_manager.create_directories()?;
        resource_manager.save_core_files(&bcm_data, &source_info)?;
        resource_manager.download_resources(&bcm_data)?;

        info!("NEMO作品解密成功!");
        info!("将反编译的文件复制到: /data/data/com.codemao.nemo/files/nemo_users_db");

        Ok(work_dir.to_string_lossy().to_string())
    }
}

impl WorkDecompiler for NemoDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let (bcm, src) = raw.expect_nemo()?;
        let path = Self::decompile_inner(context, bcm, src)?;
        Ok(DecompileResult::Path(path))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        _output_dir: Option<&Path>,
        _context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_path_result(result, "NEMO")
    }

    /// 内存形态的编辑版:NEMO 的 `.bcm` 就是明文 JSON(**没有解密步骤**,评审 §8 更正),
    /// 所以直接把取到的文档给出去即可 —— 不落盘、不下资源(`translate` 只需要这份文档)。
    ///
    /// 源版本的唯一来源是作品元信息(`source_info.bcm_version`),编辑版文档里只有 `app_version`。
    fn editable_document(
        &self,
        raw: &RawWorkData,
        _context: &DecompilerContext,
    ) -> Result<Option<EditableDocument>> {
        match raw {
            RawWorkData::Nemo(bcm, source_info) => Ok(Some(EditableDocument {
                document: (**bcm).clone(),
                source_version: source_info.get_str_or("bcm_version", "").to_string(),
            })),
            _ => Err(DecompilerError::Decompile(
                "NemoDecompiler 需要 Nemo 数据".into(),
            )),
        }
    }
}

pub(crate) struct NemoResourceManager<'a> {
    config: ResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
    sha_cache: RefCell<HashMap<String, String>>,
}

impl<'a> NemoResourceManager<'a> {
    pub(crate) fn new(config: ResourceConfig<'a>, work_dir: PathBuf) -> Self {
        Self {
            config,
            work_dir,
            dirs: HashMap::new(),
            sha_cache: RefCell::new(HashMap::new()),
        }
    }

    fn get_sha(&self, url: &str) -> String {
        let mut cache = self.sha_cache.borrow_mut();
        cache
            .entry(url.to_owned())
            .or_insert_with(|| CryptoService::sha256(url))
            .clone()
    }

    pub(crate) fn create_directories(&mut self) -> Result<&HashMap<String, PathBuf>> {
        self.dirs.insert(
            "material".to_string(),
            FileService::ensure_dir(&self.work_dir.join("user_material"))?,
        );
        self.dirs.insert(
            "works".to_string(),
            FileService::ensure_dir(
                &self
                    .work_dir
                    .join("user_works")
                    .join(self.config.work_id.to_string()),
            )?,
        );
        self.dirs.insert(
            "record".to_string(),
            FileService::ensure_dir(
                &self
                    .work_dir
                    .join("user_works")
                    .join(self.config.work_id.to_string())
                    .join("record"),
            )?,
        );
        Ok(&self.dirs)
    }

    pub(crate) fn save_core_files(&self, bcm_data: &Value, source_info: &Value) -> Result<()> {
        let works_dir = self
            .dirs
            .get("works")
            .ok_or_else(|| DecompilerError::Other {
                msg: "works目录不存在".to_string(),
                source: None,
            })?;

        let bcm_path = works_dir.join(format!("{}.bcm", self.config.work_id));
        FileService::write_json(&bcm_path, bcm_data)?;

        let user_images = self.build_user_images(bcm_data)?;
        let userimg_path = works_dir.join(format!("{}.userimg", self.config.work_id));
        FileService::write_json(&userimg_path, &user_images)?;

        let meta_data = self.build_metadata(source_info)?;
        let meta_path = works_dir.join(format!("{}.meta", self.config.work_id));
        FileService::write_json(&meta_path, &meta_data)?;

        if let Some(preview) = source_info.get("preview").and_then(|v| v.as_str())
            && !preview.is_empty()
        {
            match self.config.http_client.get_binary(preview) {
                Ok(cover_data) => {
                    let cover_path = works_dir.join(format!("{}.cover", self.config.work_id));
                    FileService::write_binary(&cover_path, &cover_data)?;
                }
                Err(e) => warn!("封面下载失败: {}", e),
            }
        }

        Ok(())
    }

    fn build_user_images(&self, bcm_data: &Value) -> Result<Value> {
        let mut user_images = serde_json::Map::new();
        let mut img_dict = serde_json::Map::new();

        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for (style_id, style_data) in styles {
                if let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) {
                    let sha_hash = self.get_sha(image_url);
                    let mut style_info = serde_json::Map::new();
                    style_info.insert("id".to_string(), Value::String(style_id.clone()));
                    style_info.insert(
                        "path".to_string(),
                        Value::String(format!("user_material/{}.webp", sha_hash)),
                    );
                    img_dict.insert(style_id.clone(), Value::Object(style_info));
                }
            }
        }

        user_images.insert("user_img_dict".to_string(), Value::Object(img_dict));
        Ok(Value::Object(user_images))
    }

    fn build_metadata(&self, source_info: &Value) -> Result<Value> {
        let work_name = source_info.get_str_or("name", "");
        let work_urls = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let bcm_version = source_info.get_str_or("bcm_version", "");
        let preview = source_info.get_str_or("preview", "");

        Ok(json!({
            "bcm_count": {
                "block_cnt_without_invisible": 0.0,
                "block_cnt": 0.0,
                "entity_cnt": 1.0,
            },
            "bcm_name": work_name,
            "bcm_url": work_urls,
            "bcm_version": bcm_version,
            "download_fail": false,
            "extra_data": {},
            "have_published_status": false,
            "have_remote_resources": false,
            "is_landscape": false,
            "is_micro_bit": false,
            "is_valid": false,
            "mcloud_variable": [],
            "publish_preview": preview,
            "publish_status": 0,
            "review_state": 0,
            "template_id": 0,
            "term_id": 0,
            "type": 0,
            "upload_status": {
                "work_id": self.config.work_id.get(),
                "have_uploaded": 2,
            },
        }))
    }

    pub(crate) fn download_resources(&self, bcm_data: &Value) -> Result<()> {
        if !self.config.download_resources {
            info!("已按 skip_resources 跳过 NEMO 资源下载");
            return Ok(());
        }
        let material_dir = self
            .dirs
            .get("material")
            .ok_or_else(|| DecompilerError::Other {
                msg: "material目录不存在".to_string(),
                source: None,
            })?;

        // 收集任务:同一 url 只下一次(内容寻址文件名 = sha256(url),与 `.userimg` 里的 path 对齐)
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut tasks = Vec::new();
        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for style_data in styles.values() {
                let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) else {
                    continue;
                };
                if !seen.insert(image_url) {
                    continue;
                }
                tasks.push(ResourceTask {
                    url: image_url.to_string(),
                    dest: material_dir.join(format!("{}.webp", self.get_sha(image_url))),
                });
            }
        }
        for (url, error) in download_resources_parallel(
            self.config.http_client,
            tasks,
            self.config.resource_concurrency,
        ) {
            warn!("资源下载失败 {url}: {error}");
        }
        Ok(())
    }
}

// 来自 src/core/convert/decompile/editors/simple.rs
// 轻量编辑器的抓取与反编译:COCO、NEKO、WOOD。
// 三者都是「单文件抓取 + 解密/重建 + 落盘」,没有 Kitten 那种 blocksXML/积木树重建
// 的复杂度,故合在一处(原 `coco.rs` / `neko.rs` / `wood.rs`);
// NEMO 因为要重组资源目录、另有素材管理器,单独成文件。

// COCO
pub(crate) struct CocoFetcher {
    ctx: HttpFetchCtx,
}

impl CocoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            ctx: HttpFetchCtx::new(http_client, config),
        }
    }
}

impl WorkFetcher for CocoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let url = format!(
            "{}/coconut/web/work/{}/load",
            self.ctx.config.creation_base_url, work_info.id
        );
        let data = self.ctx.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("data")
            .and_then(|v| v.get("bcmc_url"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取bcmc_url".to_string()))?;
        let compiled = self.ctx.http_client.get_json(compiled_url, None)?;
        Ok(RawWorkData::Coco(Arc::new(compiled)))
    }
}

pub(crate) struct CocoDecompiler;

impl CocoDecompiler {
    fn reorganize(work: &mut Value, context: &DecompilerContext) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;

        let mut widget_map = work_obj
            .remove("widgetMap")
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        let screen_list = work_obj
            .remove("screenList")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();

        work_obj.insert("authorId".to_string(), json!(context.work_info.user_id));
        work_obj.insert("title".to_string(), json!(context.work_info.name));
        // screens/screenIds 在下方由真实数据插入,无需先放空占位

        let mut screens = serde_json::Map::new();
        let mut screen_ids = Vec::with_capacity(screen_list.len());

        for screen in screen_list {
            // 直接解构出 Map 所有权,循环末尾整体移入 screens,避免整屏深克隆
            let mut screen_obj = match screen {
                Value::Object(map) => map,
                _ => {
                    return Err(DecompilerError::Decompile("screen不是对象".to_string()));
                }
            };
            let screen_id = screen_obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DecompilerError::InvalidResponse("screen缺少id".to_string()))?
                .to_string();
            screen_obj.insert("snapshot".to_string(), json!(""));
            screen_obj.insert("primitiveVariables".to_string(), json!([]));
            screen_obj.insert("arrayVariables".to_string(), json!([]));
            screen_obj.insert("objectVariables".to_string(), json!([]));
            screen_obj.insert("broadcasts".to_string(), json!(["Hi"]));
            screen_obj.insert("widgets".to_string(), json!({}));

            let widget_ids = screen_obj
                .get("widgetIds")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let invisible_widget_ids = screen_obj
                .get("invisibleWidgetIds")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut screen_widgets = serde_json::Map::new();
            let mut missing_ids = Vec::new();
            for wid in widget_ids.iter().chain(invisible_widget_ids.iter()) {
                if let Some(id) = wid.as_str() {
                    if let Some(widget) = widget_map.as_object_mut().and_then(|map| map.remove(id))
                    {
                        screen_widgets.insert(id.to_string(), widget);
                    } else {
                        warn!(
                            "屏幕 {} 中引用的部件 {} 在 widgetMap 中缺失,已保留在全局池",
                            screen_id, id
                        );
                        missing_ids.push(Value::String(id.to_string()));
                    }
                }
            }
            if !missing_ids.is_empty() {
                screen_obj.insert("missing_widget_ids".to_string(), Value::Array(missing_ids));
            }
            screen_obj.insert("widgets".to_string(), Value::Object(screen_widgets));
            screen_ids.push(Value::String(screen_id.clone()));
            screens.insert(screen_id, Value::Object(screen_obj));
        }

        work_obj.insert("screens".to_string(), Value::Object(screens));
        work_obj.insert("screenIds".to_string(), Value::Array(screen_ids));
        work_obj.insert("widgetMap".to_string(), widget_map);

        if let Some(block_json_map) = work_obj.get("blockJsonMap").and_then(|v| v.as_object()) {
            let mut blockly = serde_json::Map::new();
            for (screen_id, blocks) in block_json_map {
                blockly.insert(
                    screen_id.clone(),
                    json!({
                        "screenId": screen_id,
                        "workspaceJson": blocks,
                        "workspaceOffset": {"x": 0, "y": 0}
                    }),
                );
            }
            work_obj.insert("blockly".to_string(), Value::Object(blockly));
        }

        for (map_name, list_name) in &[
            ("imageFileMap", "imageFileList"),
            ("soundFileMap", "soundFileList"),
            ("iconFileMap", "iconFileList"),
            ("fontFileMap", "fontFileList"),
        ] {
            if let Some(map) = work_obj.get(*map_name).and_then(|v| v.as_object()) {
                let values: Vec<Value> = map.values().cloned().collect();
                work_obj.insert(list_name.to_string(), Value::Array(values));
            }
        }

        if let Some(variable_map) = work_obj.get("variableMap").and_then(|v| v.as_object()) {
            let mut var_list = Vec::new();
            let mut list_list = Vec::new();
            let mut dict_list = Vec::new();
            for (var_id, value) in variable_map {
                if value.is_array() {
                    list_list.push(json!({"id": var_id, "name": format!("列表{}", list_list.len()+1), "defaultValue": value, "value": value}));
                } else if value.is_object() {
                    dict_list.push(json!({"id": var_id, "name": format!("字典{}", dict_list.len()+1), "defaultValue": value, "value": value}));
                } else {
                    var_list.push(json!({"id": var_id, "name": format!("变量{}", var_list.len()+1), "defaultValue": value, "value": value}));
                }
            }
            work_obj.insert("globalVariableList".to_string(), json!(var_list));
            work_obj.insert("globalArrayList".to_string(), json!(list_list));
            work_obj.insert("globalObjectList".to_string(), json!(dict_list));
        }

        if let Some(widget_map) = work_obj.get("widgetMap").cloned() {
            work_obj.insert("globalWidgets".to_string(), widget_map);
        } else {
            work_obj.insert("globalWidgets".to_string(), json!({}));
        }
        if let Some(widget_map) = work_obj.get("widgetMap").and_then(|v| v.as_object()) {
            let widget_ids: Vec<String> = widget_map.keys().cloned().collect();
            work_obj.insert("globalWidgetIds".to_string(), json!(widget_ids));
        } else {
            work_obj.insert("globalWidgetIds".to_string(), json!([]));
        }
        work_obj.insert("sourceTag".to_string(), json!(1));
        work_obj.insert("sourceId".to_string(), json!(""));

        for key in &[
            "apiToken",
            "blockCode",
            "blockJsonMap",
            "fontFileMap",
            "gridMap",
            "iconFileMap",
            "id",
            "imageFileMap",
            "initialScreenId",
            "screenList",
            "soundFileMap",
            "variableMap",
            "widgetMap",
        ] {
            work_obj.remove(*key);
        }
        Ok(())
    }
}

impl WorkDecompiler for CocoDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let mut work = (*raw.expect_coco()?).clone();
        Self::reorganize(&mut work, context)?;
        Ok(DecompileResult::Json(work))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_json_result_for_editor(result, output_dir, context, "COCO")
    }
}

// ===========================================================================
// NEKO(原 neko.rs)
// ===========================================================================

// NEKO
pub(crate) struct NekoFetcher {
    ctx: HttpFetchCtx,
}

impl NekoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            ctx: HttpFetchCtx::new(http_client, config),
        }
    }
}

impl WorkFetcher for NekoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let detail_url = format!(
            "{}/neko/community/player/published-work-detail/{}",
            self.ctx.config.creation_base_url, work_info.id
        );

        let mut auth = CloudAuthenticator::new(None);
        let device_auth = auth
            .generate_x_device_auth()
            .map_err(|e| DecompilerError::Other {
                msg: format!("生成设备认证失败: {}", e),
                source: Some(Box::new(DecompilerError::Other {
                    msg: e.to_string(),
                    source: None,
                })),
            })?;

        // 修复:generate_x_device_auth 已返回 JSON 字符串({"sign":...,"timestamp":...,"client_id":...}),
        // 直接作为 header 值.此前二次 serde_json::to_string 会再包一层引号转义,
        // 服务器解析 device-auth 失败返回 500 "Not a JSON Object",导致 NEKO 作品无法获取原始数据
        let headers: Vec<(String, String)> =
            vec![("x-creation-tools-device-auth".to_string(), device_auth)];

        let detail = self.ctx.http_client.get_json(&detail_url, Some(headers))?;

        let encrypted_url = detail
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;

        let encrypted_content = self.ctx.http_client.get_text(encrypted_url)?;
        Ok(RawWorkData::NekoEncrypted(encrypted_content))
    }
}

pub(crate) struct NekoDecompiler {
    crypto_service: CryptoService,
}

impl NekoDecompiler {
    pub(crate) fn new(salt: &[u8]) -> Self {
        Self {
            crypto_service: CryptoService::new(salt),
        }
    }
}

impl WorkDecompiler for NekoDecompiler {
    fn decompile(&self, raw: RawWorkData, _context: &DecompilerContext) -> Result<DecompileResult> {
        let encrypted = raw.expect_neko_encrypted()?;
        // `CryptoService` 内部只有 `Arc<[u8]>` salt,直接借用即可,无需克隆
        let decrypted_json = self.crypto_service.decrypt_bcmkn_json(&encrypted)?;
        Ok(DecompileResult::Json(decrypted_json))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_json_result_for_editor(result, output_dir, context, "NEKO")
    }
}

// ===========================================================================
// WOOD(原 wood.rs)
// ===========================================================================

// WOOD
pub(crate) struct WoodFetcher {
    ctx: HttpFetchCtx,
}

impl WoodFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            ctx: HttpFetchCtx::new(http_client, config),
        }
    }
}

impl WorkFetcher for WoodFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let publish_url = format!(
            "{}/wood/work/{}/publish?channel_type=0",
            self.ctx.config.creation_base_url, work_info.id
        );
        let data = self.ctx.http_client.get_json(&publish_url, None)?;
        Ok(RawWorkData::Wood(Arc::new(data)))
    }
}

pub(crate) struct WoodDecompiler;

impl WoodDecompiler {
    fn decompile_inner(context: &DecompilerContext, work_data: Arc<Value>) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id.get(), "");
        // 与 `save_result` 同一落点:调用方指定了就写它,否则回退默认目录
        let base_dir = context
            .output_dir
            .as_deref()
            .unwrap_or(&context.config.default_output_dir);
        let work_dir = base_dir.join(folder_name);

        let resource_config = ResourceConfig {
            http_client: &*context.http_client,
            work_id,
            resource_concurrency: context.resource_concurrency,
            download_resources: context.download_resources,
        };
        let mut resource_manager = WoodResourceManager::new(resource_config, work_dir.clone());

        resource_manager.create_directories()?;
        resource_manager.save_work_files(&work_data)?;
        Ok(work_dir.to_string_lossy().to_string())
    }
}

impl WorkDecompiler for WoodDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let data = raw.expect_wood()?;
        let path = Self::decompile_inner(context, data)?;
        Ok(DecompileResult::Path(path))
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        _output_dir: Option<&Path>,
        _context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_path_result(result, "WOOD")
    }
}

pub(crate) struct WoodResourceManager<'a> {
    config: ResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
}

impl<'a> WoodResourceManager<'a> {
    pub(crate) fn new(config: ResourceConfig<'a>, work_dir: PathBuf) -> Self {
        Self {
            config,
            work_dir,
            dirs: HashMap::new(),
        }
    }

    pub(crate) fn create_directories(&mut self) -> Result<&HashMap<String, PathBuf>> {
        self.dirs
            .insert("root".to_string(), FileService::ensure_dir(&self.work_dir)?);
        self.dirs.insert(
            "images".to_string(),
            FileService::ensure_dir(&self.work_dir.join("images"))?,
        );
        Ok(&self.dirs)
    }

    pub(crate) fn save_work_files(&self, work_data: &Value) -> Result<()> {
        self.save_work_info(work_data)?;
        self.save_code_files(work_data)?;
        self.download_images(work_data)?;
        Ok(())
    }

    fn save_work_info(&self, work_data: &Value) -> Result<()> {
        let root_dir = self
            .dirs
            .get("root")
            .ok_or_else(|| DecompilerError::Other {
                msg: "root目录不存在".to_string(),
                source: None,
            })?;
        let info = json!({
            "id": work_data.get_i64_or_default("work_id", 0),
            "name": work_data.get_str_or("work_name", ""),
            "type": "WOOD",
            "language_type": work_data.get_i64_or_default("language_type", 3),
            "run_mode": work_data.get_i64_or_default("run_mode", 0),
            "code_visible": work_data.get("code_visible").and_then(serde_json::Value::as_bool).unwrap_or(true),
            "addition": work_data.get("addition").cloned().unwrap_or(json!({})),
        });
        FileService::write_json(&root_dir.join("work_info.json"), &info)
    }

    fn save_code_files(&self, work_data: &Value) -> Result<()> {
        let root_dir = self
            .dirs
            .get("root")
            .ok_or_else(|| DecompilerError::Other {
                msg: "root目录不存在".to_string(),
                source: None,
            })?;
        if let Some(content) = work_data.get("content").and_then(|v| v.as_array()) {
            for file_info in content {
                if file_info.get_i64_or_default("file_type", 0) == 2 {
                    let file_name = file_info.get_str_or("file_name", "");
                    if file_name.ends_with(".py")
                        && let Some(source) = file_info.get("source").and_then(|v| v.as_str())
                    {
                        std::fs::write(root_dir.join(file_name), source)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn extract_filename_from_url(&self, url: &str) -> String {
        if let Some(last_slash) = url.rfind('/') {
            let part = &url[last_slash + 1..];
            if let Some(q) = part.find('?') {
                return part[..q].to_string();
            }
            if let Some(h) = part.find('#') {
                return part[..h].to_string();
            }
            return part.to_string();
        }
        String::new()
    }

    fn download_images(&self, work_data: &Value) -> Result<()> {
        if !self.config.download_resources {
            info!("已按 skip_resources 跳过 WOOD 资源下载");
            return Ok(());
        }
        let images_dir = self
            .dirs
            .get("images")
            .ok_or_else(|| DecompilerError::Other {
                msg: "images目录不存在".to_string(),
                source: None,
            })?;
        // 收集任务:同一 url 只下一次;文件名按平台给的 `file_name`,缺失时从 URL 推
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut tasks = Vec::new();
        for file_info in work_data
            .get("content")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            if file_info.get_i64_or_default("file_type", 0) != 3 {
                continue;
            }
            let Some(image_url) = file_info.get("url").and_then(|v| v.as_str()) else {
                continue;
            };
            if !seen.insert(image_url) {
                continue;
            }
            let name = file_info.get_str_or("file_name", "");
            let name = if name.is_empty() {
                self.extract_filename_from_url(image_url)
            } else {
                name.to_string()
            };
            let name = if name.is_empty() {
                "image.png".to_string()
            } else {
                name
            };
            tasks.push(ResourceTask {
                url: image_url.to_string(),
                dest: images_dir.join(name),
            });
        }
        for (url, error) in download_resources_parallel(
            self.config.http_client,
            tasks,
            self.config.resource_concurrency,
        ) {
            warn!("图片下载失败 {url}: {error}");
        }
        Ok(())
    }
}

// ===========================================================================
// 离线单测(不联网、不落盘):blocksXML 序列化的产物字节
// ===========================================================================
#[cfg(test)]
mod xml_writer_tests {
    use super::*;
    use serde_json::json;

    /// 只把**未被引用**的块当根块输出;被引用者必须嵌在 `<next>` 里,不得再带 x/y。
    ///
    /// 守的是 blocksXML 的根块判定:同一个编译块树里,子块若被误判成根块,
    /// 就会被再输出一遍并带上 `x="0" y="220"`(编辑器里每个块都独立出现在画布上 ⇒ 散块)。
    /// 断言 `y="220"` **不出现**,正好锁住"引用块不能有第二条根输出"这条不变量;
    /// 只数 `<block type=` 个数是不够的(散块时计数同样是 2,只是多了一处 x/y)。
    #[test]
    fn write_blocks_emits_only_unreferenced_blocks_as_roots() {
        let config = DecompilerConfig::default();
        let writer = XmlBlockWriter::new(&config);
        // a 是根块,通过 next_block 引用 b;b 在块表里也有自己的条目
        let actor = json!({
            "compiled_block_map": {
                "a": {
                    "type": "controls_repeat",
                    "id": "a",
                    "params": {"TIMES": {"type": "math_number", "id": "m1", "params": {"NUM": 10}}},
                    "next_block": {"type": "data_setvariableto", "id": "b", "params": {"VALUE": "hi"}}
                },
                "b": {
                    "type": "data_setvariableto",
                    "id": "b",
                    "params": {"VALUE": "hi"}
                }
            }
        });
        let xml = writer.write_blocks(&actor).unwrap();

        assert!(xml.starts_with("<variables></variables>"));
        // 两个块各渲染一次:a 作根(+x/y),b 作 a 的 <next> 子块
        assert_eq!(
            xml.matches("<block type=").count(),
            2,
            "两个块应各渲染一次,实际:{xml}"
        );
        assert!(xml.contains(
            r#"<block type="controls_repeat" id="a" inline="true" visible="visible" x="0" y="0">"#
        ));
        assert!(xml.contains("<next>"), "next 链应嵌在 <next> 里:{xml}");
        // 关键不变量:被引用的 b 不得作为第二个根块再输出一遍(散块的症状)
        assert!(
            !xml.contains(r#"y="220""#),
            "被引用的块 b 被当成了根块(产物会出现散块):{xml}"
        );

        // 边界:`compiled_block_map` 缺失时不是错误,只输出表头
        assert_eq!(
            writer.write_blocks(&json!({})).unwrap(),
            "<variables></variables>"
        );
        // 边界:空块表同样只有表头
        assert_eq!(
            writer
                .write_blocks(&json!({"compiled_block_map": {}}))
                .unwrap(),
            "<variables></variables>"
        );
    }

    /// XML 字段文本必须**单遍转义**,不能"先替换 `<` 再替换 `&`"(那会把已生成的实体二次转义)。
    ///
    /// 守的是产物字节:一条含 `&<>"'` 的文本字段,块名/字符串里的这些字符若转义错误,
    /// blocksXML 会被编辑器判为非法 XML 或积木文本被改写(如 `&lt;` 变成 `&amp;lt;` 显示成 `&lt;`)。
    /// 断言的是**整串**具体文本,而不是"包含 &lt;"这种弱断言 —— 单遍实现的正确输出与
    /// 链式 replace 的错误输出只差 `&amp;` 这一处,只有全串比对能分开。
    #[test]
    fn write_blocks_escapes_field_text_and_leaves_empties_alone() {
        let config = DecompilerConfig::default();
        let writer = XmlBlockWriter::new(&config);
        let actor = json!({
            "compiled_block_map": {
                "root": {
                    "type": "data_setvariableto",
                    "id": "root",
                    "params": {
                        "TEXT": "a&b<c>d\"e'f",
                        "NUM": 3,
                        "FLAG": true,
                        "EMPTY": ""
                    }
                }
            }
        });
        let xml = writer.write_blocks(&actor).unwrap();

        // 单遍转义:& 先于 < 处理,结果里不会出现 &amp;lt;
        assert!(
            xml.contains(r#"<field name="TEXT">a&amp;b&lt;c&gt;d&quot;e&apos;f</field>"#),
            "文本字段转义不对(链式 replace 的二次转义症状是 &amp;lt;):{xml}"
        );
        // 数字/布尔按文本写,不加引号
        assert!(xml.contains(r#"<field name="NUM">3</field>"#));
        assert!(xml.contains(r#"<field name="FLAG">true</field>"#));
        // 空字符串字段仍要写出空 <field> 对(而不是整条丢掉)
        assert!(
            xml.contains(r#"<field name="EMPTY"></field>"#),
            "空字段被丢掉了:{xml}"
        );
    }
}
