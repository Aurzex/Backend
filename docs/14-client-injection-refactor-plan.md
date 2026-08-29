# 第六轮评审 — 客户端注入重构:消除全局单例硬编码 + 相关去重

审阅日期:2026-08-29 · 基线:HEAD `119d10f` · 范围:`src/api/{account,captcha,clouddb,codegame,community,education,forum,library,shop,user,whale,work}.rs` + `src/core/{compiler,cloudvar,converse}.rs` + `src/utils/{requests,socketio}.rs`

> 方案先行(本文档),随后落地代码;`Verification` 为计划校验项,实际执行结果见文末「Verification(实际执行结果)」。与前五轮不同:本轮**允许破坏性 pub API 变更**(用户明确授权)。`auth.rs`(已用 `ClientProvider` 依赖注入)与 `core/{retrieve,registry,services}.rs`(举报引擎,见下「不落地」)不在范围。**Phase 4(去重 WorkType/KittenVersion)经用户决定不执行**;Phase 1/2/3/5 已落地。

## Context

项目自述(`README.md`「可替换边界」「设计要点」)宣称 `CodeMaoClient` 支持全局单例 / 独立实例(`new_independent`)/ 自定义认证(`new_with_auth`)/ 自定义 `KittyAuth`,且 `ClientProvider` 用于依赖注入与测试。但实际业务层把这条边界封死,存在以下确定性缺陷与重复:

1. **业务层硬编码全局客户端**:13 个 api 域的全部 Manager 字段都是 `client: &'static CodeMaoClient`,构造函数 `new()` 一律 `Self { client: CodeMaoClient::global() }`(全仓 ≈37 处)。调用方**无法**用独立/自定义客户端构造任何 Manager——「可替换边界」在业务层不可达。
2. **反编译器硬编码全局**:`core/compiler.rs` 的 `CodemaoDecompiler::global()`,内部 `KittyFactory::global_client().clone()`。
3. **举报引擎硬编码全局**:`core/retrieve.rs` 直调 `CodeMaoClient::global()`(第 627、1071 行)。
4. **六套错误枚举**:`MewError`(utils)、`CloudError`(cloudvar)、`ChatError`(converse)、`DecompilerError`(compiler)、`ProcessorError`(registry)、`DataQueryError`(retrieve)。其中 `CloudError` 与 `ChatError` 有 7 个完全相同的变体(WebSocket/Handshake/Json/Send/NotConnected/Auth/Thread),重复维护。
5. **跨模块类型重复**:`WorkType { Kitten=1, Nemo=3, CodeGame=5 }` 与 `KittenVersion { V3, V4 }` 在 `api/user.rs` 与 `api/work.rs` 各定义一份(变体、判别值、`as_str` 映射完全一致;`work.rs` 的 `WorkType` 实为死代码,全仓零调用点)。
6. **冗余向后兼容分发**:`CodeMaoClient::new(config)` 依 `config.use_global_auth` 分发,`KittyConfig.use_global_auth`/`with_independent_auth()` 已无存在意义(构造函数已显式化为 `new_with_global_auth`/`new_independent`),两者全仓零调用点。

目标:把「可替换边界」真正打通到业务层,顺带消除上述确定性的重复与死代码。原则沿用 `CONTRIBUTING.md`:简洁可读、不新增依赖、不引入宏/多余抽象、`lock().unwrap()`、thiserror 保留底层变体。

## Approach

五个阶段彼此独立,可按任意顺序执行;建议按下述顺序,每个阶段结束 `cargo check --all-targets` + `cargo test` 均须绿。

### Phase 1 — 业务 Manager 客户端注入

**问题**:Manager 硬编码 `CodeMaoClient::global()`,可替换边界在业务层不可达。

**改动**:改为「持有可注入的 `CodeMaoClient`」。`CodeMaoClient` 是 `#[derive(Clone)]`(内部 `Arc<KittyCore>`),按值持有零成本。

穷举清单:`grep -rn "client: &'static CodeMaoClient" src/api/`,命中即为待改结构体(≈37 个,分布在 `account/captcha/clouddb/codegame/community/education/forum/library/shop/user/whale/work` 12 个文件)。

对每个命中结构体做三件事:

1. 字段类型 `client: &'static CodeMaoClient` → `client: CodeMaoClient`。
2. `new()` 改为委托 `Self::new_with_client(CodeMaoClient::global().clone())`(保留全局默认,README 与测试的 `Xxx::new()` 调用不受影响)。
3. 新增构造函数(每个 Manager 一份,签名逐字一致):

```rust
pub fn new_with_client(client: CodeMaoClient) -> Self {
    Self { client }
}
```

