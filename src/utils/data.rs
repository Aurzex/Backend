use serde::{Deserialize, Serialize};
use serde_json::{Value, from_reader, to_string_pretty};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use thiserror::Error;

// 类型别名
type ReadType = String; // 简化为String，实际可使用enum

// 错误定义
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("类型错误: {0}")]
    TypeError(String),
    #[error("未知错误: {0}")]
    Other(String),
}

// 路径配置
pub struct PathConfig;

impl PathConfig {
    pub fn current_dir() -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    pub fn cache_dir() -> PathBuf {
        Self::current_dir().join("cache")
    }

    pub fn data_dir() -> PathBuf {
        Self::current_dir().join("data")
    }

    pub fn download_dir() -> PathBuf {
        Self::current_dir().join("download")
    }

    pub fn js_dir() -> PathBuf {
        Self::current_dir().join("js_module")
    }

    pub fn compile_file_path() -> PathBuf {
        Self::download_dir().join("compile")
    }

    pub fn fiction_file_path() -> PathBuf {
        Self::download_dir().join("fiction")
    }

    pub fn cache_file_path() -> PathBuf {
        Self::cache_dir().join("info.json")
    }

    pub fn captcha_file_path() -> PathBuf {
        Self::cache_dir().join("captcha.jpg")
    }

    pub fn data_file_path() -> PathBuf {
        Self::data_dir().join("data.json")
    }

    pub fn history_file_path() -> PathBuf {
        Self::cache_dir().join("history.json")
    }

    pub fn setting_file_path() -> PathBuf {
        Self::data_dir().join("setting.json")
    }

    pub fn token_file_path() -> PathBuf {
        Self::data_dir().join("token.txt")
    }

    pub fn password_file_path() -> PathBuf {
        Self::data_dir().join("password.txt")
    }

    pub fn ensure_directories() -> Result<(), ConfigError> {
        fs::create_dir_all(Self::cache_dir())?;
        fs::create_dir_all(Self::data_dir())?;
        fs::create_dir_all(Self::download_dir())?;
        Ok(())
    }
}

