use std::collections::HashMap;
use std::io::{self};
use std::net::TcpStream;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::api::auth::CloudAuthenticator;
use serde_json::{Value, json};
use tungstenite::WebSocket;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::Message;
use tungstenite::stream::MaybeTlsStream;

// ==================== 错误类型 ====================
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("JSON 解析错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("WebSocket 错误: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("HTTP 请求错误: {0}")]
    Http(#[from] ureq::Error),
    #[error("连接超时")]
    Timeout,
    #[error("无效的变量类型")]
    InvalidVariableType,
    #[error("无效的列表元素类型")]
    InvalidListItemType,
    #[error("作品类型不支持")]
    UnsupportedWorkType,
    #[error("连接未就绪")]
    NotConnected,
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),
    #[error("其他错误: {0}")]
    Other(String),
}

// ==================== 枚举与配置 ====================
#[derive(Debug, Clone, PartialEq)]
pub enum EditorType {
    NEMO,
    KITTEN,
    NEKO, // Neko
    COCO,
}

impl EditorType {
    fn as_param(&self) -> (&str, &str) {
        match self {
            EditorType::NEMO => ("5", "2"),
            EditorType::KITTEN => ("1", "1"),
            EditorType::NEKO => ("5", "3"),
            EditorType::COCO => ("1", "1"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DataType {
    PrivateVariable = 0,
    PublicVariable = 1,
    List = 2,
}

impl TryFrom<i64> for DataType {
    type Error = CloudError;
    fn try_from(v: i64) -> Result<Self, Self::Error> {
        match v {
            0 => Ok(DataType::PrivateVariable),
            1 => Ok(DataType::PublicVariable),
            2 => Ok(DataType::List),
            _ => Err(CloudError::InvalidVariableType),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SendMessageType {
    Join,
    GetAllData,
    UpdatePrivateVariable,
    GetPrivateVariableRankingList,
    UpdatePublicVariable,
    UpdateList,
}

impl SendMessageType {
    fn as_str(&self) -> &str {
        match self {
            SendMessageType::Join => "join",
            SendMessageType::GetAllData => "list_variables",
            SendMessageType::UpdatePrivateVariable => "update_private_vars",
            SendMessageType::GetPrivateVariableRankingList => "list_ranking",
            SendMessageType::UpdatePublicVariable => "update_vars",
            SendMessageType::UpdateList => "update_lists",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReceiveMessageType {
    Join,
    ReceiveAllData,
    UpdatePrivateVariable,
    ReceivePrivateVariableRankingList,
    UpdatePublicVariable,
    UpdateList,
    IllegalEvent,
    OnlineUsersChange,
    Unknown(String),
}

impl From<&str> for ReceiveMessageType {
    fn from(s: &str) -> Self {
        match s {
            "connect_done" => ReceiveMessageType::Join,
            "list_variables_done" => ReceiveMessageType::ReceiveAllData,
            "update_private_vars_done" => ReceiveMessageType::UpdatePrivateVariable,
            "list_ranking_done" => ReceiveMessageType::ReceivePrivateVariableRankingList,
            "update_vars_done" => ReceiveMessageType::UpdatePublicVariable,
            "update_lists_done" => ReceiveMessageType::UpdateList,
            "illegal_event_done" => ReceiveMessageType::IllegalEvent,
            "online_users_change" => ReceiveMessageType::OnlineUsersChange,
            other => ReceiveMessageType::Unknown(other.to_string()),
        }
    }
}

/// 配置常量
const MAX_DISPLAY_LENGTH: usize = 50;
const TRUNCATED_SUFFIX: &str = "...";
const MAX_LIST_DISPLAY_ELEMENTS: usize = 6;
const PARTIAL_LIST_DISPLAY_COUNT: usize = 3;
const DATA_TIMEOUT_SECS: u64 = 30;
const DEFAULT_RANKING_LIMIT: u32 = 31;
const MAX_INACTIVITY_SECS: u64 = 30;
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const PING_INTERVAL_SECS: u64 = 25;
const PING_TIMEOUT_SECS: u64 = 5;
const RECONNECT_INTERVAL_SECS: u64 = 8;
const WS_PING_MESSAGE: &str = "2";
const WS_PONG_MESSAGE: &str = "3";
const WS_CONNECT_MESSAGE: &str = "40";
const WS_SERVER_CLOSED_PREFIX: &str = "41";
const WS_EVENT_MESSAGE_PREFIX: &str = "42";
const WS_HANDSHAKE_MESSAGE_PREFIX: &str = "0";
const MESSAGE_TYPE_LENGTH: usize = 2;
const BATCH_UPLOAD_INTERVAL_MS: u64 = 100;

// ==================== 命令模式（批量上传） ====================
#[derive(Debug, Clone)]
enum Command {
    VariableUpdate {
        cmd_type: String, // "update_private_vars" 或 "update_vars"
        data: Value,
    },
    ListUpdate {
        cvid: String,
        operations: Vec<Value>,
    },
}

// ==================== 云数据对象 ====================
type ChangeCallback = Box<dyn Fn(Value, Value, String) + Send + Sync>;
type RankingCallback = Box<dyn Fn(Vec<Value>) + Send + Sync>;
type ListOperationCallback = Box<dyn Fn(Vec<Value>) + Send + Sync>;
type EventCallback = Box<dyn Fn(Value) + Send + Sync>;

struct CloudDataItem {
    cloud_variable_id: String,
    name: String,
    value: Value,
    change_callbacks: Vec<ChangeCallback>,
}

impl CloudDataItem {
    fn new(cvid: String, name: String, value: Value) -> Self {
        CloudDataItem {
            cloud_variable_id: cvid,
            name,
            value,
            change_callbacks: Vec::new(),
        }
    }
    fn on_change(&mut self, cb: ChangeCallback) {
        self.change_callbacks.push(cb);
    }
    fn emit_change(&self, old: Value, new: Value, source: &str) {
        for cb in &self.change_callbacks {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cb(old.clone(), new.clone(), source.to_string());
            }));
        }
    }
}

struct CloudVariable {
    base: CloudDataItem,
}

impl CloudVariable {
    fn new(cvid: String, name: String, value: Value) -> Self {
        CloudVariable {
            base: CloudDataItem::new(cvid, name, value),
        }
    }
    fn get(&self) -> &Value {
        &self.base.value
    }
    fn set(&mut self, value: Value) -> Result<(), CloudError> {
        if !value.is_number() && !value.is_string() {
            return Err(CloudError::InvalidVariableType);
        }
        let old = self.base.value.clone();
        self.base.value = value;
        self.base.emit_change(old, self.base.value.clone(), "local");
        Ok(())
    }
    fn on_change(&mut self, cb: ChangeCallback) {
        self.base.on_change(cb);
    }
}

struct PrivateCloudVariable {
    var: CloudVariable,
    ranking_callbacks: Vec<RankingCallback>,
}

impl PrivateCloudVariable {
    fn new(cvid: String, name: String, value: Value) -> Self {
        PrivateCloudVariable {
            var: CloudVariable::new(cvid, name, value),
            ranking_callbacks: Vec::new(),
        }
    }
    fn get(&self) -> &Value {
        self.var.get()
    }
    fn set(&mut self, value: Value) -> Result<(), CloudError> {
        self.var.set(value)
    }
    fn on_change(&mut self, cb: ChangeCallback) {
        self.var.on_change(cb);
    }
    fn on_ranking_received(&mut self, cb: RankingCallback) {
        self.ranking_callbacks.push(cb);
    }
    fn emit_ranking(&self, data: Vec<Value>) {
        for cb in &self.ranking_callbacks {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                cb(data.clone());
            }));
        }
    }
}

struct PublicCloudVariable {
    var: CloudVariable,
}

impl PublicCloudVariable {
    fn new(cvid: String, name: String, value: Value) -> Self {
        PublicCloudVariable {
            var: CloudVariable::new(cvid, name, value),
        }
    }
    fn get(&self) -> &Value {
        self.var.get()
    }
    fn set(&mut self, value: Value) -> Result<(), CloudError> {
        self.var.set(value)
    }
    fn on_change(&mut self, cb: ChangeCallback) {
        self.var.on_change(cb);
    }
}

struct CloudList {
    base: CloudDataItem,
    operation_callbacks: HashMap<String, Vec<ListOperationCallback>>,
}

impl CloudList {
    fn new(cvid: String, name: String, value: Vec<Value>) -> Self {
        let mut hm = HashMap::new();
        for op in &[
            "push",
            "pop",
            "unshift",
            "shift",
            "insert",
            "remove",
            "replace",
            "clear",
            "replace_last",
        ] {
            hm.insert(op.to_string(), Vec::new());
        }
        CloudList {
            base: CloudDataItem::new(cvid, name, Value::Array(value)),
            operation_callbacks: hm,
        }
    }

    fn value(&self) -> &Vec<Value> {
        if let Value::Array(ref arr) = self.base.value {
            arr
        } else {
            unreachable!()
        }
    }

    fn value_mut(&mut self) -> &mut Vec<Value> {
        if let Value::Array(ref mut arr) = self.base.value {
            arr
        } else {
            unreachable!()
        }
    }

    fn on_change(&mut self, cb: ChangeCallback) {
        self.base.on_change(cb);
    }

    fn on_operation(&mut self, op: &str, cb: ListOperationCallback) {
        if let Some(cbs) = self.operation_callbacks.get_mut(op) {
            cbs.push(cb);
        }
    }

    fn emit_operation(&self, op: &str, args: Vec<Value>) {
        if let Some(cbs) = self.operation_callbacks.get(op) {
            for cb in cbs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cb(args.clone());
                }));
            }
        }
    }

