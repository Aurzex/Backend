# 调用侧风格统一 — 审阅与整改方案（已执行）

审阅日期:2026-08-11 · 基线:HEAD `3a799cb` · 范围:全仓 27 个 Rust 源文件(约 2.5 万行)

## Context

在定义侧整改(3a799cb)后审阅**外部调用侧**一致性(传参/构造/链式/消费形态),并执行 P0/P1 整改 + 4 项附加优化。审阅方法:3 个 scout 按簇审查 + 主代理核实关键计数。全部改动 `cargo check --all-targets` 通过,`cargo test` 3 passed。

## 已执行改动

### P0-1: limit/offset 魔法数字提常量（8 文件）

各文件顶部新增分页常量(端点服务端契约),替换 `with_limit(limit.unwrap_or(N))` 与手写 `with_param("limit", ...)` 的散落字面量:

| 文件         | 新增常量                                                                                                                                 | 替换值                |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| community.rs | `MESSAGE_PAGE_SIZE=15` `COURSE_LIST_PAGE_SIZE=10` `COURSE_PACKAGE_PAGE_SIZE=50` `STUDIO_POST_PAGE_SIZE=24` `STUDIO_COURSE_PAGE_SIZE=100` | 15/10/50/24/100(9 处) |
| forum.rs     | `REPLY_PAGE_SIZE=10` `POST_DETAIL_PAGE_SIZE=4`                                                                                           | 15/10/20/4(7 处)      |
| shop.rs      | `WORKSHOP_SEARCH_PAGE_SIZE=14` `WORKSHOP_MEMBER_PAGE_SIZE=40`                                                                            | 14/40/15/20(6 处)     |
| user.rs      | `CENTER_LIST_PAGE_SIZE=5` `WORK_LIST_PAGE_SIZE=30` `COMMENT_PAGE_SIZE=10`                                                                | 5/30/15/10/20(13 处)  |
| library.rs   | `NOVEL_LIST_PAGE_SIZE=10` `NOVEL_DETAIL_ITEMS=200`                                                                                       | 20/10/200(7 处)       |
| education.rs | `NOTICE_PAGE_SIZE=10` `CLASS_STUDENT_PAGE_SIZE=100` `MANAGED_WORK_PAGE_SIZE=50` `LESSON_PACKAGE_PAGE_SIZE=100`                           | 10/20/50/100(11 处)   |

- 通用默认 15/20 引用 `DEFAULT_PAGE_SIZE`/`DEFAULT_LIMIT`(acquire.rs);域特定值用文件常量。
- 手写分页处(`Option<i32>` 参数)常量加 `as i32` 转换;`with_limit` 处(Option<usize>)不加。
- `unwrap_or(0)`/`unwrap_or(1)`(offset/page/status 默认)非分页上限,未动。

### P0-2 + P0-3: whale 辅助收敛,TIME 置首（whale.rs）

- `whale.rs`:`build_report_paginated` 内部新增链首 `.with_iter_param("TIME", current_timestamp_13().to_string())`,删 `add_timestamp_to_paginated` 辅助与 4 处调用点追加;保留 `default_limit: usize` 参数(调用点传 `limit.unwrap_or(15)`,与 3a799cb 前的 API 契约一致)。
- 效果:TIME 由"辅助末尾追加"统一为"构造入口链首",4 调用点不再重复。
- **education.rs 保留辅助**(add_timestamp_to_builder/add_timestamp_to_paginated):31+10 个调用点依赖,删辅助全内联会产生 41 处重复代码(负优化);辅助是教育域唯一一致形态,记录为域约定。
- work.rs 39 处已置首,未动。

### P0-4: send_maybe_parse 传参统一（education.rs）

- `education.rs:862` `self.send_maybe_parse(builder, method == HttpMethod::Get, ...)` → 提取局部 `let return_data = method == HttpMethod::Get;` 后传变量,与其余 13 处 `return_data` 变量形态一致。

### P0-5: map 函数引用改闭包（5 文件 6 处）

- `std::string::ToString::to_string` / `ToString::to_string` 函数引用 → `|v| v.to_string()`(forum:149、shop:154、retrieve:772/778/782、cloudvar:1382、converse:615)。

### P0-6: unwrap_or_else 字面量改 unwrap_or（3 文件 4 处）

- `unwrap_or_else(|| "-created_at".to_string())` → `unwrap_or("-created_at".to_string())` 等(forum:182、shop:185/208、compiler:666)。字面量无闭包求值,`unwrap_or` 语义等价。

