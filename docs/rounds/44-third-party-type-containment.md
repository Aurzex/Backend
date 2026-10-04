# 第四十四轮记录 — 公共面收窄:ureq 类型隔离与错误类型口径统一

## 0. 一句话
把第三方实现类型从公共契约里摘出去,并把三处与既定口径不一致的错误类型收敛为一种:仓库现在能用"下游不声明 ureq"这一判据证明前者,后者则让同一个底层失败在全仓只有一种表示。

## 1. 任务背景
2026-10-03 的指令是修复第 43 轮只读盘点(`../goals/infra-backlog.md` §6)列出的两条公共面问题:公共请求原语泄漏 `ureq` 实现类型、错误类型边界不一致。两条都属破坏性公共面变更;按仓库既有约定(未 1.0、**不留兼容别名**)直接改,不保留旧签名或旧变体。

## 2. ureq 类型不再进公共契约

| 面 | 改动前 | 改动后 |
| --- | --- | --- |
| 请求原语返回值 | `MewRequestBuilder::{send,send_with_payload_ref,send_multipart} -> MewResult<ureq::Response<Body>>` | 前两者返回自有 `requests.rs::MewResponse`;`send_multipart` 收 `pub(crate)`(其入参是 ureq 的表单类型,仓内只被 `FileUploader` 用) |
| 响应读取助手 | `CodeMaoClient::response_to_{json,string,binary,*_large}(ureq::Response<Body>, …)` | 同上五个助手改收 `MewResponse`;`MewResponse` 对外只给 `status()` 与 `header(name)` |
| 传输层错误 | `MewError::Http(#[from] ureq::Error)` | `MewError::Http(TransportError)`;`TransportError` 只给 `message()` 与 `is_timeout()` |
| 底层 Agent | `CodeMaoClient::agent() -> &ureq::Agent`(`pub`) | 收 `pub(crate)` 并标 `#[cfg(test)]`(实际只有测试在用) |

`ureq` 现在只出现在:私有字段 `MewResponse::inner`、`pub(crate) TransportError::from_ureq` / `MewError::transport`(唯一的转换点)、以及私有辅助的签名里。**没有任何 `pub` 签名或公开字段要求下游命名 ureq 的类型**。

顺带:`api/auth.rs` 有三处直接 `response.into_body().read_to_string()/read_to_vec()`;它们原先依赖 `From<ureq::Error>`,隔离后本就不成立,已改走 `client.response_to_string` / `response_to_binary`(顺带把"读取失败"的上下文统一到同一条错误路径)。

## 3. 错误类型口径统一

口径锚点取自转换域既有的 `translate/options.rs::TranslateError` 与 `convert/shared.rs::DecompilerError`:**底层 `io` / `serde_json` 失败一律折进 `MewError`**,域错误类型只留域内变体 + 一个 `Mew` 载体。

| 类型 | 改动前 | 改动后 |
| --- | --- | --- |
| `core/registry.rs::ProcessorError` | `Processing` / `Io` / `Json` / `Mew` / `Aborted` | `Processing` / `Mew` / `Aborted`;补 `From<io::Error>` 与 `From<serde_json::Error>`(均折进 `Mew`),因此既有 `?` 调用点零改动 |
| `core/retrieve.rs::DataQueryError` | `InvalidSource` / `ParseError` / `Json` / `External(MewError)` | `InvalidSource` / `ParseError` / `Mew(MewError)`(删 `Json`、`External` 改名以与 `ProcessorError` 同名同义) |
| `utils/filedata.rs::FileError` | 只含 `Io` 的单变体,与外层 `MewError::Io` 重复 | **整型删除**;`CodeMaoFile::write_bytes` 返回 `MewResult<()>`(`api/auth.rs` 的调用点保留"验证码文件写入失败"上下文) |

## 4. 验收:外部消费 crate(不声明 ureq)

按盘点给出的判据建了一个只依赖本库的临时 crate(`temp/extcheck`,跑完已删),它**不声明 ureq、也不直接调用 ureq**,做三件事:

1. `build_request(...).send()?` 拿到 `MewResponse` 并读 `status()` / `header("date")`;
2. 用 `CodeMaoClient::response_to_json(response)` 解出响应体;
3. 匹配 `MewError::Http(err)` 并用 `err.is_timeout()`,同时调用 `CodeMaoFile::write_bytes`。

结果:编译通过并**真机跑通**,输出 `OK status=200 has_date_header=true body={"code":200,"data":…}` 与 `write_bytes -> true`。即:下游不再需要知道 ureq 存在;ureq 的大版本升级也不再自动构成本库的破坏性变更。

## 5. 边界与未做
- `TransportError` 只做"文本 + 是否超时"两件事,不镜像 `ureq::Error` 的全部分类(仓内没有任何调用方按更细分类分支)。将来确有需要时再按调用方需求扩,不放着备而不用。
- `MewResponse` 不提供直接读响应体的方法:响应体一律经五个 `response_to_*` 助手读取(那里才有 10 MB / 256 MB 护栏与 `MewError::ResponseTooLarge` 的定位信息)。
- 盘点里另外两条公共面项(`CheckConfig` 悬空面、`api` 层两处零调用全局入口)本轮未动,仍在 `../goals/infra-backlog.md` §6 待决。

## 6. 验证

| 项 | 读数 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过,零告警 |
| `cargo test` | 退出码 0:14 个目标 **144 通过 / 0 失败 / 11 忽略**;真机目标 `compile_live` 1、`convert_live` 2、`live_features` 3、`repo_hygiene` 2 全通过 |
| 外部消费 crate | 不声明 ureq 即编译通过并真机跑通(读数见 §4) |
| 残留检查 | `ureq::` / `Response<Body>` 只出现在私有或 `pub(crate)` 位置(逐处核对) |

## 依据
- 代码提交:`5e602dd`(`refactor(public-api)!`:第三方类型不进公共契约 + 错误类型口径统一)。
- 盘点来源:`../goals/infra-backlog.md` §6 第 1 条与第 4 条(2026-10-03 只读盘点)。
- 口径锚点:`src/core/convert/translate/options.rs::TranslateError`、`src/core/convert/shared.rs::DecompilerError`;耐久约定写入 `../knowledge/repo-conventions.md` §4。
- 历史形态变更登记:`../knowledge/errata.md`(2026-10-03 条:`MewError::Http` 的载荷与 `FileError` 的存废)。
