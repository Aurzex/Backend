# 代码评审与整改方案 — 第二轮(ef55995 整改之后)

## Context

请求:按 5 个维度(逻辑错误 / 性能 / 更优实现 / 性能优化 / 设计模式)评审当前库代码,指出问题并提供具体优化方案。约束:简洁可读优先、不过度抽象、不用宏、不主动删非确证死代码。

现状:上一轮评审(temp/REVIEW.md,97 条)已由 commit ef55995 整改(严重缺陷修复 + api 层样板统一 + 效率优化)。本评审针对当前 HEAD(ef55995),`cargo check --all-targets` 通过。方法与核实:7 个 reviewer 子代理按文件簇并行审查,主代理逐条对照源码复核——**本文件中全部高/中优先级发现均已亲自 `read`/`grep` 核实**;标注 `[INFERENCE]` 的条目为仓库内无法确证的服务端契约(需实测),其余为代码可证事实。行号均指当前 HEAD。

## 评审总览

### 上轮整改核实(全部正确落地)

- **多账号身份错配**(pipeline.rs:877-945):`ensure_account_login` 每次用选中账号本人凭据登录;`account_usage` 改为按用户名记账(`HashMap<String, usize>`);账号移除时 `account_usage.remove(&user)` 并修正 `current_idx`。✓ 正确。
- **flush_loop 命令丢弃**(cloudvar.rs:2444-2526):未就绪与发送失败两条路径均回退队列。✓ 已修(残留风险见 F1)。
- **connect 并发双建**(cloudvar.rs:769-777):`connect_lock` 串行化建立。✓ 已修(残留竞态见 F2)。
- **captcha 防水墙票据**(captcha.rs:127-141):已 `.with_payload(ticket)`,与 geetest 一致。✓
- **手机号 i32→&str**(account.rs):`validate_phone_number`/`execute_request_phone_change_verification` 已改 &str。✓(残留:`update_phone_number` 的 captcha 仍为 i32,见 P6-1)
- **AdminInfo 固定字段提取**(auth.rs:224-231 + main.rs)。✓(残留:auth_details 形态不对称,见 L-11)
- **日志守卫**(acquire.rs:658,692,827 等):全部日志块已加 `log_enabled!(Debug)`。✓
- **PaginatedIter 克隆消除**(acquire.rs:1306-1314):`mem::replace(Value::Null)` 逐元素取走。✓
- **retrieve N+1 并行化**(aggregate_user_comments_from_works / compute_fans_by_like_threshold)。✓

### 本轮发现统计(去重后 28 条)

| 维度 | 高 | 中 | 低 |
|---|---|---|---|
| 逻辑错误 | 1 | 12 | 6 |
| 性能问题 | — | 2 | 3 |
| 更优实现 | — | 1 | 3 |

## 发现明细(按用户 5 维度)

### 1. 逻辑错误

**[高] T1 时间戳解析链断裂 — `fetch_current_timestamp_with_provider` 与同端点解析形态互相矛盾**
- 位置:src/api/auth.rs:287-293(+1161-1179,525-541)vs src/api/community.rs:169-175
- 问题:auth.rs 用 `json["data"].as_i64().unwrap_or(0)` 解析 `/coconut/clouddb/currentTime`;community.rs 对同一端点读 `as_str()`(并 warn「不是字符串」)。两者不可能同时正确。若服务端返回字符串:`unwrap_or(0)` 静默返回 0 → `handle_password_v2` 向 `/captcha/rule/v3` 发送 `timestamp: 0`;`get_calibrated_timestamp` 算出 `time_difference = local - 0`,`generate_x_device_auth`(auth.rs:1182-1195)产出的 device-auth `timestamp ≈ 0`,该头被 cloudvar WS(cloudvar.rs:2289)与 NEKO 反编译(compiler.rs:1609)使用。**且函数文档声称毫秒,而 `get_calibrated_timestamp` 按秒与 `as_secs()` 比较——即使解析正确,两个消费者至少一个单位错误**(代码内可证)。
- 修复:见 Approach Phase 5。`[INFERENCE]` 服务端实际返回形态需实测确认。

**[中] F1 flush_loop 部分失败重放已送达帧(残留)**
- 位置:src/core/cloudvar.rs:2475-2526
- 问题:私有帧成功、公有帧失败 → 整批 `push_front` 回退,已入 mpsc 的私有帧下次重发;列表帧(append/unshift/insert/delete 均非幂等)中途失败同样整批重放。`tx.send` 返回 Ok 仅表示 rx 存活,不保证帧已写出——读线程已消费前几帧并写 socket 后死亡时,Ok 帧照样丢失(窗口缩小但未消除)。另 `merge_commands(batch.clone())` 成功路径也整批深拷贝(cloudvar.rs:2475)。
- 决定:**不改回退结构**。彻底修复需「读线程逐帧确认」的协议级改造,收益/成本比差;当前窗口极小且失败路径有 warn 日志。评审记录在案,克隆问题同留(`batch.clone()` 仅为失败回退所需)。

