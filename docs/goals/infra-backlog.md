# 仓库结构/工程待做(清理 · 类型化 · 拆分)

> 目标库:与平台协议无关的**仓库自身**未做完的事。约定见 `../knowledge/repo-conventions.md`。

## 1. 已在目标清单、待决

| 项 | 说明 | 出处 |
| --- | --- | --- |
| ~~恢复 `[lints.rust] unused` 告警~~ | **已完成(2026-10-02,rounds/40 R2,三阶段 `6414b97` / `30216c5` / `8da596d`)**,终态读数、逐条处置、两条实测坑与对外形状四项的去向**只在 §1.1 展开** | R2;`../rounds/40-gates-cleanup-and-real-defects.md` §2 |
| ~~CI 产物名列与 `crate-type` 不符(导致 job 静默绿、产物实际未上传)~~ | **已完成(2026-10-02,`f68c2e6`)**:走"改 CI"这一边 —— **删掉 artifact 上传步**及矩阵里 5 组 `artifact:`/`libname:` 键,理由:本仓 `[lib] crate-type=["rlib"]` 只产 rlib、`src/main.rs` 是需账号的**交互式控制台**、仓内没有消费这些 artifact 的地方,因此**没有可分发的产物**(不恢复 `cdylib`)。同时新增 `offline-gate` job(fmt/clippy/逐目标点名的离线测试)。旧症状与根因线索:期待 `libbackend.so`/`backend.dll`/`libbackend.dylib` 而实际不产,`if-no-files-found` 默认 `warn`,因此一直静默绿 | `f68c2e6`;口径见 `../knowledge/repo-conventions.md` §6 与 `../knowledge/errata.md` 的「非轮次条目」节 |
| ~~`src/main.rs` 作第二个 crate root,把整棵树重复编译一遍~~ | **已完成(2026-10-02,`47a8c5e`)**:`src/main.rs` 开头的 `mod api; mod core; mod utils;` 已删,顶部四条 `use crate::…` 全改成 `use backend::…`,**公共 API 零改动**(bin 用到的 11 个符号逐条核对皆 `pub`,与下方只读结论一致)。判据:bin 单元 dep-info 输入**由 49 降为 1**(`src/main.rs`;lib 单元仍覆盖整棵树);bin 单测目标**由 121 tests / 214 s 变为 0 tests / 0.00 s**;`unused = "warn"` 下 bin 侧**由 896 降为 0**。`Cargo.toml` 未动(bin 仍由 `src/main.rs` 自动发现) | `47a8c5e` |
| api 层类型化 DTO(B1)/ newtype ID 推广(B7)/ `work.rs` 再切 `WorkDataFetcher`(B8) | **唯一权威落 `./pending-decisions.md` B 组**(问题 / 选项 / 建议都在那里);本表只留指针 —— 其中 B1 自 2026-10-02 起属"明确不做" | `./pending-decisions.md` B1/B7/B8;`../rounds/15/16/17/19` §不落地 |

### 1.1 `unused` 落地记录(rounds/40 R2:基线经三阶段到 **0 条**)

> **口径**(全程同一判据,可复现):临时把 `unused` 改 `warn`,**按目标选择分别跑** `cargo clippy --lib` / `--bins` / `--tests --message-format=json`,按 `target.src_path` 归属。**不能按 `target.name`** —— lib / bin:`backend` / bin:gen 全叫 `backend`;`--all-targets` 也不行:既混合各单元又对跨单元诊断去重(旧树(`47a8c5e` 之前)实测:`--all-targets` 1447 < 三个选择之和 1547 = 946+551+50;R2 开工后的同口径是 50+50+98)。旧的"`--message-format short` 一次跑完"口径不可复现(short 不带 target 信息,无法按目标归属),因此本文只用"按目标选择分别跑"这一套(即下文的"两棵树"读数)。

