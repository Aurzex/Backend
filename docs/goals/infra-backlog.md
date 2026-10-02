# 仓库结构/工程待办(清理 · 类型化 · 拆分)

> 目标库:与平台协议无关的**仓库自身**未完成项。约定见 `docs/knowledge/repo-conventions.md`。

## 1. 已在目标清单、等决策

| 项 | 说明 | 出处 |
| -- | ---- | ---- |
| ~~恢复 `[lints.rust] unused` 告警~~ | ✅ **已完成(2026-10-02,rounds/40 R2,三阶段 `6414b97` 机械族 / `30216c5` `dead_code` / `8da596d` 收口)**:终态 `unused = "warn"`,三选择(`--lib`/`--bins`/`--tests`)诊断 **0/0/0**,四道门全绿。阶段 1 清机械族 19 条、阶段 2 处置 `dead_code` 31 条(删 13 / `#[cfg(test)]` 10 / `#[allow]`+理由 8)。两条坑记下来:**组的 `level` 要配 `priority = -1`**(否则 clippy `lint_groups_priority`(deny 默认)直接报错)、**显式 `warn` 挡不住 `-D warnings`**。口径与逐条处置见 §1.1,登记待决项见 §1.1 末 | R2;`README.md` 第 40 轮目标表 R2 行 |
| ~~CI 产物名列与 `crate-type` 不符(⇒ job 静默绿、产物其实没上传)~~ | ✅ **已决并落地(2026-10-02,`f68c2e6`)**:走"改 CI"这一边 —— **删掉 artifact 上传步**及矩阵里 5 组 `artifact:`/`libname:` 键,理由:本仓 `[lib] crate-type=["rlib"]` 只产 rlib、`src/main.rs` 是需账号的**交互式控制台**、仓内没有消费这些 artifact 的地方 ⇒ **没有可分发的产物**(不恢复 `cdylib`)。同时新增 `offline-gate` job(fmt/clippy/逐目标点名的离线测试)。旧症状与根因线索:期待 `libbackend.so`/`backend.dll`/`libbackend.dylib` 而实际不产,`if-no-files-found` 默认 `warn` ⇒ 一直静默绿 | `f68c2e6`;口径见 `repo-conventions` §6 与 `errata.md` 末节 |
| ~~`src/main.rs` 作第二个 crate root,把整棵树重复编译一遍~~ | **已修(2026-10-02,`47a8c5e`)**:`src/main.rs` 开头的 `mod api; mod core; mod utils;` 已删,4 条 `use crate::…`(`:8-11`)改 `use backend::…`,**公共 API 零改动**(bin 用到的 11 个符号逐条核对皆 `pub`,与下方只读结论一致)。判据:bin 单元 dep-info 输入 **49 → 1**(`src/main.rs`;lib 单元仍覆盖整棵树);bin 单测目标 **121 tests / 214 s → 0 tests / 0.00 s**;`unused = "warn"` 下 bin 侧 **896 → 0**。`Cargo.toml` 未动(bin 仍由 `src/main.rs` 自动发现) | `47a8c5e` |
| api 层类型化 DTO(351 处 `MewResult<Value>`) | 逐端点核对响应形态;建议按域分批 | `docs/rounds/15/16/17` §不落地 |
| newtype ID 推广(`UserId` 等) | 先看 `WorkId` 试点收益 | `docs/rounds/19` §不落地 |
| `work.rs` 再切 `WorkDataFetcher` | 纯搬迁,`re-export` 保路径;按需 | `docs/rounds/19` §不落地 |

### 1.1 `unused` 落地记录(rounds/40 R2:基线 → 三阶段 → **0 条**)

> **口径**(全程同一把尺子,可复现):临时把 `unused` 改 `warn`,**按目标选择分别跑** `cargo clippy --lib` / `--bins` / `--tests --message-format=json`,按 `target.src_path` 归属。**不能按 `target.name`** —— lib / bin:`backend` / bin:gen 全叫 `backend`;`--all-targets` 也不行:既混单元又去重跨单元诊断(旧树(`47a8c5e` 之前)实测:`--all-targets` 1447 < 三个选择之和 1547 = 946+551+50;R2 开工后的同口径是 50+50+98)。旧的"`--message-format short` 一次跑完"口径不可复现(short 不带 target 信息,无法按目标归属)⇒ 本文只用"按目标选择分别跑"这一套(即下文的"两棵树"读数)。

