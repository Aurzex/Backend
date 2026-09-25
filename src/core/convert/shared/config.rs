use crate::utils::filedata::PathConfig;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

// 阴影模板
#[derive(Debug, Clone)]
pub(crate) struct ShadowTemplate {
    pub(crate) editable: bool,
    pub(crate) visible: String,
    pub(crate) extra_fields: Vec<(String, String)>,
    pub(crate) default_text: Option<String>,
    pub(crate) use_custom_name: bool,
    pub(crate) main_field: Option<String>, // 新增,指明主字段名(在 shadow_fields 中的键)
}

impl Default for ShadowTemplate {
    fn default() -> Self {
        Self {
            editable: true,
            visible: "visible".to_string(),
            extra_fields: vec![],
            default_text: None,
            use_custom_name: false,
            main_field: None,
        }
    }
}

// 配置
#[derive(Debug, Clone)]
pub(crate) struct DecompilerConfig {
    pub(crate) base_url: String,
    pub(crate) creation_base_url: String,
    pub(crate) crypto_salt: Vec<u8>,
    pub(crate) default_output_dir: PathBuf,
    pub(crate) toolbox_categories: Vec<String>,
    pub(crate) shadow_types: Arc<HashSet<String>>,
    pub(crate) shadow_fields: Arc<HashMap<String, HashMap<String, String>>>,
    pub(crate) file_extensions: Arc<HashMap<String, String>>,
    pub(crate) shadow_templates: Arc<HashMap<String, ShadowTemplate>>,
}