- **读数(注意:分两棵树,不可混用)**:**终态**(`8da596d`,`--lib`/`--bins`/`--tests`)= **0 / 0 / 0**。
  - **R2 开工时**(`47a8c5e` 已落地、`src/main.rs` 已是 136 行薄壳):`--lib` **50**、`--bins` **50**(全部来自被连带重编的 **lib 依赖单元**,bin 自身 **0**)、`--tests` **98**(全在 lib 的 test 单元)—— **§1.1 的清单就是这 50 条**。
  - 注意:**`47a8c5e` 之前**(旧 `src/main.rs` 还把 `mod api/core/utils` 再编一遍)的同口径旧读数:`--lib` 50、`--bins` **946**(其中 `src/main.rs` **896**)、`--tests` **551**(其中 `src/main.rs` 453)。**这批 bin 侧噪声是那次重写清掉的、不是 R2 清的**,也无法在现有代码树复现(现有 `--bins` = **0**)——`../rounds/39` §0.3 W5 行所称的 `896` 降为 `0` 指的就是它。
- **阶段 1(机械族,`6414b97`)**:lib 单元 **19 条** —— `unused_imports` 13、`unused_variables` 2、`unused_mut` 2、`unused_assignments` 1(`decompile/editors.rs` 的 `restore_groups`:值本身在后面被读,死的是 `= None` 初值,因此改为延迟初始化)、`unused_must_use` 1(`core/converse.rs` 的 `must_use` 调用处加 `let _ =`);**test 目标侧另清 8 条**(`reverse_tests.rs`/`nemo_tests.rs`/`translate/mod.rs` 的 cfg(test) 模块 —— 门是 `--all-targets`,只清 lib 单元不够)。`translate/model.rs` 的 `Deserializer` 按 Main 裁定删除(全文件另有 3 处用全限定 `serde::Deserializer<'de>`)。
- **阶段 2(`dead_code` 31 条,`30216c5`)**:
  - **删 13**(零调用点;注意:按符号定位,不写行号):`auth.rs` 的 `as_str`+`from_str`、`forum.rs` 的 `TargetType::as_str`、`shared.rs` 的 `ValueExt::get_string_or`(声明+实现)、`pipeline.rs` 的 `clear_processed_records`、`registry.rs` 的 `description`/`reason_id_field`/`prompt`(后者连带其 `format!` 与孤立的 `parts` 绑定)、`retrieve.rs` 的两处 `as_str`、`decompile/mod.rs` 的 `BlockContext::new`。**踩坑记录**:原清单把 `forum.rs` 的 `DeleteItemType::as_str` 记为死项,但**相邻**的 `ItemType::as_str` 有调用点,误删被编译器即时报错拦下,已还原原文、改删 `DeleteItemType`/`TargetType` 两处。
  - **`#[cfg(test)]` 10**(从生产构建移出,比"留着再闭嘴"合仓库口径):`mapping::{SHADOW_XML_INDEX, shadow_xml, kitten_names_for, reverse_candidates}`、`xml::{remove_attr, parse, Parser::run}`、`model::count_types` ×2(依 `./convert-backlog.md` §2 第 10 条)、`tables_gen::SHADOW_XML`(**生成物 + 生成器 `src/bin/gen_translate_tables.rs` 两处同步**;本机 `temp/tables` 不在库内,因此无法重跑生成器核对,属刻意手改,提交信息已写明);`mapping.rs` 的 import 也拆出 cfg(test) 一条。
  - **`#[allow(dead_code)]` + 理由(阶段 2 处置 **8 项**;注意:行号会漂,按符号定位)**:`RankingData.cvid`、`CloudCommand::Variable.private`(代码里本就写明"刻意不读")、`UserInfo` 三字段、`AdminReportStatistics.total_admins`、`FanByLikesStatistics` 五字段、`ActionRegistry.client`、`ReportFetcher.client`、`ReportTypeRegistry.default_actions`(注释明写"保留")。
    - **「8」与「6」的关系**:**8 是阶段 2 当时的处置项数**(含字段级 allow),**不是现存属性数**。其中 4 项(`RankingData.cvid`、`UserInfo` 三字段、`AdminReportStatistics.total_admins`、`FanByLikesStatistics` 五字段)已随 `104964f` 撤销;`ActionRegistry.client` 与 `ReportFetcher.client` 两个字段随后(2026-10-03)随 `./pending-decisions.md` D6 的 2) 删除。**因此现在全仓是 4 处属性**:`CloudCommand::Variable.private`、`ReportTypeRegistry.default_actions`、`KITTEN4_VOCAB_EXPORTED` 的 `cfg_attr(not(test), allow(dead_code))`、`translate/mod.rs` 的 `dump_real_work_for_external_validation`(另有 1 处仅注释提及)。2026-10-02 的逐处实测写在 `cloudvar.rs`/`pipeline.rs`/`registry.rs` 里的行号锚点已漂,故这里只按符号定位。
