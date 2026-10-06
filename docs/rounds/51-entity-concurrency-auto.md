# 实体级并发改为"默认自动"(2026-10-06)

范围:`src/core/convert/translate/{options.rs,pipeline.rs,nemo.rs,report.rs}`。

结论:`TranslateOptions::entity_concurrency` 的默认值从"固定 1"改成**自动** —— 作品够大才开;
显式给数仍是固定值(`1` 与 `0` 都等于强制串行,老口径不变)。阈值由**一次性探针**夹逼标定:
374 条源积木无收益、1 491 条起 `e2e` 就有 1.30–1.51×,故取 **1 000 条**(字节口径 400 KB)。
产物不变由基准的"并发 1 vs 8 同 SHA256"门守(它的两侧都显式传值,行义不受本次默认值变更影响)。

## 1. 背景

`../goals/convert-backlog.md` §1 的待决项原文:"默认值为 1,由使用者显式传入;自动开需定阈值。
该选项现覆盖正向与 NEMO 两个方向(`../rounds/48`:同轮 `core` 1.44×/1.21×、`e2e` 1.31×/1.21×);
NEMO 侧因记录法使分配 +13.9%,阈值决策要把这点一并权衡。"

已有的 1 vs 8 读数(同轮)分布在两端:正向 0.3 MB(374 条)1.00×、正向 10.8 MB(12 764 条)核心 1.55×、
NEMO 1.5 MB(4 421 条)1.21×、NEMO 3.4 MB(13 637 条)1.44× —— **交叉点在 374 与 1 491 之间是空的**,
阈值无从凭空定 ⇒ 先按仓库纪律采一次探针(`rounds/37` §4:"先量后改")。

## 2. 阈值标定(一次性探针,跑完即删)

装置:`taskset -c 0-3`(可用核数 4)、`--profile bench_perf`、同一探针内 `entity_concurrency` 1 与 8
各预热一轮 + 3 轮取最小(与基准同口径);样本取本地语料的中段(1.3 / 1.6 / 3.2 / 6.0 MB)。

| 样本 | 源块 | `core` 1 → 8 | `core` 加速 | `e2e` 1 → 8 | `e2e` 加速 |
| --- | --- | --- | --- | --- | --- |
| k4-1.3MB | 1 491 | 28 → 18 | 1.56× | 48 → 35 | **1.36×** |
| k4-1.6MB | 2 063 | 111 → 66 | 1.68× | 148 → 97 | **1.51×** |
| k4-3.2MB | 3 497 | 85 → 57 | 1.49× | 146 → 112 | **1.30×** |
| k4-6.0MB | 7 762 | 153 → 84 | 1.82× | 258 → 192 | **1.34×** |

**读法**:收益在 **1 491 条**就已经是 1.36×,比"374 条无收益"那一点外推出来的预期更早;而 374 条那个样本
本身 `core` 只有 6 ms(e2e 10 ms)—— 它测不到收益是**量级问题**(可并行的那点工作比固定开销还小),
不是"并行没用"。⇒ 阈值取两点之间:条数口径 **1 000 条**;字节口径按同一**意图**取 **400 KB**
(换算依据:正向骨架路径 ≈760 B/块、NEMO ≈250 B/块)。

## 3. 方案:取值与判据分离

**选项层**(`options.rs`):

| 取值 | 含义 |
| --- | --- |
| `Auto { cap }`(**默认**) | 作品够大才开;`cap` 是批量入口折算出的核数上限(`None` = 未折算) |
| `Fixed(n)`(显式 `entity_concurrency(n)`) | 固定线程数;`n = 0/1` 都是强制串行(与老口径一致) |

`fold_entity_concurrency`(作品级 × 实体级的预算折算)对两种取值分头处理:**固定值**折成
`min(请求, 可用核数 / 有效作品并发)`;**自动**只压低核数预算(`cap = Some(share)`),
**不动"够不够大"的判断** —— 阈值按作品大小判,与作品级并发无关。

**管线层**(`pipeline::entity_workers(plan, items, total_weight, unit)`):

- `Fixed(n)` ⇒ `min(n, 工作项数, 可用核数)`;
- `Auto` ⇒ 先判"够大":`Σweight ≥ 阈值`,阈值按权重口径二选一(`WeightUnit::{Bytes, Blocks}`);
  够大才用 `min(cap, 工作项数, 可用核数)`,否则 **1**。

权重的口径是既有事实(只服务并行装箱,不进产物):**正向原文(骨架)路径与 NEMO 是块表原文字节数**,
**`translate_value(Value)` 内存路径是源积木条数** ⇒ 阈值相应给两个常量(注释写明换算依据)。

**可观测**:`report.entity_workers`(既有字段)仍报"实际开了几个线程" ⇒ "自动真的开了"能被单测与基准挡住。

