use futures_util::{SinkExt, StreamExt};
use http::header::HeaderName;
use http::header::HeaderValue;
use rand::Rng;
use rand::RngExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::{Notify, mpsc};
use url::Url;
use wreq::Client;
use wreq_util::Emulation;

// ==================== 错误类型 ====================
#[derive(Debug)]
pub enum ChatError {
    WebSocket(String),
    Json(serde_json::Error),
    Url(url::ParseError),
    Connection(String),
    Timeout(String),
    SendError(String),
    Reqwest(wreq::Error),
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatError::WebSocket(e) => write!(f, "WebSocket error: {}", e),
            ChatError::Json(e) => write!(f, "JSON error: {}", e),
            ChatError::Url(e) => write!(f, "URL error: {}", e),
            ChatError::Connection(e) => write!(f, "Connection error: {}", e),
            ChatError::Timeout(e) => write!(f, "Timeout error: {}", e),
            ChatError::SendError(e) => write!(f, "Send error: {}", e),
            ChatError::Reqwest(e) => write!(f, "HTTP client error: {}", e),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<serde_json::Error> for ChatError {
    fn from(e: serde_json::Error) -> Self {
        ChatError::Json(e)
    }
}
impl From<url::ParseError> for ChatError {
    fn from(e: url::ParseError) -> Self {
        ChatError::Url(e)
    }
}
impl From<mpsc::error::SendError<String>> for ChatError {
    fn from(e: mpsc::error::SendError<String>) -> Self {
        ChatError::SendError(e.to_string())
    }
}
impl From<wreq::Error> for ChatError {
    fn from(e: wreq::Error) -> Self {
        ChatError::Reqwest(e)
    }
}

// ==================== 流事件类型 ====================
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEventType {
    Start,
    Text,
    End,
    Error,
}

// ==================== 配置 ====================
#[derive(Debug, Clone)]
pub struct ChatConfig {
    ws_base_url: String,
    connect_timeout: u64,
    response_start_timeout: u64,
    response_timeout: u64,
    verbose: bool,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            ws_base_url: "wss://cr-aichat.codemao.cn/aichat/".to_string(),
            connect_timeout: 10,
            response_start_timeout: 10,
            response_timeout: 60,
            verbose: false,
        }
    }
}

impl ChatConfig {
    fn build_ws_url(&self, token: &str) -> Result<Url, ChatError> {
        let mut params = HashMap::new();
        params.insert("stag", "6");
        params.insert("rf", "");
        params.insert("source_label", "kn");
        params.insert("question_type", "undefined");
        params.insert("EIO", "3");
        params.insert("transport", "websocket");
        params.insert("token", token);

        let query: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!("{}?{}", self.ws_base_url, query);
        Url::parse(&url).map_err(ChatError::Url)
    }
}

fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char)
            }
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

// ==================== 回调类型 ====================
pub type StreamCallback = Box<dyn FnMut(&str, StreamEventType) + Send>;

// ==================== 内部状态 ====================
#[derive(Default)]
struct ClientState {
    is_receiving_response: bool,
    current_response: String,
    response_complete: bool,
    user_info: HashMap<String, Value>,
    conversation_history: Vec<HashMap<String, String>>,
    conversation_id: String,
    user_id: Option<u64>,
    search_session: Option<String>,
    connected: bool,
    joined: bool,
}

// ==================== WebSocket 异步句柄 ====================
struct WsHandle {
    sender: mpsc::Sender<String>,
    handle: tokio::task::JoinHandle<()>,
}

impl WsHandle {
    async fn send(&self, msg: String) -> Result<(), ChatError> {
        self.sender.send(msg).await?;
        Ok(())
    }

    fn shutdown(self) {
        // 向通道发送关闭信号，然后等待任务结束
        // 注意：这里需要异步上下文，实际在 close() 时通过 runtime block_on 处理
    }
}

