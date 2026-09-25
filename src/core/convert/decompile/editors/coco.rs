use crate::core::convert::decompile::{
    DecompileResult, DecompilerContext, WorkDecompiler, save_json_result,
};
use crate::core::convert::shared::{
    DecompilerConfig, DecompilerError, HttpClient, RawWorkData, Result, WorkFetcher, WorkInfo,
};
use log::warn;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
