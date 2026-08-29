# 第十轮评审 — 架构与 API 设计评审:登录流精简 / compiler 切分 / 并发锁模型 / 错误模型收敛 / 命名与可见性收敛

审阅日期:2026-08-30 · 基线:HEAD `4a62bb3` · 范围:`src/api/auth.rs` + `src/core/{compiler,unpacker,converse}.rs` + `src/utils/requests.rs` + `src/core/{retrieve,registry,pipeline}.rs`(改名调用点)+ `README.md` + `CONTRIBUTING.md`

> 方案先行(本文档),随后落地代码。前九轮已打通客户端注入、错误模型收敛与反编译返回语义修正。本轮从「架构与 API 设计」六原则视角做全景评审,输出按优先级排序的整改路线图。评审结论全部锚定实际读到的文件/符号/行号。

## Context

评审范围 25,510 行。本轮不推翻已定稿设计(客户端注入、错误分层骨架、反编译返回 `PathBuf`),聚焦确定性改进,分五类:

1. **登录流过度分层**:`LoginBuilder`+`LoginSession` 两类型一 DSL;`AuthManager` 三个委托字段两个冗余(创建两个 `AuthProcessor` + 三个 provider box)。
2. **大文件**:`compiler.rs` 4117 行,切两个文件(遵循 core 层 8 字母文件名约定)。
3. **并发锁模型**:审计 ~134 处 `lock().unwrap()` 后结论「保持 std,不引入第三方锁库」;唯一真实缺陷是 converse `connect()` 活性缺陷。
4. **错误模型**:`MewError` 实际 6 变体,`Other(String)` 语义不达意,文档与实际不符。
5. **命名与可见性**:`GlobalKittyAuth`/`LocalKittyAuth`/`KittyIdentityManager` 本可 `pub(crate)`;57 个 `fetch_*_gen` 方法后缀非惯用。

**演示层结论(评估后不落地)**:`core::terminal`(922 行)的 `ProcessorUi`(trait)/`ConsoleUi`/`ReportConsole` 定位为**可外部调用的举报控制台 UI 组件**——下游通过实现 `ProcessorUi` 或直接调 `ReportConsole::run(&mut ui, &processor, admin_id)` 嵌入自己的工具,无需独立运行(无 `main`)。因此 `terminal.rs` 保留在库内(`pub mod terminal`),**不迁入 `main.rs`、不迁入 `examples/`、不删除**;`main.rs` 仍作为默认 binary 的薄演示驱动。原「演示泄漏」判断撤回。

原则沿用 `CONTRIBUTING.md`(不引入额外锁依赖、不过度抽象、thiserror 保留底层变体、简洁可读)。**延续允许破坏性 pub API 变更**(0.1.0 窗口)。

### 优先级总表

| 优先级 | 编号   | 建议                                                             | 破坏 API?             | 性质       |
| ------ | ------ | ---------------------------------------------------------------- | --------------------- | ---------- |
| P1     | #1     | 登录流精简(合并 LoginBuilder+LoginSession、AuthManager 去重)     | 是(删 `LoginSession`) | 结构精简   |
| P1     | #2     | compiler.rs 切两个文件(`compiler` 门面 / `unpacker` 引擎)        | 否(路径不变)          | 可读性     |
| P1     | #3     | 并发锁:保持 std + 修 converse 活性缺陷                           | 否                    | 正确性     |
| P1     | #4     | 错误模型:`Other`→`InvalidArgument` + 文档对齐                    | 是(改名 `Other`)      | 语义清晰   |
| P1     | #5     | pub(crate) 收紧(3 个 auth 类型)                                  | 否                    | API 面收敛 |
| P1     | #6     | `fetch_*_gen` → `fetch_*_iter`(57 方法 + 17 调用点)              | 是(方法改名)          | 命名惯用   |
| P2     | #7     | 非锁裸 unwrap 硬化(约 13 处,均安全)                              | 否                    | 风格一致   |
| P2     | #8-#11 | 公开 API 萌化名→直白名 / prelude / newtype ID / 反编译 HTTP 注入 | 部分                  | 见下文     |

---

## Approach

各阶段相互独立,按优先级顺序执行(每阶段结束 `cargo check --all-targets` 绿)。

### Phase 1 — 登录流精简:合并 LoginBuilder+LoginSession、AuthManager 去重(P1)

**问题定位**

