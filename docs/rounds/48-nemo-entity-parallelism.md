# NEMO 方向的实体级并行(2026-10-05)

范围:`src/core/convert/translate/{nemo.rs,nemo_mapping.rs,pipeline.rs,report.rs,options.rs,nemo_tests.rs}`。

结论:NEMO 的两处实体循环改成正向同构的四阶段(临时 id 记录 → 串行兑现 → 并行兑现与编码 → 串行装配)。
**默认并发 1 下产物逐字节不变**(`convert_bench` 的 SHA256 与 `#meta` 全绿,该基线是串行实现时代写下的);
并发 8 下同轮对照:`nemo-3.4MB` 的 `core` **1.44×**、`e2e` **1.31×**,`nemo-old-1.5MB` 两项均 **1.21×**。
代价:NEMO 腿的**分配次数 +13.9%**(记录法的临时 id 与兑现表;正向同款设计,见 §4)。

## 1. 背景与选定依据

NEMO 是 `47` 轮 §2.2 六行目标里唯一没达标的方向,而当时的候选(产物侧流式写出)要写**第三套**写出器
(上界 17%–20%)。本轮先按"先采一次符号级剖面再决定做什么"的纪律量了一次(`../knowledge/convert-performance.md` §2bis.11):

| 观测 | 值 |
| --- | --- |
| `convert_nemo_document` 占整轮样本的 self | 9.47% |
| 其中每实体工作(`parse_entity` 68% + `tree_to_json` 15%) | **占 NEMO `core` 的 83%** |
| 实体负载分布(`nemo-3.4MB` 演员 847 个) | 最大实体仅占 **2.2%**、前 3 占 6.4% |
| 实体负载分布(`nemo-old-1.5MB` 演员 280 个) | 最大 7.1%、前 3 占 17.4% |
| 现状 | `run_items` 只服务正向;`entity_concurrency` 对 NEMO 不生效(基准里 NEMO 的并发腿恒 1.00×) |

⇒ "每实体 83% + 负载极均"是并行最有利的形状,且设计可整套复用正向已验证的做法,故先做这一步,
产物侧流式写出留作后续(两者正交:一个降 CPU 时间、一个降分配)。

## 2. 方案(四阶段,与 `pipeline::convert_kitten4_document` 同构)

1. **阶段 0(串行)**:收集工作项 —— 先演员后场景(顺序与旧实现一致),每项带源对象、`NemoEntity`、
   是否场景、权重(= `blocksXML` 字节数,单实体工作量与它成正比);
2. **阶段 1(并行)**:每项一份 `NemoParseContext`(程序集表与广播字典是 `Arc`,每项只加一次引用计数)
   加 `IdSource::recording(index)` 与局部 `TranslateReport`,解析 + 前置改写 + 语义映射,产出**临时 id** 的树;
3. **阶段 2(串行)**:按项序把临时 id 兑现成最终 id(程序集的 id 已在它那一步用真源铸好,排在前面)
   ⇒ 铸造顺序与串行实现逐次一致;
4. **阶段 3(并行)**:`remap_tree` 兑现 + 组装实体条目 + `tree_to_json`;**阶段 4(串行)**:按项序装两个
   容器、按「每项 解析期 → 装配期」并入报告与计数。

三个必须守住的口径:

- **只置不清的上下文**:官方 `current_actor`/`current_params` 只置不清(`NemoParseContext` 的文档),
  实体阶段开始时仍带着程序集阶段的残留 ⇒ 每个工作项先照原值播种,行为与串行逐项跑一致;
- **QC 迁移的字符串**:`qc_audio_blocks` 注入节点的 id 会落进"迁移后的整份 XML"快照(它早于 `replace_*`),
  那一处也必须走同一张改写表,否则临时哨兵会写进产物 —— 单测扫整份产物守(§5);
- **键序与完成顺序无关**:容器是 `serde_json::Map`(= `BTreeMap`,无 `preserve_order`),产物字节不随
  并行完成顺序变化。

## 3. 落地(改动面)