impl Default for DecompilerConfig {
    fn default() -> Self {
        let mut shadow_types = HashSet::new();
        for st in [
            "broadcast_input",
            "controller_shadow",
            "default_value",
            "get_audios",
            "get_current_costume",
            "get_current_scene",
            "get_sensing_current_scene",
            "get_whole_audios",
            "lists_get",
            "logic_empty",
            "logic_boolean",
            "math_number",
            "text",
            "shadow_text",
            "shadow_number",
        ] {
            shadow_types.insert(st.to_string());
        }

        let mut shadow_fields = HashMap::new();
        let mut math_number = HashMap::new();
        math_number.insert("name".to_string(), "NUM".to_string());
        math_number.insert("text".to_string(), "0".to_string());
        math_number.insert(
            "constraints".to_string(),
            "-Infinity,Infinity,0,".to_string(),
        );
        math_number.insert("allow_text".to_string(), "true".to_string());
        shadow_fields.insert("math_number".to_string(), math_number);

        let mut controller_shadow = HashMap::new();
        controller_shadow.insert("name".to_string(), "NUM".to_string());
        controller_shadow.insert("text".to_string(), "0".to_string());
        controller_shadow.insert(
            "constraints".to_string(),
            "-Infinity,Infinity,0,false".to_string(),
        );
        shadow_fields.insert("controller_shadow".to_string(), controller_shadow);

        let mut text = HashMap::new();
        text.insert("name".to_string(), "TEXT".to_string());
        text.insert("text".to_string(), String::new());
        shadow_fields.insert("text".to_string(), text);

        let mut lists_get = HashMap::new();
        lists_get.insert("name".to_string(), "VAR".to_string());
        lists_get.insert("text".to_string(), "?".to_string());
        shadow_fields.insert("lists_get".to_string(), lists_get);

        let mut broadcast_input = HashMap::new();
        broadcast_input.insert("name".to_string(), "MESSAGE".to_string());
        broadcast_input.insert("text".to_string(), "Hi".to_string());
        shadow_fields.insert("broadcast_input".to_string(), broadcast_input);

        let mut get_audios = HashMap::new();
        get_audios.insert("name".to_string(), "sound_id".to_string());
        get_audios.insert("text".to_string(), "?".to_string());
        shadow_fields.insert("get_audios".to_string(), get_audios);

        let mut get_whole_audios = HashMap::new();
        get_whole_audios.insert("name".to_string(), "sound_id".to_string());
        get_whole_audios.insert("text".to_string(), "all".to_string());
        shadow_fields.insert("get_whole_audios".to_string(), get_whole_audios);

        let mut get_current_costume = HashMap::new();
        get_current_costume.insert("name".to_string(), "style_id".to_string());
        get_current_costume.insert("text".to_string(), String::new());
        shadow_fields.insert("get_current_costume".to_string(), get_current_costume);

        let mut default_value = HashMap::new();
        default_value.insert("name".to_string(), "TEXT".to_string());
        default_value.insert("text".to_string(), "0".to_string());
        default_value.insert("has_been_edited".to_string(), "false".to_string());
        shadow_fields.insert("default_value".to_string(), default_value);

        let mut get_current_scene = HashMap::new();
        get_current_scene.insert("name".to_string(), "scene".to_string());
        get_current_scene.insert("text".to_string(), String::new());
        shadow_fields.insert("get_current_scene".to_string(), get_current_scene);

        let mut get_sensing_current_scene = HashMap::new();
        get_sensing_current_scene.insert("name".to_string(), "scene".to_string());
        get_sensing_current_scene.insert("text".to_string(), String::new());
        shadow_fields.insert(
            "get_sensing_current_scene".to_string(),
            get_sensing_current_scene,
        );

        let mut shadow_text = HashMap::new();
        shadow_text.insert("name".to_string(), "TEXT".to_string());
        shadow_text.insert("text".to_string(), String::new());
        shadow_fields.insert("shadow_text".to_string(), shadow_text);

        let mut shadow_number = HashMap::new();
        shadow_number.insert("name".to_string(), "NUM".to_string());
        shadow_number.insert("text".to_string(), "0".to_string());
        shadow_number.insert(
            "constraints".to_string(),
            "-Infinity,Infinity,0,".to_string(),
        );
        shadow_fields.insert("shadow_number".to_string(), shadow_number);

        // 新增 variables_get 字段定义
        let mut variables_get = HashMap::new();
        variables_get.insert("name".to_string(), "VAR".to_string());
        variables_get.insert("text".to_string(), "?".to_string());
        shadow_fields.insert("variables_get".to_string(), variables_get);

        let mut file_extensions = HashMap::new();
        file_extensions.insert("KITTEN2".to_string(), ".bcm".to_string());
        file_extensions.insert("KITTEN3".to_string(), ".bcm".to_string());
        file_extensions.insert("KITTEN4".to_string(), ".bcm4".to_string());
        file_extensions.insert("COCO".to_string(), ".json".to_string());
        file_extensions.insert("NEKO".to_string(), ".bcmkn".to_string());
        file_extensions.insert("NEMO".to_string(), String::new());
        file_extensions.insert("WOOD".to_string(), String::new());

        let mut shadow_templates = HashMap::new();
        shadow_templates.insert(
            "logic_empty".to_string(),
            ShadowTemplate {
                editable: false,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: None,
                use_custom_name: false,
                main_field: None,
            },
        );
        shadow_templates.insert(
            "logic_boolean".to_string(),
            ShadowTemplate {
                editable: false,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: None,
                use_custom_name: false,
                main_field: None,
            },
        );
        shadow_templates.insert(
            "math_number".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![
                    (
                        "constraints".to_string(),
                        "-Infinity,Infinity,0,".to_string(),
                    ),
                    ("allow_text".to_string(), "true".to_string()),
                ],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "math_angle".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![("constraints".to_string(), "0,360,0,".to_string())],
                default_text: Some("90".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "text".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "broadcast_input".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("Hi".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "lists_get".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("?".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "default_value".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![("has_been_edited".to_string(), "false".to_string())],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_audios".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("?".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_whole_audios".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("all".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_current_costume".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_current_scene".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "get_sensing_current_scene".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "controller_shadow".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![(
                    "constraints".to_string(),
                    "-Infinity,Infinity,0,false".to_string(),
                )],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "shadow_text".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some(String::new()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        shadow_templates.insert(
            "shadow_number".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![(
                    "constraints".to_string(),
                    "-Infinity,Infinity,0,".to_string(),
                )],
                default_text: Some("0".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );
        // 新增 variables_get 模板
        shadow_templates.insert(
            "variables_get".to_string(),
            ShadowTemplate {
                editable: true,
                visible: "visible".to_string(),
                extra_fields: vec![],
                default_text: Some("?".to_string()),
                use_custom_name: true,
                main_field: Some("text".to_string()),
            },
        );

        Self {
            base_url: "https://api.codemao.cn".to_string(),
            creation_base_url: "https://api-creation.codemao.cn".to_string(),
            crypto_salt: (0..31).collect(),
            default_output_dir: PathConfig::global().compile_file_path(),
            toolbox_categories: vec![
                "action",
                "advanced",
                "ai",
                "ai_game",
                "ai_lab",
                "appearance",
                "arduino",
                "audio",
                "camera",
                "cloud_list",
                "cloud_variable",
                "cognitive",
                "control",
                "data",
                "event",
                "micro_bit",
                "midi_music",
                "mobile_control",
                "operator",
                "pen",
                "physic",
                "physics2",
                "procedure",
                "sensing",
                "video",
                "wee_make",
                "wood",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            shadow_types: Arc::new(shadow_types),
            shadow_fields: Arc::new(shadow_fields),
            file_extensions: Arc::new(file_extensions),
            shadow_templates: Arc::new(shadow_templates),
        }
    }
}
