use std::collections::{HashMap, VecDeque};
use std::net::TcpStream;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{WebSocket, connect};

use crate::api::auth::CloudAuthenticator;

/// WebSocket 流类型别名（tungstenite + rustls）。
type WsStream = MaybeTlsStream<TcpStream>;
type Ws = WebSocket<WsStream>;

// ==================== 常量配置 ====================

/// 云存储 WebSocket 服务器地址。
pub const CLOUD_WS_BASE_URL: &str = "wss://socketcv.codemao.cn:9096/cloudstorage/";
/// Socket.IO 帧前缀。
const HANDSHAKE_PREFIX: &str = "0";
const CONNECTED_MESSAGE: &str = "40";
const SERVER_CLOSE_PREFIX: &str = "41";
const EVENT_MESSAGE_PREFIX: &str = "42";
const PING_MESSAGE: &str = "2";
const PONG_MESSAGE: &str = "3";
/// 批量上传合并间隔。
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
/// 默认重连间隔与最大次数。
const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(8);
const DEFAULT_MAX_RECONNECT_ATTEMPTS: usize = 5;
/// 排行榜限制范围。
pub const MIN_RANKING_LIMIT: i64 = 1;
pub const MAX_RANKING_LIMIT: i64 = 31;
pub const ASCENDING_ORDER: i64 = 1;
pub const DESCENDING_ORDER: i64 = -1;

// ==================== 错误类型 ====================

/// 云存储操作错误。
#[derive(Debug, Error)]
pub enum CloudError {
    #[error("WebSocket 错误: {0}")]
    WebSocket(#[from] Box<tungstenite::Error>),
    #[error("HTTP 握手失败: {0}")]
    Handshake(String),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("连接未就绪")]
    NotConnected,
    #[error("变量未找到: {0}")]
    VariableNotFound(String),
    #[error("列表未找到: {0}")]
    ListNotFound(String),
    #[error("无效参数: {0}")]
    InvalidArgument(String),
    #[error("鉴权错误: {0}")]
    Auth(String),
    #[error("线程错误: {0}")]
    Thread(String),
}

impl From<tungstenite::Error> for CloudError {
    fn from(err: tungstenite::Error) -> Self {
        CloudError::WebSocket(Box::new(err))
    }
}

/// 本模块统一的 `Result` 别名。
pub type Result<T> = std::result::Result<T, CloudError>;

// ==================== 基础类型 ====================

/// 编辑器类型，决定 WebSocket 查询参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorType {
    #[default]
    Kitten,
    Nemo,
    KittenN,
    Coco,
}

impl EditorType {
    /// 返回 `(authorization_type, stag)` 查询参数。
    pub fn query_params(self) -> (&'static str, &'static str) {
        match self {
            EditorType::Nemo => ("5", "2"),
            EditorType::Kitten | EditorType::Coco => ("1", "1"),
            EditorType::KittenN => ("5", "3"),
        }
    }
}

/// 云数据值类型：整数或字符串（与 Python `CloudValueType` 对应）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CloudValue {
    Number(i64),
    Text(String),
}

impl CloudValue {
    /// 云协议要求的 `param_type`。
    pub fn param_type(&self) -> &'static str {
        match self {
            CloudValue::Number(_) => "number",
            CloudValue::Text(_) => "string",
        }
    }

    /// 从 JSON 值转换（布尔/浮点按 Python 语义尽量转整数，否则转为文本）。
    pub fn from_json(v: &Value) -> CloudValue {
        match v {
            Value::Number(n) => n
                .as_i64()
                .map(CloudValue::Number)
                .unwrap_or_else(|| CloudValue::Text(n.to_string())),
            Value::String(s) => CloudValue::Text(s.clone()),
            Value::Bool(b) => CloudValue::Number(*b as i64),
            other => CloudValue::Text(other.to_string()),
        }
    }
}

impl std::fmt::Display for CloudValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudValue::Number(n) => write!(f, "{n}"),
            CloudValue::Text(s) => write!(f, "{s}"),
        }
    }
}

impl From<i64> for CloudValue {
    fn from(v: i64) -> Self {
        CloudValue::Number(v)
    }
}

impl From<String> for CloudValue {
    fn from(v: String) -> Self {
        CloudValue::Text(v)
    }
}

impl From<&str> for CloudValue {
    fn from(v: &str) -> Self {
        CloudValue::Text(v.to_string())
    }
}

/// 回调句柄，用于取消注册回调。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackHandle(pub(crate) usize);

/// 变量/列表变更来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// 本地操作产生。
    Local,
    /// 云端推送产生。
    Cloud,
}

impl ChangeSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChangeSource::Local => "local",
            ChangeSource::Cloud => "cloud",
        }
    }
}

/// 连接事件（供 `on_connection` 回调使用）。
#[derive(Debug, Clone)]
pub enum ConnectionEvent {
    Opened,
    Closed { was_connected: bool },
    Error(String),
    ServerClosed(String),
}

/// 排行榜条目中的用户信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingUser {
    pub id: i64,
    pub nickname: String,
    pub avatar_url: String,
}

/// 排行榜条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingItem {
    pub value: CloudValue,
    pub user: RankingUser,
}

/// 一次排行榜查询结果。
#[derive(Debug, Clone, Default)]
pub struct RankingData {
    pub cvid: String,
    pub name: String,
    pub items: Vec<RankingItem>,
}

// 回调类型别名
type ChangeCallback = Box<dyn Fn(&CloudValue, &CloudValue, &str) + Send + Sync>;
type ListChangeCallback = Box<dyn Fn(&[CloudValue], &[CloudValue], &str) + Send + Sync>;
type OperationCallback = Box<dyn Fn(&str, &[CloudValue]) + Send + Sync>;
type RankingCallback = Box<dyn Fn(RankingData) + Send + Sync>;
type ReadyCallback = Box<dyn Fn() + Send + Sync>;
type OnlineUsersCallback = Box<dyn Fn(i64, i64) + Send + Sync>;
type ConnectionCallback = Box<dyn Fn(ConnectionEvent) + Send + Sync>;

// ==================== 命令模式 + 工厂 ====================

/// 云数据更新命令（命令模式：请求对象化，可排队、批量执行）。
///
/// 由 [`CommandFactory`] 创建。
#[derive(Debug, Clone)]
pub enum CloudCommand {
    /// 变量更新：私有/公有。
    Variable { private: bool, data: Value },
    /// 列表更新：cvid + 操作序列。
    List { cvid: String, ops: Vec<Value> },
}

/// 命令工厂：集中创建云数据更新命令。
pub struct CommandFactory;

impl CommandFactory {
    /// 创建私有变量更新命令。
    pub fn update_private_variable(cvid: &str, value: &CloudValue) -> CloudCommand {
        CloudCommand::Variable {
            private: true,
            data: json!({
                "cvid": cvid,
                "value": value,
                "param_type": value.param_type(),
            }),
        }
    }

    /// 创建公有变量更新命令。
    pub fn update_public_variable(cvid: &str, value: &CloudValue) -> CloudCommand {
        CloudCommand::Variable {
            private: false,
            data: json!({
                "action": "set",
                "cvid": cvid,
                "value": value,
                "param_type": value.param_type(),
            }),
        }
    }

    /// 创建列表更新命令。
    pub fn update_list(cvid: &str, ops: Vec<Value>) -> CloudCommand {
        CloudCommand::List {
            cvid: cvid.to_string(),
            ops,
        }
    }
}

/// 批量合并后的上传载荷。
#[derive(Debug, Default)]
pub struct BatchedUploads {
    pub private_updates: Vec<Value>,
    pub public_updates: Vec<Value>,
    pub list_updates: Vec<(String, Vec<Value>)>,
}

/// 将待上传命令合并为最少次数的网络请求：
/// 私有/公有变量各合并为一条消息，列表按 cvid 合并操作序列。
pub fn merge_commands(commands: Vec<CloudCommand>) -> BatchedUploads {
    let mut out = BatchedUploads::default();
    for cmd in commands {
        match cmd {
            CloudCommand::Variable { data, .. } => {
                // 通过 data 是否含 "action":"set" 区分公有/私有
                let is_public = data.get("action").and_then(Value::as_str) == Some("set");
                if is_public {
                    out.public_updates.push(data);
                } else {
                    out.private_updates.push(data);
                }
            }
            CloudCommand::List { cvid, ops } => {
                if let Some((_, existing)) = out
                    .list_updates
                    .iter_mut()
                    .find(|(existing_cvid, _)| existing_cvid == &cvid)
                {
                    existing.extend(ops);
                } else {
                    out.list_updates.push((cvid, ops));
                }
            }
        }
    }
    out
}

// ==================== 数据模型 ====================

/// 变量种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarKind {
    Private,
    Public,
}

/// 单个云变量（内部存储 + 回调）。
struct VariableData {
    cvid: String,
    name: String,
    value: CloudValue,
    callbacks: Vec<(usize, ChangeCallback)>,
    ranking_callbacks: Vec<(usize, RankingCallback)>,
    next_cb_id: usize,
}

impl VariableData {
    fn new(cvid: String, name: String, value: CloudValue) -> Self {
        Self {
            cvid,
            name,
            value,
            callbacks: Vec::new(),
            ranking_callbacks: Vec::new(),
            next_cb_id: 0,
        }
    }
}

