use std::collections::HashMap;
use std::net::TcpStream;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use http::HeaderValue;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tungstenite::Message;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{WebSocket, connect};

/// WebSocket 流类型别名(tungstenite + rustls)。
type WsStream = MaybeTlsStream<TcpStream>;
type Ws = WebSocket<WsStream>;

// ==================== 常量配置 ====================

/// CodeMao AI 聊天 WebSocket 服务器地址。
pub const CHAT_WS_BASE_URL: &str = "wss://cr-aichat.codemao.cn/aichat/";
/// 默认请求头(与 Python `CodeMaoConfig.HEADERS` 一致)。
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0";

/// Socket.IO 帧前缀。
const HANDSHAKE_PREFIX: &str = "0";
const CONNECTED_MESSAGE: &str = "40";
const EVENT_MESSAGE_PREFIX: &str = "42";
const PING_MESSAGE: &str = "2";
const PONG_MESSAGE: &str = "3";

/// 等待 AI 开始回复的默认超时。
pub const DEFAULT_RESPONSE_START_TIMEOUT: Duration = Duration::from_secs(10);
/// 等待回复完成的默认超时。
pub const DEFAULT_RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);
/// 连接建立等待超时。
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// ==================== 错误类型 ====================

/// AI 对话操作错误。
#[derive(Debug, Error)]
pub enum ChatError {
    #[error("WebSocket 错误: {0}")]
    WebSocket(#[from] Box<tungstenite::Error>),
    #[error("HTTP 握手失败: {0}")]
    Handshake(String),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("连接未就绪")]
    NotConnected,
    #[error("正在接收回复,请等待完成")]
    Busy,
    #[error("超时: {0}")]
    Timeout(String),
    #[error("未提供 token")]
    MissingToken,
    #[error("鉴权错误: {0}")]
    Auth(String),
    #[error("线程错误: {0}")]
    Thread(String),
}

impl From<tungstenite::Error> for ChatError {
    fn from(err: tungstenite::Error) -> Self {
        ChatError::WebSocket(Box::new(err))
    }
}

/// 本模块统一的 `Result` 别名。
pub type Result<T> = std::result::Result<T, ChatError>;

// ==================== 基础类型 ====================

/// 流式回复事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatEventType {
    /// 开始接收回复。
    Start,
    /// 增量文本。
    Text,
    /// 回复结束(载荷为完整回复)。
    End,
    /// 错误。
    Error,
}

/// 对话消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// 单条对话历史消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryMessage {
    pub role: Role,
    pub content: String,
}

impl HistoryMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
        }
    }
}

/// 用户配额信息。
#[derive(Debug, Clone, Default)]
pub struct UserInfo {
    pub user_id: Option<i64>,
    /// 剩余对话次数。
    pub chat_count: Option<i64>,
    /// 剩余图片生成次数。
    pub remaining_image_times: Option<i64>,
}

/// 回调句柄,用于取消注册回调。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackHandle(pub(crate) usize);

// 回调类型别名
type StreamCallback = Box<dyn Fn(&str, ChatEventType) + Send + Sync>;

// ==================== 事件存储 ====================

/// 泛型回调存储。
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

    fn take_all(&mut self) -> Vec<(usize, T)> {
        std::mem::take(&mut self.items)
    }
}

/// 条件变量通知器。
#[derive(Default)]
struct Notify {
    lock: Mutex<()>,
    cond: Condvar,
}

impl Notify {
    /// 持锁设置状态并通知等待者,避免 Condvar 丢失唤醒。
    fn notify_with<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.lock.lock().unwrap();
        let result = f();
        self.cond.notify_all();
        result
    }
}

// ==================== 连接核心 ====================

/// AI 对话共享内部状态。
struct ChatInner {
    token: String,
    user_agent: String,
    stopping: AtomicBool,
    connected: AtomicBool,
    /// 是否正在接收流式回复。
    receiving: AtomicBool,
    /// JOIN 消息已发送标记(防止重复 join 被服务器拒绝)。
    join_sent: AtomicBool,
    /// 当前待接收回复的回合编号(每次发送消息时递增)。
    pending_round: AtomicU64,
    /// 已收到 Begin 的最大回合编号(用于快速回复场景下避免状态竞态)。
    completed_round: AtomicU64,
    /// 发送端(读线程独占 WebSocket)。
    tx: Mutex<Option<mpsc::Sender<Message>>>,
    read_join: Mutex<Option<JoinHandle<()>>>,
    session_id: Mutex<Option<String>>,
    search_session: Mutex<Option<String>>,
    user_id: Mutex<Option<i64>>,
    user_info: Mutex<HashMap<String, Value>>,
    current_response: Mutex<String>,
    conversation_id: Mutex<String>,
    history: Mutex<Vec<HistoryMessage>>,
    callbacks: Mutex<CallbackStore<StreamCallback>>,
    notify: Notify,
}