- `src/api/auth.rs` 六类型全 pub(`AuthProcessor` L365、`LoginHandler` L583、`AuthManager` L823、`LoginBuilder` L1243、`LoginSession` L1351、`CloudAuthenticator` L1160)。
- **LoginBuilder+LoginSession 无缝两段式**:`build()`(L1324)仅把 9 字段打包成 `LoginCredentials` 并搬进 `LoginSession`,`execute()`(L1359)一行透传 `AuthManager::login`。两类型只为「构造无副作用 → execute 才发网络」一条,而这条已由「builder 只有 setter」天然保证。
- **AuthManager 三个委托字段两个冗余**(L824-826):`client_provider` 仅用于 logout 端点与 `configure_authentication_token`;`processor` 仅用于 `admin_login` 的 `fetch_admin_details`(L1044)。`new_with_provider`(L832-834)创建**两个** `AuthProcessor` + **三个** `Box<dyn ClientProvider>`。

**改动 — A:合并 LoginBuilder ∪ LoginSession**

删除 `LoginSession`(L1349-1363 整个类型)与 `LoginBuilder::build`(L1324-1340),在 `LoginBuilder` 上新增 `execute(&mut self)`:

```rust
impl LoginBuilder {
    // … 9 个 setter 不变 …

    /// 执行登录(网络请求)。构造阶段无副作用,此处才发起请求。
    pub fn execute(&mut self) -> MewResult<LoginResult> {
        let credentials = LoginCredentials {
            identity: self.identity.take().unwrap_or_default(),
            password: self.password.take().unwrap_or_default(),
            token: self.token.take().unwrap_or_default(),
            pid: self.pid.take().unwrap_or_else(|| DEFAULT_PID.to_string()),
            status: self.status,
            role: self.role,
            timestamp: self.timestamp,
            captcha: self.captcha.take(),
        };
        self.auth_manager.login(&credentials, self.prefer_method)
    }
}
```

调用点 `src/core/pipeline.rs:985`(login_student)由 `.build(); session.execute()` 改为 `builder.execute()`;README L75-80 示例 `let mut session = …; session.execute()?` 改为 `builder.execute()?`。

**改动 — B:AuthManager 字段去重**

```rust
pub struct AuthManager {
    handler: LoginHandler,
    current_credentials: Option<LoginCredentials>,
    current_method: Option<LoginMethod>,
}

impl AuthManager {
    pub fn new_with_provider(provider: Box<dyn ClientProvider>) -> Self {
        Self {
            handler: LoginHandler::new_with_provider(provider),
            current_credentials: None,
            current_method: None,
        }
    }

    fn client(&self) -> &CodeMaoClient {
        self.handler.client()
    }
}
```

`admin_login` 内 L1044 的 `self.processor.fetch_admin_details()` 改 `self.handler.processor.fetch_admin_details()`(同模块内私有字段可见,无需加方法)。

**保留不动**

- `AuthProcessor`(纯传输,返回裸 `Value`,无 token 副作用)↔ `LoginHandler`(token 提取 + 持久化 + `LoginResult` 塑形):真实 seam,保留。
- `AuthManager` 去重后是「一个协作者 + 两个状态字段」的真门面,保留。

**影响面与风险**

- 6 类型 → 5 类型;删 1 类型 + 2 字段 + 1 冗余 `AuthProcessor` + 1 provider box。破坏面:`LoginSession` 仅被 `pipeline.rs:985` 与 README 消费。
- 风险:`execute(&mut self)` 用 `.take()` 消费 String 字段(零克隆),`status`/`role` 为 Copy 枚举直接拷。

**验证**

- `grep -n "LoginSession" src/` → 0;`grep -n "client_provider" src/api/auth.rs` → 仅 `ClientProvider` trait/impl 相关,`AuthManager` 无字段。

---

### Phase 2 — compiler.rs 切两个文件(门面 / 引擎)(P1)

**问题定位**:`compiler.rs` 4117 行。切分必须保留公开路径 `backend::core::compiler::{DecompileOptions, CodemaoDecompiler, decompile_work, decompile_work_with, decompile_works, DecompilerError}`(README L149 + 测试 `tests/compile_live.rs:22`、`tests/live_features.rs:21-22,214`)。

**文件名约定**:core 层现有 8 个文件(`cloudvar`/`compiler`/`converse`/`pipeline`/`registry`/`retrieve`/`services`/`terminal`)均为 8 字母,切分后两文件亦取 8 字母:门面沿用 `compiler`,引擎命名 `unpacker`(un-pack-er = 8 字母;「unpack = 反编译」,把编译产物解包为 Blockly XML;备选 `decipher` = 8 字母,强调解密)。

**切分方案(纯搬迁,零签名改写)**:门面 / 引擎两段,按 `// 反编译选项(构建器)`(L3764)为界:

