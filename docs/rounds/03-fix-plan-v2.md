# 修复方案 v2 — 评审整改(按 11 条回退约束重写)

基线:commit `8e87466`,工作树干净。`cargo check` 基线通过(29.5s,见 temp/FIX_PLAN.md 记录;执行时先确认)。

## Context

对 `temp/REVIEW.md` 的 97 条评审发现做整改。v1 方案( `temp/FIX_PLAN.md`)被否:过度抽象(宏、内部 helper、endpoint 参数)、过度删除(死代码)、过度拆分(mod.rs 目录)。本方案按 11 条指令重写:只做 6 条严重缺陷修复(第 6 条 compiler.rs `write_blocks` 放弃,理由见 Verification §5)、管理员信息固定字段提取、api 层改用 acquire.rs 既有原生 `ClientAccess` 方法、5 处无争议死代码删除、若干零抽象效率优化。**不新增任何抽象层,不拆分文件,不新增共享工具函数。**

### 明确不做(用户指令 + 因此放弃的评审条目)

| 指令                                                    | 落实                                                                                                                                                                                                                                                               |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1 回退 impl_api_manager                                 | 不定义宏;各 struct 的 `new()/Default/ClientAccess` 保持手写                                                                                                                                                                                                        |
| 2 回退 post_empty/request/get_json/build_report_iter 等 | api 层不新增任何私有辅助函数;重复样板统一改用 acquire.rs 既有 `ClientAccess` 默认方法(见 Phase 2 替换规则)                                                                                                                                                         |
| 3 管理员信息固定字段                                    | 只读 `/admins/info` 响应的 `admin.id / admin.username / admin.role_name / admin.full_name`,无形态回退链                                                                                                                                                            |
| 4 fetch_auth_details 留在 auth.rs                       | 不合并进 user.rs `UserDataFetcher::fetch_account_details`,原样不动                                                                                                                                                                                                 |
| 5 回退 with_time/with_iter_time                         | 不新增这两个方法;现有 `with_param("TIME"/"CMTIME"/"timeStamp", …)` 调用原样保留(education/community/work 等)                                                                                                                                                       |
| 6 build_paginated 即可                                  | 不新增 `build_page_paginated`;4 个文件内的私有 `build_paginated` 副本原样保留(它们附加 base_key/Page 分页/limit 等域内默认,属既有清晰代码)                                                                                                                         |
| 7 删掉 join_or_default                                  | shop.rs 两处 `Option<Vec>` 拼接保持内联                                                                                                                                                                                                                            |
| 8 api 层不要 endpoint 参数                              | 不创建 `fetch_follows_gen`/`fetch_works_with_types`/`fetch_editor_update`/`fetch_lesson_meta`/`fetch_edu_get`/`build_report_iter`/`list_user_databases(db_type,detail)` 之类;clouddb.rs 两个 list_user_databases 保持两个显式函数                                  |
| 9 不拆分文件                                            | 零文件拆分、零目录/mod.rs;维持现有扁平组织                                                                                                                                                                                                                         |
| 10 generate_meow_id 独立                                | acquire.rs `generate_meow_id`(1673)原样;auth.rs `generate_client_id` 原样;不建 `generate_random_id`                                                                                                                                                                |
| 11 保留 PaginatedIter 死代码 / KittyFactory 死方法      | `with_config/with_response_offset_key/current_page_number/yielded_count/total_pages/page_size()/remaining_items` 与 `PaginationConfig.response_offset_key` 保留;`create_global_client/create_independent_client/create_file_uploader/global_identity_manager` 保留 |

其余放弃项(评审提出但本方案不做):HTTPStatus 迁移到 `ureq::StatusCode`、`handle_password_v0/v1/v2` 合并、`ReportReasonId`/`NemoMessageType` 枚举合并、`fetch_nemo_messages` 枚举化、`GlobalKittyAuth/LocalKittyAuth` 去重、`send` 分发去重、`build_params` 缓存、`event_frame`、`CloudList::join` fold、`truncate`/`str_field`/`hex_encode` 等共享工具、compiler.rs 的 `insert_child_block`/`MutationDecompiler` 合并/`decompile_actor_scene` 合并/`ValueExt`/`WorkType` 死方法删除、`main.rs` 委托 `AuthManager::login`、`auth_details` 复用省一次请求(保留 main.rs 显式 `fetch_admin_details()`,启动时多一次请求可接受)。

