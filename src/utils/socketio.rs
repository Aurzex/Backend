//! Socket.IO over WebSocket 共享基础设施
//!
//! 由 `core::cloudvar`(云变量)与 `core::converse`(AI 对话)两个 WS 客户端共用:
//! 流类型别名、Socket.IO 帧解析、回调存储、条件变量等待、日志截断与读超时设置。

use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::WebSocket;
use tungstenite::stream::MaybeTlsStream;

/// WebSocket 流类型别名(tungstenite + rustls)
pub(crate) type WsStream = MaybeTlsStream<TcpStream>;
pub(crate) type Ws = WebSocket<WsStream>;

/// Socket.IO 帧前缀
pub(crate) const HANDSHAKE_PREFIX: &str = "0";
pub(crate) const CONNECTED_MESSAGE: &str = "40";
pub(crate) const SERVER_CLOSE_PREFIX: &str = "41";
pub(crate) const EVENT_MESSAGE_PREFIX: &str = "42";
pub(crate) const PING_MESSAGE: &str = "2";
pub(crate) const PONG_MESSAGE: &str = "3";

/// 回调句柄,用于取消注册回调
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallbackHandle(pub(crate) usize);

/// 解析后的 Socket.IO 帧
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Frame {
    Handshake(Value),
    Connected,
    Ping,
    Pong,
    ServerClose,
    /// 事件帧:事件名 + 载荷
    Event(String, Value),
    /// 未知/无法解析的文本
    Unknown(String),
}

/// 纯函数:解析 Socket.IO 文本帧(可测)
pub(crate) fn parse_frame(text: &str) -> Frame {
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
    {
        let mut items = items.into_iter();
        if let Some(Value::String(name)) = items.next() {
            let payload = items.next().unwrap_or(Value::Null);
            return Frame::Event(name, payload);
        }
    }
    Frame::Unknown(text.to_string())
}

/// 设置 WebSocket 底层流的读取超时(Plain 或 rustls 两种形态)
pub(crate) fn set_stream_read_timeout(
    stream: &mut WsStream,
    timeout: Duration,
) -> std::io::Result<()> {
    match stream {
        MaybeTlsStream::Plain(s) => s.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(owned) => owned.sock.set_read_timeout(Some(timeout)),
        _ => Ok(()),
    }
}

/// 进程级回调句柄分配器。
///
/// 句柄必须跨 store 唯一:`CloudConnection::remove_callback` 按句柄清理四类事件 store,
/// 若各 store 各自从 0 编号,删除一个回调会误删其它 store 中同号的回调。
static NEXT_CALLBACK_ID: AtomicUsize = AtomicUsize::new(0);

fn next_callback_id() -> usize {
    NEXT_CALLBACK_ID.fetch_add(1, Ordering::Relaxed)
}

/// 泛型回调存储:支持按句柄增删。
///
/// 分发时用 [`Self::snapshot`] 克隆一份回调引用,而不是把 store 清空后再放回;
/// 这样回调执行期间调用 `remove_*` 会立即从 store 删除,不会被放回复活。
/// 当前所有回调类型都存成 `Arc<dyn Fn ...>`(`T: Clone`)。
pub(crate) struct CallbackStore<T> {
    pub(crate) items: Vec<(usize, T)>,
}

impl<T> std::fmt::Debug for CallbackStore<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackStore")
            .field("count", &self.items.len())
            .finish()
    }
}

impl<T> Default for CallbackStore<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> CallbackStore<T> {
    pub(crate) fn add(&mut self, cb: T) -> CallbackHandle {
        let id = next_callback_id();
        self.items.push((id, cb));
        CallbackHandle(id)
    }

    pub(crate) fn remove(&mut self, handle: CallbackHandle) {
        self.items.retain(|(id, _)| *id != handle.0);
    }

    /// 锁内快照;调用方在锁外执行回调。回调执行期间的新增/移除以 store 当前状态为准。
    pub(crate) fn snapshot(&self) -> Vec<(usize, T)>
    where
        T: Clone,
    {
        self.items.clone()
    }
}

/// 条件变量通知器,供 `wait_for_*` 使用
#[derive(Default)]
pub(crate) struct Notify {
    lock: Mutex<()>,
    cond: Condvar,
}

impl Notify {
    /// 持锁设置状态并通知等待者,避免 Condvar 丢失唤醒(与 [`wait_flag`] 的
    /// 检查-等待原子性配合,消除状态型等待的竞态窗口)
    pub(crate) fn notify_with<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.lock.lock().unwrap();
        let result = f();
        self.cond.notify_all();
        result
    }
}

/// 基于条件变量的超时等待
pub(crate) fn wait_flag(notify: &Notify, timeout: Duration, flag: impl Fn() -> bool) -> bool {
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

/// 日志截断
pub(crate) fn truncate(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    const SUFFIX: &str = "...";
    let half = max.saturating_sub(SUFFIX.len()) / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(count - half).collect();
    format!("{head}{SUFFIX}{tail}")
}

/// Socket.IO 客户端(云变量 / AI 对话)共用的错误类型
#[derive(Debug, thiserror::Error)]
pub enum SocketError {
    #[error("WebSocket 错误: {0}")]
    WebSocket(#[from] Box<tungstenite::Error>),
    #[error("HTTP 握手失败: {0}")]
    Handshake(String),
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("发送失败: {0}")]
    Send(#[from] std::sync::mpsc::SendError<tungstenite::Message>),
    #[error("连接未就绪")]
    NotConnected,
    #[error("正在接收回复,请等待完成")]
    Busy,
    #[error("超时: {0}")]
    Timeout(String),
    #[error("未提供 token")]
    MissingToken,
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

impl From<tungstenite::Error> for SocketError {
    fn from(err: tungstenite::Error) -> Self {
        SocketError::WebSocket(Box::new(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    type TestCallback = Arc<dyn Fn() + Send + Sync>;

    fn noop() -> TestCallback {
        Arc::new(|| {})
    }

    /// 句柄跨 store 唯一:`CloudConnection::remove_callback` 按句柄清理四类事件 store,
    /// 各 store 若各自从 0 编号会误删同号回调。
    #[test]
    fn callback_ids_are_unique_across_stores() {
        let mut a: CallbackStore<TestCallback> = CallbackStore::default();
        let mut b: CallbackStore<TestCallback> = CallbackStore::default();
        let ha = a.add(noop());
        let hb = b.add(noop());
        assert_ne!(ha, hb);
    }

    /// 分发快照不清空 store;执行期间移除后,后续快照不再包含该回调。
    #[test]
    fn snapshot_keeps_store_and_remove_during_dispatch_sticks() {
        let mut store: CallbackStore<TestCallback> = CallbackStore::default();
        let handle = store.add(noop());
        let in_flight = store.snapshot();
        assert_eq!(in_flight.len(), 1);
        assert_eq!(store.snapshot().len(), 1, "快照不能清空 store");
        store.remove(handle);
        assert!(store.snapshot().is_empty(), "移除后不能被快照复活");
    }
}
