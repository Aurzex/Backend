# 基础设施 / 核心引擎优化方案(第四轮评审之后)

审阅日期:2026-08-13 · 范围:`utils/requests.rs` + `core/{services,pipeline,retrieve,compiler}.rs` · 基线:当前 HEAD

> 本轮分两阶段:**先写方案(本文档),再落地代码**。cloudvar/converse(已第三轮评审)、13 个薄封装 api 域、`terminal.rs`(演示 UI)不在本轮范围。

## Context

按 5 个维度(逻辑错误 / 性能 / 更优实现 / 性能优化 / 设计模式)评审库代码后,筛选出**可落地、低风险、行为等价**的改动项。原则:简洁可读优先、避免过早优化、不引入新依赖、不改破坏性 pub API 签名。

已确认无高危逻辑缺陷(无确定 panic、无错误结果)。落地的改动聚焦:一处锁争用、一处死分支、几处常量项分配与 idiomatic 收尾。

## 落地项(按优先级)

### P1-1 `ReportProcessor::totals()` 锁外取数(services.rs:227)

**问题**:`totals_cache` 互斥锁在 `get_totals_pair`(并发发起 2×N 个 HTTP 请求 + `thread::scope` join)整个网络往返期间被独占;`apply_action` 等触发的 `invalidate_totals()` 会被阻塞。

**改动**:先短锁查缓存,命中直接返回;未命中则**释放锁**取数,取回后再加锁写缓存。行为等价,消除锁争用。

### P1-2 `ClientAccess` 三个默认方法抽 `send_checked`(requests.rs:1889)

**问题**:`check_status` / `send_and_parse` / `send_maybe_parse` 各自重复「send → 4xx/5xx 检查 → read_to_string → 格式化错误」约 7 行。

**改动**:新增模块级私有函数 `fn send_checked(builder) -> MewResult<Response<Body>>`(builder 自带 client,无需 client 参数),三个方法只保留差异部分。净删约 15 行,错误文案单一来源。

### P2-1 `CryptoService::sha256` 十六进制化(compiler.rs:762)

**问题**:`result.iter().map(|b| format!("{:02x}", b)).collect()` 每字节一次 `format!`(32 次分配)。

**改动**:预分配 `String::with_capacity(64)`,用 `std::fmt::Write` 的 `write!(out, "{b:02x}")` 单次写入。

### P2-2 `ensure_account_login` 死分支清理(pipeline.rs:895)

**问题**:`if idx < *current_idx` 恒为 false(`select_report_account` 返回前已 `*current_idx = idx`),是死分支;实际行为靠末尾 `% accounts.len().max(1)` 碰巧正确。

**改动**:删除死分支,显式处理「账号清空归零 / 否则取模」,并注释说明 remove 后 `current_idx` 指向补位账号即"前进到下一个"的语义。行为不变。

### P3 idiom 收尾(机械、低风险)

| #    | 项                                                          | 位置                | 改动                                                                                           |
| ---- | ----------------------------------------------------------- | ------------------- | ---------------------------------------------------------------------------------------------- |
| P3-1 | `(limit + 1) / 2` 溢出                                      | retrieve.rs:679     | 改 `limit / 2 + limit % 2`(有符号整数 `div_ceil` 在本工具链仍 unstable,`int_roundings` 未稳定) |
| P3-2 | `starts_with("http")` 误判                                  | requests.rs:642     | 改 `starts_with("http://") \|\| starts_with("https://")`                                       |
| P3-3 | `base64_to_bytes` / `reverse_string` 拿 `&self` 不读 `self` | compiler.rs:769/775 | 改关联函数(去 `&self`),唯一调用点 `decrypt_bcmkn` 同步改 `Self::`                              |
| P3-4 | `generate_random_id` 的 `u8 as char` footgun                | requests.rs:1686    | 仅补 doc 注释「仅支持单字节 ASCII 字符集」,不改 pub 签名(3 处调用均传 `b"..."`)                |

## 不落地(记录在案)

- `map_comments_chunked` 每项一线程 + 中间分配(retrieve.rs:200):行为正确,线程粒度优化需权衡,收益有限。
- `auth_header()` 每请求 `format!`(requests.rs:335):热路径小分配,避免过早优化。
- `IdGenerator`/`CryptoService` `Clone` 语义(compiler.rs):62 元素拷贝可忽略。
- `switch_identity` TOCTOU、`unwrap_or(usize::MAX)` 哨兵:边界/契约项,维持「实测前不改」立场。
- `KittyFactory` 冗余门面(requests.rs:1785):被多处引用,非死代码,按约定不主动删。

## Verification

1. `cargo check --all-targets` 0 error。
2. `cargo clippy --all-targets` 不新增警告。
3. `cargo test` 全绿(库单测:AdminInfo、分块迭代器终止性;集成测试未配置时自动跳过)。
4. 行为等价性以 code review 为准:改动均为「等价重写」或「删死分支」,不改变对外契约。