**`src/core/unpacker.rs`(新,引擎,私有模块)** — 原 compiler.rs L22-3762,全部 `pub(crate)`/私有内部项 + 唯一 pub 项 `DecompilerError`:

- 错误:`DecompilerError`(L24)、`From<io::Error>`/`From<serde_json::Error>`(L47/53)、`Result`(L59)、`ResultExt`(L62)、`ValueExt`(L76)。
- 配置/模型/文件/加密:`ShadowTemplate`(L160)、`DecompilerConfig`(L184)、`WorkType`(L568)、`WorkInfo`(L639)、`FileService`(L681)、`IdGenerator`(L728)、`CryptoService`(L758)、`NONCE_SIZE`(L762)、`BCMKNDecryptor`(L835)。
- 积木块/反编译引擎:`ShadowBuilder`(L855)、`BlockDecompilerBehavior`(L1001)、`BlockBehavior`(L1006)、`BlockContext`(L1033)、`BlockDecompilerCore`(L1099)、`DecompilerContext`/`Builder`(L1452/1461)、`DecompileResult`(L1533)、`RawWorkData`(L1538)、`WorkFetcher`(L1546)、`WorkDecompiler`(L1550)、`save_json_result`/`save_path_result`(L1561/1589)、5 个 fetcher + 5 个 decompressor + resource manager + `XmlBlockWriter`(L1600-3193)、`BlockDecompiler` trait + 9 impl(L3195-3633)、`create_block_decompiler`(L3663)、`BlockDecompilerFactory`(L3690)。
- HTTP 客户端:`HttpClient`(L3709)、`CodeMaoHttpClient`(L3723)。

**`src/core/compiler.rs`(保留,门面 + 公开 API)** — 原 L3764-4117:

- `DecompileOptions`(L3767)、`FetcherFactory`(L3812)、`DecompilerFactory`(L3815)、`WorkProcessorRegistry`(L3820)、`CodemaoDecompiler`(L3911)、`decompile_work`(L4105)、`decompile_work_with`(L4110)、`decompile_works`(L4115)。
- 顶部补 re-export(唯一需 `pub use` 的 pub 项)+ `use` 引用的引擎项:

```rust
pub use crate::core::unpacker::DecompilerError;
use crate::core::unpacker::{
    CodeMaoHttpClient, DecompilerConfig, DecompilerContext, DecompileResult, FileService,
    HttpClient, IdGenerator, Result, WorkFetcher, WorkInfo, WorkType,
};
```

**`src/core.rs`**:加 `mod unpacker;`(私有)。

**影响面与风险**

- 依赖方向单向:门面(`compiler.rs`)→ 引擎(`unpacker.rs`),引擎不反向引用门面。公开路径不变(仅 `DecompilerError` 经 `pub use` 保留原路径)。纯移动无逻辑变更,回归风险低。
- 已知局限:引擎 `unpacker.rs` 仍约 3740 行(门面约 350 行),这是「只切两个文件」的固有结果;若后续需进一步拆分,再按段切子模块(本轮不做,见「不落地」)。
- 风险:分文件后 `use` 语句未下沉会报未使用 import;`#[cfg(test)]` 单测若跨文件引用需一并迁。

**验证**

- `cargo check --all-targets` + `cargo test` + `cargo clippy --all-targets` 零新增警告;`use backend::core::compiler::{DecompileOptions, decompile_work, decompile_works}` 在 `tests/live_features.rs`/`tests/compile_live.rs` 仍编译。

---

### Phase 3 — 并发/锁:保持 std 同步模型,不引入第三方锁库;修 converse connect() 活性缺陷(P1)

**结论先行**:审计(`cloudvar.rs` ~90 处、`converse.rs` ~30 处、`services.rs` 11 处、`pipeline.rs` 1 处、`socketio.rs` 2 处 `lock().unwrap()`;`retrieve.rs` 零锁)显示现有 std 模型**无死锁环、中毒面极小、锁粒度合理**。第三方锁库只能带来边际抛光,不带来正确性收益,不值得为此破坏「零第三方锁依赖」卖点。**保持 `CONTRIBUTING.md`「不引入额外锁依赖」约定,不修订为引入锁库**;仅修一个活性缺陷。

**替换点清单 + 量化(说明为何不迁移)**

