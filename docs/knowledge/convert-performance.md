# 转换与反编译性能(实测基线)

> 知识库条目:**已经量到的数字、瓶颈归因、已落地优化及其收益、判定不做的项**。
> 方案与执行记录见 `docs/rounds/22-*`(NEMO 反编译)、`docs/rounds/23-*`(转换总体)、`docs/rounds/25-*`(实体级并行)、`docs/rounds/29-*`(扫描台账)。

## 1. 基线(实测)

| 场景 | 优化前 | 现在 |
| ---- | ------ | ---- |
| NEMO 作品反编译(含资源下载) | **2m42s** | **13s**(并发 + `skip_resources`) |
| KN 作品反编译(模式分派) | 46s | **1.5s** |
| Kitten4 作品反编译 | 单块 `from_value` | **1.5s**(逐块克隆改掉) |
| 转换 Kitten4→KN(离线、确定性) | — | **38 ms**(374 输入积木 → 439 产物节点) |
| 转换 NEMO→KN(离线、确定性) | — | **core 365–442 ms / e2e 466–556 ms**(3.5 MB 源 → 7.7 MB 产物;15 482 源元素 → 13 637 产物节点) |
| 转换 NEMO→KN(老版本 0.11.0,**含 YC 迁移**) | — | **core 108–121 ms / e2e 142–152 ms**(1.5 MB 源 → 2.4 MB 产物;4 862 源元素 → 4 421 产物节点) |
| 转换(官方 JS 同输入参照) | 355 ms(Node) | — |
| 资源删除(批量) | 7.9s | **5.3s**(并行 delete) |

## 2. 瓶颈归因(按证据)

1. **请求数 × RTT**,不是 CPU:资源下载耗时几乎全部在这里。实测曲线(同一 NEMO 作品):

   | 并发 | 1 | 8 | 16 | 32 |
   | ---- | - | - | -- | -- |
   | 耗时 | 402 s | 103 s | **92 s** | 127 s + **CDN 限流丢 2 个文件** |

   ⇒ 最优区间 8–16。下载是 I/O 密集,**不能用 `available_parallelism` 折算**(低核机器会折到近串行、高核机器会放宽到限流区)。本库把"作品级 × 单作品资源级"总线程**封顶 16**(常量 `RESOURCE_DOWNLOAD_BUDGET`)。
2. **`locate_resource` 在 hot path 上占比最大**(转换剖析里约 2/3)。
3. **NEMO 逐条 XML 解析**:3.5 MB 在 Node 下要 4–5 分钟(逐条 `DOMParser`);Rust 侧按"单文件秒级"设计。
4. **整份文档 JSON 三进三出**:`translate_file` 先 `read_to_string + from_str`,产物再 `to_string + write`;`translate_work` 还多一轮"落盘→读回"。

## 3. 已落地的优化(都有数字)

| 优化 | 做法 | 收益 |
| ---- | ---- | ---- |
| 资源下载并发 | 分块并发,总线程封顶 16 | 4× 级(402 → 92 s) |
| `skip_resources` | 只要积木/结构时跳过资源下载 | 53× 级 |
| 内存直通 | `DecompiledArtifact` 直接交给 translate,避免落盘→读回 | 省一次 serialize + 一次 parse(10 MB 级 ≈0.3–1.0 s) |
| 去逐块克隆 | `BlockJson::from_value` 不再 `obj.clone()` | 12 764 块受益 |
| **实体级并行(正向)** | 临时 id + 串行兑现/改写;`entity_concurrency` 默认 1 | 10.8 MB:`core` 285 → 184 ms(**1.55×**)、`e2e` 696 → 590 ms(1.18×);**并发 1 与并发 N 产物 SHA256 相同** |
| 两级预算折算 | `entity_concurrency = min(请求, 可用核数 / 有效作品并发)` | 防止 `batch × entity` 超订;只改并行度不碰产物 |
| 反编译侧并发折算 | `decompile_batch` 作品级 × 资源级总线程封顶 16 | 见 §2 |