/// AI 对话客户端句柄:线程安全,可克隆共享。
#[derive(Clone)]
pub struct ChatClient {
    inner: Arc<ChatInner>,
}

impl std::fmt::Debug for ChatClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatClient")
            .field("connected", &self.is_connected())
            .field("receiving", &self.is_receiving())
            .field("conversation_count", &self.conversation_count())
            .finish()
    }
}

/// 建造者模式:链式配置后构造 [`ChatClient`]。
#[derive(Debug, Clone)]
pub struct ChatBuilder {
    token: String,
    user_agent: String,
}

impl ChatBuilder {
    /// 以授权 token 创建建造者。
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
        }
    }

    /// 自定义 User-Agent 请求头。
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// 构建客户端(尚未连接,需调用 [`ChatClient::connect`])。
    pub fn build(self) -> ChatClient {
        let inner = ChatInner {
            token: self.token,
            user_agent: self.user_agent,
            stopping: AtomicBool::new(true),
            connected: AtomicBool::new(false),
            receiving: AtomicBool::new(false),
            join_sent: AtomicBool::new(false),
            pending_round: AtomicU64::new(0),
            completed_round: AtomicU64::new(0),
            tx: Mutex::new(None),
            read_join: Mutex::new(None),
            session_id: Mutex::new(None),
            search_session: Mutex::new(None),
            user_id: Mutex::new(None),
            user_info: Mutex::new(HashMap::new()),
            current_response: Mutex::new(String::new()),
            conversation_id: Mutex::new(generate_session_id()),
            history: Mutex::new(Vec::new()),
            callbacks: Mutex::new(CallbackStore::default()),
            notify: Notify::default(),
        };
        ChatClient {
            inner: Arc::new(inner),
        }
    }
}

impl ChatClient {
    /// 建立与 AI 服务器的 WebSocket 连接,并等待 Socket.IO 层就绪。
    pub fn connect(&self) -> Result<bool> {
        if self.inner.token.is_empty() {
            return Err(ChatError::MissingToken);
        }
        if self.inner.connected.load(Ordering::Acquire) {
            return Ok(true);
        }
        self.inner.stopping.store(false, Ordering::Release);
        establish(&self.inner)?;
        Ok(self.wait_for_connection(DEFAULT_CONNECT_TIMEOUT))
    }

    /// 等待 WebSocket 层连接建立(超时返回 false)。
    pub fn wait_for_connection(&self, timeout: Duration) -> bool {
        wait_flag(&self.inner.notify, timeout, || {
            self.inner.connected.load(Ordering::Acquire)
        })
    }

    /// 发送聊天消息(用户消息自动进入历史记录)。
    pub fn send_message(&self, message: &str, include_history: bool) -> Result<()> {
        if !self.inner.connected.load(Ordering::Acquire) {
            return Err(ChatError::NotConnected);
        }
        if self.inner.receiving.load(Ordering::Acquire) {
            return Err(ChatError::Busy);
        }
        // 追加用户消息
        let messages = {
            let mut history = self.inner.history.lock().unwrap();
            history.push(HistoryMessage::user(message));
            if include_history && history.len() > 1 {
                history.clone()
            } else {
                vec![history.last().cloned().unwrap()]
            }
        };
        let session_id = self.inner.conversation_id.lock().unwrap().clone();
        let frame = build_chat_frame(&session_id, &messages)?;
        self.send_text(&frame)?;
        // 记录本轮回合编号,供快速回复场景的等待判定
        self.inner.pending_round.fetch_add(1, Ordering::AcqRel);
        info!("聊天消息已发送: {}", truncate(message, 60));
        Ok(())
    }

    /// 等待 AI 开始回复(超时返回 false)。
    ///
    /// 通过回合编号判定:即使 AI 在调用前已完成整轮快速回复,
    /// 也不会误报超时。
    pub fn wait_for_response_start(&self, timeout: Duration) -> bool {
        let target = self.inner.pending_round.load(Ordering::Acquire);
        wait_flag(&self.inner.notify, timeout, || {
            let receiving = self.inner.receiving.load(Ordering::Acquire);
            if target == 0 {
                return receiving;
            }
            receiving || self.inner.completed_round.load(Ordering::Acquire) >= target
        })
    }