    fn push(&mut self, item: Value) -> Result<(), CloudError> {
        if !item.is_number() && !item.is_string() {
            return Err(CloudError::InvalidListItemType);
        }
        let arr = self.value_mut();
        arr.push(item.clone());
        let idx = arr.len() - 1;
        self.emit_operation("push", vec![item, Value::Number(idx.into())]);
        Ok(())
    }

    fn pop(&mut self) -> Option<Value> {
        let arr = self.value_mut();
        let item = arr.pop();
        if let Some(ref v) = item {
            let len = arr.len();
            self.emit_operation("pop", vec![v.clone(), Value::Number(len.into())]);
        }
        item
    }

    fn unshift(&mut self, item: Value) -> Result<(), CloudError> {
        if !item.is_number() && !item.is_string() {
            return Err(CloudError::InvalidListItemType);
        }
        let arr = self.value_mut();
        arr.insert(0, item.clone());
        self.emit_operation("unshift", vec![item, Value::Number(0.into())]);
        Ok(())
    }

    fn shift(&mut self) -> Option<Value> {
        let arr = self.value_mut();
        if arr.is_empty() {
            return None;
        }
        let item = arr.remove(0);
        self.emit_operation("shift", vec![item.clone(), Value::Number(0.into())]);
        Some(item)
    }

