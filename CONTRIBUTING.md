# Contributing

## 开发环境

- Rust stable(edition 2024)
- 无需外部服务即可完成构建与测试;实际运行需要能访问编程猫(codemao)服务与有效账号

## 常用命令

```bash
cargo check --all-targets   # 编译检查(含测试目标)
cargo test                  # 运行测试
cargo clippy --all-targets  # 静态检查,提交前不应新增警告
```

## 编码约定

以简洁、可读为最高原则,避免过早优化。

- **不过度抽象**:不引入 trait/泛型/多层抽象,除非能显著减少重复且不损害可读性;不引入宏,除非同样标准。
- **锁**:使用标准库 `std::sync::{Mutex, RwLock, Condvar}` 与 `lock().unwrap()` 风格,不引入额外锁依赖。
- **删除代码**:不主动删除,除非已用 `grep` 全仓确证零调用点(死代码)。
- **请求样板**:优先复用 `utils/acquire.rs` 的 `ClientAccess` 默认方法(`send_and_parse` / `check_status` / `send_maybe_parse`),不要手写 `send() + response_to_json`。这些方法已统一携带服务端错误响应体,错误路径可读。
- **日志**:构造日志字符串(尤其 pretty-print)前先加 `log_enabled!(log::Level::Debug)` 守卫,避免 Info 级别下无谓分配。
- **错误**:用 `thiserror` 枚举;包装错误时保留底层变体(如 `Http` / `Json` / `Io`),不要全部压成 `Auth(String)`。
- **注释与提交信息**:使用中文。

## 提交规范

采用 conventional commits 风格,历史示例:`feat:` / `fix:` / `refactor:` / `docs:`。

提交前必须:

1. `cargo check --all-targets` 无 error
2. `cargo test` 通过
3. `cargo clippy --all-targets` 不新增警告

## 工作流建议

- 跨文件改动先明确影响面(函数签名变更前用 `lsp references` 或 `grep` 列出全部调用点)。
- 评审与整改记录放 `docs/`,例如 `docs/02-review-round1.md`、`docs/04-review-round2.md`、`docs/05-style-unify-plan.md`。