// ==================== 主客户端 ====================
pub struct CodeMaoChatClient {
    config: ChatConfig,
    token: String,
    ws_handle: Option<WsHandle>,
    state: Arc<(std::sync::Mutex<ClientState>, Notify)>,
    global_callbacks: Arc<std::sync::Mutex<Vec<StreamCallback>>>,
    runtime: Runtime,
}

impl CodeMaoChatClient {
    pub fn builder() -> ChatClientBuilder {
        ChatClientBuilder::new()
    }

    /// 同步连接接口，保持原有用法不变
    pub fn connect(&mut self) -> Result<(), ChatError> {
        self.runtime.block_on(self.async_connect())
    }

    async fn async_connect(&mut self) -> Result<(), ChatError> {
        let url = self.config.build_ws_url(&self.token)?;
        let state = Arc::clone(&self.state);
        let global_callbacks = Arc::clone(&self.global_callbacks);
        let verbose = self.config.verbose;

        let (tx, mut rx) = mpsc::channel::<String>(256);
        let reader_tx = tx.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = run_ws_loop(url, rx, reader_tx, state, global_callbacks, verbose).await
            {
                if verbose {
                    eprintln!("WebSocket loop error: {}", e);
                }
            }
        });

        self.ws_handle = Some(WsHandle { sender: tx, handle });

        // 等待连接建立和加入房间完成
        let timeout = Duration::from_secs(self.config.connect_timeout);
        let notify = &self.state.1;
        let start = tokio::time::Instant::now();

        // 使用 Notify 异步等待
        loop {
            {
                let guard = self.state.0.lock().unwrap();
                if guard.connected && guard.joined {
                    break;
                }
            }
            if start.elapsed() >= timeout {
                let guard = self.state.0.lock().unwrap();
                if !guard.connected {
                    return Err(ChatError::Timeout("连接超时".to_string()));
                }
                if !guard.joined {
                    return Err(ChatError::Timeout("加入房间超时".to_string()));
                }
            }
            // 等待通知，带超时
            tokio::select! {
                _ = notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
        }

        if self.config.verbose {
            println!("连接成功");
        }
        Ok(())
    }

    pub fn send_message(&self, content: &str) -> MessageRequestBuilder<'_> {
        MessageRequestBuilder::new(self, content)
    }

    async fn ws_send_async(&self, msg: String) -> Result<(), ChatError> {
        if let Some(ref ws) = self.ws_handle {
            ws.send(msg).await?;
            Ok(())
        } else {
            Err(ChatError::Connection("未连接".to_string()))
        }
    }

    // 内部同步包装（供同步上下文使用）
    fn ws_send(&self, msg: String) -> Result<(), ChatError> {
        self.runtime.block_on(self.ws_send_async(msg))
    }

    pub fn get_user_info(&self) -> HashMap<String, Value> {
        self.state.0.lock().unwrap().user_info.clone()
    }

    pub fn get_conversation_history(&self) -> Vec<HashMap<String, String>> {
        self.state.0.lock().unwrap().conversation_history.clone()
    }

    pub fn new_conversation(&self) {
        let mut state = self.state.0.lock().unwrap();
        state.conversation_history.clear();
        state.conversation_id = generate_session_id(8);
        if self.config.verbose {
            println!("新对话已创建");
        }
    }

    pub fn close(&mut self) {
        if let Some(ws) = self.ws_handle.take() {
            // 通知写循环退出
            let _ = self
                .runtime
                .block_on(async { ws.sender.send("__CLOSE__".to_string()).await });
            // 等待任务结束
            let _ = self.runtime.block_on(ws.handle);
        }
    }

    /// 便捷同步接口
    pub fn quick_chat(token: &str, message: &str) -> Result<String, ChatError> {
        let mut client = CodeMaoChatClient::builder().token(token).build()?;
        client.connect()?;
        // 短暂停确保初始化完成
        std::thread::sleep(Duration::from_secs(1));
        let response = client.send_message(message).send_and_wait()?;
        client.close();
        Ok(response)
    }
}

