//! 可流式写出的产物文档(写出器,方向无关的机械)。
//!
//! 装配(`assembly`)只登记"哪个实体的块表挂在哪";这里负责:
//! - [`ProductDocument`]:KN 产物(正向 / NEMO 同形,只差 `shield` 口径);
//! - [`Kitten4ProductDocument`]:Kitten4 产物的反向流式形态。
//!
//! 两条路径都支持 `into_value()`(与整份 `Value` 路径逐字节相同)与 `write_to()`(占位处直写)。

use super::model;
use super::model::{EncodedBlocks, type_name};
use crate::core::convert::shared::{ConvertError, Result};
use serde_json::{Map, Value};
use std::collections::HashMap;

/// 可**流式写出**的产物文档(`rounds/47` §4 Step 2/3)
///
/// 三处 `nekoBlockJsonList`(角色 / 场景 / 程序集定义体)在 `doc` 里只留 `null` **占位**
/// (位置由 `Map` 的字典序决定),真正的块表挂在 [`BlockPlacement`] 上:
///
/// - 内存路径([`Self::into_value`])把树物化回 `Value` 填占位 —— 与旧口径**逐字节相同**;
/// - 文件路径([`Self::write_to`])在占位处**直接写**块树,不建那份产物 `Value`
///   (产物侧最大的分配来源:十万级块的整份物化 + 序列化遍历 + 整体析构)。
///
/// 两条 KN 产物路径共用本类型:正向(Kitten4 → KN,`build_document_product`)与
/// NEMO(NEMO → KN,[`Self::new_nemo`])产物同形,只有块表编码口径([`model::ShieldPolicy`])不同。
pub(super) struct ProductDocument {
    doc: Map<String, Value>,
    hooks: Vec<BlockPlacement>,
    /// 块表编码口径:正向(`ShieldPolicy::Always`)与 NEMO(`OnlyWhenTrue`)产物同为 KN 文档,
    /// 唯一差别就在这里(见 [`model::ShieldPolicy`] 与 `model::write_block_tree`)
    shield: model::ShieldPolicy,
}

/// 一处 `nekoBlockJsonList` 的挂点(装配侧登记,写出侧按它填占位)
///
/// NEMO 方向的产物同样只有这三处(见 `nemo::convert_nemo_document_product`),因此两条路径
/// 共用这一份挂点 + 写出实现。
pub(super) struct BlockPlacement {
    pub(super) container: BlockContainer,
    pub(super) id: String,
    pub(super) tree: model::BlockTree,
}

/// 挂点所在的三处容器(各自的"段名"与"字典键"不同,官方口径见 `build_document`)
#[derive(Clone, Copy, PartialEq)]
pub(super) enum BlockContainer {
    Actors,
    Scenes,
    Procedures,
}

impl BlockContainer {
    fn section_key(self) -> &'static str {
        match self {
            BlockContainer::Actors => "actors",
            BlockContainer::Scenes => "scenes",
            BlockContainer::Procedures => "procedures",
        }
    }

    fn dict_key(self) -> &'static str {
        match self {
            BlockContainer::Actors => "actorsDict",
            BlockContainer::Scenes => "scenesDict",
            BlockContainer::Procedures => "proceduresDict",
        }
    }
}

impl ProductDocument {
    /// 正向(Kitten4 → KN)的产物:块表编码口径 [`model::ShieldPolicy::Always`]
    pub(super) fn new(doc: Map<String, Value>, hooks: Vec<BlockPlacement>) -> Self {
        ProductDocument {
            doc,
            hooks,
            shield: model::ShieldPolicy::Always,
        }
    }

    /// NEMO → KN 的产物(`nemo::convert_nemo_document_product` 用它):同形的 KN 文档,
    /// 块表按 NEMO 旧口径编码(不补 `shield` 假值,见 [`model::ShieldPolicy`])
    pub(super) fn new_nemo(doc: Map<String, Value>, hooks: Vec<BlockPlacement>) -> Self {
        ProductDocument {
            doc,
            hooks,
            shield: model::ShieldPolicy::OnlyWhenTrue,
        }
    }

