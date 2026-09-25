# 第二十五轮方案 — convert 实体级并行(方案 23 S3 拆分)

日期:2026-09-25 · 基线:`d355cbe`(方案 23 S1/S2 已落地)· 是 `docs/23` §3 P1-5 的子方案

> **状态:只出方案,待评审后执行。** 本文件是"大型更改先拆方案再评审"的那份方案
> (方案 23 §4 把 S3 定为"实体级并行(角色/场景)"),设计目标是**在产物逐字节不变的前提下**
> 用 `std::thread::scope` 并行,不引新依赖、不改公开 API 形状(只做加法)。

---

## 1. 为什么值得做(实测分布)

本机真作品样本,按"我方能独立处理的子树"统计(统计脚本:读编辑版 JSON,数各实体
`block_data_json.blocks` / `nekoBlockJsonList` 条数,以及 `procedures.proceduresDict` 各定义):

| 样本 | 方向 | 工作项(实体 / 程序集定义) | 积木合计 | **最大工作项占比** | 判断 |
| ---- | ---- | ------------------------ | -------- | ----------------- | ---- |
| `原气骑士 且听风吟_136021231.bcm4`(10.8 MB) | Kitten4 → KN | **209 个实体**(0 个程序集) | 12 764 | **4%**(555 块) | 极均衡,8 线程理论 ≈ 5–7× |
| `Phigros 自制谱模拟器_195038626.kn.bcmkn`(9.4 MB) | KN → Kitten4 | 59 个实体 + **4 个程序集定义** | 5 235 | **≈97% 在 `proceduresDict`**(实体侧仅 153 块) | 必须**并行程序集定义**,只并行实体几乎无收益 |

结论:两个方向都要拆工作项 —— 正向按实体、反向按"实体 + 程序集定义"。

现状(证据):`translate/mod.rs`(`convert_kitten4_document`)/`assembly.rs`
(`convert_kn_document`)都是 `for (id, entity) in map.iter_mut()` 串行;`translate_works`
只在**作品**粒度并行(`translate/mod.rs` `translate_works`,`batch_concurrency` 默认 1)。
内核耗时 debug 下 ≈ 137 µs/块,`core_ms` 在 10.8 MB 正向样本上 196–261 ms(绑核测),是当前
最大的单项可压缩空间(方案 23 §5 基准:`tests/convert_bench.rs`)。

---

## 2. 拦路石:`IdSource` 与 `TranslateReport` 是共享可变状态

- `model::IdSource { deterministic, counter, chars }`(`model.rs:321`):全流程**一个**
  `&mut`,每铸一个 id 就 `counter += 1`(`uuid()` 334 / `short()` 361)。产物里的新铸 id
  (影子、参数、程序集调用点)的**值**依赖"全局第几次铸造"。
- `TranslateReport.warnings: Vec<TranslateWarning>`:按发生顺序 push,`warnings()` 是**公开**
  访问器,顺序可观察。
- 因此"每实体一个线程 + 各自铸 id"会直接改变产物 id 与告警顺序 → 逐字节对齐失败。

---

## 3. 设计:临时 id + 串行改写(保住"与串行完全一致")

核心不变量:**全局第 k 次铸造 = 串行顺序下的第 k 次铸造**。串行顺序 = 工作项按
(方向规定的)固定顺序拼接、项内按树遍历顺序。只要逐项铸造序列可复现,就能并行。

**阶段 A(并行,项内自足)**:每个工作项(实体 / 程序集定义)拿自己的
`IdSource`(新建一个 `IdSource::recording(item_index)` 模式):
- 铸造时不取全局计数,而是产出**临时 id**:`"\u{1}prov:{item}:{seq}:{kind}"`
  (`item` = 工作项序号,`seq` = 项内铸造序号,`kind` = uuid/short)。
- 同时把该项的 `TranslateReport` 收集成**局部**报告(不 push 全局)。
- 阶段 A 内同时完成:解析(`kitten::parse_block_data_json` / `neko::parse_kn_entity`)、
  语义映射(`mapping::translate_kitten_to_kn` / `translate_kn_to_kitten`)、
  抽程序集(`neko::split_procedures`)。