impl Drop for CodeMaoChatClient {
    fn drop(&mut self) {
        self.close();
    }
}

// ==================== 消息请求构建器 ====================
pub struct MessageRequestBuilder<'a> {
    client: &'a CodeMaoChatClient,
    content: String,
    include_history: bool,
    timeout: u64,
    callbacks: Vec<StreamCallback>,
}

impl<'a> MessageRequestBuilder<'a> {
    fn new(client: &'a CodeMaoChatClient, content: &str) -> Self {
        Self {
            client,
            content: content.to_string(),
            include_history: true,
            timeout: client.config.response_timeout,
            callbacks: Vec::new(),
        }
    }

    pub fn include_history(mut self, include: bool) -> Self {
        self.include_history = include;
        self
    }

    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout = secs;
        self
    }

    pub fn on_event<F>(mut self, f: F) -> Self
    where
        F: FnMut(&str, StreamEventType) + Send + 'static,
    {
        self.callbacks.push(Box::new(f));
        self
    }

    pub fn on_text<F>(self, mut f: F) -> Self
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.on_event(move |text, event| {
            if event == StreamEventType::Text {
                f(text)
            }
        })
    }

    pub fn on_end<F>(self, f: F) -> Self
    where
        F: FnOnce(String) + Send + 'static,
    {
        let mut f = Some(f);
        self.on_event(move |_text, event| {
            if event == StreamEventType::End {
                if let Some(cb) = f.take() {
                    cb(_text.to_string());
                }
            }
        })
    }

    /// 发送并阻塞等待完整回复
    pub fn send_and_wait(self) -> Result<String, ChatError> {
        self.client.runtime.block_on(self.send_and_wait_async())
    }

    async fn send_and_wait_async(self) -> Result<String, ChatError> {
        self.send_internal_async().await?;

        // 注册临时回调
        let callbacks = self.callbacks;
        let original_len = if !callbacks.is_empty() {
            let mut guard = self.client.global_callbacks.lock().unwrap();
            let len = guard.len();
            guard.extend(callbacks);
            Some(len)
        } else {
            None
        };

        let timeout = Duration::from_secs(self.timeout);
        let state_tuple = &*self.client.state; // 类型是 &(Mutex<ClientState>, Notify)
        let lock = &state_tuple.0;
        let notify = &state_tuple.1;
        let start = tokio::time::Instant::now();

        // 异步等待回复开始
        loop {
            let mut state = lock.lock().unwrap();
            if state.is_receiving_response || state.response_complete {
                break;
            }
            drop(state);
            if start.elapsed() >= timeout {
                return Err(ChatError::Timeout("等待回复开始超时".to_string()));
            }
            tokio::select! {
                _ = notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
        }

        // 等待回复完成
        loop {
            let mut state = lock.lock().unwrap();
            if state.response_complete {
                let result = state.current_response.clone();
                // 清理回调
                if let Some(len) = original_len {
                    drop(state);
                    let mut guard = self.client.global_callbacks.lock().unwrap();
                    guard.truncate(len);
                }
                return Ok(result);
            }
            drop(state);
            if start.elapsed() >= timeout {
                return Err(ChatError::Timeout("等待回复完成超时".to_string()));
            }
            tokio::select! {
                _ = notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_millis(100)) => {},
            }
        }
    }

    pub fn send(self) -> Result<(), ChatError> {
        self.client.runtime.block_on(self.send_internal_async())
    }

    async fn send_internal_async(&self) -> Result<(), ChatError> {
        {
            let state = self.client.state.0.lock().unwrap();
            if !state.joined {
                return Err(ChatError::Connection(
                    "尚未加入房间，请稍后重试".to_string(),
                ));
            }
            if state.is_receiving_response {
                return Err(ChatError::Connection("正在接收回复，请等待".to_string()));
            }
        }

        let state = self.client.state.0.lock().unwrap();
        let mut messages = if self.include_history {
            state.conversation_history.clone()
        } else {
            vec![]
        };
        drop(state);

        let mut user_msg = HashMap::new();
        user_msg.insert("role".to_string(), "user".to_string());
        user_msg.insert("content".to_string(), self.content.clone());
        messages.push(user_msg);

        {
            let mut state = self.client.state.0.lock().unwrap();
            state.conversation_history = messages.clone();
            state.is_receiving_response = false;
            state.response_complete = false;
            state.current_response.clear();
        }

        let conversation_id = self.client.state.0.lock().unwrap().conversation_id.clone();

        let chat_data = json!({
            "session_id": conversation_id,
            "messages": messages,
            "chat_type": "chat_v3",
            "msg_channel": 0
        });

        let message_str = format!(r#"42 ["chat",{}]"#, serde_json::to_string(&chat_data)?);
        self.client.ws_send_async(message_str).await?;

        if self.client.config.verbose {
            println!("消息已发送: {}", self.content);
        }
        Ok(())
    }
}