// 数据类定义
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountData {
    #[serde(default)]
    pub author_level: String,
    #[serde(default)]
    pub create_time: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub identity: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserData {
    #[serde(default)]
    pub ads: Vec<String>,
    #[serde(default)]
    pub answers: Vec<HashMap<String, Value>>,
    #[serde(default)]
    pub black_room: Vec<String>,
    #[serde(default)]
    pub comments: Vec<String>,
    #[serde(default)]
    pub emojis: Vec<String>,
    #[serde(default)]
    pub replies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMaoData {
    #[serde(default)]
    pub account_data: AccountData,
    #[serde(default)]
    pub info: HashMap<String, String>,
    #[serde(default)]
    pub user_data: UserData,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtraBody {
    #[serde(default)]
    pub enable_search: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct More {
    #[serde(default)]
    pub extra_body: ExtraBody,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Parameter {
    #[serde(default)]
    pub all_read_type: Vec<String>,
    #[serde(default)]
    pub log: bool,
    #[serde(default)]
    pub password_login_method: String,
    #[serde(default)]
    pub report_work_max: i32,
    #[serde(default)]
    pub spam_del_max: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Program {
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub member: String,
    #[serde(default)]
    pub slogan: String,
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UploadHistory {
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub file_size: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub save_url: String,
    #[serde(default)]
    pub upload_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMaoCache {
    #[serde(default)]
    pub collected: i32,
    #[serde(default)]
    pub fans: i32,
    #[serde(default)]
    pub level: i32,
    #[serde(default)]
    pub liked: i32,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub timestamp: i64,
    #[serde(default)]
    pub user_id: i64,
    #[serde(default)]
    pub view: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMaoSetting {
    #[serde(default)]
    pub parameter: Parameter,
    #[serde(default)]
    pub program: Program,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodemaoHistory {
    #[serde(default)]
    pub history: Vec<UploadHistory>,
}

// 默认数据
pub fn default_setting_data() -> CodeMaoSetting {
    let mut headers = HashMap::new();
    headers.insert(
        "Accept-Encoding".to_string(),
        "gzip, deflate, br, zstd".to_string(),
    );
    headers.insert(
        "Accept-Language".to_string(),
        "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6".to_string(),
    );
    headers.insert("User-Agent".to_string(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0".to_string());

    CodeMaoSetting {
        parameter: Parameter {
            all_read_type: vec![
                "COMMENT_REPLY".to_string(),
                "LIKE_FORK".to_string(),
                "SYSTEM".to_string(),
            ],
            log: false,
            password_login_method: "token".to_string(),
            report_work_max: 8,
            spam_del_max: 3,
        },
        program: Program {
            author: "Aurzex".to_string(),
            headers,
            member: "Aurzex, MoonLeaaaf, Nomen, MiTao, DontLoveBy".to_string(),
            slogan: "欢迎使用 Aumiao-PY! 你说的对, 但是《Aumiao》是一款由 Aumiao 开发团队开发的编程猫自动化工具, 于 2023 年 5 月 2 日发布 工具以编程猫宇宙为舞台, 玩家可以扮演扮演毛毡用户, 在这个社区毛线坍缩并邂逅各种不同的乐子人 在领悟了《猫站圣经》后, 打败强敌扫厕所, 在维护编程猫核邪铀删的局面的同时, 逐步揭开编程猫社区的真相".to_string(),
            team: "Aumiao Team".to_string(),
            version: "2.7.0".to_string(),
        },
    }
}

pub fn default_data_data() -> CodeMaoData {
    let mut answers = Vec::new();
    let mut answer1 = HashMap::new();
    answer1.insert(
        "牢大".to_string(),
        Value::String("孩子们, 我回来了".to_string()),
    );
    answers.push(answer1);

    let mut answer2 = HashMap::new();
    answer2.insert("奶龙".to_string(), Value::String("我才是奶龙".to_string()));
    answers.push(answer2);

    let mut answer3 = HashMap::new();
    answer3.insert(
        "name".to_string(),
        Value::String("I'm {nickname}".to_string()),
    );
    answers.push(answer3);

    let mut answer4 = HashMap::new();
    answer4.insert(
        "QQ".to_string(),
        Value::String("It's {qq_number}".to_string()),
    );
    answers.push(answer4);

    let mut answer5 = HashMap::new();
    let reasons = vec![
        "不许你黑我家鸽鸽!😡",
        "想要绿尸函了食不食?",
        "香精煎鱼食不食?",
    ];
    answer5.insert(
        "只因".to_string(),
        Value::Array(
            reasons
                .iter()
                .map(|s| Value::String(s.to_string()))
                .collect(),
        ),
    );
    answers.push(answer5);

    let mut info = HashMap::new();
    info.insert("e_mail".to_string(), "zybqw@qq.com".to_string());
    info.insert("nickname".to_string(), "喵鱼 a".to_string());
    info.insert("qq_number".to_string(), "3611198191".to_string());

    CodeMaoData {
        account_data: AccountData {
            author_level: "1".to_string(),
            create_time: "1800000000".to_string(),
            description: "".to_string(),
            id: 1742185446,
            identity: "********".to_string(),
            nickname: " 猫猫捏 ".to_string(),
            password: "******".to_string(),
        },
        info,
        user_data: UserData {
            ads: vec![
                "codemao.cn/work".to_string(),
                "cpdd".to_string(),
                "scp".to_string(),
                "不喜可删".to_string(),
                "互关".to_string(),
                "互赞".to_string(),
                "交友".to_string(),
                "光头强".to_string(),
                "关注".to_string(),
                "再创作".to_string(),
                "冲传说".to_string(),
                "冲大佬".to_string(),
                "冲高手".to_string(),
                "协作项目".to_string(),
                "基金会".to_string(),
                "处cp".to_string(),
                "家族招人".to_string(),
                "我的作品".to_string(),
                "戴雨默".to_string(),
                "所有作品".to_string(),
                "扫厕所".to_string(),
                "找徒弟".to_string(),
                "找闺".to_string(),
                "招人".to_string(),
                "有赞必回".to_string(),
                "点个".to_string(),
                "爬虫".to_string(),
                "看一下我的".to_string(),
                "看我的".to_string(),
                "看看我的".to_string(),
                "粘贴到别人作品".to_string(),
                "赞我".to_string(),
                "转发".to_string(),
            ],
            answers,
            black_room: vec![
                "2233".to_string(),
                "114514".to_string(),
                "1919810".to_string(),
            ],
            comments: vec![
                "666".to_string(),
                "不错不错".to_string(),
                "前排:P".to_string(),
                "加油!:O".to_string(),
                "沙发 */ω\\*".to_string(),
                "针不戳:D".to_string(),
            ],
            emojis: vec![
                "星能猫_好吃".to_string(),
                "星能猫_耶".to_string(),
                "编程猫_666".to_string(),
                "编程猫_加油".to_string(),
                "编程猫_好厉害".to_string(),
                "编程猫_我来啦".to_string(),
                "编程猫_打call".to_string(),
                "编程猫_抱大腿".to_string(),
                "编程猫_棒".to_string(),
                "编程猫_点手机".to_string(),
                "编程猫_爱心".to_string(),
                "编程猫_爱心".to_string(),
                "雷电猴_哇塞".to_string(),
                "雷电猴_哈哈哈".to_string(),
                "雷电猴_嘻嘻嘻".to_string(),
                "雷电猴_围观".to_string(),
                "魔术喵_开心".to_string(),
                "魔术喵_收藏".to_string(),
                "魔术喵_点赞".to_string(),
                "魔术喵_点赞".to_string(),
                "魔术喵_魔术".to_string(),
            ],
            replies: vec![
                "{nickname} 很忙 oh, 机器人来凑热闹 (*^^*)".to_string(),
                "{nickname} 的自动回复来喽".to_string(),
                "嗨嗨嗨! 这事 {nickname} の自动回复鸭!".to_string(),
                "对不起,{nickname} 它又搞忘了时间, 一定是在忙呢".to_string(),
                "这是 {nickname} 的自动回复, 不知道你在说啥 (".to_string(),
            ],
        },
    }
}

// JSON文件处理器
pub struct JsonFileHandler;

impl JsonFileHandler {
    pub fn load_json_file<T>(path: &Path, create_if_missing: bool) -> Result<T, ConfigError>
    where
        T: for<'de> Deserialize<'de> + Default + Serialize, // 添加 Serialize 约束
    {
        if !path.exists() {
            if create_if_missing {
                println!("文件 {} 不存在, 使用默认值创建...", path.display());
                let instance = T::default();
                Self::save_json_file(path, &instance)?;
                return Ok(instance);
            }
            return Ok(T::default());
        }

        let file = fs::File::open(path)?;
        match from_reader(file) {
            Ok(data) => Ok(data),
            Err(e) => {
                println!("加载 {} 错误: {}", path.display(), e);
                println!("使用默认值...");
                Ok(T::default())
            }
        }
    }

    pub fn save_json_file<T>(path: &Path, data: &T) -> Result<(), ConfigError>
    where
        T: Serialize,
    {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = path.with_extension("tmp");
        let json_str = to_string_pretty(data)?;
        fs::write(&temp_path, json_str)?;
        fs::rename(&temp_path, path)?;
        println!("文件 {} 已保存", path.display());
        Ok(())
    }
}

// 基础管理器
pub struct BaseManager<T> {
    file_path: PathBuf,
    data: RwLock<Option<T>>,
}

impl<T> BaseManager<T>
where
    T: for<'de> Deserialize<'de> + Serialize + Default + Clone,
{
    pub fn new(file_path: PathBuf) -> Self {
        let manager = Self {
            file_path,
            data: RwLock::new(None),
        };

        // 确保文件存在
        if !manager.file_path.exists() {
            let _ = JsonFileHandler::load_json_file::<T>(&manager.file_path, true);
        }

        manager
    }

    pub fn data(&self) -> Result<T, ConfigError> {
        let mut data = self.data.write().unwrap();
        if data.is_none() {
            *data = Some(JsonFileHandler::load_json_file(&self.file_path, true)?);
        }
        Ok(data.as_ref().unwrap().clone())
    }

    pub fn update(&self, new_data: HashMap<String, Value>) -> Result<(), ConfigError> {
        let mut current = self.data()?;

        // 简单实现：将current转为Value，合并new_data，再转回T
        let mut current_value = serde_json::to_value(&current)?;

        if let Value::Object(ref mut map) = current_value {
            for (key, value) in new_data {
                map.insert(key, value);
            }
        }

        // 从Value转换回T
        let json_str = serde_json::to_string(&current_value)?;
        current = serde_json::from_str(&json_str)?;

        *self.data.write().unwrap() = Some(current.clone());
        self.save()?;
        Ok(())
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let data = self.data()?;
        JsonFileHandler::save_json_file(&self.file_path, &data)?;
        Ok(())
    }

    pub fn reload(&self) -> Result<(), ConfigError> {
        *self.data.write().unwrap() = None;
        Ok(())
    }
}

// 单例管理器 - 使用 std::sync::OnceLock 替代 once_cell::sync::Lazy
pub struct DataManager {
    inner: BaseManager<CodeMaoData>,
}

impl DataManager {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<DataManager> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            PathConfig::ensure_directories().unwrap();
            DataManager {
                inner: BaseManager::new(PathConfig::data_file_path()),
            }
        })
    }

    pub fn data(&self) -> Result<CodeMaoData, ConfigError> {
        self.inner.data()
    }

    pub fn update(&self, new_data: HashMap<String, Value>) -> Result<(), ConfigError> {
        self.inner.update(new_data)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.inner.save()
    }

    pub fn reload(&self) -> Result<(), ConfigError> {
        self.inner.reload()
    }
}

pub struct CacheManager {
    inner: BaseManager<CodeMaoCache>,
}

impl CacheManager {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<CacheManager> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            PathConfig::ensure_directories().unwrap();
            CacheManager {
                inner: BaseManager::new(PathConfig::cache_file_path()),
            }
        })
    }

    pub fn data(&self) -> Result<CodeMaoCache, ConfigError> {
        self.inner.data()
    }

    pub fn update(&self, new_data: HashMap<String, Value>) -> Result<(), ConfigError> {
        self.inner.update(new_data)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.inner.save()
    }

    pub fn reload(&self) -> Result<(), ConfigError> {
        self.inner.reload()
    }
}

pub struct SettingManager {
    inner: BaseManager<CodeMaoSetting>,
}

impl SettingManager {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<SettingManager> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            PathConfig::ensure_directories().unwrap();
            SettingManager {
                inner: BaseManager::new(PathConfig::setting_file_path()),
            }
        })
    }

    pub fn data(&self) -> Result<CodeMaoSetting, ConfigError> {
        self.inner.data()
    }

    pub fn update(&self, new_data: HashMap<String, Value>) -> Result<(), ConfigError> {
        self.inner.update(new_data)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.inner.save()
    }

    pub fn reload(&self) -> Result<(), ConfigError> {
        self.inner.reload()
    }
}

pub struct HistoryManager {
    inner: BaseManager<CodemaoHistory>,
}

impl HistoryManager {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<HistoryManager> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            PathConfig::ensure_directories().unwrap();
            HistoryManager {
                inner: BaseManager::new(PathConfig::history_file_path()),
            }
        })
    }

    pub fn data(&self) -> Result<CodemaoHistory, ConfigError> {
        self.inner.data()
    }

    pub fn update(&self, new_data: HashMap<String, Value>) -> Result<(), ConfigError> {
        self.inner.update(new_data)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.inner.save()
    }

    pub fn reload(&self) -> Result<(), ConfigError> {
        self.inner.reload()
    }
}

