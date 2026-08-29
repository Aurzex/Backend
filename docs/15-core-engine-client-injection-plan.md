# 第七轮评审 — 核心举报引擎客户端注入:完成可替换边界 + 移除 KittyFactory 门面

审阅日期:2026-08-29 · 基线:HEAD `fe2c9e6` · 范围:`src/core/{retrieve,registry,services,pipeline}.rs` + `src/utils/requests.rs`

> 方案先行(本文档),随后落地代码。承接第六轮(api 层 37 个 Manager 与反编译器已注入客户端),本轮打通 core 举报引擎的「可替换边界」,并移除因此变死代码的 `KittyFactory` 门面。本轮**延续允许破坏性 pub API 变更**。

## Context

第六轮把 `src/api/` 全部 Manager 与 `CodemaoDecompiler` 改为可注入(新增 `new_with_client`)。但 core 举报引擎仍在 4 个文件里硬编码全局客户端,方式分两类:

1. **api-Manager `.new()`(默认全局)**:举报引擎内部大量调用 `WorkDataFetcher::new()` / `ForumDataFetcher::new()` / `WhaleReportFetcher::new()` / `EduDataFetcher::new()` / `UserDataFetcher::new()` / `CommunityDataFetcher::new()` / `WorkshopDataFetcher::new()` / `EduUserAction::new()` / `ForumActionHandler::new()` / `BaseWorkOperations::new()` / `CommentOperations::new()` / `WorkshopActionHandler::new()` / `ReportHandler::new()` 等(共 ≈30 处),这些 `.new()` 都退回全局,第六轮新增的 `new_with_client` 在 core 层不可达。
2. **直接全局单例 / 门面**:
    - `retrieve.rs:627` `let client = CodeMaoClient::global();`(`DataQuery::count_comments`)
    - `retrieve.rs:1071` `CodeMaoClient::global().switch_identity(...)`(`stream_edu_accounts_with_reset_passwords`)
    - `pipeline.rs:842` `KittyFactory::global_client().switch_identity(...)`(自动举报流程)
    - `services.rs:75` `KittyFactory::global_client().clone()`(`FileProcessor::handle_file_upload`)

结构现状:

- `retrieve.rs`:`DataQuery` 是**单元结构体** `pub struct DataQuery;`,`CommentQueryBuilder` 无 client 字段(字段 `source/target_id/limit`),二者方法内直接 `XxxFetcher::new()`。
- `registry.rs`:`ReportFetcher` 仅持 `registry: Arc<ReportTypeRegistry>`,`new()` 里用闭包调 `WhaleReportFetcher::new()`。
- `pipeline.rs`:`ViolationChecker` 持 `config` + `network_lock`(无 client),方法内调 `XxxFetcher::new()` / `XxxActionHandler::new()`;自动举报流程调 `KittyFactory`。
- `services.rs`:`ReportProcessor` 持 `fetcher: ReportFetcher`、`violation_checker: ViolationChecker` 等;`FileProcessor` 是单元结构体,静态方法内调 `KittyFactory::global_client()`。

目标:给上述 5 个 core 类型(`DataQuery` / `CommentQueryBuilder` / `ReportFetcher` / `ViolationChecker` / `ReportProcessor` / `FileProcessor`)注入 `CodeMaoClient`,把 ≈30 处 `.new()` 改为 `new_with_client`,消除 4 处全局单例/门面调用,随后 `KittyFactory` 变死代码并删除。原则沿用 `CONTRIBUTING.md`:不引入宏/泛型抽象、`lock().unwrap()`、破坏性变更不留兼容别名。

## Approach

五个阶段按依赖顺序执行(每阶段结束 `cargo check --all-targets` 绿);Phase 5 依赖 1-4 完成。

### Phase 1 — `retrieve.rs`:`DataQuery` + `CommentQueryBuilder` 注入

1. `pub struct DataQuery;` → `pub struct DataQuery { client: CodeMaoClient }`。
2. `impl DataQuery`:
    - `pub fn new() -> Self` → `Self::new_with_client(CodeMaoClient::global().clone())`。
    - 新增 `pub fn new_with_client(client: CodeMaoClient) -> Self { Self { client } }`。
    - `query_comments(&self)`(545 行)返回 `CommentQueryBuilder::new_with_client(self.client.clone())`。