- **四处对外形状 / 公共面项已移入 `./pending-decisions.md` D6**(2026-10-02;按分库纪律:决策不写进 backlog),因此**逐项口径与状态只在 D6 展开**,此处不复制。

## 2. 小改(机械、低风险,可批量做)

1. ~~`ConvertError`(当时的 `DecompilerError`)自带 `Io/Json/Http` 与 `MewError` 重复,改为包装~~ —— **已不成立(2026-10-01 核实)**:`ConvertError` 已无 `Io`/`Json`/`Http` 变体,`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew`(与 `ProcessorError`/`DataQueryError` 同处置);残留的死变体 `UnsupportedType` **已于 2026-10-02 删除**(`fef30e7`,公共面破坏性变更、已授权;见 `./pending-decisions.md`「已决」的 2026-10-02 移入记录、`../rounds/39` §W52) ),本项**已清零**。
2. **已完成(2026-10-02,`cbb167a`)**:非锁裸 `unwrap` 硬化 —— **重新枚举后真实 50 处**(生产 **10** / `cfg(test)` **40**),**硬化 49、按约定保留 1**。
   - **旧计数「49 处」不可复现**:它点名的四族(`auth.rs::time_difference`、`registry.rs` 的 `active.as_mut().unwrap()`、`compiler.rs`(已并入 `core/convert/`)的 `template.unwrap()` 与 10 处 `write!(String).unwrap()`)在本树**已全部不存在**(时差缓存改 `Option` 判定、`active.as_mut()` 已是 `let Some(…) else`、`write!` 现均写作 `let _ = write!(…)`)。
   - **旧口径的坑**:`grep '\.unwrap()' | grep -v 'lock()\.unwrap()'` 会**漏掉跨行书写的 `lock()` 换行后接 `.unwrap()` 的链**、把大量锁的解锁误计为非锁,导致数字虚高且不可复现。正确口径要把 `.unwrap()` 的**接收者跨行回溯**,排除 `.lock()/.read()/.write()`。
   - **保留 1 处**:`utils/socketio.rs` 的 `Condvar::wait_timeout(...).unwrap()` —— 错误来源是**同一 Mutex 毒化**,与紧邻的 `lock().unwrap()` 同失败类,按"锁解锁保留"的约定**判为保留**。
   - **已完成(2026-10-03,见 `../rounds/43`)**:`core/terminal.rs` 的 stdin 读失败原先只有 `expect` fail-fast(避免空串让 `choose`/`menu` 死循环);现按 `./pending-decisions.md` D6 的 4) 落地 —— `ProcessorUi::{input,choose,menu}` 返回 `MewResult<String>`,**读到 EOF 也算错误**并逐调用点传播,`panic = "abort"` 下直接终结进程的风险随之消失。
   - 另:测试夹具层的 5 处(`tests/**`)不在本口径内,未动。逐处说明见 `cbb167a` 的 diff。