**[中] F2 connect() 与自动重连仍可双建 socket(残留)**
- 位置:src/core/cloudvar.rs:2283(establish_locked 入口)+ 2399-2441(on_connection_lost 重试循环)
- 问题:重连线程睡醒后调 `establish()` → 持有 `connect_lock` 后进 `establish_locked`,但 **establish_locked 无 connected 复查**。若用户在退避 sleep(最长 5 分钟)期间手动 `connect()`(此时已置 connected=true),重连线程随后仍会再建一条 socket 并覆盖 `inner.tx`/`read_join`,双读线程双 socket,旧 socket 的读线程收尾时经 `on_connection_lost` 踩掉新连接的 connected 标志。另外 `connect()` 的 `reset_state()` 会清空断线期间排队命令与本地状态(769-777)。
- 修复:Phase 2-1(establish_locked 入口复查)+ Phase 2-2(connect 在重连在途时跳过 reset_state 的决策,见 Approach)。

**[中] F3 close() 持 read_join 锁 join 自身,死锁窗口**
- 位置:src/core/cloudvar.rs:833-837(+flush_join 同型)
- 问题:`if let Some(handle) = inner.read_join.lock().unwrap().take() { handle.join(); }` 中,临时 MutexGuard 存活到整个 if-let 结束,join() 期间持有锁;若读线程此刻正进入重连 establish_locked 等待该锁 → 死锁,close() 永不返回。
- 修复:Phase 2-3。

**[中] F4 ConnectionEvent::Opened 每连接触发两次**
- 位置:cloudvar.rs:2333(establish_locked)+ 1832(handle_frame 收到 "40")
- 问题:WS 升级成功与 Socket.IO 握手确认各发一次 Opened;以 Opened/Closed 配对管理资源的调用方会错乱。
- 修复:Phase 2-4(删除 handle_frame 处,保留 establish_locked 处,与 connected 标志置位点一致)。

**[中] F5 update_private_vars_done 处理器与发送格式不匹配**
- 位置:cloudvar.rs:1926-1946(handler)vs 2455-2461(flush_loop 发送)
- 问题:客户端发 `("update_private_vars", [ {cvid,value}, … ])` 数组,`UpdatePrivateVarHandler` 却按对象 `payload.get("cvid")` 解析——数组上恒为 None,处理器空操作;同文件 `UpdatePublicVarHandler`(1949-1984)按数组解析,两通道形态矛盾。断线重连补发的私有变量回显将永不落地本地。`[INFERENCE]` 服务端回显格式,但代码内形态矛盾可证。
- 修复:Phase 2-5(handler 兼容数组与单对象两种形态)。

**[中] F6 分页参数重名:base_params 与 amount/offset 键冲突**
- 位置:src/utils/acquire.rs:1220-1231(build_params);调用点 work.rs:919-931、education.rs:393/519/763、forum.rs:143-152
- 问题:`build_params` 先 clone base_params 再无条件 append amount_key/offset_key。调用方在 base_params 预置同名键:work.rs `with_iter_param("page_size","100")` + `with_amount_key("page_size")`(计算值 15)、`with_iter_param("current_page","1")` + `with_offset_key("current_page")`;education/forum 同款 `page=1` + offset_key("page")。每请求携带 `page_size=100&page_size=15` / `page=1&page=2` 重复键:服务端取首值则永远第 1 页(重复数据),取末值则预设参数无效(work.rs 的 100/40 页大小从未生效,往返次数多 3-7 倍)。
- 修复:Phase 1-1(过滤同名键)+ Phase 1-2(删除/替换调用点预设)。

**[中] F7 分页 EOF 判定假设每页恰好 page_size 条,页被截断时静默丢数据**
- 位置:acquire.rs:1343-1345
- 问题:`total.is_some_and(|t| (current_page + 1) * page_size >= t)` 以「每页满页」为前提;服务端页上限低于请求值(work.rs 24/30/100、education.rs 150/100 混用)或过滤条目时提前终止,剩余举报/作品本会话静默不处理。
- 修复:Phase 1-3(改用累计 `yielded >= total`)。

**[中] F8 每举报类型 100 条硬上限 + 「所有举报处理完成」误报**
- 位置:src/core/registry.rs:388-391(+439,480,524 三处同型),配合 whale.rs:175(`with_limit(default_limit)`)、terminal.rs:351
- 问题:4 个 `gen_from(... Some(100))` → `PaginatedIter.with_limit(100)`,`reached_limit`(acquire.rs:1340-1341)在产出 100 条后终止。某类型待处理 >100 时:处理会话只处理前 100 条却打印「所有举报处理完成」;pass_all 一键通过同样只过 100 条/类型;done 浏览器每类型最多看 100 条。回归测试(registry.rs:710-736)用无限生成器,测不出该截断。
- 修复:Phase 3-3(4 处 `Some(100)` → `None`,让 chunk_size 控制节奏;完成文案随之变准确)。