// ==================== Builder ====================
pub struct ChatClientBuilder {
    token: Option<String>,
    config: ChatConfig,
    callbacks: Vec<StreamCallback>,
}

impl ChatClientBuilder {
    fn new() -> Self {
        Self {
            token: None,
            config: ChatConfig::default(),
            callbacks: Vec::new(),
        }
    }

    pub fn token(mut self, token: &str) -> Self {
        self.token = Some(token.to_string());
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    pub fn connect_timeout(mut self, secs: u64) -> Self {
        self.config.connect_timeout = secs;
        self
    }

    pub fn response_start_timeout(mut self, secs: u64) -> Self {
        self.config.response_start_timeout = secs;
        self
    }

    pub fn response_timeout(mut self, secs: u64) -> Self {
        self.config.response_timeout = secs;
        self
    }

    pub fn on_event<F>(mut self, f: F) -> Self
    where
        F: FnMut(&str, StreamEventType) + Send + 'static,
    {
        self.callbacks.push(Box::new(f));
        self
    }

    pub fn on_text<F>(self, mut f: F) -> Self
    where
        F: FnMut(&str) + Send + 'static,
    {
        self.on_event(move |text, event| {
            if event == StreamEventType::Text {
                f(text)
            }
        })
    }

    pub fn build(self) -> Result<CodeMaoChatClient, ChatError> {
        let token = self
            .token
            .ok_or_else(|| ChatError::Connection("Token未设置".to_string()))?;
        let state = Arc::new((
            std::sync::Mutex::new(ClientState {
                conversation_id: generate_session_id(8),
                ..Default::default()
            }),
            Notify::new(),
        ));
        let runtime = Runtime::new()
            .map_err(|e| ChatError::Connection(format!("创建异步运行时失败: {}", e)))?;
        Ok(CodeMaoChatClient {
            config: self.config,
            token,
            ws_handle: None,
            state,
            global_callbacks: Arc::new(std::sync::Mutex::new(self.callbacks)),
            runtime,
        })
    }
}

// ==================== WebSocket 事件循环 (wreq 异步) ====================
async fn run_ws_loop(
    url: Url,
    mut rx: mpsc::Receiver<String>,
    reader_tx: mpsc::Sender<String>,
    state: Arc<(std::sync::Mutex<ClientState>, Notify)>,
    callbacks: Arc<std::sync::Mutex<Vec<StreamCallback>>>,
    verbose: bool,
) -> Result<(), ChatError> {
    // ★ 核心：使用 wreq 创建 Client，并启用浏览器指纹模拟
    let client = Client::builder()
        .emulation(Emulation::Chrome137) // 模拟 Chrome 140 的 TLS/JA3/HTTP2 指纹
        .build()?;

    let ws_response = client
        .websocket(url.as_str())
        .header("origin", "https://kn.codemao.cn")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0",
        )
        .header("accept-encoding", "gzip, deflate, br, zstd")
        .header("accept-language", "zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6")
        .header("cache-control", "no-cache")
        .header("pragma", "no-cache")
        .send()
        .await?;