- **读数(⚠️ 分两棵树,别混)**:**终态**(`8da596d`,`--lib`/`--bins`/`--tests`)= **0 / 0 / 0**。
  - **R2 开工时**(`47a8c5e` 已落地、`src/main.rs` 已是 136 行薄壳):`--lib` **50**、`--bins` **50**(全部来自被顺带重编的 **lib 依赖单元**,bin 自身 **0**)、`--tests` **98**(全在 lib 的 test 单元)—— **§1.1 的清单就是这 50 条**。
  - ⚠️ **`47a8c5e` 之前**(旧 `src/main.rs` 还把 `mod api/core/utils` 再编一遍)的同口径旧读数:`--lib` 50、`--bins` **946**(其中 `src/main.rs` **896**)、`--tests` **551**(其中 `src/main.rs` 453)。**这批 bin 侧噪声是那次重写清掉的、不是 R2 清的**,也无法在当前树复现(现行 `--bins` = **0**)——`rounds/39` §0.3 W5 行的"`896 → 0`"指的就是它。
- **阶段 1(机械族,`6414b97`)**:lib 单元 **19 条** —— `unused_imports` 13、`unused_variables` 2、`unused_mut` 2、`unused_assignments` 1(`decompile/editors.rs:179 restore_groups`:值本身在 `:340` 被读,死的是 `= None` 初值 ⇒ 改延迟初始化)、`unused_must_use` 1(`core/converse.rs:582` 加 `let _ =`);**test 目标侧另清 8 条**(`reverse_tests.rs`/`nemo_tests.rs`/`translate/mod.rs` 的 cfg(test) 模块 —— 门是 `--all-targets`,只清 lib 单元不够)。`translate/model.rs:5` 的 `Deserializer` 按 Main 裁定删除(全文件另有 3 处用全限定 `serde::Deserializer<'de>`)。
- **阶段 2(`dead_code` 31 条,`30216c5`)**:
  - **删 13**(零调用点):`auth.rs:91`/`:116` 的 `as_str`+`from_str`、`forum.rs:72 TargetType::as_str`、`shared.rs:370 ValueExt::get_string_or`(声明+实现)、`pipeline.rs:187 clear_processed_records`、`registry.rs:78 description`、`:204 reason_id_field`、`:274 prompt`(连带其 `format!` 与孤立的 `parts` 绑定)、`retrieve.rs:52`/`:82` 的 `as_str`、`decompile/mod.rs:905 BlockContext::new`。**踩坑记录**:原清单把 `forum.rs:55` 记为死项,但那一行是 `DeleteItemType::as_str`,而**相邻**的 `ItemType::as_str` 有调用点(`forum.rs:418/436`)⇒ 误删被编译器当场拦下,已还原原文、改删 `DeleteItemType`/`TargetType` 两处。
  - **`#[cfg(test)]` 10**(从生产构建移出,比"留着再闭嘴"合仓库口径):`mapping::{SHADOW_XML_INDEX, shadow_xml, kitten_names_for, reverse_candidates}`、`xml::{remove_attr, parse, Parser::run}`、`model::count_types` ×2(依 `convert-backlog.md` §2 第 10 条)、`tables_gen::SHADOW_XML`(**生成物 + 生成器 `src/bin/gen_translate_tables.rs` 两处同步**;本机 `temp/tables` 不在库内 ⇒ 无法重跑生成器核对,属刻意手改,提交信息已写明);`mapping.rs` 的 import 也拆出 cfg(test) 一条。
  - **`#[allow(dead_code)]` + 理由(阶段 2 处置 **8 项**;⚠️ 行号会漂,按符号定位)**:`RankingData.cvid`、`CloudCommand::Variable.private`(代码里本就写明"刻意不读")、`UserInfo` 三字段、`AdminReportStatistics.total_admins`、`FanByLikesStatistics` 五字段、`ActionRegistry.client`、`ReportFetcher.client`、`ReportTypeRegistry.default_actions`(注释明写"保留")。
    - **「8」与「6」的关系**:**8 是阶段 2 当时的处置项数**(含字段级 allow),**不是现存属性数**。其中 4 项(`RankingData.cvid`、`UserInfo` 三字段、`AdminReportStatistics.total_admins`、`FanByLikesStatistics` 五字段)已随 `104964f` 撤销;现存 4 项(`CloudCommand::Variable.private`、`ActionRegistry.client`、`ReportFetcher.client`、`ReportTypeRegistry.default_actions`)+ 2 项后加的(`KITTEN4_VOCAB_EXPORTED` 的 `cfg_attr(not(test), allow(dead_code))`、`translate/mod.rs` 的手工排障 `dump_real_work_for_external_validation`)= **全仓现 6 处属性**(2026-10-02 逐处实测;R2 当时的行号锚点 `cloudvar.rs:190/211`、`pipeline.rs:197`、`registry.rs:398/283` 已漂)。
