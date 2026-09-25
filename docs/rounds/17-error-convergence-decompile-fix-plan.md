# 第九轮评审 — 错误模型收敛 + 反编译返回语义修正

审阅日期:2026-08-29 · 基线:HEAD `5d76687` · 范围:`src/utils/requests.rs` + `src/core/compiler.rs` + `README.md` + `tests/{compile_live,live_features}.rs`

> 方案先行(本文档),随后落地代码。前八轮已打通客户端注入(api 层 + 反编译器 + 举报引擎 + 登录/动作分发)与部分错误合并(CloudError/ChatError→SocketError)。本轮收敛剩余错误模型并修正反编译返回语义。**延续允许破坏性 pub API 变更**。

## Context

剩余可优化点中,本轮聚焦三处确定性缺陷(其余如 `compiler.rs` 4115 行分解、Manager 命名统一、剩余 `LazyLock` 全局单例、类型化 DTO 归入后续):

1. **`MewError` 丢失结构化 HTTP 错误**:`send_checked` 把 4xx/5xx 压成 `MewError::Other(format!("HTTP {status}: {body}"))`,调用方无法用 `status`/`body` 分支处理,只能 `to_string()` 匹配。
2. **`DecompilerError` 重复传输层变体**:自带 `Io`/`Json`/`Http(String)` 与 `MewError` 的 `Io`/`Json`/`Http` 重复(已 grep 确证 `DecompilerError::Io`/`::Json` 从不显式构造,只经 `?`;`Http(String)` 仅在 `CodeMaoHttpClient` 的 get_json/get_binary/get_text 6 处 `map_err` 出现)。
3. **`decompile_work` 返回语义与文档矛盾**:doc 注释(4102)与 README 示例 5 声称「`output_dir` 传 `None` 表示不落盘,仅返回 JSON 字符串」,但 `decompile_inner`(4028)对 `None` 回退 `default_output_dir` 并总是 `save_result` 写盘返回**文件路径**——「返回 JSON 字符串」模式从未实现,返回类型 `Result<String>` 实际恒为路径。

目标:给 `MewError` 加结构化 HTTP 状态变体;让 `DecompilerError` 包装 `MewError`(消除传输层重复);把 `decompile_work*` 返回类型改为 `PathBuf` 并修正文档。原则沿用 `CONTRIBUTING.md`(thiserror 保留底层变体、简洁可读、不引入新依赖)。

## Approach

三个阶段相互独立,按顺序执行(每阶段结束 `cargo check --all-targets` 绿)。

### Phase 1 — `MewError` 结构化 HTTP 错误(`requests.rs`)

1. `MewError`(19-32 行区域)新增变体(置于 `Http` 之后):

```rust
/// 服务端返回 4xx/5xx 时的结构化错误(状态码 + 响应体)
#[error("HTTP {status}: {body}")]
HttpStatus { status: u16, body: String },
```

2. `send_checked`(≈1947 行)把 `return Err(MewError::Other(format!("HTTP {status}: {body}")));` 改为 `return Err(MewError::HttpStatus { status, body });`(`status` 已是 `u16`,直接搬入)。

其余不变:`MewError::Http(#[from] ureq::Error)` 仍是传输层错误,`Auth(String)`/`Other(String)` 保持(非本轮范围)。

### Phase 2 — `DecompilerError` 包装 `MewError`(`compiler.rs`)

1. `DecompilerError`(24-49 行)删除 `Io`(26)、`Json`(28)、`Http(String)`(30)三个变体,新增:

```rust
#[error("外部错误: {0}")]
Mew(#[from] MewError),
```

2. 保留 `Crypto`/`Decompile`/`UnsupportedType`/`InvalidResponse`/`MissingField`/`TypeMismatch`/`Other`(反编译专属变体)。

3. 为保住既有 `?` 链(文件内大量 `io::Error`/`serde_json::Error` 经 `?` 冒泡),新增两条 `From` 透传(enum 定义后):

```rust
impl From<std::io::Error> for DecompilerError {
    fn from(e: std::io::Error) -> Self {
        DecompilerError::Mew(e.into())
    }
}

impl From<serde_json::Error> for DecompilerError {
    fn from(e: serde_json::Error) -> Self {
        DecompilerError::Mew(e.into())
    }
}
```

4. `CodeMaoHttpClient`(3719 行区域)的 6 处 `map_err(|e| DecompilerError::Http(...))`(3733/3736/3744/3747/3755/3758)改为直接 `?`:`self.client.build_request(...).send()?` 与 `self.client.response_to_json(response)?` 等。顶部补 `use crate::utils::requests::{CodeMaoClient, HttpMethod, MewError};`(当前只有 `CodeMaoClient, HttpMethod`)。

验证此步无残留:`grep -n "DecompilerError::Http\|DecompilerError::Io\|DecompilerError::Json" src/core/compiler.rs` → 0。

### Phase 3 — 反编译返回类型 `String` → `PathBuf` + 文档修正(`compiler.rs` + `README.md` + tests)