**阶段 B(串行,零成本)**:按工作项顺序求前缀和 —— 第 i 项起点
`base_i = Σ_{j<i} mints_j`(mints 数由阶段 A 记录,确定性成立:同一棵树、同一映射代码
→ 同样的铸造序列与种类)。于是
`临时 id (i, seq, kind)` → 最终 id = `counter = base_i + seq` 处 `IdSource` 的取值
(`uuid()`/`short()` 都是 counter 的纯函数,见 `model.rs:334/361`)。

**阶段 C(串行/可并行,改写)**:把各工作项树与已编码块里的临时 id 替换为最终 id
(遍历替换字符串;临时 id 带哨兵前缀,不与真实 id 冲突)。随后 `rewrite_calls`(正向
第二遍,需要全局 `procedures`)**同样**按项并行、用同一临时 id 方案(它的铸造顺序
也要拼进全局序列,位置在阶段 A 之后 ✓ 与今天串行代码的顺序一致)。

**阶段 D(装配)**:仍按今天的顺序装配(`assembly::build_document` /
`build_kitten4_document`,后者已在上一次提交改成按值消费)。

**告警顺序**:阶段 A 的局部报告按工作项顺序拼接,等价于串行 push 顺序 ✓
(`TranslateReport::warnings()` 顺序不变,`counts()` 是聚合,天然无关)。

**并发入口**:`TranslateOptions::entity_concurrency(usize)`(新增,默认 1 ⇒ 默认行为、
产物、性能完全不变);`std::thread::scope` + 手工分块(与 `translate_works` 同风格,
不引 rayon)。分块策略:按"积木数"降序贪心装箱(最大工作项 4% 的正向样本几乎线性)。

---

## 4. 验收

1. **产物逐字节不变**:`tests/convert_bench.rs` 的四个样本 SHA256 与
   `tests/fixtures/translate/convert_bench_baseline.json` **完全相同**;
   差分门(`translate/mod.rs` `diff_tests`)+ 往返(`reverse_tests.rs`)全绿。
2. **并发确定性**:同一输入 `entity_concurrency = 1` 与 `= 8` 产物 SHA256 相同
   (新增单测,`deterministic_ids(true)`),且都等于**旧串行实现**的 SHA256(比方案 23 §4
   要求的"1 vs 8 相同"更强:连 id 值都不变)。
3. **加速比**:绑核基准下(避免调频噪声)正向 10.8 MB 样本 `core_ms` 目标 ≥ 3×(8 线程,
   最大项 4%);反向 9.4 MB 需先把 `proceduresDict` 拆成工作项,再测。
4. 报告新增一项:"工作项数 / 最大项占比 / 实际加速比",避免"看起来快了"的无据结论。

---

## 5. 风险与缓解

| 风险 | 证据/缓解 |
| ---- | -------- |
| **有绕过 `IdSource` 的直接铸 id** | 先全仓 `grep` `IdGenerator`、`fastrand` 在 `translate/**` 的使用;发现直接铸造点必须先收口到 `IdSource`(否则临时 id 方案漏改 → 产物不一致,基准会立刻抓到) |
| 铸造序列不可复现(依赖 map 迭代顺序) | 现有实现已假定 `serde_json::Map` 有序(BTreeMap)且遍历顺序固定(正向按 id 排序取根,见 `kitten.rs` `parse_parts`);单测"1 vs 8"若不一致即说明还有隐式顺序依赖,先定位再并行 |
| 改写临时 id 漏改(某些 id 已被写进字符串内部,如 mutation XML) | 临时 id 只在 **id 字段/引用**里出现;改写按节点字段走,并加"产物中不得残留哨兵前缀"的断言(便宜的兜底) |
| 线程数爆掉(作品级 × 实体级) | 两级预算:`translate_works` 的 `batch_concurrency` × `entity_concurrency`,在 `translate_works` 里按 `min(可用并发, 作品数)` 折算,并在文档写明(方案 23 §3 P2-11) |
| 并行下错误聚合 | 项内 `Result` 汇成 `Vec<Result<..>>` 后按序 `?` 收集,首个错误按**顺序**冒泡(与串行一致) |

## 6. 不做

- 不引入 rayon / crossbeam 等依赖,只用 `std::thread::scope`;
- 不改公开 API 形状(只加 `entity_concurrency`);
- 不在 id 方案未证明"与串行完全一致"之前合入(S3 是"要么全对要么回滚"的一步);
- 不顺带做方案 23 的 P1-6(`RawValue`)与 P1-7(单遍遍历),它们各自单独成轮
  (见 `docs/26`)。