    /// 等待当前回复完成(超时返回 false)。
    pub fn wait_for_response(&self, timeout: Duration) -> bool {
        wait_flag(&self.inner.notify, timeout, || {
            !self.inner.receiving.load(Ordering::Acquire)
        })
    }

    /// 发送消息并等待回复完成,返回完整回复文本。
    pub fn send_and_wait(
        &self,
        message: &str,
        include_history: bool,
        response_timeout: Duration,
    ) -> Result<String> {
        self.send_message(message, include_history)?;
        if !self.wait_for_response_start(DEFAULT_RESPONSE_START_TIMEOUT) {
            return Err(ChatError::Timeout("AI 未开始回复".into()));
        }
        if !self.wait_for_response(response_timeout) {
            return Err(ChatError::Timeout("回复超时".into()));
        }
        Ok(self.current_response())
    }

    /// 注册流式回复回调(内容, 事件类型)。
    pub fn add_stream_callback(
        &self,
        cb: impl Fn(&str, ChatEventType) + Send + Sync + 'static,
    ) -> CallbackHandle {
        self.inner.callbacks.lock().unwrap().add(Box::new(cb))
    }

    /// 移除流式回复回调。
    pub fn remove_stream_callback(&self, handle: CallbackHandle) {
        self.inner.callbacks.lock().unwrap().remove(handle);
    }

    /// 当前已累积的回复内容。
    pub fn current_response(&self) -> String {
        self.inner.current_response.lock().unwrap().clone()
    }

    /// 获取用户配额信息。
    pub fn get_user_info(&self) -> UserInfo {
        let user_info = self.inner.user_info.lock().unwrap();
        let chat_count = user_info.get("chat_count").and_then(Value::as_i64);
        let remaining_image_times = user_info
            .get("remaining_image_times")
            .and_then(Value::as_i64);
        let user_id = *self.inner.user_id.lock().unwrap();
        UserInfo {
            user_id,
            chat_count,
            remaining_image_times,
        }
    }

    /// 创建新对话(清空历史并生成新会话 ID)。
    pub fn new_conversation(&self) {
        *self.inner.conversation_id.lock().unwrap() = generate_session_id();
        self.inner.history.lock().unwrap().clear();
        info!("新对话已创建");
    }

    /// 获取当前对话历史快照。
    pub fn conversation_history(&self) -> Vec<HistoryMessage> {
        self.inner.history.lock().unwrap().clone()
    }

    /// 获取当前对话轮数(用户消息条数)。
    pub fn conversation_count(&self) -> usize {
        self.inner
            .history
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.role == Role::User)
            .count()
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Acquire)
    }

    pub fn is_receiving(&self) -> bool {
        self.inner.receiving.load(Ordering::Acquire)
    }

    /// 关闭连接。
    pub fn close(&self) {
        let inner = &self.inner;
        inner.stopping.store(true, Ordering::Release);
        if let Some(tx) = inner.tx.lock().unwrap().clone() {
            let _ = tx.send(Message::Close(None));
        }
        drop(inner.tx.lock().unwrap().take());
        if let Some(handle) = inner.read_join.lock().unwrap().take() {
            let _ = handle.join();
        }
        inner.notify.notify_with(|| {
            inner.connected.store(false, Ordering::Release);
            inner.receiving.store(false, Ordering::Release);
        });
        info!("AI 对话连接已关闭");
    }

    // ========== 内部发送 ==========

    pub(crate) fn send_event(&self, name: &str, payload: &Value) -> Result<()> {
        let frame = format!(
            "{EVENT_MESSAGE_PREFIX} {}",
            serde_json::to_string(&(name, payload))?
        );
        self.send_text(&frame)
    }

    pub(crate) fn send_text(&self, payload: &str) -> Result<()> {
        let tx = self
            .inner
            .tx
            .lock()
            .unwrap()
            .clone()
            .ok_or(ChatError::NotConnected)?;
        tx.send(Message::text(payload))
            .map_err(|_| ChatError::NotConnected)
    }
}

// ==================== 帧构造与解析 ====================

/// 构建 `chat` 事件帧:`42["chat",{...}]`(可测)。
pub fn build_chat_frame(session_id: &str, messages: &[HistoryMessage]) -> Result<String> {
    let payload = json!({
        "session_id": session_id,
        "messages": messages,
        "chat_type": "chat_v3",
        "msg_channel": 0,
    });
    let frame = format!(
        "{EVENT_MESSAGE_PREFIX} {}",
        serde_json::to_string(&("chat", payload))?
    );
    Ok(frame)
}

