# 第五十四轮记录 — 2026-11-06 代码审计与优化整改(第一批)

## 0. 一句话

对 HEAD `911d5e7` 做功能与性能审计,确认举报引擎、粉丝统计、论坛回复三处默认分页上限缺陷,以及两处无界线程、回调句柄与分页重试缺陷;按优先级修复并逐个提交。

## 1. 范围与方法

- 范围:`src/core/{registry,pipeline,services,retrieve,cloudvar,converse}.rs`、`src/utils/{requests,socketio}.rs`、`src/api/{whale,user,forum}.rs`、`src/core/convert/**`。
- 方法:源码通读、`cargo test --lib` 全量、`cargo clippy --all-targets -- -D warnings`、一次性只读探针(读取分页迭代器的 `size_hint`/`remaining_items`,不发网络请求)。
- 产物类改动的门:`convert_bench` 的产物 SHA256 与 `#meta` 基线、实体并发 1 与 N 的同 SHA 门;不计入产物的修复以离线单测与 `cargo test --lib` 为准。

## 2. 功能缺陷

### 2.1 举报引擎每类型分页被截断

- 链路:`ReportFetcher` 的四类 `SourceConfig::fetch_generator` 均以 `None` 作为 `limit` 调用 `WhaleReportFetcher::fetch_*_reports_iter`;后者用 `limit.unwrap_or(15)` 转成 `build_report_paginated(..., 15)`,而 `build_report_paginated` 的行为是 `with_limit(15)`(`PaginatedIter` 的“最多获取元素数”,不是页大小)。`fetch_chunked` 对每类型只建一个生成器并持续取,生成器到 15 条即结束。
- 证据:只读探针读取 `fetch_work_reports_iter(..., None).remaining_items()`,结果为 `Some(15)`;另三类同构。
- 影响:`pending_session`、`pass_all`、`done_chunks` 三条消费路径都只能看到每类型 15 条;同类文档 `../goals/pending-decisions.md` B6 的“现默认移除上限”与 `../rounds/04-review-round2.md` 的“`None` 即全量迭代”均与实现不符;`terminal` 的“所有举报处理完成”文案因此可能误报。
- 修复:注册处显式传 `Some(requests::FETCH_ALL)`;补一条使用真实分页迭代器(或可注入桩)的回归测试,钉住“生成器不是单页上限”。

### 2.2 粉丝统计只取单页粉丝

- 链路:`DataQuery::compute_fans_by_like_threshold` 调用 `UserDataFetcher::fetch_followers_iter(user_id, None)`;该方法 `limit.unwrap_or(DEFAULT_PAGE_SIZE)`,默认页大小 15。
- 证据:探针读取 `fetch_followers_iter(..., None).remaining_items()`,结果为 `Some(15)`。
- 影响:返回结构中的 `total_fans`、`qualified_fans` 只覆盖前 15 个粉丝;粉丝数超过 15 时统计失真。
- 修复:调用方显式传 `Some(FETCH_ALL)`;与无界线程修复同批落地,避免修上限后线程数失控。

### 2.3 论坛回复流只取单页回复

- 链路:`CommentQueryBuilder::reply_items` 对论坛来源调用 `fetch_reply_comments_iter(..., None)`;该方法默认上限 `REPLY_PAGE_SIZE = 10`。
- 证据:探针读取 `fetch_reply_comments_iter(..., None).remaining_items()`,结果为 `Some(10)`;函数文档写的是“所有回复对象”。
- 影响:`stream_user_ids`、`stream_comment_ids`、`stream_detailed_comments` 对论坛来源静默漏掉第 11 条及以后的回复。
- 修复:调用方显式传 `Some(FETCH_ALL)`;补最小回归测试(可用分页迭代器的 `remaining_items` 断言,不发网络)。

### 2.4 两处“有界并行”实为无界线程

