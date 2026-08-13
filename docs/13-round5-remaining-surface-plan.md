# 第五轮评审 — api 层 / registry / terminal / socketio 剩余面重构方案

审阅日期:2026-08-14 · 基线:HEAD `4661c7b`(`refactor: 优化基础设施与举报引擎热点路径`)· 范围:`src/api/{work,community,education,forum,account,auth}.rs` + `src/core/{registry,terminal}.rs` + `src/utils/socketio.rs`

> 方案先行(本文档),随后落地代码;`Verification` 记录实际执行结果。cloudvar/converse(第三轮)、requests/services/pipeline/retrieve/compiler(第四轮)、`captcha/clouddb/codegame/library/shop/user/whale`(本轮复查确认仅为薄封装,无问题)不在范围。

## Context

按 5 个维度(逻辑错误 / 性能 / 更优实现 / 性能优化 / 设计模式)评审前四轮未覆盖的剩余面。已核实 docs/04 的 Phase 1/2/3/5/6 整改全部落地(分页同名键过滤、`Some(100)`→`None`、process_item Ask 分支、value_to_i64、hex 预分配、`(Captcha)` 结构化判定等),不重复列出。

本轮落地项:1 处逻辑错误(官方内容自动通过分支单条失败即中断整个会话,与 docs/04 F10 同型但漏改)、1 处数据丢失(`create_wood_file`)、1 处 N+1(`fetch_broadcast_messages_gen`)、1 处热路径深克隆(`parse_frame`),外加若干低风险 idiomatic 收尾。原则同前:简洁可读优先、不引入新依赖、不改破坏性 pub API 签名、不主动删非确证死代码。

## Approach

### P1 — `terminal.rs` 官方内容自动通过分支:单条失败中断整个会话(逻辑错误)

**位置**:`src/core/terminal.rs:460-471`(`process_item` 的 `if ctx.official[idx]` 分支)。

**问题**:官方内容自动通过调用 `processor.apply_action(item, ReportAction::Pass, admin_id)?`,用 `?` 传播错误。`process_pending`(279-360)仅对 `ProcessorError::Aborted` 特判,其余 `Err` 一路冒泡——一条官方举报的 PATCH 瞬时失败会让整个待处理会话退出,剩余举报全部不处理;若该条持续失败,每次进入都卡在同一项。这与同函数 Ask 分支(498-540)用 `match` 逐条记录错误、不中断的语义矛盾,是 docs/04 F10(Phase 3-2)漏改的分支。

**改动**:把 `?` 改成 `match`,失败时记录错误并返回 `Ok(RunStats::default())`(跳过本条,不中断会话),与 Ask 分支对齐:

```rust
if ctx.official[idx] {
    match processor.apply_action(item, ReportAction::Pass, admin_id) {
        Ok(()) => {
            let (type_name, record_id) = processor.item_brief(item).unwrap_or_default();
            ui.info(&format!(
                "--- [{}/{}] {} (举报ID: {}) --- 官方内容,自动通过",
                index, total, type_name, record_id
            ));
            return Ok(RunStats {
                passed: 1,
                ..RunStats::default()
            });
        }
        Err(e) => {
            ui.error(&format!("官方内容自动通过失败: {}", e));
            return Ok(RunStats::default());
        }
    }
}
```

**边界**:`apply_action` 失败时该条记录**不会**被标记已处理(状态语义不变),下次会话可重试;这是与 Ask 分支一致的预期行为,不是数据丢失。

### P2-1 — `work.rs create_wood_file` 丢字段(数据丢失)

**位置**:`src/api/work.rs:830-887`(`create_wood_file`)。

**问题**:函数先 `fetch_wood_project` 拿到完整项目,只抽出 `files` 追加新文件,然后**经 `create_wood_project`(770-792)重建 payload**——而 `create_wood_project` 的 `json!` 把 `addition.readonly_paths` 硬编码为 `[]`、`addition.locking_file_lines` 硬编码为 `{}`(778-779)。结果:只要原项目这两个字段非空,调用 `create_wood_file` 就把它们**静默清空**。属确定性数据丢失,非 `[INFERENCE]`。

**改动**:不再经 `create_wood_project` 重建,而是把抓到的完整项目原地追加文件后直接 POST,保留全部字段(尤其 `addition`)。具体:

1. 行 843 的 `let project = self.fetch_wood_project(work_id)?;` 改为 `let mut project = ...`。
2. 保留 844-859 的 `files` 提取与 `files.push(file_data)`。
3. 删掉 862-887 的 `self.create_wood_project(CreateWoodProjectArgs { ... })` 整段,替换为:

