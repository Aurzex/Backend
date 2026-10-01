//! 反编译子域的影子构建器:把影子(占位/输入)积木渲染成 XML(Kitten 系)或 JSON(NEKO/NEMO)。
//!
//! 原先躺在地基 `shared.rs`(合并前是 `shared/config.rs` 的影子构建器段);这是
//! **反编译器独占**的实现细节 ⇒ 回归子域(见 `docs/rounds/39` §W2b)。

use super::config::DecompilerConfig;
use crate::core::convert::shared::{EditorType, IdGenerator};
use log::warn;
use serde_json::Value;
use serde_json::json;
use std::fmt::Write as _;
use std::sync::Arc;

// ===== 影子构建器(原 decompile/shadow.rs)=====

// 阴影构建器
#[derive(Clone)]
pub(crate) struct ShadowBuilder {
    pub(crate) config: Arc<DecompilerConfig>,
    pub(crate) id_generator: IdGenerator,
    work_type: EditorType,
}

impl ShadowBuilder {
    pub(crate) fn new(
        config: Arc<DecompilerConfig>,
        id_generator: IdGenerator,
        work_type: EditorType,
    ) -> Self {
        Self {
            config,
            id_generator,
            work_type,
        }
    }

    pub(crate) fn create(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> Value {
        if self.work_type.use_xml_shadow() {
            let xml = self.create_xml(shadow_type, block_id, text);
            Value::String(xml)
        } else {
            self.create_json(shadow_type, block_id, text)
        }
    }

    pub(crate) fn create_json(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> Value {
        let template = self.config.shadow_templates.get(shadow_type);
        let block_id = block_id.unwrap_or_else(|| self.id_generator.generate(20));

        if let Some(tmpl) = template {
            let display_text = text.or(tmpl.default_text.as_deref()).unwrap_or("");

            let mut map = serde_json::Map::new();
            map.insert("type".to_string(), Value::String(shadow_type.to_string()));
            map.insert("id".to_string(), Value::String(block_id));
            map.insert("visible".to_string(), Value::String(tmpl.visible.clone()));
            map.insert("editable".to_string(), Value::Bool(tmpl.editable));

            if tmpl.use_custom_name {
                let mut fields = serde_json::Map::new();
                if let Some(field_map) = self.config.shadow_fields.get(shadow_type) {
                    if let Some(main_field) = &tmpl.main_field {
                        for (fname, default_val) in field_map {
                            if fname == main_field {
                                fields
                                    .insert(fname.clone(), Value::String(display_text.to_string()));
                            } else {
                                fields.insert(fname.clone(), Value::String(default_val.clone()));
                            }
                        }
                    } else {
                        for (fname, val) in field_map {
                            fields.insert(fname.clone(), Value::String(val.clone()));
                        }
                    }
                }
                for (key, value) in &tmpl.extra_fields {
                    fields.insert(key.clone(), Value::String(value.clone()));
                }
                map.insert("fields".to_string(), Value::Object(fields));
            }
            Value::Object(map)
        } else {
            warn!("未找到影子类型 {} 的模板,使用默认回退", shadow_type);
            json!({
                "type": "logic_empty",
                "id": block_id,
                "visible": "visible",
                "editable": false,
            })
        }
    }

    fn create_xml(
        &self,
        shadow_type: &str,
        block_id: Option<String>,
        text: Option<&str>,
    ) -> String {
        let template = self.config.shadow_templates.get(shadow_type);
        let block_id = block_id.unwrap_or_else(|| self.id_generator.generate(20));

        let Some(tmpl) = template else {
            warn!("未找到影子类型 {} 的模板,回退为 logic_empty", shadow_type);
            return format!(
                r#"<shadow type="logic_empty" id="{}" visible="visible" editable="false"></shadow>"#,
                block_id
            );
        };
        let display_text = text.or(tmpl.default_text.as_deref()).unwrap_or("");

        if !tmpl.use_custom_name {
            return format!(
                r#"<shadow type="{}" id="{}" visible="{}" editable="{}"></shadow>"#,
                shadow_type, block_id, tmpl.visible, tmpl.editable
            );
        }

        let mut fields: Vec<(String, String)> = Vec::new();
        if let Some(field_map) = self.config.shadow_fields.get(shadow_type) {
            if let Some(main_field) = &tmpl.main_field {
                for (fname, default_val) in field_map {
                    if fname == main_field {
                        fields.push((fname.clone(), display_text.to_string()));
                    } else {
                        fields.push((fname.clone(), default_val.clone()));
                    }
                }
            } else {
                for (fname, val) in field_map {
                    fields.push((fname.clone(), val.clone()));
                }
            }
        }
        for (k, v) in &tmpl.extra_fields {
            fields.push((k.clone(), v.clone()));
        }

        let mut xml = format!(
            r#"<shadow type="{}" id="{}" visible="{}" editable="{}">"#,
            shadow_type, block_id, tmpl.visible, tmpl.editable
        );
        for (name, value) in fields {
            let _ = write!(xml, r#"<field name="{}">{}</field>"#, name, value);
        }
        xml.push_str("</shadow>");
        xml
    }
}