- 链路:`aggregate_user_comments_from_works` 对 `works.chunks(8)` 一次性 `s.spawn` 全部块;`compute_fans_by_like_threshold` 对 `qualified.chunks(16)` 一次性 `s.spawn` 全部块。线程数为 `ceil(n/chunk)`,与注释所称“有界并行”不符。
- 影响:作品数或合格粉丝数较大时线程数线性增长,每线程还串行发起多次 HTTP;在修复 2.2 的上限后会放大。
- 修复:改为固定 worker 数的工作队列,保持输入顺序;并发上界取配置常量并与仓库既有 `shared::batch_map` 的“块内并发、块间串行”语义对齐。

### 2.5 回调句柄与移除语义

- `CallbackHandle` 只是 `usize`;`CloudConnection` 的四个事件回调 store 各自从 0 分配 id,而 `remove_callback` 会把同一 id 从四个 store 全部删除,存在跨类型误删。
- `CloudVariable::on_ranking` 返回句柄,但没有对应的移除方法;排行榜回调无法取消,只能随连接一起丢弃。
- 各事件/变更/流式回调在分发时先 `take_all`/`mem::take`,锁外执行后再 `extend` 放回;回调内部调用移除方法时,移除作用于空 store,放回后旧回调被复活。
- 修复:句柄携带类别或使用连接级唯一 id;补排行榜回调移除入口;分发期间记录已移除句柄并在放回时过滤。

### 2.6 分页首屏错误不可重试

- `PaginatedIter::next_item` 在 `initialize()` 失败时把状态置为 `Finished` 并返回错误;后续页错误则恢复 `Ready`,允许再次 `next()` 重试。首屏因此是唯一不可重试的位置。
- 影响:与 README“单页瞬时错误可以重试”的承诺不一致;调用方重试同一迭代器时只能得到 `None`。
- 修复:首屏失败保留 `Uninit`(或引入 RetryInit 状态),下一次 `next()` 重试初始化;补错误桩单测。

### 2.7 `response_offset_key` 无实现

- `PaginationConfig::response_offset_key` 与 `with_response_offset_key` 存在,但全仓无读取点,设置后无效果。
- 处置:实现“按响应偏移修正下一页”需要明确服务端契约;若无使用方,按预 1.0 约定删除该公共入口并在提交信息中记录破坏性变更。见 `../goals/optimization-plan.md` P1-4。

## 3. 性能与资源缺陷

### 3.1 门面 `translate_work` 未接流式产物

- `translate_work_in` 走 `translate_value`(产物 `ProductDocument` 被 `into_value()` 物化)再 `FileService::write_json`;文件入口 `translate_file`/`translate_text` 已有流式产物分支。
- 正向从 `&mut Value` 生成 `ProductDocument` 的私有实现已存在,只缺一个 `pub(super)` 薄包装;反向与 NEMO 的 product 入口已有。`ProductDocument` 缺写顶层 `source` 的方法,而 `keep_source` 默认开启。
- 既有 `convert_facade_bench` 的 590 ms 对 637 ms 不能隔离产物侧流式收益(旧流程含读回改写,内存流程含整份源解析);结论需要同一内存源文档的三腿 A/B 支撑。
- 处置(2026-11-06,`48890df` / `6b9e5cc`):补 `translate_value_product` 与 `ProductDocument` 顶层字段写入入口,门面按方向选择 product;`product_value_equivalence_tests` 守三条方向的逐字节等价;`facade_product_bench` 记录三腿读数(`bench_perf` 绑核复跑:内存 product 376–379 ms,内存物化 456–470 ms,约 1.20–1.30 倍;见 `../knowledge/convert-performance.md` §2bis.17)。

### 3.2 NEMO 内存形态整份深拷贝

- `NemoDecompiler::editable_document` 对 `Arc<Value>` 执行整份 `clone`;调用点持有 `RawWorkData` 所有权,trait 方法收 `&RawWorkData`。
- 处置:trait 方法改收 `&mut RawWorkData`,用 `Arc::get_mut` + `std::mem::take` 在唯一所有权时移出文档,非唯一时保留 clone 回退。