```rust
    // 更新项目:保留抓取到的全部字段(尤其 addition.readonly_paths / locking_file_lines),
    // 只替换 files;不再经 create_wood_project 重建(那会把二者置空)
    project["files"] = Value::Array(files);
    let builder = self
        .client
        .build_request(HttpMethod::Post, "/wood/project", Some(BaseKey::Creation))
        .with_payload(project);
    self.send_and_parse(builder)
```

**注意**:`create_wood_project` 与 `CreateWoodProjectArgs` 保持不变(不破坏 pub API,`create_wood_project` 仍有独立用途)。删除后 `CreateWoodProjectArgs` 在 work.rs 内不再被 `create_wood_file` 构造,但 `create_wood_project` 本身及 args 结构体保留。

**边界/风险**:POST `/wood/project` 收到的是 GET 返回的完整形态(比 `create_wood_project` 的 9 字段超集)。若服务端拒绝多余字段,fallback 见「Assumptions」——届时把 `readonly_paths`/`locking_file_lines` 两个字段加进 `CreateWoodProjectArgs` 并从 `project["addition"]` 透传(属 pub 结构体加字段,需单独评估破坏性,默认不做)。

### P2-2 — `community.rs fetch_broadcast_messages_gen` 每消息一次请求(N+1)

**位置**:`src/api/community.rs:801-812`。

**问题**:`.with_page_size(1)` 让每个广播消息一次 HTTP;默认 `with_limit(COURSE_LIST_PAGE_SIZE)`(=`10`,行 11)意味着拉满 10 条要串行发 10 次请求。

**改动**:`with_page_size(1)` → `with_page_size(MESSAGE_PAGE_SIZE)`(该文件已有常量 `MESSAGE_PAGE_SIZE = 15`,行 10)。`with_limit(limit.unwrap_or(COURSE_LIST_PAGE_SIZE))` 保持不动(仍是总量上限)。行为:1 次请求拉一批,10 条上限从 10 次往返降到 1 次。

**边界/风险**:若服务端无视 `limit` 参数、固定每页 1 条,则此改动为无害 no-op(仍 1 条/页,但分页逻辑不变);若服务端按 `limit` 返回批量,则消除 N+1。二者皆行为安全,`[INFERENCE]` 仅为「是否真有收益」。

### P2-3 — `socketio.rs parse_frame` 热路径深克隆 name + payload(性能)

**位置**:`src/utils/socketio.rs:64-70`(Event 帧解析,cloudvar/converse 读线程每帧调用)。

**问题**:`items.first()` 借出 `name` 后 `name.clone()`、`items.get(1).cloned()` 深拷贝 payload(`Value` 的整棵 JSON 树)。读线程热路径每事件一次深拷贝。

**改动**:解析出的 `Value::Array(items)` 是自有值,直接 `into_iter()` 逐项 move 出去,零克隆:

```rust
    if let Some(rest) = text.strip_prefix(EVENT_MESSAGE_PREFIX)
        && let Ok(Value::Array(items)) = serde_json::from_str::<Value>(rest)
    {
        let mut items = items.into_iter();
        if let Some(Value::String(name)) = items.next() {
            let payload = items.next().unwrap_or(Value::Null);
            return Frame::Event(name, payload);
        }
    }
    Frame::Unknown(text.to_string())
```

**行为等价**:空数组 / 首元素非字符串 → 走 `Frame::Unknown`(与旧 `&& let Some(Value::String(name)) = items.first()` 短路一致);多出的第 3+ 元素被丢弃(旧代码同样只取前两个)。

### P3 — 低风险收尾(idiomatic / 常量 / 死分支)

按优先级降序,均为机械、行为等价改动:

| #   | 项                                                                                                                                               | 位置                                                      | 改动                                                                                                                                                                                                                                                                                                                          |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 3-1 | `CloudAuthenticator.time_difference` 用 `==0` 作「未校准」哨兵:真 0 时差永不缓存,每次调用都重拉服务器时间                                        | `src/api/auth.rs:1135`(字段)、`1150`(初始化)、`1163-1171` | 字段 `time_difference: i64` → `Option<i64>`;`new_with_provider` 初始化 `None`;`get_calibrated_timestamp` 改 `if self.time_difference.is_none() { ...; self.time_difference = Some(local_time - server_time); }` 与 `Ok(now - self.time_difference.unwrap())`。字段为私有、仅本函数使用                                        |
| 3-2 | `create_account` 手写 `serde_json::Map` + 逐项 `.to_string()` 分配                                                                               | `src/api/account.rs:346-360`                              | 改用 `json!({...})`,同 `update_profile_details`(375-384)风格。行为等价,186/13 协议 id 保留                                                                                                                                                                                                                                    |
| 3-3 | `total_from` 把 `usize` 总数压成 `i32`(>2.1e9 即报错),下游却按 `i64` 读                                                                          | `src/core/registry.rs:359-367`                            | `i32::try_from(total)` → `i64::try_from(total)`(错误文案「总数超出 i64 范围」)。`total_items()` 返回 `Option<usize>`,`json!(total)` 后消费者按 i64 解析,消除无谓的 2.1e9 上限                                                                                                                                                 |
| 3-4 | `fetch_custom_lesson_packages_gen` 打的是 `/edu/zone/lesson/offical/packages`(官方)端点,与函数名「自定义」矛盾;grep 确证零调用点                 | `src/api/education.rs:835`                                | 端点改 `/edu/zone/lesson/customized/packages`,与同文件 `get_or_delete_custom_package`(`/edu/zone/lesson/customized/packages/{id}`,856)及 `fetch_custom_package_contents`(`/edu/zone/lesson/customized/package/lessons`,879)的 customized 前缀一致。精确列表路径 `[INFERENCE]`,但当前 `offical` 路径对「自定义」函数确定性错误 |
| 3-5 | `fetch_organization_ids` 对绝对 URL 传 `Some(BaseKey::Education)`,`build_url`(requests.rs:642)对 http(s) 开头直接返回,base_key 是死参数          | `src/api/education.rs:920-921`                            | `Some(BaseKey::Education)` → `None`。行为等价                                                                                                                                                                                                                                                                                 |
| 3-6 | `fetch_7day_hot_posts_gen` 把 `board_id` 内联进 URL 而非走 `with_iter_param`,与同文件其余分页器(如 search_posts_gen:311)不一致且多一次 `format!` | `src/api/forum.rs:320-332`                                | 端点固定为 `"/web/forums/boards/posts/7dayHot"`;`build_paginated` 赋给可变绑定后,`if let Some(id) = board_id { paginated = paginated.with_iter_param("board_id", id.to_string()); }` 再链式 `.with_page_size(10)...`。行为等价(`with_iter_param` 是 `self -> Self` 构建器风格)                                                |

## 不落地(记录在案)

- **`create_wood_file` 的每文件一次 GET+POST(O(N²) 上传字节)**:属 API 粒度问题,无批量端点可改,收益/成本比差,维持现状(本次只修其中的字段丢失,见 P2-1)。
- **`terminal.rs view_done` 全量加载 + 关键字过滤逐条 `serde_json::to_string`**(626-640、762-768、905-909):交互式演示 UI,内存/序列化量受人工会话制约;关键字为空时已短路不序列化;加 memo 需并行 Vec,收益有限,不改。
- **`terminal.rs:351-353`「所有举报处理完成」在预取流提前截断时略误导**:registry.rs 跳过某类型时已 `error!` 打日志(658),错误已可见,文案不改。
- **`registry.rs:658` `error!` 在「最后一页失败→重试遇 EOF」场景文案称「跳过该类型余下数据」略不精确**:行为正确(有界重试耗尽后放弃),仅文案,不改。
- **`registry.rs fetch_reports_chunked`(697-702)是 `fetch_chunked` 的纯转发**:有 3 处调用(services.rs:291/478/642),非死代码,保留。
- **`work.rs` 三个 KN 分页迭代器(2121-2184)结构重复 / 13 个 `ClientAccess` impl**:属「抽 helper / 宏」类抽象,违反「不过度抽象、不用宏」约束,不改。
- **`shop.rs:434` `reason_id` 序列化为 String 而 `forum.rs:447/469` 为数字**:跨文件不一致,服务端接受哪种形态 `[INFERENCE]`,实测前不改。

## Critical files & anchors

