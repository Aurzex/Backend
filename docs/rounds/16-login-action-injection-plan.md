# 第八轮评审 — 登录流与动作分发的客户端注入:完成自动举报与动作处理路径

审阅日期:2026-08-29 · 基线:HEAD `9d7b4d9` · 范围:`src/api/auth.rs` 与 `src/core/{pipeline,services}.rs`

> 方案先行(本文档),随后落地代码。第七轮已注入举报引擎的**查询/取数**路径(`DataQuery`/`CommentQueryBuilder`/`ReportFetcher`/`ViolationChecker` 查询方法/`ReportProcessor`/`FileProcessor`),但**登录流**与**动作分发**仍硬编码全局。第八轮打通这两条剩余路径,使自动举报(多账号登录、举报与身份恢复)与动作处理(execute_process_*)可用注入的客户端。**延续允许破坏性 pub API 变更**。

## Context

第七轮落地后,`core` 层剩余的全局硬编码集中在两条「全局身份」与「全局单例」路径:

1. **自动举报登录流**(`pipeline.rs`):
    - `login_student`(关联函数)调用 `LoginBuilder::new()`,经 `AuthManager::new()`、`GlobalClientProvider` 到 `CodeMaoClient::global()`,把学生令牌写进**全局身份槽**。
    - `switch_identity(Catsona::Judge)` 调用 `CodeMaoClient::global()`,把身份切回管理员。
    - `execute_single_report` 的 5 处 api-Manager `.new()`(`ForumActionHandler`/`BaseWorkOperations`/`CommentOperations`/`WorkshopActionHandler`)仍走全局默认。
    - 三者必须**同一身份源**(登录写入全局,举报携带学生令牌,随后切回管理员),第七轮因此**整体保持全局**,第八轮一次性打通。

2. **动作分发**(`pipeline.rs` 与 `services.rs`):
    - `ActionFn` 是 `fn` 指针(`fn(i32, i32, Resolution) -> Result<bool, ProcessorError>`),不可捕获 client。
    - `ActionRegistry` 是全局 `LazyLock` 单例(`static ACTION_REGISTRY`),`global_action_registry()` 供 `apply_action_by_key` 分发 `execute_process_*` 动作,内部 `ReportHandler::new()` 走全局。

`auth.rs` 已具备 `ClientProvider` 依赖注入(`GlobalClientProvider` 返回全局,`AuthManager::new_with_provider(Box<dyn ClientProvider>)` 存在),但缺一个「持有 `CodeMaoClient` 的本地 provider」,且 `LoginBuilder::new()` 硬编码 `AuthManager::new()`(全局)。补齐这两个缺口即可让登录流可注入。

目标:给 `LoginBuilder` 增加客户端注入入口,让 `login_student`/`switch_identity`/`execute_single_report` 改用 `self.client`;把 `ActionFn` 改为可捕获闭包、`ActionRegistry` 改为注入实例,消除全局 `LazyLock` 单例。原则沿用 `CONTRIBUTING.md`。

## Approach

两个阶段相互独立,按顺序执行(每阶段结束时 `cargo check --all-targets` 通过)。

### Phase 1 — 登录流客户端注入(`auth.rs` 与 `pipeline.rs`)

**`src/api/auth.rs`**:

1. 新增本地客户端 provider(置于 `GlobalClientProvider` 之后):

```rust
/// 持有独立 `CodeMaoClient` 的本地 provider,供登录流写入注入的客户端身份槽
#[derive(Debug, Clone)]
pub struct LocalClientProvider {
    client: CodeMaoClient,
}

impl LocalClientProvider {
    pub fn new(client: CodeMaoClient) -> Self {
        Self { client }
    }
}

impl ClientProvider for LocalClientProvider {
    fn client(&self) -> &CodeMaoClient {
        &self.client
    }

    fn clone_box(&self) -> Box<dyn ClientProvider> {
        Box::new(self.clone())
    }
}
```

2. `LoginBuilder`:
    - `pub fn new()` 改为 `Self::new_with_client(CodeMaoClient::global().clone())`(保留全局默认,README 与既有调用不受影响)。
    - 新增 `pub fn new_with_client(client: CodeMaoClient) -> Self`,把 `new()` 原字段初始化原样搬入,`auth_manager` 用 `AuthManager::new_with_provider(Box::new(LocalClientProvider::new(client)))`。