**[中] F9 execute_action / apply_action_by_key 丢弃 check_status 的 Ok(false)**
- 位置:src/core/pipeline.rs:267-273(`apply_action_by_method(...)?` 吞 bool)+ services.rs:411-416(execute_action 同)
- 问题:服务端返回非 204 的 2xx/3xx 时 `check_status` 返回 `Ok(false)`,被 `?` 静默吞掉 → 记录被 `mark_record_processed` 标记、UI 报「已处理」。同文件 pass_all(services.rs:491-501)正确检查 `Ok(true)`——两条路径语义不一致。
- 修复:Phase 3-1。

**[中] F10 terminal.rs 单条动作失败即中止整个待处理会话**
- 位置:src/core/terminal.rs:484-487
- 问题:`processor.apply_action(item, &key, admin_id)?` 用 `?` 传播:任一条 PATCH 瞬时失败 → process_pending 整段退出,本会话剩余举报全部未处理;若该条持续失败,每次进入都卡在同一项。与批量路径(逐条 Err 仅 ui.error,不中断)及 pass_all 语义不一致。同段 515 行 `decided.insert(j)` 在失败时也照插。
- 修复:Phase 3-2。

**[中] F11 send_and_wait 断连时静默返回半截回复**
- 位置:src/core/converse.rs:370-380(+708-713 Begin 置 completed_round,+928-938 收尾)
- 问题:Begin 事件后连接中断,收尾只清 receiving(不清 completed_round):`wait_for_response` 谓词 `!receiving` 立即为真 → `send_and_wait` 返回 Ok(部分文本),调用方无法区分成功与断连。
- 修复:Phase 4-1。

**[中] F12 converse connect() 无并发防护**
- 位置:src/core/converse.rs:302-309
- 问题:check-then-act(先读 connected 再 establish),并发调用双建连接,第二次覆盖 tx,第一条读线程收尾踩掉 connected 标志(与 cloudvar 上轮已修的缺陷同型)。ChatClient 文档宣称线程安全可克隆共享。
- 修复:Phase 4-2。

**[中] F13 主动 close() 触发虚假「连接已断开」Error 事件**
- 位置:src/core/converse.rs:928-938
- 问题:收尾无条件按 `was_connected=true` 发 `ChatEventType::Error("连接已断开")`;主动 close()(451-457,先置 stopping)也会触发,与真实异常断连不可区分。
- 修复:Phase 4-3。

**[中] F14 同端点页大小矛盾(KN 作品列表 24 vs 15;课程包 150 vs 100)**
- 位置:work.rs:2125/2146/2164(page_size 24)vs user.rs:448-465(page_size 15);education.rs:757-768(150)vs 808-812(100)
- 问题:同一上游端点两种页大小;若服务端封顶低于请求值,offset 步进按请求值推进 → 跳条,且 F7 的总数终止判定截断尾部。
- 修复:Phase 1-4(统一页大小 + 优先 `with_response_amount_key`,education.rs:663-664 已有先例)。

**[低] L1 get_ranking push-before-send 顺序竞态** — cloudvar.rs:1039-1052:先入队后发送,并发请求时队列顺序与 wire 顺序可能相反,响应错配。修复:Phase 2-6(先发后入队,缩小窗口;协议无请求关联 ID,并发错配无法根除,记录)。
**[低] L2 list_pop / CloudList::pop / shift 空列表返回 Err 而非 Ok(None),且读-删间 TOCTOU** — cloudvar.rs:1085-1092,1279-1286。修复:Phase 2-7(空列表短路返回 Ok(None);读-删原子化需锁内执行,属协议级改动,仅修空列表分支)。
**[低] L3 自动重连不清除本地 state,服务端已删变量永久残留** — cloudvar.rs:2399-2412:on_connection_lost 不 reset_state,重连后 list_variables_done 只增不删。修复:Phase 2-8(收到新一轮 list_variables_done 时全量替换 store)。
**[低] L4 reconnect_attempts 只写不读;ConnectionEvent::Error 从未构造** — cloudvar.rs:631,1830,2330。修复:Phase 2-9(删除该字段;Error 事件在 establish 失败路径构造发出)。
**[低] L5 stream_works_from_both_sources 奇数 limit 少一条,limit=1 时发 limit=0** — retrieve.rs:612-614(合并流无 take 截断,已核实 645-678)。修复:Phase 6-2。
**[低] L6 compiler write_blocks/block_xml:字符串 next 引用断链 + 从不输出 mutation** — compiler.rs:2087-2093(引用收集只认对象形式),2207-2212(next 链要求 is_object),block_xml 全函数(2132-2265)不序列化 JSON 路径精心合成的 mutation(3274-3356)→ Kitten2/3 输出丢 text_join 槽数/if-else/过程调用结构。**上轮用户决定放弃此项,本轮仅记录不改**。
**[低] L7 FunctionCallDecompiler 参数块绕过 create_block_decompiler 工厂** — compiler.rs:3564-3566:嵌套调用参数内的专用块失去 NAME/mutation 处理,与 process_params 路径不一致。**同上轮决策,仅记录不改**。