1. `WorkDecompiler::save_result` trait(1544-1549)返回 `Result<String>` → `Result<PathBuf>`。
2. `save_json_result`(1553)与 `save_path_result`(1581)返回 `Result<String>` → `Result<PathBuf>`;`save_json_result` 内 `Ok(filename)` 改为 `Ok(output_path.join(filename))`;`save_path_result` 内 `Ok(path.clone())` 改为 `Ok(PathBuf::from(path))`。
3. 7 个反编译器 impl 的 `save_result` 签名与 3 处 `save_json_result`/`save_path_result` 调用(1676/2473/2569/2833/3182)同步返回 `PathBuf`。
4. `decompile_inner`(4004)、`decompile`(3946)、`decompile_with_options`(3955)、`decompile_batch`(3964)与 3 个自由函数 `decompile_work`(4103)/`decompile_work_with`(4108)/`decompile_works`(4113)返回 `Result<String>`/`Vec<Result<String>>` → `Result<PathBuf>`/`Vec<Result<PathBuf>>`。
5. 修正文档:删掉 4102 行「`output_dir` 传 `None` 表示不落盘,仅返回 JSON 字符串」,改为「`output_dir` 传 `None` 时写入 `default_output_dir`,返回产物文件路径」;README 示例 5 的「None = 不写文件,只返回 JSON 字符串」改为「None = 写入默认输出目录,返回文件路径」。
6. `tests/compile_live.rs` 与 `tests/live_features.rs` 的 `Path::new(&saved).exists()`(`saved` 由 `String` 变 `PathBuf`)改为 `saved.exists()`(若 `Path::new(&saved)` 经 deref 仍可编译则不必改,以 `cargo test --test compile_live` 报错为准)。

## Critical files & anchors

| 文件                                               | 锚点                                                                                                                                                                                          | 原因             |
| -------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------- |
| `src/utils/requests.rs`                            | `MewError`(19-32)、`send_checked`(≈1947)                                                                                                                                                      | Phase 1 落点     |
| `src/core/compiler.rs`                             | `DecompilerError`(24-49)、`CodeMaoHttpClient`(3719-3760)、`WorkDecompiler::save_result`(1544)、`save_json_result`/`save_path_result`(1553/1581)、`decompile_inner`(4004)、自由函数(4103-4113) | Phase 2/3 落点   |
| `tests/compile_live.rs` / `tests/live_features.rs` | 125 / 204 行的 `Path::new(&saved).exists()`                                                                                                                                                   | Phase 3 同步点   |
| `README.md`                                        | 示例 5(≈151 行)                                                                                                                                                                               | Phase 3 文档修正 |

## Verification

前置:每阶段结束 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全绿(库单测 + `compile_live` + `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. **Phase 1**:`grep -n "HttpStatus" src/utils/requests.rs` 命中 enum 变体 + `send_checked` 两处;`grep -n 'MewError::Other(format!("HTTP' src/` → 0。
2. **Phase 2**:`grep -n "DecompilerError::Io\|DecompilerError::Json\|DecompilerError::Http" src/core/compiler.rs` → 0;`grep -n "DecompilerError::Mew" src/core/compiler.rs` 命中 `From<io::Error>`/`From<serde_json::Error>` 两处 + enum 变体。
3. **Phase 3**:`grep -n "-> Result<String>" src/core/compiler.rs` → 0(`decompile*`/`save_*` 均 `Result<PathBuf>`);`grep -n "仅返回 JSON 字符串\|不写文件" README.md src/core/compiler.rs` → 0。

新行为检查:Phase 3 后 `decompile_work(work_id, None)` 返回 `PathBuf`,可 `assert!(path.exists())` 直接校验;集成测试 `cargo test --test compile_live`(反编译真机)与 `cargo test --test live_features` 的 `decompile_works` 用例即为端到端证明(反编译产物文件存在)。

其余行为以 code review + 编译为准:Phase 1/2 为等价重写(错误结构更精确),Phase 3 为返回类型更精确 + 文档修正,不改动任何端点/请求体/落盘行为。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error。
- `cargo clippy --all-targets` 0 warning。
- `cargo test` 全绿:库单测 5 passed、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(真机命中 codemao 服务)、doc-tests 0。
- 归零验证:`DecompilerError::Io\|Json\|Http` → 0;`仅返回 JSON 字符串\|不写文件` → 0;`-> Result<String>` 仅剩 `write_blocks`(2094)、`NemoDecompiler::decompile_inner`(2533)、`WoodDecompiler::decompile_inner`(2802)、`get_text`(3712/3751)等非 decompile 路径(合法保留);`decompile*`/`save_*` 均 `Result<PathBuf>`(19 处)。

## 范围偏差(实际执行中确定,记录在案)

- **`response.status()` 实际返回 `http::StatusCode`,非 `u16`**(计划假设错误):`MewError::HttpStatus.status` 仍存 `u16`,`send_checked` 用 `status.as_u16()` 转换(该类型有 `is_client_error()`/`Display`/`PartialEq<u16>`)。
- **测试文件 `PathBuf` 适配**:`tests/{compile_live,live_features}.rs` 的 `decompile_ok` 返回 `String`→`PathBuf`,`Path::new(&saved).exists()`→`saved.exists()`,断言/日志消息 `{}`→`saved.display()`;`compile_live.rs` NEMO 用例的 `eprintln!` 同样改 `.display()`。

## Assumptions & contingencies

- **`MewError::HttpStatus.status` 用 `u16`**:`ureq` 的 `response.status()` 返回 `u16`,直接存 `u16`。若实现时发现 `is_client_error`/`is_server_error` 依赖非标准方法,以 `cargo check` 定位并沿用现有判定写法。
- **`save_path_result` 的 `path.clone()` → `PathBuf`**:`DecompileResult::Path(path)` 的 `path` 是 `String`,`Ok(PathBuf::from(path))` 语义不变。
- **Phase 3 的 `Path::new(&saved)` 兼容性**:若 `saved: PathBuf` 下 `Path::new(&saved)` 实际可编译(靠 deref),则测试文件无需改;否则改 `saved.exists()`。实现时以 `cargo test --test compile_live` 报错为准,二选一。
- **`DecompilerError::Mew` 命名**:与 `ProcessorError::Mew`/`DataQueryError::External` 对齐;默认 `Mew`,不引入 `External` 别名。