### 3.3 `cloudvar` 断线 flush 抖动与关闭阻塞

- `flush_loop` 在未连接时以 `flush_interval`(默认 100 ms)为周期反复 drain/requeue 整个队列并打 `warn`;队列无上限。
- `on_connection_lost` 在重连退避中 `thread::sleep`,而 `close()` 会 join 读线程,关闭可被退避阻塞。
- 处置:未就绪时等待连接就绪通知并退避,队列设上限或合并;重连退避改为可被 `stopping` 打断的等待。

### 3.4 库内嵌路径与客户端注入

- `PathConfig::global()` 长期是无 setter 的 `OnceLock`;`PathConfig::with_root` 在生产代码零消费;`translate_work` 的 staging 硬编码全局转化目录,即使设置 `TranslateOptions::output_dir` 也只影响产物;`translate_work`/`translate_works` 只用全局反编译器和客户端。
- 处置(2026-11-06,`68dfb10` / `a8a4925`):新增 `PathConfig::set_global`,必须在任何全局路径使用前调用;新增 `translate_work_with_client` / `translate_works_with_client`,把注入客户端贯穿反编译、源文件上传与建草稿;staging 在指定输出目录时跟随 `output_dir`。默认入口仍使用全局客户端与全局反编译器。

### 3.5 `translate_file` 网络选项与源引用失败报告

- `TranslateOptions::upload` / `keep_source` 同时挂在文件入口与门面上,但文件入口不碰网络,设置后静默无效;`keep_source` 的上传失败只写日志,未进入 `TranslateReport`。
- 处置(2026-11-06,`53c0ef8`):两个开关的 rustdoc 明确“仅门面生效”;新增非损失类 `TranslateWarning::SourceReferenceFailed`,门面在上传或引用注入失败时写入报告,`strict` 不会因此失败。

## 4. 文档与测试缺口

- `../goals/pending-decisions.md` B6 的“现默认移除上限”与 §2.1 实现不符;已按 `0324b4c` 改为“现行已全量”,并把 `work.rs` 行数同步为 2026-11-06 的读数。
- `../rounds/04-review-round2.md` 的“`None` 即全量迭代”结论在 `0324b4c` 前未真正生效;历史正文不改写,勘误见 `../knowledge/errata.md`。
- `../knowledge/nemo-runtime-and-upload.md` §6 的“body 无读超时、未修”已被 `../knowledge/errata.md` 推翻;2026-11-06 已改为三段预算的正确表述。
- `../knowledge/repo-conventions.md` §7 称 pre-commit 会执行 fmt/clippy/test;2026-11-06 已改为“只执行 `repo_hygiene`,完整离线门在 CI”。
- `TranslateOptions::output_dir` 的文档写“编译目录”,实际默认产物落在 `convert` 目录;2026-11-06 已改。
- 全量库单测可通过,但上述路径缺少针对性回归;第一批已补举报生成器上限、回调句柄、首屏分页重试、product/内存逐字节等价四类测试。

## 5. 后续候选

### 5.1 `ide/load` 编辑格式快速通道(待核验)

- 匿名请求实测 401,文档称按权限返回 `source_urls`;转换域当前没有调用点,不能把 `ide/load` 当作默认路径。
- 多版本排序无接口契约。本地 20 份 k4edit 的 `application_version`:`174408420` 的 0 号是 4.11.18、1–9 是 4.11.12;`215246857` 的 0/1 是 4.11.13、2–9 是 4.11.12。块数上 `215246857-0` 与反编译夹具同为 374 块;`174408420-0` 为 3237 块,与仓内反编译夹具的 2376 块不一致(夹具版本较旧),不能据此判断同版等价。
- 进入条件:用带权限的账号确认 `source_urls` 中当前版本的选择规则、空列表/404 回退,做同版语义等价与块数/程序集比对,并重建 `convert_bench` 基线。

