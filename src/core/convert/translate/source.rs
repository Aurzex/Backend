//! 源侧**骨架**解析:读作品文档时把"最大的那份积木表"留成原文
//!
//! 两个方向共用这一套三层 `Visitor`,按 [`SourceShape`] 参数化(形状 = 段 / 字典 / 叶子键的路径表):
//!
//! | 方向 | 段 | 字典 | 叶子键 |
//! | --- | --- | --- | --- |
//! | Kitten4(`.bcm4`,正向) | `theatre` | `scenes` / `actors` | `block_data_json` |
//! | KittenN(`.bcmkn`,反向) | `actors` / `scenes` / `procedures` | `actorsDict` / `scenesDict` / `proceduresDict` | `nekoBlockJsonList` |
//!
//! 两份叶子键都是源文档里最大的字段。Kitten4 侧 10.3 MiB 样本实测(同轮 A/B,见
//! `../../../../docs/rounds/47-data-layer-rewrite-plan.md` Step 5 的 spike):整份文档走 `Value`
//! 要 110.8 ms / 555 517 次分配,而只扫描**跳过**这份子树只要 24.7 ms / 29 302 次 —— 也就是说,
//! 源侧 `parse` 列的绝大部分花在给积木树建 `Value` 中间树上,而那棵树接着又只被反序列化成强类型树
//! (`BlockJson` / `parse_kn_entity`)。这里把它原样留成 [`RawValue`],由 `pipeline` 直接反序列化
//! (`model::parse_block_data_json_typed` / `model::parse_kn_entity_typed`),中间树整棵省掉。
//!
//! ## 怎么读(为什么手写 `Deserialize`)
//!
//! 若用 `#[serde(flatten)]` 收"其余字段",serde 会先把**整个**结构缓冲成 `Content`
//! ——那等于又建了一棵同规模的中间树,收益归零。所以这里对三层(`文档` / `段` / `实体`)
//! 各写一个流式 `Visitor`:遇到目标键用 [`RawValue`] 捕获原文,其余键仍走 `Value`。
//!
//! ## 等价性
//!
//! 重建出的 `doc` 与"整份 `Value` 解析再摘掉叶子键"逐字段相同(键序无关:serde_json 默认 `Map`
//! 是有序的 `BTreeMap`;数值不经重编码)。形状不合(非对象、段不是对象、实体不是对象……)时
//! **直接失败**,调用方回落到整份 `Value` 解析 —— 旧口径行为不变。
//!
//! 判据(哪些键算段/字典/叶子)只能来自编译期的 [`SourceShape`]:段名与字典名在**同一形状内唯一**
//! (Kitten4 的两个字典同名于段下、KittenN 的三个字典名互不相同),因此第二层不需要知道"自己在哪个段里"。

use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use serde_json::value::RawValue;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

/// 骨架解析的产出
pub(super) struct Skeleton {
    /// 除叶子键外的源文档(可直接喂给管线与装配)
    pub(super) doc: Value,
    /// 摘出来的积木表原文,键 = ([`Target::label`], 实体 id)
    pub(super) block_data: BTreeMap<(String, String), Box<RawValue>>,
}

/// 文档形状:骨架按哪张挂点表读(见模块文档的表)
pub(super) trait SourceShape: 'static {
    /// 挂点表:`(段, 字典, 旁表键里的容器名)`
    const TARGETS: &'static [Target];
    /// 实体下的目标键(同一形状内唯一)
    const LEAF: &'static str;
}

/// 一处挂点
pub(super) struct Target {
    /// 文档下的一级键
    pub(super) section: &'static str,
    /// 段下的键(同一形状内唯一)
    pub(super) dict: &'static str,
    /// 旁表键里的"容器"名(调用方按它取树)
    pub(super) label: &'static str,
}

/// Kitten4 编辑版(`theatre.{scenes,actors}.*.block_data_json`)
pub(super) struct Kitten4Shape;

impl SourceShape for Kitten4Shape {
    const TARGETS: &'static [Target] = &[
        Target {
            section: "theatre",
            dict: "scenes",
            label: "scenes",
        },
        Target {
            section: "theatre",
            dict: "actors",
            label: "actors",
        },
    ];
    const LEAF: &'static str = "block_data_json";
}

/// KittenN 编辑版(`{actors.actorsDict,scenes.scenesDict,procedures.proceduresDict}.*.nekoBlockJsonList`)
pub(super) struct KnShape;

impl SourceShape for KnShape {
    const TARGETS: &'static [Target] = &[
        Target {
            section: "actors",
            dict: "actorsDict",
            label: "actors",
        },
        Target {
            section: "scenes",
            dict: "scenesDict",
            label: "scenes",
        },
        Target {
            section: "procedures",
            dict: "proceduresDict",
            label: "procedures",
        },
    ];
    const LEAF: &'static str = "nekoBlockJsonList";
}