### 2. 性能问题

**[中] P1 每违规一次完整登录(身份修复的直接代价)**
- 位置:src/core/pipeline.rs:925-945(report_violations 循环)
- 问题:每处理一条违规都 `login_student`(2-3 个 HTTP 请求);单账号 + max_reports_per_account=25 时一轮自动举报发 25 次重复登录。安全前提已核实:循环全程持 network_lock,无其他线程切换身份,`last_login == user` 时跳过登录是安全的。
- 修复:Phase 3-4。

**[中] P2 论坛评论回复逐条串行 HTTP(N+1)**
- 位置:src/core/retrieve.rs:242-265(reply_items Forum 分支)
- 问题:每条主评论惰性触发一次 `fetch_reply_comments_gen`;build_raw_stream 上限 MAX_COMMENTS=1000,即单流最多 1000 个串行往返。ef55995 只并行化了另两处 N+1。
- 修复:Phase 6-3(有界并行 + 保序)。

**[低] P3 merge_commands(batch.clone()) 每周期整批深拷贝** — 见 F1,决定不改。
**[低] P4 stream_user_ids 每条评论整对象克隆** — retrieve.rs:322-323:仅为传引用给 reply_items 深拷贝整个 JsonObject;同文件 stream_comment_ids(364-366)用借用。修复:Phase 6-4。
**[低] P5 generate_x_device_auth 32 次逐字节 hex 堆分配** — auth.rs:1188。修复:Phase 6-5(预分配 String::with_capacity(64) + write!)。

### 3. 更优实现

**[中] I1 统一 ClientAccess 错误语义:4xx/5xx 错误体被丢弃**
- 位置:acquire.rs:1817-1843 + 483-489(status_as_error 默认 true)
- 问题:统一后的 `check_status`/`send_and_parse`/`send_maybe_parse` 经 `builder.send()?` 发送,任何 4xx/5xx 直接转 Err 且响应体(服务端错误消息)被丢;`send_maybe_parse` 的 `{success:false}` 分支对 4xx/5xx 不可达。全仓仅 auth.rs:450 一处用 `with_error_body()`。后果:几十个调用点拿不到服务端拒绝原因,无法区分「服务器拒绝」与「网络故障」。
- 修复:Phase 6-6(在 ClientAccess 三个默认方法内统一关闭 status_as_error 并按 expected 检查,把非预期状态与错误体并入错误信息)——不改任何调用点签名。

**[低] I2 main.rs 以 `msg.contains("验证码")` 判定重试** — main.rs:128-135:字符串匹配中文错误文案决定重试语义,任何含「验证码」的其他错误会静默改变重试行为。修复:Phase 6-7(LoginHandler 暴露结构化错误码或 error_code 匹配)。
**[低] I3 handle_password_v0/v1/v2 把 Http/Json/Io 错误全部压成 MewError::Auth** — auth.rs:610-612,640-642,670-672:变体信息丢失,调用方无法区分凭据错误与网络错误。**用户上轮已决策不合并三函数;本轮仅建议错误透传不改结构**:Phase 6-8。
**[低] I4 时间戳工具分散**(auth.rs 手写 SystemTime+expect panic 点、acquire.rs current_timestamp_13):由 Phase 5 统一吸收,不做独立工具函数。

### 4. 性能优化

- **[中] O1 分页页大小统一 + response_amount_key 兜底**(F14 修复的一部分,教育 fetch_all_works_gen 已有先例)→ Phase 1-4。
- **[低] O2 KITTY_HEADERS 每请求循环添加** — acquire.rs:716-737:4 个静态头每次请求 `builder.header` 设置;ureq Agent::config_builder 支持 default_headers,可在 KittyCore::new 一次配置。**决定不改**(收益小,且 per-request 语义更显式)。
- 其余(锁内聚合、双哈希查找、fire_list_outcome 锁 5 次等上轮条目)按用户回退指令保持现状。

### 5. 设计模式

- 无新抽象需求。本轮修复全部为「既有正确抽象未用对」的修正(bool 吞掉、形态不匹配、锁使用错误),不需要引入 trait/宏/Builder。
- 唯一涉及写法的模式点:close() 持锁 join(RAII 守卫生命周期陷阱)→ Phase 2-3 用显式作用域 drop 守卫,属写法修正而非新抽象。

## Approach(整改步骤,按行为分组;行号基于 HEAD,实施前重读)

### Phase 1 — 分页参数与终止判定(acquire.rs + 3 文件调用点,独立)