`ClientAccess` trait 签名不变:`fn client(&self) -> &CodeMaoClient` 返回 `&self.client`(按值字段可直接借用)。

**特例** — 组合结构体:`work.rs` 的 `KittenWorkManager`(418 行附近)与 `NekoWorkManager`(580 行附近)持有公开字段 `pub operations: BaseWorkOperations`、`pub comments: CommentOperations`。其 `new_with_client` 必须把同一个 client 传播给子 Manager:

```rust
pub fn new_with_client(client: CodeMaoClient) -> Self {
    Self {
        client: client.clone(),
        operations: BaseWorkOperations::new_with_client(client.clone()),
        comments: CommentOperations::new_with_client(client),
    }
}
```

其余 Manager 的 `new()` 若内部显式构造 `BaseWorkOperations::new()`/`CommentOperations::new()`,一并改为用同一 client(`client.clone()`)构造,避免组合体内部又退回全局。

不引入宏、不抽泛型基类(违反「不过度抽象」);`new_with_client` 逐结构体手写。

### Phase 2 — 反编译器客户端注入

**位置**:`core/compiler.rs`。

**改动**:

1. `pub(crate) struct CodemaoDecompiler` → `pub struct CodemaoDecompiler`;字段 `client: Arc<CodeMaoClient>` 保持不变。
2. 构造函数改为公开的 `pub fn new(client: CodeMaoClient) -> Self`;原 `pub(crate) fn new(config: Option<DecompilerConfig>, client: Arc<CodeMaoClient>)` 改名私有 `fn new_inner(config: Option<DecompilerConfig>, client: Arc<CodeMaoClient>) -> Self`,`new` 内部直接 `Self::new_inner(None, Arc::new(client))`(不保留双签名,避免二义)。
3. `pub(crate) fn global()` → `pub fn global() -> &'static Self`,内部把 `KittyFactory::global_client().clone()` 改为 `CodeMaoClient::global().clone()`。
4. `pub(crate) fn decompile` / `decompile_with_options` / `decompile_batch` → 全部改为 `pub`。
5. 模块级三个自由函数 `decompile_work`(4098)、`decompile_work_with`(4103)、`decompile_works`(4108)保留为 `CodemaoDecompiler::global()` 的薄委托(README 示例与 `tests/compile_live.rs`、`tests/live_features.rs` 均依赖 `decompile_work_with`),仅更新其 doc 注明「自定义客户端请用 `CodemaoDecompiler::new(client)`」。

`decompile_work(work_id, output_dir: Option<&Path>) -> Result<String>` 的「`None` 返回 JSON 字符串 / `Some` 返回文件路径」返回类型重载为既有已文档化行为(README 示例 5),本轮**不改**其签名。

### Phase 3 — 清理冗余的 `CodeMaoClient::new` 与 `use_global_auth`

**位置**:`src/utils/requests.rs`。

**改动**:

1. 删除 `KittyConfig` 字段 `use_global_auth: bool`(260 行)、`Default` 中的 `use_global_auth: true`(269 行)、方法 `with_independent_auth`(300-303 行)。
2. 删除 `CodeMaoClient::new(config: KittyConfig)`(957-963 行)——已 grep 确证全仓零调用点(`CodeMaoClient::new(` 只命中该定义本身)。三个显式构造函数 `global` / `new_with_global_auth` / `new_independent` / `new_with_auth` 全部保留。

无需改 README(README 未提及 `new()` 与 `with_independent_auth`)。

### Phase 4 — 去重 `WorkType` / `KittenVersion`(已取消 — 用户决定不执行,方案保留备查)

**位置**:`src/api/user.rs` + `src/api/work.rs`。

**改动**:新建 `src/api/types.rs`,并在 `src/api.rs` 加一行 `pub mod types;`。内容:

```rust
//! 跨业务域共享的作品类型枚举(避免 user / work 重复定义)

/// 作品类型
#[derive(Debug, Clone, Copy)]
pub enum WorkType {
    Kitten = 1,
    Nemo = 3,
    CodeGame = 5,
}

impl WorkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkType::Kitten => "1",
            WorkType::Nemo => "3",
            WorkType::CodeGame => "5",
        }
    }
}

/// Kitten 版本
#[derive(Debug, Clone, Copy)]
pub enum KittenVersion {
    V3,
    V4,
}

impl KittenVersion {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            KittenVersion::V3 => "KITTEN_V3",
            KittenVersion::V4 => "KITTEN_V4",
        }
    }
}
```