| 文件                    | 锚点                                                                                          | 原因                           |
| ----------------------- | --------------------------------------------------------------------------------------------- | ------------------------------ |
| `src/core/terminal.rs`  | `process_item`(447)、官方分支(460-471)、Ask 分支(498-540)                                     | P1 落点;Ask 分支是错误处理范本 |
| `src/api/work.rs`       | `create_wood_file`(830-887)、`create_wood_project`(770-792)、`CreateWoodProjectArgs`(732-743) | P2-1 落点与数据丢失根因        |
| `src/api/community.rs`  | `fetch_broadcast_messages_gen`(801-812)、常量(10-11)                                          | P2-2 落点                      |
| `src/utils/socketio.rs` | `parse_frame`(45-72)                                                                          | P2-3 落点                      |
| `src/api/auth.rs`       | `CloudAuthenticator`(1131-1171)                                                               | P3-1 落点                      |

## Verification

前置:`cargo check --all-targets` 0 error(基线已通过);`cargo clippy --all-targets` 不新增警告。

1. **P1**:`cargo check --all-targets` + code review 确认官方分支已改 `match`、`Err` 分支返回 `Ok(RunStats::default())` 而非 `?` 传播。行为需真实举报接口,以 code review + 编译为准(无网络环境)。
2. **P2-1**:`cargo check --all-targets`;`grep -n "create_wood_project(CreateWoodProjectArgs" src/api/work.rs` 应返回 0(create_wood_file 不再走该路径,`create_wood_project` 定义本身保留)。确认 `project["files"] = Value::Array(files)` 保留了 `addition`。行为需真实 Wood 项目,标注人工验证项。
3. **P2-2**:`grep -n "with_page_size(1)" src/api/community.rs` 应返回 0。
4. **P2-3**:`cargo test`;`parse_frame` 是纯函数,建议补一条单测覆盖「`"2[...]"` Event 帧 → `Frame::Event(name, payload)`」「空数组 → `Frame::Unknown`」两条。若项目测试约定不允许新增,则以 `cargo check` + code review 为准。
5. **P3**:`cargo check --all-targets` 通过;`grep -n "offical/packages" src/api/education.rs` 只剩 `fetch_expiring_lessons`(官方过期包,正确)与官方端点,`fetch_custom_lesson_packages_gen` 不再命中 offical;`grep -n "Some(BaseKey::Education)" src/api/education.rs` 中绝对 URL 那处不再出现。

## Assumptions & contingencies

- **P2-1 POST 载荷形态**:默认假设 POST `/wood/project` 接受 GET 返回的完整项目形态(超集)。若实测服务端拒绝多余字段,fallback:给 `CreateWoodProjectArgs` 增加 `readonly_paths`/`locking_file_lines` 两字段并从 `project["addition"]` 透传——但这是 pub 结构体加字段,先与用户确认破坏性,默认不执行,仅保留「字段丢失」这一确定性 bug 的最小修复(即本方案:直接 POST 完整项目)。
- **P2-2 服务端分页**:若 `/web/message-record/broadcast` 固定每页 1 条,`with_page_size(MESSAGE_PAGE_SIZE)` 是无害 no-op,不必回退。
- **P3-1 `time_difference` 引用点**:仅 `get_calibrated_timestamp` 使用该私有字段(已 grep 确证)。
- **P3-4 精确列表端点**:自定义课程包**列表**端点路径为 `[INFERENCE]`;当前 `offical` 路径确定性错误。若实测 `/edu/zone/lesson/customized/packages` 非列表端点,按真实接口对齐(该函数零调用点,风险为零)。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error。
- `cargo clippy --all-targets` 0 warning。
- `cargo test` 全绿:库单测 3 passed(`admin_info_from_details` ×2、`fetch_chunked_terminates_without_duplicates`)、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(`login_and_ai_chat` / `cloud_variables` / `decompile_works`)、doc-tests 0。
- 归零验证:
    - `grep -n "create_wood_project(CreateWoodProjectArgs" src/api/work.rs` → 0(`create_wood_file` 不再重建 payload)。
    - `grep -n "with_page_size(1)" src/api/community.rs` → 0。
    - `grep -n "offical/packages" src/api/education.rs` → 仅剩 `fetch_official_lesson_packages_gen`(782)与 `fetch_expiring_lessons`(905)两处官方端点;`fetch_custom_lesson_packages_gen` 已不命中 offical。
    - `fetch_organization_ids` 绝对 URL 处的 `Some(BaseKey::Education)` 已改 `None`。
- P2-3 `parse_frame` 未新增单测(改动行为等价、非新契约,维持最小测试面),以 `cargo check` + code review + 既有测试全绿为准。
- P1 / P2-1 行为依赖真实举报 / Wood 接口,无网络环境下以 code review 为准:均为确定性 bug 修复或等价重写,不改对外 pub 契约。