/// 按 Kitten4 形状读一份作品文档(尾部有多余内容即错,与 `serde_json::from_str` 同口径)
pub(super) fn parse(text: &str) -> std::result::Result<Skeleton, serde_json::Error> {
    parse_with::<Kitten4Shape>(text)
}

/// 按 KittenN 形状读一份作品文档(同上)
pub(super) fn parse_kn(text: &str) -> std::result::Result<Skeleton, serde_json::Error> {
    parse_with::<KnShape>(text)
}

fn parse_with<M: SourceShape>(text: &str) -> std::result::Result<Skeleton, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let raw = RawDoc::<M>::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(raw.into_skeleton())
}

/// 文档层:命中挂点表的键(`Target::section`)进 [`RawSection`],其余仍是 `Value`
struct RawDoc<M> {
    sections: BTreeMap<String, RawSection<M>>,
    rest: Map<String, Value>,
}

/// 段层:命中挂点表的字典名(`Target::dict`)进 [`RawDict`],其余仍是 `Value`
struct RawSection<M> {
    dicts: BTreeMap<String, RawDict<M>>,
    rest: Map<String, Value>,
}

/// 字典层:一张"实体表" + 它在旁表里的容器名(构造期从挂点表带过来,免得回收时反查)
struct RawDict<M> {
    label: &'static str,
    entities: BTreeMap<String, RawEntity<M>>,
}

/// 实体层:命中叶子键(`SourceShape::LEAF`)用 [`RawValue`] 捕获原文,其余仍是 `Value`
struct RawEntity<M> {
    leaf: Option<Box<RawValue>>,
    rest: Map<String, Value>,
    shape: PhantomData<M>,
}

impl<'de, M: SourceShape> Deserialize<'de> for RawDoc<M> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct DocVisitor<M>(PhantomData<M>);

        impl<'de, M: SourceShape> Visitor<'de> for DocVisitor<M> {
            type Value = RawDoc<M>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("作品文档(JSON 对象)")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<RawDoc<M>, A::Error> {
                let mut doc = RawDoc {
                    sections: BTreeMap::new(),
                    rest: Map::new(),
                };
                while let Some(key) = map.next_key::<String>()? {
                    if M::TARGETS.iter().any(|target| target.section == key) {
                        doc.sections.insert(key, map.next_value::<RawSection<M>>()?);
                    } else {
                        doc.rest.insert(key, map.next_value::<Value>()?);
                    }
                }
                Ok(doc)
            }
        }

        deserializer.deserialize_map(DocVisitor::<M>(PhantomData))
    }
}

impl<'de, M: SourceShape> Deserialize<'de> for RawSection<M> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct SectionVisitor<M>(PhantomData<M>);

        impl<'de, M: SourceShape> Visitor<'de> for SectionVisitor<M> {
            type Value = RawSection<M>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("作品段(JSON 对象)")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<RawSection<M>, A::Error> {
                let mut section = RawSection {
                    dicts: BTreeMap::new(),
                    rest: Map::new(),
                };
                while let Some(key) = map.next_key::<String>()? {
                    match M::TARGETS.iter().find(|target| target.dict == key) {
                        Some(target) => {
                            let entities = map.next_value::<BTreeMap<String, RawEntity<M>>>()?;
                            section.dicts.insert(
                                key,
                                RawDict {
                                    label: target.label,
                                    entities,
                                },
                            );
                        }
                        None => {
                            section.rest.insert(key, map.next_value::<Value>()?);
                        }
                    }
                }
                Ok(section)
            }
        }

        deserializer.deserialize_map(SectionVisitor::<M>(PhantomData))
    }
}

impl<'de, M: SourceShape> Deserialize<'de> for RawEntity<M> {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct EntityVisitor<M>(PhantomData<M>);

        impl<'de, M: SourceShape> Visitor<'de> for EntityVisitor<M> {
            type Value = RawEntity<M>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("实体(JSON 对象)")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<RawEntity<M>, A::Error> {
                let mut entity = RawEntity {
                    leaf: None,
                    rest: Map::new(),
                    shape: PhantomData,
                };
                while let Some(key) = map.next_key::<String>()? {
                    if key == M::LEAF {
                        entity.leaf = Some(map.next_value()?);
                    } else {
                        entity.rest.insert(key, map.next_value::<Value>()?);
                    }
                }
                Ok(entity)
            }
        }

        deserializer.deserialize_map(EntityVisitor::<M>(PhantomData))
    }
}

