use http::header::HeaderName;
use http::header::HeaderValue;
use rand::RngExt;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{
    Arc, Condvar, Mutex,
    mpsc::{self, Receiver, Sender},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tungstenite::Message;
use tungstenite::{client::IntoClientRequest, connect};
use url::Url;

// ==================== 错误类型 ====================
#[derive(Debug)]
pub enum ChatError {
    WebSocket(String),
    Json(serde_json::Error),
    Url(url::ParseError),
    Connection(String),
    Timeout(String),
    SendError(String),
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

impl From<tungstenite::Error> for ChatError {
    fn from(e: tungstenite::Error) -> Self {
        ChatError::WebSocket(e.to_string())
    }
}

impl<T> From<std::sync::mpsc::SendError<T>> for ChatError {
    fn from(e: std::sync::mpsc::SendError<T>) -> Self {
        ChatError::SendError(e.to_string())
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
        Ok(Url::parse(&url)?)
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

impl Default for ClientState {
    fn default() -> Self {
        Self {
            is_receiving_response: false,
            current_response: String::new(),
            response_complete: false,
            user_info: HashMap::new(),
            conversation_history: Vec::new(),
            conversation_id: generate_session_id(8),
            user_id: None,
            search_session: None,
            connected: false,
            joined: false,
        }
    }
}

// ==================== WebSocket 管理器 ====================
struct WsHandle {
    sender: Sender<String>,
    thread_handle: JoinHandle<()>,
}

impl WsHandle {
    fn send(&self, msg: String) -> Result<(), ChatError> {
        self.sender.send(msg)?;
        Ok(())
    }

    fn shutdown(self) {
        let _ = self.sender.send("__CLOSE__".to_string());
        let _ = self.thread_handle.join();
    }
}

// ==================== 主客户端 ====================
pub struct CodeMaoChatClient {
    config: ChatConfig,
    token: String,
    ws_handle: Option<WsHandle>,
    state: Arc<(Mutex<ClientState>, Condvar)>,
    global_callbacks: Arc<Mutex<Vec<StreamCallback>>>,
}

impl CodeMaoChatClient {
    pub fn builder() -> ChatClientBuilder {
        ChatClientBuilder::new()
    }

    pub fn connect(&mut self) -> Result<(), ChatError> {
        let url = self.config.build_ws_url(&self.token)?;
        let state = Arc::clone(&self.state);
        let global_callbacks = Arc::clone(&self.global_callbacks);
        let verbose = self.config.verbose;

        let (tx, rx) = mpsc::channel::<String>();
        let reader_tx = tx.clone();

        let handle = thread::spawn(move || {
            if let Err(e) = run_ws_loop(url, rx, reader_tx, state, global_callbacks, verbose) {
                if verbose {
                    eprintln!("WebSocket loop error: {}", e);
                }
            }
        });

        self.ws_handle = Some(WsHandle {
            sender: tx,
            thread_handle: handle,
        });

        // 等待连接建立和加入房间完成
        let timeout = Duration::from_secs(self.config.connect_timeout);
        let (lock, cvar) = &*self.state;
        let mut guard = lock.lock().unwrap();
        let start = std::time::Instant::now();

        while (!guard.connected || !guard.joined) && start.elapsed() < timeout {
            guard = cvar
                .wait_timeout(guard, Duration::from_millis(100))
                .unwrap()
                .0;
        }

        if !guard.connected {
            return Err(ChatError::Timeout("连接超时".to_string()));
        }
        if !guard.joined {
            return Err(ChatError::Timeout("加入房间超时".to_string()));
        }

        if self.config.verbose {
            println!("连接成功");
        }

        Ok(())
    }

    pub fn send_message(&self, content: &str) -> MessageRequestBuilder<'_> {
        MessageRequestBuilder::new(self, content)
    }

    fn ws_send(&self, msg: String) -> Result<(), ChatError> {
        if let Some(ref ws) = self.ws_handle {
            ws.send(msg)?;
            Ok(())
        } else {
            Err(ChatError::Connection("未连接".to_string()))
        }
    }

    fn state_condvar(&self) -> &(Mutex<ClientState>, Condvar) {
        &self.state
    }

    fn add_global_callback(&self, cb: StreamCallback) {
        self.global_callbacks.lock().unwrap().push(cb);
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
            ws.shutdown();
        }
    }

    // 便捷工具函数
    pub fn quick_chat(token: &str, message: &str) -> Result<String, ChatError> {
        let mut client = CodeMaoChatClient::builder().token(token).build()?;
        client.connect()?;
        thread::sleep(Duration::from_secs(1));
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

    /// 发送消息并阻塞等待完整回复
    pub fn send_and_wait(self) -> Result<String, ChatError> {
        self.send_internal()?;

        let callbacks = self.callbacks;
        let _cleanup = if !callbacks.is_empty() {
            let mut guard = self.client.global_callbacks.lock().unwrap();
            let original_len = guard.len();
            guard.extend(callbacks);
            Some((original_len, Arc::clone(&self.client.global_callbacks)))
        } else {
            None
        };

        let timeout = Duration::from_secs(self.timeout);
        let (lock, cvar) = self.client.state_condvar();
        let mut state = lock.lock().unwrap();

        let start = std::time::Instant::now();
        while !state.is_receiving_response && !state.response_complete && start.elapsed() < timeout
        {
            state = cvar
                .wait_timeout(state, Duration::from_millis(100))
                .unwrap()
                .0;
        }

        if !state.is_receiving_response && !state.response_complete {
            return Err(ChatError::Timeout("等待回复开始超时".to_string()));
        }

        if state.response_complete {
            return Ok(state.current_response.clone());
        }

        let start = std::time::Instant::now();
        while !state.response_complete && start.elapsed() < timeout {
            state = cvar
                .wait_timeout(state, Duration::from_millis(100))
                .unwrap()
                .0;
        }

        if !state.response_complete {
            return Err(ChatError::Timeout("等待回复完成超时".to_string()));
        }

        let result = state.current_response.clone();

        if let Some((original_len, callbacks_arc)) = _cleanup {
            let mut guard = callbacks_arc.lock().unwrap();
            guard.truncate(original_len);
        }

        Ok(result)
    }

    pub fn send(self) -> Result<(), ChatError> {
        self.send_internal()
    }

    fn send_internal(&self) -> Result<(), ChatError> {
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
        self.client.ws_send(message_str)?;

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
            Mutex::new(ClientState {
                conversation_id: generate_session_id(8),
                ..Default::default()
            }),
            Condvar::new(),
        ));
        Ok(CodeMaoChatClient {
            config: self.config,
            token,
            ws_handle: None,
            state,
            global_callbacks: Arc::new(Mutex::new(self.callbacks)),
        })
    }
}

// ==================== WebSocket 事件循环 ====================
fn run_ws_loop(
    url: Url,
    rx: Receiver<String>,
    reader_tx: Sender<String>,
    state: Arc<(Mutex<ClientState>, Condvar)>,
    callbacks: Arc<Mutex<Vec<StreamCallback>>>,
    verbose: bool,
) -> Result<(), ChatError> {
    let mut request = url
        .as_str()
        .into_client_request()
        .map_err(|e| ChatError::Connection(format!("构建请求失败: {}", e)))?;

    let headers = request.headers_mut();
    headers.insert(
        HeaderName::from_static("origin"),
        HeaderValue::from_static("https://kn.codemao.cn"),
    );
    headers.insert(
        HeaderName::from_static("user-agent"),
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0"),
    );
    headers.insert(
        HeaderName::from_static("accept-encoding"),
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert(
        HeaderName::from_static("accept-language"),
        HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"),
    );
    headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        HeaderName::from_static("pragma"),
        HeaderValue::from_static("no-cache"),
    );

    let (ws, _) = connect(request)?;
    let (lock, _cvar) = &*state;
    // 标记 WebSocket 连接已建立（Python 在 on_open 中做）
    {
        let mut st = lock.lock().unwrap();
        st.connected = true;
        _cvar.notify_all();
    }

    // ★ 立即发送 "40"（不等待任何消息）
    let ws_writer = Arc::new(Mutex::new(ws));
    let mut ws_guard = ws_writer.lock().unwrap();
    ws_guard.send(Message::Text("40".into()))?;
    drop(ws_guard);

    // ★ 延迟 1 秒发送 join（完全复制 Python）
    let sender_for_join = reader_tx.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(1));
        let _ = sender_for_join.send(r#"42 ["join"]"#.to_string());
    });

    let ws_writer_clone = Arc::clone(&ws_writer);

    // writer 线程处理后续发送
    let writer_handle = thread::spawn(move || {
        for msg in rx {
            if msg == "__CLOSE__" {
                break;
            }
            if verbose && msg != "2" && msg != "3" && !msg.starts_with("40") {
                println!("[SEND] {}", msg);
            }
            let mut guard = ws_writer_clone.lock().unwrap();
            if let Err(e) = guard.send(Message::Text(msg.into())) {
                if verbose {
                    eprintln!("发送失败: {}", e);
                }
                break;
            }
        }
        if let Ok(mut ws_guard) = ws_writer_clone.lock() {
            let _ = ws_guard.close(None);
        }
    });

    // 读循环
    loop {
        let msg = {
            let mut ws_guard = ws_writer.lock().unwrap();
            ws_guard.read()
        };
        match msg {
            Ok(Message::Text(text)) => {
                let keep_going = handle_message(&text, &state, &callbacks, verbose, &reader_tx);
                if !keep_going {
                    let _ = reader_tx.send("__CLOSE__".to_string());
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                if verbose {
                    println!("连接关闭");
                }
                let _ = reader_tx.send("__CLOSE__".to_string());
                break;
            }
            Err(e) => {
                if verbose {
                    eprintln!("WebSocket read error: {}", e);
                }
                let _ = reader_tx.send("__CLOSE__".to_string());
                break;
            }
            _ => {}
        }
    }

    let _ = writer_handle.join();
    Ok(())
}

