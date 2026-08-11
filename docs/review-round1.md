# 代码评审报告 — 优化建议(冗余 / 效率 / 组织)

评审日期:2026-08-10 · 范围:全仓 24 个 Rust 源文件(24,834 行)· 方法:7 个并行评审子代理按文件簇分工,主代理逐条核实高优先级发现并去重合并。

## 1. 总览

同步客户端/控制台后端:主流程在 `main.rs`(管理员登录 → 举报处理控制台),API 层(`api/`,13 个业务域)通过 `utils/acquire.rs` 的 HTTP/WS 客户端访问远程服务,`core/` 承载编译器/云变量/检索/流水线等核心逻辑。三层边界基本清晰,但存在三类系统性问题:

- **冗余**:api 层约 80 处手写"send + parse"样板绕过已有的 `ClientAccess` 默认方法;同构函数/枚举/管理器样板大量复制。
- **效率**:日志路径在 Info 级别下仍做 pretty-print;分页迭代双重克隆;多处 N+1 串行 HTTP 请求。
- **组织**:`compiler.rs`(4,101 行)、`acquire.rs`(1,842 行)、`work.rs`(2,512 行)三个 god file;入口层持有本应属于 api 层的登录/解析逻辑。

共 120 条发现,合并去重后 97 条(严重缺陷 7、冗余 42、效率 21、组织 27)。全部高优先级发现已经逐条对照源码核实。

## 2. Top 10 优先建议

| # | 建议 | 位置 | 类型 |
|---|---|---|---|
| 1 | 修复多账号举报身份错配:`ensure_account_login` 用 `account_usage[idx]>0` 短路跳过登录,而 token 存在全局单槽,实际请求携带最后登录账号身份 | `core/pipeline.rs:891-899` | 严重缺陷 |
| 2 | 修复 `flush_loop` 静默丢弃命令:`tx.is_none()` 时 `continue` 丢弃整批,3 处 `send(frame)` 忽略失败返回值,与"未就绪即重排队"语义不一致 | `core/cloudvar.rs:2457-2490` | 严重缺陷 |
| 3 | 全 api 层统一接入 `ClientAccess::send_and_parse / check_status / send_maybe_parse`,消灭约 80 处内联样板 | 见 §6.1 | 冗余 |
| 4 | 修复防水墙票据丢弃与手机号 i32 溢出两个缺陷 | `api/captcha.rs:127-141`、`api/account.rs:653-700` | 严重缺陷 |
| 5 | 删除/合并同构函数与死代码:`fetch_kn_work_state`、`list_user_databases`×2、`handle_password_v0/v1/v2`、`MutationDecompiler` 三重复、`get_total_reports`、`HTTPStatus` 枚举、`PaginatedIter` 7 个死方法 | 见 §3 | 冗余 |
| 6 | 抽公共 helper:`post_empty`(account×10)、`toggle`(work×10)、`fetch_editor_update`(community×4)、`item_config`(services×12)、`build_paginated` 收进 acquire(4 文件) | 见 §3 | 冗余 |
| 7 | 日志块改 `log_enabled!(Debug)` 守卫(当前 `log_requests` 默认 true 而 main 只开 Info,每次请求白做 pretty-print);`PaginatedIter` 消除逐页整数组克隆+逐元素克隆 | `utils/acquire.rs:823-844, 1252-1256` | 效率 |
| 8 | `retrieve.rs` 两处 N+1 串行请求改有界并行(复用本文件 `thread::scope` 先例);`DataStore` 4 个查找函数去 contains_key 双哈希 | `core/retrieve.rs:679-740, 865-930`、`core/cloudvar.rs:427-464` | 效率 |
| 9 | 宏化管理器样板(`impl_api_manager!`/`client_struct!`):全仓约 33 个 struct 的 `new()/Default/ClientAccess` 三件套 | 见 §6.3 | 冗余 |
| 10 | 拆分 3 个 god file(`compiler.rs`/`acquire.rs`/`work.rs`)+ `community.rs` 杂项;`main.rs` 登录改委托 `AuthManager`、`value_to_i64` 上移 utils 层 | 见 §5 | 组织 |

## 3. 冗余(42 条,按严重度)

### 高(值得立即改)