3. **P2 收尾四项已逐项结案(2026-10-03,见 `../rounds/45`)**:
   - `simple.rs` 的 `Arc<Value>`:**已完成** —— `CocoDecompiler::decompile` 改为 `Arc::try_unwrap`,拿不到唯一所有权才回退克隆。
   - `nemo.rs::get_sha` 的 64 字节 clone:**不可行** —— 缓存是 `RefCell<HashMap<String,String>>`,借用守卫无法把内部 `&str` 带出函数边界(改 `Rc/Arc` 或 `&mut self` 的改动面远超该 clone 的收益)。
   - `AccountStatus` 与 `Identity` 平行枚举:**已完成**(合并为单一 `Identity`,删 `AccountStatus` 与 `to_identity`;映射 `Average→Fluffy`/`Edu→Scholar`/`Judgement→Judge` 一一对应,默认身份 `Fluffy` 不变)。
   - `MessageHandler` / `ChatEventHandler` 两个 trait 与 `registry.rs` 错位工具函数:**已完成**(两个私有 trait 改为自由函数;`value_to_string`/`timestamp_to_string`/`html_to_text`/`bytes_to_human` 四个与举报无关的工具从 `core::registry` 移到 `utils::filedata`,与既在那里的 `value_to_i64` 同处)。

## 3. 待核验(证据不足)

> 三条已于 2026-10-03 核验并结案(见 `../rounds/45`)。

1. **判不做(已核实,2026-10-03)**:`services.rs` 没有 `report_processor_new_with_client_uses_injected_client` 这个测试名,且该文件**没有测试模块**;全仓同名 0 命中(`src/`、`tests/`)。这不是遗漏而是 `../rounds/15` 方案自留的退化(同文"实际执行结果"只记了 `account.rs::manager_new_with_client_uses_injected_client` 与 `registry.rs::fetch_chunked_terminates_without_duplicates`)。要闭合就在 `services.rs` 末尾加同文件 `#[cfg(test)]` 断言注入客户端身份。
2. **已完成(2026-10-03 核对;`659c030`)**:`../rounds/19` Phase 4 要求的三处 CONTRIBUTING 改动**都在现行文件里**(命名段直白名、锁段含"默认 std + 仅经评审才引 `parking_lot`"、错误段含"保留底层变体,不要压成 `Auth(String)`")。
3. ~~`../rounds/11` §P4-3、`../rounds/13` §P3-5/§P3-6 三条~~ **已完成(2026-10-03 核验;`659c030`)**:三条均已落实(`converse.rs::handle_frame` 的"刻意不二次解析"注释、`education.rs::fetch_organization_ids` 传 `None`、`forum.rs::fetch_7day_hot_posts_iter` 端点固定),且 `./platform-backlog.md` §2 已同名标完成 ⇒ 本条是**重复挂账**,已删。

## 4. 已决(不做/暂缓)

| 项 | 结论 | 出处 |
| --- | --- | --- |
| `core` 向 `api` 的依赖倒置(**C8**) | **暂缓(待重新立项)** —— 唯一权威落 `./pending-decisions.md` C8(架构级重构,收益不明确) | `./pending-decisions.md` C8;`../rounds/05` 未执行项 |
| `cloudvar.rs` 深拆 | 判不做(需 60+ 处可见性提升 + `Arc<CloudInner>` 贯通,非纯搬迁) | `../rounds/18/19` §不落地 |
| `compiler.rs` 切子模块 | 判不做(用户指定"两个文件、不放 core 子文件夹") | `../rounds/18` §不落地 |
| 公开 `HttpClient` trait / 类型级 WS 状态机 | 判不做(前者可经 `CodeMaoClient` 注入;后者维持运行时 `connect_and_wait()`) | `../rounds/18/19` §不落地 |
| `MewError`/`MewResult` 改名 | 判不做(保留品牌名) | `../rounds/19` §不落地 |
| god file 拆分 / 管理器样板宏化 | 判不做(用户 11 条回退指令) | `../rounds/02/03` |