| 替换点(符号:行)                                                                                  | 现状                          | 候选 crate           | 收益量化               | 成本/风险                                             | 结论        |
| ------------------------------------------------------------------------------------------------ | ----------------------------- | -------------------- | ---------------------- | ----------------------------------------------------- | ----------- |
| `CloudInner.state`(cloudvar.rs:548,DataStore 6 HashMap)                                          | `Mutex<DataStore>`            | dashmap              | 读多写少,getter 可免锁 | `create_*`/`clear_all` 跨多 map 事务,dashmap 失原子性 | 不迁        |
| `ChatInner.user_info`(converse.rs:139)/`history`(142)                                            | `Mutex<HashMap>`/`Mutex<Vec>` | RwLock               | 收益≈0                 | —                                                     | 不迁        |
| `CloudInner.editor`(cloudvar.rs:525)、`*.tx`(cloudvar.rs:542/converse.rs:132)                    | `Mutex<Option<T>>`            | OnceLock/arc-swap    | write-once 后多读      | clone-then-drop 已正确,收益≈0                         | 不迁        |
| `connect_lock`(cloudvar.rs:546/converse.rs:135)、`network_lock`(services.rs:211/pipeline.rs:556) | `Mutex<()>`                   | parking_lot::Mutex   | 消除中毒 + 微快        | 持锁跨网络 IO 是设计取舍;仅省 unwrap 中毒             | 可选,不强制 |
| `Notify`(socketio.rs:133-134)                                                                    | `Mutex<()>`+`Condvar`         | parking_lot::Condvar | 无(模式已正确)         | —                                                     | 保持 std    |

**量化结论**

- **panic 面**:回调全部锁外 `take_all()` 执行且包 `catch_unwind`(cloudvar 1619-1622 等 8 处;converse 691-694);`notify_with` 闭包只做原子存——锁内从不执行用户代码,中毒实际不可能发生。锁内仅有的两处 `.unwrap()`(converse.rs:302 `history.last()`、services.rs:815 `pending.remove()`)由紧邻 push/长度检查保证。故 `lock().unwrap()` 在此库**安全**。
- **锁竞争**:所有锁语句/块级作用域,guard 在每次 `send`/`join`/回调前显式释放(cloudvar.rs:772-773 注释明确防死锁窗口;converse.rs:294 注释避免嵌套取锁)。唯一跨阻塞持有的是 `connect_lock`(建连串行化),属持有时长取舍非竞争热点。
- **死锁**:无嵌套双锁持有;`connect_lock` 是唯一外层锁,内层锁在其下逐个获取、从不并发持有两个。无死锁环。

**唯一真实缺陷(需修)**:converse.rs `connect()`(L255-274)持 `connect_lock` 跨一次阻塞 `wait_flag` 等待(L269-273),谓词只判 `joined`。若另一线程在等待期间调 `close()`(L419-436),`close()` 已置 `stopping=true`(L421)并 `notify_with` 通知(L429-434,内部 `notify_all`),但 `connect()` 的谓词不含 `stopping`,唤醒后重查 `joined`(仍 false)继续睡 → `connect()` 挂满 `connect_timeout`。**缺口在 `connect()` 谓词,不在 `close()`(close 已通知)。**

改动(`converse.rs` L269-273):

```rust
let joined = wait_flag(
    &self.inner.notify,
    self.inner.connect_timeout,
    || {
        self.inner.joined.load(Ordering::Acquire)
            || self.inner.stopping.load(Ordering::Acquire)
    },
);
// 等待期间被 close():按「未连接成功」返回 false,而非挂满 connect_timeout
Ok(joined && !self.inner.stopping.load(Ordering::Acquire))
```

说明:`stopping` 在 `connect()` 开头(L264)已 `store(false)` 复位,故谓词中的 `stopping` 只反映本次 connect 期间发生的 `close()`;`joined && !stopping` 正确处理「连接成功但随即被 close」的边角。

**保持 std 的场景(明确)**

- callback 存储、DataStore、队列、会话/历史/响应缓冲:std::sync::Mutex。
- write-once 配置(`editor`/`tx`/`read_join`/`flush_join`):std::sync::Mutex<Option<T>>,不用 OnceLock/arc-swap。
- 条件等待:std::sync::Condvar + Mutex(现有 `Notify` 模式正确)。
- 跨线程身份切换令牌 `network_lock`:std::sync::Mutex<()>。
- **破例场景(仅当未来实测出现竞争热点/中毒才引入)**:`parking_lot::Mutex` 替代 `connect_lock`/`network_lock`;`RwLock` 替代 `user_info`/`history`/`BatchActionManager`。当前无测量,不做。

**渐进步骤**

1. 先修 converse `connect()` 谓词(小、独立、有测试价值)。
2. 更新 `CONTRIBUTING.md` 锁条款,把「不引入额外锁依赖」明确为「默认 std;仅当测量证明竞争/中毒热点时,经评审可对粗粒度令牌引入 parking_lot」——记录性澄清,不改变现状行为。