    fn insert(&mut self, index: usize, item: Value) -> Result<(), CloudError> {
        if !item.is_number() && !item.is_string() {
            return Err(CloudError::InvalidListItemType);
        }
        let arr = self.value_mut();
        if index > arr.len() {
            return Err(CloudError::InvalidListItemType);
        }
        arr.insert(index, item.clone());
        self.emit_operation("insert", vec![item, json!(index)]);
        Ok(())
    }

    fn remove(&mut self, index: usize) -> Option<Value> {
        let arr = self.value_mut();
        if index >= arr.len() {
            return None;
        }
        let item = arr.remove(index);
        self.emit_operation("remove", vec![item.clone(), json!(index)]);
        Some(item)
    }

    fn replace(&mut self, index: usize, item: Value) -> Result<(), CloudError> {
        if !item.is_number() && !item.is_string() {
            return Err(CloudError::InvalidListItemType);
        }
        let arr = self.value_mut();
        if index >= arr.len() {
            return Err(CloudError::InvalidListItemType);
        }
        let old = arr[index].clone();
        arr[index] = item.clone();
        self.emit_operation("replace", vec![old, item, json!(index)]);
        Ok(())
    }

    fn replace_last(&mut self, item: Value) -> Result<(), CloudError> {
        if !item.is_number() && !item.is_string() {
            return Err(CloudError::InvalidListItemType);
        }
        let arr = self.value_mut();
        if arr.is_empty() {
            return Err(CloudError::InvalidListItemType);
        }
        let last_idx = arr.len() - 1;
        let old = arr[last_idx].clone();
        arr[last_idx] = item.clone();
        self.emit_operation("replace_last", vec![old, item]);
        Ok(())
    }

    fn clear(&mut self) {
        let arr = self.value_mut();
        let old = arr.clone();
        arr.clear();
        self.emit_operation("clear", vec![Value::Array(old)]);
    }

    fn length(&self) -> usize {
        self.value().len()
    }
}

// ==================== 内部数据结构（用于线程间传递） ====================
struct CloudConnectionData {
    data_ready: Arc<(Mutex<bool>, Condvar)>,
    online_users: Arc<RwLock<u32>>,
    private_variables: Arc<RwLock<HashMap<String, PrivateCloudVariable>>>,
    public_variables: Arc<RwLock<HashMap<String, PublicCloudVariable>>>,
    lists: Arc<RwLock<HashMap<String, CloudList>>>,
    command_queue: Arc<Mutex<Vec<Command>>>,
    event_callbacks: Arc<RwLock<HashMap<String, Vec<EventCallback>>>>,
    pending_ranking_requests: Arc<Mutex<Vec<String>>>,
}

// ==================== CloudConnection (已连接状态) ====================
pub struct CloudConnection {
    work_id: u64,
    editor: EditorType,
    authenticator: CloudAuthenticator,
    // 连接相关
    ws: Arc<Mutex<WebSocket<MaybeTlsStream<TcpStream>>>>,
    connected: Arc<(Mutex<bool>, Condvar)>,
    data_ready: Arc<(Mutex<bool>, Condvar)>,
    online_users: Arc<RwLock<u32>>,
    // 数据存储
    private_variables: Arc<RwLock<HashMap<String, PrivateCloudVariable>>>,
    public_variables: Arc<RwLock<HashMap<String, PublicCloudVariable>>>,
    lists: Arc<RwLock<HashMap<String, CloudList>>>,
    // 命令队列及上传线程
    command_queue: Arc<Mutex<Vec<Command>>>,
    upload_thread: Option<JoinHandle<()>>,
    stop_upload: Arc<Mutex<bool>>,
    // 事件回调
    event_callbacks: Arc<RwLock<HashMap<String, Vec<EventCallback>>>>,
    pending_ranking_requests: Arc<Mutex<Vec<String>>>,
    // 生命周期管理
    shutdown: Arc<Mutex<bool>>,
    reader_thread: Option<JoinHandle<()>>,
    ping_thread: Option<JoinHandle<()>>,
    // 重连参数
    auto_reconnect: bool,
}