1-1 **acquire.rs `build_params`(1220-1231)过滤同名键**:
```rust
fn build_params(&self, page: usize) -> Vec<(String, String)> {
    let mut params: Vec<(String, String)> = self
        .base_params
        .iter()
        .filter(|(k, _)| {
            Some(k.as_str()) != self.config.amount_key.as_deref()
                && Some(k.as_str()) != self.config.offset_key.as_deref()
        })
        .cloned()
        .collect();
    if let Some(key) = &self.config.amount_key {
        params.push((key.clone(), self.config.page_size.to_string()));
    }
    if let Some(key) = &self.config.offset_key {
        let offset = self.pagination_method.calc_offset(page, self.config.page_size);
        params.push((key.clone(), offset));
    }
    params
}
```
行为:重复键消除,计算值恒为唯一来源。

1-2 **删除/替换 3 个文件的预设同名参数**:
- work.rs:919-931 `fetch_custom_widgets_gen`:删 `.with_iter_param("current_page", "1")` 与 `.with_iter_param("page_size", "100")`,改 `.with_page_size(100)`;`fetch_collaborators_gen` / `fetch_collaboration_coco_works_gen` 同款(page_size 40/100)——用 `grep -n 'with_iter_param("page_size"\|with_iter_param("current_page"' src/api/work.rs` 定位全部 3 处。
- education.rs:393(build_paginated)、:519(fetch_class_students_gen)、:763(fetch_official_lesson_packages_gen):删 `.with_iter_param("page", "1")`(offset_key 已从 page+1=1 起算)。
- forum.rs:146(build_paginated):删 `.with_iter_param("page", "1")`。
行为:每个分页请求只携带一个 page/page_size/current_page 键。

1-3 **acquire.rs `next_item` EOF 判定(1343-1345)改按累计产出**:
```rust
// 已知总数且累计产出已达总数,则不再请求下一页;总数未知时必须尝试
if total.is_some_and(|t| self.yielded >= t) {
    return None;
}
```
行为:页被截断/过滤时不再提前终止,迭代直到真正取满 total(或空页)。注意:`yielded` 为迭代器已产出计数,与 reached_limit 共用,无冲突。

1-4 **统一同端点页大小**(3 处):
- work.rs:2125/2146/2164 三个 KN 迭代器 page_size 24 → 15(与 user.rs:448-465 一致),并补 `.with_response_amount_key("page_size")` 兜底(education.rs:663-664 同款用法)。
- education.rs:757-768 的 150 → 100(:808-812 同端点),同样补 `with_response_amount_key("limit")`。
行为:同端点单页大小一致;服务端截断页大小时按实际返回条数推进。

### Phase 2 — cloudvar 连接/事件/回显族(同一文件,独立)

2-1 **establish_locked 入口(2283 行函数首行)加 connected 复查**:
```rust
if inner.connected.load(Ordering::Acquire) {
    return Ok(());
}
```
行为:重连线程睡醒后与手动 connect() 竞态时不再双建(connect() 的检查在锁内,此复查在锁内覆盖重连路径)。

2-2 **connect()(769-777)在重连在途时不 reset_state**:
```rust
pub fn connect(&self) -> Result<()> {
    let _connect_guard = self.inner.connect_lock.lock().unwrap();
    if self.inner.connected.load(Ordering::Acquire) {
        return Ok(());
    }
    // 若自动重连循环正在退避中(connected=false 但 stopping=false 且 auto_reconnect=true),
    // 直接返回,由重连循环完成建立,避免 reset_state 清空断线期间排队命令
    if self.inner.auto_reconnect.load(Ordering::Acquire) && !self.inner.stopping.load(Ordering::Acquire) {
        return Ok(());
    }
    self.reset_state();
    ...
}
```
行为:手动 connect 不再丢弃断线期间排队的命令;重连由既有循环独占。

2-3 **close()(824-847)先取句柄、drop 守卫后再 join**(read_join 与 flush_join 两处同改):
```rust
let handle = {
    let mut guard = inner.read_join.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.take()
};
if let Some(handle) = handle {
    let _ = handle.join();
}
```
行为:join 期间不持锁,消除与 establish_locked 写回句柄的死锁窗口。

2-4 **删除 handle_frame:1832 的 `emit_connection_event(inner, ConnectionEvent::Opened);`**,保留 establish_locked:2333 一处(与 connected 置位点一致)。

2-5 **UpdatePrivateVarHandler(1926-1946)兼容数组与单对象**:抽出 `fn apply_one(inner, payload)` 承载现有 cvid/value 逻辑,handle 改为:
```rust
if let Some(items) = payload.as_array() {
    for item in items { Self::apply_one(inner, item); }
} else {
    Self::apply_one(inner, payload);
}
```
行为:与客户端数组发送格式及公有通道 handler 对齐;单对象形态(若服务端如此回显)也保留。

2-6 **get_ranking(1039-1052)先发送后入队**:payload 中 `Value::String(cvid.clone())`,`send_event` 成功后 `push_back(cvid)`,删除失败回滚分支。行为:入队顺序与 wire 顺序一致,消除并发错配主窗口(协议无关联 ID,极端并发仍可能错配,注释说明)。