- **`api/work.rs:1714-1721` 与 `:1760-1767`** — `fetch_kn_work_state` 与 `fetch_work_status` 请求同一端点 `/neko/works/status/{id}`,方法体逐行相同,且全仓无调用点。删除其一(保留 `fetch_work_status`)。已核实。
- **`api/work.rs:154-206`(+335-377、517-537、1151-1163)** — 10 处"空负载 toggle"样板(`build_request + with_payload(json!({})) + check_status`,仅端点和期望状态码不同)。抽私有 `fn toggle(&self, method, endpoint, expected) -> MewResult<bool>`。
- **`core/compiler.rs:3650-3654`** — `create_block_decompiler` 把 text_join/ask_and_choose/text_select_changeable 分派到 3 个结构体,其 `decompile` 体(3261-3388)与 `MutationDecompiler`(3617-3640)逐字相同。删除 3 结构体,统一构造 `MutationDecompiler`。
- **`core/compiler.rs:1173-1179`** — `process_next/process_children/process_conditions/process_params/FunctionCallDecompiler` 五处重复"`blocks.insert` → `get_mut` 补 parent_id → insert_connection"收尾,`layout_col` 缩进包裹另重复 4 次。在 `BlockContext` 加 `insert_child_block(parent_id, block, conn)`。
- **`core/compiler.rs:1833-1975`** — `decompile_actor_blocks` 与 `decompile_scene_blocks` 前半段几乎逐行重复(仅 actor_info/variable_map 不同)。参数化合并为一个 `decompile_entity_blocks`。
- **`core/cloudvar.rs:1921-1930` 与 `:1946-1983`** — `UpdatePrivateVarHandler` 与 `UpdatePublicVarHandler` 的核心更新逻辑(取 cvid/value → `CloudValue::from_json` → `mem::replace` → `emit_variable_change`)逐字重复(外层:单条 vs 数组+`"fail"` 分支)。抽按 `VarKind` 参数化的共享函数,两个 handler 只做外层形状处理。已核实。
- **`api/account.rs:91-650`** — 10 个"`POST + json!({}) + send_and_parse`"同构包装(send_login_captcha、register_by_phone、login_by_phone 等),约 100 行。抽 `fn post_empty(&self, endpoint) -> MewResult<Value>`。已核实。
- **`api/auth.rs:556-616`** — `handle_password_v0/v1/v2` 结构相同(switch_identity → processor → 提取 token → set_token_and_identity → 构造 LoginResult),v1/v2 仅登录方式名不同,三段 `Err(MewError::Auth(format!("vN 登录失败…")))` 重复。抽公共私有方法(processor 闭包 + token 提取路径 + LoginMethod 参数)。
- **`api/auth.rs:331-403`(+460-480)** — `AuthProcessor` 6 处手写 `build_request(...).send()?` + `response_to_json`,与 `ClientAccess::send_and_parse` 等价;`get_login_security_info` 又手写状态码检查+`read_to_string`+`from_str`。为 `AuthProcessor` 实现 `ClientAccess`,补 `send_and_parse_with_error_body` 默认方法。见 §6.1。
- **`api/clouddb.rs:366-397`** — `list_user_databases` 与 `list_user_databases_detail` 函数体逐字相同,仅 endpoint 不同。合并为 `list_user_databases(db_type, detail: bool)`。已核实。
- **`api/community.rs:233-240`** — `fetch_nemo_messages(types: &str)` 用 `if types == "like" { "1" } else { "3" }` 映射 URL 段,非 "like" 输入静默按评论处理。定义 `NemoMessageType { Like, Comment }` 枚举 + `as_str()`,参数改枚举。
- **`api/forum.rs:76-101` 与 `api/shop.rs:41-51`** — `ForumReportReasonId`/`PostReportReasonId`/`WorkshopReportReasonId` 三枚举同构(Custom=0、Reason1..Reason8)。合并为公共 `ReportReasonId`。
- **`api/education.rs:166-186`、`:827-865`、`api/library.rs:652-657`** — 手写 `if return_data { response_to_json } else { Ok(json!({"success": status == expected})) }`,正是 `ClientAccess::send_maybe_parse` 的既有能力。整体替换为 `self.send_maybe_parse(builder, return_data, expected)`。
- **`api/whale.rs:160-170`** — `add_timestamp_to_builder` 本文件从未被调用(死代码),且与 `api/education.rs:380-390` 同名函数逐字重复;education.rs 中 `EduUserAction` 又不用该 helper 而手写 `with_param("TIME", …)`。删除死代码,helper 上提到 acquire.rs(见 §6.2)。

### 中