## Approach

### Phase 0 — 基线确认(主代理)

1. `cargo check` 必须 0 error 后再动手。若失败,先修基线。

### Phase 1 — 管理员信息固定字段提取(主代理直改,3 文件 + main)

同一编译单元的三处联动,由主代理一次完成,随后 `cargo check` 验证:

1. **`src/utils/data.rs`**:新增 `pub fn value_to_i64(v: &serde_json::Value) -> Option<i64>`,函数体从 `src/core/registry.rs:41-47` 原样搬入(逐字复制,含注释)。
2. **`src/core/registry.rs`**:删除 `value_to_i64` 定义(约 41-47 行)。
3. **`src/core/services.rs` 第 21 行**:`use crate::core::registry::{…, value_to_i64, …}` 中移除 `value_to_i64`,新增 `use crate::utils::data::value_to_i64;`。
4. **`src/api/auth.rs`**:新增管理员信息结构(放在"数据结构"区,`LoginResult` 附近):

```rust
/// 管理员信息(`GET /admins/info` 响应的 `admin` 对象)
#[derive(Debug, Clone)]
pub struct AdminInfo {
    pub id: i64,
    pub username: String,
    pub role_name: String,
    pub full_name: String,
}

impl AdminInfo {
    /// 从 `GET /admins/info` 响应中提取管理员信息。
    /// 固定读取 `admin` 对象下的 id/username/role_name/full_name 四个字段,
    /// 不做任何形态回退尝试;任一字段缺失或类型不符时返回 None。
    pub fn from_details(details: &Value) -> Option<AdminInfo> {
        let admin = details.get("admin")?;
        Some(AdminInfo {
            id: value_to_i64(admin.get("id")?)?,
            username: admin.get("username")?.as_str()?.to_string(),
            role_name: admin.get("role_name")?.as_str()?.to_string(),
            full_name: admin.get("full_name")?.as_str()?.to_string(),
        })
    }
}
```

文件头加 `use crate::utils::data::value_to_i64;`(auth.rs 已有 `use crate::utils::acquire::{…}`;若 `use` 列表冲突按现有风格并入)。