**验证**

- 新增行为测试:线程 A `connect()`(设短 `connect_timeout`)、线程 B 立即 `close()`,断言 `connect()` 在超时前返回 `Ok(false)`(而非挂满 timeout)。

---

### Phase 4 — 错误模型:消除 `Other(String)` 字符串魔法 + 文档对齐(P1)

**问题定位**

- 定稿错误模型宣称 `MewError` = `Http/Io/Json/HttpStatus{status,body}`,但实际 `src/utils/requests.rs:21-35` 有 6 变体,多出 `Auth(String)`(L32)与 `Other(String)`(L34)。
- `Other(String)` 实为**客户端侧参数校验/前置条件错误**:forum.rs:146「数据长度需小于 20」、forum.rs:526/532「必须提供 board_id/workshop_id」、whale.rs:389「不支持此决议类型」、requests.rs:773/783「该 HTTP 方法不支持/需要请求体」、requests.rs:142「无效 base URL 键」。
- 另有两处 `Other` 是**服务端契约违背**(非参数错误):requests.rs:1619/1627「上传响应缺少必填字段」。
- `Auth(String)` 是认证域错误,字符串承载消息,调用方只能 `matches!(MewError::Auth(_))`。
- 二者是合法的新错误类别(非把传输错误压成 String,不违「保留底层变体」),但 `Other` 命名不达意,README L215/CONTRIBUTING L38 与实际 6 变体不符。

**改动**

1. `requests.rs` `MewError` 的 `Other(String)` 重命名为 `InvalidArgument(String)`:

```rust
#[derive(ThisError, Debug)]
pub enum MewError {
    #[error("HTTP error: {0}")]
    Http(#[from] ureq::Error),
    #[error("HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Auth error: {0}")]
    Auth(String),
    /// 调用方参数非法(客户端前置条件错误)
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
}
```

2. 7 处校验类构造点 `MewError::Other(…)` → `MewError::InvalidArgument(…)`:forum.rs:146/526/532、whale.rs:389、requests.rs:142/773/783。

3. requests.rs:1619/1627 是服务端契约违背,改走 `Json` 错误(`serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, msg))` 包装,`serde_json::Error` 已 `#[from]` 进 `Json`),不再走 `InvalidArgument`。

4. `Auth(String)` 保留为认证域类别,更新 doc 注释「凭据/身份域错误,区别于传输错误」;不细分成员(克制:消息已够信息量,细分破坏 `matches!` 惯用)。

5. 更新 README L215「错误模型分层」与 CONTRIBUTING L38,补 `Auth`/`InvalidArgument` 两类域错误。

**影响面与风险**

- `Other` → `InvalidArgument` 是公开 API 命名变更(SemVer breaking),crate 处 0.1.0 可接受;grep `MewError::Other` 全仓 9 处一次性改名。

**验证**

- `grep -n "MewError::Other" src/` → 0;`cargo check --all-targets` + `cargo test` 通过。

---

### Phase 5 — pub(crate) 收紧:3 个 auth 类型(P1)

**问题定位**:`GlobalKittyAuth`(requests.rs:398)、`LocalKittyAuth`(L433)、`KittyIdentityManager`(L234)三个 `pub` 类型是「本可私有却公开」的泄漏。用户经 `CodeMaoClient::global()`/`new_with_global_auth()`/`new_independent()` 使用默认实现,或经 `new_with_auth(config, Arc<dyn KittyAuth>)` 注入自定义 `impl KittyAuth`,均无需具名这三者。

**改动**

- 三者 `pub` → `pub(crate)`。`KittyAuth` trait 保持 pub(下游自定义认证提供者的扩展 seam);`KittyConfig` 保持 pub(`new_with_auth` 构造参数);`MewError`/`MewResult`/`Catsona`/`CodeMaoClient`/`PaginatedIter`/`HTTPStatus`/`BaseKey` 保持 pub(公开契约)。

**保持 pub 的常量**:`DEFAULT_PAGE_SIZE`/`DEFAULT_LIMIT`/`DEFAULT_OFFSET`/`DEFAULT_PID`(L149-155)与 `FETCH_ALL`(L158)均为内部使用且文档承诺的公开常量,不动(`FETCH_ALL` 内部无消费点但 README L157 承诺为 `.with_limit(FETCH_ALL)` 的全量标记,保留)。

**影响面与风险**

- 低;`lib.rs` 已 `#![warn(unreachable_pub)]`,收紧后无死代码告警。`impl KittyAuth for KittyIdentityManager`(L347)对 `pub(crate)` 类型实现 pub trait 合法(下游不具名该 impl,经 `new_with_auth` 自实现 `KittyAuth` 即可)。