**`src/core/pipeline.rs`**:

3. `login_student` 由关联函数改为 `&self` 方法:`fn login_student(&self, username: &str, password: &str) -> Result<(), ProcessorError>`,内部 `LoginBuilder::new()` 改为 `LoginBuilder::new_with_client(self.client.clone())`。调用点 `Self::login_student(&user, &pass)` 改为 `self.login_student(&user, &pass)`。
4. `switch_identity` 中 `CodeMaoClient::global().switch_identity(Catsona::Judge)` 改为 `self.client.switch_identity(Catsona::Judge)`。
5. `execute_single_report` 内 5 处 api-Manager `.new()` 改为 `.new_with_client(self.client.clone())`,涉及 `ForumActionHandler`、`BaseWorkOperations`、`CommentOperations`、`WorkshopActionHandler`。

至此自动举报全链路(登录、举报、恢复身份)统一走 `self.client`,第七轮「保持全局」的限制解除。

### Phase 2 — 动作分发注入(`pipeline.rs` 与 `services.rs`)

**`src/core/pipeline.rs`**:

1. `ActionFn` 改为 `type ActionFn = Box<dyn Fn(i32, i32, Resolution) -> Result<bool, ProcessorError> + Send + Sync>;`。
2. `ActionRegistry` 新增字段 `client: CodeMaoClient`(私有)。
    - `pub(crate) fn new()` 改为 `Self::new_with_client(CodeMaoClient::global().clone())`;新增 `pub(crate) fn new_with_client(client: CodeMaoClient) -> Self`。
    - `register_report_handler!` 宏里的 `|report_id, admin_id, resolution| -> Result<bool, ...> { ReportHandler::new().$handler(...) }` 改为 `move |report_id, admin_id, resolution| { ReportHandler::new_with_client(client.clone()).$handler(...) }`(每 handler 捕获 `client.clone()`,经宏外 `client` 闭包可见)。
3. 删除全局单例:`static ACTION_REGISTRY: LazyLock<ActionRegistry>` 与 `pub(crate) fn global_action_registry()` 一并删除。
4. `apply_action_by_key` 签名增加 `client: &CodeMaoClient` 参数,函数体把 `global_action_registry().apply(...)` 改为 `ActionRegistry::new_with_client(client.clone()).apply(...)`。若 `apply_action_by_key` 内部还经另一个函数(如 `apply_action`)调用 `global_action_registry()`,一并把该函数签名增加 `client` 并透传(以 `cargo check` 定位完整调用链)。

**`src/core/services.rs`**:

5. `apply_action_by_key(config, report_id, admin_id, action)?` 改为 `apply_action_by_key(config, report_id, admin_id, action, &self.client)?`。
6. 顶部 import 移除 `global_action_registry`。

## 不落地(记录在案)

- **`cloudvar.rs` 的 `detect_editor`**:自由函数用 `WorkDataFetcher::new()`(全局)自动识别编辑器类型。注入需让 `CloudBuilder` 额外持有 `CodeMaoClient`(当前只持 `authorization_token`),牵涉 WS 客户端与 HTTP 客户端的关系,属另一处设计决策。
- **`pipeline.rs` 的 `forum_post_content_line`**:展示助手用 `ForumDataFetcher::new()`。注入需改 `ReportDisplay` trait 签名并贯穿展示注册表(`LazyLock` 全局),收益低,不在第八轮。
- **类型化返回(`MewResult<Value>` 改为 DTO)**:api 层 351 处返回 `serde_json::Value`,独立大轮。
- **`DecompilerError` 包装 `MewError`**:消除其自带 Io/Json/Http 重复,独立小改。

## Critical files & anchors

| 文件 | 锚点 | 原因 |
| --- | --- | --- |
| `src/api/auth.rs` | `ClientProvider`、`GlobalClientProvider`、`AuthManager::new_with_provider`、`LoginBuilder::new` | Phase 1 落点;`LocalClientProvider` 插入点 |
| `src/core/pipeline.rs` | `ActionFn`、`ActionRegistry`、`global_action_registry`、`apply_action_by_key`、`login_student`、`switch_identity`、`execute_single_report` | Phase 1/2 落点 |
| `src/core/services.rs` | `apply_action_by_key` 调用、import | Phase 2 落点 |