/// 单个云列表（内部存储 + 回调）。
struct ListData {
    cvid: String,
    name: String,
    items: Vec<CloudValue>,
    change_callbacks: Vec<(usize, ListChangeCallback)>,
    operation_callbacks: HashMap<String, Vec<(usize, OperationCallback)>>,
    next_cb_id: usize,
}

impl ListData {
    fn new(cvid: String, name: String, items: Vec<CloudValue>) -> Self {
        Self {
            cvid,
            name,
            items,
            change_callbacks: Vec::new(),
            operation_callbacks: HashMap::new(),
            next_cb_id: 0,
        }
    }
}

impl std::fmt::Debug for VariableData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VariableData")
            .field("cvid", &self.cvid)
            .field("name", &self.name)
            .field("value", &self.value)
            .field("callbacks", &self.callbacks.len())
            .finish()
    }
}

impl std::fmt::Debug for ListData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListData")
            .field("cvid", &self.cvid)
            .field("name", &self.name)
            .field("items", &self.items)
            .field("change_callbacks", &self.change_callbacks.len())
            .field("operation_callbacks", &self.operation_callbacks.len())
            .finish()
    }
}

/// 云数据存储：按名字索引，另维护 cvid → 名字 映射以支持数字 id 查询。
#[derive(Default)]
struct DataStore {
    private_vars: HashMap<String, VariableData>,
    public_vars: HashMap<String, VariableData>,
    lists: HashMap<String, ListData>,
    private_cvid: HashMap<String, String>,
    public_cvid: HashMap<String, String>,
    list_cvid: HashMap<String, String>,
}

impl DataStore {
    fn variable_mut(&mut self, kind: VarKind, key: &str) -> Option<&mut VariableData> {
        match kind {
            VarKind::Private => Self::variable_in(&mut self.private_vars, &self.private_cvid, key),
            VarKind::Public => Self::variable_in(&mut self.public_vars, &self.public_cvid, key),
        }
    }

    fn variable(&self, kind: VarKind, key: &str) -> Option<&VariableData> {
        match kind {
            VarKind::Private => Self::variable_ref(&self.private_vars, &self.private_cvid, key),
            VarKind::Public => Self::variable_ref(&self.public_vars, &self.public_cvid, key),
        }
    }

    fn variable_in<'a>(
        vars: &'a mut HashMap<String, VariableData>,
        cvid: &HashMap<String, String>,
        key: &str,
    ) -> Option<&'a mut VariableData> {
        if vars.contains_key(key) {
            return vars.get_mut(key);
        }
        cvid.get(key).and_then(|name| vars.get_mut(name))
    }

    fn variable_ref<'a>(
        vars: &'a HashMap<String, VariableData>,
        cvid: &HashMap<String, String>,
        key: &str,
    ) -> Option<&'a VariableData> {
        if let Some(v) = vars.get(key) {
            return Some(v);
        }
        cvid.get(key).and_then(|name| vars.get(name))
    }

    fn list_mut(&mut self, key: &str) -> Option<&mut ListData> {
        if self.lists.contains_key(key) {
            return self.lists.get_mut(key);
        }
        self.list_cvid
            .get(key)
            .and_then(|name| self.lists.get_mut(name))
    }

    fn list(&self, key: &str) -> Option<&ListData> {
        if let Some(l) = self.lists.get(key) {
            return Some(l);
        }
        self.list_cvid
            .get(key)
            .and_then(|name| self.lists.get(name))
    }

    fn create_private(&mut self, cvid: String, name: String, value: CloudValue) {
        self.private_cvid.insert(cvid.clone(), name.clone());
        if let Some(existing) = self.private_vars.get_mut(&name) {
            // 重连全量重建时保留已注册的回调，仅更新值与 cvid
            existing.cvid = cvid;
            existing.value = value;
            return;
        }
        self.private_vars
            .insert(name.clone(), VariableData::new(cvid, name, value));
    }

    fn create_public(&mut self, cvid: String, name: String, value: CloudValue) {
        self.public_cvid.insert(cvid.clone(), name.clone());
        if let Some(existing) = self.public_vars.get_mut(&name) {
            existing.cvid = cvid;
            existing.value = value;
            return;
        }
        self.public_vars
            .insert(name.clone(), VariableData::new(cvid, name, value));
    }

    fn create_list(&mut self, cvid: String, name: String, items: Vec<CloudValue>) {
        self.list_cvid.insert(cvid.clone(), name.clone());
        if let Some(existing) = self.lists.get_mut(&name) {
            existing.cvid = cvid;
            existing.items = items;
            return;
        }
        self.lists
            .insert(name.clone(), ListData::new(cvid, name, items));
    }

    fn clear_all(&mut self) {
        self.private_vars.clear();
        self.public_vars.clear();
        self.lists.clear();
        self.private_cvid.clear();
        self.public_cvid.clear();
        self.list_cvid.clear();
    }
}

impl std::fmt::Debug for DataStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataStore")
            .field("private_vars", &self.private_vars)
            .field("public_vars", &self.public_vars)
            .field("lists", &self.lists)
            .finish()
    }
}

// ==================== 事件存储 ====================

/// 泛型回调存储：支持按句柄增删，触发时整体取出。
struct CallbackStore<T> {
    next_id: usize,
    items: Vec<(usize, T)>,
}

impl<T> std::fmt::Debug for CallbackStore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackStore")
            .field("next_id", &self.next_id)
            .field("count", &self.items.len())
            .finish()
    }
}

impl<T> Default for CallbackStore<T> {
    fn default() -> Self {
        Self {
            next_id: 0,
            items: Vec::new(),
        }
    }
}

impl<T> CallbackStore<T> {
    fn add(&mut self, cb: T) -> CallbackHandle {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push((id, cb));
        CallbackHandle(id)
    }

    fn remove(&mut self, handle: CallbackHandle) {
        self.items.retain(|(id, _)| *id != handle.0);
    }

    /// 取出全部回调（锁外执行后由调用方放回）。
    fn take_all(&mut self) -> Vec<(usize, T)> {
        std::mem::take(&mut self.items)
    }
}

/// 连接级事件回调集合。
#[derive(Default)]
struct Events {
    data_ready: CallbackStore<ReadyCallback>,
    online_users: CallbackStore<OnlineUsersCallback>,
    ranking: CallbackStore<RankingCallback>,
    connection: CallbackStore<ConnectionCallback>,
}

impl std::fmt::Debug for Events {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Events")
            .field("data_ready", &self.data_ready)
            .field("online_users", &self.online_users)
            .field("ranking", &self.ranking)
            .field("connection", &self.connection)
            .finish()
    }
}

/// 条件变量通知器，供 `wait_for_*` 使用。
#[derive(Default)]
struct Notify {
    lock: Mutex<()>,
    cond: Condvar,
}

impl Notify {
    /// 持锁设置状态并通知等待者，避免 Condvar 丢失唤醒（与 [`wait_flag`] 的
    /// 检查-等待原子性配合，消除状态型等待的竞态窗口）。
    fn notify_with<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.lock.lock().unwrap();
        let result = f();
        self.cond.notify_all();
        result
    }
}

// ==================== 连接核心 ====================

/// 云连接共享内部状态。
struct CloudInner {
    work_id: i64,
    editor: EditorType,
    token: Option<String>,
    auto_reconnect: AtomicBool,
    max_reconnect_attempts: usize,
    reconnect_interval: Duration,
    flush_interval: Duration,
    stopping: AtomicBool,
    connected: AtomicBool,
    data_ready: AtomicBool,
    /// Socket.IO 握手已完成（收到 40），flush 线程据此判断可安全发送。
    io_ready: AtomicBool,
    online_users: AtomicI64,
    reconnect_attempts: AtomicUsize,
    /// JOIN 消息已发送标记（防止重复 join 被服务器拒绝）。
    join_sent: AtomicBool,
    /// 发送端：读线程持有 WebSocket 独占，写操作经 channel 转发。
    tx: Mutex<Option<mpsc::Sender<Message>>>,
    read_join: Mutex<Option<JoinHandle<()>>>,
    flush_join: Mutex<Option<JoinHandle<()>>>,
    /// 建立连接的互斥锁：防止 connect() 与自动重连并发执行 establish。
    connect_lock: Mutex<()>,
    commands: Mutex<VecDeque<CloudCommand>>,
    state: Mutex<DataStore>,
    pending_rankings: Mutex<VecDeque<String>>,
    events: Mutex<Events>,
    notify: Notify,
    last_activity: Mutex<Instant>,
}

impl std::fmt::Debug for CloudInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudInner")
            .field("work_id", &self.work_id)
            .field("editor", &self.editor)
            .field("connected", &self.connected.load(Ordering::Acquire))
            .field("data_ready", &self.data_ready.load(Ordering::Acquire))
            .field("online_users", &self.online_users.load(Ordering::Acquire))
            .field("pending_commands", &self.commands.lock().unwrap().len())
            .finish()
    }
}

/// 云连接句柄：线程安全，可克隆共享。
#[derive(Clone)]
pub struct CloudConnection {
    inner: Arc<CloudInner>,
}