2-7 **list_pop(1085-1092)与 CloudList::pop/shift(1279-1286)空列表短路**:读锁内 `last()/first()/get(index)` 为 None 时直接 `return Ok(None)`,不再走 list_apply_local 越界报错。行为:空列表弹出返回 Ok(None)(与文档语义一致);TOCTOU 需锁内执行,记录不改。

2-8 **AllDataHandler(约 1900-1918)改为全量替换 store**:收到 list_variables_done 时,`store.private_vars/public_vars/lists` 以本次数据重建(clear + 插入)而非 create_private/create_list 增量 merge。行为:断线期间服务端删除的变量不再残留本地。

2-9 **删除 reconnect_attempts 字段(631)及 1830/2330 两处写 0**;establish 失败路径(establish_locked 返回 Err 处,约 2300-2310)构造并发出 `ConnectionEvent::Error`。行为:消除只写不读的死状态;Error 事件接入真实失败路径。

### Phase 3 — 举报处理状态语义(pipeline.rs + services.rs + terminal.rs + registry.rs,独立)

3-1 **apply_action_by_key(pipeline.rs:276-283)检查 bool**:
```rust
if !apply_action_by_method(&config.handle_method, report_id, admin_id, resolution)? {
    return Err(ProcessorError::Processing(format!(
        "服务端未确认动作(状态码非预期): report_id={report_id}"
    )));
}
```
行为:非预期状态不再静默标记已处理,与 pass_all 语义一致。

3-2 **terminal.rs process_item(484-487)单条失败不中断会话**:
```rust
ActionChoice::Apply(key) => {
    match processor.apply_action(item, &key, admin_id) {
        Ok(()) => {
            ui.info(&format!("  => 已处理: {}", action_name(&key)));
            ...现有批量逻辑...
            Ok(st)
        }
        Err(e) => {
            ui.error(&format!("处理失败: {}", e));
            Ok(RunStats::default())
        }
    }
}
```
并把批量循环内 `decided.insert(j)`(515 行)移入 `Ok(())` 分支(失败项本 chunk 内仍可再问)。Abort 路径保持 Err(ProcessorError::Aborted) 不变。

3-3 **registry.rs 4 处 `Some(100)` → `None`**(388-391 及 439、480、524 同型,`grep -n 'Some(100)' src/core/registry.rs` 定位):`gen_from(|| fetch_*_reports_gen(..., None))`。行为:每类型全量迭代,chunk_size 控制节奏;「所有举报处理完成」(terminal.rs:351)与 pass_all 完成文案变为准确。无 limit 上限后 PaginatedIter 以 total/空页终止(Phase 1-3 已保证完整)。

3-4 **report_violations(pipeline.rs:925-945)同账号连续举报跳过重复登录**:`ensure_account_login` 增加参数 `last_login: &mut Option<String>`;函数开头:
```rust
let (user, pass) = accounts[idx].clone();
if last_login.as_deref() == Some(user.as_str()) {
    return true; // 同一账号刚登录过,令牌槽未变(network_lock 全程持有)
}
match Self::login_student(&user, &pass) { ... 成功后 *last_login = Some(user.clone()); }
```
失败分支照旧移除账号并清 usage(同时 `*last_login = None`,避免与移除后新选中账号混淆);调用处传 `&mut last_login`(循环外初始化 `let mut last_login: Option<String> = None;`)。行为:多账号轮换时每账号仍每次登录(身份正确性不变);单账号批量举报从 25 次登录降为 1 次。

### Phase 4 — converse 断连语义(独立)

4-1 **send_and_wait(370-380)等待后校验连接**:
```rust
if !self.wait_for_response(response_timeout) {
    return Err(ChatError::Timeout("回复超时".into()));
}
if !self.is_connected() {
    return Err(ChatError::Timeout("连接已断开,回复不完整".into()));
}
Ok(self.current_response())
```
行为:中途断连返回错误而非半截文本。

4-2 **connect(302-309)加连接锁**:`ChatInner` 增加 `connect_lock: Mutex<()>` 字段(build 处初始化),connect() 整体包 `let _guard = self.inner.connect_lock.lock().unwrap_or_else(PoisonError::into_inner);`(检查与 establish 都在临界区内)。行为:并发 connect 串行化,第二次直接命中 connected 复查返回。

4-3 **read_loop 收尾(928-938)主动关闭不发 Error**:
```rust
if was_connected && !inner.stopping.load(Ordering::Acquire) {
    info!("AI 对话连接已断开");
    emit_stream(&inner, "连接已断开", ChatEventType::Error);
}
```
行为:主动 close() 不再触发虚假断连回调。

### Phase 5 — 时间戳解析与单位统一(auth.rs + community.rs,独立)

5-1 **auth.rs:287-293 解析改 `value_to_i64`**(该函数已在 auth.rs 导入,utils/data.rs:157-162,同时兼容数字与数字字符串):
```rust
Ok(value_to_i64(&json["data"]).unwrap_or(0))
```
并更新 doc 为「服务器当前时间戳(秒)」。