| 文件 | 改动 |
| --- | --- |
| `translate/nemo.rs` | 两处实体循环 → 上述四阶段;新增工作项/中间结果结构;`report.entity_workers` 接线 |
| `translate/nemo_mapping.rs` | `NemoParseContext::{procedures,broadcast_names}` 改 `Arc<BTreeMap<…>>`(读侧不变,写入点各一处) |
| `translate/pipeline.rs` | `remap_string` 放开为 `pub(super)`(NEMO 的迁移字符串复用它) |
| `translate/{report,options}.rs` | `entity_workers` / `entity_concurrency` 的文档写明 NEMO 方向同样生效 |
| `translate/nemo_tests.rs` | 新增真作品门(并发 1 与 8 逐字节相同 + 真的开了线程 + 无临时哨兵);QC 用例补哨兵断言 |

## 4. 读数(2026-10-05,`taskset -c 0-3`,可用核数 4;同轮内 1 vs 8 对照)

| 样本 | `core` | `e2e` | 实际线程 | 分配次数(1 / 8) | 产物 SHA256 |
| --- | --- | --- | --- | --- | --- |
| nemo-3.4MB | 282 → **196 ms(1.44×)** | 398 → **303 ms(1.31×)** | 1 → 4 | 1 541 425 / 1 541 583 | 与基线一致 |
| nemo-old-1.5MB | 85 → **70 ms(1.21×)** | 126 → **104 ms(1.21×)** | 1 → 4 | 472 850 / 472 974 | 与基线一致 |
| kitten4-10.8MB(对照) | 275 → 165 ms(1.67×) | 433 → 318 ms(1.36×) | 1 → 4 | 909 987 / 910 168 | 与基线一致 |
| kn-9.4MB / kn-3.7MB(反向) | 1.00× / 1.07× | 1.00× / 0.95× | 1 | 两次相同 | 与基线一致 |

- **分配代价**:NEMO 串行腿的分配由改动前的 1 353 318 涨到 **1 541 425(+13.9%)** —— 记录法每铸一个 id 多付
  「临时 id + 兑现表键 + 改写后替换」几次分配,与正向并行(方案 25 S3a)是同一套设计、同一量级代价
  (`rounds/25` §9 对正向的结论是"默认并发 1 的串行开销本机测不出")。因此 `47` 轮 §2.2 的 NEMO 分配目标
  (≤1 300 000)距离更远;这也让"NEMO 产物侧流式写出"这一项更值钱(它降的正是分配)。
- **机器态声明**:本轮跨日/跨跑漂移很大 —— 同一份未改动的正向代码,`core` 在本会话两次读数相差 35%
  (204 → 275 ms)。因此本节只采信**同轮内**的 1 vs 8 对照;"默认并发 1 是否变慢"未做 worktree A/B
  (与 `rounds/25` §9 对同款路径的判定一致:本机不可测)。

## 5. 门与不变量

- **字节门(最强)**:`convert_bench` 严格模式的产物 SHA256 与 `#meta` 全绿,且基线来自**串行实现时代**
  ⇒ 默认路径改走临时 id 机制后逐字节不变;并发 1 与 8 同 SHA256。
- **单测**:`cargo test --lib` 141 项(新增 `nemo_entity_parallelism_is_byte_identical_and_really_parallel`:
  并发 1 与 8 逐字节相同 + `entity_workers > 1` 挡"空门" + 产物无临时哨兵);QC 迁移用例补
  "整份产物不得残留哨兵"(那条覆盖 `migrated_xml` 字符串的改写)。
- **调试断言**:临时 id 记账槽位唯一(`debug_assert`)、阶段 4 结束时未命中数必须为 0(同正向)。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 全绿。

## 6. 不做与后续

- **反向(KN → Kitten4)实体级并行**仍不做(`rounds/25` §10:Amdahl 上限 1.9×、反向 `core` 仅约 200 ms)。
- **NEMO 产物侧流式写出**仍待决(`../goals/convert-backlog.md` §1);本轮不动它,但它与本轮的收益正交,
  且因本轮抬高了分配基线而更有价值。
- 未做 worktree A/B 的"默认并发 1 时间开销"不列为结论。