impl std::fmt::Debug for CloudConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudConnection")
            .field("work_id", &self.inner.work_id)
            .field("editor", &self.inner.editor)
            .field("connected", &self.is_connected())
            .field("data_ready", &self.is_data_ready())
            .field("online_users", &self.online_users())
            .finish()
    }
}

/// 建造者模式：链式配置后构造 [`CloudConnection`]。
#[derive(Debug, Clone)]
pub struct CloudBuilder {
    work_id: i64,
    editor: Option<EditorType>,
    authorization_token: Option<String>,
    auto_reconnect: bool,
    max_reconnect_attempts: usize,
    reconnect_interval: Duration,
    flush_interval: Duration,
}

impl CloudBuilder {
    /// 以作品 ID 创建建造者。
    pub fn new(work_id: i64) -> Self {
        Self {
            work_id,
            editor: None,
            authorization_token: None,
            auto_reconnect: true,
            max_reconnect_attempts: DEFAULT_MAX_RECONNECT_ATTEMPTS,
            reconnect_interval: DEFAULT_RECONNECT_INTERVAL,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
        }
    }

    /// 显式指定编辑器类型（缺省按作品推断为 Kitten）。
    pub fn editor(mut self, editor: EditorType) -> Self {
        self.editor = Some(editor);
        self
    }

    /// 设置授权令牌（写入 `Cookie: Authorization=...`）。
    pub fn authorization_token(mut self, token: impl Into<String>) -> Self {
        self.authorization_token = Some(token.into());
        self
    }

    /// 是否自动重连（默认开启）。
    pub fn auto_reconnect(mut self, enabled: bool) -> Self {
        self.auto_reconnect = enabled;
        self
    }

    /// 最大重连次数（默认 5）。
    pub fn max_reconnect_attempts(mut self, attempts: usize) -> Self {
        self.max_reconnect_attempts = attempts;
        self
    }

    /// 重连基础间隔（指数退避，默认 8s）。
    pub fn reconnect_interval(mut self, interval: Duration) -> Self {
        self.reconnect_interval = interval;
        self
    }

    /// 批量上传合并间隔（默认 100ms）。
    pub fn flush_interval(mut self, interval: Duration) -> Self {
        self.flush_interval = interval;
        self
    }

    /// 构建云连接（尚未连接，需调用 [`CloudConnection::connect`]）。
    pub fn build(self) -> CloudConnection {
        let inner = CloudInner {
            work_id: self.work_id,
            editor: self.editor.unwrap_or_default(),
            token: self.authorization_token,
            auto_reconnect: AtomicBool::new(self.auto_reconnect),
            max_reconnect_attempts: self.max_reconnect_attempts,
            reconnect_interval: self.reconnect_interval,
            flush_interval: self.flush_interval,
            stopping: AtomicBool::new(true),
            connected: AtomicBool::new(false),
            data_ready: AtomicBool::new(false),
            io_ready: AtomicBool::new(false),
            online_users: AtomicI64::new(0),
            reconnect_attempts: AtomicUsize::new(0),
            join_sent: AtomicBool::new(false),
            tx: Mutex::new(None),
            read_join: Mutex::new(None),
            flush_join: Mutex::new(None),
            connect_lock: Mutex::new(()),
            commands: Mutex::new(VecDeque::new()),
            state: Mutex::new(DataStore::default()),
            pending_rankings: Mutex::new(VecDeque::new()),
            events: Mutex::new(Events::default()),
            notify: Notify::default(),
            last_activity: Mutex::new(Instant::now()),
        };
        CloudConnection {
            inner: Arc::new(inner),
        }
    }
}

impl CloudConnection {
    /// 建立云存储 WebSocket 连接（后台读线程 + 批量上传线程）。
    pub fn connect(&self) -> Result<()> {
        if self.inner.connected.load(Ordering::Acquire) {
            return Ok(());
        }
        self.reset_state();
        self.inner.stopping.store(false, Ordering::Release);
        establish(&self.inner)?;
        // 批量上传线程随首次连接启动，随 close 结束
        if self.inner.flush_join.lock().unwrap().is_none() {
            let inner = self.inner.clone();
            let handle = thread::Builder::new()
                .name("cloud-flush".into())
                .spawn(move || flush_loop(inner))
                .map_err(|e| CloudError::Thread(e.to_string()))?;
            *self.inner.flush_join.lock().unwrap() = Some(handle);
        }
        info!(
            "云存储连接已发起: work_id={}, editor={:?}",
            self.inner.work_id, self.inner.editor
        );
        Ok(())
    }

    /// 等待 WebSocket 层连接建立（超时返回 false）。
    pub fn wait_for_connection(&self, timeout: Duration) -> bool {
        wait_flag(&self.inner.notify, timeout, || {
            self.inner.connected.load(Ordering::Acquire)
        })
    }

    /// 等待首轮云数据加载完成（超时返回 false）。
    pub fn wait_for_data(&self, timeout: Duration) -> bool {
        wait_flag(&self.inner.notify, timeout, || {
            self.inner.data_ready.load(Ordering::Acquire)
        })
    }

    /// 连接并等待数据就绪的便捷方法。
    pub fn connect_and_wait(
        &self,
        connect_timeout: Duration,
        data_timeout: Duration,
    ) -> Result<bool> {
        self.connect()?;
        if !self.wait_for_connection(connect_timeout) {
            return Ok(false);
        }
        Ok(self.wait_for_data(data_timeout))
    }

    /// 关闭连接并清理资源。
    ///
    /// 注意：请勿在回调（如 `on_data_ready`）中调用本方法——回调在连接读线程内执行，
    /// 调用 `close()` 会 join 自身线程导致死锁。
    pub fn close(&self) {
        let inner = &self.inner;
        inner.stopping.store(true, Ordering::Release);
        inner.auto_reconnect.store(false, Ordering::Release);
        // 发送关闭帧，通知读线程退出
        if let Some(tx) = inner.tx.lock().unwrap().clone() {
            let _ = tx.send(Message::Close(None));
        }
        drop(inner.tx.lock().unwrap().take());
        if let Some(handle) = inner.read_join.lock().unwrap().take() {
            let _ = handle.join();
        }
        if let Some(handle) = inner.flush_join.lock().unwrap().take() {
            let _ = handle.join();
        }
        inner.commands.lock().unwrap().clear();
        inner.pending_rankings.lock().unwrap().clear();
        inner.connected.store(false, Ordering::Release);
        inner.data_ready.store(false, Ordering::Release);
        inner.state.lock().unwrap().clear_all();
        info!("云存储连接已关闭: work_id={}", inner.work_id);
    }