3. `CommentQueryBuilder`(176 行)新增字段 `client: CodeMaoClient`:
    - `pub fn new()` → `Self::new_with_client(CodeMaoClient::global().clone())`。
    - 新增 `pub fn new_with_client(client: CodeMaoClient) -> Self { Self { client, source: None, target_id: None, limit: None } }`(字段名与现有 `new()` 逐一对应,`client` 放首位)。
    - `Default` impl(182 行)保持 `Self::new()`。
4. `count_comments`(626 行):`let client = CodeMaoClient::global();` → `let client = &self.client;`。
5. `stream_edu_accounts_with_reset_passwords`(1068 行):`CodeMaoClient::global().switch_identity(Catsona::Scholar)` → `self.client.switch_identity(Catsona::Scholar)`。
6. 把 `retrieve.rs` 内所有 api-Manager `.new()` 改为 `.new_with_client(self.client.clone())`(穷举,用 `grep -n "::new()" src/core/retrieve.rs` 定位):`WorkDataFetcher`(290/704/709)、`ForumDataFetcher`(296/318)、`WorkshopDataFetcher`(302)、`WhaleReportFetcher`(882/904)、`UserDataFetcher`(979/1015)、`EduDataFetcher`(1078)、`EduUserAction`(1094)、`CommunityDataFetcher`(1117/1201)。共 14 处。

注意:`CommentQueryBuilder` 各方法内部 `.new()` 用 `self.client.clone()`;`DataQuery` 各方法内部用 `self.client.clone()`。`retrieve.rs` 顶部已 `use crate::utils::requests::{..., CodeMaoClient, ...}`,无需新增 use。

### Phase 2 — `registry.rs`:`ReportFetcher` 注入

1. `ReportFetcher`(392 行)新增字段 `client: CodeMaoClient`(置于 `registry` 之后,私有)。
2. `pub(crate) fn new()` → `Self::new_with_client(CodeMaoClient::global().clone())`;新增 `pub(crate) fn new_with_client(client: CodeMaoClient) -> Self`。
3. `new_with_client` 内 `ReportTypeRegistry::new()` 不变;所有配置闭包里 `WhaleReportFetcher::new()` → `WhaleReportFetcher::new_with_client(client.clone())`。共 8 处(413/422/463/472/508/514/549/555)。闭包通过 `let client = client.clone();` 在每次 `|status|` 前捕获(或直接 `move` + 外部 `client.clone()`,实现方自选,但每闭包必须持有独立 `CodeMaoClient`)。
4. `Default for ReportFetcher`(397 行)保持 `Self::new()`。

### Phase 3 — `pipeline.rs`:`ViolationChecker` 注入

1. `ViolationChecker`(541 行)新增字段 `client: CodeMaoClient`。
2. `pub(crate) fn new(config, network_lock)` → `pub(crate) fn new(config: CheckConfig, network_lock: Arc<Mutex<()>>, client: CodeMaoClient) -> Self`;初始化加 `client`。
3. `services.rs` 的调用点同步改(见 Phase 4):`ViolationChecker::new(config.clone(), Arc::clone(&network_lock), client.clone())`。
4. `ViolationChecker` 方法内所有 api-Manager `.new()` → `.new_with_client(self.client.clone())`,穷举(用 `grep -n "::new()" src/core/pipeline.rs` 定位):`ReportHandler`(215)、`ForumDataFetcher`(438/782)、`ForumActionHandler`(1010/1044)、`BaseWorkOperations`(1020)、`CommentOperations`(1034)、`WorkshopActionHandler`(1056)。共 8 处。
5. 自动举报流程(842 行)`KittyFactory::global_client().switch_identity(Catsona::Judge)` → `self.client.switch_identity(Catsona::Judge)`。`self` 此时是 `ViolationChecker`(该函数属 `ViolationChecker` 或持其引用,按实际调用链取 `self.client`;若为自由函数则改签名传入 `&CodeMaoClient`,实现时以 `cargo check` 定位调用链)。

### Phase 4 — `services.rs`:`ReportProcessor` + `FileProcessor` 注入

1. `ReportProcessor`(186 行)新增字段 `client: CodeMaoClient`。
2. 构造链:
    - `pub fn new()` → `Self::new_with_client(CodeMaoClient::global().clone())`。
    - 新增 `pub fn new_with_client(client: CodeMaoClient) -> Self` → `Self::new_with_config_and_client(CheckConfig::default(), client)`。
    - `pub fn new_with_config(config)` 保留(默认全局),新增 `pub fn new_with_config_and_client(config: CheckConfig, client: CodeMaoClient) -> Self`;其内 `ReportFetcher::new()` → `ReportFetcher::new_with_client(client.clone())`,`ViolationChecker::new(...)` → 加 `client.clone()`。
    - `Default` impl(196 行)保持 `Self::new()`。