/// 解析后的 Socket.IO 帧。
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Handshake(Value),
    Connected,
    Ping,
    Pong,
    /// 事件帧:事件名 + 载荷。
    Event(String, Value),
    Unknown(String),
}

/// 纯函数:解析 Socket.IO 文本帧(可测)。
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
    if let Some(rest) = text.strip_prefix(EVENT_MESSAGE_PREFIX)
        && let Ok(Value::Array(items)) = serde_json::from_str::<Value>(rest)
        && let Some(Value::String(name)) = items.first()
    {
        let payload = items.get(1).cloned().unwrap_or(Value::Null);
        return Frame::Event(name.clone(), payload);
    }
    Frame::Unknown(text.to_string())
}

/// 流式回复事件(由 `chat_ack` 载荷解析)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Begin,
    Chunk(String),
    End(String),
}

/// 纯函数:解析 `chat_ack` 载荷为流式事件(可测)。
pub fn parse_chat_ack(payload: &Value) -> Option<StreamEvent> {
    if payload.get("code").and_then(Value::as_i64) != Some(1) {
        return None;
    }
    let data = payload.get("data")?;
    let content_type = data.get("content_type")?.as_str()?;
    match content_type {
        "stream_output_begin" => Some(StreamEvent::Begin),
        "stream_output_content" => Some(StreamEvent::Chunk(
            data.get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )),
        "stream_output_end" => Some(StreamEvent::End(
            data.get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        )),
        _ => None,
    }
}

/// 生成 8 位会话/客户端 ID(与 Python `_generate_session_id` 一致)。
fn generate_session_id() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..8)
        .map(|_| CHARSET[fastrand::usize(0..CHARSET.len())] as char)
        .collect()
}

// ==================== 事件处理策略 ====================

/// 事件处理策略接口:每种事件一个处理器。
trait ChatEventHandler: Send + Sync {
    fn handle(&self, inner: &Arc<ChatInner>, payload: &Value);
}

/// `on_connect_ack`:记录连接确认信息(剩余对话次数),并发送 JOIN。
///
/// JOIN 在收到连接确认后发送(服务器就绪),与 Python 的时序一致。
struct ConnectAckHandler;

impl ChatEventHandler for ConnectAckHandler {
    fn handle(&self, inner: &Arc<ChatInner>, payload: &Value) {
        if payload.get("code").and_then(Value::as_i64) != Some(1) {
            return;
        }
        if let Some(data) = payload.get("data").and_then(Value::as_object) {
            inner.user_info.lock().unwrap().extend(data.clone());
            let chat_count = data
                .get("chat_count")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "未知".into());
            info!("连接确认 - 剩余对话次数: {chat_count}");
        }
        // 服务器可能重复确认,只发送一次 JOIN(帧格式与 Python 的 `42 ["join"]` 一致)
        if !inner.join_sent.swap(true, Ordering::AcqRel) {
            let _ = send_raw(inner, "42 [\"join\"]");
        }
    }
}

/// `join_ack`:记录用户信息并发送预设消息。
struct JoinAckHandler;

impl ChatEventHandler for JoinAckHandler {
    fn handle(&self, inner: &Arc<ChatInner>, payload: &Value) {
        if payload.get("code").and_then(Value::as_i64) != Some(1) {
            return;
        }
        let data = payload.get("data");
        if let Some(data) = data {
            // 服务器将 user_id 以字符串形式返回(如 "1742185446")
            let user_id = data.get("user_id").and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            });
            if let Some(user_id) = user_id {
                *inner.user_id.lock().unwrap() = Some(user_id);
            }
            if let Some(session) = data.get("search_session").and_then(Value::as_str) {
                *inner.search_session.lock().unwrap() = Some(session.to_string());
            }
        }
        info!(
            "加入成功 - 用户 ID: {:?}, 会话: {:?}",
            *inner.user_id.lock().unwrap(),
            *inner.search_session.lock().unwrap()
        );
        // 发送预设消息
        let _ = send_event_on(
            inner,
            "preset_chat_message",
            &json!({
                "turn_count": 5,
                "system_content_enum": "default",
            }),
        );
        let _ = send_event_on(inner, "get_text2Img_remaining_times", &Value::Null);
    }
}

