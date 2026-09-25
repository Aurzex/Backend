# 全仓 API 类型化重构与 README 示例统一 — 整改记录（已执行）

审阅日期:2026-08-11 · 基线:HEAD `f5fd314` · 范围:全仓 pub API 裸 bool/裸 &str/裸 i64 参数、字符串分发、README 示例 6 段

## Context

两轮审阅合并后执行:① README 示例代码风格(6 处);② 全仓裸参数/字符串分发统一枚举化。**破坏性替换签名**(用户允许不考虑兼容性)。全部改动 `cargo check --all-targets` 通过,`cargo test` 3 passed,`cargo clippy --all-targets` 0 warning。

## 已执行改动

### 核心层

1. **ResponseMode 枚举化**(acquire.rs + 6 api 文件)
    - acquire.rs 新增 `pub enum ResponseMode { Data, Status }`;`ClientAccess::send_maybe_parse(builder, return_data: bool, ...)` → `(builder, mode: ResponseMode, ...)`。
    - 8 处 `return_data: bool` 参数改 `mode: ResponseMode`(account:822/education:175/work:257,279/library:378,395,409,511,536,668/forum:383,401,446,468,521/shop:453,493);education:860 计算式改 `if method == Get { Data } else { Status }`。
    - pipeline.rs 2 处 `report_post/report_item` 传 `false` 改 `ResponseMode::Status`。
    - 残留验证:`grep return_data src/` 返回 0。

2. **HistoryMode 枚举化**(converse.rs)
    - 新增 `pub enum HistoryMode { Include, Exclude }`;`send_message(message, include_history: bool)` → `(message, mode: HistoryMode)`;`send_and_wait(message, include_history: bool, timeout)` → `(message, mode: HistoryMode, timeout)`。
    - 内部 `mode == HistoryMode::Include` 判断;无外部调用点。

3. **RankingOrder 枚举化**(cloudvar.rs)
    - 新增 `pub enum RankingOrder { Ascending, Descending }` + `as_code()`(1/-1);`get_ranking(variable_name, limit, order: i64)` → `(..., order: RankingOrder)`。
    - 非法 order 运行期校验删除(编译期穷尽,行为改善);`ASCENDING_ORDER`/`DESCENDING_ORDER` 常量保留供 as_code。

### 外围层

4. **LogoutMethod 接入 logout_v12**(auth.rs)
    - `logout_v12(method: &str)` → `logout_v12(method: LogoutMethod)`;内部 match Web→"web"/Mobile→"mobile",V0/Admin 返回 Err;调用点 `logout_v12("web")` → `logout_v12(LogoutMethod::Web)` 等。
    - 非法字符串实参编译期禁止。

5. **ToggleAction 统一**(acquire.rs + work/forum/library)
    - acquire.rs 新增 `pub enum ToggleAction { On, Off }` + `to_http_method(on_method, off_method)`。
    - work.rs:删 `SelectMethod` 枚举;`toggle_follow/toggle_collection/toggle_like/toggle_comment_like` 的 `method: SelectMethod` → `action: ToggleAction`(Post/Delete 对);`toggle_comment_pin` 裸 `HttpMethod` 参数 → `ToggleAction`(Put/Delete 对)。
    - forum.rs:`toggle_like(action: &str)` → `(action: ToggleAction)`(Put/Delete 对,删 "like"/"unlike" match);`toggle_comment_top_status(should_top: bool)` → `(action: ToggleAction)`(Put/Delete 对)。
    - library.rs:`toggle_novel_favorite/toggle_comment_like/toggle_book_like` 的 `like: bool` → `action: ToggleAction`(Post/Delete 对)。
    - 残留验证:`grep '"like"\|"unlike"' src/api/` 返回 0。

6. **NemoMessageType 枚举化**(community.rs)
    - 新增 `pub enum NemoMessageType { Like, Comment }` + `as_url_code()`("1"/"3");`fetch_nemo_messages(types: &str)` → `(message_type: NemoMessageType)`,消除静默 else。

7. **UploadChannel 枚举化**(acquire.rs + services.rs)
    - acquire.rs 新增 `pub enum UploadChannel { Pgaot, Codegame, Codemao }` + `parse(s) -> Option`;`FileUploader::upload(file_path, method: &str, save_path)` → `(file_path, channel: UploadChannel, save_path)`,match 穷尽(删 `_` 分支)。
    - services.rs:`handle_file_upload`/`handle_directory_upload` 的 `method: &str` → `channel: UploadChannel`;`uploader.upload(file_path, channel, save_path)`。
    - 无字符串调用点,`parse` 为备用 API(非法字符串沿用原错误语义)。

