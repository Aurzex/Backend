use crate::api::auth::CloudAuthenticator;
use crate::core::unpacker::{
    BCMKNDecryptor, BlockBehavior, BlockContext, BlockDecompilerBehavior, CryptoService,
    DecompileResult, DecompilerConfig, DecompilerContext, DecompilerError, FileService, HttpClient,
    IdGenerator, RawWorkData, Result, ResultExt, ShadowBuilder, ValueExt, WorkDecompiler,
    WorkFetcher, WorkId, WorkInfo, WorkType, save_json_result, save_path_result,
};
use crate::utils::filedata::PathConfig;
use crate::utils::requests::{CodeMaoClient, HttpMethod, MewError};
use aes_gcm::aead::array::Array;
use aes_gcm::aead::array::typenum::{U12, U32};
use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose};
use log::{error, info, warn};
use serde_json::{Value, json, to_string};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

// NEKO
pub(crate) struct NekoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NekoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for NekoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let detail_url = format!(
            "{}/neko/community/player/published-work-detail/{}",
            self.config.creation_base_url, work_info.id
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

        let detail = self.http_client.get_json(&detail_url, Some(headers))?;

        let encrypted_url = detail
            .get("source_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取source_urls".to_string()))?;

        let encrypted_content = self.http_client.get_text(encrypted_url)?;
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
        match raw {
            RawWorkData::NekoEncrypted(encrypted) => {
                let decryptor = BCMKNDecryptor::new(self.crypto_service.clone());
                let decrypted_json = decryptor.decrypt(&encrypted)?;
                Ok(DecompileResult::Json(decrypted_json))
            }
            _ => Err(DecompilerError::Decompile(
                "NekoDecompiler 需要 NekoEncrypted 数据".into(),
            )),
        }
    }

    fn save_result(
        &self,
        result: &DecompileResult,
        output_dir: Option<&Path>,
        context: &DecompilerContext,
    ) -> Result<PathBuf> {
        save_json_result(result, output_dir, context, "bcmkn", "NEKO")
    }
}

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

pub(crate) struct KittenDecompiler;

