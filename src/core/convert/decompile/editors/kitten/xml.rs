use crate::core::convert::decompile::blocks::{child_input_name, referenced_ids};
use crate::core::convert::shared::{DecompilerConfig, Result, ValueExt};
use serde_json::Value;
use std::collections::HashSet;
use std::fmt::Write as _;

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