    /// 物化回整份文档(`TranslateDocument.document`,公开面口径不变)
    pub(super) fn into_value(mut self) -> Result<Value> {
        for hook in std::mem::take(&mut self.hooks) {
            let missing = || ConvertError::Other {
                msg: format!(
                    "产物缺少 {} 段的 {} 条目:无法放回块表",
                    hook.container.section_key(),
                    hook.id
                ),
                source: None,
            };
            let entry = self
                .doc
                .get_mut(hook.container.section_key())
                .and_then(|section| section.get_mut(hook.container.dict_key()))
                .and_then(|dict| dict.get_mut(&hook.id))
                .and_then(Value::as_object_mut)
                .ok_or_else(missing)?;
            entry.insert(
                "nekoBlockJsonList".into(),
                Value::Array(model::tree_to_value(&hook.tree, self.shield)?),
            );
        }
        Ok(Value::Object(self.doc))
    }

    /// 写一个顶层字段(域门面在流式落盘前补 `source` 等信封字段)
    pub(super) fn set_top_level(&mut self, key: &str, value: Value) {
        self.doc.insert(key.to_string(), value);
    }

    /// 直接写整份文档(文件路径):除三处块表外逐键交给 `serde_json`,块表在占位处流式写
    pub(super) fn write_to(&self, w: &mut impl std::io::Write) -> Result<()> {
        w.write_all(b"{")?;
        for (index, (key, value)) in self.doc.iter().enumerate() {
            if index > 0 {
                w.write_all(b",")?;
            }
            serde_json::to_writer(&mut *w, key)?;
            w.write_all(b":")?;
            match key.as_str() {
                "actors" => {
                    write_section(value, BlockContainer::Actors, &self.hooks, self.shield, w)?
                }
                "scenes" => {
                    write_section(value, BlockContainer::Scenes, &self.hooks, self.shield, w)?
                }
                "procedures" => write_section(
                    value,
                    BlockContainer::Procedures,
                    &self.hooks,
                    self.shield,
                    w,
                )?,
                _ => serde_json::to_writer(&mut *w, value)?,
            }
        }
        w.write_all(b"}")?;
        Ok(())
    }
}

/// 写一个"段"(`{<字典键>: {...}, 其余键原样}`):只有字典键里带块表挂点
fn write_section(
    section: &Value,
    container: BlockContainer,
    hooks: &[BlockPlacement],
    shield: model::ShieldPolicy,
    w: &mut impl std::io::Write,
) -> Result<()> {
    let map = section
        .as_object()
        .ok_or_else(|| ConvertError::TypeMismatch {
            expected: format!("object({} 段)", container.section_key()),
            actual: type_name(section).into(),
        })?;
    w.write_all(b"{")?;
    for (index, (key, value)) in map.iter().enumerate() {
        if index > 0 {
            w.write_all(b",")?;
        }
        serde_json::to_writer(&mut *w, key)?;
        w.write_all(b":")?;
        if key == container.dict_key() {
            write_entries(value, container, hooks, shield, w)?;
        } else {
            serde_json::to_writer(&mut *w, value)?;
        }
    }
    w.write_all(b"}")?;
    Ok(())
}

/// 写"条目字典"(`{<id>: {…}}`):命中挂点的条目走 [`write_entry_with_blocks`]
fn write_entries(
    dict: &Value,
    container: BlockContainer,
    hooks: &[BlockPlacement],
    shield: model::ShieldPolicy,
    w: &mut impl std::io::Write,
) -> Result<()> {
    let map = dict.as_object().ok_or_else(|| ConvertError::TypeMismatch {
        expected: format!("object({} 字典)", container.dict_key()),
        actual: type_name(dict).into(),
    })?;
    // 挂点按 id 建一次索引:写出路径上按条目线性扫挂点会是 O(实体 × 挂点)
    let hook_by_id: HashMap<&str, &BlockPlacement> = hooks
        .iter()
        .filter(|hook| hook.container == container)
        .map(|hook| (hook.id.as_str(), hook))
        .collect();
    w.write_all(b"{")?;
    for (index, (id, entry)) in map.iter().enumerate() {
        if index > 0 {
            w.write_all(b",")?;
        }
        serde_json::to_writer(&mut *w, id)?;
        w.write_all(b":")?;
        match hook_by_id.get(id.as_str()) {
            Some(hook) => write_entry_with_blocks(entry, &hook.tree, shield, w)?,
            None => serde_json::to_writer(&mut *w, entry)?,
        }
    }
    w.write_all(b"}")?;
    Ok(())
}

