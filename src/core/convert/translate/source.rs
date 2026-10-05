//! 源侧**骨架**解析:读作品文档时把 `theatre.{scenes,actors}.*.block_data_json` 留成原文
//!
//! ## 为什么
//!
//! `block_data_json` 是源文档里最大的字段。10.3 MiB 样本实测(同轮 A/B,见
//! `../../../../docs/rounds/47-data-layer-rewrite-plan.md` Step 5 的 spike):
//! 整份文档走 `Value` 要 110.8 ms / 555 517 次分配,而只扫描**跳过**这份子树只要
//! 24.7 ms / 29 302 次 —— 也就是说,源侧 `parse` 列的绝大部分花在给积木树建
//! `Value` 中间树上,而那棵树接着又只被反序列化成强类型树(`BlockJson`)。
//! 这里把它原样留成 [`RawValue`],由 `pipeline` 直接反序列化(`model::parse_block_data_json_typed`),
//! 中间树整棵省掉。
//!
//! ## 怎么读(为什么手写 `Deserialize`)
//!
//! 若用 `#[serde(flatten)]` 收"其余字段",serde 会先把**整个**结构缓冲成 `Content`
//! ——那等于又建了一棵同规模的中间树,收益归零。所以这里对三层(`文档` / `theatre` /
//! 实体)各写一个流式 `Visitor`:遇到目标键用 [`RawValue`] 捕获原文,其余键仍走 `Value`。
//!
//! ## 等价性
//!
//! 重建出的 `doc` 与"整份 `Value` 解析再摘掉 `block_data_json`"逐字段相同(键序无关:
//! serde_json 默认 `Map` 是有序的 `BTreeMap`;数值不经重编码)。形状不合(非对象、
//! 实体不是对象……)时**直接失败**,调用方回落到整份 `Value` 解析 —— 旧口径行为不变。

use serde::Deserialize;
use serde::de::{MapAccess, Visitor};
use serde_json::value::RawValue;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;

/// 骨架解析的产出
pub(super) struct Skeleton {
    /// 除 `block_data_json` 外的源文档(可直接喂给管线与装配)
    pub(super) doc: Value,
    /// 摘出来的积木树原文,键 = (容器 `scenes`/`actors`, 实体 id)
    pub(super) block_data: BTreeMap<(String, String), Box<RawValue>>,
}

/// 按骨架读一份作品文档(尾部有多余内容即错,与 `serde_json::from_str` 同口径)
pub(super) fn parse(text: &str) -> std::result::Result<Skeleton, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let raw = RawSkeleton::deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(raw.into_skeleton())
}

#[derive(Default)]
struct RawSkeleton {
    theatre: Option<RawTheatre>,
    rest: Map<String, Value>,
}

#[derive(Default)]
struct RawTheatre {
    scenes: Option<BTreeMap<String, RawEntity>>,
    actors: Option<BTreeMap<String, RawEntity>>,
    rest: Map<String, Value>,
}

#[derive(Default)]
struct RawEntity {
    block_data_json: Option<Box<RawValue>>,
    rest: Map<String, Value>,
}

impl<'de> Deserialize<'de> for RawSkeleton {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct SkeletonVisitor;

        impl<'de> Visitor<'de> for SkeletonVisitor {
            type Value = RawSkeleton;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("作品文档(JSON 对象)")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<RawSkeleton, A::Error> {
                let mut skeleton = RawSkeleton::default();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "theatre" {
                        skeleton.theatre = Some(map.next_value::<RawTheatre>()?);
                    } else {
                        skeleton.rest.insert(key, map.next_value::<Value>()?);
                    }
                }
                Ok(skeleton)
            }
        }

        deserializer.deserialize_map(SkeletonVisitor)
    }
}

impl<'de> Deserialize<'de> for RawTheatre {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct TheatreVisitor;

        impl<'de> Visitor<'de> for TheatreVisitor {
            type Value = RawTheatre;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("theatre(JSON 对象)")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<RawTheatre, A::Error> {
                let mut theatre = RawTheatre::default();
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "scenes" => theatre.scenes = Some(map.next_value()?),
                        "actors" => theatre.actors = Some(map.next_value()?),
                        _ => {
                            theatre.rest.insert(key, map.next_value::<Value>()?);
                        }
                    }
                }
                Ok(theatre)
            }
        }

        deserializer.deserialize_map(TheatreVisitor)
    }
}

impl<'de> Deserialize<'de> for RawEntity {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct EntityVisitor;

        impl<'de> Visitor<'de> for EntityVisitor {
            type Value = RawEntity;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("实体(JSON 对象)")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<RawEntity, A::Error> {
                let mut entity = RawEntity::default();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "block_data_json" {
                        entity.block_data_json = Some(map.next_value()?);
                    } else {
                        entity.rest.insert(key, map.next_value::<Value>()?);
                    }
                }
                Ok(entity)
            }
        }

        deserializer.deserialize_map(EntityVisitor)
    }
}

impl RawSkeleton {
    fn into_skeleton(self) -> Skeleton {
        let RawSkeleton { theatre, rest } = self;
        let mut block_data = BTreeMap::new();
        let mut doc = rest;
        if let Some(theatre) = theatre {
            let RawTheatre {
                scenes,
                actors,
                rest,
            } = theatre;
            let mut theatre_doc = rest;
            for (container, table) in [("scenes", scenes), ("actors", actors)] {
                let Some(table) = table else {
                    continue;
                };
                let mut entities = Map::new();
                for (id, entity) in table {
                    if let Some(block_data_json) = entity.block_data_json {
                        block_data.insert((container.to_string(), id.clone()), block_data_json);
                    }
                    entities.insert(id, Value::Object(entity.rest));
                }
                theatre_doc.insert(container.to_string(), Value::Object(entities));
            }
            doc.insert("theatre".to_string(), Value::Object(theatre_doc));
        }
        Skeleton {
            doc: Value::Object(doc),
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

    /// 非对象 / 尾部多余内容的输入:骨架解析失败(调用方据此回落整份 `Value`)
    #[test]
    fn skeleton_rejects_non_object_and_trailing_garbage() {
        assert!(parse("[1,2]").is_err());
        assert!(parse(r#"{"theatre": 1}"#).is_err());
        assert!(parse(r#"{"a": 1} {"b": 2}"#).is_err());
    }
}