    // ==================== 状态查询 ====================

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Acquire)
    }

    pub fn is_data_ready(&self) -> bool {
        self.inner.data_ready.load(Ordering::Acquire)
    }

    pub fn online_users(&self) -> i64 {
        self.inner.online_users.load(Ordering::Acquire)
    }

    pub fn work_id(&self) -> i64 {
        self.inner.work_id
    }

    /// 检查连接健康状态：连接存在且空闲未超过阈值。
    pub fn check_connection_health(&self, max_inactivity: Duration) -> bool {
        if !self.is_connected() {
            return false;
        }
        let inactive = self.inner.last_activity.lock().unwrap().elapsed();
        if inactive > max_inactivity {
            warn!("云连接空闲超时: {inactive:?}");
            return false;
        }
        true
    }

    // ==================== 事件监听（观察者模式） ====================

    /// 注册数据就绪回调。
    pub fn on_data_ready(&self, cb: impl Fn() + Send + Sync + 'static) -> CallbackHandle {
        self.inner
            .events
            .lock()
            .unwrap()
            .data_ready
            .add(Box::new(cb))
    }

    /// 注册在线用户数变更回调（旧值，新值）。
    pub fn on_online_users_change(
        &self,
        cb: impl Fn(i64, i64) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner
            .events
            .lock()
            .unwrap()
            .online_users
            .add(Box::new(cb))
    }

    /// 注册排行榜数据接收回调。
    pub fn on_ranking_received(
        &self,
        cb: impl Fn(RankingData) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner.events.lock().unwrap().ranking.add(Box::new(cb))
    }

    /// 注册连接生命周期事件回调。
    pub fn on_connection(
        &self,
        cb: impl Fn(ConnectionEvent) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner
            .events
            .lock()
            .unwrap()
            .connection
            .add(Box::new(cb))
    }

    /// 按句柄移除事件回调。
    pub fn remove_callback(&self, handle: CallbackHandle) {
        let mut events = self.inner.events.lock().unwrap();
        events.data_ready.remove(handle);
        events.online_users.remove(handle);
        events.ranking.remove(handle);
        events.connection.remove(handle);
    }

    // ==================== 变量操作 ====================

    /// 获取私有变量句柄。
    pub fn get_private_variable(&self, name: &str) -> Option<CloudVariable> {
        let store = self.inner.state.lock().unwrap();
        store
            .variable(VarKind::Private, name)
            .map(|v| CloudVariable {
                inner: self.inner.clone(),
                kind: VarKind::Private,
                cvid: v.cvid.clone(),
                name: v.name.clone(),
            })
    }

    /// 获取公有变量句柄。
    pub fn get_public_variable(&self, name: &str) -> Option<CloudVariable> {
        let store = self.inner.state.lock().unwrap();
        store
            .variable(VarKind::Public, name)
            .map(|v| CloudVariable {
                inner: self.inner.clone(),
                kind: VarKind::Public,
                cvid: v.cvid.clone(),
                name: v.name.clone(),
            })
    }

    /// 获取云列表句柄。
    pub fn get_list(&self, name: &str) -> Option<CloudList> {
        let store = self.inner.state.lock().unwrap();
        store.list(name).map(|l| CloudList {
            inner: self.inner.clone(),
            cvid: l.cvid.clone(),
            name: l.name.clone(),
        })
    }

    /// 设置私有变量值（本地更新 + 回调 + 批量上传）。
    pub fn set_private_variable(&self, name: &str, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .set_variable(VarKind::Private, name, value.into())
    }

    /// 设置公有变量值。
    pub fn set_public_variable(&self, name: &str, value: impl Into<CloudValue>) -> Result<()> {
        self.inner.set_variable(VarKind::Public, name, value.into())
    }

    /// 获取全部私有变量（名字 → 值）。
    pub fn get_all_private_variables(&self) -> HashMap<String, CloudValue> {
        self.inner
            .state
            .lock()
            .unwrap()
            .private_vars
            .iter()
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }

    /// 获取全部公有变量（名字 → 值）。
    pub fn get_all_public_variables(&self) -> HashMap<String, CloudValue> {
        self.inner
            .state
            .lock()
            .unwrap()
            .public_vars
            .iter()
            .map(|(k, v)| (k.clone(), v.value.clone()))
            .collect()
    }

    /// 获取全部云列表（名字 → 元素快照）。
    pub fn get_all_lists(&self) -> HashMap<String, Vec<CloudValue>> {
        self.inner
            .state
            .lock()
            .unwrap()
            .lists
            .iter()
            .map(|(k, l)| (k.clone(), l.items.clone()))
            .collect()
    }

    /// 获取私有变量排行榜（结果经 `on_ranking_received` 回调返回）。
    pub fn get_ranking(&self, variable_name: &str, limit: i64, order: i64) -> Result<()> {
        if !(MIN_RANKING_LIMIT..=MAX_RANKING_LIMIT).contains(&limit) {
            return Err(CloudError::InvalidArgument(format!(
                "排行榜限制数量必须在 {MIN_RANKING_LIMIT}..={MAX_RANKING_LIMIT} 之间"
            )));
        }
        if order != ASCENDING_ORDER && order != DESCENDING_ORDER {
            return Err(CloudError::InvalidArgument(format!(
                "排序顺序必须是 {ASCENDING_ORDER} (正序) 或 {DESCENDING_ORDER} (逆序)"
            )));
        }
        let cvid = self
            .inner
            .state
            .lock()
            .unwrap()
            .variable(VarKind::Private, variable_name)
            .map(|v| v.cvid.clone())
            .ok_or_else(|| CloudError::VariableNotFound(variable_name.to_string()))?;
        self.inner
            .pending_rankings
            .lock()
            .unwrap()
            .push_back(cvid.clone());
        if let Err(e) = self.send_event(
            "list_ranking",
            &json!({
                "cvid": cvid,
                "limit": limit,
                "order_type": order,
            }),
        ) {
            // 发送失败时回滚待处理队列，避免后续排行榜结果错配
            self.inner.pending_rankings.lock().unwrap().pop_back();
            return Err(e);
        }
        Ok(())
    }

    // ==================== 列表便捷操作 ====================

    pub fn list_push(&self, name: &str, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(name, ListAction::Append(value.into()))
    }

    pub fn list_pop(&self, name: &str) -> Result<Option<CloudValue>> {
        let popped = self
            .inner
            .state
            .lock()
            .unwrap()
            .list(name)
            .and_then(|l| l.items.last().cloned());
        self.inner.list_apply_local(name, ListAction::DeleteLast)?;
        Ok(popped)
    }

    pub fn list_unshift(&self, name: &str, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(name, ListAction::Unshift(value.into()))
    }

    pub fn list_shift(&self, name: &str) -> Result<Option<CloudValue>> {
        let popped = self
            .inner
            .state
            .lock()
            .unwrap()
            .list(name)
            .and_then(|l| l.items.first().cloned());
        self.inner.list_apply_local(name, ListAction::DeleteAt(0))?;
        Ok(popped)
    }

    pub fn list_insert(
        &self,
        name: &str,
        index: usize,
        value: impl Into<CloudValue>,
    ) -> Result<()> {
        self.inner
            .list_apply_local(name, ListAction::Insert(index, value.into()))
    }

    pub fn list_remove(&self, name: &str, index: usize) -> Result<()> {
        self.inner
            .list_apply_local(name, ListAction::DeleteAt(index))
    }

    pub fn list_replace(
        &self,
        name: &str,
        index: usize,
        value: impl Into<CloudValue>,
    ) -> Result<()> {
        self.inner
            .list_apply_local(name, ListAction::ReplaceAt(index, value.into()))
    }

    pub fn list_replace_last(&self, name: &str, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(name, ListAction::ReplaceLast(value.into()))
    }

    pub fn list_clear(&self, name: &str) -> Result<()> {
        self.inner.list_apply_local(name, ListAction::DeleteAll)
    }

    // ==================== 内部发送 ====================

    /// 发送 Socket.IO 事件帧:`42["name",payload]`。
    pub(crate) fn send_event(&self, name: &str, payload: &Value) -> Result<()> {
        let frame = format!(
            "{EVENT_MESSAGE_PREFIX}{}",
            serde_json::to_string(&(name, payload))?
        );
        self.send_text(&frame)
    }

    /// 发送原始文本帧。
    pub(crate) fn send_text(&self, payload: &str) -> Result<()> {
        let tx = self
            .inner
            .tx
            .lock()
            .unwrap()
            .clone()
            .ok_or(CloudError::NotConnected)?;
        tx.send(Message::text(payload))
            .map_err(|_| CloudError::NotConnected)
    }

    fn reset_state(&self) {
        self.inner.connected.store(false, Ordering::Release);
        self.inner.data_ready.store(false, Ordering::Release);
        self.inner.io_ready.store(false, Ordering::Release);
        self.inner.online_users.store(0, Ordering::Release);
        self.inner.join_sent.store(false, Ordering::Release);
        self.inner.commands.lock().unwrap().clear();
        self.inner.pending_rankings.lock().unwrap().clear();
        self.inner.state.lock().unwrap().clear_all();
    }
}

// ==================== 云变量 / 云列表句柄 ====================

/// 云变量句柄：提供取值、赋值与变更订阅。
#[derive(Debug, Clone)]
pub struct CloudVariable {
    inner: Arc<CloudInner>,
    kind: VarKind,
    cvid: String,
    name: String,
}

impl CloudVariable {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn cvid(&self) -> &str {
        &self.cvid
    }

    pub fn kind(&self) -> VarKind {
        self.kind
    }

    /// 获取当前值。
    pub fn get(&self) -> Option<CloudValue> {
        self.inner
            .state
            .lock()
            .unwrap()
            .variable(self.kind, &self.name)
            .map(|v| v.value.clone())
    }

    /// 设置新值（本地生效，经批量队列上传云端）。
    pub fn set(&self, value: impl Into<CloudValue>) -> Result<()> {
        self.inner.set_variable(self.kind, &self.name, value.into())
    }

    /// 注册变更回调（旧值，新值，来源）。
    pub fn on_change(
        &self,
        cb: impl Fn(&CloudValue, &CloudValue, &str) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner
            .state
            .lock()
            .unwrap()
            .variable_mut(self.kind, &self.name)
            .map(|v| {
                let id = v.next_cb_id;
                v.next_cb_id += 1;
                v.callbacks.push((id, Box::new(cb)));
                CallbackHandle(id)
            })
            .unwrap_or(CallbackHandle(usize::MAX))
    }

    /// 移除变更回调。
    pub fn remove_change_callback(&self, handle: CallbackHandle) {
        if let Some(v) = self
            .inner
            .state
            .lock()
            .unwrap()
            .variable_mut(self.kind, &self.name)
        {
            v.callbacks.retain(|(id, _)| *id != handle.0);
        }
    }

    /// 注册排行榜数据回调（仅私有变量有效）。
    pub fn on_ranking(&self, cb: impl Fn(RankingData) + Send + Sync + 'static) -> CallbackHandle {
        self.inner
            .state
            .lock()
            .unwrap()
            .variable_mut(self.kind, &self.name)
            .map(|v| {
                let id = v.next_cb_id;
                v.next_cb_id += 1;
                v.ranking_callbacks.push((id, Box::new(cb)));
                CallbackHandle(id)
            })
            .unwrap_or(CallbackHandle(usize::MAX))
    }
}

/// 云列表句柄：提供元素操作与订阅。
#[derive(Debug, Clone)]
pub struct CloudList {
    inner: Arc<CloudInner>,
    cvid: String,
    name: String,
}

impl CloudList {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn cvid(&self) -> &str {
        &self.cvid
    }

    pub fn length(&self) -> usize {
        self.inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .map(|l| l.items.len())
            .unwrap_or(0)
    }

    /// 返回元素快照。
    pub fn items(&self) -> Vec<CloudValue> {
        self.inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .map(|l| l.items.clone())
            .unwrap_or_default()
    }

    pub fn get(&self, index: usize) -> Option<CloudValue> {
        self.inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .and_then(|l| l.items.get(index).cloned())
    }