/// 写一个带块表的条目:除 `nekoBlockJsonList` 占位处外逐键原样写
fn write_entry_with_blocks(
    entry: &Value,
    tree: &model::BlockTree,
    shield: model::ShieldPolicy,
    w: &mut impl std::io::Write,
) -> Result<()> {
    let map = entry
        .as_object()
        .ok_or_else(|| ConvertError::TypeMismatch {
            expected: "object(实体/程序集条目)".into(),
            actual: type_name(entry).into(),
        })?;
    w.write_all(b"{")?;
    for (index, (key, value)) in map.iter().enumerate() {
        if index > 0 {
            w.write_all(b",")?;
        }
        serde_json::to_writer(&mut *w, key)?;
        w.write_all(b":")?;
        if key == "nekoBlockJsonList" {
            model::write_block_tree(tree, shield, w)?;
        } else {
            serde_json::to_writer(&mut *w, value)?;
        }
    }
    w.write_all(b"}")?;
    Ok(())
}

/// 可**流式写出**的反向产物文档(`rounds/47` §4 Step 4)
///
/// 每实体的 `block_data_json` 在文档里只留 `null` 占位(键序由 `Map` 决定),真正的
/// [`EncodedBlocks`] 挂在 [`Kitten4Hook`] 上:
/// - [`Self::into_value`] 物化回 `Value`(与旧口径逐字节相同);
/// - [`Self::write_to`] 在占位处**直接写**(不建节点 `Value` —— 与正向 Step 2/3 同一套做法)。
pub(super) struct Kitten4ProductDocument {
    doc: Map<String, Value>,
    hooks: Vec<Kitten4Hook>,
}

pub(super) struct Kitten4Hook {
    is_scene: bool,
    id: String,
    blocks: EncodedBlocks,
    /// 待挂「原内容注释」的标记块(`(块 id, 原 KN 快照)`);注释 id 在装配末尾统一铸
    pending_comments: Vec<(String, super::origin::KnOrigin)>,
}

impl Kitten4Hook {
    pub(super) fn new(
        is_scene: bool,
        id: String,
        blocks: EncodedBlocks,
        pending_comments: Vec<(String, super::origin::KnOrigin)>,
    ) -> Self {
        Self {
            is_scene,
            id,
            blocks,
            pending_comments,
        }
    }

    /// 取走待挂的「原内容注释」(装配末尾统一铸注释 id)
    pub(super) fn take_pending_comments(&mut self) -> Vec<(String, super::origin::KnOrigin)> {
        std::mem::take(&mut self.pending_comments)
    }

    /// 给标记块挂上「原内容注释」(返回是否找到该块)
    pub(super) fn add_origin_comment(
        &mut self,
        block_id: &str,
        comment_id: &str,
        origin: super::origin::KnOrigin,
    ) -> bool {
        self.blocks.add_origin_comment(block_id, comment_id, origin)
    }
}

impl Kitten4ProductDocument {
    /// 反向(KN → Kitten4)的流式产物:文档 + 每实体的块表挂点
    pub(super) fn new(doc: Map<String, Value>, hooks: Vec<Kitten4Hook>) -> Self {
        Kitten4ProductDocument { doc, hooks }
    }

    /// 物化回整份文档(`TranslateDocument.document` 口径不变)
    pub(super) fn into_value(mut self) -> Result<Value> {
        for hook in std::mem::take(&mut self.hooks) {
            let container = if hook.is_scene { "scenes" } else { "actors" };
            let filled = model::encoded_to_value(hook.blocks)?;
            if let Some(entry) = self
                .doc
                .get_mut("theatre")
                .and_then(Value::as_object_mut)
                .and_then(|theatre| theatre.get_mut(container))
                .and_then(Value::as_object_mut)
                .and_then(|table| table.get_mut(&hook.id))
                .and_then(Value::as_object_mut)
            {
                entry.insert("block_data_json".into(), filled);
            } else {
                // 装配端一定放好了占位;真到不了这里(到了就是写出器的 bug,留痕而不是静默丢键)
                debug_assert!(false, "反向产物缺少 {container}.{} 条目", hook.id);
            }
        }
        Ok(Value::Object(self.doc))
    }