- **`core/cloudvar.rs:467-499`** — `create_private/create_public/create_list` 结构相同(插 cvid→name 映射,再更新或新建);新建路径 name 克隆两次。参数化合并 + `HashMap::entry` 消除多余克隆。
- **`core/cloudvar.rs:1588-1640`** — `list_apply_local` 与 `list_apply_cloud` 重复"仅在有变更回调时克隆旧表 + execute_list_action"块。抽 `fn apply_action(store, key, action)` 共享。
- **`core/cloudvar.rs:1058-1084` 与 `:1282-1310`** — `CloudConnection::list_pop/list_shift` 与 `CloudList::pop/shift` 的"锁内读 → 解锁 → 删除"两段式完全重复,且读-删间存在 TOCTOU 竞态。在 `CloudInner` 提供原子"读+执行动作"辅助。
- **`core/cloudvar.rs:2179-2185`(+2474-2490)** — `"42" + serde_json::to_string((name, payload))` 事件帧构造重复 4 处,`flush_loop` 内 `unwrap` 绕过错误路径。抽 `fn event_frame(name, payload) -> Result<String, _>`。
- **`core/cloudvar.rs:978-999`** — `get_all_private_variables` 与 `get_all_public_variables` 除映射字段外完全相同。合并为 `get_all_variables(kind: VarKind)`。
- **`core/cloudvar.rs:2323`(另 1827)** — `ConnectionEvent::Opened` 在 establish(WS 升级成功)与收到 `"40"` 两处各触发一次,订阅方每次连接收到两次 Opened。只保留一处(建议保留 `"40"` 确认处)。
- **`core/retrieve.rs:291-296` 与 `:400-405`** — `extract_reply_user_id` 闭包(含 user_field 选择 match)完全重复。提为模块级 `fn reply_user_id(reply, user_field)`。
- **`core/retrieve.rs:280-386`** — `stream_user_ids` 与 `stream_comment_ids` 骨架逐段相同(flat_map、错误传播、reply_items 迭代),仅"推入什么 ID"不同,约 50 行。抽公共评论遍历辅助 + ID 提取闭包参数。
- **`core/retrieve.rs:570-601`** — `count_comments` 三个 configure 闭包:Work 与 Forum 分支完全一致,Shop 仅多两个 iter_param。抽基配置函数。
- **`core/retrieve.rs:795-860`** — `compute_admin_report_stats` 线程闭包内 comment/work 两段重复"fetch_metadata → total_items → try_from → ParseError"。抽 `fn total_of(paginated, label)`。
- **`core/services.rs:315-317`** — `infer_report_type(item)?` + `registry.get_config(report_type)?` 两行样板在约 12 处重复(item_view、check_violations、apply_action、mark_decided、done_row 等)。抽 `fn item_config(&self, item) -> Option<(&str, &SourceConfig)>`。
- **`core/services.rs:486-497`** — `pass_all` 手工"get_report_id → apply → mark_record_processed",与 `execute_action`(391-416)的序列重复。循环内直接调 `execute_action(report_type, cfg, &item, "P", admin_id)`。
- **`core/services.rs:516-520` vs `:532-537`** — `group_saved_action(group)` 与 `saved_action_for_key(group_type, group_key)` 完全同构。前者改为委托后者。
- **`core/services.rs:658-666` vs `:704-716`** — `done_row` 与 `done_item_details` 各自实现"admin_username_field 非空优先,否则回退 admin_id_field"。抽 `fn admin_label(item, config)`。
- **`core/registry.rs:680-698`** — `get_total_reports` 无生产调用方(仅单元测试引用),并行求和逻辑与 `get_totals_pair`(701-727)重复。删除该函数及其测试(788-805)。
- **`core/registry.rs:426-440`** — 四个举报类型注册块重复 `total_from(...)` + `gen_from(...Some(100))` 闭包对。抽工厂 `fn handlers(f: impl Fn(ReportStatus) -> PaginatedIter) -> (FetchTotal, FetchGenerator)`。
- **`api/auth.rs:331-339` vs `api/user.rs:291`** — `fetch_auth_details`(手拼 Cookie authorization 头)与 `fetch_account_details`(走客户端默认认证)请求同一端点 GET /web/users/details。合并为共享方法,`token: Option<&str>` 时附加 Cookie 头。
- **`api/auth.rs:1119-1127` vs `utils/acquire.rs:1673`** — `generate_client_id` 与私有 `generate_meow_id` 实现逐字相同(fastrand 随机字符)。公开 `generate_meow_id` 加 charset 参数,删除 auth.rs 副本。
- **`api/auth.rs:852-886`(+805-850)** — `get_user_login_method` 与 `get_admin_login_method` 除最终 `determine_*` 外逐字相同;`validate_login_parameters` 又重复角色匹配检查。抽 `non_empty()` 与按角色参数化的 `resolve_method(role, credentials, prefer)`。
- **`api/user.rs:632-648` vs `:670`** — `fetch_published_works` 与 `fetch_user_collections` 结构逐字相同(types→types_str 拼接 + 查询参数 + send_and_parse),仅 endpoint 不同。合并为 `fetch_works_with_types(endpoint, user_id, types, limit)`。
- **`api/user.rs:469-491`(+508-533、610-621)** — 6 个分页迭代器重复固定参数组合(`with_page_size(30)` + work_status/published_status + `limit.unwrap_or(30)`)。抽 `build_creation_works_iter(...)` 与 `fetch_follows_gen(endpoint, user_id, limit)`。
- **`api/work.rs:932-941`(+1104-1132、1166-1191)** — 三条完整分页链内联(`with_iter_param` + total/data key + PaginationMethod::Page),与同文件 `build_paginated`(1649)能力重叠。统一走 `build_paginated`。
- **`api/work.rs:1508-1520` vs `:2359-2378`** — `PackageManager::fetch_package_list` 与 `fetch_resource_pack` 请求同一端点 `/neko/package/list`,却分别用 20 与 16(注释标明"单页上限")分页。合一并统一页大小。
- **`api/work.rs:242-2421`** — `with_param("TIME", timestamp.to_string())` 重复约 29 处(education.rs 约 15 处)。在 `KittyRequestBuilder/PaginatedIter` 加 `with_time()`/`with_iter_time()` 全仓替换。
- **`api/work.rs:2162-2217`** — 三个 KN 作品列表迭代器重复 name/work_business_classify 参数与 24 页大小组装。抽 base builder,三个方法只传 status/端点差异。
- **`api/education.rs:783-820`(+747-764)** — `fetch_lesson_topics` 与 `fetch_lesson_tags` 除端点外完全相同,共用 magic 参数 `pacakgeEntryType=0`/`topicType=all`。抽 `fetch_lesson_meta(endpoint)` + 常量。
- **`api/education.rs:911-1031`** — 11 个分析类 fetch 方法逐字同构(GET + BaseKey::Education + add_timestamp + send_and_parse),仅端点不同。抽 `fn fetch_edu_get(&self, endpoint)`。
- **`api/whale.rs:202-296`** — 四个举报迭代器(fetch_post/discussion/work/comment_reports_gen)同一骨架,仅 endpoint 不同。抽 `build_report_iter(endpoint, ...)`。
- **`api/community.rs:265-337`** — `fetch_kitten4/kitten/wood_editor/matrix_editor_update` 四函数仅 URL 与时间戳参数名("TIME"/"timeStamp")不同,重复 raw_timestamp_10+extract_time_string。抽 `fetch_editor_update(url, time_param)`。
- **`core/converse.rs:580-585`** — `generate_session_id` 与 `core/compiler.rs` `IdGenerator::generate`(747-760)同为 fastrand 随机 ID。统一随机工具。