## 4. 落地(改动面)

| 文件 | 改动 |
| --- | --- |
| `translate/options.rs` | 新增 `pub(super) enum EntityConcurrency`;字段改类型、默认 `Auto { cap: None }`;`entity_concurrency(n)` → `Fixed(n.max(1))`;新增 `entity_concurrency_plan()`;`fold_entity_concurrency` 分头折算;rustdoc 重写(取值、代价、反向不生效) |
| `translate/pipeline.rs` | 新增 `WeightUnit` 与阈值常量 `AUTO_MIN_BLOCKS`/`AUTO_MIN_BYTES`、`entity_workers(plan, items, total_weight, unit)`;正向调用点按路径给单位并传 `Σweight` |
| `translate/nemo.rs` | 调用点改走 `entity_workers`(单位 = 字节) |
| `translate/report.rs` | `entity_workers` 的文档改准(默认自动;反向恒 1) |
| `translate/pipeline.rs`(tests) | 折算门改为断言**取值**;新增阈值门与真作品"自动开"门 |

## 5. 守门

| 门 | 内容 |
| --- | --- |
| `auto_entity_workers_respect_threshold` | 阈值是硬边(差一条不开,两种口径各测);够大时受工作项数与核数夹取;折算预算优先;固定值不看阈值;显式 1 = 串行 |
| `auto_entity_concurrency_turns_on_for_large_real_work_only` | 真作品:10.8 MB(12 764 条)默认**并行**、0.3 MB(374 条)默认**串行**(缺夹具时跳过并点名) |
| `entity_concurrency_is_folded_by_work_and_core_budget`(改) | 断言从"线程数"改为"取值":默认(auto)只被压低预算,不被折成固定值 |
| `convert_bench`(不变) | 产物 SHA256 与 `#meta` 与基线一致;每样本"并发 1 vs 8 同 SHA256"(**基准两侧都显式传值**,故本次默认值变更不影响它的行义) |

**"自动"这条新默认路径的产物不变,由上面两条合起来覆盖**(如实记):线程数解析由
`auto_entity_workers_respect_threshold` 与真作品门钉住,而"线程数不同不改产物"由 `convert_bench` 的
1 vs 8 同 SHA256 门(自动在本机与显式 8 算出同一线程数)与 `real_work_matches_serial_reference_when_sample_present`
(1 / 8 / 测试内串行参考三者逐字节相同)共同守住 —— 没有单独造"默认 vs 显式 1"的门。

未覆盖的边界(如实记):阈值在 (374, 1 491) 之间的**逐点**读数没有(本地语料在该区间没有样本),
阈值取的是中点;§6 给复核条件。

## 6. 代价与边界

- **大作品默认会多付 NEMO 侧的分配**(+13.9%,`../rounds/48` §4),换来 1.2–1.5× 的时间 ——
  这正是 §1 里"要一并权衡"的那一项,本轮的选择是**按时间收益开**,并把分配代价收窄到"够大的作品";
- **小作品仍串行**(374 条那档),不再由使用者自己记得关;
- **反向(KN → Kitten4)不受影响**(没有实体级并行,`report.entity_workers` 恒 1);
- **`translate_value(Value)` 内存路径**的阈值走"条数"口径(1 000 条),与文件路径的字节口径
  (400 KB)是同一意图的两个常量;两者都写在同一处并注明换算依据。

## 7. 未做

- 阈值在 (374, 1 491) 之间的逐点复核:拿到该量级样本时按同轮 A/B 复标(探针脚本是一次性的,已删;
  复跑按本节 §2 的装置重建即可);
- 作品级 × 实体级的预算折算沿用既有口径(`fold_entity_concurrency`),不改;
- NEMO 侧"自动开"的**分配代价**没有做新的规避(如让记录法更省),按 `../rounds/47` 的纪律先量后改。

## 依据

- 代码:`src/core/convert/translate/{options.rs,pipeline.rs,nemo.rs,report.rs}`(符号:`EntityConcurrency`、
  `TranslateOptions::{entity_concurrency_plan,fold_entity_concurrency}`、
  `pipeline::{entity_workers,WeightUnit,AUTO_MIN_BLOCKS,AUTO_MIN_BYTES}`);测试:`pipeline::forward_parallel_tests::*`。
- 读数:一次性探针(`/tmp` 临时件,跑完即删;装置见 §2)+ 既有四条 1 vs 8 读数
  (`../rounds/48` §4、`../knowledge/convert-performance.md` §2bis.12)。
- 决策出处:`../goals/convert-backlog.md` §1(原待决行)。
- 落地提交:`0ead279`(代码 + 门);本篇记录与三个库的同步在其后单独提交。
