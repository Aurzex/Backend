# Contributing(喵)

## 开发环境

- Rust stable(edition 2024),一只即可
- 构建和单测不需要外部服务;实际运行需要能访问编程猫(codemao)服务 + 有效账号

## 常用命令

```bash
cargo check --all-targets   # 编译检查(含测试目标)
cargo test                  # 跑单测 + 集成测试(没配置时集成测试自动跳过)
cargo clippy --all-targets  # 静态检查,提交前不应新增警告
```

集成测试需要本地配置(详见 `README.md`「测试」一节):

```bash
cp tests/fixtures/test-config.example.json data/test-config.json
cargo test --test live_features
```

## 编码约定

以简洁、可读为最高原则,避免过早优化。代码可以喵,但不要让别的猫看不懂。

- **不过度抽象**:不引入 trait/泛型/多层抽象,除非能显著减少重复且不损害可读性;不引入宏,除非同样标准。
- **命名**:基础设施层沿用「萌化」命名约定(已在 `utils/requests.rs` 落地):身份 `Catsona`、身份管理器 `KittyIdentityManager`、认证 `KittyAuth`、请求构建器 `KittyRequestBuilder` 等;业务域命名保持直白(如 `CaptchaManager`、`ReportProcessor`)。新代码要和所在模块既有命名风格一致。
- **锁**:使用标准库 `std::sync::{Mutex, RwLock, Condvar}` 与 `lock().unwrap()` 风格,不引入额外锁依赖。
- **删除代码**:不主动删除,除非已用 `lsp references`(或 `grep`)全仓确证零调用点(死代码)。`Cargo.toml` 已配置 `unused = "allow"`,死代码不阻塞编译,但应避免累积。
- **请求样板**:优先复用 `utils/requests.rs` 的 `ClientAccess` 默认方法(`send_and_parse` / `check_status` / `send_maybe_parse`),不要手写 `send() + response_to_json`。这些方法已统一携带服务端错误响应体,错误路径可读。

- **客户端注入**:业务 Manager / 反编译器 / 举报引擎 / 登录构建器等类型提供 `new()`(全局默认,委托 `new_with_client(CodeMaoClient::global().clone())`)与 `new_with_client(client: CodeMaoClient)`;不硬编码 `CodeMaoClient::global()`。例外:自动举报的多账号登录是全局身份槽特性,`login_student` 登录与身份切换保持全局。
- **日志**:构造日志字符串(尤其 pretty-print)前先加 `log_enabled!(log::Level::Debug)` 守卫,避免 Info 级别下无谓分配。
- **错误**:用 `thiserror` 枚举;包装错误时保留底层变体(如 `Http` / `Json` / `Io`),不要全部压成 `Auth(String)`。

- **`Result` 别名**:模块内若定义 `type Result<T>` 别名,必须暴露默认错误参数——`type Result<T, E = XxxError> = std::result::Result<T, E>`——让 `?` 透传的同时保留精确错误的逃生口。
- **错误模型分层**:传输层 / 通用错误归 `MewError`(`Http` / `Io` / `Json` / `HttpStatus`),WS 归 `SocketError`;业务域错误包装 `MewError`(如 `DecompilerError::Mew(#[from] MewError)`),不重复 Io/Json/Http 变体。
- **注释与提交信息**:使用中文(喵语自由,但要让别的猫看懂)。

## 提交规范

采用 conventional commits 风格,历史示例:`feat:` / `fix:` / `refactor:` / `docs:`。

提交前必须:

1. `cargo check --all-targets` 无 error
2. `cargo test` 通过
3. `cargo clippy --all-targets` 不新增警告

## 工作流建议

- 跨文件改动先明确影响面:函数签名变更前用 `lsp references`(或 `grep`)列出全部调用点。
- 评审与整改记录放 `docs/`,命名沿用编号前缀(如 `docs/02-review-round1.md`、`docs/05-style-unify-plan.md`)。