- 同文件追加一行 `impl ClientAccess for AuthProcessor { fn client(&self) -> &CodeMaoClient { self.client_provider.client() } }`(放 `impl Default for AuthProcessor` 附近;同时把 `ClientAccess` 加进文件头 `use crate::utils::acquire::{…}` 导入列表),并按总规则模式 A 替换 AuthProcessor 的 5 处字面样板:`fetch_admin_details`(342-348)、`get_login_ticket`(350-368)、`authenticate_admin_user`(407-427,含 `.with_error_body()`,属 builder 配置,等价)、`handle_password_v0`(465-477)、`handle_password_v1`(484-497)。豁免不动:`fetch_auth_details`(指令 #4)、`get_login_security_info`(自读 body)、`fetch_admin_captcha`(二进制)、`fetch_current_timestamp_with_provider`、三个 logout(204 检查,AuthManager 不接入 ClientAccess)。

5. **`src/main.rs`**:
    - 第 9 行 `use crate::core::registry::value_to_i64;` 删除;第 8 行 auth 导入追加 `AdminInfo`。
    - 删除整个 `extract_admin_id` 函数(141-158 行)及其 doc 注释。
    - 第 58-66 行"提取管理员 ID"块替换为:

```rust
    let admin_info = match AdminInfo::from_details(&admin_details) {
        Some(info) => info,
        None => {
            eprintln!("无法从管理员信息中提取管理员ID(缺少 admin.id/username/role_name/full_name), 请检查接口返回");
            return;
        }
    };
    println!("管理员: {} (ID {})", admin_info.full_name, admin_info.id);
```

`admin_details` 的获取(第 49-57 行 `AuthProcessor::new().fetch_admin_details()`)与打印(59-61 行)不动;后面 `i32::try_from(admin_info.id)` 逻辑不动。`value_to_i64` 只在 `AdminInfo::from_details` 内使用,不再出现在 main.rs。6. `cargo check` 通过后进入 Phase 2。

### Phase 2 — 并行修复(8 个子代理,文件互不重叠)

执行方式:一个 `task` 批次同时派发 Agent A/B/C/D/E/F1/F2/F3,每个代理的任务文本 = 本文件该 Agent 的完整清单 + 下述总规则 + 验收(按清单完成、不跑 cargo、不格式化无关代码)。文件所有权互不重叠,无跨代理契约。

**总规则(写入每个代理的任务文本):**

- 禁止运行任何 `cargo` 命令(并行中互相依赖);全部完成后主代理统一构建。
- 禁止格式化/整理无关代码;只改任务清单内的点。
- 不新增私有辅助函数、不新增宏、不改任何 `pub` 签名(清单内注明的除外)、不动 `temp/` 下文件。
- 替换模式(api 层样板统一改用原生 `ClientAccess`,见各代理清单):
    - **模式 A(send_and_parse)**:`let response = <builder 表达式/变量>.send()?;` 紧接 `self.client.response_to_json(response)`(或 `client.response_to_json(response)`),两语句相邻、之间无其他逻辑 → `self.send_and_parse(<builder>)`。builder 可跨多行,可含 `.with_error_body()`(属 builder 配置,等价)。
    - **模式 B(check_status)**:`let response = <builder>.send()?;` 紧接 `Ok(response.status() == HTTPStatus::X as u16)` → `self.check_status(<builder>, HTTPStatus::X)`。
    - **模式 C(send_maybe_parse)**:`let response = <builder>.send()?;` 紧接 `if <cond> { self.client.response_to_json(response) } else { Ok(json!({"success": response.status() == HTTPStatus::X as u16})) }`,且 `<cond>` 为 `return_data` 或 `method == HttpMethod::Get` → `self.send_maybe_parse(<builder>, <cond>, HTTPStatus::X)`。
    - **豁免(原样保留)**:任何含 `read_to_vec/read_to_string/into_body()`、状态码分支后继续读 body、`json["data"]` 二次提取、`with_header("Cookie", …)` 的站点;`auth.rs` 的 `fetch_auth_details`(用户指令 #4)、`get_login_security_info`、`fetch_admin_captcha`、`fetch_current_timestamp_with_provider` 与三个 logout(204 检查);`account.rs` 的 `phone_login_silence` 等已用 `send_and_parse` 的站点不动。
    - 每个替换点先读原代码确认与模式逐字等价(else 分支、状态码常量),不等价就跳过。

#### Agent A — `src/utils/acquire.rs`(效率)

1. **日志守卫**:`KittyCore` 内 6 处 `if self.config.log_requests {`(在 `log_request`、`log_response`、`response_to_json`(2 处块)、`response_to_string`、`response_to_binary`)改为 `if self.config.log_requests && log::log_enabled!(log::Level::Debug) {`。效果:Info 级别下不再构造 pretty-print/预览字符串。
2. **`PaginatedIter` 克隆消除**(纯内部,公开 API 不变):
    - `extract_page_data(json: &Value, data_pointer: &str) -> Vec<Value>` 改为 `fn take_page_data(json: &mut Value, data_pointer: &str) -> Vec<Value>`,体:`json.pointer_mut(data_pointer).and_then(|v| v.as_array_mut()).map(std::mem::take).unwrap_or_default()`(整页数组移动而非克隆)。
    - `initialize`:`let json = self.request_page(0)?;` 后先做 total 提取与 `response_amount_key` 覆盖(两者只借用),再 `let data = Self::take_page_data(&mut json, &self.data_pointer);`,最后构造 `Ready`。
    - `next_item` 翻页分支:`let mut json = match self.request_page(next_page) { … };` 后 `let data = Self::take_page_data(&mut json, &self.data_pointer);`(原 `extract_page_data` 调用)。
    - `next_item` 元素产出:`let item = current_page_data[current_index].clone();` → `let item = std::mem::replace(&mut current_page_data[current_index], Value::Null);`。
3. **不删任何东西**:`with_config`、`with_response_offset_key`、`current_page_number`、`yielded_count`、`total_pages`、`page_size`、`remaining_items`、`PaginationConfig.response_offset_key`、`KittyFactory` 5 个方法、`generate_meow_id`、`HTTPStatus` 枚举全部原样保留。

#### Agent B — `src/core/pipeline.rs`(2 条严重缺陷 + 1 效率)

1. **多账号身份错配(缺陷 1)**:`ensure_account_login`(约 885-899)删除前三行短路 `if account_usage.get(&idx).copied().unwrap_or(0) > 0 { return true; }` —— 每次选中账号都重新 `login_student`,token 全局单槽语义下身份不再错配。`account_usage` 参数保留(失败移除用)。
2. **usage 下标错位(缺陷 7)**:`account_usage` 类型 `HashMap<usize, usize>` → `HashMap<String, usize>`(键 = 用户名):
    - `select_report_account`:`let usage = account_usage.get(&accounts[idx].0).copied().unwrap_or(0);`(`accounts[idx].0` 即用户名)。
    - `ensure_account_login` 失败分支:`account_usage.remove(&idx)` → `account_usage.remove(&user)`(`user` 是上方已 clone 的元组首元素)。
    - `report_violations` 成功分支:`*account_usage.entry(chosen_idx).or_insert(0) += 1;` → `*account_usage.entry(accounts[chosen_idx].0.clone()).or_insert(0) += 1;`。
    - `current_idx` 的移除补偿逻辑(921-924)不动。
3. **`check_spam_posts` 阈值提前终止**(784-822):改为边收边匹配——遍历 `search_posts_gen` 流,命中 `user.id == user_id` 即 `matches += 1` 并立即 push 违规串,`matches >= self.config.spam_threshold` 时 `break`;循环后 `matches >= threshold` 才 `warn!` 并返回 violations,否则返回空 Vec。错误分支(`error!` + break)保留。行为变化(有意):violations 上限 = 阈值,不再收集超阈值部分。

#### Agent C — `src/core/cloudvar.rs`(2 条严重缺陷 + 1 效率)

1. **`flush_loop` 静默丢弃(缺陷 2)**(约 2457-2490):
    - 将 `let Some(tx) = inner.tx.lock().unwrap().clone() else { continue; };` 上移到 `let merged = merge_commands(batch);` **之前**,None 分支改为与"未就绪"分支一致的回退:
        ```rust
        let Some(tx) = inner.tx.lock().unwrap().clone() else {
            warn!("云连接未就绪, {} 条命令保留待上传", batch.len());
            let mut queue = inner.commands.lock().unwrap();
            for cmd in batch.into_iter().rev() { queue.push_front(cmd); }
            continue;
        };
        ```
    - 合并改为 `let merged = merge_commands(batch.clone());`(保留 batch 供失败回退)。
    - 三段发送(私有/公有/列表)改为顺序发送、失败即停:每段发送前检查 `failed` 标志,`send(frame)` 返回 false 时置 `failed = true` 并跳过后续段;列表段循环内失败同样置位并 `break`。
    - 段末 `if failed { warn!("批量上传发送失败, {} 条命令回退待上传", batch.len()); let mut queue = inner.commands.lock().unwrap(); for cmd in batch.into_iter().rev() { queue.push_front(cmd); } continue; }`。
    - 已知边界(接受):某段已发出后段失败时整批重发,变量段(set)幂等;列表段重复风险仅在"多 cvid 同批且后段失败"时出现,而发送失败只发生在通道断开瞬间,此时先前帧是否送达本就不确定。`merge_commands` 签名不动(仍收 `Vec<CloudCommand>`;clone 开销相对 flush 批次很小)。
2. **`connect()` 并发双建(缺陷 3)**(约 762-780):
    - 把现 `fn establish(inner: &Arc<CloudInner>) -> Result<()>` 的函数体(2269 起,不含锁行)抽为 `fn establish_locked(inner: &Arc<CloudInner>) -> Result<()>`;`establish` 改为 `{ let _connect_guard = inner.connect_lock.lock().unwrap(); establish_locked(inner) }`。
    - `connect()` 开头(第一行)加 `let _connect_guard = self.inner.connect_lock.lock().unwrap();`,`connected` 检查、`reset_state()`、`establish_locked(&self.inner)?`、flush 线程启动全部在锁内执行。`on_connection_lost` 重连循环仍调 `establish`(内部加锁),不变。
3. **`DataStore` 单次哈希**(420-464):`variable_in`/`list_mut` 的 `if vars.contains_key(key) { return vars.get_mut(key); }` → `if let Some(v) = vars.get_mut(key) { return Some(v); }`;`variable_ref`/`list` 的 `if let Some(l) = …get(key) { return Some(l); }` 保持(已单次),`variable_ref` 中 `if vars.contains_key(key) { return vars.get(key); }` → `if let Some(v) = vars.get(key) { return Some(v); }`。

#### Agent D — `src/core/retrieve.rs`(2 处 N+1 → 有界并行,复用本文件 `compute_admin_report_stats` 的 `thread::scope` 先例)

1. **`compute_fans_by_like_threshold`**(865-930):两段式。
    - 第一段(串行,无 HTTP):遍历 `fetch_followers_gen` 流,保留流序,收集 `total_likes >= like_threshold` 的 `(id: i64, fan: Value, total_likes: i64)` 到 `Vec`;`total_fans` 计数照旧。
    - 第二段:`let results: Vec<Option<JsonObject>> = vec![None; qualified.len()];` + `thread::scope` 按 chunk=16 分片,每线程内对每粉丝执行现有"honors 尽力而为"逻辑(逐字搬入: `i32::try_from(id).ok().and_then(|id32| UserDataFetcher::new().fetch_user_honors(id32).ok())` + N/A 回退 + nickname/total_likes/n_works 字段组装),按 `start + i` 写回 `results`。线程内各自 `UserDataFetcher::new()`。
    - scope 后 `qualified_fans = results.into_iter().flatten().collect()`。字段与错误语义与现状完全一致(仅顺序保持,无并发写共享状态)。
2. **`aggregate_user_comments_from_works`**(679-740):先串行把 `stream_works_from_both_sources(work_limit)` 的错误处理完、作品收集进 `Vec<Value>`(任一流错误 → 直接 `Err` 返回,与现状一致);再 `thread::scope` 按 chunk=8 分片,每线程建本地 `HashMap<String,(String,String,Vec<String>,i32)>`,对每作品执行现有 `stream_detailed_comments` 提取逻辑(逐字搬入);每线程返回 `Result<本地map, DataQueryError>`(作品流错误记入 Err)。scope 后按原顺序合并各线程 map(`entry(uid).or_insert_with(…)`,comments 追加、count 累加);若任一线程 Err,返回第一个 Err(按线程顺序)。最终 `into_values()` + 按 `comment_count` 降序排序逻辑不动。

#### Agent E — `src/core/services.rs` + `src/core/registry.rs`

1. **锁惯例统一**:services.rs 内全部剩余的 `.lock().unwrap()`(约 6 处,`grep '\.lock()\.unwrap()' src/core/services.rs` 列出)改为 `.lock().unwrap_or_else(std::sync::PoisonError::into_inner)`(与 `auto_report`/`save_group_action` 既有写法一致)。
2. **`split_chunk` 锁外提**(约 745-774):循环前 `let mut batch_guard = self.batch_manager.lock().unwrap_or_else(std::sync::PoisonError::into_inner);`,循环内 `self.batch_manager.lock().unwrap().is_record_processed(&record_id)` 改为 `batch_guard.is_record_processed(&record_id)`。该函数内无嵌套加锁(已核实),guard 可跨迭代复用;`chunk` 迭代中其余逻辑不动。
3. **registry.rs 死代码**:删除 `get_total_reports`(680-698)与测试 `get_total_reports_sums_across_types`(约 788-805)。`get_totals_pair` 保留;若删除后 `AtomicI64`/`thread::scope`/`Ordering` 导入仅剩 `get_totals_pair` 使用则保留导入(仍在用)。
4. **不做**:`item_config`、`pass_all` 复用 `execute_action`、`group_saved_action` 委托、`admin_label`、`apply_group` 外层检查删除 —— 全部放弃(避免行为/错误语义变化)。

#### Agent F1 — `src/api/community.rs` + `src/api/clouddb.rs` + `src/api/codegame.rs`

1. 补 7 个一行 `impl ClientAccess`(字段均为 `client: &'static CodeMaoClient`):`CommunityDataFetcher`、`UserAction`、`Ranking`、`CoconutCloud`、`CoconutCloudAdmin`、`OverseaDataClient`、`UserActionHandler`。每个: `impl ClientAccess for X { fn client(&self) -> &CodeMaoClient { self.client } }`,放文件尾部(按各文件现有 impl 位置风格)。
2. 按总规则替换样板(全部为模式 A):
    - community.rs:约 20 处,覆盖 163-166、185-188、199-202、215-218、239-242、249-252、259-262、277-280、295-298、313-316、331-334、346-349、361-364、372-375、386-389、400-403、410-413、420-423、430-433、444-447(以 `grep '\.send()\?;' src/api/community.rs` 实际清单为准)。
    - clouddb.rs:约 17 处,覆盖 29-32、44-47、60-63、85-88、100-103、113-116、163-166、175-178、186-189、197-200、209-212、226-229、249-252、264-267、281-284、292-295、304-307、320-323、332-335、376-378。
    - codegame.rs:24-27、34-37 → 模式 A;107-110(`Ok(response.status() == HTTPStatus::Created as u16)`)与 135-138(`HTTPStatus::Ok`) → 模式 B(`check_status(builder, HTTPStatus::Created / Ok)`)。
3. 不做:`fetch_editor_update`、`fetch_config` 合并、URL 拼接改 builder、`fetch_nemo_messages` 枚举化、`ReportReasonId` 合并、`list_user_databases` 合并。

#### Agent F2 — `src/api/education.rs` + `forum.rs` + `library.rs` + `shop.rs`

1. education.rs:
    - 模式 A:83-86、133-136、149-152、229-232、244-247、485-488、903-906。
    - 模式 C:187-192(`return_data` 分支,先确认 else 分支为 `Ok(json!({"success": response.status() == HTTPStatus::Ok as u16}))`)与 840-844(`get_or_delete_custom_package`,cond = `method == HttpMethod::Get`,expected `HTTPStatus::Ok`)。
    - `add_timestamp_to_builder`/`add_timestamp_to_paginated`、`build_paginated`(393)、CMTIME、`pacakgeEntryType` 等全部原样。
2. forum.rs:模式 A 共约 10 处:169-172、180-183、221-224、231-234、242-245、252-255、263-266、275-278、291-294、301-304(以 grep 清单为准)。`build_paginated`(138)原样。
3. library.rs:652-656 → 模式 C(`return_data`,`HTTPStatus::Ok`,先确认 else 分支)。`build_paginated`(132)原样。
4. shop.rs:331-334 → 模式 A。**不做** `join_or_default`;88/177 空注释段与 `create_workshop` 不动。
5. 不做:`fetch_lesson_topics/tags` 合并、`fetch_edu_get`、`ReportReasonId` 合并、`PublishStatus` 相关。

#### Agent F3 — `src/api/account.rs` + `src/api/whale.rs` + `src/api/work.rs`

1. **account.rs 手机号 i32→&str(缺陷 5)**:`update_phone_number(&self, captcha: i32, phonenum: &str)`、`validate_phone_number(&self, phone_num: &str)`、`execute_request_phone_change_verification(&self, old_phonenum: &str, new_phonenum: &str)`。体内:`with_param("phone_number", phone_num.to_string())` → `with_param("phone_number", phone_num)`(Into<String> 支持 &str);payload 的 `"phone_number": phonenum` 不变(serde_json 序列化 &str 等价)。全仓 grep 确认这三个函数无调用方(已核实),无需改调用点。
2. **account.rs 样板**:369-371、435-437 → 模式 A。
3. **whale.rs**:补 `impl ClientAccess for WhaleReportFetcher` + `impl ClientAccess for ReportHandler`;删除死代码 `add_timestamp_to_builder`(161-164,全文件无调用,且与 education.rs 同名函数重复);329-331 → 模式 B(`check_status(builder, HTTPStatus::NoContent)`)。`build_report_paginated`/`add_timestamp_to_paginated`/`apply_optional_filter` 与 4 个举报迭代器(202-296)原样。
4. **work.rs 死代码删除**:
    - `fetch_kn_work_state`(1714-1721):与 `fetch_work_status`(1760-1767)请求同一端点 `/neko/works/status/{id}`、体逐字相同、全仓无调用 → 删除。删除前 `grep fetch_kn_work_state` 全仓确认 0 调用。
    - `fetch_sample_detail`(1594-1609):全仓无调用、`params: Vec<(String,String)>` 形参迫使调用方堆分配 → 删除。
    - `PublishStatus` 枚举 + impl(32-45):全仓无使用(user.rs 另有同名枚举) → 删除。删除前 `grep PublishStatus` 全仓确认 work.rs 版 0 使用。
5. 不做:`toggle`、`with_time`、`build_page_paginated`、KN 迭代器合并、package list 合一、`fetch_work_status` 不动。

### Phase 3 — 收尾验证(主代理)

1. `cargo check` 0 error;修复跨文件/跨代理编译问题(预计点:services.rs 导入、main.rs 导入、`value_to_i64` 迁移)。
2. `cargo test` 全绿(registry 剩 `fetch_chunked_terminates_without_duplicates`;auth.rs 新增 `admin_info` 测试,见 Verification)。
3. `cargo build`(dev)成功。
4. grep 断言与行为核对见 Verification。

## Critical files & anchors

| 文件                   | 锚点                                                                                                                                                                            | 原因                                                                                      |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `src/utils/acquire.rs` | `KittyCore::log_request/log_response/response_to_json/response_to_string/response_to_binary`(约 658-920)、`PaginatedIter::extract_page_data/initialize/next_item`(约 1252-1370) | 日志守卫 + 克隆消除都在此;同时是"不删 PaginatedIter/KittyFactory/generate_meow_id"的边界  |
| `src/core/cloudvar.rs` | `connect()`(762-780)、`establish`(2269)、`flush_loop`(2457-2490)、`merge_commands`(291)、`DataStore` 4 函数(420-464)                                                            | 两条严重缺陷 + 单次哈希;establish 拆 `_locked` 的调用方只有 connect 与 on_connection_lost |
| `src/core/pipeline.rs` | `ensure_account_login`(885-899)、`select_report_account`(约 865-884)、`report_violations`(906-960)、`check_spam_posts`(784-822)                                                 | 两条严重缺陷 + 阈值提前终止                                                               |
| `src/api/auth.rs`      | `fetch_auth_details`(331-339,不动)、`fetch_admin_details`(342-348)、"数据结构"区                                                                                                | AdminInfo 落点;fetch_auth_details 是指令 #4 的边界                                        |
| `src/main.rs`          | 第 8-9 行导入、58-66 提取块、141-158 `extract_admin_id`                                                                                                                         | 固定字段提取的消费端                                                                      |

## Verification

1. **Phase 1 后**:`cargo check` 0 error。
2. **最终**:`cargo check` 0 error;`cargo test` 全绿;`cargo build` 成功。工作目录 `/home/aurzex/项目/Backend`,无额外 env/依赖。
3. **新增单元测试**(Phase 1 落地,验证固定字段契约):`src/api/auth.rs` 底部加:

```rust
#[cfg(test)]
mod tests {
    use super::AdminInfo;
    use serde_json::json;

    #[test]
    fn admin_info_from_details_reads_fixed_fields() {
        let v = json!({"admin": {"id": 42, "username": "u1", "role_name": "r1", "full_name": "F1"}});
        let info = AdminInfo::from_details(&v).expect("固定字段应可提取");
        assert_eq!(info.id, 42);
        assert_eq!(info.username, "u1");
        assert_eq!(info.role_name, "r1");
        assert_eq!(info.full_name, "F1");
    }

    #[test]
    fn admin_info_from_details_rejects_missing_fields() {
        assert!(AdminInfo::from_details(&json!({"admin": {"id": 42}})).is_none());
        assert!(AdminInfo::from_details(&json!({"id": 42})).is_none());
        assert!(AdminInfo::from_details(&json!({"admin": {"id": "x", "username": "u", "role_name": "r", "full_name": "f"}})).is_none());
    }
}
```

4. **grep 断言**(全仓):
    - `fetch_kn_work_state|fetch_sample_detail` → 0 命中(work.rs 死代码);
    - `get_total_reports` → 0 命中(registry 删除);
    - `add_timestamp_to_builder` 仅 education.rs 1 处定义 + 其调用点(whale.rs 副本已删);
    - `impl_api_manager|post_empty|build_report_iter|fetch_follows_gen|build_page_paginated|with_iter_time|join_or_default|extract_admin_id` → 0 命中(约束落实);
    - `PublishStatus` 仅 user.rs 定义 + 其使用点;
    - `\.send\(\)\?;` 在 api/ 下只剩豁免清单:auth.rs 的 `fetch_auth_details`/`get_login_security_info`/`fetch_admin_captcha`/`fetch_current_timestamp_with_provider`/3 个 logout、education.rs 0 处、其余文件 0 处(以 grep 结果与豁免清单比对);
    - `value_to_i64` 定义仅在 `src/utils/data.rs`;main.rs 与 services.rs 不再从 `core::registry` 引用。
5. **7 条缺陷逐条 diff 复审**:对照 REVIEW.md 第 6 节,确认修复点语义(尤其 pipeline 登录短路删除、flush_loop 回退、connect 锁内检查、phone &str、write_blocks 未包含在本方案 → 见下)。**compiler.rs write_blocks 缺陷不在本方案范围**(它要求给 `block_xml` 增加字符串 next 链支持,属行为扩展而非缺陷修复,且触发依赖仓库内未出现的数据形态;放弃并记录)。

> 注意:7 条严重缺陷中 6 条在本方案修复(1、2、3、4、5、7);第 6 条(compiler.rs `write_blocks` 字符串引用形态)按上述理由放弃。

6. **行为核对**:无法联网冒烟(CLI 需管理员账密 + 验证码交互)。HTTP 行为变化点(样板替换)靠"替换规则逐字等价"保证;缺陷修复靠 diff 复审 + 上述 grep。

## Assumptions & contingencies

- **基线**:假设 `cargo check` 在 8e87466 通过。若实际失败,先修基线(与本次改动无关的存量问题)再执行 Phase 1。
- **代理失败**:任一 Phase 2 代理交付物编译失败或未按清单完成 → 只重跑该代理,不重跑整波。
- **grep 清单偏差**:各文件 `.send()?` 站点行号以实施时 grep 结果为准(清单行号来自 2026-08-10 阅读);替换只按"模式逐字等价"规则判定,不等价即跳过并记录。
- **`check_spam_posts` 行为变化**(达到阈值即停)为有意改动,符合评审建议;若需保留"收集全部用于日志计数"的行为,则在循环内继续计数但不收集,阈值后 break——实施时采用本方案的简化版(break),不做日志计数增强。
- **flush_loop 失败回退的重复发送边界**(见 Agent C):接受;若实施中发现 `merge_commands(batch.clone())` 造成可见性能问题(批内命令数极大),改法:先取 tx 再 drain,失败回退仍 clone——保持 clone 方案即可,不引入新结构。
- **PublishStatus/fetch_kn_work_state 删除前**再次全仓 grep 确认 0 使用;若 grep 出现使用点,保留并记录(评审已核实为零,预期不会发生)。