/// `preset_chat_message_ack`:预设消息确认。
struct PresetAckHandler;

impl ChatEventHandler for PresetAckHandler {
    fn handle(&self, _inner: &Arc<ChatInner>, _payload: &Value) {
        debug!("预设消息确认");
    }
}

/// `get_text2Img_remaining_times_ack`:剩余图片生成次数。
struct RemainingTimesHandler;

impl ChatEventHandler for RemainingTimesHandler {
    fn handle(&self, inner: &Arc<ChatInner>, payload: &Value) {
        if payload.get("code").and_then(Value::as_i64) != Some(1) {
            return;
        }
        if let Some(remaining) = payload.get("data").and_then(|d| d.get("remaining_times")) {
            inner
                .user_info
                .lock()
                .unwrap()
                .insert("remaining_image_times".into(), remaining.clone());
            info!("剩余图片生成次数: {}", remaining);
        }
    }
}

/// `chat_ack`:处理流式回复。
struct ChatAckHandler;

impl ChatEventHandler for ChatAckHandler {
    fn handle(&self, inner: &Arc<ChatInner>, payload: &Value) {
        let Some(event) = parse_chat_ack(payload) else {
            return;
        };
        match event {
            StreamEvent::Begin => {
                if let Some(session_id) = payload
                    .get("data")
                    .and_then(|d| d.get("session_id"))
                    .and_then(Value::as_str)
                {
                    *inner.session_id.lock().unwrap() = Some(session_id.to_string());
                }
                *inner.current_response.lock().unwrap() = String::new();
                inner.notify.notify_with(|| {
                    inner.receiving.store(true, Ordering::Release);
                    inner.completed_round.store(
                        inner.pending_round.load(Ordering::Acquire),
                        Ordering::Release,
                    );
                });
                emit_stream(inner, "", ChatEventType::Start);
            }
            StreamEvent::Chunk(content) => {
                if inner.receiving.load(Ordering::Acquire) {
                    inner.current_response.lock().unwrap().push_str(&content);
                    emit_stream(inner, &content, ChatEventType::Text);
                }
            }
            StreamEvent::End(_) => {
                inner.notify.notify_with(|| {
                    inner.receiving.store(false, Ordering::Release);
                });
                // 先落历史,再发 End 事件:End 回调中可读到完整对话
                let full = inner.current_response.lock().unwrap().clone();
                if !full.is_empty() {
                    inner
                        .history
                        .lock()
                        .unwrap()
                        .push(HistoryMessage::assistant(full.clone()));
                }
                emit_stream(inner, &full, ChatEventType::End);
            }
        }
    }
}

/// 事件分派(策略注册表)。
fn dispatch_event(inner: &Arc<ChatInner>, name: &str, payload: &Value) {
    debug!(
        "收到 AI 事件: {name}, 载荷: {}",
        truncate(&payload.to_string(), 200)
    );
    match name {
        "on_connect_ack" => ConnectAckHandler.handle(inner, payload),
        "join_ack" => JoinAckHandler.handle(inner, payload),
        "preset_chat_message_ack" => PresetAckHandler.handle(inner, payload),
        "get_text2Img_remaining_times_ack" => RemainingTimesHandler.handle(inner, payload),
        "chat_ack" => ChatAckHandler.handle(inner, payload),
        other => {
            debug!("未知事件: {other}");
        }
    }
}

/// 触发流式回调。
fn emit_stream(inner: &Arc<ChatInner>, content: &str, event: ChatEventType) {
    let callbacks = {
        let mut store = inner.callbacks.lock().unwrap();
        store.take_all()
    };
    for (_, cb) in &callbacks {
        if let Err(e) = catch_unwind(AssertUnwindSafe(|| cb(content, event))) {
            warn!("流式回调 panic: {e:?}");
        }
    }
    inner.callbacks.lock().unwrap().items.extend(callbacks);
}

/// 在 `ChatInner` 上发送事件(供读线程内使用)。
fn send_event_on(inner: &Arc<ChatInner>, name: &str, payload: &Value) -> Result<()> {
    let frame = format!(
        "{EVENT_MESSAGE_PREFIX} {}",
        serde_json::to_string(&(name, payload))?
    );
    let tx = inner
        .tx
        .lock()
        .unwrap()
        .clone()
        .ok_or(ChatError::NotConnected)?;
    tx.send(Message::text(frame))
        .map_err(|_| ChatError::NotConnected)
}

// ==================== 帧处理 ====================