5-2 **get_calibrated_timestamp(1161-1179)单位对齐**:server_time 现与 `as_secs()` 同为秒,逻辑保持不变(解析修复后自动正确);删除「毫秒」表述残留。

5-3 **handle_password_v2(525-541)的 ticket 时间戳改用本地毫秒**:`let timestamp = current_timestamp_13();`(acquire.rs 已有,13 位毫秒;与官方客户端一致),不再依赖服务器时钟。

5-4 **community.rs `extract_time_string`(169-175)改 `value_to_i64(...).map(|v| v.to_string()).unwrap_or_default()`**:与 auth.rs 解析一致,消除同端点双形态矛盾。

行为:无论服务端返回数字还是数字字符串,两条路径都正确;秒/毫秒按用途分离(校准用秒、登录票据用本地毫秒)。`[INFERENCE]` 若实测服务端返回毫秒数字,则校准基准仍错——届时把 5-2 改为 `server_time / 1000`(该 fallback 见 Assumptions)。

### Phase 6 — 小项(独立,可并行)

6-1 **account.rs:651-656 `update_phone_number` 的 `captcha: i32` → `&str`**,payload 同步 `"captcha": captcha`(字符串;与全模块其余 captcha 参数一致)。该函数全仓零调用(已 grep),属公共 API 修正,不改字段名(`phone_number` vs `phone` 待实测,见 Assumptions)。
6-2 **retrieve.rs:612-614**:
```rust
if limit <= 0 { return Box::new(std::iter::empty()); }
let per_source_limit = Some((limit + 1) / 2);
...
Box::new(nemo_stream.chain(web_stream).take(limit as usize))
```
行为:奇数 limit 恰好返回 limit 条;limit=1 不再发 limit=0。
6-3 **retrieve.rs:242-265 论坛回复 N+1 有界并行**:仿照本文件 `aggregate_user_comments_from_works`(679+)的 `thread::scope` + 分块先例——reply_items 改为按评论分块(如 8 条/块)并行拉取回复,按原顺序合并;无回复源(Work/Shop)路径保持现状。行为:单流串行 1000 请求降为 125 批并发;顺序保持。
6-4 **retrieve.rs:322-323 去克隆**:`if let Some(comment_obj) = comment.as_object() { ... reply_items(source, comment_id, comment_obj) ... }`,与 stream_comment_ids 借用风格一致。
6-5 **auth.rs:1188 hex 编码预分配**:
```rust
let mut sign = String::with_capacity(64);
use std::fmt::Write as _;
for b in result { let _ = write!(sign, "{b:02X}"); }
```
保持大写(已实测有效)。
6-6 **acquire.rs ClientAccess 三个默认方法(1817-1843)统一错误语义**:
```rust
fn check_status(&self, builder: KittyRequestBuilder, expected: HTTPStatus) -> MewResult<bool> {
    let response = builder.with_error_body().send()?;
    let status = response.status();
    if status == expected as u16 {
        return Ok(true);
    }
    if status.is_client_error() || status.is_server_error() {
        let body = response.into_body().read_to_string().unwrap_or_default();
        return Err(MewError::Http(format!("HTTP {status}: {body}")));
    }
    Ok(false)
}
```
`send_and_parse`/`send_maybe_parse` 同型处理(4xx/5xx 解析错误体并入 MewError;2xx/3xx 非预期走原分支)。行为:全部调用点获得服务端拒绝原因,可区分拒绝与网络故障;不改任何调用点签名。`MewError::Http` 变体形态以 acquire.rs 现有定义为准(实施时读取)。
6-7 **main.rs:128-135 重试判定结构化**:`handle_admin_password`(auth.rs:723-768)错误含 `error_code`(如 "Captcha-Error@Community-Admin")时,main 改为匹配 error_code 前缀 `"Captcha"`;无 error_code 时保持现状。实现:在 `MewError::Auth` 文案不可靠时,由 `handle_admin_password` 把 `error_code` 附入错误字符串的固定位置,或在 LoginResult 增加 `error_kind` 字段——**实施时选代价最小者:匹配 error_code 子串**(auth.rs 错误信息已包含 error_code,核实后按 `msg.contains("Captcha-")` 判定)。
6-8 **auth.rs:610-612/640-642/670-672 错误透传**:三处 `Err(MewError::Auth(format!("vN 登录失败: {e}")))` 改为仅对 `MewError::Auth` 包装、其余变体 `?` 透传:
```rust
Err(e) => match e {
    MewError::Auth(msg) => Err(MewError::Auth(format!("v0 登录失败: {msg}"))),
    other => Err(other),
},
```
行为:Http/Json/Io 变体保留,调用方可区分凭据与网络错误。

### 不改(记录在案,带理由)