**验证**

- `grep -rn "GlobalKittyAuth\|LocalKittyAuth\|KittyIdentityManager" src/ --include=*.rs` 仅命中 requests.rs 定义;`cargo check --all-targets` 通过。

---

### Phase 6 — `fetch_*_gen` → `fetch_*_iter`(P1)

**问题定位**:57 个分页方法用 `_gen`(generator)后缀,非 Rust 惯用(惯用为无后缀或 `_iter`)。分布:user.rs 12、work.rs 11、community.rs 10、education.rs 10、forum.rs 6、shop.rs 4、whale.rs 4。另有 17 处内部调用点(retrieve.rs 8、registry.rs 8、pipeline.rs 1)与 README L83 示例。

**改动**

1. 57 个 `pub fn fetch_*_gen(` → `pub fn fetch_*_iter(`(机械改名,`_gen` 后缀 → `_iter`,方法体不动)。
2. 17 处内部调用点 `.fetch_*_gen(` → `.fetch_*_iter(`:retrieve.rs:297/303/309/326/905/927/1003/1108、registry.rs:427/442/488/502/543/552/590/599、pipeline.rs:804。
3. README L83 `fetch_all_works_gen` → `fetch_all_works_iter`。

**影响面与风险**

- 公开方法改名(SemVer breaking),0.1.0 可接受;共 74 处(57 定义 + 17 调用 + README)机械改名,`cargo check` 即可定位残留。

**验证**

- `grep -n "fn .*_gen(" src/` → 0;`grep -n "_iter(" src/` 覆盖 57 定义 + 17 调用。

---

### Phase 7 — 非锁裸 unwrap 硬化(P2,可选)

**问题定位**:doc 05 约定「禁裸 unwrap/expect(锁除外)」,但存在约 13 处非锁裸 `unwrap()`(全部**经检查安全**,属风格硬化而非 bug):

- `auth.rs:1199` `self.time_difference.unwrap()`——前置 `if is_none { set Some }` 保证 Some(`?` 早退),安全。
- `registry.rs:670` `active.as_mut().unwrap()`——前置 `if active.is_none() { …active = Some(…) }` 保证 Some,安全。
- `compiler.rs:958` `template.unwrap()`——前置 `if template.is_none() { return … }` 保证 Some,安全。
- `compiler.rs` 10 处 `write!(String, …).unwrap()`(L993/2163/2171/2188/2212/2240/3454/3552/3553/3561)——`std::fmt::Write for String` 永不 Err,安全但应改 `let _ = write!(…)`(与 auth.rs:1213 现有写法一致)。

**改动**

- `auth.rs:1199`/`registry.rs:670`/`compiler.rs:958` → `ok_or_else`/`expect("…")` 或重组为 `if let Some`,消除裸 unwrap。
- 10 处 `write!(String).unwrap()` → `let _ = write!(String, …);`。

**影响面与风险**:低,纯风格;不改行为。与 doc 05 目标风格一致。

---

## 低优先级清单(P2)

| 编号 | 建议                   | 问题定位                                                                                                                                                                     | 改动要点                                                                                                                                                                                        | 风险                                              |
| ---- | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| #8   | 公开 API 萌化名→直白名 | 11/15 萌化标识符下游可见(`MewError` L21、`Catsona` L183、`KittyAuth` L333、`KittyRequestBuilder` L483、`KittyConfig` L258 等),`MewError` 作为主错误类型下游每个 `use` 都要写 | `MewError`→`ClientError`、`MewResult`→`Result`、`Catsona`→`Identity`、`KittyAuth`→`AuthProvider`、`KittyRequestBuilder`→`RequestBuilder`、`HTTPStatus`→`HttpStatus`;私有 `KittyCore` 等保留萌化 | 大破坏面,仅 1.0 前做;与 #6 同批,每步 check 零残留 |
| #9   | prelude 模块           | 无 prelude,下游需 6-7 行 `use`                                                                                                                                               | 加 `backend::prelude` 重导出 `CodeMaoClient`/`ClientAccess`/`ClientError`/`Catsona`/`PaginatedIter`/`RequestBuilder`;不做 13 manager 全量(避免 work.rs/user.rs 各自 `KittenVersion` 同名冲突)   | 低,纯增量                                         |
| #10  | newtype ID(分阶段)     | `work_id`/`user_id`/`admin_id` 裸 `i32`/`i64` 混用(`main.rs:79`、`decompile_work(123456, None)`、`education.rs:30`)                                                          | 仅试点 `WorkId(i64)`(带 `From<i64>`/`Display`),`decompile_work`/`fetch_work_comments_gen` 收窄;验证后推广 `UserId`;不做全量 newtype                                                             | 中,试点可控                                       |
| #11  | 反编译器 HTTP 注入     | `HttpClient` trait(L3709)/`CodeMaoHttpClient`(L3723)是 `pub(crate)`,下游无法注入 mock                                                                                        | 若需可测试性,公开 `HttpClient`,`DecompileOptions` 增 `with_http_client`;当前不做(已可经 `CodeMaoClient` 注入)                                                                                   | 不做                                              |