8. **ReportAction 枚举化**(registry.rs + services.rs + terminal.rs)
    - registry.rs 新增 `pub enum ReportAction { Delete, Mute7d, Mute3m, Unpublish, Pass, CheckViolation, Skip }` + `key()`("D".."J") + `from_key(s) -> Option`。
    - services.rs:`apply_action(item, action: &str, ...)` → `(item, action: ReportAction, ...)`;`apply_group(group, action: &str, ...)` → `(group, action: ReportAction, ...)`;`save_group_action(group, action: &str)` → `(group, action: ReportAction)`;内部传 `action.key()` 给 &str 契约方法(`execute_action`/`is_action_available`/`save_batch_action`)。
    - terminal.rs:`"P"` → `ReportAction::Pass`(3 处);`&key`/`&saved` → `ReportAction::from_key(&key)` 映射,None 时 warn + 跳过(防御注册表扩展);`st.record(action.key(), n)` 保持字符串统计键。
    - registry 注册键字符串 `actions(&[...])` 保留(注册契约)。

9. **保留 bool**(未改):`auto_reconnect(enabled)`/`save_raw(on)`/`with_log_requests(log)`(纯 builder 配置)。

### README 示例同步(6 处)

| #   | 改动                                                                                                                             |
| --- | -------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `work["name"]` → `work.get("name").and_then(serde_json::Value::as_str).unwrap_or("")`;`fetch_all_works_gen(Some(50))` → `(None)` |
| 3   | `get_ranking("score", 10, 0)` → `get_ranking("score", 10, RankingOrder::Descending)` + use 行                                    |
| 4   | `send_and_wait("你好", true, ...)` → `send_and_wait("你好", HistoryMode::Exclude, ...)` + use 行                                 |
| 5   | `decompile_work(123456, None)` 后注释 None 语义;compiler.rs doc 补说明                                                           |
| 6   | `apply_action(&item, "P", admin_id)` → `apply_action(&item, ReportAction::Pass, admin_id)` + use 行                              |
| 2   | 无问题,未动                                                                                                                      |

## Verification(实际执行结果)

- 每步 `cargo check` 0 error;最终 `cargo check --all-targets` 通过,`cargo test` 3 passed,`cargo clippy --all-targets` 0 warning。
- 归零验证:
    - `grep -rn "return_data" src/` → 0
    - `grep -rn "include_history" src/` → 0
    - `grep -rn '"like"\|"unlike"' src/api/` → 0(仅注释)
    - `grep -rn "order: i64" src/` → 0
    - `grep -rn "SelectMethod" src/` → 0
- 行为等价声明:
    - ResponseMode/HistoryMode/RankingOrder/ToggleAction/UploadChannel:纯签名替换,内部逻辑逐字等价。
    - ReportAction:apply 系列经 `key()` 转 &str 后与旧逻辑一致;terminal 未知键从"透传"变为"warn 跳过"(防御性改善)。
    - LogoutMethod:v0/admin 字符串实参原本会拼出错误端点,现编译期禁止(行为改善)。
    - NemoMessageType:消除静默 else("like"→1 保持,其余 2 种语义枚举化)。

## Critical files & anchors（执行后）

- `src/utils/acquire.rs` — `ResponseMode`/`ToggleAction`/`UploadChannel` 枚举区;`send_maybe_parse`/`upload` 签名。
- `src/core/registry.rs` — `ReportAction` 枚举 + `action_name`(保留)。
- `src/core/services.rs` — `apply_action`/`apply_group`/`save_group_action`/`handle_file_upload` 签名。
- `src/core/converse.rs` — `HistoryMode` 枚举 + `send_message`/`send_and_wait`。
- `src/core/cloudvar.rs` — `RankingOrder` 枚举 + `get_ranking`。
- `src/api/{work,forum,library,community,auth}.rs` — toggle 系列/NemoMessageType/LogoutMethod。
- `README.md` — 示例 1/3/4/5/6 同步。

## 未执行/保留

- builder 开关 bool(auto_reconnect/save_raw/with_log_requests)保留。
- `actions(&["D","S",...])`/`action_config.key`/`"execute_process_*"` 注册键字符串保留(注册契约)。
- `UploadChannel::parse` 备用(当前无字符串调用入口)。