### 低

- **`core/compiler.rs:601-618`** — `WorkType::is_kitten/is_nemo/is_neko/is_coco/is_wood` 全仓无调用。删除。
- **`core/compiler.rs:67-142`** — `ValueExt::get_str/get_i64/get_bool/get_object/get_array/get_str_opt` 全仓无调用(在用的是 get_str_or/get_i64_or_default 等)。删除 6 个死方法。
- **`core/compiler.rs:3666-3668`** — `procedures_2_stable_parameter/procedures_2_parameter` 分支与 `_` 兜底返回相同。删除显式分支。
- **`core/compiler.rs:3674-3693`** — `BlockDecompilerFactory` 的 config/id_generator 字段从未使用,`create()` 仅转发自由函数。删除结构体,4 个调用方直接调 `create_block_decompiler`。
- **`core/compiler.rs:762-768` vs `api/auth.rs:1153-1157`** — `CryptoService::sha256` 与 auth.rs 内联 SHA256→hex 逻辑重复(仅大小写)。抽公共 hex helper。
- **`core/retrieve.rs:129-160`(+430-460)** — `build_compact_reply` 内 5 处 `.and_then(as_str).unwrap_or("").to_string()` 重复。抽 `fn str_field(obj, key) -> String`。
- **`api/auth.rs:90-130`** — `UserRole::as_str/from_str`、`AccountStatus::as_str/from_str` 全仓无调用。删除或改实现 `FromStr/Display`。
- **`api/work.rs:33-45`** — `PublishStatus` 枚举全仓无使用,且与 `api/user.rs:61-75` 同名枚举重复(user.rs 版多 `All`)。删除 work.rs 版。
- **`api/work.rs:1594-1609`** — `fetch_sample_detail` 全仓无调用,且 `params: Vec<(String,String)>` 形参迫使调用方堆分配。删除。
- **`api/work.rs:2226-2251`** — `search_works_by_name_web` 与 `_nemo` 仅端点(`/nemo/community/...` vs `/nemo/v2/...`)与参数名(query vs key)不同。合并为私有 helper。
- **`api/user.rs:147-264`** — 7 处"`format!(endpoint)` → build_request(Get) → send_and_parse"三行组合。抽 `fn get_json(&self, endpoint)`。
- **`api/account.rs:431-438`** — `fetch_agreements` 手写 `send()+response_to_json`,而同文件其余 40+ 方法用 `send_and_parse`。统一。
- **`api/captcha.rs:19-148`** — `CaptchaManager` 全部 12 个方法为 4-8 行同构"debug! + build_request + send_and_parse"包装,148 行中约 120 行样板。抽 `fn request(&self, method, endpoint, payload)`。
- **`utils/acquire.rs:1770-1795`** — `KittyFactory::create_global_client/create_independent_client/create_file_uploader/global_identity_manager` 全仓无调用(仅 `global_client()` 在用)。删除 4 个方法。
- **`utils/acquire.rs:408-474`** — `GlobalKittyAuth` 与 `LocalKittyAuth` 两个 `KittyAuth` 实现逐方法重复委托(4 方法 × 2 份)。统一为持有 `Arc<KittyIdentityManager>` 的单一实现。
- **`utils/acquire.rs:1546-1550` 与 `:1634-1637`** — `extension().map(|e| format!(".{}", ...)).unwrap_or_default()` 表达式复制两份。抽 `fn ext_with_dot(path)`。
- **`core/cloudvar.rs:1360-1370`** — `CloudList::join` 先 collect Vec 再 join。直接 fold 拼接。
- **`core/terminal.rs:753-824`** — `pick_type` 与 `pick_status` 菜单函数结构几乎相同。合并为泛型 `pick_option(title, options, current)`。