- **四处对外形状 / 公共面项已移入 `pending-decisions.md` D6**(2026-10-02;按分库纪律:决策不进待办库)⇒ **逐项口径与状态只在 D6 展开**,此处不复制。

## 2. 小改(机械、低风险,可批量做)

1. ~~`DecompilerError` 自带 `Io/Json/Http` 与 `MewError` 重复 ⇒ 改为包装~~ —— **已不成立(2026-10-01 核实)**:`DecompilerError` 已无 `Io`/`Json`/`Http` 变体,`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew`(与 `ProcessorError`/`DataQueryError` 同处置);残留的死变体 `UnsupportedType` **已于 2026-10-02 删除**(`fef30e7`,公共面破坏性变更、已授权;见 `pending-decisions.md`「已决」的 2026-10-02 移入记录、`docs/rounds/39` §W5②)⇒ 本项**已清零**。
2. ✅ **已完成(2026-10-02,`cbb167a`)**:非锁裸 `unwrap` 硬化 —— **重新枚举后真实 50 处**(生产 **10** / `cfg(test)` **40**),**硬化 49、按约定保留 1**。
   - **旧计数「49 处」不可复现**:它点名的四族(`auth.rs::time_difference`、`registry.rs` 的 `active.as_mut().unwrap()`、`compiler.rs`(已并入 `core/convert/`)的 `template.unwrap()` 与 10 处 `write!(String).unwrap()`)在本树**已全部不存在**(时差缓存改 `Option` 判定、`active.as_mut()` 已是 `let Some(…) else`、`write!` 现均写作 `let _ = write!(…)`)。
   - **旧口径的坑**:`grep '\.unwrap()' | grep -v 'lock()\.unwrap()'` 会**漏掉跨行书写的 `lock()` ⏎ `.unwrap()` 链**、把大量锁解锁算成非锁 ⇒ 数字虚高且不可复现。正确口径要把 `.unwrap()` 的**接收者跨行回溯**,排除 `.lock()/.read()/.write()`。
   - **保留 1 处**:`utils/socketio.rs` 的 `Condvar::wait_timeout(...).unwrap()` —— 错误来源是**同一 Mutex 毒化**,与紧邻的 `lock().unwrap()` 同失败类,按"锁解锁保留"的约定**判为保留**。
   - **待复核 1 处**:`core/terminal.rs` 的 stdin 读失败本应错误返回,但 `ProcessorUi::input(&mut self, &str) -> String` 是**公共 trait 形状** ⇒ 现退为 `expect` fail-fast(避免空串让 `choose/menu` 死循环)。**已登记为 `pending-decisions.md` D6④**(是否改 `MewResult<String>` 并逐调用点传播 —— 破坏性,需拍板)。
   - 另:测试夹具层的 5 处(`tests/**`)不在本口径内,未动。逐处消息见 `cbb167a` 的 diff。
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
