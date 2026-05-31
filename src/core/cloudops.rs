use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::Notify as AsyncNotify;
use wreq::Client;
use wreq_util::Emulation;

use crate::api::auth::CloudAuthenticator;

// ==================== 常量 ====================
const MAX_DISPLAY_LENGTH: usize = 50;
const TRUNCATED_SUFFIX: &str = "...";
const MAX_LIST_DISPLAY_ELEMENTS: usize = 6;
const PARTIAL_LIST_DISPLAY_COUNT: usize = 3;
const BATCH_UPLOAD_INTERVAL_MS: u64 = 100;

// ==================== 错误类型 ====================
#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("JSON 解析错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("WebSocket 错误: {0}")]
    WebSocket(String),
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
    #[error("其他错误: {0}")]
    Other(String),
}

impl From<wreq::Error> for CloudError {
    fn from(e: wreq::Error) -> Self {
        CloudError::WebSocket(e.to_string())
    }
}

// ==================== 枚举与配置 ====================
#[derive(Debug, Clone, PartialEq)]
pub enum EditorType {
    NEMO,
    KITTEN,
    NEKO,
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

// ==================== 回调类型 ====================
type ChangeCallback = Box<dyn Fn(Value, Value, String) + Send + Sync>;
type RankingCallback = Box<dyn Fn(Vec<Value>) + Send + Sync>;
type ListOperationCallback = Box<dyn Fn(Vec<Value>) + Send + Sync>;
type EventCallback = Box<dyn Fn(Value) + Send + Sync>;

// ==================== 云数据对象 ====================
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

// ==================== 命令模式（批量上传） ====================
#[derive(Debug, Clone)]
enum Command {
    VariableUpdate {
        cmd_type: String,
        data: Value,
    },
    ListUpdate {
        cvid: String,
        operations: Vec<Value>,
    },
}

// ==================== 内部共享状态 ====================
struct CloudSharedState {
    data_ready: (Mutex<bool>, Condvar),
    online_users: RwLock<u32>,
    private_variables: RwLock<HashMap<String, PrivateCloudVariable>>,
    public_variables: RwLock<HashMap<String, PublicCloudVariable>>,
    lists: RwLock<HashMap<String, CloudList>>,
    command_queue: Mutex<Vec<Command>>,
    event_callbacks: RwLock<HashMap<String, Vec<EventCallback>>>,
    pending_ranking_requests: Mutex<Vec<String>>,
    shutdown: Mutex<bool>,
}

// ==================== CloudConnection ====================
pub struct CloudConnection {
    work_id: u64,
    editor: EditorType,
    authenticator: CloudAuthenticator,
    state: Arc<CloudSharedState>,
    runtime: Runtime,
    writer_tx: tokio::sync::mpsc::Sender<String>,
    _tasks: Vec<tokio::task::JoinHandle<()>>,
    connected: Arc<(Mutex<bool>, AsyncNotify)>,
}

impl CloudConnection {
    // ---------- 构造与初始化 ----------