    pub fn push(&self, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(&self.name, ListAction::Append(value.into()))
    }

    pub fn pop(&self) -> Result<Option<CloudValue>> {
        let popped = self
            .inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .and_then(|l| l.items.last().cloned());
        self.inner
            .list_apply_local(&self.name, ListAction::DeleteLast)?;
        Ok(popped)
    }

    pub fn unshift(&self, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(&self.name, ListAction::Unshift(value.into()))
    }

    pub fn shift(&self) -> Result<Option<CloudValue>> {
        let popped = self
            .inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .and_then(|l| l.items.first().cloned());
        self.inner
            .list_apply_local(&self.name, ListAction::DeleteAt(0))?;
        Ok(popped)
    }

    pub fn insert(&self, index: usize, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(&self.name, ListAction::Insert(index, value.into()))
    }

    pub fn remove(&self, index: usize) -> Result<Option<CloudValue>> {
        let removed = self
            .inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .and_then(|l| l.items.get(index).cloned());
        self.inner
            .list_apply_local(&self.name, ListAction::DeleteAt(index))?;
        Ok(removed)
    }

    pub fn replace(&self, index: usize, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(&self.name, ListAction::ReplaceAt(index, value.into()))
    }

    pub fn replace_last(&self, value: impl Into<CloudValue>) -> Result<()> {
        self.inner
            .list_apply_local(&self.name, ListAction::ReplaceLast(value.into()))
    }

    pub fn clear(&self) -> Result<()> {
        self.inner
            .list_apply_local(&self.name, ListAction::DeleteAll)
    }

    pub fn index_of(&self, item: &CloudValue) -> Option<usize> {
        self.inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .and_then(|l| l.items.iter().position(|v| v == item))
    }

    pub fn includes(&self, item: &CloudValue) -> bool {
        self.index_of(item).is_some()
    }

    /// 将列表元素连接为字符串。
    pub fn join(&self, separator: &str) -> String {
        self.inner
            .state
            .lock()
            .unwrap()
            .list(&self.name)
            .map(|l| {
                l.items
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(separator)
            })
            .unwrap_or_default()
    }

    /// 注册整表变更回调（旧列表，新列表，来源）。
    pub fn on_change(
        &self,
        cb: impl Fn(&[CloudValue], &[CloudValue], &str) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner
            .state
            .lock()
            .unwrap()
            .list_mut(&self.name)
            .map(|l| {
                let id = l.next_cb_id;
                l.next_cb_id += 1;
                l.change_callbacks.push((id, Box::new(cb)));
                CallbackHandle(id)
            })
            .unwrap_or(CallbackHandle(usize::MAX))
    }

    /// 注册列表操作回调（操作名，参数）。
    pub fn on_operation(
        &self,
        operation: &str,
        cb: impl Fn(&str, &[CloudValue]) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner
            .state
            .lock()
            .unwrap()
            .list_mut(&self.name)
            .map(|l| {
                let id = l.next_cb_id;
                l.next_cb_id += 1;
                l.operation_callbacks
                    .entry(operation.to_string())
                    .or_default()
                    .push((id, Box::new(cb)));
                CallbackHandle(id)
            })
            .unwrap_or(CallbackHandle(usize::MAX))
    }

    /// 按句柄移除列表回调。
    pub fn remove_callback(&self, handle: CallbackHandle) {
        if let Some(l) = self.inner.state.lock().unwrap().list_mut(&self.name) {
            l.change_callbacks.retain(|(id, _)| *id != handle.0);
            for callbacks in l.operation_callbacks.values_mut() {
                callbacks.retain(|(id, _)| *id != handle.0);
            }
        }
    }
}

// ==================== 列表操作（策略式动作） ====================

/// 列表操作动作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListAction {
    Append(CloudValue),
    Unshift(CloudValue),
    Insert(usize, CloudValue),
    DeleteLast,
    DeleteAll,
    DeleteAt(usize),
    ReplaceLast(CloudValue),
    ReplaceAt(usize, CloudValue),
}

/// 一次列表操作的结果：操作名、回调参数、对应云协议操作 JSON。
#[derive(Debug)]
struct ListOutcome {
    op: String,
    args: Vec<CloudValue>,
    wire: Value,
}

/// 纯函数：对元素序列执行列表动作，返回结果（可测）。
fn execute_list_action(items: &mut Vec<CloudValue>, action: &ListAction) -> Option<ListOutcome> {
    match action {
        ListAction::Append(v) => {
            items.push(v.clone());
            let index = items.len() - 1;
            Some(ListOutcome {
                op: "push".into(),
                args: vec![v.clone(), CloudValue::Number(index as i64)],
                wire: json!({"action": "append", "value": v}),
            })
        }
        ListAction::Unshift(v) => {
            items.insert(0, v.clone());
            Some(ListOutcome {
                op: "unshift".into(),
                args: vec![v.clone(), CloudValue::Number(0)],
                wire: json!({"action": "unshift", "value": v}),
            })
        }
        ListAction::Insert(index, v) => {
            if *index > items.len() {
                return None;
            }
            items.insert(*index, v.clone());
            Some(ListOutcome {
                op: "insert".into(),
                args: vec![v.clone(), CloudValue::Number(*index as i64)],
                wire: json!({"action": "insert", "nth": index + 1, "value": v}),
            })
        }
        ListAction::DeleteLast => {
            let popped = items.pop()?;
            Some(ListOutcome {
                op: "pop".into(),
                args: vec![popped, CloudValue::Number(items.len() as i64)],
                wire: json!({"action": "delete", "nth": "last"}),
            })
        }
        ListAction::DeleteAll => {
            items.clear();
            Some(ListOutcome {
                op: "clear".into(),
                args: vec![],
                wire: json!({"action": "delete", "nth": "all"}),
            })
        }
        ListAction::DeleteAt(index) => {
            if *index >= items.len() {
                return None;
            }
            let removed = items.remove(*index);
            Some(ListOutcome {
                op: "remove".into(),
                args: vec![removed, CloudValue::Number(*index as i64)],
                wire: json!({"action": "delete", "nth": index + 1}),
            })
        }
        ListAction::ReplaceLast(v) => {
            let last = items.len().checked_sub(1)?;
            let old = std::mem::replace(&mut items[last], v.clone());
            Some(ListOutcome {
                op: "replace_last".into(),
                args: vec![old, v.clone()],
                wire: json!({"action": "replace", "nth": "last", "value": v}),
            })
        }
        ListAction::ReplaceAt(index, v) => {
            if *index >= items.len() {
                return None;
            }
            let old = std::mem::replace(&mut items[*index], v.clone());
            Some(ListOutcome {
                op: "replace".into(),
                args: vec![old, v.clone(), CloudValue::Number(*index as i64)],
                wire: json!({"action": "replace", "nth": index + 1, "value": v}),
            })
        }
    }
}

