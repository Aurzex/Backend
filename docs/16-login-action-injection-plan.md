# 第八轮评审 — 登录流与动作分发的客户端注入:完成自动举报与动作处理路径

审阅日期:2026-08-29 · 基线:HEAD `9d7b4d9` · 范围:`src/api/auth.rs` + `src/core/{pipeline,services}.rs`

> 方案先行(本文档),随后落地代码。第七轮已注入举报引擎的**查询/取数**路径(`DataQuery`/`CommentQueryBuilder`/`ReportFetcher`/`ViolationChecker` 查询方法/`ReportProcessor`/`FileProcessor`),但**登录流**与**动作分发**仍硬编码全局。本轮打通这两条剩余路径,使自动举报(多账号登录 + 举报 + 身份恢复)与动作处理(execute_process_*)可用注入的客户端。**延续允许破坏性 pub API 变更**。

## Context

第七轮落地后,`core` 层剩余的全局硬编码集中在两条「全局身份」与「全局单例」路径:

1. **自动举报登录流**(`pipeline.rs`):
    - `login_student`(973,关联函数)用 `LoginBuilder::new()` → `AuthManager::new()` → `GlobalClientProvider` → `CodeMaoClient::global()`,把学生令牌写进**全局身份槽**。
    - `switch_identity(Catsona::Judge)`(849)用 `CodeMaoClient::global()`,把身份切回管理员。
    - `execute_single_report`(998)的 5 处 api-Manager `.new()`(`ForumActionHandler`/`BaseWorkOperations`/`CommentOperations`/`WorkshopActionHandler`,1017/1027/1041/1051/1063)仍走全局默认。
    - 三者必须**同一身份源**(登录写全局 → 举报带学生令牌 → 切回管理员),第七轮因此**整体保持全局**,本轮一次性打通。

2. **动作分发**(`pipeline.rs` + `services.rs`):
    - `ActionFn`(193)是 `fn` 指针(`fn(i32, i32, Resolution) -> Result<bool, ProcessorError>`),不可捕获 client。
    - `ActionRegistry`(195)是全局 `LazyLock` 单例(`static ACTION_REGISTRY` 253),`global_action_registry()`(254)供 `apply_action_by_key`(270)分发 `execute_process_*` 动作,内部 `ReportHandler::new()` 走全局。

`auth.rs` 已具备 `ClientProvider` 依赖注入(`GlobalClientProvider` 返回全局,`AuthManager::new_with_provider(Box<dyn ClientProvider>)` 存在),但缺一个「持有 `CodeMaoClient` 的本地 provider」,且 `LoginBuilder::new()` 硬编码 `AuthManager::new()`(全局)。补齐这两个缺口即可让登录流可注入。

目标:给 `LoginBuilder` 增加客户端注入入口,让 `login_student`/`switch_identity`/`execute_single_report` 改用 `self.client`;把 `ActionFn` 改可捕获闭包、`ActionRegistry` 改注入实例,消除全局 `LazyLock` 单例。原则沿用 `CONTRIBUTING.md`。

## Approach

两个阶段相互独立,按顺序执行(每阶段结束 `cargo check --all-targets` 绿)。

### Phase 1 — 登录流客户端注入(`auth.rs` + `pipeline.rs`)

**`src/api/auth.rs`**:

1. 新增本地客户端 provider(置于 `GlobalClientProvider` 之后,266 行附近):

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

2. `LoginBuilder`(1214 行区域):
    - `pub fn new()` → `Self::new_with_client(CodeMaoClient::global().clone())`(保留全局默认,README 与既有调用不受影响)。
    - 新增 `pub fn new_with_client(client: CodeMaoClient) -> Self`,把 `new()` 原字段初始化原样搬入,`auth_manager` 用 `AuthManager::new_with_provider(Box::new(LocalClientProvider::new(client)))`。

**`src/core/pipeline.rs`**:

3. `login_student`(973)由关联函数改 `&self` 方法:`fn login_student(&self, username: &str, password: &str) -> Result<(), ProcessorError>`,内部 `LoginBuilder::new()` → `LoginBuilder::new_with_client(self.client.clone())`。调用点 892 `Self::login_student(&user, &pass)` → `self.login_student(&user, &pass)`。
4. `switch_identity`(849)`CodeMaoClient::global().switch_identity(Catsona::Judge)` → `self.client.switch_identity(Catsona::Judge)`。
5. `execute_single_report`(998)内 5 处 api-Manager `.new()` → `.new_with_client(self.client.clone())`:`ForumActionHandler`(1017/1051)、`BaseWorkOperations`(1027)、`CommentOperations`(1041)、`WorkshopActionHandler`(1063)。

至此自动举报全链路(登录 → 举报 → 恢复身份)统一走 `self.client`,第七轮「保持全局」的限制解除。

### Phase 2 — 动作分发注入(`pipeline.rs` + `services.rs`)

**`src/core/pipeline.rs`**:

1. `ActionFn`(193)→ `type ActionFn = Box<dyn Fn(i32, i32, Resolution) -> Result<bool, ProcessorError> + Send + Sync>;`。
2. `ActionRegistry`(195)新增字段 `client: CodeMaoClient`(私有)。
    - `pub(crate) fn new()` → `Self::new_with_client(CodeMaoClient::global().clone())`;新增 `pub(crate) fn new_with_client(client: CodeMaoClient) -> Self`。
    - `register_report_handler!` 宏里的 `|report_id, admin_id, resolution| -> Result<bool, ...> { ReportHandler::new().$handler(...) }` → `move |report_id, admin_id, resolution| { ReportHandler::new_with_client(client.clone()).$handler(...) }`(每 handler 捕获 `client.clone()`,经宏外 `client` 闭包可见)。
3. 删除全局单例:`static ACTION_REGISTRY: LazyLock<ActionRegistry>`(253)与 `pub(crate) fn global_action_registry()`(254-256)删除。
4. `apply_action_by_key`(270)签名加 `client: &CodeMaoClient` 参数,函数体把 `global_action_registry().apply(...)` 改为 `ActionRegistry::new_with_client(client.clone()).apply(...)`。若 `apply_action_by_key` 内部还经另一个函数(如 264 行的 `apply_action`)调用 `global_action_registry()`,一并把该函数签名加 `client` 并透传(以 `cargo check` 定位完整调用链)。

**`src/core/services.rs`**:

5. `apply_action_by_key(config, report_id, admin_id, action)?`(443)→ `apply_action_by_key(config, report_id, admin_id, action, &self.client)?`。
6. 顶部 import(16)移除 `global_action_registry`。

## 不落地(记录在案)

- **`cloudvar.rs:2207` `detect_editor`**:自由函数用 `WorkDataFetcher::new()`(全局)自动识别编辑器类型。注入需让 `CloudBuilder` 额外持有 `CodeMaoClient`(当前只持 `authorization_token`),牵涉 WS 客户端与 HTTP 客户端的关系,属另一处设计决策。
- **`pipeline.rs:431` `forum_post_content_line`**:展示助手用 `ForumDataFetcher::new()`。注入需改 `ReportDisplay` trait 签名并贯穿展示注册表(`LazyLock` 全局),收益低,不在本轮。
- **类型化返回(`MewResult<Value>` → DTO)**:api 层 351 处返回 `serde_json::Value`,独立大轮。
- **`DecompilerError` 包装 `MewError`**:消除其自带 Io/Json/Http 重复,独立小改。

## Critical files & anchors

| 文件                   | 锚点                                                                                                                                                                              | 原因                                      |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `src/api/auth.rs`      | `ClientProvider`(238)、`GlobalClientProvider`(250)、`AuthManager::new_with_provider`(803)、`LoginBuilder::new`(1228)                                                              | Phase 1 落点;`LocalClientProvider` 插入点 |
| `src/core/pipeline.rs` | `ActionFn`(193)、`ActionRegistry`(195/206)、`global_action_registry`(253)、`apply_action_by_key`(270)、`login_student`(973)、`switch_identity`(849)、`execute_single_report`(998) | Phase 1/2 落点                            |
| `src/core/services.rs` | `apply_action_by_key` 调用(443)、import(16)                                                                                                                                       | Phase 2 落点                              |