3. `FileProcessor`(47 行):`pub struct FileProcessor;` → `pub struct FileProcessor { client: CodeMaoClient }`。
    - 新增 `pub fn new() -> Self`(全局默认)与 `pub fn new_with_client(client: CodeMaoClient) -> Self`。
    - `handle_file_upload` / `handle_directory_upload` 从关联函数改为实例方法(`&self`),`handle_directory_upload` 内 `Self::handle_file_upload(...)` → `self.handle_file_upload(...)`。
    - `handle_file_upload` 内 `KittyFactory::global_client().clone()`(75 行)→ `self.client.clone()`。
    - **调用点**:`grep -rn "FileProcessor::" src/` 穷举全部调用点,改为先构造 `FileProcessor::new_with_client(client.clone())` 再调实例方法(或 `FileProcessor::new()` 走全局)。若存在 `terminal.rs`/`main.rs` 调用,同步改。

### Phase 5 — 移除 `KittyFactory` 门面(`requests.rs`)

Phase 1-4 完成后 `grep -rn "KittyFactory" src/` 应只剩 `requests.rs` 的定义与 import。确认零调用点后:

1. 删除 `pub struct KittyFactory;` 及整个 `impl KittyFactory`(1795-1841 行区域,`create_global_client` / `create_independent_client` / `create_file_uploader` / `global_client` / `global_identity_manager`)。
2. 删除 `compiler.rs` / `pipeline.rs` / `services.rs` 顶部残留的 `KittyFactory` import(Phase 1-4 已不引用)。

## 不落地(记录在案)

- **`cloudvar.rs:2209` 自动识别编辑器类型**:`WorkDataFetcher::new().fetch_work_details(...)` 依赖全局 api 客户端。注入需让 `CloudBuilder` 额外持有 `CodeMaoClient`(当前只持 `authorization_token`),牵涉 WS 客户端与 HTTP 客户端的关系,属另一处设计决策,不在本轮。
- **类型化返回(`MewResult<Value>` → DTO)**:api 层 351 处返回 `serde_json::Value`。全量类型化需逐端点核实响应形态(OpenAPI / 真机),是独立的大轮,不在本轮。
- **错误类型收敛**:`DecompilerError` 自带 Io/Json/Http(与 `MewError` 重复)尚未改为包装 `MewError`(`ProcessorError`/`DataQueryError` 已包装)。小改动但独立,不在本轮。
- **`terminal.rs` / `main.rs`**:演示 UI,通过 `ReportProcessor::new()`(全局默认)驱动,无需改。

## Critical files & anchors

| 文件                    | 锚点                                                                                                    | 原因                             |
| ----------------------- | ------------------------------------------------------------------------------------------------------- | -------------------------------- |
| `src/core/retrieve.rs`  | `DataQuery`(537)、`CommentQueryBuilder`(176/252)、`count_comments`(626)、`stream_edu_accounts...`(1068) | Phase 1 落点;14 处 `.new()` 改造 |
| `src/core/registry.rs`  | `ReportFetcher`(392)、`new()`(404)、8 处 `WhaleReportFetcher::new()`                                    | Phase 2 落点                     |
| `src/core/pipeline.rs`  | `ViolationChecker`(541/549)、`KittyFactory`(842)、8 处 `.new()`                                         | Phase 3 落点                     |
| `src/core/services.rs`  | `ReportProcessor`(186/209)、`FileProcessor`(47/75)                                                      | Phase 4 落点                     |
| `src/utils/requests.rs` | `KittyFactory`(1795-1841)                                                                               | Phase 5 删除点                   |

## Verification