// 文件写入相关
pub struct CodeMaoFile;

impl CodeMaoFile {
    pub fn file_write(path: &Path, content: &FileContent, method: &str) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        match content {
            FileContent::Text(s) => {
                if method.contains('b') {
                    return Err(ConfigError::TypeError(format!(
                        "文本内容不能使用二进制模式: {}",
                        method
                    )));
                }
                fs::write(path, s)?;
            }
            FileContent::Bytes(b) => {
                fs::write(path, b)?;
            }
            FileContent::Json(obj) => {
                if method.contains('b') {
                    return Err(ConfigError::TypeError(format!(
                        "JSON内容不能使用二进制模式: {}",
                        method
                    )));
                }
                let json_str = serde_json::to_string_pretty(obj)?;
                fs::write(path, json_str)?;
            }
            FileContent::Lines(lines) => {
                if method.contains('b') {
                    return Err(ConfigError::TypeError(format!(
                        "文本内容不能使用二进制模式: {}",
                        method
                    )));
                }
                let content = lines.join("\n");
                fs::write(path, content)?;
            }
        }

        Ok(())
    }
}

pub enum FileContent {
    Text(String),
    Bytes(Vec<u8>),
    Json(Value),
    Lines(Vec<String>),
}

// 嵌套默认字典
#[derive(Debug, Clone)]
pub struct NestedDefaultDict {
    data: HashMap<String, Value>,
}