## Verification

前置:每阶段结束 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全绿(库单测 + `compile_live` + `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. `grep -rn "global_action_registry" src/` → 0(全局单例已删)。
2. `grep -rn "LoginBuilder::new()" src/` → 仅 `LoginBuilder::new_with_client` 内无裸 `new()` 调用;`grep -rn "CodeMaoClient::global()" src/core/pipeline.rs` → 0(登录/举报/身份恢复全走 `self.client`)。
3. `grep -rn "ActionHandler::new()\|Operations::new()\|ReportHandler::new()" src/core/pipeline.rs` → 0(全部 `new_with_client`)。
4. `grep -rn "new_with_client" src/core/pipeline.rs` → 覆盖 `ActionRegistry` 定义 + `login_student`/`execute_single_report` 内 5 处。

新行为检查:仿第六/七轮契约测试,在 `pipeline.rs` 的 `#[cfg(test)]` 加一条 `action_registry_new_with_client_uses_injected_client`(用 `ActionRegistry::new_with_client(CodeMaoClient::new_independent(KittyConfig::default()))`,断言 `apply` 对未注册 method 的报错行为与全局一致——仅验证构造不 panic 且可分发;实际动作需真机接口,以 code review + 编译为准)。

其余行为以 code review + 编译为准:本轮为等价重写(登录/举报/动作分发从全局默认改为可注入,`new()` 仍走全局),不改动任何端点/参数/请求体。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error。
- `cargo clippy --all-targets` 0 warning。
- `cargo test` 全绿:库单测 5 passed、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(真机命中 codemao 服务)、doc-tests 0。
- 归零验证:`grep "global_action_registry" src/` → 0;`grep "CodeMaoClient::global()" src/core/pipeline.rs` → 仅 `ActionRegistry::new()` 委托;`grep "LoginBuilder::new()" src/core/pipeline.rs` → 0;`grep "ActionHandler::new()\|Operations::new()\|ReportHandler::new()" src/core/pipeline.rs` → 0(仅剩 `forum_post_content_line` 的 `ForumDataFetcher::new()`,见「不落地」)。

## 范围偏差(实际执行中确定,记录在案)

- **`LocalClientProvider` 不能 `#[derive(Debug)]`**:`CodeMaoClient` 未实现 `std::fmt::Debug`,但 `ClientProvider` trait 要求 `Debug`。改为 `#[derive(Clone)]` + 手写 `impl std::fmt::Debug`(仅打印结构名,`finish_non_exhaustive`)。
- **`ActionFn` 闭包需 `Box::new`**:`fn` 指针改 `Box<dyn Fn>` 后,`register_report_handler!` 宏内的闭包不再自动协变为 `fn` 指针,需显式 `Box::new(move |...| {...})`(每个 handler 经 `let c = client.clone()` 捕获独立克隆)。
- **`services.rs` 的 `apply_group`(批量动作)也调用了 `global_action_registry()`**:计划只提了 `apply_action_by_key`(443),实际还有 520 行的批量动作路径,已一并改为 `apply_action_by_method(&self.client, ...)`。

## Assumptions & contingencies

- **`apply_action_by_key` 每调用重建 `ActionRegistry`**:4 个 handler 的 `Box<dyn Fn>` 构造成本可忽略;若嫌浪费,可改为 `ReportProcessor` 持有 `Arc<ActionRegistry>` 并在构造时注入——实现时二选一,默认选「每调用重建」(改动最小)。
- **`LocalClientProvider` 命名**:与 `GlobalClientProvider` 对称,沿用 `ClientProvider` trait;若存在同名校验,实现时以编译为准。
- **`register_report_handler!` 宏闭包捕获**:宏内闭包改 `move` 后需 `client.clone()` 进每个闭包(`CodeMaoClient` 是 `Clone`,`Arc` 廉价);若宏展开处 `client` 不可见,把 `client.clone()` 作为宏参数传入(实现时以编译为准)。
- **`LoginBuilder::new_with_client` 的 `pid` 缺省**:与 `new()` 一致,`pid` 用 `DEFAULT_PID.to_string()`(沿用现有逻辑,不改缺省语义)。