    /// 直接写整份文档(文件路径):除两处块表外逐键交给 `serde_json`,块表在占位处流式写
    pub(super) fn write_to(&self, w: &mut impl std::io::Write) -> Result<()> {
        w.write_all(b"{")?;
        for (index, (key, value)) in self.doc.iter().enumerate() {
            if index > 0 {
                w.write_all(b",")?;
            }
            serde_json::to_writer(&mut *w, key)?;
            w.write_all(b":")?;
            if key == "theatre" {
                self.write_theatre(value, w)?;
            } else {
                serde_json::to_writer(&mut *w, value)?;
            }
        }
        w.write_all(b"}")?;
        Ok(())
    }

    /// `theatre` 段:`actors` / `scenes` 两个字典里挂块表
    fn write_theatre(&self, theatre: &Value, w: &mut impl std::io::Write) -> Result<()> {
        let map = theatre
            .as_object()
            .ok_or_else(|| ConvertError::TypeMismatch {
                expected: "object(theatre)".into(),
                actual: type_name(theatre).into(),
            })?;
        w.write_all(b"{")?;
        for (index, (key, value)) in map.iter().enumerate() {
            if index > 0 {
                w.write_all(b",")?;
            }
            serde_json::to_writer(&mut *w, key)?;
            w.write_all(b":")?;
            match key.as_str() {
                "actors" => self.write_entity_table(value, false, w)?,
                "scenes" => self.write_entity_table(value, true, w)?,
                _ => serde_json::to_writer(&mut *w, value)?,
            }
        }
        w.write_all(b"}")?;
        Ok(())
    }

    /// `theatre.{actors,scenes}` 字典:命中挂点的实体走占位处流写
    fn write_entity_table(
        &self,
        table: &Value,
        is_scene: bool,
        w: &mut impl std::io::Write,
    ) -> Result<()> {
        let map = table
            .as_object()
            .ok_or_else(|| ConvertError::TypeMismatch {
                expected: "object(实体字典)".into(),
                actual: type_name(table).into(),
            })?;
        w.write_all(b"{")?;
        let hook_by_id: HashMap<&str, &Kitten4Hook> = self
            .hooks
            .iter()
            .filter(|hook| hook.is_scene == is_scene)
            .map(|hook| (hook.id.as_str(), hook))
            .collect();
        for (index, (id, entry)) in map.iter().enumerate() {
            if index > 0 {
                w.write_all(b",")?;
            }
            serde_json::to_writer(&mut *w, id)?;
            w.write_all(b":")?;
            match hook_by_id.get(id.as_str()) {
                Some(hook) => write_entity_with_blocks(entry, &hook.blocks, w)?,
                None => serde_json::to_writer(&mut *w, entry)?,
            }
        }
        w.write_all(b"}")?;
        Ok(())
    }
}

/// 写一个带块表的实体条目:除 `block_data_json` 占位处外逐键原样写
fn write_entity_with_blocks(
    entry: &Value,
    blocks: &EncodedBlocks,
    w: &mut impl std::io::Write,
) -> Result<()> {
    let map = entry
        .as_object()
        .ok_or_else(|| ConvertError::TypeMismatch {
            expected: "object(实体条目)".into(),
            actual: type_name(entry).into(),
        })?;
    w.write_all(b"{")?;
    for (index, (key, value)) in map.iter().enumerate() {
        if index > 0 {
            w.write_all(b",")?;
        }
        serde_json::to_writer(&mut *w, key)?;
        w.write_all(b":")?;
        if key == "block_data_json" {
            model::write_encoded_blocks(blocks, w)?;
        } else {
            serde_json::to_writer(&mut *w, value)?;
        }
    }
    w.write_all(b"}")?;
    Ok(())
}