    async fn new_async(
        work_id: u64,
        editor: EditorType,
        authenticator: CloudAuthenticator,
        url: &str,
        device_auth: &str,
        auth_token: Option<String>,
    ) -> Result<Self, CloudError> {
        // ★ 创建带指纹模拟的 Client
        let client = Client::builder().emulation(Emulation::Chrome137).build()?;

        let mut req = client.websocket(url);
        req = req.header("X-Creation-Tools-Device-Auth", device_auth);
        req = req.header("Origin", "https://socketcv.codemao.cn:9096");
        if let Some(token) = auth_token {
            req = req.header("Cookie", format!("Authorization={}", token));
        }

        let ws_response = req.send().await?;
        let (mut ws_sink, mut ws_stream) = ws_response.into_websocket().await?.split();

        // 创建状态
        let state = Arc::new(CloudSharedState {
            data_ready: (Mutex::new(false), Condvar::new()),
            online_users: RwLock::new(0),
            private_variables: RwLock::new(HashMap::new()),
            public_variables: RwLock::new(HashMap::new()),
            lists: RwLock::new(HashMap::new()),
            command_queue: Mutex::new(Vec::new()),
            event_callbacks: RwLock::new(HashMap::new()),
            pending_ranking_requests: Mutex::new(Vec::new()),
            shutdown: Mutex::new(false),
        });

        let connected = Arc::new((Mutex::new(true), AsyncNotify::new()));

        // 创建消息通道：writer_tx -> writer 任务 -> ws_sink
        let (writer_tx, mut writer_rx) = tokio::sync::mpsc::channel::<String>(256);

        // writer 任务
        let writer_handle = tokio::spawn(async move {
            while let Some(msg) = writer_rx.recv().await {
                if let Err(e) = ws_sink.send(wreq::Message::text(msg)).await {
                    eprintln!("WS send error: {}", e);
                    break;
                }
            }
            let _ = ws_sink.close().await;
        });

        // reader 任务
        let reader_state = state.clone();
        let reader_connected = connected.clone();
        let reader_handle = tokio::spawn(async move {
            while let Some(msg) = ws_stream.next().await {
                match msg {
                    Ok(wreq::Message::Text(text)) => {
                        Self::process_message(&text, &reader_state, &reader_connected);
                    }
                    Ok(wreq::Message::Close(_)) => {
                        break;
                    }
                    Err(e) => {
                        eprintln!("WS read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            *reader_connected.0.lock().unwrap() = false;
            reader_connected.1.notify_one();
        });

        // upload 循环任务
        let upload_state = state.clone();
        let upload_tx = writer_tx.clone();
        let upload_handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(BATCH_UPLOAD_INTERVAL_MS)).await;

                if *upload_state.shutdown.lock().unwrap() {
                    break;
                }

                let commands: Vec<Command> = {
                    let mut q = upload_state.command_queue.lock().unwrap();
                    if q.is_empty() {
                        continue; // continue 会离开外层 loop，锁在此作用域结束时自动释放
                    }
                    q.drain(..).collect()
                }; // 锁在此处自动释放

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

                if !private_updates.is_empty() {
                    let payload = json!(["update_private_vars", private_updates]);
                    let msg = format!("42{}", serde_json::to_string(&payload).unwrap());
                    let _ = upload_tx.send(msg).await;
                }
                if !public_updates.is_empty() {
                    let payload = json!(["update_vars", public_updates]);
                    let msg = format!("42{}", serde_json::to_string(&payload).unwrap());
                    let _ = upload_tx.send(msg).await;
                }
                for (cvid, ops) in list_updates {
                    let payload = json!(["update_lists", {cvid: ops}]);
                    let msg = format!("42{}", serde_json::to_string(&payload).unwrap());
                    let _ = upload_tx.send(msg).await;
                }
            }
        });

        // 发送初始握手 "40"
        writer_tx
            .send("40".to_string())
            .await
            .map_err(|_| CloudError::WebSocket("Failed to send init message".into()))?;

        let runtime = Runtime::new().map_err(|e| CloudError::Other(e.to_string()))?;

        let conn = CloudConnection {
            work_id,
            editor,
            authenticator,
            state,
            runtime,
            writer_tx,
            _tasks: vec![writer_handle, reader_handle, upload_handle],
            connected,
        };

        Ok(conn)
    }

    // ---------- 内部消息发送 ----------

    fn send_message_async(&self, msg_type: SendMessageType, data: Value) -> Result<(), CloudError> {
        let payload = json!([msg_type.as_str(), data]);
        let msg = format!("42{}", serde_json::to_string(&payload)?);
        let tx = self.writer_tx.clone();
        self.runtime.block_on(async {
            tx.send(msg)
                .await
                .map_err(|_| CloudError::WebSocket("Send channel closed".into()))
        })
    }

    fn send_raw(&self, msg: String) -> Result<(), CloudError> {
        let tx = self.writer_tx.clone();
        self.runtime.block_on(async {
            tx.send(msg)
                .await
                .map_err(|_| CloudError::WebSocket("Send channel closed".into()))
        })
    }

    // ---------- 公共 API ----------

    /// 等待数据就绪（阻塞式）
    pub fn wait_for_data(&self, timeout: Duration) -> Result<(), CloudError> {
        let (lock, cvar) = &self.state.data_ready;
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
        *self.connected.0.lock().unwrap()
    }

