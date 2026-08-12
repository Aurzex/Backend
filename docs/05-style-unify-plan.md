# 代码调用风格统一 — 审阅与整改方案(已执行)

审阅日期:2026-08-11 · 基线:HEAD `bee812d` · 范围:全仓 27 个 Rust 源文件(约 2.5 万行)

## Context

请求:审阅全仓 Rust 代码(同步命令行/控制台后端),找出**同一语义、多种写法**的调用风格不一致,给出统一修改方案并执行。三轮执行:① 用户指定执行 P2 的 build_paginated/随机 ID/ChangeSource 三项 + WorkType 收敛;② 用户追加执行 P0/P1 全部项;③ 用户要求 WorkType/KittenVersion 回退到原文件(接受重复),不追求 api 层共享枚举。

前两轮评审(`docs/02-review-round1.md` → `docs/03-fix-plan-v2.md` → `docs/04-review-round2.md`,原 `temp/REVIEW.md`/`FIX_PLAN.md`/`backend-review-plan.md`)已整改严重缺陷与样板冗余。

## 统一规则(目标风格,全仓唯一写法)

| 维度       | 统一目标写法                                                                                       | 状态               |
| ---------- | -------------------------------------------------------------------------------------------------- | ------------------ |
| 错误传播   | `?` + `ok_or_else(                                                                                 |                    | ...)`,禁裸 `unwrap`/`expect`(锁除外) | 已执行(P0-1) |
| 错误消息   | 一律中文                                                                                           | 已执行(P0-3)       |
| Value 读取 | 一律 `.get("key")`,禁 `["key"]` 索引                                                               | 已执行(P0-2)       |
| 值提取     | `.get(k).and_then(Value::as_str)`,不裸 `as_str().unwrap()`                                         | 已执行(P0-2)       |
| 时间戳     | 统一 `current_timestamp_13()`(毫秒)/ `current_timestamp_secs()`(秒)                                | 已执行(P1-1)       |
| 同实体 ID  | 同一实体 ID 全仓同一类型                                                                           | 已执行(P1-2)       |
| 方法命名   | `fetch_`=远端拉取、`get_`=本地读取、`list_`=列表、`*_gen`=分页迭代器、动作方法不加 `execute_` 前缀 | 已执行(P0-5、P1-3) |
| 日志       | 顶部 `use log::{...}` 按需导入;禁别名与全限定                                                      | 已执行(P0-4)       |
| 校验       | `Option` 缺失用 `ok_or_else` + `?`,不嵌套 match+return Err                                         | 已执行(P1-4)       |

## 已执行改动(全部 `cargo check --all-targets` 通过,`cargo test` 3 passed)

### 1. build_paginated 统一 — 删除四文件私有副本,改用 acquire 公共版

删除 4 个私有副本(education/forum/user/work),28 个调用点内联为 `self.client.build_paginated(endpoint)` + 域内默认(education: Education base_key+Page 分页;forum: Page 分页;work: TIME 参数)。调用点原尾链保留。

### 2. 随机 ID 三函数统一 — acquire 公共版 `generate_random_id`

- `acquire.rs`:新增 `pub fn generate_random_id(length, charset)`;私有 `generate_meow_id` 改其薄封装。
- `auth.rs`:删 `generate_client_id`,改调 `generate_random_id(8, b"...")`。
- `converse.rs`:删 `generate_session_id`,2 处调用改公共版。
- 字符集/位数不变,仅收敛实现位置。

### 3. ChangeSource 枚举化 — 消除字符串字面量

- `cloudvar.rs`:`emit_variable_change` 第 7 参 `&str` → `ChangeSource`;内部 `source.as_str()` 传回调(回调签名不可改)。
- 3 处调用点 `"local"/"cloud"` → `ChangeSource::Local/Cloud`。

### 4. P0-1: ok_or → ok_or_else(4 处)

- `cloudvar.rs:2257`、`converse.rs:501/792/827` `.ok_or(Error::NotConnected)?` → `.ok_or_else(|| ...)`。

