# 仓库结构/工程待办(清理 · 类型化 · 拆分)

> 目标库:与平台协议无关的**仓库自身**未完成项。约定见 `docs/knowledge/repo-conventions.md`。

## 1. 已在目标清单、等决策

| 项 | 说明 | 出处 |
| -- | ---- | ---- |
| 恢复 `[lints.rust] unused` 告警 | **2026-10-02 重测(口径已修正)。** 旧记录「943 条 = `lib` 55 + `bin "backend"` 898」**不可复现**:那条用的 `--message-format short` **不带 target 信息**,分不出编译单元;且 `943 ≠ 55+898=953`,数字自相矛盾。新口径(临时把 `unused` 改 `warn`,**按目标选择分别跑**,清单见 §1.1):`cargo clippy --lib` = **50**(全在 `src/lib.rs`);`cargo clippy --bins` = 50(**全部来自被顺带重编的 lib 依赖单元,bin 自身 0 条**);`cargo clippy --tests` = 98(全在 `src/lib.rs`;bin-test 与集成测试目标 0 条)。对照改前同口径:`--lib` 50、`--bins` **946**(其中 `src/main.rs` 896)、`--tests` **551**(其中 `src/main.rs` 453)。⇒ **bin 侧噪声已随「去第二个 crate root」清零**;**开关仍维持 `allow`** —— 剩余 lib 侧 50 条未清,改 `warn` 会让 `clippy --all-targets -- -D warnings` 变红(rustc 1.98.0 实测:显式 `--warn=unused` **挡不住** `-D warnings`,`--allow=unused` 挡得住;Cargo 把 `[lints]` 表当 `--warn/--allow=unused` 传给 rustc,可在 `cargo build -v` 的 rustc 命令行里直接看到) | 2026-10-02 实测(本轮提交);旧数字出处 `docs/rounds/39` §W5④ |
| CI 产物名列与 `crate-type` 不符(⇒ **job 静默绿、产物其实没上传**) | `.github/workflows/CI.yml:28/33/39/44/49` 期待 `libbackend.so` / `backend.dll` / `libbackend.dylib`,而 `Cargo.toml:8` 是 `crate-type = ["rlib"]` ⇒ **根本不产 `.so/.dll`**;上传步(`CI.yml:63-67`)按该路径找文件,`actions/upload-artifact` 的 `if-no-files-found` **默认 `warn`** ⇒ 找不到也绿。根因线索:`docs/rounds/01:791/809`(为可测性把 `"rlib"` 加进 `crate-type`,随后 `cdylib` 被去掉而 CI 的 `libname` 列表没跟上)。修哪边未定(改 CI 的 `libname`/产物形态,或恢复 `crate-type = ["cdylib","rlib"]`) | 2026-10-02 实测;勘误落 `docs/knowledge/errata.md` |
| ~~`src/main.rs` 作第二个 crate root,把整棵树重复编译一遍~~ | **已修(2026-10-02,本轮提交)**:`src/main.rs` 开头的 `mod api; mod core; mod utils;` 已删,4 条 `use crate::…`(`:8-11`)改 `use backend::…`,**公共 API 零改动**(bin 用到的 11 个符号逐条核对皆 `pub`,与下方只读结论一致)。判据:bin 单元 dep-info 输入 **49 → 1**(`src/main.rs`;lib 单元仍覆盖整棵树);bin 单测目标 **121 tests / 214 s → 0 tests / 0.00 s**;`unused = "warn"` 下 bin 侧 **896 → 0**。`Cargo.toml` 未动(bin 仍由 `src/main.rs` 自动发现) | 本轮提交 |
| api 层类型化 DTO(351 处 `MewResult<Value>`) | 逐端点核对响应形态;建议按域分批 | `docs/rounds/15/16/17` §不落地 |
| newtype ID 推广(`UserId` 等) | 先看 `WorkId` 试点收益 | `docs/rounds/19` §不落地 |
| `work.rs` 再切 `WorkDataFetcher` | 纯搬迁,`re-export` 保路径;按需 | `docs/rounds/19` §不落地 |

### 1.1 `unused` 剩余清单(**lib 侧 50 条**,2026-10-02 用 `unused = "warn"` 实测)