    /// 在线用户数
    pub fn online_users(&self) -> u32 {
        *self.state.online_users.read().unwrap()
    }

    /// 注册事件回调
    pub fn on<F>(&self, event: &str, callback: F) -> &Self
    where
        F: Fn(Value) + Send + Sync + 'static,
    {
        self.state
            .event_callbacks
            .write()
            .unwrap()
            .entry(event.to_string())
            .or_default()
            .push(Box::new(callback));
        self
    }

    /// 获取所有私有变量名称
    pub fn get_all_private_variable_names(&self) -> Vec<String> {
        self.state
            .private_variables
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// 获取所有公共变量名称
    pub fn get_all_public_variable_names(&self) -> Vec<String> {
        self.state
            .public_variables
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// 获取所有列表名称
    pub fn get_all_list_names(&self) -> Vec<String> {
        self.state.lists.read().unwrap().keys().cloned().collect()
    }

    // ---------- 私有变量操作 ----------

    pub fn get_private_variable(&self, name: &str) -> Option<Value> {
        self.state
            .private_variables
            .read()
            .unwrap()
            .get(name)
            .map(|v| v.get().clone())
    }

    pub fn set_private_variable(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut vars = self.state.private_variables.write().unwrap();
        if let Some(var) = vars.get_mut(name) {
            var.set(value.clone())?;
            let cvid = var.var.base.cloud_variable_id.clone();
            drop(vars);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::VariableUpdate {
                    cmd_type: "update_private_vars".into(),
                    data: json!({"cvid": cvid, "value": value}),
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidVariableType)
        }
    }

    pub fn on_private_variable_change<F>(&self, name: &str, cb: F) -> &Self
    where
        F: Fn(Value, Value, String) + Send + Sync + 'static,
    {
        if let Some(var) = self.state.private_variables.write().unwrap().get_mut(name) {
            var.on_change(Box::new(cb));
        }
        self
    }

    pub fn request_ranking(
        &self,
        variable_name: &str,
        limit: u32,
        order: i32,
    ) -> Result<&Self, CloudError> {
        let vars = self.state.private_variables.read().unwrap();
        if let Some(var) = vars.get(variable_name) {
            let cvid = var.var.base.cloud_variable_id.clone();
            self.state
                .pending_ranking_requests
                .lock()
                .unwrap()
                .push(cvid.clone());
            drop(vars);
            let data = json!({"cvid": cvid, "limit": limit, "order_type": order});
            self.send_message_async(SendMessageType::GetPrivateVariableRankingList, data)?;
            Ok(self)
        } else {
            Err(CloudError::InvalidVariableType)
        }
    }

    pub fn on_private_variable_ranking<F>(&self, name: &str, cb: F) -> &Self
    where
        F: Fn(Vec<Value>) + Send + Sync + 'static,
    {
        if let Some(var) = self.state.private_variables.write().unwrap().get_mut(name) {
            var.on_ranking_received(Box::new(cb));
        }
        self
    }

    // ---------- 公共变量操作 ----------

    pub fn get_public_variable(&self, name: &str) -> Option<Value> {
        self.state
            .public_variables
            .read()
            .unwrap()
            .get(name)
            .map(|v| v.get().clone())
    }

    pub fn set_public_variable(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut vars = self.state.public_variables.write().unwrap();
        if let Some(var) = vars.get_mut(name) {
            var.set(value.clone())?;
            let cvid = var.var.base.cloud_variable_id.clone();
            drop(vars);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::VariableUpdate {
                    cmd_type: "update_vars".into(),
                    data: json!({"action": "set", "cvid": cvid, "value": value}),
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidVariableType)
        }
    }

    pub fn on_public_variable_change<F>(&self, name: &str, cb: F) -> &Self
    where
        F: Fn(Value, Value, String) + Send + Sync + 'static,
    {
        if let Some(var) = self.state.public_variables.write().unwrap().get_mut(name) {
            var.on_change(Box::new(cb));
        }
        self
    }

    // ---------- 列表操作 ----------

    pub fn get_list(&self, name: &str) -> Option<Vec<Value>> {
        self.state
            .lists
            .read()
            .unwrap()
            .get(name)
            .map(|l| l.value().clone())
    }

    pub fn get_list_length(&self, name: &str) -> Option<usize> {
        self.state
            .lists
            .read()
            .unwrap()
            .get(name)
            .map(|l| l.length())
    }

    pub fn get_list_item(&self, name: &str, index: usize) -> Option<Value> {
        self.state
            .lists
            .read()
            .unwrap()
            .get(name)
            .and_then(|l| l.value().get(index).cloned())
    }

    pub fn list_push(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.push(value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "append", "value": value})],
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_pop(&self, name: &str) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            if list.pop().is_some() {
                let cvid = list.base.cloud_variable_id.clone();
                drop(lists);
                self.state
                    .command_queue
                    .lock()
                    .unwrap()
                    .push(Command::ListUpdate {
                        cvid,
                        operations: vec![json!({"action": "delete", "nth": "last"})],
                    });
                Ok(self)
            } else {
                Err(CloudError::InvalidListItemType)
            }
        } else {
            Err(CloudError::NotConnected)
        }
    }