## 4. 判定**不做**(有证据)

| 项 | 结论 | 理由 |
| -- | ---- | ---- |
| 反向(KN→Kitten4)实体级并行 | **不做** | 63 个工作项、最大一项占 **36.7%**,且两段必须串行(`unrewrite_calls` 依赖全局 `call_targets`、`def_root_from_entry` 把定义根挂进宿主实体)⇒ Amdahl 上限 1.9×,**实际远低于 1.5×**,而反向 `core` 只有 ~200 ms |
| `RawValue` 顶层只透传 | **不做** | 透传占比 ≈0%(见 `convert-semantics.md` §7) |
| 单遍遍历合并 | **不做** | 只省遍历,不省逐块匹配/字段改写 |

## 5. 基准方法(避免自欺)

- **绑核**:`taskset -c 0-3`,**5 轮取最小**(本机 2 物理核 / 4 逻辑核);报告要区分 `core` 与 `e2e`。
- **字节门**:并发优化必须证"并发 1 与并发 N 产物 SHA256 相同",再谈加速比。
  样本现为 **6 个**(4 Kitten + 2 NEMO);NEMO 那两件的 `#meta` 带 `source_version`
  (`0.11.0` 那件 < 0.15.0 ⇒ **YC 版本迁移真的进基线**;`0.16.2` 那件等于迁移目标版本,迁移是 no-op)。
- **`#meta` 与产物 SHA 是两类不同性质的信号**(判断"回归 vs 夹具问题"就看这个):
  - `#meta` 不一致 = **输入夹具被换 / 元信息漂了**(`source_sha256`/`source_bytes`/块数/告警数);
  - **产物 SHA 不一致 = 行为/产物变了**(真正的回归)。
  两者都不通过时,**修 `tests/convert_bench.rs` 的断言顺序**(`#meta` 先 panic 会遮住产物 SHA 的红;
  已派单改为"收集完所有不一致再一次性报、并区分两类"——2026-10-02 的一次误判就是这么来的,
  见 `../rounds/40-bench-fixture-discipline.md` §3.1)。
- **两种门要分清(一遍知就够)**:
  - **可复现门**:样本来自可控采集(`download/compile/**` 等)⇒ 产物 SHA 原则上可复现,变了就是行为变了;
  - **快照冻结门**:`kn-9.4MB` 的产物 SHA **是输入快照的函数** —— 夹具里的实体/积木 id 是**随机 UUID**
    (未开 `deterministic_ids`),而反向会把源实体 id 带进产物 ⇒ **重新采集(或再跑一次产它的真机门)
    就必变 SHA**,必须再走一次**有据刷新**(`BACKEND_BENCH_REFRESH=1` + 逐键解释)。
    详见 `../rounds/40-bench-fixture-discipline.md` §3。
- **无数据不做**:任何"看起来更快"的改动,要么有基准点,要么承认在噪声内(例:某次优化 `core` 259 vs 261 ms 属噪声,保留它的理由只是"构造上更少分配")。
- 官方差分门只做**语义比较**(官方从不逐字节对齐)。

## 依据

- `docs/rounds/22-nemo-decompile-performance.md` §1–§4(现象/根因/实测 A-B)、§7(复现)。
- `docs/rounds/23-convert-performance-plan.md` §1–§3、§5、§7(落地记录)。
- `docs/rounds/25-convert-entity-parallelism-plan.md` §1(分布)、§9(正向落地)、§10(反向判不做)。
- `docs/rounds/29-optimization-scan-ledger.md`(P0/P1 台账)。
- 代码锚点:`src/core/convert/decompile/mod.rs`(`RESOURCE_DOWNLOAD_BUDGET`)、`src/core/convert/mod.rs`(两级预算折算)、`tests/convert_bench.rs`(自有 SHA256 基线:6 样本,4 Kitten + 2 NEMO)。