/// 从云端操作 JSON 解析列表动作。
fn parse_list_action(op: &Value) -> Option<ListAction> {
    let action = op.get("action")?.as_str()?;
    let value = || op.get("value").map(CloudValue::from_json);
    match action {
        "append" => Some(ListAction::Append(value()?)),
        "unshift" => Some(ListAction::Unshift(value()?)),
        "insert" => {
            let nth = op.get("nth")?.as_i64()?;
            Some(ListAction::Insert(
                nth.saturating_sub(1).max(0) as usize,
                value()?,
            ))
        }
        "delete" => match op.get("nth") {
            Some(Value::String(s)) if s == "last" => Some(ListAction::DeleteLast),
            Some(Value::String(s)) if s == "all" => Some(ListAction::DeleteAll),
            Some(Value::Number(n)) => {
                let idx = n.as_i64()?.saturating_sub(1).max(0) as usize;
                Some(ListAction::DeleteAt(idx))
            }
            _ => None,
        },
        "replace" => {
            let nth = op.get("nth")?;
            let v = value()?;
            match nth {
                Value::String(s) if s == "last" => Some(ListAction::ReplaceLast(v)),
                Value::Number(n) => {
                    let idx = n.as_i64()?.saturating_sub(1).max(0) as usize;
                    Some(ListAction::ReplaceAt(idx, v))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

impl CloudInner {
    /// 本地列表操作：修改数据 + 触发回调 + 入队上传。
    fn list_apply_local(&self, name: &str, action: ListAction) -> Result<()> {
        let (cvid, old_items, outcome) = {
            let mut store = self.state.lock().unwrap();
            let list = store
                .list_mut(name)
                .ok_or_else(|| CloudError::ListNotFound(name.to_string()))?;
            let old_items = list.items.clone();
            let cvid = list.cvid.clone();
            let outcome = execute_list_action(&mut list.items, &action).ok_or_else(|| {
                CloudError::InvalidArgument(format!("列表操作越界或非法: {action:?}"))
            })?;
            (cvid, old_items, outcome)
        };
        self.fire_list_outcome(&cvid, &outcome, &old_items, ChangeSource::Local);
        self.queue(CommandFactory::update_list(&cvid, vec![outcome.wire]));
        Ok(())
    }

    /// 云端列表操作：修改数据 + 触发回调，不入队。
    fn list_apply_cloud(&self, cvid: &str, ops: &[Value]) {
        for op in ops {
            let Some(action) = parse_list_action(op) else {
                warn!("无法解析列表操作: {op}");
                continue;
            };
            let outcome = {
                let mut store = self.state.lock().unwrap();
                let Some(list) = store.list_mut(cvid) else {
                    warn!("收到未知 cvid 的列表更新: {cvid}");
                    continue;
                };
                let old_items = list.items.clone();
                let Some(outcome) = execute_list_action(&mut list.items, &action) else {
                    warn!("云端列表操作越界: cvid={cvid} op={op}");
                    continue;
                };
                (old_items, outcome)
            };
            let (old_items, outcome) = outcome;
            self.fire_list_outcome(cvid, &outcome, &old_items, ChangeSource::Cloud);
        }
    }

    /// 触发列表操作回调与整表变更回调。
    fn fire_list_outcome(
        &self,
        cvid: &str,
        outcome: &ListOutcome,
        old_items: &[CloudValue],
        source: ChangeSource,
    ) {
        // 操作回调
        let op_callbacks = {
            let mut store = self.state.lock().unwrap();
            store
                .list_mut(cvid)
                .and_then(|l| l.operation_callbacks.get_mut(&outcome.op))
                .map(std::mem::take)
        };
        if let Some(callbacks) = op_callbacks {
            for (_, cb) in &callbacks {
                if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(&outcome.op, &outcome.args))) {
                    warn!("列表操作回调 panic: {e:?}");
                }
            }
            if let Some(l) = self.state.lock().unwrap().list_mut(cvid)
                && let Some(entry) = l.operation_callbacks.get_mut(&outcome.op)
            {
                entry.extend(callbacks);
            }
        }
        // 整表变更回调
        let new_items = {
            let store = self.state.lock().unwrap();
            store.list(cvid).map(|l| l.items.clone())
        };
        let change_callbacks = {
            let mut store = self.state.lock().unwrap();
            store
                .list_mut(cvid)
                .map(|l| std::mem::take(&mut l.change_callbacks))
        };
        if let Some(callbacks) = change_callbacks {
            for (_, cb) in &callbacks {
                if let Err(e) = catch_unwind(AssertUnwindSafe(|| {
                    if let Some(new_items) = &new_items {
                        cb(old_items, new_items, source.as_str());
                    }
                })) {
                    warn!("列表变更回调 panic: {e:?}");
                }
            }
            if let Some(l) = self.state.lock().unwrap().list_mut(cvid) {
                l.change_callbacks.extend(callbacks);
            }
        }
    }

    /// 设置变量：本地更新 + 回调 + 入队。
    fn set_variable(&self, kind: VarKind, name: &str, new_value: CloudValue) -> Result<()> {
        let (cvid, old_value) = {
            let mut store = self.state.lock().unwrap();
            let var = store
                .variable_mut(kind, name)
                .ok_or_else(|| CloudError::VariableNotFound(name.to_string()))?;
            let old = std::mem::replace(&mut var.value, new_value.clone());
            (var.cvid.clone(), old)
        };
        emit_variable_change(self, kind, name, &old_value, &new_value, "local");
        let command = match kind {
            VarKind::Private => CommandFactory::update_private_variable(&cvid, &new_value),
            VarKind::Public => CommandFactory::update_public_variable(&cvid, &new_value),
        };
        self.queue(command);
        Ok(())
    }

    fn queue(&self, command: CloudCommand) {
        self.commands.lock().unwrap().push_back(command);
    }
}

/// 触发变量变更回调（取走 → 锁外执行 → 放回）。
fn emit_variable_change(
    inner: &CloudInner,
    kind: VarKind,
    key: &str,
    old: &CloudValue,
    new: &CloudValue,
    source: &str,
) {
    let callbacks = {
        let mut store = inner.state.lock().unwrap();
        match store.variable_mut(kind, key) {
            Some(v) => std::mem::take(&mut v.callbacks),
            None => return,
        }
    };
    for (_, cb) in &callbacks {
        if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(old, new, source))) {
            warn!("变量变更回调 panic: {e:?}");
        }
    }
    if let Some(v) = inner.state.lock().unwrap().variable_mut(kind, key) {
        v.callbacks.extend(callbacks);
    }
}

/// 设置 WebSocket 底层流的读取超时（Plain 或 rustls 两种形态）。
fn set_stream_read_timeout(stream: &mut WsStream, timeout: Duration) -> std::io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(owned) => owned.sock.set_read_timeout(Some(timeout)),
        _ => Ok(()),
    }
}

// ==================== 消息帧解析 ====================

/// 解析后的 Socket.IO 帧。
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Handshake(Value),
    Connected,
    Ping,
    Pong,
    ServerClose,
    /// 事件帧：事件名 + 载荷。
    Event(String, Value),
    /// 未知/无法解析的文本。
    Unknown(String),
}

/// 纯函数：解析 Socket.IO 文本帧（可测）。
pub fn parse_frame(text: &str) -> Frame {
    if text == PING_MESSAGE {
        return Frame::Ping;
    }
    if text == PONG_MESSAGE {
        return Frame::Pong;
    }
    if let Some(rest) = text.strip_prefix(HANDSHAKE_PREFIX) {
        return match serde_json::from_str(rest) {
            Ok(v) => Frame::Handshake(v),
            Err(_) => Frame::Unknown(text.to_string()),
        };
    }
    if text == CONNECTED_MESSAGE {
        return Frame::Connected;
    }
    if text.starts_with(SERVER_CLOSE_PREFIX) {
        return Frame::ServerClose;
    }
    if let Some(rest) = text.strip_prefix(EVENT_MESSAGE_PREFIX)
        && let Ok(Value::Array(items)) = serde_json::from_str::<Value>(rest)
        && let Some(Value::String(name)) = items.first()
    {
        let mut payload = items.get(1).cloned().unwrap_or(Value::Null);
        // 部分事件（如 list_variables_done）载荷是 JSON 字符串，需二次解析
        // （与 Python _handle_event_message 行为一致；解析失败则保持原字符串）
        if let Value::String(s) = &payload
            && let Ok(parsed) = serde_json::from_str::<Value>(s)
        {
            payload = parsed;
        }
        return Frame::Event(name.clone(), payload);
    }
    Frame::Unknown(text.to_string())
}

/// 处理单帧（错误仅记录日志，不中断连接）。
fn handle_frame(inner: &Arc<CloudInner>, text: &str) -> Result<()> {
    *inner.last_activity.lock().unwrap() = Instant::now();
    match parse_frame(text) {
        Frame::Ping => send_inner_text(inner, PONG_MESSAGE),
        Frame::Pong => Ok(()),
        Frame::Handshake(_) => {
            info!("握手成功,发送连接请求");
            send_inner_text(inner, CONNECTED_MESSAGE)
        }
        Frame::Connected => {
            inner.notify.notify_with(|| {
                inner.connected.store(true, Ordering::Release);
                inner.io_ready.store(true, Ordering::Release);
                inner.reconnect_attempts.store(0, Ordering::Release);
            });
            emit_connection_event(inner, ConnectionEvent::Opened);
            // 服务器可能重复回 40，只发送一次 JOIN
            if inner.join_sent.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            info!("云存储连接确认,发送 JOIN 消息");
            send_inner_event(inner, "join", &json!(inner.work_id.to_string()))
        }
        Frame::ServerClose => {
            info!("收到服务器关闭请求 (41)");
            inner.connected.store(false, Ordering::Release);
            emit_connection_event(
                inner,
                ConnectionEvent::ServerClosed("服务器主动要求关闭连接".into()),
            );
            // 服务器关闭：清理发送端，读循环随后退出并走重连逻辑
            inner.tx.lock().unwrap().take();
            Ok(())
        }
        Frame::Event(name, payload) => dispatch_message(inner, &name, &payload),
        Frame::Unknown(text) => {
            debug!("收到未知消息: {}", truncate(&text, 80));
            Ok(())
        }
    }
}

// ==================== 消息处理策略 ====================

/// 消息处理策略接口：每种消息类型一个处理器。
trait MessageHandler: Send + Sync {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()>;
}

/// `connect_done`：加入成功，请求全量数据。
struct JoinHandler;

impl MessageHandler for JoinHandler {
    fn handle(&self, inner: &Arc<CloudInner>, _payload: &Value) -> Result<()> {
        info!("加入成功,请求所有数据");
        send_inner_event(inner, "list_variables", &json!({}))
    }
}

/// `list_variables_done`：创建数据项并标记就绪。
struct AllDataHandler;

impl MessageHandler for AllDataHandler {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()> {
        if let Some(items) = payload.as_array() {
            for item in items {
                if let Err(e) = create_data_item(inner, item) {
                    warn!("创建数据项失败: {e}");
                }
            }
        } else {
            warn!(
                "list_variables_done 载荷不是数组: {}",
                truncate(&payload.to_string(), 200)
            );
        }
        inner
            .notify
            .notify_with(|| inner.data_ready.store(true, Ordering::Release));
        let callbacks = {
            let mut events = inner.events.lock().unwrap();
            events.data_ready.take_all()
        };
        for (_, cb) in &callbacks {
            if let Err(e) = catch_unwind(AssertUnwindSafe(cb)) {
                warn!("数据就绪回调 panic: {e:?}");
            }
        }
        inner
            .events
            .lock()
            .unwrap()
            .data_ready
            .items
            .extend(callbacks);
        let store = inner.state.lock().unwrap();
        info!(
            "数据准备完成: 私有 {} 公有 {} 列表 {}",
            store.private_vars.len(),
            store.public_vars.len(),
            store.lists.len()
        );
        Ok(())
    }
}