    pub fn list_unshift(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.unshift(value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "unshift", "value": value})],
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_shift(&self, name: &str) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            if list.shift().is_some() {
                let cvid = list.base.cloud_variable_id.clone();
                drop(lists);
                self.state
                    .command_queue
                    .lock()
                    .unwrap()
                    .push(Command::ListUpdate {
                        cvid,
                        operations: vec![json!({"action": "delete", "nth": 1})],
                    });
                Ok(self)
            } else {
                Err(CloudError::InvalidListItemType)
            }
        } else {
            Err(CloudError::NotConnected)
        }
    }

    pub fn list_insert(&self, name: &str, index: usize, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.insert(index, value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "insert", "nth": index + 1, "value": value})],
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_remove(&self, name: &str, index: usize) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            if list.remove(index).is_some() {
                let cvid = list.base.cloud_variable_id.clone();
                drop(lists);
                self.state
                    .command_queue
                    .lock()
                    .unwrap()
                    .push(Command::ListUpdate {
                        cvid,
                        operations: vec![json!({"action": "delete", "nth": index + 1})],
                    });
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
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.replace(index, value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::ListUpdate {
                    cvid,
                    operations: vec![
                        json!({"action": "replace", "nth": index + 1, "value": value}),
                    ],
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_replace_last(&self, name: &str, value: Value) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.replace_last(value.clone())?;
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "replace", "nth": "last", "value": value})],
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn list_clear(&self, name: &str) -> Result<&Self, CloudError> {
        let mut lists = self.state.lists.write().unwrap();
        if let Some(list) = lists.get_mut(name) {
            list.clear();
            let cvid = list.base.cloud_variable_id.clone();
            drop(lists);
            self.state
                .command_queue
                .lock()
                .unwrap()
                .push(Command::ListUpdate {
                    cvid,
                    operations: vec![json!({"action": "delete", "nth": "all"})],
                });
            Ok(self)
        } else {
            Err(CloudError::InvalidListItemType)
        }
    }

    pub fn on_list_change<F>(&self, name: &str, cb: F) -> &Self
    where
        F: Fn(Value, Value, String) + Send + Sync + 'static,
    {
        if let Some(list) = self.state.lists.write().unwrap().get_mut(name) {
            list.on_change(Box::new(cb));
        }
        self
    }

    pub fn on_list_operation<F>(&self, name: &str, op: &str, cb: F) -> &Self
    where
        F: Fn(Vec<Value>) + Send + Sync + 'static,
    {
        if let Some(list) = self.state.lists.write().unwrap().get_mut(name) {
            list.on_operation(op, Box::new(cb));
        }
        self
    }

    // ---------- 清理 ----------

    /// 关闭连接
    pub fn close(mut self) {
        *self.state.shutdown.lock().unwrap() = true;
        self.runtime.block_on(async {
            for task in self._tasks.drain(..) {
                task.abort();
            }
        });
    }
}

impl Drop for CloudConnection {
    fn drop(&mut self) {
        *self.state.shutdown.lock().unwrap() = true;
    }
}