    let (mut ws_sink, mut ws_stream) = ws_response.into_websocket().await?.split();

    // 标记连接已建立
    {
        let mut st = state.0.lock().unwrap();
        st.connected = true;
        state.1.notify_one();
    }

    // 发送 engine.io open 包 "40"
    ws_sink
        .send(wreq::Message::text("40"))
        .await
        .map_err(|e| ChatError::WebSocket(e.to_string()))?;

    // 延迟 1 秒发送 join
    let sender_for_join = reader_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let _ = sender_for_join.send(r#"42 ["join"]"#.to_string()).await;
    });

    // 写任务：从 rx 接收消息并发送到 WebSocket
    let write_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if msg == "__CLOSE__" {
                break;
            }
            if verbose && msg != "2" && msg != "3" && !msg.starts_with("40") {
                println!("[SEND] {}", msg);
            }
            if let Err(e) = ws_sink.send(wreq::Message::text(msg)).await {
                if verbose {
                    eprintln!("发送失败: {}", e);
                }
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // 读循环
    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(wreq::Message::Text(text)) => {
                let keep_going =
                    handle_message(&text, &state, &callbacks, verbose, &reader_tx).await;
                if !keep_going {
                    let _ = reader_tx.send("__CLOSE__".to_string()).await;
                    break;
                }
            }
            Ok(wreq::Message::Close(_)) => {
                if verbose {
                    println!("连接关闭");
                }
                let _ = reader_tx.send("__CLOSE__".to_string()).await;
                break;
            }
            Err(e) => {
                if verbose {
                    eprintln!("WebSocket read error: {}", e);
                }
                let _ = reader_tx.send("__CLOSE__".to_string()).await;
                break;
            }
            _ => {}
        }
    }

    let _ = write_handle.await;
    Ok(())
}