### 5. P0-2: Value 索引 → .get()(15 处)

- `work.rs:850-877` 9 行 `project["..."]` 索引 → `.get("...")`(双层 `["addition"]["isTurnOnDebug"]` → `.get("addition").and_then(|v| v.get(...))`)。
- `auth.rs:295/536`、`community.rs:172` 索引 → `.get(...).and_then(...)`。
- `compiler.rs:2032/2041` 数组字面量、`:3030` json! 宏键不动。

### 6. P0-3: 错误消息中文化(4 处)

- `forum.rs:531/539` 英文 → 中文(同时被 P1-4 重构吸收)。
- `acquire.rs:139/749/761` "invalid base key"/"does not support"/"requires a request body" → 中文。

### 7. P0-4: 日志 import 统一(3 文件)

- `services.rs:10` 别名 `error as log_error` → 直用 `error!`/`info!`(10 处宏调用)。
- `registry.rs` `log::error!` 全限定 3 处 → 顶部 `use log::error`。
- `acquire.rs` `use log::debug` → `{debug, warn}`;`log::warn!` → `warn!`。

### 8. P0-5: execute_ 前缀去除(全仓 24 处)

动作方法统一去 `execute_` 前缀。涉及:

- `forum.rs`:toggle_like、toggle_comment_top_status(原 execute_toggle_*)。
- `work.rs`:toggle_follow、toggle_collection、toggle_like、toggle_comment_pin、toggle_comment_like、fork_work、share_work、report_work、report_comment、publish_kitten_work、unpublish_work(_web)、empty_kitten_trash、publish_kn_work、unpublish_kn_work、empty_kn_trash、recover_kn_trash、update_coco_work、publish_coco_work、enable_collaboration。
- `library.rs`:toggle_novel_favorite、toggle_comment_like、toggle_book_like。
- `whale.rs`:report_comment、apply_avatar_frame、process_post_report、process_discussion_report、process_comment_report、process_work_report。
- `account.rs`:unbind_qq、unbind_wechat、bind_phone、request_phone_change_verification、change_password_by_phone、init_password、set_username。
- `auth.rs`:logout_v0、logout_v12(+ 2 处 `self.` 调用)。
- `user.rs`:apply_avatar_frame;`shop.rs`:apply_to_join、review_join_application、report_comment。
- `community.rs`:sign_agreement;`education.rs`:bulk_reset_passwords、transfer_to_unassigned、mark_all_messages_as_read、grade_student_work、invite_to_class、accept_class_invite、improve_teacher_info。
- **保留**:core 引擎方法 `execute_action`/`execute_single_report`(非 api 动作封装);registry.rs 动作键字符串 `"execute_process_*"`(外部契约,宏 `register_report_handler!` 的 method 键,与 pipeline.rs 方法标识同步改新名)。

### 9. P1-1: 时间戳统一(4 处内联 SystemTime 消除)

- `acquire.rs`:新增 `pub fn current_timestamp_secs() -> i64`(基于 current_timestamp_13)。
- `auth.rs:459`(fetch_admin_captcha,毫秒)→ `current_timestamp_13() as i64`;`:1165/:1173`(get_calibrated_timestamp,秒)→ `current_timestamp_secs()`;use 删 `SystemTime, UNIX_EPOCH`。
- `services.rs:79`(上传日志,秒)→ `current_timestamp_secs()`;use 收窄 `{Duration, Instant}`。
- 语义:系统时间异常从崩溃(expect)变为返回 0。

### 10. P1-2: workshop_id 同实体同类型 → i32(2 处 &str 改 i32)

- `shop.rs:81` fetch_workshop_details、`:297` update_workshop `&str` → `i32`(全文件 i32 占 9 处、&str 仅 2 处,统一 i32;format!/json! 对 i32 自动 Display)。无外部调用点。

### 11. P1-3: clouddb 读前缀统一 fetch_(5 处)

- `clouddb.rs:184/195/285/296/409` get_dictionary_keys/get_dictionary_value/get_table_row_count/get_table_info/get_dict_entries → fetch_ 前缀(远端拉取,与 fetch_ranking_records 一致;list_* 保留)。无外部调用点。