impl CloudConnection {
    /// 由 Builder 调用此构造函数（内部用）
    fn new_connected(
        work_id: u64,
        editor: EditorType,
        authenticator: CloudAuthenticator,
        ws: WebSocket<MaybeTlsStream<TcpStream>>,
    ) -> Self {
        let ws = Arc::new(Mutex::new(ws));
        let connected = Arc::new((Mutex::new(true), Condvar::new()));
        let data_ready = Arc::new((Mutex::new(false), Condvar::new()));
        let online_users = Arc::new(RwLock::new(0));
        let private_variables = Arc::new(RwLock::new(HashMap::new()));
        let public_variables = Arc::new(RwLock::new(HashMap::new()));
        let lists = Arc::new(RwLock::new(HashMap::new()));
        let command_queue = Arc::new(Mutex::new(Vec::new()));
        let stop_upload = Arc::new(Mutex::new(false));
        let event_callbacks = Arc::new(RwLock::new(HashMap::new()));
        let pending_ranking_requests = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(Mutex::new(false));

        // 创建内部数据结构（用于线程传递）
        let conn_data = Arc::new(CloudConnectionData {
            data_ready: data_ready.clone(),
            online_users: online_users.clone(),
            private_variables: private_variables.clone(),
            public_variables: public_variables.clone(),
            lists: lists.clone(),
            command_queue: command_queue.clone(),
            event_callbacks: event_callbacks.clone(),
            pending_ranking_requests: pending_ranking_requests.clone(),
        });

        let mut conn = CloudConnection {
            work_id,
            editor,
            authenticator,
            ws: ws.clone(),
            connected: connected.clone(),
            data_ready: data_ready.clone(),
            online_users: online_users.clone(),
            private_variables: private_variables.clone(),
            public_variables: public_variables.clone(),
            lists: lists.clone(),
            command_queue: command_queue.clone(),
            upload_thread: None,
            stop_upload: stop_upload.clone(),
            event_callbacks: event_callbacks.clone(),
            pending_ranking_requests: pending_ranking_requests.clone(),
            shutdown: shutdown.clone(),
            reader_thread: None,
            ping_thread: None,
            auto_reconnect: true,
        };

        // 启动 reader 线程
        let reader_ws = ws.clone();
        let reader_shutdown = shutdown.clone();
        let reader_connected = connected.clone();
        let reader_conn_data = conn_data.clone();
        let reader = thread::spawn(move || {
            Self::reader_loop(
                reader_ws,
                reader_shutdown,
                reader_connected,
                reader_conn_data,
            );
        });
        conn.reader_thread = Some(reader);

        // 启动 upload 线程
        let upload_ws = ws;
        let upload_stop = stop_upload;
        let upload_shutdown = shutdown;
        let upload_queue = command_queue;
        let upload = thread::spawn(move || {
            Self::upload_loop(upload_ws, upload_shutdown, upload_stop, upload_queue);
        });
        conn.upload_thread = Some(upload);

        conn
    }

    // ---------- 公共 API ----------

    /// 等待数据就绪（阻塞式）
    pub fn wait_for_data(&self, timeout: Duration) -> Result<(), CloudError> {
        let (lock, cvar) = &*self.data_ready;
        let mut ready = lock.lock().unwrap();
        let deadline = Instant::now() + timeout;
        while !*ready {
            let now = Instant::now();
            if now >= deadline {
                return Err(CloudError::Timeout);
            }
            let remaining = deadline - now;
            let (result, timeout_res) = cvar.wait_timeout(ready, remaining).unwrap();
            ready = result;
            if timeout_res.timed_out() {
                return Err(CloudError::Timeout);
            }
        }
        Ok(())
    }

    /// 是否连接
    pub fn is_connected(&self) -> bool {
        let (lock, _) = &*self.connected;
        *lock.lock().unwrap()
    }

    /// 在线用户数
    pub fn online_users(&self) -> u32 {
        *self.online_users.read().unwrap()
    }