/// `update_private_vars_done`：云端私有变量更新。
struct UpdatePrivateVarHandler;

impl MessageHandler for UpdatePrivateVarHandler {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()> {
        if let (Some(cvid), Some(value)) = (
            payload.get("cvid").and_then(Value::as_str),
            payload.get("value"),
        ) {
            let new_value = CloudValue::from_json(value);
            let old = {
                let mut store = inner.state.lock().unwrap();
                match store.variable_mut(VarKind::Private, cvid) {
                    Some(v) => Some(std::mem::replace(&mut v.value, new_value.clone())),
                    None => None,
                }
            };
            if let Some(old) = old {
                emit_variable_change(inner, VarKind::Private, cvid, &old, &new_value, "cloud");
            }
        }
        Ok(())
    }
}

/// `update_vars_done`：云端公有变量更新（可能是列表或 "fail"）。
struct UpdatePublicVarHandler;

impl MessageHandler for UpdatePublicVarHandler {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()> {
        if payload.as_str() == Some("fail") {
            return Ok(());
        }
        if let Some(items) = payload.as_array() {
            for item in items {
                if let (Some(cvid), Some(value)) =
                    (item.get("cvid").and_then(Value::as_str), item.get("value"))
                {
                    let new_value = CloudValue::from_json(value);
                    let old = {
                        let mut store = inner.state.lock().unwrap();
                        match store.variable_mut(VarKind::Public, cvid) {
                            Some(v) => Some(std::mem::replace(&mut v.value, new_value.clone())),
                            None => None,
                        }
                    };
                    if let Some(old) = old {
                        emit_variable_change(
                            inner,
                            VarKind::Public,
                            cvid,
                            &old,
                            &new_value,
                            "cloud",
                        );
                    }
                }
            }
        }
        Ok(())
    }
}

/// `update_lists_done`：云端列表操作序列。
struct UpdateListHandler;

impl MessageHandler for UpdateListHandler {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()> {
        if let Some(map) = payload.as_object() {
            for (cvid, ops) in map {
                if let Some(ops) = ops.as_array() {
                    inner.list_apply_cloud(cvid, ops);
                }
            }
        }
        Ok(())
    }
}

/// `online_users_change`：在线用户数更新。
struct OnlineUsersHandler;

impl MessageHandler for OnlineUsersHandler {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()> {
        if let Some(total) = payload.get("total").and_then(Value::as_i64) {
            let old = inner.online_users.swap(total, Ordering::AcqRel);
            emit_online_users_change(inner, old, total);
        }
        Ok(())
    }
}

/// `list_ranking_done`：排行榜数据接收。
struct RankingHandler;

impl MessageHandler for RankingHandler {
    fn handle(&self, inner: &Arc<CloudInner>, payload: &Value) -> Result<()> {
        let cvid = match inner.pending_rankings.lock().unwrap().pop_front() {
            Some(cvid) => cvid,
            None => {
                warn!("收到排行榜数据但没有待处理的请求");
                return Ok(());
            }
        };
        let mut ranking = RankingData {
            cvid: cvid.clone(),
            name: String::new(),
            items: Vec::new(),
        };
        {
            let store = inner.state.lock().unwrap();
            if let Some(v) = store.variable(VarKind::Private, &cvid) {
                ranking.name = v.name.clone();
            }
        }
        if let Some(items) = payload.get("items").and_then(Value::as_array) {
            for item in items {
                if let (Some(value), Some(identifier), Some(nickname), Some(avatar_url)) = (
                    item.get("value"),
                    item.get("identifier"),
                    item.get("nickname"),
                    item.get("avatar_url"),
                ) {
                    ranking.items.push(RankingItem {
                        value: CloudValue::from_json(value),
                        user: RankingUser {
                            id: identifier
                                .as_str()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0),
                            nickname: nickname.as_str().unwrap_or_default().to_string(),
                            avatar_url: avatar_url.as_str().unwrap_or_default().to_string(),
                        },
                    });
                }
            }
        } else {
            warn!("排行榜 items 不是列表");
        }
        // 变量级回调
        let callbacks = {
            let mut store = inner.state.lock().unwrap();
            store
                .variable_mut(VarKind::Private, &cvid)
                .map(|v| std::mem::take(&mut v.ranking_callbacks))
        };
        if let Some(callbacks) = callbacks {
            for (_, cb) in &callbacks {
                if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(ranking.clone()))) {
                    warn!("排行榜回调 panic: {e:?}");
                }
            }
            if let Some(v) = inner
                .state
                .lock()
                .unwrap()
                .variable_mut(VarKind::Private, &cvid)
            {
                v.ranking_callbacks.extend(callbacks);
            }
        }
        // 连接级事件
        let event_callbacks = {
            let mut events = inner.events.lock().unwrap();
            events.ranking.take_all()
        };
        for (_, cb) in &event_callbacks {
            if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(ranking.clone()))) {
                warn!("排行榜事件回调 panic: {e:?}");
            }
        }
        inner
            .events
            .lock()
            .unwrap()
            .ranking
            .items
            .extend(event_callbacks);
        Ok(())
    }
}

/// `illegal_event_done`：非法事件提示。
struct IllegalEventHandler;

impl MessageHandler for IllegalEventHandler {
    fn handle(&self, _inner: &Arc<CloudInner>, _payload: &Value) -> Result<()> {
        warn!("检测到非法事件");
        Ok(())
    }
}

/// 策略注册表：事件名 → 处理器。
fn dispatch_message(inner: &Arc<CloudInner>, name: &str, payload: &Value) -> Result<()> {
    match name {
        "connect_done" => JoinHandler.handle(inner, payload),
        "list_variables_done" => AllDataHandler.handle(inner, payload),
        "update_private_vars_done" => UpdatePrivateVarHandler.handle(inner, payload),
        "update_vars_done" => UpdatePublicVarHandler.handle(inner, payload),
        "update_lists_done" => UpdateListHandler.handle(inner, payload),
        "online_users_change" => OnlineUsersHandler.handle(inner, payload),
        "list_ranking_done" => RankingHandler.handle(inner, payload),
        "illegal_event_done" => IllegalEventHandler.handle(inner, payload),
        other => {
            debug!(
                "未知消息类型: {other}, 载荷: {}",
                truncate(&payload.to_string(), 80)
            );
            Ok(())
        }
    }
}

// ==================== 数据项创建 ====================

/// 从数据项 JSON 创建变量或列表（云端类型：0 私有变量 / 1 公有变量 / 2 列表）。
fn create_data_item(inner: &Arc<CloudInner>, item: &Value) -> Result<()> {
    let Some(cvid) = item.get("cvid").and_then(Value::as_str) else {
        return Err(CloudError::InvalidArgument("数据项缺少 cvid".into()));
    };
    let Some(name) = item.get("name").and_then(Value::as_str) else {
        return Err(CloudError::InvalidArgument("数据项缺少 name".into()));
    };
    let Some(value) = item.get("value") else {
        return Err(CloudError::InvalidArgument("数据项缺少 value".into()));
    };
    let Some(data_type) = item.get("type").and_then(Value::as_i64) else {
        return Err(CloudError::InvalidArgument("数据项缺少 type".into()));
    };
    let mut store = inner.state.lock().unwrap();
    match data_type {
        0 => {
            store.create_private(
                cvid.to_string(),
                name.to_string(),
                CloudValue::from_json(value),
            );
        }
        1 => {
            store.create_public(
                cvid.to_string(),
                name.to_string(),
                CloudValue::from_json(value),
            );
        }
        2 => {
            let items = value
                .as_array()
                .map(|arr| arr.iter().map(CloudValue::from_json).collect())
                .unwrap_or_default();
            store.create_list(cvid.to_string(), name.to_string(), items);
        }
        other => {
            warn!("未知数据类型: {other}");
        }
    }
    Ok(())
}

// ==================== 事件触发辅助 ====================

/// 在 `CloudInner` 上发送事件帧（供读线程内使用）。
fn send_inner_event(inner: &Arc<CloudInner>, name: &str, payload: &Value) -> Result<()> {
    let frame = format!(
        "{EVENT_MESSAGE_PREFIX}{}",
        serde_json::to_string(&(name, payload))?
    );
    send_inner_text(inner, &frame)
}

/// 在 `CloudInner` 上发送原始文本帧。
fn send_inner_text(inner: &Arc<CloudInner>, payload: &str) -> Result<()> {
    let tx = inner
        .tx
        .lock()
        .unwrap()
        .clone()
        .ok_or(CloudError::NotConnected)?;
    tx.send(Message::text(payload))
        .map_err(|_| CloudError::NotConnected)
}

fn emit_online_users_change(inner: &Arc<CloudInner>, old: i64, new: i64) {
    let callbacks = {
        let mut events = inner.events.lock().unwrap();
        events.online_users.take_all()
    };
    for (_, cb) in &callbacks {
        if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(old, new))) {
            warn!("在线用户回调 panic: {e:?}");
        }
    }
    inner
        .events
        .lock()
        .unwrap()
        .online_users
        .items
        .extend(callbacks);
}