---

## 不落地(记录在案)

- **演示层解耦(terminal)**:`core::terminal` 保留在库内(`pub mod terminal`),作为可外部调用的举报控制台 UI 组件(无 `main`),不迁入 `main.rs`、不迁入 `examples/`、不删除。`main.rs` 保持默认 binary 现状。
- **work.rs / cloudvar.rs 切分**:本轮只切 compiler.rs。work.rs(纯搬迁可切)/ cloudvar.rs(只能浅切叶子)留待后续;cloudvar 深拆需 60+ 可见性提升 + `Arc<CloudInner>` 贯通,非纯搬迁,不做。
- **compiler.rs 切子模块/子文件夹**:用户指定「两个文件、不放 core 子文件夹」,故不切 `compiler/{…}` 子模块;引擎单文件仍偏大是「只切两个文件」的固有结果。
- **parking_lot/dashmap/arc-swap/crossbeam**:审计证明 std 模型无死锁、中毒面极小、锁粒度合理,维持「零第三方锁依赖」。仅在实测出现竞争/中毒热点时按 Phase 3 表破例。
- **类型级 WS 状态机**(`CloudConnection<Connected>`):同步阻塞库连接态由运行时网络决定,维持运行时 `connect_and_wait()`。
- **全量 newtype ID**:13 manager 数百签名机械量大、收益递减,仅试点 `WorkId`。

## Critical files & anchors

| 文件                                                                                                       | 锚点                                                                          | 原因           |
| ---------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | -------------- |
| `src/api/auth.rs:823-850,1044,1243-1363`                                                                   | `AuthManager` 三字段 + `admin_login` 的 fetch + `LoginBuilder`/`LoginSession` | Phase 1 落点   |
| `src/core/compiler.rs:24,3764-4117` / `src/core.rs`                                                        | 公开面 + 切分边界(L3764)+ `mod unpacker;`                                     | Phase 2 落点   |
| `src/core/converse.rs:255-274,419-436`                                                                     | `connect()` 谓词 + `close()` 通知                                             | Phase 3 落点   |
| `src/utils/requests.rs:21-35,142,234,398,433,773,783,1619,1627`                                            | `MewError` 6 变体 + `Other` 构造点 + 3 个 pub(crate) 目标                     | Phase 4/5 落点 |
| `src/api/{user,work,community,education,forum,shop,whale}.rs` + `src/core/{retrieve,registry,pipeline}.rs` | 57 个 `fetch_*_gen` 定义 + 17 调用点                                          | Phase 6 落点   |

## Verification