    /// 注册事件回调
    pub fn on<F>(&self, event: &str, callback: F) -> &Self
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        let mut cb_map = self.event_callbacks.write().unwrap();
        cb_map
            .entry(event.to_string())
            .or_default()
            .push(Box::new(callback));
        self
    }

    /// 发送底层消息
    fn send_message(&self, msg_type: SendMessageType, data: Value) -> Result<(), CloudError> {
        let payload = json!([msg_type.as_str(), data]);
        let msg = format!("42{}", serde_json::to_string(&payload)?);
        let mut ws = self.ws.lock().unwrap();
        ws.send(Message::Text(msg.into()))?;
        Ok(())
    }

    // ---------- 变量操作 ----------
    pub fn get_private_variable(&self, name: &str) -> Option<Value> {
        let vars = self.private_variables.read().unwrap();
        vars.get(name).map(|v| v.get().clone())
    }

    pub fn set_private_variable(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut vars = self.private_variables.write().unwrap();
        if let Some(var) = vars.get_mut(name) {
            var.set(value.clone())?;
            let cvid = var.var.base.cloud_variable_id.clone();
            drop(vars);
            let cmd = Command::VariableUpdate {
                cmd_type: "update_private_vars".into(),
                data: json!({"cvid": cvid, "value": value}),
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidVariableType)
        }
    }

    pub fn get_public_variable(&self, name: &str) -> Option<Value> {
        let vars = self.public_variables.read().unwrap();
        vars.get(name).map(|v| v.get().clone())
    }

    pub fn set_public_variable(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut vars = self.public_variables.write().unwrap();
        if let Some(var) = vars.get_mut(name) {
            var.set(value.clone())?;
            let cvid = var.var.base.cloud_variable_id.clone();
            drop(vars);
            let cmd = Command::VariableUpdate {
                cmd_type: "update_vars".into(),
                data: json!({"action": "set", "cvid": cvid, "value": value}),
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidVariableType)
        }
    }

    // ---------- 列表操作 ----------
    pub fn get_list(&self, name: &str) -> Option<Vec<Value>> {
        let lists = self.lists.read().unwrap();
        lists.get(name).map(|l| l.value().clone())
    }

    pub fn list_push(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.push(value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            let cmd = Command::ListUpdate {
                cvid,
                operations: vec![json!({"action": "append", "value": value})],
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_pop(&self, name: &str) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            if list.pop().is_some() {
                let cvid = list.base.cloud_variable_id.clone();
                drop(lists);
                let cmd = Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "delete", "nth": "last"})],
                };
                self.command_queue.lock().unwrap().push(cmd);
                Ok(self)
            } else {
                Err(CloudError::InvalidListItemType)
            }
        } else {
            Err(CloudError::NotConnected)
        }
    }

    pub fn list_unshift(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.unshift(value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            let cmd = Command::ListUpdate {
                cvid,
                operations: vec![json!({"action": "unshift", "value": value})],
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_shift(&self, name: &str) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            if list.shift().is_some() {
                let cvid = list.base.cloud_variable_id.clone();
                drop(lists);
                let cmd = Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "delete", "nth": 1})],
                };
                self.command_queue.lock().unwrap().push(cmd);
                Ok(self)
            } else {
                Err(CloudError::InvalidListItemType)
            }
        } else {
            Err(CloudError::NotConnected)
        }
    }

    pub fn list_insert(&self, name: &str, index: usize, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.insert(index, value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            let cmd = Command::ListUpdate {
                cvid,
                operations: vec![json!({"action": "insert", "nth": index + 1, "value": value})],
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_remove(&self, name: &str, index: usize) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            if list.remove(index).is_some() {
                let cvid = list.base.cloud_variable_id.clone();
                drop(lists);
                let cmd = Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "delete", "nth": index + 1})],
                };
                self.command_queue.lock().unwrap().push(cmd);
                Ok(self)
            } else {
                Err(CloudError::InvalidListItemType)
            }
        } else {
            Err(CloudError::NotConnected)
        }
    }

    pub fn list_replace(
        &self,
        name: &str,
        index: usize,
        value: Value,
    ) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.replace(index, value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            let cmd = Command::ListUpdate {
                cvid,
                operations: vec![json!({"action": "replace", "nth": index + 1, "value": value})],
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_replace_last(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.replace_last(value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            let cmd = Command::ListUpdate {
                cvid,
                operations: vec![json!({"action": "replace", "nth": "last", "value": value})],
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_clear(&self, name: &str) -> Result<&Self, CloudError> {
        let mut lists = self.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.clear();
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            let cmd = Command::ListUpdate {
                cvid,
                operations: vec![json!({"action": "delete", "nth": "all"})],
            };
            self.command_queue.lock().unwrap().push(cmd);
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    /// 请求排行榜
    pub fn request_ranking(
        &self,
        variable_name: &str,
        limit: u32,
        order: i32,
    ) -> Result<&Self, CloudError> {
        let vars = self.private_variables.read().unwrap();
        if let Some(var) = vars.get(variable_name) {
            let cvid = var.var.base.cloud_variable_id.clone();
            self.pending_ranking_requests
                .lock()
                .unwrap()
                .push(cvid.clone());
            drop(vars);
            let data = json!({"cvid": cvid, "limit": limit, "order_type": order});
            self.send_message(SendMessageType::GetPrivateVariableRankingList, data)?;
            Ok(self)
        } else {
            Err(CloudError::InvalidVariableType)
        }
    }

    /// 关闭连接
    pub fn close(mut self) {
        *self.shutdown.lock().unwrap() = true;
        *self.stop_upload.lock().unwrap() = true;
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.upload_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.ping_thread.take() {
            let _ = handle.join();
        }
    }

    // ---------- 内部方法 ----------

    fn emit_event(&self, event: &str, data: Value) {
        let cb_map = self.event_callbacks.read().unwrap();
        if let Some(cbs) = cb_map.get(event) {
            for cb in cbs {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cb(data.clone());
                }));
            }
        }
    }

    fn reader_loop(
        ws: Arc<Mutex<WebSocket<MaybeTlsStream<TcpStream>>>>,
        shutdown: Arc<Mutex<bool>>,
        connected: Arc<(Mutex<bool>, Condvar)>,
        conn_data: Arc<CloudConnectionData>,
    ) {
        loop {
            if *shutdown.lock().unwrap() {
                break;
            }
            let msg = {
                let mut ws_lock = ws.lock().unwrap();
                match ws_lock.read() {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("WebSocket read error: {}", e);
                        break;
                    }
                }
            };
            Self::process_message(msg, &connected, &conn_data, &shutdown);
        }
        let (lock, cvar) = &*connected;
        *lock.lock().unwrap() = false;
        cvar.notify_all();
    }

    fn process_message(
        msg: Message,
        _connected: &Arc<(Mutex<bool>, Condvar)>,
        conn_data: &Arc<CloudConnectionData>,
        shutdown: &Arc<Mutex<bool>>,
    ) {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => match String::from_utf8(b.to_vec()) {
                Ok(s) => s,
                Err(_) => return,
            },
            Message::Ping(_) => {
                return;
            }
            Message::Close(_) => {
                *shutdown.lock().unwrap() = true;
                return;
            }
            _ => return,
        };

        if text == WS_PING_MESSAGE || text == WS_PONG_MESSAGE {
            return;
        }

        if text.starts_with(WS_HANDSHAKE_MESSAGE_PREFIX) {
            // 握手处理
        } else if text.starts_with(WS_SERVER_CLOSED_PREFIX) {
            *shutdown.lock().unwrap() = true;
        } else if text.starts_with(WS_EVENT_MESSAGE_PREFIX) {
            let payload = &text[MESSAGE_TYPE_LENGTH..];
            if let Ok(arr) = serde_json::from_str::<Vec<Value>>(payload) {
                if arr.len() >= 2 {
                    let msg_type = arr[0].as_str().unwrap_or("");
                    let msg_data = arr[1].clone();
                    let parsed = if let Some(inner) = msg_data.as_str() {
                        serde_json::from_str::<Value>(inner).unwrap_or(msg_data)
                    } else {
                        msg_data
                    };
                    match ReceiveMessageType::from(msg_type) {
                        ReceiveMessageType::Join => {
                            // 发送获取所有数据请求
                        }
                        ReceiveMessageType::ReceiveAllData => {
                            if let Some(items) = parsed.as_array() {
                                for item in items {
                                    if let Some(obj) = item.as_object() {
                                        let cvid = obj
                                            .get("cvid")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let name = obj
                                            .get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        let value =
                                            obj.get("value").cloned().unwrap_or(Value::Null);
                                        let dtype: i64 =
                                            obj.get("type").and_then(|v| v.as_i64()).unwrap_or(-1);
                                        if let Ok(dt) = DataType::try_from(dtype) {
                                            match dt {
                                                DataType::PrivateVariable => {
                                                    conn_data
                                                        .private_variables
                                                        .write()
                                                        .unwrap()
                                                        .insert(
                                                            name.clone(),
                                                            PrivateCloudVariable::new(
                                                                cvid, name, value,
                                                            ),
                                                        );
                                                }
                                                DataType::PublicVariable => {
                                                    conn_data
                                                        .public_variables
                                                        .write()
                                                        .unwrap()
                                                        .insert(
                                                            name.clone(),
                                                            PublicCloudVariable::new(
                                                                cvid, name, value,
                                                            ),
                                                        );
                                                }
                                                DataType::List => {
                                                    let arr = value
                                                        .as_array()
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    conn_data.lists.write().unwrap().insert(
                                                        name.clone(),
                                                        CloudList::new(cvid, name, arr),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            let (lock, cvar) = &*conn_data.data_ready;
                            *lock.lock().unwrap() = true;
                            cvar.notify_all();
                        }
                        ReceiveMessageType::UpdatePrivateVariable => {
                            if let Some(obj) = parsed.as_object() {
                                let cvid = obj.get("cvid").and_then(|v| v.as_str());
                                let value = obj.get("value");
                                if let (Some(cvid), Some(value)) = (cvid, value) {
                                    let mut vars = conn_data.private_variables.write().unwrap();
                                    for var in vars.values_mut() {
                                        if var.var.base.cloud_variable_id == cvid {
                                            let old = var.get().clone();
                                            var.var.base.value = value.clone();
                                            var.var.base.emit_change(old, value.clone(), "cloud");
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        ReceiveMessageType::UpdatePublicVariable => {
                            if let Some(arr) = parsed.as_array() {
                                for item in arr {
                                    if let Some(obj) = item.as_object() {
                                        let cvid = obj.get("cvid").and_then(|v| v.as_str());
                                        let value = obj.get("value");
                                        if let (Some(cvid), Some(value)) = (cvid, value) {
                                            let mut vars =
                                                conn_data.public_variables.write().unwrap();
                                            for var in vars.values_mut() {
                                                if var.var.base.cloud_variable_id == cvid {
                                                    let old = var.get().clone();
                                                    var.var.base.value = value.clone();
                                                    var.var.base.emit_change(
                                                        old,
                                                        value.clone(),
                                                        "cloud",
                                                    );
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ReceiveMessageType::ReceivePrivateVariableRankingList => {
                            let mut pending = conn_data.pending_ranking_requests.lock().unwrap();
                            if pending.is_empty() {
                                return;
                            }
                            let cvid = pending.remove(0);
                            drop(pending);
                            if let Some(items) = parsed.get("items").and_then(|v| v.as_array()) {
                                let rankings: Vec<Value> = items.iter().filter_map(|item| {
                                    let obj = item.as_object()?;
                                    Some(json!({
                                        "value": obj.get("value").cloned().unwrap_or(Value::Null),
                                        "user": {
                                            "id": obj.get("identifier").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0),
                                            "nickname": obj.get("nickname").and_then(|v| v.as_str()).unwrap_or(""),
                                            "avatar_url": obj.get("avatar_url").and_then(|v| v.as_str()).unwrap_or("")
                                        }
                                    }))
                                }).collect();
                                let vars = conn_data.private_variables.read().unwrap();
                                for var in vars.values() {
                                    if var.var.base.cloud_variable_id == cvid {
                                        var.emit_ranking(rankings.clone());
                                        break;
                                    }
                                }
                            }
                        }
                        ReceiveMessageType::UpdateList => {
                            if let Some(obj) = parsed.as_object() {
                                let mut lists_guard = conn_data.lists.write().unwrap();
                                for (cvid, ops_val) in obj {
                                    if let Some(list) = lists_guard.get_mut(cvid.as_str()) {
                                        if let Some(ops_arr) = ops_val.as_array() {
                                            for op in ops_arr {
                                                if let Some(action) =
                                                    op.get("action").and_then(|v| v.as_str())
                                                {
                                                    match action {
                                                        "append" => {
                                                            if let Some(val) = op.get("value") {
                                                                let _ = list.push(val.clone());
                                                            }
                                                        }
                                                        "unshift" => {
                                                            if let Some(val) = op.get("value") {
                                                                let _ = list.unshift(val.clone());
                                                            }
                                                        }
                                                        "insert" => {
                                                            if let (Some(nth), Some(val)) = (
                                                                op.get("nth")
                                                                    .and_then(|v| v.as_u64()),
                                                                op.get("value"),
                                                            ) {
                                                                let _ = list.insert(
                                                                    (nth as usize)
                                                                        .saturating_sub(1),
                                                                    val.clone(),
                                                                );
                                                            }
                                                        }
                                                        "delete" => {
                                                            if let Some(nth) = op.get("nth") {
                                                                match nth {
                                                                    Value::String(s)
                                                                        if s == "last" =>
                                                                    {
                                                                        list.pop();
                                                                    }
                                                                    Value::String(s)
                                                                        if s == "all" =>
                                                                    {
                                                                        list.clear();
                                                                    }
                                                                    Value::Number(n) => {
                                                                        if let Some(idx) =
                                                                            n.as_u64()
                                                                        {
                                                                            list.remove(
                                                                                (idx as usize)
                                                                                    .saturating_sub(
                                                                                        1,
                                                                                    ),
                                                                            );
                                                                        }
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                        }
                                                        "replace" => {
                                                            if let (Some(nth), Some(val)) =
                                                                (op.get("nth"), op.get("value"))
                                                            {
                                                                match nth {
                                                                    Value::String(s)
                                                                        if s == "last" =>
                                                                    {
                                                                        let _ = list.replace_last(
                                                                            val.clone(),
                                                                        );
                                                                    }
                                                                    Value::Number(n) => {
                                                                        if let Some(idx) =
                                                                            n.as_u64()
                                                                        {
                                                                            let _ = list.replace(
                                                                                (idx as usize)
                                                                                    .saturating_sub(
                                                                                        1,
                                                                                    ),
                                                                                val.clone(),
                                                                            );
                                                                        }
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                        }
                                                        _ => {}
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        ReceiveMessageType::OnlineUsersChange => {
                            if let Some(total) = parsed.get("total").and_then(|v| v.as_u64()) {
                                *conn_data.online_users.write().unwrap() = total as u32;
                            }
                        }
                        ReceiveMessageType::IllegalEvent => {
                            eprintln!("检测到非法事件");
                        }
                        ReceiveMessageType::Unknown(t) => {
                            eprintln!("未知消息类型: {}", t);
                        }
                    }
                }
            }
        }
    }

    fn upload_loop(
        ws: Arc<Mutex<WebSocket<MaybeTlsStream<TcpStream>>>>,
        shutdown: Arc<Mutex<bool>>,
        stop: Arc<Mutex<bool>>,
        queue: Arc<Mutex<Vec<Command>>>,
    ) {
        loop {
            if *shutdown.lock().unwrap() || *stop.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(BATCH_UPLOAD_INTERVAL_MS));
            let mut q = queue.lock().unwrap();
            if q.is_empty() {
                continue;
            }
            let commands: Vec<Command> = q.drain(..).collect();
            drop(q);

            let mut private_updates = Vec::new();
            let mut public_updates = Vec::new();
            let mut list_updates: HashMap<String, Vec<Value>> = HashMap::new();

            for cmd in commands {
                match cmd {
                    Command::VariableUpdate { cmd_type, data } => {
                        if cmd_type == "update_private_vars" {
                            private_updates.push(data);
                        } else if cmd_type == "update_vars" {
                            public_updates.push(data);
                        }
                    }
                    Command::ListUpdate { cvid, operations } => {
                        list_updates.entry(cvid).or_default().extend(operations);
                    }
                }
            }

            let mut ws_lock = ws.lock().unwrap();
            if !private_updates.is_empty() {
                let payload = json!(["update_private_vars", private_updates]);
                let msg = format!("42{}", serde_json::to_string(&payload).unwrap());
                if ws_lock.send(Message::Text(msg.into())).is_err() {
                    break;
                }
            }
            if !public_updates.is_empty() {
                let payload = json!(["update_vars", public_updates]);
                let msg = format!("42{}", serde_json::to_string(&payload).unwrap());
                if ws_lock.send(Message::Text(msg.into())).is_err() {
                    break;
                }
            }
            for (cvid, ops) in list_updates {
                let payload = json!(["update_lists", {cvid: ops}]);
                let msg = format!("42{}", serde_json::to_string(&payload).unwrap());
                if ws_lock.send(Message::Text(msg.into())).is_err() {
                    break;
                }
            }
        }
    }
    pub fn get_all_private_variable_names(&self) -> Vec<String> {
        self.private_variables
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// 获取所有公共变量名称
    pub fn get_all_public_variable_names(&self) -> Vec<String> {
        self.public_variables
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// 获取所有列表名称
    pub fn get_all_list_names(&self) -> Vec<String> {
        self.lists.read().unwrap().keys().cloned().collect()
    }
}

// ==================== Builder ====================
pub struct CloudConnectionBuilder {
    work_id: u64,
    editor: EditorType,
    auth_token: Option<String>,
}

impl CloudConnectionBuilder {
    pub fn new(work_id: u64) -> Self {
        CloudConnectionBuilder {
            work_id,
            editor: EditorType::KITTEN,
            auth_token: None,
        }
    }

    pub fn with_editor(mut self, editor: EditorType) -> Self {
        self.editor = editor;
        self
    }

    pub fn with_auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    /// 建立连接，返回 CloudConnection
    pub fn connect(self) -> Result<CloudConnection, CloudError> {
        let mut authenticator = CloudAuthenticator::new(self.auth_token);
        let (auth_type, stag) = self.editor.as_param();
        let url = format!(
            "wss://socketcv.codemao.cn:9096/cloudstorage/?session_id={}&authorization_type={}&stag={}&EIO=3&transport=websocket",
            self.work_id, auth_type, stag
        );
        let mut req = url.into_client_request().unwrap();
        let device_auth_result = authenticator.generate_x_device_auth();
        let device_auth_str = device_auth_result
            .map_err(|e| CloudError::Other(format!("Failed to generate device auth: {}", e)))?;

        req.headers_mut().insert(
            http::header::HeaderName::from_static("x-creation-tools-device-auth"),
            http::header::HeaderValue::from_str(&device_auth_str)
                .map_err(|e| CloudError::Other(format!("Invalid header value: {}", e)))?,
        );
        if let Some(token) = authenticator.authorization_token() {
            req.headers_mut().insert(
                http::header::COOKIE,
                http::header::HeaderValue::from_str(&format!("Authorization={}", token)).unwrap(),
            );
        }

        let (mut ws, _resp) = tungstenite::connect(req)?;
        let _handshake_msg = ws.read()?;
        ws.send(Message::Text(WS_CONNECT_MESSAGE.into()))?;
        Ok(CloudConnection::new_connected(
            self.work_id,
            self.editor,
            authenticator,
            ws,
        ))
    }
}

// ==================== 辅助函数 ====================
pub fn truncate_value(value: &Value) -> String {
    match value {
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.len() <= MAX_DISPLAY_LENGTH {
                s.clone()
            } else {
                let half = (MAX_DISPLAY_LENGTH - TRUNCATED_SUFFIX.len()) / 2;
                format!("{}{}{}", &s[..half], TRUNCATED_SUFFIX, &s[s.len() - half..])
            }
        }
        Value::Array(arr) => {
            if arr.len() <= MAX_LIST_DISPLAY_ELEMENTS {
                format!("{:?}", arr)
            } else {
                let first: Vec<String> = arr[..PARTIAL_LIST_DISPLAY_COUNT]
                    .iter()
                    .map(|v| v.to_string())
                    .collect();
                let last: Vec<String> = arr[arr.len() - PARTIAL_LIST_DISPLAY_COUNT..]
                    .iter()
                    .map(|v| v.to_string())
                    .collect();
                format!("[{}, ..., {}]", first.join(", "), last.join(", "))
            }
        }
        _ => value.to_string(),
    }
}