async fn handle_message(
    text: &str,
    state: &(std::sync::Mutex<ClientState>, Notify),
    callbacks: &Arc<std::sync::Mutex<Vec<StreamCallback>>>,
    verbose: bool,
    sender: &mpsc::Sender<String>,
) -> bool {
    let (lock, notify) = state;
    let mut state_guard = lock.lock().unwrap();

    if verbose && !text.starts_with('3') {
        println!("[RECV] {}", text);
    }

    if text.starts_with('0') {
        if verbose {
            println!("收到服务器 open 包");
        }
    } else if text == "40" {
        if verbose {
            println!("Socket.IO 握手确认");
        }
    } else if text == "2" {
        if verbose {
            println!("收到 PING，回复 PONG");
        }
        let _ = sender.send("3".to_string()).await;
    } else if text == "3" {
        if verbose {
            println!("收到 PONG");
        }
    } else if text.starts_with("42") {
        let payload = &text[2..];
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(payload) {
            if arr.len() >= 2 {
                let event_name = arr[0].as_str().unwrap_or("");
                let event_data = &arr[1];

                match event_name {
                    "on_connect_ack" => {
                        if let Some(code) = event_data.get("code").and_then(|v| v.as_i64()) {
                            if code == 1 {
                                if verbose {
                                    println!("连接确认成功");
                                }
                                if let Some(data) = event_data.get("data") {
                                    if let Some(obj) = data.as_object() {
                                        for (k, v) in obj {
                                            state_guard.user_info.insert(k.clone(), v.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "join_ack" => {
                        if let Some(code) = event_data.get("code").and_then(|v| v.as_i64()) {
                            if code == 1 {
                                if verbose {
                                    println!("加入房间成功");
                                }
                                if let Some(data) = event_data.get("data") {
                                    state_guard.user_id =
                                        data.get("user_id").and_then(|v| v.as_u64());
                                    state_guard.search_session = data
                                        .get("search_session")
                                        .and_then(|v| v.as_str())
                                        .map(String::from);
                                }
                                state_guard.joined = true;
                                notify.notify_one();

                                let sender_clone = sender.clone();
                                let _ = sender_clone
                                    .send(
                                        r#"42 ["preset_chat_message",{"turn_count":5,"system_content_enum":"default"}]"#
                                            .to_string(),
                                    )
                                    .await;
                                let _ = sender_clone
                                    .send(r#"42 ["get_text2Img_remaining_times"]"#.to_string())
                                    .await;
                            }
                        }
                    }
                    "chat_ack" => {
                        if let Some(code) = event_data.get("code").and_then(|v| v.as_i64()) {
                            if code == 1 {
                                if let Some(data) = event_data.get("data") {
                                    let content_type = data
                                        .get("content_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let content =
                                        data.get("content").and_then(|v| v.as_str()).unwrap_or("");

                                    match content_type {
                                        "stream_output_begin" => {
                                            if verbose {
                                                println!("[回复开始]");
                                            }
                                            state_guard.is_receiving_response = true;
                                            state_guard.current_response.clear();
                                            state_guard.response_complete = false;
                                            drop(state_guard);
                                            notify_callbacks(callbacks, "", StreamEventType::Start);
                                            state_guard = lock.lock().unwrap();
                                        }
                                        "stream_output_content" => {
                                            if verbose {
                                                print!("{}", content);
                                                let _ =
                                                    std::io::Write::flush(&mut std::io::stdout());
                                            }
                                            state_guard.current_response.push_str(content);
                                            drop(state_guard);
                                            notify_callbacks(
                                                callbacks,
                                                content,
                                                StreamEventType::Text,
                                            );
                                            state_guard = lock.lock().unwrap();
                                        }
                                        "stream_output_end" => {
                                            if verbose {
                                                println!("\n[回复结束]");
                                            }
                                            state_guard.is_receiving_response = false;
                                            state_guard.response_complete = true;

                                            let final_response =
                                                state_guard.current_response.clone();
                                            if !final_response.is_empty() {
                                                let mut entry = HashMap::new();
                                                entry.insert(
                                                    "role".to_string(),
                                                    "assistant".to_string(),
                                                );
                                                entry.insert(
                                                    "content".to_string(),
                                                    final_response.clone(),
                                                );
                                                state_guard.conversation_history.push(entry);
                                            }

                                            notify.notify_one();
                                            drop(state_guard);
                                            notify_callbacks(
                                                callbacks,
                                                &final_response,
                                                StreamEventType::End,
                                            );
                                            state_guard = lock.lock().unwrap();
                                        }
                                        _ => {}
                                    }
                                }
                            } else {
                                if let Some(code_msg) =
                                    event_data.get("code_msg").and_then(|v| v.as_str())
                                {
                                    if verbose {
                                        eprintln!("聊天错误: {} (code: {})", code_msg, code);
                                    }
                                    state_guard.is_receiving_response = false;
                                    state_guard.response_complete = true;
                                    state_guard.current_response = format!("错误: {}", code_msg);
                                    notify.notify_one();
                                }
                            }
                        }
                    }
                    _ => {
                        if verbose {
                            println!("[未处理事件] {}: {}", event_name, event_data);
                        }
                    }
                }
            }
        }
    }

    true
}

fn notify_callbacks(
    callbacks: &Arc<std::sync::Mutex<Vec<StreamCallback>>>,
    text: &str,
    event: StreamEventType,
) {
    let mut cbs = callbacks.lock().unwrap();
    for cb in cbs.iter_mut() {
        (cb)(text, event.clone());
    }
}

fn generate_session_id(length: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..length)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect()
}