fn handle_frame(inner: &Arc<ChatInner>, text: &str) -> Result<()> {
    match parse_frame(text) {
        Frame::Ping => send_raw(inner, PONG_MESSAGE),
        Frame::Pong => Ok(()),
        Frame::Handshake(_) => {
            info!("握手成功,发送连接请求");
            send_raw(inner, CONNECTED_MESSAGE)
        }
        Frame::Connected => {
            info!("Socket.IO 连接成功");
            Ok(())
        }
        Frame::Event(name, payload) => {
            dispatch_event(inner, &name, &payload);
            Ok(())
        }
        Frame::Unknown(text) => {
            debug!("收到未知消息: {}", truncate(&text, 80));
            Ok(())
        }
    }
}

fn send_raw(inner: &Arc<ChatInner>, payload: &str) -> Result<()> {
    let tx = inner
        .tx
        .lock()
        .unwrap()
        .clone()
        .ok_or(ChatError::NotConnected)?;
    tx.send(Message::text(payload))
        .map_err(|_| ChatError::NotConnected)
}

// ==================== 连接建立与读循环 ====================

/// 建立连接并启动读线程。
fn establish(inner: &Arc<ChatInner>) -> Result<()> {
    // 与 Python build_websocket_url 一致(token 经 URL 编码)
    let mut url =
        url::Url::parse(CHAT_WS_BASE_URL).map_err(|e| ChatError::Handshake(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("stag", "6")
        .append_pair("rf", "")
        .append_pair("source_label", "kn")
        .append_pair("question_type", "undefined")
        .append_pair("EIO", "3")
        .append_pair("transport", "websocket")
        .append_pair("token", &inner.token);

    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| ChatError::Handshake(e.to_string()))?;
    request.headers_mut().insert(
        "User-Agent",
        HeaderValue::from_str(&inner.user_agent).map_err(|e| ChatError::Auth(e.to_string()))?,
    );
    request
        .headers_mut()
        .insert("Origin", HeaderValue::from_static("https://kn.codemao.cn"));

    let (mut ws, response) = connect(request)?;
    // WebSocket 升级成功返回 HTTP 101 Switching Protocols
    if response.status() != http::StatusCode::SWITCHING_PROTOCOLS {
        return Err(ChatError::Handshake(format!(
            "HTTP 状态: {}",
            response.status()
        )));
    }
    // 设置底层流读取超时:read 周期性苏醒,避免服务器静默时发送通道饥饿
    let _ = set_stream_read_timeout(ws.get_mut(), Duration::from_millis(200));
    info!("AI 对话 WebSocket 已建立");

    let (tx, rx) = mpsc::channel::<Message>();
    *inner.tx.lock().unwrap() = Some(tx);
    inner.join_sent.store(false, Ordering::Release);
    inner.notify.notify_with(|| {
        inner.connected.store(true, Ordering::Release);
    });

    let inner_loop = inner.clone();
    let handle = thread::Builder::new()
        .name("chat-ws-read".into())
        .spawn(move || read_loop(inner_loop, ws, rx))
        .map_err(|e| ChatError::Thread(e.to_string()))?;
    *inner.read_join.lock().unwrap() = Some(handle);
    Ok(())
}

/// 读线程:独占 WebSocket,转发写消息,解析帧并分发。
fn read_loop(inner: Arc<ChatInner>, mut ws: Ws, rx: mpsc::Receiver<Message>) {
    'outer: loop {
        if inner.stopping.load(Ordering::Acquire) {
            break;
        }
        // 优先转发待发送消息(最多阻塞 100ms;连续发送上限避免写洪水饿死入站读取)
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
            // 读取超时:回到循环顶部处理待发送消息
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(e) => {
                info!("AI 对话读取结束: {e}");
                break;
            }
        }
    }
    drop(rx);
    let was_connected = inner.connected.load(Ordering::Acquire);
    inner.notify.notify_with(|| {
        inner.connected.store(false, Ordering::Release);
        inner.receiving.store(false, Ordering::Release);
    });
    *inner.tx.lock().unwrap() = None;
    if was_connected {
        info!("AI 对话连接已断开");
        emit_stream(&inner, "连接已断开", ChatEventType::Error);
    }
}

/// 设置 WebSocket 底层流的读取超时(Plain 或 rustls 两种形态)。
fn set_stream_read_timeout(stream: &mut WsStream, timeout: Duration) -> std::io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(owned) => owned.sock.set_read_timeout(Some(timeout)),
        _ => Ok(()),
    }
}

// ==================== 工具 ====================

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