// ==================== 消息处理（内部） ====================

impl CloudConnection {
    fn process_message(
        text: &str,
        state: &Arc<CloudSharedState>,
        _connected: &Arc<(Mutex<bool>, AsyncNotify)>,
    ) {
        if text == "2" || text == "3" {
            return;
        }

        if text.starts_with("0") {
            // 握手消息，忽略
        } else if text.starts_with("41") {
            *state.shutdown.lock().unwrap() = true;
        } else if text.starts_with("42") {
            let payload = &text[2..];
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
                                                    state
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
                                                    state.public_variables.write().unwrap().insert(
                                                        name.clone(),
                                                        PublicCloudVariable::new(cvid, name, value),
                                                    );
                                                }
                                                DataType::List => {
                                                    let arr = value
                                                        .as_array()
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    state.lists.write().unwrap().insert(
                                                        name.clone(),
                                                        CloudList::new(cvid, name, arr),
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            let (lock, cvar) = &state.data_ready;
                            *lock.lock().unwrap() = true;
                            cvar.notify_all();
                        }
                        ReceiveMessageType::UpdatePrivateVariable => {
                            if let Some(obj) = parsed.as_object() {
                                let cvid = obj.get("cvid").and_then(|v| v.as_str());
                                let value = obj.get("value");
                                if let (Some(cvid), Some(value)) = (cvid, value) {
                                    let mut vars = state.private_variables.write().unwrap();
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
                                            let mut vars = state.public_variables.write().unwrap();
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
                            let mut pending = state.pending_ranking_requests.lock().unwrap();
                            if pending.is_empty() {
                                return;
                            }
                            let cvid = pending.remove(0);
                            drop(pending);

                            if let Some(items) = parsed.get("items").and_then(|v| v.as_array()) {
                                let rankings: Vec<Value> = items
                                    .iter()
                                    .filter_map(|item| {
                                        let obj = item.as_object()?;
                                        Some(json!({
                                            "value": obj.get("value").cloned().unwrap_or(Value::Null),
                                            "user": {
                                                "id": obj.get("identifier")
                                                    .and_then(|v| v.as_str())
                                                    .and_then(|s| s.parse::<u64>().ok())
                                                    .unwrap_or(0),
                                                "nickname": obj.get("nickname")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or(""),
                                                "avatar_url": obj.get("avatar_url")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                            }
                                        }))
                                    })
                                    .collect();

                                let vars = state.private_variables.read().unwrap();
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
                                let mut lists_guard = state.lists.write().unwrap();
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
                                *state.online_users.write().unwrap() = total as u32;
                            }
                        }
                        ReceiveMessageType::IllegalEvent => {
                            eprintln!("[Cloud] 检测到非法事件");
                        }
                        ReceiveMessageType::Unknown(t) => {
                            eprintln!("[Cloud] 未知消息类型: {}", t);
                        }
                    }
                }
            }
        }
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

    pub async fn connect(self) -> Result<CloudConnection, CloudError> {
        let mut authenticator = CloudAuthenticator::new(self.auth_token);
        let (auth_type, stag) = self.editor.as_param();

        let url_str = format!(
            "wss://socketcv.codemao.cn:9096/cloudstorage/?session_id={}&authorization_type={}&stag={}&EIO=3&transport=websocket",
            self.work_id, auth_type, stag
        );

        let device_auth_str = authenticator
            .generate_x_device_auth()
            .await
            .map_err(|e| CloudError::Other(format!("Failed to generate device auth: {}", e)))?;

        let auth_cookie = authenticator.authorization_token();

        let rt = Runtime::new().map_err(|e| CloudError::Other(e.to_string()))?;
        let mut conn = rt.block_on(CloudConnection::new_async(
            self.work_id,
            self.editor,
            authenticator,
            &url_str,
            &device_auth_str,
            auth_cookie,
        ))?;

        // 发送 join 消息获取数据
        conn.send_message_async(SendMessageType::Join, json!({"session_id": conn.work_id}))?;
        conn.send_message_async(SendMessageType::GetAllData, Value::Null)?;

        // 等待数据就绪
        conn.wait_for_data(Duration::from_secs(30))?;

        Ok(conn)
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