## 5. 文档类

1. `../rounds/01` 的「附录:坑 10 全文」与正文重复,且残留写作指令句,宜去重或改成链接(该文属历史稿,**按"历史保真"约定不改**;需要时在勘误里注明)。
2. "回调不应 panic"必须写进 rustdoc(`release` 是 `panic="abort"`,catch_unwind 无效)(`../rounds/01` 坑 13)。
3. 本库(`../knowledge/`、`../goals/`)已替代 `../rounds/` 作为**入口**;新增改动优先更新本库 + 一个新轮次记录。
4. **待方案**:文档体例尚无自动化门 —— 第五十二轮的复核靠一次性脚本核(每张表的列数与缩进、正文里的 `本轮|本次|最新|目前`、表格外的 `=>`/`->`/`<->`、状态近义词);是否把这几类检查固化成 `docs` 侧的门(或并入既有 hygiene 测试)待定(出处:`../rounds/52-docs-survey-and-reverify.md` §9)。

## 6. 架构盘点(2026-10-03;四条已全部处置)

> 只读盘点认为整体分层健康:`utils` 为叶子、`api` 居中层、`core` 居上,全仓无模块环,`core → api` 为既定方向(C8 暂缓)。该节四条**已全部结案**(两条见 `../rounds/44`、两条见 `../rounds/46`),下面保留结论备查。

| 项 | 证据 | 说明与代价 |
| --- | --- | --- |
| **公共请求原语把 `ureq` 实现类型泄漏进 `pub` 签名** **已完成(2026-10-03,`../rounds/44`)** | 改后:公共原语返回自有 `requests.rs::MewResponse`(只给 `status()`/`header()`),读取助手收 `MewResponse`;`MewError::Http` 装自有 `requests.rs::TransportError`(文本 + `is_timeout()`);`agent()`/`send_multipart()` 收 `pub(crate)` | 判据:外部消费 crate **不声明 ureq** 即可编译并跑通完整链路(实测 `status=200` + body);ureq 升级不再自动构成本库破坏性变更 |
| **`CheckConfig` 是 `pub struct` 但字段全 `pub(crate)` 且无公开构造器** **已完成(2026-10-03,`../rounds/46`)** | 取"收窄"这一边:`ReportProcessor::{new_with_config,new_with_config_and_client}` 收 `pub(crate)`;其中 `new_with_config` 收窄后零调用 ⇒ 直接删除。对外只留 `ReportProcessor::{new,new_with_client}`(`CheckConfig` 类型本身保留在 crate 内,由 `ViolationChecker` 消费) | 判据:全仓 `ReportProcessor::` 调用点只有 `new()`(`src/main.rs`)与静态助手(`terminal.rs`)⇒ 无外部使用者受损;公共签名不再承诺不可达的"自定义配置" |
| **`api` 层两处零调用的全局入口** **已完成(2026-10-03,`../rounds/46`)** | 删除 `auth.rs::{global_auth_manager,GLOBAL_AUTH_MANAGER(static),fetch_current_timestamp}`(零调用);保留在用的 `fetch_current_timestamp_with_provider` | 判据:全仓(含 `tests/`)这三项只出现在定义处 ⇒ 删除后 `clippy -D warnings` 反而抓到两处因此变空的 import(`Arc`/`OnceLock`),已一并清掉;注入纪律不再有"看似可用的全局登录入口" |
| **错误类型边界不一致** **已完成(2026-10-03,`../rounds/44`)** | `registry.rs::ProcessorError` 现为 `Processing`/`Mew`/`Aborted`(`io`/`serde_json` 由 `From` 折进 `Mew`);`retrieve.rs::DataQueryError` 删 `Json`、`External` 改名 `Mew`;`filedata.rs::FileError` 整型删除,`CodeMaoFile::write_bytes` 返回 `MewResult<()>` | 口径与 `translate/options.rs::TranslateError` 一致;同一个底层失败在全仓只有一种表示 |