前置:每阶段结束 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全绿(库单测 + `compile_live` + `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. **Phase 1**:`grep -n "LoginSession" src/` → 0。
2. **Phase 2**:`use backend::core::compiler::{DecompileOptions, decompile_work, decompile_works}` 在 tests 仍编译;`pub use crate::core::unpacker::DecompilerError` 保留原路径。
3. **Phase 3**:converse `connect()` 谓词含 `stopping`;行为测试断言并发 `close()` 时提前返回 `Ok(false)`。
4. **Phase 4**:`grep -n "MewError::Other" src/` → 0。
5. **Phase 5**:`grep -rn "GlobalKittyAuth\|LocalKittyAuth\|KittyIdentityManager" src/ --include=*.rs` 仅命中 requests.rs 定义。
6. **Phase 6**:`grep -n "fn .*_gen(" src/` → 0;`grep -n "_iter(" src/` 覆盖 57 定义 + 17 调用。

新行为检查:

- Phase 1:真机 `cargo run` 走完登录(`builder.execute()` 路径)。
- Phase 3:并发 `close()` 时 `connect()` 不再挂满 `connect_timeout`。

其余行为以 code review + 编译为准:Phase 1/2 为等价搬迁/精简,Phase 3 为活性修复,Phase 4/5/6 为改名 + 语义/可见性澄清,不改任何端点/请求体/落盘行为。

## Assumptions & contingencies

- **0.1.0 破坏窗口**:Phase 1(删 `LoginSession`)、Phase 4(改 `Other`)、Phase 6(`_gen`→`_iter`)、#8(公开 API 改名)属 SemVer breaking。假设团队 1.0 前可接受一次性破坏;**若不可接受**,仅执行非破坏项(Phase 2/3/5、#7/#9/#10 试点),Phase 4/6/#8 降级为「文档标注 deprecated 别名,1.0 移除」。
- **引擎文件名**:默认 `unpacker`(8 字母);若团队更倾向「解密」语义,等价改用 `decipher`(8 字母),不影响切分方案与 `pub use` 路径(仅换 `mod` 名)。
- **converse `stopping` 复位语义**:`connect()` 开头 L264 已 `stopping.store(false)`,故谓词加 `stopping` 不误判历史 close;若实现时发现 `stopping` 在其它路径被提前置位,以 `cargo check` 定位复位点。
- **`serde_json::Error::io` 构造**:Phase 4 的 requests.rs:1619/1627 用 `serde_json::Error::io(io::Error::new(ErrorKind::InvalidData, msg))`;若该构造器不可用,回退为新增专用变体 `InvalidResponse(String)`。实现时以 `cargo check` 为准。
- **`execute(&mut self)` 字段消费**:`status`/`role` 为 Copy 枚举直接拷,`Option<String>` 用 `.take()` 零克隆;若 `AccountStatus`/`UserRole` 非 Copy(实现时确认),改用 `.clone()`。
- **锁库结论随测量可逆**:若未来实测 `connect_lock`/`network_lock` 竞争热点或 `DataStore` 读吞吐成瓶颈,回退已定——parking_lot::Mutex 替换粗粒度令牌、RwLock 替换 `user_info`/`history`/`BatchActionManager`(Phase 3 表)。当前无测量,不引入。
- **真机验证依赖账号**:Phase 1 端到端验证需有效编程猫管理员账号;无账号时退化为 `cargo check --all-targets` + `cargo test`(集成测试自动跳过)作为最低验证,交付说明标注「真机路径未跑」。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error;`cargo clippy --all-targets` 0 warning。
- `cargo test` 全绿:库单测 5+5 passed、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(真机命中 codemao 服务)、doc-tests 0。
- 归零验证:`LoginSession` → 0;`MewError::Other` → 0;`GlobalKittyAuth\|LocalKittyAuth\|KittyIdentityManager` 跨文件引用 → 0;`fn .*_gen(` → 0;`_gen(` 全仓(含 tests/README)→ 0。
- core 层 9 个文件全部 8 字母:`compiler`=8、`unpacker`=8(其余 8 个不变)。
- `compiler.rs` 4117 → 367 行,`unpacker.rs` 3759 行(引擎仍偏大,「只切两个文件」固有结果)。
- 剩余非锁 `.unwrap()` 49 处经逐类核查:cloudvar 31/converse 7/services 6 为多行 `.lock()\n.unwrap()`(审计安全集),converse `history.last()` 与 services `pending.remove()` 由紧邻检查保证,terminal 2/main 1 为 demo io、account 1 为 `#[cfg(test)]` 断言、socketio 1 为 `wait_timeout`——无新 panic 面。

## 范围偏差(实际执行中确定,记录在案)

- **`MewError::Other` 实际 10 处而非 9 处**:计划列了 9 处,漏了 `auth.rs:498`「验证码文件写入失败」。该处是文件写入 I/O 失败(非参数校验),改为 `MewError::Io(std::io::Error::other(...))` 而非 `InvalidArgument`,语义更准。
- **`response_missing_field` 落点**:计划放在模块级,实际 `first_token_entry`/`required_str_field` 位于 `impl FileUploader` 内,helper 改为该 impl 的关联函数并以 `Self::response_missing_field` 调用(纯函数,不依赖 self)。
- **`search_*_gen` 一并改名**:Phase 6 计划只列 `fetch_*_gen`,实际 3 个 `search_posts_gen`/`search_kn_works_gen`/`search_published_kn_works_gen` 也是分页方法(返回 `PaginatedIter`),按同一约定改 `search_*_iter`(共 60 定义 + 18 调用)。
- **Phase 3 未补 CONTRIBUTING 锁条款**:计划「记录性澄清」,本轮仅改代码,`CONTRIBUTING.md` 锁条款原文未动(维持「不引入额外锁依赖」,与结论一致,无必要改)。
- **`lib.rs` 编辑损坏后重写**:prelude 追加时 edit 工具误操作致 `lib.rs` 内容损坏(utils 重复、api/core 丢失),已用 `write` 重写为 5 行正确内容,`cargo check` 通过。