- F1 flush_loop 整批回退重放风险:协议级确认机制成本高,窗口极小,维持现状。
- L6/L7 compiler.rs write_blocks/mutation/工厂绕过:上轮用户决策放弃,行为依赖外部数据形态,不改。
- captcha.rs verify_aliyun/netease/tencent 空负载(28-34/66-72/84-90):全仓零调用(已 grep 确证),属公共 API 缺口而非活路径缺陷;启用时按 verify_geetest_captcha 模式补参数。不删(公共 API,外部消费者未知)、不改。
- forum 201-vs-200、community delete 204-vs-200、logout 204-vs-200、create_account identity-vs-phone、rebind-captcha 字段名:与第三方整理 OpenAPI 文档冲突,服务端契约 `[INFERENCE]`,实测前不改。
- O2 KITTY_HEADERS 默认头、P3 merge_commands 克隆、HTTPStatus 枚举迁移、god file 拆分、manager 宏:上轮用户回退指令,维持现状。

## Critical files & anchors

| 文件 | 锚点 | 原因 |
|---|---|---|
| src/utils/acquire.rs | `build_params`(1220)、`next_item` EOF(1343)、`ClientAccess`(1817-1843) | 分页参数/终止/错误语义三处核心 |
| src/core/cloudvar.rs | `establish_locked`(2283)、`close`(824)、`flush_loop`(2444)、`UpdatePrivateVarHandler`(1926)、`get_ranking`(1039) | 连接/事件/回显族全部改动 |
| src/core/pipeline.rs | `apply_action_by_key`(276)、`report_violations`(925)、`ensure_account_login`(877) | 状态语义 + 登录缓存 |
| src/core/terminal.rs | `process_item`(470-530) | 单条失败中断会话 |
| src/api/auth.rs | `fetch_current_timestamp_with_provider`(287)、`get_calibrated_timestamp`(1161)、`handle_password_v2`(525) | 时间戳链 |

## Verification

前置:`cargo check --all-targets` 0 error(基线已通过,32s)。

1. **Phase 1**:`grep -n 'with_iter_param("page"\|with_iter_param("page_size"\|with_iter_param("current_page"' src/api/work.rs src/api/education.rs src/api/forum.rs` 应只剩 TIME 类参数,无 page/page_size/current_page 预设;`cargo test`(acquire.rs PaginatedIter 相关测试全绿);手写断言:`build_params` 对含同名键的 base_params 输出无重复键(可加单测或临时 println 验证)。
2. **Phase 2**:`cargo test`(cloudvar.rs merge_commands/连接相关测试);`cargo clippy` 不新增告警;人工 code review 确认 2-1 复查在锁内、2-3 守卫已 drop。
3. **Phase 3**:`cargo test`(registry.rs `fetch_chunked_terminates_without_duplicates` 仍绿,且它现在验证的是无限生成器语义);对 3-3 用 `grep -n 'Some(100)' src/core/registry.rs` 应无输出;3-1 语义验证:构造 `check_status` 返回 Ok(false) 的假 handler 不可行(HTTP 层),以单测覆盖 `apply_action_by_key` 对 `Ok(false)` 返回 Err(需 mock——如不可行,以 code review + cargo check 为准,标注)。
4. **Phase 4**:code review + cargo check(连接行为需真实 WS 服务,标注人工验证项)。
5. **Phase 5**:`cargo check`;解析逻辑以单测覆盖 `value_to_i64` 对数字/字符串两种输入;**实测项**:对真实服务 `curl /coconut/clouddb/currentTime`(或运行程序)确认 data 形态与单位,若为毫秒数字则执行 Assumptions 中的 `/1000` fallback。
6. **Phase 6**:`cargo test` 全绿;`cargo clippy` 无新增告警;6-6 后运行一次真实请求观察错误信息含服务端 body(人工项)。

## Assumptions & contingencies

- **时间戳服务端形态**(Phase 5 唯一外部依赖):已按「数字或数字字符串、秒级」决策实施,数字字符串与数字均被 `value_to_i64` 覆盖。若实测为**毫秒数字**(>1e12),fallback:`get_calibrated_timestamp` 中 `server_time / 1000` 后再计算时差(一行改动,其余不动)。
- **update_phone_number 字段名**(`phone_number` vs `phone`):OpenAPI 声明 `phone`,现代码用 `phone_number`,两者均有同族先例;函数零调用,实测后按真实接口对齐,本次只改 captcha 类型。
- **3-1 单测可行性**:若 `apply_action_by_key` 的 HTTP 层无法在无网络环境 mock,该步以 code review + 现有测试为准,不引入测试框架依赖。
- **100 条上限意图**:若产品上确实需要每会话限量,则改为「保留 Some(100) 但在 UI 显示剩余数量」——默认按本方案(移除上限),因「所有举报处理完成」文案与上限矛盾。
- 全部 `[INFERENCE]` 项(forum/community/logout 状态码、create_account 字段、captcha 空负载)默认不改,仅实测后按真实契约处理。