fn emit_connection_event(inner: &Arc<CloudInner>, event: ConnectionEvent) {
    let callbacks = {
        let mut events = inner.events.lock().unwrap();
        events.connection.take_all()
    };
    for (_, cb) in &callbacks {
        if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(event.clone()))) {
            warn!("连接事件回调 panic: {e:?}");
        }
    }
    inner
        .events
        .lock()
        .unwrap()
        .connection
        .items
        .extend(callbacks);
}

/// 基于条件变量的超时等待。
fn wait_flag(notify: &Notify, timeout: Duration, flag: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    let guard = notify.lock.lock().unwrap();
    let mut guard = guard;
    while !flag() {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        let (g, _) = notify.cond.wait_timeout(guard, deadline - now).unwrap();
        guard = g;
    }
    true
}

/// 日志截断。
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        const SUFFIX: &str = "...";
        let half = (max.saturating_sub(SUFFIX.len())) / 2;
        let head: String = text.chars().take(half).collect();
        let tail: String = text.chars().skip(text.chars().count() - half).collect();
        format!("{head}{SUFFIX}{tail}")
    }
}

// ==================== 连接建立与读循环 ====================

/// 建立 WebSocket 连接并启动读线程。
fn establish(inner: &Arc<CloudInner>) -> Result<()> {
    // 串行化建立过程，避免 connect() 与自动重连竞态产生双读线程
    let _connect_guard = inner.connect_lock.lock().unwrap();
    let (auth_type, stag) = inner.editor.query_params();
    let url = format!(
        "{CLOUD_WS_BASE_URL}?session_id={}&authorization_type={auth_type}&stag={stag}&EIO=3&transport=websocket",
        inner.work_id
    );

    // 设备认证头（复用现有 CloudAuthenticator）
    let mut auth = CloudAuthenticator::new(None);
    let device_auth = auth
        .generate_x_device_auth()
        .map_err(|e| CloudError::Auth(e.to_string()))?;

    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| CloudError::Handshake(e.to_string()))?;
    let headers = request.headers_mut();
    headers.insert(
        "X-Creation-Tools-Device-Auth",
        HeaderValue::from_str(&device_auth)
            .map_err(|e| CloudError::InvalidArgument(e.to_string()))?,
    );
    if let Some(token) = &inner.token {
        let cookie = format!("Authorization={token}");
        headers.insert(
            "Cookie",
            HeaderValue::from_str(&cookie)
                .map_err(|e| CloudError::InvalidArgument(e.to_string()))?,
        );
    }

    let (mut ws, response) = connect(request)?;
    // WebSocket 升级成功返回 HTTP 101 Switching Protocols
    if response.status() != tungstenite::http::StatusCode::SWITCHING_PROTOCOLS {
        return Err(CloudError::Handshake(format!(
            "HTTP 状态: {}",
            response.status()
        )));
    }
    // 设置底层流读取超时：read 周期性苏醒，避免服务器静默时发送通道饥饿
    let _ = set_stream_read_timeout(ws.get_mut(), Duration::from_millis(200));
    info!("云存储 WebSocket 已建立: {url}");

    let (tx, rx) = mpsc::channel::<Message>();
    *inner.tx.lock().unwrap() = Some(tx);
    inner.io_ready.store(false, Ordering::Release);
    inner.notify.notify_with(|| {
        inner.connected.store(true, Ordering::Release);
        inner.reconnect_attempts.store(0, Ordering::Release);
    });
    inner.join_sent.store(false, Ordering::Release);
    emit_connection_event(inner, ConnectionEvent::Opened);

    let inner_loop = inner.clone();
    let handle = thread::Builder::new()
        .name("cloud-ws-read".into())
        .spawn(move || read_loop(inner_loop, ws, rx))
        .map_err(|e| CloudError::Thread(e.to_string()))?;
    *inner.read_join.lock().unwrap() = Some(handle);
    Ok(())
}

/// 读线程：独占 WebSocket，转发写消息，解析帧并分发。
fn read_loop(inner: Arc<CloudInner>, mut ws: Ws, rx: mpsc::Receiver<Message>) {
    'outer: loop {
        if inner.stopping.load(Ordering::Acquire) {
            break;
        }
        // 优先转发待发送消息（最多阻塞 100ms；连续发送上限避免写洪水饿死入站读取）
        let mut sent = 0;
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(msg) => {
                    if let Err(e) = ws.send(msg) {
                        info!("发送失败: {e}");
                        break 'outer;
                    }
                    sent += 1;
                    if sent >= 16 {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => break 'outer,
            }
        }
        // 读取入站消息
        match ws.read() {
            Ok(Message::Text(text)) => {
                if let Err(e) = handle_frame(&inner, text.as_str()) {
                    warn!("处理消息失败: {e}");
                }
            }
            Ok(Message::Ping(payload)) => {
                let _ = ws.send(Message::Pong(payload));
            }
            Ok(Message::Pong(_)) => {}
            Ok(Message::Binary(data)) => {
                debug!("收到二进制消息 {} 字节", data.len());
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Frame(_)) => {}
            // 读取超时：回到循环顶部处理待发送消息
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                info!("云存储读取结束: {e}");
                break;
            }
        }
    }
    drop(rx);
    on_connection_lost(inner);
}

/// 连接丢失后的清理与自动重连。
fn on_connection_lost(inner: Arc<CloudInner>) {
    let was_connected = inner.connected.load(Ordering::Acquire);
    inner.notify.notify_with(|| {
        inner.connected.store(false, Ordering::Release);
        inner.data_ready.store(false, Ordering::Release);
    });
    inner.join_sent.store(false, Ordering::Release);
    inner.io_ready.store(false, Ordering::Release);
    *inner.tx.lock().unwrap() = None;
    emit_connection_event(&inner, ConnectionEvent::Closed { was_connected });
    if inner.stopping.load(Ordering::Acquire) || !inner.auto_reconnect.load(Ordering::Acquire) {
        return;
    }
    // 退避循环重试：establish 失败（如网络不通）不会再有新的连接丢失事件，
    // 因此必须在循环内重试，直到成功、停止或达到最大次数。
    let mut attempts = 0;
    loop {
        attempts += 1;
        if attempts > inner.max_reconnect_attempts {
            warn!(
                "已达最大重连次数 ({}), 停止重连",
                inner.max_reconnect_attempts
            );
            return;
        }
        let backoff = inner
            .reconnect_interval
            .saturating_mul(1u32 << (attempts - 1).min(5));
        let delay = backoff.min(Duration::from_secs(300));
        info!("连接断开,第 {attempts} 次重连将于 {delay:?} 后进行");
        thread::sleep(delay);
        if inner.stopping.load(Ordering::Acquire) {
            return;
        }
        match establish(&inner) {
            Ok(()) => {
                info!("重连成功(第 {attempts} 次)");
                return;
            }
            Err(e) => warn!("第 {attempts} 次重连失败: {e}"),
        }
    }
}

/// 批量上传线程：周期性地合并队列命令并发送。
fn flush_loop(inner: Arc<CloudInner>) {
    while !inner.stopping.load(Ordering::Acquire) {
        thread::sleep(inner.flush_interval);
        let batch: Vec<CloudCommand> = {
            let mut queue = inner.commands.lock().unwrap();
            if queue.is_empty() {
                continue;
            }
            queue.drain(..).collect()
        };
        if batch.is_empty() {
            continue;
        }
        // 仅在连接且 Socket.IO 握手完成后上传，避免消息发往未握手会话被丢弃
        if !inner.connected.load(Ordering::Acquire) || !inner.io_ready.load(Ordering::Acquire) {
            // 未连接（断线/重连窗口）：命令保留，待连接恢复后补发，避免数据丢失
            warn!("云连接未就绪, {} 条命令保留待上传", batch.len());
            let mut queue = inner.commands.lock().unwrap();
            for cmd in batch.into_iter().rev() {
                queue.push_front(cmd);
            }
            continue;
        }
        let merged = merge_commands(batch);
        let Some(tx) = inner.tx.lock().unwrap().clone() else {
            continue;
        };
        let send = |payload: String| -> bool {
            match tx.send(Message::text(payload)) {
                Ok(_) => true,
                Err(e) => {
                    warn!("批量上传发送失败: {e}");
                    false
                }
            }
        };
        if !merged.private_updates.is_empty() {
            let frame = format!(
                "{EVENT_MESSAGE_PREFIX}{}",
                serde_json::to_string(&("update_private_vars", &merged.private_updates)).unwrap()
            );
            send(frame);
        }
        if !merged.public_updates.is_empty() {
            let frame = format!(
                "{EVENT_MESSAGE_PREFIX}{}",
                serde_json::to_string(&("update_vars", &merged.public_updates)).unwrap()
            );
            send(frame);
        }
        let list_count = merged.list_updates.len();
        for (cvid, ops) in merged.list_updates {
            let frame = format!(
                "{EVENT_MESSAGE_PREFIX}{}",
                serde_json::to_string(&("update_lists", json!({ cvid: ops }))).unwrap()
            );
            send(frame);
        }
        debug!(
            "批量上传完成: 私有 {} 公有 {} 列表 {list_count}",
            merged.private_updates.len(),
            merged.public_updates.len(),
        );
    }
}