### 附加优化 2: core 评论默认值四层收敛（retrieve.rs）

- 模块常量:`DEFAULT_COMMENT_STREAM_LIMIT=500`、`MAX_COMMENT_STREAM_LIMIT=1000`、`COMMENT_DETAIL_PER_WORK=20`;替换 `unwrap_or(500)`/`1000`/`Some(20)`;`with_page_size(15)` → `DEFAULT_PAGE_SIZE`(3 处)。
- 保留:`pipeline.rs` `comment_fetch_default_limit: 100` 是 CheckConfig 配置字段(与 retrieve 用户上限语义不同)。

## 判定不执行项（核实后记录理由）

| 条目                         | 判定       | 理由                                                                                                                                                                                                                                                 |
| ---------------------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1-1 分页构造统一 with_page  | **不执行** | 全仓 6 处 limit+offset 配对均为必填 i32 直接 to_string(无 Option 默认语义),`with_page` 需 `Some()` 包装反而更啰嗦;14 处异键(page/amount_items/page_number/current_page/page_size)服务端契约不同,`with_page` 固定产出 limit/offset 无法替换。零净收益 |
| P1-2 链式形态统一            | **不执行** | 9 处 `let mut builder` 全部是条件追加(`if let Some`/`if` 分支续接),`let mut` 是必要形态;全内联(85)与中间变量(67)是等价排版差异,rustfmt 已处理                                                                                                        |
| P1-3 错误构造调用侧统一      | **不执行** | 核实 forum.rs:144/419、auth.rs:314/329/422/473/601-678 全部是真分支错误(长度校验/非法 action/HTTP 状态/登录失败),直接 `Err(...)` 符合"仅真分支错误用 Err"规则;无字段缺失误用                                                                         |
| 附加 1 fetcher 实例化复用    | **不执行** | `FetchTotal`/`FetchGenerator` 是 fn 指针(`fn(ReportStatus) -> ...`),闭包必须无捕获,`XxxFetcher::new()` 在闭包内是唯一可行形态;fetcher 无状态(仅持 &'static CodeMaoClient),new() 零开销                                                               |
| 附加 3 apply_action 消费形态 | **不执行** | 三处语义不同:452 官方自动通过用 `?`(失败应中断)、485/507 单条/批量用 match(失败记录继续),统一会破坏错误处理语义                                                                                                                                      |
| 附加 4 ParseError 构造形态   | **不执行** | 四种形态对应不同语义:ok_or_else(Option 缺失)/map_err(i32 转换)/vec![Err](惰性流内)/直接 Err(线程 panic),全部正确;同形 i32 转换 2 处已一致                                                                                                            |
| 附加 1 services.rs 复用      | **不执行** | services.rs 的 `ReportFetcher::new()` 一次持有是局部优化,与 registry/retrieve 的闭包内 new 场景不同,无强制统一必要                                                                                                                                   |

## Critical files & anchors（执行后）

- `src/api/{community,forum,shop,user,library,education}.rs` — 文件顶部新增分页常量区,替换散落字面量。
- `src/api/whale.rs` — `build_report_paginated`(161)TIME 置首 + default_limit 参数;`add_timestamp_to_paginated` 已删。
- `src/core/retrieve.rs` — 模块常量区(评论流默认/上限/每作品抽样)+ `DEFAULT_PAGE_SIZE` 引用。

## Verification（实际执行结果）

- 每步 `cargo check` 0 error;最终 `cargo check --all-targets` 通过,`cargo test` 3 passed。
- 行为等价声明:P0-1 仅替换字面量为等值常量;P0-2/3 TIME 位置变化(查询参数顺序对服务端无影响,已核实同端点其余参数不变);P0-4 计算式提局部变量;P0-5/6 纯形态改写。
- 验证命令:`grep -rn "unwrap_or(4)\|unwrap_or(24)\|unwrap_or(200)" src/api/` 应返回 0(常量替换完成);`grep -rn "ToString::to_string" src/` 应返回 0(闭包替换完成)。

## Assumptions

- P0-2 education 辅助保留:41 处内联 TIME 是负优化,保留辅助作为教育域统一形态(与方案原意"删除辅助"不同,工程判断优先)。
- 各端点分页默认值视为服务端契约,常量仅命名不改变数值。
- 判定不执行项均经源码核实,若未来端点契约变化(P1-1 键名)或类型约束放松(附加 1),可重新评估。