impl KittenDecompiler {
    /// 从 work 的 theatre 中移出角色信息(所有权转移,避免整角色深克隆);
    /// 缺失时回退为默认角色 JSON
    fn take_actor_info(work: &mut Value, actor_id: &str) -> Value {
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

    fn decompile_actor_blocks(
        config: &Arc<DecompilerConfig>,
        id_generator: &IdGenerator,
        actor_compiled: &Value,
        functions: &Arc<HashMap<String, Value>>,
        actor_info: Value,
        variable_map: Arc<HashMap<String, String>>,
        work_type: WorkType,
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

    fn decompile_scene_blocks(
        config: &Arc<DecompilerConfig>,
        id_generator: &IdGenerator,
        actor_compiled: &Value,
        scene_info: Value,
        work_type: WorkType,
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

    fn update_work_info(
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
        let is_k3 = matches!(work_info.work_type, WorkType::Kitten2 | WorkType::Kitten3);
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

    fn clean_work_data(work: &mut Value, work_type: WorkType) -> Result<()> {
        let work_obj = work
            .as_object_mut()
            .ok_or_else(|| DecompilerError::Decompile("work不是对象".to_string()))?;
        let keys_to_remove = ["compile_result", "preview", "author_nickname"];
        for key in &keys_to_remove {
            work_obj.remove(*key);
        }
        // 清理编译版 theatre 的运行时字段:Kitten4 编辑版无这些键,
        // 但 Kitten3 编辑版(如春风得意)保留 current_entity/current_scene/style_collections
        if !matches!(work_type, WorkType::Kitten2 | WorkType::Kitten3)
            && let Some(theatre) = work.get_mut("theatre").and_then(|t| t.as_object_mut())
        {
            for key in ["current_entity", "current_scene", "style_collections"] {
                theatre.remove(key);
            }
        }
        Ok(())
    }

    fn restore_global_fields(
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
            // 收集被引用的块 id,只将顶层根块作为独立 XML 块输出
            let mut referenced_ids: HashSet<String> = HashSet::new();
            for (_, block) in blocks {
                if let Some(next) = block.get("next_block")
                    && let Some(id) = next.get("id").and_then(|v| v.as_str())
                {
                    referenced_ids.insert(id.to_string());
                }
                if let Some(children) = block.get("child_block").and_then(|v| v.as_array()) {
                    for child in children {
                        if let Some(id) = child.get("id").and_then(|v| v.as_str()) {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(conds) = block.get("conditions").and_then(|v| v.as_array()) {
                    for c in conds {
                        if let Some(id) = c.get("id").and_then(|v| v.as_str()) {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
                if let Some(params) = block.get("params").and_then(|v| v.as_object()) {
                    for (_, pv) in params {
                        if let Some(id) = pv.get("id").and_then(|v| v.as_str()) {
                            referenced_ids.insert(id.to_string());
                        }
                    }
                }
            }
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
                let name = match bt {
                    "controls_if" | "controls_if_no_else" => {
                        if i < conditions_len {
                            format!("DO{}", i)
                        } else {
                            "ELSE".to_string()
                        }
                    }
                    "procedures_2_defnoreturn" => "STACK".to_string(),
                    _ => "DO".to_string(),
                };
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
        let use_blocks_xml = matches!(work_type, WorkType::Kitten2 | WorkType::Kitten3);

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

// NEMO
pub(crate) struct NemoResourceConfig<'a> {
    pub(crate) http_client: &'a dyn HttpClient,
    pub(crate) file_service: &'a FileService,
    pub(crate) work_id: WorkId,
}

pub(crate) struct NemoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl NemoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for NemoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let source_url = format!(
            "{}/creation-tools/v1/works/{}/source/public",
            self.config.base_url, work_info.id
        );
        let source_info = self.http_client.get_json(&source_url, None)?;

        let bcm_url = source_info
            .get("work_urls")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取work_urls".to_string()))?;

        let bcm_data = self.http_client.get_json(bcm_url, None)?;
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
        let base_dir = &context.config.default_output_dir;
        let work_dir = base_dir.join(folder_name);

        let resource_config = NemoResourceConfig {
            http_client: &*context.http_client,
            file_service: &context.file_service,
            work_id,
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
        let (bcm, src) = match raw {
            RawWorkData::Nemo(b, s) => (b, s),
            _ => {
                return Err(DecompilerError::Decompile(
                    "NemoDecompiler 需要 Nemo 数据".into(),
                ));
            }
        };
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
}

pub(crate) struct NemoResourceManager<'a> {
    config: NemoResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
    sha_cache: RefCell<HashMap<String, String>>,
}

impl<'a> NemoResourceManager<'a> {
    pub(crate) fn new(config: NemoResourceConfig<'a>, work_dir: PathBuf) -> Self {
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
        let material_dir = self
            .dirs
            .get("material")
            .ok_or_else(|| DecompilerError::Other {
                msg: "material目录不存在".to_string(),
                source: None,
            })?;

        if let Some(styles) = bcm_data
            .get("styles")
            .and_then(|v| v.get("styles_dict"))
            .and_then(|v| v.as_object())
        {
            for style_data in styles.values() {
                if let Some(image_url) = style_data.get("url").and_then(|v| v.as_str()) {
                    match self.config.http_client.get_binary(image_url) {
                        Ok(image_data) => {
                            let sha_hash = self.get_sha(image_url);
                            let image_path = material_dir.join(format!("{}.webp", sha_hash));
                            FileService::write_binary(&image_path, &image_data)?;
                        }
                        Err(e) => warn!("资源下载失败 {}: {}", image_url, e),
                    }
                }
            }
        }
        Ok(())
    }
}

// WOOD
pub(crate) struct WoodResourceConfig<'a> {
    pub(crate) http_client: &'a dyn HttpClient,
    pub(crate) file_service: &'a FileService,
    pub(crate) work_id: WorkId,
}

pub(crate) struct WoodFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl WoodFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for WoodFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let publish_url = format!(
            "{}/wood/work/{}/publish?channel_type=0",
            self.config.creation_base_url, work_info.id
        );
        let data = self.http_client.get_json(&publish_url, None)?;
        Ok(RawWorkData::Wood(Arc::new(data)))
    }
}

pub(crate) struct WoodDecompiler;

impl WoodDecompiler {
    fn decompile_inner(context: &DecompilerContext, work_data: Arc<Value>) -> Result<String> {
        let work_id = context.work_info.id;
        let folder_name = FileService::safe_filename(&context.work_info.name, work_id.get(), "");
        let base_dir = &context.config.default_output_dir;
        let work_dir = base_dir.join(folder_name);

        let resource_config = WoodResourceConfig {
            http_client: &*context.http_client,
            file_service: &context.file_service,
            work_id,
        };
        let mut resource_manager = WoodResourceManager::new(resource_config, work_dir.clone());

        resource_manager.create_directories()?;
        resource_manager.save_work_files(&work_data)?;
        Ok(work_dir.to_string_lossy().to_string())
    }
}

impl WorkDecompiler for WoodDecompiler {
    fn decompile(&self, raw: RawWorkData, context: &DecompilerContext) -> Result<DecompileResult> {
        let data = match raw {
            RawWorkData::Wood(d) => d,
            _ => {
                return Err(DecompilerError::Decompile(
                    "WoodDecompiler 需要 Wood 数据".into(),
                ));
            }
        };
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
    config: WoodResourceConfig<'a>,
    work_dir: PathBuf,
    dirs: HashMap<String, PathBuf>,
}

impl<'a> WoodResourceManager<'a> {
    pub(crate) fn new(config: WoodResourceConfig<'a>, work_dir: PathBuf) -> Self {
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
        let images_dir = self
            .dirs
            .get("images")
            .ok_or_else(|| DecompilerError::Other {
                msg: "images目录不存在".to_string(),
                source: None,
            })?;
        if let Some(content) = work_data.get("content").and_then(|v| v.as_array()) {
            for file_info in content {
                if file_info.get_i64_or_default("file_type", 0) == 3
                    && let Some(image_url) = file_info.get("url").and_then(|v| v.as_str())
                {
                    match self.config.http_client.get_binary(image_url) {
                        Ok(data) => {
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
                            FileService::write_binary(&images_dir.join(name), &data)?;
                        }
                        Err(e) => warn!("图片下载失败 {}: {}", image_url, e),
                    }
                }
            }
        }
        Ok(())
    }
}

// COCO
pub(crate) struct CocoFetcher {
    http_client: Box<dyn HttpClient>,
    config: Arc<DecompilerConfig>,
}

impl CocoFetcher {
    pub(crate) fn new(http_client: Box<dyn HttpClient>, config: Arc<DecompilerConfig>) -> Self {
        Self {
            http_client,
            config,
        }
    }
}

impl WorkFetcher for CocoFetcher {
    fn fetch(&self, work_info: &WorkInfo) -> Result<RawWorkData> {
        let url = format!(
            "{}/coconut/web/work/{}/load",
            self.config.creation_base_url, work_info.id
        );
        let data = self.http_client.get_json(&url, None)?;
        let compiled_url = data
            .get("data")
            .and_then(|v| v.get("bcmc_url"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| DecompilerError::InvalidResponse("无法获取bcmc_url".to_string()))?;
        let compiled = self.http_client.get_json(compiled_url, None)?;
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
        let mut work = match raw {
            RawWorkData::Coco(data) => (*data).clone(),
            _ => {
                return Err(DecompilerError::Decompile(
                    "CocoDecompiler 需要 Coco 数据".into(),
                ));
            }
        };
        Self::reorganize(&mut work, context)?;
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
        save_json_result(result, output_dir, context, &extension, "COCO")
    }
}

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
fn create_block_decompiler<'a>(compiled: &'a Value) -> Box<dyn BlockDecompiler<'a> + 'a> {
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