impl<M: SourceShape> RawDoc<M> {
    fn into_skeleton(self) -> Skeleton {
        let RawDoc { sections, mut rest } = self;
        let mut block_data = BTreeMap::new();
        for (section_key, section) in sections {
            let RawSection {
                dicts,
                rest: section_rest,
            } = section;
            let mut section_doc = section_rest;
            for (dict_key, dict) in dicts {
                let RawDict { label, entities } = dict;
                let mut rebuilt = Map::new();
                for (id, entity) in entities {
                    if let Some(leaf) = entity.leaf {
                        block_data.insert((label.to_string(), id.clone()), leaf);
                    }
                    rebuilt.insert(id, Value::Object(entity.rest));
                }
                section_doc.insert(dict_key, Value::Object(rebuilt));
            }
            rest.insert(section_key, Value::Object(section_doc));
        }
        Skeleton {
            doc: Value::Object(rest),
            block_data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 骨架读出的文档 == 整份 `Value` 解析再摘掉 `block_data_json`(逐字段)
    #[test]
    fn skeleton_doc_matches_plain_value_minus_block_data() {
        let text = r#"{
            "size": {"width": 480, "height": 360},
            "theatre": {
                "scenes": {"s1": {"name": "舞台", "block_data_json": {"blocks": {"a": {"type": "x"}}}}},
                "actors": {"a1": {"name": "角色", "block_data_json": {"blocks": {}}, "x": 1.5},
                            "a2": {"name": "无积木"}},
                "id_counter": 3
            },
            "extra": [1, 2, {"k": null}]
        }"#;
        let skeleton = parse(text).expect("骨架解析应成功");
        let mut plain: Value = serde_json::from_str(text).unwrap();
        for container in ["scenes", "actors"] {
            if let Some(map) = plain["theatre"][container].as_object_mut() {
                for entity in map.values_mut() {
                    entity.as_object_mut().unwrap().remove("block_data_json");
                }
            }
        }
        assert_eq!(skeleton.doc, plain);
        assert_eq!(skeleton.block_data.len(), 2);
        assert_eq!(
            skeleton
                .block_data
                .get(&("actors".to_string(), "a1".to_string()))
                .unwrap()
                .get(),
            r#"{"blocks": {}}"#
        );
        assert_eq!(
            skeleton
                .block_data
                .get(&("scenes".to_string(), "s1".to_string()))
                .unwrap()
                .get()
                .replace(' ', ""),
            r#"{"blocks":{"a":{"type":"x"}}}"#
        );
    }

    /// KN:骨架读出的文档 == 整份 `Value` 解析再摘掉三处 `nekoBlockJsonList`(逐字段);
    /// 旁表键 = (段名, id),三处段都要命中
    #[test]
    fn kn_skeleton_doc_matches_plain_value_minus_block_tables() {
        let text = r#"{
            "version": "1.0.0",
            "actors": {
                "actorsDict": {"a1": {"name": "角色", "nekoBlockJsonList": [{"type": "x", "id": "n1"}], "x": 1}},
                "currentActor": "a1"
            },
            "scenes": {
                "scenesDict": {"s1": {"name": "背景", "nekoBlockJsonList": []}},
                "scenes_order": ["s1"]
            },
            "procedures": {"proceduresDict": {"p1": {"name": "定义", "nekoBlockJsonList": [{"type": ""}]}}},
            "variables": {"variablesDict": {}}
        }"#;
        let skeleton = parse_kn(text).expect("骨架解析应成功");
        let mut plain: Value = serde_json::from_str(text).unwrap();
        for (section, dict) in [
            ("actors", "actorsDict"),
            ("scenes", "scenesDict"),
            ("procedures", "proceduresDict"),
        ] {
            if let Some(map) = plain[section][dict].as_object_mut() {
                for entity in map.values_mut() {
                    entity.as_object_mut().unwrap().remove("nekoBlockJsonList");
                }
            }
        }
        assert_eq!(skeleton.doc, plain);
        assert_eq!(skeleton.block_data.len(), 3);
        assert_eq!(
            skeleton
                .block_data
                .get(&("actors".to_string(), "a1".to_string()))
                .unwrap()
                .get()
                .replace(' ', ""),
            r#"[{"type":"x","id":"n1"}]"#
        );
        assert_eq!(
            skeleton
                .block_data
                .get(&("scenes".to_string(), "s1".to_string()))
                .unwrap()
                .get(),
            "[]"
        );
        // 段名与字典名之外的键仍走 `Value`(空的定义体数组也要认得出来)
        assert!(skeleton.doc["actors"]["currentActor"].is_string());
        assert_eq!(skeleton.doc["scenes"]["scenes_order"][0], "s1");
    }

    /// 非对象 / 段不是对象 / 尾部多余内容的输入:骨架解析失败(调用方据此回落整份 `Value`)
    #[test]
    fn skeleton_rejects_non_object_and_trailing_garbage() {
        assert!(parse("[1,2]").is_err());
        assert!(parse(r#"{"theatre": 1}"#).is_err());
        assert!(parse(r#"{"a": 1} {"b": 2}"#).is_err());
        // 反向形状:段/字典不是对象时同样失败(回落到整份 `Value`)
        assert!(parse_kn("[1,2]").is_err());
        assert!(parse_kn(r#"{"actors": {"actorsDict": 1}}"#).is_err());
        assert!(parse_kn(r#"{"actors": {"actorsDict": {"a1": 1}}}"#).is_err());
        assert!(parse_kn(r#"{"a": 1} {"b": 2}"#).is_err());
    }
}