### 12. P1-4: forum create_post 校验改 ?(1 处)

- `forum.rs:518-536` target_type 分支内嵌 `None => return Err(...)` → 提前 `ok_or_else(...)?`,消除嵌套 match。

### 13. WorkType / KittenVersion — 回退到原文件(按用户指令,接受重复)

- 撤销"上移 api.rs 根"方案:`src/api.rs` 恢复纯 mod 声明;work.rs 恢复本地 WorkType(V3,V4 顺序)/KittenVersion;user.rs 恢复本地 WorkType(含 pub as_str,原 V4,V3 顺序)/KittenVersion。
- 现状:两文件各持一份重复定义(与改动前一致),compiler.rs 的独立 WorkType 从未动。

## Critical files & anchors(执行后)

- `src/utils/acquire.rs` — `generate_random_id`、`current_timestamp_13()`、`current_timestamp_secs()`(新增)、公共 `build_paginated`、`use log::{debug, warn}`。
- `src/api/{education,forum,user,work}.rs` — 28 处内联 `self.client.build_paginated(...)` 链;各文件本地 WorkType/KittenVersion。
- `src/core/cloudvar.rs` — `emit_variable_change`(ChangeSource 参数)+ 3 调用点;ok_or_else。
- `src/core/pipeline.rs` — `register_report_handler!` 宏 4 个方法标识改新名(`process_*`),动作键字符串保留 `"execute_process_*"`。

## Verification(实际执行结果)

- 三轮执行中每步 `cargo check` 0 error;最终 `cargo check --all-targets` 通过,`cargo test` 3 passed。
- 行为等价声明:
    - build_paginated:内联链与原私有副本逐方法一致。
    - 随机 ID:字符集/长度/位数不变。
    - ChangeSource:枚举 as_str 输出与字面量一致。
    - execute_ 改名:纯重命名,方法体未动;registry 动作键字符串保留,宏方法标识同步。
    - 时间戳:系统时间异常从崩溃变为返回 0(行为改善)。
    - workshop_id/clouddb 前缀:纯签名/改名,format! 与 json! 自动适配。

## 未执行项(记录在案)

| 条目                                                                          | 位置         | 不执行原因                                  |
| ----------------------------------------------------------------------------- | ------------ | ------------------------------------------- |
| add_timestamp_to_paginated(education/whale)与 community raw_timestamp_10 体系 | api 三文件   | 域内参数名(TIME/timeStamp)不同,统一需新抽象 |
| get_user_login_method / get_admin_login_method 同构(auth.rs)                  | auth.rs      | 语义分角色,抽参需 fn 指针,过度抽象          |
| `with_page(Option<i32>)` vs `with_limit(usize)`(acquire.rs)                   | acquire.rs   | 已桥接,改 usize 牵连 30+ 调用点             |
| core → api 依赖倒置(pipeline/registry/services/retrieve use api::whale 等)    | core 四文件  | 架构级重构,超调用风格范围                   |
| registry.rs 错位工具函数                                                      | registry.rs  | 模块组织问题(见 04-review-round2.md)        |
| retrieve.rs for+push vs iterator 链                                           | retrieve.rs  | 行为等价、改动低价值                        |
| UFCS 混用(pipeline/converse/retrieve)                                         | 三文件       | 三种写法均常见,统一属审美                   |
| compiler.rs ValueExt 报错版 5 方法零调用                                      | compiler.rs  | 死代码(见 02-review-round1.md)              |
| WorkType/KittenVersion 去重                                                   | user/work.rs | 用户明确接受重复,回退原文件                 |

## Assumptions

- 历史 FIX_PLAN 的其余"不合并"决策(add_timestamp helper、with_time 等)继续有效。
- WorkType/KittenVersion 保持两文件重复定义(用户决定,不回退到共享枚举)。
- 动作键字符串 `"execute_process_*"` 视为外部契约保留;若未来改键,需同步 registry.rs 与所有 `execute_action` 调用实参。