> **口径**:`cargo clippy --lib --message-format=json`,取 `reason == compiler-message`、`level ∈ {warning,error}`、`target.src_path` 属本包者 ⇒ 即 **lib 的非 test 编译单元**。
> 另有 lib 的 **test 编译单元 98 条**(`cargo clippy --tests`;含 `#[cfg(test)]` 代码,数字不同是因为测试会"用活"一部分私有项);bin 与集成测试目标 **0 条**。
> 机械项(`unused_imports` 13 + `unused_variables` 2 + `unused_mut` 2 + `unused_assignments` 1 + `unused_must_use` 1 = **19 条**)可一轮清掉;`dead_code` 31 条要逐条判「删 / 接线 / 留 `#[allow]` 并写理由」,其中**删 `pub` 面是红线**(先取授权,先例:rounds/39 §W5②)。

- `unused_imports`(13):`api/community.rs:3 DEFAULT_LIMIT`、`core/cloudvar.rs:3 AtomicUsize`、`core/cloudvar.rs:14 WebSocket`、`core/converse.rs:14 WebSocket`、`core/convert/decompile/editors.rs:20 HashSet`、`core/convert/translate/assembly.rs:2 BlockTree`、`core/convert/translate/assembly.rs:5 mapping`、`core/convert/translate/mod.rs:154 PathConfig`、`core/convert/translate/model.rs:5 Deserializer`、`core/convert/translate/nemo_mapping.rs:8 TEXT_PLACEHOLDER_BLOCKS`、`core/convert/translate/pipeline.rs:7 StageSize`、`core/convert/translate/pipeline.rs:543 Map`、`utils/filedata.rs:2 serde_json::Value`
- `unused_variables`(2):`core/convert/decompile/mod.rs:1400 conditions_count`、`core/convert/translate/nemo.rs:591 roots`
- `unused_mut`(2):`core/convert/translate/assembly.rs:128`、`core/convert/translate/nemo_mapping.rs:1892`
- `unused_assignments`(1):`core/convert/decompile/editors.rs:179 restore_groups`
- `unused_must_use`(1):`core/converse.rs:582`(未用的 `Result`)
- `dead_code`(31,均为"never read/never used"):
  - 私有字段未读:`core/cloudvar.rs:190 cvid`、`:211 private`、`core/converse.rs:101 user_id/chat_count/remaining_image_times`、`core/convert/decompile/mod.rs:895 variable_map`、`core/pipeline.rs:197 client`、`core/registry.rs:78 description`、`:204 reason_id_field`、`:274 prompt`、`:283 default_actions`、`:398 client`、`core/retrieve.rs:1192 total_admins`、`:1202 target_user_id/like_threshold/total_fans/qualified_fans_count`
  - 关联项/方法未用:`api/auth.rs:91 as_str+from_str`(UserRole)、`:116 as_str+from_str`(AccountStatus)、`api/forum.rs:55 as_str`、`:72 as_str`、`core/retrieve.rs:52 as_str`、`:82 as_str`、`core/convert/decompile/mod.rs:905 new`、`core/convert/shared.rs:370 get_string_or`、`core/convert/translate/model.rs:172 count_types`、`:209 count_types`、`core/convert/translate/xml.rs:239 remove_attr`、`:555 run`、`core/pipeline.rs:187 clear_processed_records`
  - 自由函数/常量未用:`core/convert/translate/mapping.rs:222 SHADOW_XML_INDEX`、`:298 shadow_xml`、`:888 kitten_names_for`、`:899 reverse_candidates`、`core/convert/translate/tables_gen.rs:1153 SHADOW_XML`、`core/convert/translate/xml.rs:372 parse`

## 2. 小改(机械、低风险,可批量做)

1. ~~`DecompilerError` 自带 `Io/Json/Http` 与 `MewError` 重复 ⇒ 改为包装~~ —— **已不成立(2026-10-01 核实)**:`DecompilerError` 已无 `Io`/`Json`/`Http` 变体,`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew`(与 `ProcessorError`/`DataQueryError` 同处置);残留的死变体 `UnsupportedType` **已于 2026-10-02 删除**(`fef30e7`,公共面破坏性变更、已授权;见 `pending-decisions.md` C2、`docs/rounds/39` §W5②)⇒ 本项**已清零**。
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