- `WorkType::as_str` 用 `pub`(原 `user.rs` 为 `pub fn`,保持可见性);`KittenVersion::as_str` 用 `pub(crate)`(原两处均为私有 `fn`,但移动到 sibling 模块后 `user.rs`/`work.rs` 需调用,必须至少 `pub(crate)`)。
- `src/api/user.rs`:删除 `WorkType`(16-31 行)与 `KittenVersion`(84-97 行)定义,顶部加 `use crate::api::types::{KittenVersion, WorkType};`。两个 `WorkType` 调用点(636、677 行的 `types: Vec<WorkType>`)与一个 `KittenVersion` 调用点(472 行)无需改动(仅类型路径变化)。
- `src/api/work.rs`:删除 `WorkType` 定义(16-21 行,死代码,全仓零调用点,`grep WorkType` 只命中定义本身)与 `KittenVersion` 定义(24-37 行),顶部加 `use crate::api::types::KittenVersion;`。`KittenVersion` 调用点(2058 行)无需改动。
- 注意:`core/compiler.rs` 有**另一个**同名 `pub(crate) enum WorkType { Kitten2..Wood }`(560 行),与 api 层的 `WorkType` 语义完全不同,**不合并、不改名**。

### Phase 5 — 合并 `CloudError` + `ChatError` → `SocketError`

**问题**:`CloudError` 与 `ChatError` 的 7 个公共变体完全重复。

**改动**:合并为单一 WS 客户端错误类型,放在共享 WS 基础设施 `src/utils/socketio.rs`(cloudvar / converse 均已依赖该模块)。

1. `src/utils/socketio.rs` 新增(变体为二者并集,`#[from]`/`impl From<tungstenite::Error>` 语义与现有一致):

```rust
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
```

2. `src/core/cloudvar.rs`:删除 `enum CloudError`(63-85 行)与 `impl From<tungstenite::Error> for CloudError`(87-91 行);`pub(crate) type Result<T> = std::result::Result<T, CloudError>` → `= std::result::Result<T, SocketError>`;加 `use crate::utils::socketio::SocketError;`;把文件中所有 `CloudError`/`CloudError::` 出现处(用 `grep CloudError src/core/cloudvar.rs` 穷举)替换为 `SocketError`/`SocketError::`。变体名逐一对应(WebSocket/Handshake/Json/Send/NotConnected/VariableNotFound/ListNotFound/InvalidArgument/Auth/Thread 全部同名)。

3. `src/core/converse.rs`:同法删除 `enum ChatError`(42-64 行)与 `impl From<tungstenite::Error> for ChatError`(66-70 行),`Result` 别名改 `SocketError`,替换全部 `ChatError` 出现处(Busy/Timeout/MissingToken 是 converse 独有但已并入 `SocketError`,其余同名)。

**边界**:其余四套错误(`MewError` / `DecompilerError` / `ProcessorError` / `DataQueryError`)**不做扁平化合并**:`ProcessorError`、`DataQueryError` 已通过 `#[from] MewError` 正确包装传输层;`DecompilerError` 携带反编译专属结构化变体(MissingField/TypeMismatch 等),强行并入一个巨型枚举会破坏内聚、违反「包装错误时保留底层变体」——本轮只消除最明确的 `CloudError`/`ChatError` 重复。

## 不落地(记录在案)

- **`core/retrieve.rs` 的全局硬编码**(`DataQuery` 是单元结构体、`CommentQueryBuilder` 无 client 字段,却硬编码 `CodeMaoClient::global()`;`stream_edu_accounts_with_reset_passwords` 内 `EduDataFetcher::new()` 与 `switch_identity` 直调全局):其注入需给两个类型新增 `client: CodeMaoClient` 字段并贯穿全部请求构造,与 api 层机械替换不同,连同 `core/{registry,services}.rs` 举报引擎的同类硬编码,归入后续单独一轮。
- **`decompile_work` 返回类型重载**(`None` 返回 JSON 字符串 / `Some` 返回文件路径):既有已文档化行为,拆分为独立 `decompile_to_json`/`decompile_to_file` 属另一处 API 语义重构,不在本轮。
- **Phase 4 去重 `WorkType`/`KittenVersion`**:用户决定不执行(2026-08-29),方案保留在 Approach 备查;`user.rs`/`work.rs` 的两处重复定义维持现状。

## Critical files & anchors

| 文件                                            | 锚点                                                                                 | 原因                                                                 |
| ----------------------------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| `src/utils/requests.rs`                         | `CodeMaoClient`(925)、`new()`(957)、`KittyConfig`(255)、`ClientAccess`(1913)         | Phase 1/3 落点;`CodeMaoClient::global().clone()` 是 `new()` 委托目标 |
| `src/api/work.rs`                               | `KittenWorkManager`(418)、`NekoWorkManager`(580)、`WorkType`(17)/`KittenVersion`(25) | 组合体需传播 client;死代码删除点                                     |
| `src/core/compiler.rs`                          | `CodemaoDecompiler`(≈3920)、`global()`(3933)、自由函数(4098-4110)                    | Phase 2 落点                                                         |
| `src/core/cloudvar.rs` / `src/core/converse.rs` | `CloudError`(63)/`ChatError`(42)、`Result` 别名(94/73)                               | Phase 5 落点                                                         |
| `src/api/user.rs`                               | `WorkType`(17)/`KittenVersion`(85) 定义与 472/636/677 调用点                         | Phase 4 落点                                                         |

