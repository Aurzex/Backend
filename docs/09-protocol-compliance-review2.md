# 六协议复查 — 第二轮扫描与整改记录（已执行）

审阅日期:2026-08-11 · 基线:HEAD `6254d24`(首轮协议整改已提交)· 范围:六协议全仓复查

## Context

首轮六协议整改(6254d24)后,用户要求继续检查同类问题。本轮全仓复查六个协议维度,发现 1 处需整改,其余判定合规并记录理由。

## 整改项

### PaginatedIter::next_item 降私有（分页协议）

- `src/utils/acquire.rs:1314`:`pub fn next_item(&mut self) -> Option<MewResult<Value>>` 是 `Iterator` 实现细节(内部 1395/1457 调用),对外无任何调用点。
- 协议 3"用户统一用 for 循环遍历,不暴露额外入口"——公开 `next_item` 诱导绕过 `Iterator::next`。
- 整改:`pub fn next_item` → `fn next_item`(私有),`Iterator` 实现不变。
- 验证:`cargo check --all-targets` 通过,`cargo test` 3 passed,`cargo clippy --all-targets` 0 warning;`grep "next_item" src/` 仅剩内部 3 处引用。

## 判定合规项（记录理由）

| 项目 | 位置 | 判定理由 |
|---|---|---|
| `KittyRequestBuilder::send/send_with_payload_ref/send_multipart` | acquire.rs:559-574 | 请求构造器的**执行步骤**(构造 + send 分离),是 ClientAccess 默认方法/PaginatedIter/FileUploader/compiler 内部共用的发送原语;非"业务配置 Builder 直接发网络"(协议 1 针对 LoginBuilder 类) |
| `wait_for_connection/wait_for_data/wait_for_response*` | cloudvar/converse | 等待原语(内部/高级用),非 `_and_wait` 同步方法;协议 5 只管 `_and_wait` 后缀方法 |
| `CommentQueryBuilder::stream_*` | retrieve.rs:345-451 | Builder 构造无副作用,`stream_*` 是**惰性执行方法**(返回迭代器,next 时才拉取);构造/执行分离合规 |
| `CheckConfig`/`KittyConfig`/`PaginationConfig` | pipeline/acquire | 内部配置结构体(`ReportProcessor::new_with_config` 等),非"Options 结构体传给自由函数" |
| `DecompileOptions` 传自由函数 | compiler.rs | 本身是 Builder 链式(output_dir/save_raw/batch_concurrency),符合"配置进 Builder"精神(首轮已判定) |
| main.rs 调用形态 | main.rs | 全部 `Xxx::new().method()`(构造即实例,网络由实例方法触发),无 Builder 直接网络 |
| `done_chunks` 返回 Box<dyn Iterator> | services.rs:639 | 已完成块的历史查询收尾 API,非分页数据源,terminal 内部使用 |
| 全部 `fetch_*_gen` 返回 PaginatedIter | api 层 | 均实现 `Iterator<Item = MewResult<Value>>`,for 循环遍历合规 |
| 回调注册 | 全仓 | 首轮整改后无 add_/register_ 前缀回调(全部 on_ 前缀) |
| `check_connection_health(max_inactivity: Duration)` | cloudvar.rs:919 | 健康检查方法(非 _and_wait),超时参数是检查阈值非同步等待超时,合规 |

## Verification（实际执行结果）

- `cargo check --all-targets` 通过;`cargo test` 3 passed;`cargo clippy --all-targets` 0 warning。
- 六协议维度复查均达标:
  1. 构造:无 Builder 直接网络(LoginSession 已分离)
  2. 执行:网络均由实例方法触发,返回 Result
  3. 分页:next_item 私有化后仅 Iterator 入口
  4. 回调:全 on_ 前缀
  5. 等待:_and_wait 无 Duration 参数(超时在 Builder)
  6. 配置:无独立 Options 结构体传自由函数