impl NestedDefaultDict {
    pub fn new(data: HashMap<String, Value>) -> Self {
        Self { data }
    }

    pub fn get(&self, key: &str) -> Value {
        self.data
            .get(key)
            .cloned()
            .unwrap_or(Value::String("UNKNOWN".to_string()))
    }

    pub fn to_dict(&self) -> HashMap<String, Value> {
        self.data.clone()
    }
}

// 初始化函数
pub fn initialize_config_files() -> Result<(), ConfigError> {
    println!("正在初始化配置文件...");

    PathConfig::ensure_directories()?;

    // 设置文件
    if !PathConfig::setting_file_path().exists() {
        println!(
            "创建配置文件: {}",
            PathConfig::setting_file_path().display()
        );
        let data = default_setting_data();
        JsonFileHandler::save_json_file(&PathConfig::setting_file_path(), &data)?;
    } else {
        println!(
            "配置文件已存在: {}",
            PathConfig::setting_file_path().display()
        );
    }

    // 数据文件
    if !PathConfig::data_file_path().exists() {
        println!("创建配置文件: {}", PathConfig::data_file_path().display());
        let data = default_data_data();
        JsonFileHandler::save_json_file(&PathConfig::data_file_path(), &data)?;
    } else {
        println!("配置文件已存在: {}", PathConfig::data_file_path().display());
    }

    // 缓存文件
    if !PathConfig::cache_file_path().exists() {
        println!("创建配置文件: {}", PathConfig::cache_file_path().display());
        JsonFileHandler::save_json_file(&PathConfig::cache_file_path(), &CodeMaoCache::default())?;
    } else {
        println!(
            "配置文件已存在: {}",
            PathConfig::cache_file_path().display()
        );
    }

    // 历史文件
    if !PathConfig::history_file_path().exists() {
        println!(
            "创建配置文件: {}",
            PathConfig::history_file_path().display()
        );
        JsonFileHandler::save_json_file(
            &PathConfig::history_file_path(),
            &CodemaoHistory::default(),
        )?;
    } else {
        println!(
            "配置文件已存在: {}",
            PathConfig::history_file_path().display()
        );
    }

    println!("配置文件初始化完成!");
    Ok(())
}