### 5.2 两个独立作品源串行请求(已完成(2026-11-06,`0aee1e6`))

- `stream_works_from_both_sources` 的两个来源已在同一 `thread::scope` 内并发获取,结果顺序仍为 NEMO→Web。

### 5.3 目录上传逐文件串行(已完成(2026-11-06,`5e4be02`))

- `FileProcessor::handle_directory_upload` 已改为固定批次并发;失败清单与返回值语义不变。

### 5.4 举报分页死分支(已完成(2026-11-06,`6e665f8`))

- `fetch_chunked` 的恒空 `pending_items` 分支已删除;分块行为由既有测试固定。

## 6. 实施记录(2026-11-06,第一批)

| 序 | 提交 | 内容 | 验证 |
| --- | --- | --- | --- |
| 1 | `0324b4c` | 四类举报生成器显式传 `FETCH_ALL`;新增生成器上限回归测试 | `core::registry::tests` 2 passed;`cargo test --lib` |
| 2 | `4fa07ad` | 粉丝统计与论坛回复显式取全量 | 编译与离线测试;注释记录 `None` 语义 |
| 3 | `9cd837f` | 评论聚合与粉丝统计改为固定批次并发 | `clippy --all-targets -D warnings`;`cargo test --lib` |
| 4 | `a5c44e3` | 回调句柄进程级唯一;分发改快照;补 `remove_ranking_callback` | `utils::socketio::tests` 2 passed;`cargo test --lib` |
| 5 | `77a2869` | 分页首屏错误保留 `Uninit` 供重试 | 本地 HTTP 桩测试 `first_page_error_is_retryable` |
| 6 | `fd45861` | 删除无实现的 `response_offset_key` 公共入口 | 编译与离线测试;破坏性公共面变更 |
| 7 | `2d83e33` | NEMO 内存编辑版在 Arc 唯一时移出,免深拷贝 | `nemo_decompiler_offers_in_memory_editable_document` 断言文档已移出 |
| 8 | `53c0ef8` | 源引用失败写入 `TranslateReport`;网络选项生效范围写入 rustdoc | `translate::report::tests` |
| 9 | `48890df` | 门面改用 `translate_value_product`;补 product/内存逐字节等价测试 | `product_value_equivalence_tests` 三条方向通过 |
| 10 | `105b7f0` | 重连退避可被 `close` 打断;未就绪时不再按短周期搬移队列 | `clippy --all-targets -D warnings` |
| 11 | `0aee1e6` | NEMO/Web 两个作品源并发获取 | 编译与 clippy |
| 12 | `6e665f8` | 删除举报分页恒空的 `pending_items` 分支 | `core::registry::tests` |
| 13 | `5e4be02` | 目录上传改为固定批次并发 | 编译与 clippy |
| 14 | `72bd47b` | 目标/知识库/轮次索引同步;`cargo test --lib` 全量 158 passed、1 ignored | 成功 |
| 15 | `68dfb10` | `PathConfig::set_global`;全局路径根可在启动期重定向 | `utils::filedata::tests` |
| 16 | `a8a4925` | `translate_work_with_client` / `translate_works_with_client`;staging 跟随 `output_dir` | `staging_dir` 两测例;clippy |
| 17 | `e6a635f` | README/知识库登记路径根与注入门面、三腿读数与 P2-4 收口 | 文档检查 |
| 18 | `6b9e5cc` | 门面三腿性能探针 | `facade_product_bench`(ignored)读数入 `../knowledge/convert-performance.md` §2bis.17 |

## 依据

- 审计对象:HEAD `911d5e7`;源码符号与只读探针读数见各节;全量库单测 152 passed、1 ignored。
- 相关目标:`../goals/optimization-plan.md`;相关约定:`../knowledge/repo-conventions.md`、`../knowledge/errata.md`。