## 4. 效率(21 条,按严重度)

### 高

- **`utils/acquire.rs:823-844`(+651-684、847-862、865-896)** — `response_to_json`/`log_request` 以 `config.log_requests`(默认 **true**,178-185)为开关执行 `serde_json::to_string_pretty` 美化请求/响应体并遍历响应头,而 `main.rs:22` 只设 `LevelFilter::Info`,debug! 不会输出——每次请求这些格式串都被白白构造后丢弃。所有日志块改 `if log::log_enabled!(log::Level::Debug)` 守卫(或将 `log_requests` 默认置 false)。

### 中

- **`utils/acquire.rs:1252-1256`(+1330)** — 分页双重克隆:`extract_page_data` 对整页数组 `.cloned()`(page_size 最高 150,元素为完整 `serde_json::Value`),`next_item` 每产出一条再 `current_page_data[i].clone()`。改为从解析后的 JSON 中移动数组所有权,产出时 `mem::replace(&mut current_page_data[i], Value::Null)`。
- **`core/retrieve.rs:865-930`** — `compute_fans_by_like_threshold` 逐粉丝串行同步 HTTP `fetch_user_honors`(N+1 请求)。本文件 `compute_admin_report_stats` 已有 `thread::scope` 并行先例——按 8-16 分片有界并发拉取。
- **`core/retrieve.rs:679-740`** — `aggregate_user_comments_from_works` 逐作品串行拉取评论流(N+1 HTTP 往返)。作品列表分片并发拉取后按 user_id 聚合。
- **`core/cloudvar.rs:427-464`** — `variable_in/variable_ref/list_mut/list` 四处先 `contains_key` 再 `get/get_mut`,同一 key 两次哈希查找,位于 get/set/回调高频路径。改 `if let Some(v) = vars.get_mut(key) { return Some(v); }`。
- **`core/cloudvar.rs:1642-1700`** — `fire_list_outcome` 每次列表操作对 state 互斥锁 5 次(取回调/放回/克隆新表/取整表回调/放回)。一次临界区内完成"取全部回调 + 克隆新表",锁外执行回调后一次性放回。
- **`core/retrieve.rs:322`** — `stream_user_ids` 对每条评论整对象 `.cloned()`(Forum 源根本不用该对象,Work/Shop 无回复时也白克隆;对照 `stream_comment_ids:359` 用引用零克隆)。改传引用。
- **`core/pipeline.rs:784-822`** — `check_spam_posts` 先全量收集搜索结果再过滤;命中 `spam_threshold=3` 后仍继续拉取剩余页面。边收边匹配,达到阈值立即 break。
- **`core/pipeline.rs:757-763`** — `count_comments` 与 `fetch_detailed_comments` 两次独立请求,且 total 仅用于 info 日志。去掉前者或顺带统计。
- **`api/community.rs:265-337`** — 四个更新接口各自先发一次 `/coconut/clouddb/currentTime` 往返;批量检查更新时同一时间戳被请求 4 次。提供一次取时间戳+参数化请求的组合接口。
- **`utils/acquire.rs:716-737`** — `KITTY_HEADERS`(4 个头)每请求由 `apply_to_request_builder` 循环添加;ureq `Agent::config_builder()` 支持 `default_headers`,应在 `KittyCore::new` 一次性配置。
- **`api/auth.rs:995-1008` 配合 `main.rs:58-66`** — `admin_login` 已请求 `/admins/info` 并把 `dashboard["admin"]` 存入 `result.auth_details`;main.rs 又无条件 `fetch_admin_details()` 重取一遍。优先从 `auth_details` 提取,None 时才回退。
- **`core/services.rs:758-764`** — `split_chunk` 对 chunk(默认 100 条)逐条 lock/unlock 检查 `is_record_processed`。循环前一次 lock,循环内复用 guard(该函数内无嵌套加锁,安全)。
- **`core/services.rs:609-620`** — `apply_group` 外层 `is_action_available` 检查与循环内 `execute_action` 的检查重复。删除外层(execute_action 已覆盖并返回 Err)。