fn handle_message(
    text: &str,
    state: &(Mutex<ClientState>, Condvar),
    callbacks: &Arc<Mutex<Vec<StreamCallback>>>,
    verbose: bool,
    sender: &Sender<String>,
) -> bool {
    let (lock, cvar) = state;
    let mut state_guard = lock.lock().unwrap();

    if verbose && !text.starts_with("3") {
        println!("[RECV] {}", text);
    }

    // 协议消息处理
    if text.starts_with('0') {
        if verbose {
            println!("收到服务器 open 包");
        }
        // 不再从这里发 40，因为已经在连接建立时发送
    } else if text == "40" {
        if verbose {
            println!("Socket.IO 握手确认");
        }
        // join 已在连接后异步发送，此处不再处理
    } else if text == "2" {
        // 服务器 ping，回复 pong
        if verbose {
            println!("收到 PING，回复 PONG");
        }
        let _ = sender.send("3".to_string());
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
                                cvar.notify_all();

                                let sender_clone = sender.clone();
                                let _ = sender_clone.send(
                                    r#"42 ["preset_chat_message",{"turn_count":5,"system_content_enum":"default"}]"#
                                        .to_string(),
                                );
                                let _ = sender_clone
                                    .send(r#"42 ["get_text2Img_remaining_times"]"#.to_string());
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

                                            cvar.notify_all();
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
                                    cvar.notify_all();
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
    callbacks: &Arc<Mutex<Vec<StreamCallback>>>,
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
