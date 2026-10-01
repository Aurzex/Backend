# 仓库结构/工程待办(清理 · 类型化 · 拆分)

> 目标库:与平台协议无关的**仓库自身**未完成项。约定见 `docs/knowledge/repo-conventions.md`。

## 1. 已在目标清单、等决策

| 项 | 说明 | 出处 |
| -- | ---- | ---- |
| 恢复 `[lints.rust] unused` 告警 | **2026-10-01 试跑结论:一轮清不完,开关维持 `allow`**。临时改 `warn` 后 `cargo clippy --all-targets --message-format short` 共 **943 条**(其中 913 条是 `never used/constructed/read`):`lib` **55** 条、`bin "backend"` **898** 条。898 条来自 `src/main.rs` 是**第二个 crate root**(见下一行),`--all-targets` 会把它们全算进 `-D warnings` ⇒ 门直接红。**前置**是先让 bin 依赖库 crate(下一行),再分批清 `lib` 那 55 条 | `docs/rounds/39` §W5④ |
| `src/main.rs` 作第二个 crate root,**把整棵树重复编译一遍** | `src/main.rs:1-3` 的 `mod api; mod core; mod utils;` 让它把同一份源码再编一遍(编译时间 + 体积),也正是上一条 898 条 dead-code 噪声的根因。**只读结论:改成 `use backend::…` 不需要动公共 API** —— 库根已 `pub mod api/core/prelude/utils`(`lib.rs:3-6`),三个子根也全是 `pub mod`(`core.rs:1-8`、`api.rs:1-13`、`utils.rs:1-3`);bin 用到的项全在公开面里且都是 `pub`:`LoginResult`/`AdminInfo`/`AuthProcessor`/`LoginHandler`(`api/auth.rs:175/214/366/585`)、`ReportProcessor`(`core/services.rs:204`)、`ProcessorUi`/`ConsoleUi`/`ReportConsole`(`core/terminal.rs:5/26/195`)、`PathConfig`(`utils/filedata.rs:17`)(另一个 bin `src/bin/gen_translate_tables.rs` 自带依赖,不受影响)。**只登记,不动代码**:属跨模块改动,须先出方案 | 2026-10-01 实测(`docs/rounds/39` §W5④) |
| api 层类型化 DTO(351 处 `MewResult<Value>`) | 逐端点核对响应形态;建议按域分批 | `docs/rounds/15/16/17` §不落地 |
| newtype ID 推广(`UserId` 等) | 先看 `WorkId` 试点收益 | `docs/rounds/19` §不落地 |
| `work.rs` 再切 `WorkDataFetcher` | 纯搬迁,`re-export` 保路径;按需 | `docs/rounds/19` §不落地 |

## 2. 小改(机械、低风险,可批量做)

1. ~~`DecompilerError` 自带 `Io/Json/Http` 与 `MewError` 重复 ⇒ 改为包装~~ —— **已不成立(2026-10-01 核实)**:`DecompilerError` 已无 `Io`/`Json`/`Http` 变体,`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew`(与 `ProcessorError`/`DataQueryError` 同处置);残留只有死变体 `UnsupportedType`(删它 = 动公共枚举,需授权,见 `pending-decisions.md` C2、`docs/rounds/39` §W5②)。
2. **49 处非锁裸 `unwrap`** 硬化:`auth.rs::time_difference`、`registry.rs` 的 `active.as_mut().unwrap()`、`compiler.rs` 的 `template.unwrap()` 与 10 处 `write!(String).unwrap()` → `let _ = write!(...)`(`docs/rounds/18` Phase 7,P2)。
3. `P2` 收尾:`nemo.rs::get_sha` 每次 `clone()` 64 字节 hex(可返回 `&str`);`auth.rs::AccountStatus` 与 `requests.rs::Identity` 平行枚举合并(易漂移);`MessageHandler`/`ChatEventHandler` 两个 trait 改自由函数;`registry.rs` 错位工具函数归位(`docs/rounds/29` §3-5/§3-6、`docs/rounds/11` P2、`docs/rounds/05` 未执行项)。

## 3. 待核验(证据不足)

1. `services.rs` 是否落了 `report_processor_new_with_client_uses_injected_client` 契约测试(第六轮的"库单测 5 passed"里没看到它)(`docs/rounds/15` §Verification)。
2. `docs/rounds/19` Phase 4 声称的 CONTRIBUTING 三处改动与 `parking_lot` 锁条款校验,在"实际执行结果"段未逐条确认。
3. `docs/rounds/11` §P4-3、`docs/rounds/13` §P3-5/§P3-6 三条(同时列在 `platform-backlog.md` §2)。

## 4. 已决(不做/暂缓)

| 项 | 结论 | 出处 |
| -- | ---- | ---- |
| `core → api` 依赖倒置 | **暂缓**(架构级重构,收益不明确) | `docs/rounds/05` 未执行项 |
| `cloudvar.rs` 深拆 | 不做(需 60+ 处可见性提升 + `Arc<CloudInner>` 贯通,非纯搬迁) | `docs/rounds/18/19` §不落地 |
| `compiler.rs` 切子模块 | 不做(用户指定"两个文件、不放 core 子文件夹") | `docs/rounds/18` §不落地 |
| 公开 `HttpClient` trait / 类型级 WS 状态机 | 不做(前者可经 `CodeMaoClient` 注入;后者维持运行时 `connect_and_wait()`) | `docs/rounds/18/19` §不落地 |
| `MewError`/`MewResult` 改名 | 不做(保留品牌名) | `docs/rounds/19` §不落地 |
| god file 拆分 / 管理器样板宏化 | 不做(用户 11 条回退指令) | `docs/rounds/02/03` |

## 5. 文档类

1. `docs/rounds/01` 的「附录:坑 10 全文」与正文重复,且残留写作指令句 ⇒ 去重或改成链接(该文属历史稿,**按"历史保真"约定不改**;需要时在勘误里注明)。
2. "回调不应 panic"必须写进 rustdoc(`release` 是 `panic="abort"`,catch_unwind 无效)(`docs/rounds/01` 坑 13)。
3. 本库(`docs/knowledge/`、`docs/goals/`)已替代 `docs/rounds/` 作为**入口**;新增改动优先更新本库 + 一个新轮次记录。