### 低

- **`utils/acquire.rs:1220-1230`** — `build_params` 每翻一页 `self.base_params.clone()` 并重新 to_string 分页参数。构造携带固定参数+可替换 offset 槽位的 Vec,翻页仅替换 offset。
- **`utils/acquire.rs:332-335`(+673、724)** — `auth_header()` 每请求 2 次 `format!("Bearer {}", token)` 分配。在 `KittyCore::send` 计算一次,传入 log 与 apply。
- **`utils/acquire.rs:767-806`** — `send` 先 match method 分无体/有体两路,`bodyless_builder`/`bodied_builder` 内部又各自 match 同一 method,分发重复两遍。单次 match 直接构造对应 RequestBuilder。
- **`api/auth.rs:1151-1165`** — `generate_x_device_auth` 逐字节 `format!("{:02X}")` 拼 hex,32 次堆分配。预分配 `String::with_capacity(64)` 或引入 hex crate。
- **`core/compiler.rs:3116-3120`** — `reorganize` 中 `widgetMap` 整表 `.cloned()` 深拷贝再写 globalWidgets,与 3069 行写回内容重复。用同一局部变量产出两个字段。
- **`core/compiler.rs:2584-2590`** — `NemoResourceManager::get_sha` 每次 `url.to_owned()` 做 entry 查询,缓存命中也有一次分配。先 `cache.get(url)`。
- **`core/terminal.rs:882-886`** — `DoneFilter::matches` 对每条记录 `serde_json::to_string(item)` 全量序列化+to_lowercase,拉取与过滤时反复执行。在 chunk 拉取时一次性预计算检索串(仅 keyword 非空时)。
- **`api/shop.rs:89-116` 与 `:143-170`** — 两处各自实现 Option\<Vec>→逗号字符串拼接与默认值。抽 `join_or_default(opt, default)`。

## 5. 组织(27 条,按严重度)

### 高

- **`core/compiler.rs:1-4101`** — 全仓最大文件,塞入 15+ 组件:错误/配置/加密/阴影/积木反编译核心/9 个专用反编译器/XML 序列化/HTTP 客户端/注册表/门面。拆为 `core/compiler/{config,crypto,fetchers,decompilers,block,xml,registry}` 3-4 个模块文件。
- **`utils/acquire.rs:1-1842`** — 七类职责挤在单文件:身份管理(31-483)、HTTP 客户端(484-1019)、分页(1020-1452)、文件上传(1453-1676)、HTTP 状态枚举(1682-1765)、工厂/时间戳(1767-1804)、ClientAccess(1806-1842)。拆为 `utils/{client,identity,pagination,upload}.rs`,acquire.rs 只做 re-export。
- **`api/work.rs:1-2512`** — 全仓第二大文件:13 个管理器、10 余个参数结构体、13 个 ClientAccess impl。按业务域拆为 `src/api/work/` 子模块(kitten/neko/wood/coco/collaboration 等)。
- **`api/community.rs:1-856`** — 杂项 god 文件:CommunityDataFetcher 聚合更新检查/头图/配置/作品/课程/工作室/消息约 40 个跨 BaseKey 方法,UserAction 混入消息删除/协议签署。按职责拆分(UpdateFetcher/ConfigFetcher/BannerFetcher),与其余 6 个按业务域划分的文件对齐。

### 中