## Verification

前置:每阶段结束时 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全部通过(库单测、`compile_live` 与 `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. `grep -rn "global_action_registry" src/` 结果为 0(全局单例已删)。
2. `grep -rn "LoginBuilder::new()" src/` 仅命中 `LoginBuilder::new_with_client` 内部,无裸 `new()` 调用;`grep -rn "CodeMaoClient::global()" src/core/pipeline.rs` 结果为 0(登录、举报与身份恢复全走 `self.client`)。
3. `grep -rn "ActionHandler::new()\|Operations::new()\|ReportHandler::new()" src/core/pipeline.rs` 结果为 0(全部为 `new_with_client`)。
4. `grep -rn "new_with_client" src/core/pipeline.rs` 命中 `ActionRegistry` 定义与 `login_student`/`execute_single_report` 内 5 处。

新行为检查:仿第六/七轮契约测试,在 `pipeline.rs` 的 `#[cfg(test)]` 增加一条 `action_registry_new_with_client_uses_injected_client`(用 `ActionRegistry::new_with_client(CodeMaoClient::new_independent(KittyConfig::default()))`,断言 `apply` 对未注册 method 的报错行为与全局一致——仅验证构造不 panic 且可分发;实际动作需真机接口,以 code review 与编译为准)。

其余行为以 code review 与编译为准:第八轮为等价重写(登录、举报与动作分发从全局默认改为可注入,`new()` 仍走全局),不改动任何端点、参数与请求体。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error。
- `cargo clippy --all-targets` 0 warning。
- `cargo test` 全部通过:库单测 5 passed、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(真机命中 codemao 服务)、doc-tests 0。
- 归零验证:`grep "global_action_registry" src/` 结果为 0;`grep "CodeMaoClient::global()" src/core/pipeline.rs` 仅命中 `ActionRegistry::new()` 委托;`grep "LoginBuilder::new()" src/core/pipeline.rs` 结果为 0;`grep "ActionHandler::new()\|Operations::new()\|ReportHandler::new()" src/core/pipeline.rs` 结果为 0(仅剩 `forum_post_content_line` 的 `ForumDataFetcher::new()`,见「不落地」)。

## 范围偏差(实际执行中确定,记录在案)

- **`LocalClientProvider` 不能 `#[derive(Debug)]`**:`CodeMaoClient` 未实现 `std::fmt::Debug`,但 `ClientProvider` trait 要求 `Debug`。改为 `#[derive(Clone)]` 与手写 `impl std::fmt::Debug`(仅打印结构名,`finish_non_exhaustive`)。
- **`ActionFn` 闭包需 `Box::new`**:`fn` 指针改为 `Box<dyn Fn>` 后,`register_report_handler!` 宏内的闭包不再自动协变为 `fn` 指针,需显式 `Box::new(move |...| {...})`(每个 handler 经 `let c = client.clone()` 捕获独立克隆)。
- **`services.rs` 的 `apply_group`(批量动作)也调用了 `global_action_registry()`**:计划只提及 `apply_action_by_key`,实际批量动作路径同样调用,已一并改为 `apply_action_by_method(&self.client, ...)`。

## Assumptions & contingencies

- **`apply_action_by_key` 每调用重建 `ActionRegistry`**:4 个 handler 的 `Box<dyn Fn>` 构造成本可忽略;若认为该构造存在开销,可改为 `ReportProcessor` 持有 `Arc<ActionRegistry>` 并在构造时注入——实现时二选一,默认选「每调用重建」(改动最小)。
- **`LocalClientProvider` 命名**:与 `GlobalClientProvider` 对称,沿用 `ClientProvider` trait;若存在同名校验,实现时以编译为准。
- **`register_report_handler!` 宏闭包捕获**:宏内闭包改为 `move` 后需 `client.clone()` 进入每个闭包(`CodeMaoClient` 实现 `Clone`,`Arc` 克隆代价低);若宏展开处 `client` 不可见,把 `client.clone()` 作为宏参数传入(实现时以编译为准)。
- **`LoginBuilder::new_with_client` 的 `pid` 缺省**:与 `new()` 一致,`pid` 用 `DEFAULT_PID.to_string()`(沿用现有逻辑,不改缺省语义)。