前置:每个阶段结束 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全绿(库单测 + `compile_live` + `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. `grep -rn "CodeMaoClient::global()" src/core/` → 仅 `compiler.rs` 的 `CodemaoDecompiler::global()`(有意保留的全局单例门面),`retrieve.rs` 两处归零。
2. `grep -rn "KittyFactory" src/` → 0(整个门面已删)。
3. `grep -rn "::new()" src/core/{retrieve,registry,services,pipeline}.rs` → 命中数显著下降(仅剩 `ReportTypeRegistry::new()` / `BatchActionManager::new()` 等非 api 结构),api-Manager `.new()` 全部变为 `.new_with_client(...)`。
4. `grep -rn "new_with_client" src/core/` → 覆盖 `DataQuery` / `CommentQueryBuilder` / `ReportFetcher` / `ViolationChecker` / `ReportProcessor` / `FileProcessor` 六处定义 + ≈30 处调用。

新行为检查:仿第六轮契约测试,在 `services.rs` 的 `#[cfg(test)]` 加一条 `report_processor_new_with_client_uses_injected_client`(用 `ReportProcessor::new_with_client(CodeMaoClient::new_independent(KittyConfig::default()))`,断言 `processor.client` 身份槽与独立客户端一致、不受全局影响)。若 `ReportProcessor.client` 为私有,则经 `totals()` 触发的网络请求路径无法离线断言,退化为「构造后 `cargo check` 通过 + code review」,测试仅验证构造不 panic。

其余行为(网络语义)以 code review + 编译为准:本轮均为等价重写(把全局默认改为可注入,`new()` 仍走全局),不改动任何端点/参数/请求体。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error。
- `cargo clippy --all-targets` 0 warning(补 `FileProcessor` 的 `Default` impl 消除 `new_without_default`)。
- `cargo test` 全绿:库单测 5 passed(含第六轮 `manager_new_with_client_uses_injected_client` + 第七轮改造后的 `fetch_chunked_terminates_without_duplicates`)、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(真机命中 codemao 服务)、doc-tests 0。
- 归零验证:`grep KittyFactory src/` → 0(门面已删);`grep "client: &'static CodeMaoClient" src/` → 0。

## 范围偏差(实际执行中确定,记录在案)

- **`FetchTotal`/`FetchGenerator` 原为 `fn` 指针**(计划未预见):`ReportFetcher::new` 的闭包因是 `fn` 指针而不可捕获 client。已改为 `Box<dyn Fn + Send + Sync>`;`FetchTotal` 因 `get_totals_pair` 需跨线程共享而改用 `Arc<dyn Fn>`。`SourceConfig` 因此移除 `#[derive(Clone, Debug)]`(仅经 `Arc` 共享,全仓无 `.clone()`/`{:?}` 使用,已确证)。
- **自动举报 / 动作分发 / 展示路径保持全局**(计划未明确,实为全局身份特性,不能部分注入——登录与请求必须同一身份源):
    - `pipeline.rs` 自动举报:`login_student`(经 `LoginBuilder` 写全局身份)、`execute_single_report` 的 `ForumActionHandler::new()`/`BaseWorkOperations::new()`/`CommentOperations::new()`/`WorkshopActionHandler::new()`(1017-1063)、`switch_identity(Judge)`(849)——多账号自动举报本质是切换全局身份槽。
    - `pipeline.rs` `ActionRegistry`(全局 `LazyLock` 单例,215 `ReportHandler::new()`)、`forum_post_content_line`(438,展示助手)。
    - `cloudvar.rs:2209` 自动识别编辑器类型(见「不落地」)。
    - 注入覆盖的是查询/取数路径(`DataQuery`/`CommentQueryBuilder`/`ReportFetcher`/`ViolationChecker` 的查询方法/`ReportProcessor`/`FileProcessor`),这些路径无身份切换,可安全注入。

## Assumptions & contingencies

- **`ViolationChecker` 自动举报流程的 `self` 归属**:842 行 `KittyFactory::global_client().switch_identity(...)` 所在函数可能是 `ViolationChecker` 方法或自由函数。若为自由函数,则把 `&CodeMaoClient` 作为参数传入(改该函数签名并同步其调用点),不引入全局回退。
- **`FileProcessor` 关联函数改实例方法**:属破坏性 API 变更(用户已授权)。若 `grep "FileProcessor::"` 发现外部 crate 无法改的调用点(仓内已确证仅 services.rs 内自用),则退而保留关联函数、改为新增 `client: &CodeMaoClient` 首参——实现时以仓内调用点为准,二选一,不留双签名。
- **`DataQuery` 由单元结构体改为带私有字段结构体**:外部若以 `DataQuery`(单元值)直接构造会破坏,但仓内零此类调用(均经 `new()`);用户已授权破坏性变更,直接改。
- **`KittyFactory` 删除安全性**:Phase 1-4 落地后再次 `grep KittyFactory` 确证零调用点才删(遵守 `CONTRIBUTING.md`「全仓确证零调用点才删」);若仍有遗漏调用点,先补齐注入再删,不阻塞。