- **`core/services.rs:412-415`(+494-498、517-520、533-536、548-551、760-763)** — 提交 8e87466 声明锁处理统一为 `PoisonError::into_inner`,实际只改了 1 处,其余 6 处仍是 `lock().unwrap()`。全部改 `unwrap_or_else(PoisonError::into_inner)` 或抽 `fn lock_batch(&self)`。
- **`core/terminal.rs:589-751`** — `view_done` 单函数 160+ 行,糅合懒加载/过滤/分页渲染/命令分发四层。拆分渲染表头、应用过滤、命令分发。
- **`src/main.rs:91-136`** — `admin_login_with_retry` 手工编排登录(取验证码 → `handle_admin_password` → 重试),绕过 api/auth.rs 已提供的 `AuthManager`(765)/`LoginBuilder`(1186)统一入口,参数校验/令牌设置/错误映射在 main 与 api 层重复。main 只保留验证码重试循环,单次尝试委托 `AuthManager::login`。
- **`src/main.rs:141-158`(+58-66)** — 入口层持有本应属于 api 层的管理员信息解析:本地 `extract_admin_id` 与 `full_name` 提取硬编码 `/admins/info` 响应结构(auth.rs:995-1008 也各自解析一次)。在 auth.rs 提供 `AdminInfo::from_details(&Value) -> Option<AdminInfo{id, full_name}>`,main.rs 删除本地解析。
- **`src/main.rs:6,141-155`** — 入口直接 `use crate::core::registry::value_to_i64`(core 内部工具),分层倒挂。将 `value_to_i64` 移到 `utils/data.rs`,core 与 main 统一从 utils 引用。
- **`src/main.rs:22-89`** — main() 单函数 60+ 行承担登录编排/三段打印/ID 提取/控制台启动。抽 `print_login_result(&LoginResult)` 等,main 只留流程调用。
- **`api/community.rs:416-447` 与 `api/codegame.rs:30-37`** — 4 个 `/config` 类接口(fetch_nemo_config/fetch_community_config/fetch_client_config/fetch_platform_config)散落两文件。合并为 `fetch_config(base_key)` 或集中 ConfigFetcher。
- **`api/community.rs:585-590` 与 `api/forum.rs:315-333`** — `fetch_community_status`(?config_type=)与 `fetch_7day_hot_posts_gen`(?board_id=)把查询参数拼进 endpoint 字符串,同类方法均用 builder 参数。统一 `with_param/with_iter_param`,避免 URL 拼接与转义隐患。
- **`core/compiler.rs:3203-3206`** — 6 个专用反编译器额外持有 `compiled: &'a Value` 字段,与 `BlockDecompilerCore` 内部(1086)重复。改经 `self.core.compiled` 访问。

### 低

- **`core/terminal.rs:437-446`** — `process_item` 11 个参数 + `#[allow(clippy::too_many_arguments)]`。打包为 ProcessContext 结构体。
- **`api/work.rs:17-28`** — `SelectMethod` 枚举仅本文件使用,forum.rs:405 同功能用 `action: &str`。同一语义两种表达,统一。
- **`api/shop.rs:88,177,316-334`** — 两个空的 `// 私有辅助` 注释段;`create_workshop` 是唯一内联 send+parse 的方法,与相邻风格割裂。删空注释,改 `send_and_parse`。

## 6. 严重缺陷(7 条,全部已对照源码核实)

1. **[高] `core/pipeline.rs:891-899` 多账号举报身份错配** — `ensure_account_login` 中 `account_usage[idx] > 0` 即跳过登录,但所有学生账号登录写入同一个全局身份槽(token_bowl,`AccountStatus::Edu → Catsona::Scholar`),每次登录覆盖上一个账号的 token。轮询回到已用账号时跳过登录,`execute_single_report` 实际携带最后登录账号的 token,却把 success/usage 记在选中账号名下:身份错配 + 每账号上限保护失效(超额集中在最后登录账号,有封号风险)。改法:去掉 usage>0 短路、每次选中都重新登录;或按账号分批(整块处理完 A 再切 B)。已核实(源码 885-899)。
2. **[高] `core/cloudvar.rs:2457-2460`(+2474-2490) flush_loop 静默丢弃命令批次** — `let Some(tx) = ... else { continue; }` 在 tx 为 None(断线竞态窗口或 close 先 take 走 tx)时直接丢弃已合并批次;3 处 `send(frame);` 忽略闭包返回的 bool 失败。与"未就绪"分支的 push_front 重排队语义不一致。改法:tx 为 None 或 send 失败时把未发送帧重新 push_front 回队列。已核实。
3. **[中] `core/cloudvar.rs:766-775` connect() 并发双建** — `connected` 检查在 `connect_lock` 之外,多线程并发调用会双双通过检查并各自 establish,第二个 establish 覆盖 tx/read_join,第一个连接的读线程退出后经 `on_connection_lost` 拆掉新连接,触发重连风暴。改法:检查移入锁临界区或用 establishing 原子标志。已核实(检查在锁外)。
4. **[中] `api/captcha.rs:127-141` 防水墙票据校验丢弃传入票据** — `verify_waterproof_wall_ticket(_ticket: Value)` 参数以下划线开头、请求无 payload,而同族 `verify_geetest_slide_ticket`(113-124)把票据放进请求体。票据不参与校验。改法:与 geetest 一致 `.with_payload(ticket)`;若接口确不需要则删参数。代码事实已核实;对服务端语义为 [INFERENCE](仓库内无调用方)。
5. **[中] `api/account.rs:653-700` 手机号用 i32 承载** — `update_phone_number(phonenum: i32)`、`validate_phone_number(phone_num: i32)`、`execute_request_phone_change_verification(old/new: i32)`。11 位手机号(13800138000 = 138 亿)远超 i32::MAX(21.47 亿),parse 失败或 `as` 截断,请求体出现错误号码。与同文件 `check_phone(604, phone: &str)` 保持一致改用 &str。
6. **[中] `core/compiler.rs:2086-2110` write_blocks 引用收集形态不一致** — 内联引用收集只识别对象形式(`next_block.get("id")`),而同文件 `collect_referenced_ids`(1742-1761)同时支持字符串 id 形式;一旦数据出现字符串形式引用,该块被当根块重复输出,且 block_xml 的 next 链(2206-2212,要求 `nb.is_object()`)同样断链。统一两处引用收集并让 block_xml 支持字符串 next。代码事实已核实;触发依赖数据形态(该路径数据未在仓库内出现)。
7. **[中] `core/pipeline.rs:906-920` 账号移除后 usage 下标错位** — 登录失败时 `accounts.remove(idx)`,后续账号整体左移,但 `account_usage` 仍按旧下标记账;`select_report_account`/`ensure_account_login`/计数全部按索引,同一账号的用量被拆到多个键,`max_reports_per_account` 上限可被突破。改法:以账号用户名为 usage 键,或在移除后重建映射。已核实(remove + remove(&idx) 仅清当前键,其余键错位)。