## Verification

前置:每个阶段结束 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全绿(库单测 + `compile_live` + `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. **Phase 1**:`grep -rn "client: &'static CodeMaoClient" src/` → 0;`grep -rn "pub fn new_with_client" src/api/` 命中数 == `grep -rn "client: &'static" src/api/`(改前)的结构体数。
2. **Phase 3**:`grep -rn "CodeMaoClient::new(" src/` → 0(仅剩 `new_with_*`);`grep -rn "use_global_auth\|with_independent_auth" src/` → 0。
3. **Phase 4**:`grep -rn "enum WorkType\|enum KittenVersion" src/api/` → 仅 `src/api/types.rs` 命中;`grep -rn "WorkType" src/api/work.rs` → 0(死代码已删)。
4. **Phase 5**:`grep -rn "CloudError\|ChatError" src/` → 0,仅剩 `SocketError`。

新行为检查(Phase 1 的契约测试,加到 `src/api/account.rs` 的 `#[cfg(test)]` 模块):

```rust
#[test]
fn manager_new_with_client_uses_injected_client() {
    let a = CodeMaoClient::new_independent(KittyConfig::default());
    let b = CodeMaoClient::new_independent(KittyConfig::default());
    let m = AccountManager::new_with_client(a.clone());
    a.set_token(Catsona::Fluffy, "tok-a").unwrap();
    assert_eq!(m.client().current_token().as_deref(), Some("tok-a"));
    assert_eq!(b.current_token(), None); // b 独立,不受影响
}
```

该测试观察到的行为是「Manager 用的是注入的客户端身份状态,而非全局」,若 `new_with_client` 退回全局会失败。

其余行为(网络请求语义)在无网络环境下以 code review + 编译为准:Phase 1/2/4/5 均为等价重写或纯可见性放宽,不改动任何 HTTP/WS 端点、参数或请求体。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error。
- `cargo clippy --all-targets` 0 warning。
- `cargo test` 全绿:库单测 5 passed(含新增 `manager_new_with_client_uses_injected_client` + 既有 4)、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(`login_and_ai_chat` / `cloud_variables` / `decompile_works`,真机命中 codemao 服务)、doc-tests 0。
- 归零验证:
    - `grep "client: &'static CodeMaoClient" src/` → 0(Phase 1)。
    - `grep "CodeMaoClient::new(\|use_global_auth\|with_independent_auth" src/` → 0(Phase 3)。
    - `grep "CloudError\|ChatError" src/` → 0,仅剩 `SocketError`(Phase 5)。
- `KittyFactory` 仍被 `core/{pipeline,services}.rs` 引用(非死代码),保留;仅清理了 `compiler.rs` 中因 Phase 2 失效的 `KittyFactory` import。

## Assumptions & contingencies

- **`WorkType`/`KittenVersion` 判别值一致**:已读取两处定义确认完全一致(均为 `Kitten=1,Nemo=3,CodeGame=5` 与 `{V3,V4}`)。若实现时发现某处被第三方外部 crate 以 `backend::api::work::WorkType` 路径引用(仓内 grep 已确认零引用),无需保留兼容别名——用户已授权破坏性变更,直接删除。
- **`KittyFactory` 处置**:`docs/12` 记载其为「冗余门面,被多处引用,非死代码」。Phase 2 只移除其唯一与全局强耦合的使用(`KittyFactory::global_client()`);若改后 `grep -rn "KittyFactory" src/` 返回 0,则删除该 struct 及其 impl(遵守 `CONTRIBUTING.md`「全仓确证零调用点才删」);若仍有调用点,保留不动,不阻塞。
- **`CodeMaoClient::new(config)` 删除安全性**:已 grep 确证 `CodeMaoClient::new(` 全仓仅命中定义本身,零调用点;`use_global_auth` 仅被 `new()` 读取。删除无外部影响。
- **`SocketError` 放置位置**:若 `src/utils/socketio.rs` 未直接依赖 `serde_json`/`std::sync::mpsc`(其已解析 tungstenite 帧,必然依赖 tungstenite;serde_json 为全仓公共依赖),按需补 `use` 即可,不新增 Cargo 依赖。