## 7. 跨文件共性模式(建议的公共归属)

1. **手写 send+parse 样板(约 80 处)绕过 `ClientAccess`** — community.rs(~25)、clouddb.rs(~20)、forum.rs(~10)、auth.rs(6)、education.rs(7)、codegame.rs(2)、shop.rs(1)、library.rs(1)、account.rs(1)。归属:`utils/acquire.rs` 的 `ClientAccess` 默认方法(send_and_parse/check_status/send_maybe_parse),给缺失的 ~12 个 struct 补 impl。
2. **`build_paginated` 私有副本 ×4** — work.rs:1649、education.rs:393、forum.rs:138、user.rs:132,签名一致。归属:acquire.rs 公共构造(参数化 offset/amount/total/data key 与 BaseKey)。
3. **时间戳 helper 重复 ×5** — auth.rs:431-437/1133-1147(手写 SystemTime + `.expect` panic 点)、whale.rs:160(死代码)、education.rs:380-390、`current_timestamp_13`(acquire.rs:1799)。归属:acquire.rs 增加秒/毫秒 i64 版,删除全部手写副本与 panic 点。
4. **struct new()/Default/ClientAccess 三件套 ×约 33 处** — work.rs 13 个管理器、misc api 18 个 struct、education.rs 2 个。归属:`impl_api_manager!`/`client_struct!` 宏(acquire.rs)。
5. **`truncate` ×3** — converse.rs:969、cloudvar.rs:2254、pipeline.rs:99(truncate_chars 变体),且 `chars().count()` 重复计算两遍。归属:`utils/data.rs` 文本工具。
6. **WS 工具 ×2** — converse.rs 与 cloudvar.rs 各自复制 `Notify`/`wait_flag`/`set_stream_read_timeout`。归属:新建 `core/ws_util.rs`。
7. **随机 ID 生成 ×3** — auth.rs `generate_client_id`、acquire.rs `generate_meow_id`、converse.rs `generate_session_id`、compiler.rs `IdGenerator`。归属:统一 `generate_random_id(length, charset)`。
8. **SHA256→hex ×2** — compiler.rs `CryptoService::sha256`、auth.rs:1151-1165(顺带 32 次堆分配)。归属:utils 公共 hex helper。
9. **`HTTPStatus` 枚举(34 变体,~85 行)与 `ureq::http::StatusCode` 重复** — acquire.rs:1683-1765;`reason_phrase`/`From`/`Display` 全仓无调用方。全仓 `HTTPStatus::X as u16` 替换为 `StatusCode::X`,删除枚举(注意 `[lints.rust] unused="allow"` 掩盖了死代码,建议恢复默认告警)。
10. **举报原因枚举 ×3** — forum.rs `ForumReportReasonId`/`PostReportReasonId`、shop.rs `WorkshopReportReasonId`。归属:公共 `ReportReasonId`。

## 8. 评审方法

- 7 个 `reviewer` 子代理并行,按文件簇分工:`core/{compiler,converse,terminal}`、`core/{cloudvar,retrieve}`、`core/{pipeline,services,registry}`、`api/{work,education}`、`api/{auth,account,user,captcha}`、`api/{community,forum,library,shop,clouddb,whale,codegame}`、`utils/{acquire,data}+main.rs`,每簇覆盖全部三维度 + 顺带标记可确证缺陷。
- 覆盖检查:24 个源文件与 7 个簇的并集一致(模块桩文件 api.rs/core.rs/lib.rs/utils.rs 无逻辑)。
- 核实:全部高优先级与全部严重缺陷发现已逐条 `read`/`grep` 对照源码;中/低优先级抽查 ≥30%。标注 [INFERENCE] 的条目为代码事实已核实、外部语义(服务端 API 契约/数据形态)未确认。
- 去重:跨代理重复发现(truncate、时间戳 helper、send_maybe_parse 分支、管理器宏、内联 send+parse、`/admins/info` 相关)已合并,合计 120 → 97 条。
