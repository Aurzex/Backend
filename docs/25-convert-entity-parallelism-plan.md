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
   **落地实测口径**:本机 `available_parallelism = 4`,故实际线程数被夹到 4,加速比按 4 线程记
   (§9.5);方案给的 3× 目标按 8 线程,本机只能验到 4 线程的等效值。
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

---

## 7. 子代理评审结论(2026-09-25,ReviewS3Parallel)

**判定:有条件可行。** 原设计对**正向**成立(修掉 3 处细节),对**反向**不成立,须重设计。
以下逐条记评审给的证据与修法(均已核对到 `文件:行号`)。

| # | 阻塞问题 | 证据 | 修法 |
| - | -------- | ---- | ---- |
| 1 | **反向管线在方案里缺席**:阶段 A 只写 `translate_kn_to_kitten` + `split_procedures`,而反向没有 `split_procedures`;`unrewrite_calls` 逐实体/逐程序集**按全局顺序**铸 id 且依赖全局 `call_targets`;`def_root_from_entry` 把程序集定义根 **push 进宿主实体的树**(跨工作项改树);`build_block_data_json` 在合并后按宿主实体编码并铸 id | `assembly.rs:864-870/878-886/897`、`neko.rs:692/769`、`kitten.rs:220` | 反向改为三段:**串行** `unrewrite_calls` → **串行** `def_root_from_entry` 合并 → 之后才按"宿主实体(含合并进来的定义)"并行编码;临时 id 全局序列 = 实体遍历 → 程序集遍历的拼接顺序 |
| 2 | **"counter 纯函数"只在确定性模式成立** | `model.rs:342-368`:`uuid()` 确定性分支 343、非确定性走 `fastrand` 346-350;`short()` 364 vs `chars.generate` 368(`shared/model.rs:160-169`) | 明确:字节一致保证**只承诺 `deterministic_ids(true)`**(基准/测试都在该模式);非确定性模式下阶段 B 改为**串行现铸**随机 id 并建 temp→final 映射(只要求合法+唯一) |
| 3 | **正向阶段 C 顺序自相矛盾**:`rewrite_calls` 会把 `ProcedureEntry` 的阶段 A 临时 id 复制进实体树,而方案把替换放在 `rewrite_calls` 之前;且临时 id 被用作 `inputs` 的 **BTreeMap 键**,"遍历值替换"覆盖不到 | `neko.rs:381-467`(`fields.NAME` 441、`mutation def_id/name` 433-436、`arg id` 447-449、`node.inputs.insert(param.id.clone(), input)` 467) | 阶段 B 先把 `ProcedureEntry.id/param.id` 换算成最终 id,让 `rewrite_calls` 直接复制**最终** id;或把替换挪到 `rewrite_calls` 之后并同时覆盖"键 + 值 + mutation/shadow XML 字符串",`rewrite_calls` 自身铸的影子 id(`neko.rs:461`)另排一条临时序列 |
| 4 | **告警顺序无法只靠"阶段 A 按项拼接"复现**:反向 warn 横跨五段(`reverse_node`、`unrewrite_call`、`def_root_from_entry`、`duplicate_ids`、`build_kitten4_document`) | `mapping.rs:1254/1272/1319/1332`、`neko.rs:699-701/725-730`、`assembly.rs:894-895/1084-1087/1125-1128/1160-1163/1322-1325/1372-1375` | 五段各自按串行顺序收集并按序拼接;`warnings()` 是公开访问器,顺序可观察,不能"反正 counts 是聚合" |
| 5 | **验收基座抓不住"只有某个实体 id 错位"**:基准整测 `#[ignore]` 且依赖 gitignored 样本;正向样本 0 程序集;`diff_tests` 只跑单实体且**逐块比较显式跳过 id** | `tests/convert_bench.rs:39-73`;`mod.rs:690+`;`reverse_tests.rs` 单实体 | 新增**非 ignored** 的自造小文档单测(见 §8) |
| 6 | **两级并发超订**:`batch_concurrency × entity_concurrency`,方案只限了作品数 | `convert/mod.rs:173-174`;`TranslateOptions` `mod.rs:80-83` | 在 `translate_works` 里把 `entity_concurrency` 按 `batch_concurrency` 折算,并封顶 `available_parallelism`。评审确认翻译内核**不碰**全局客户端、`LazyLock` 只读、`fastrand` 线程本地 ⇒ **无数据竞争/死锁**,只是超订 |

> 评审另有正面结论:并发本身安全(无共享可变全局被翻译期触碰),失败模式是"产物不一致"而非 UB。

## 8. 执行前置(评审要求补的检查,先写测试再动并行)

1. **正向**:自造 ≥2 实体文档 —— 实体 A 定义 `procedures_2_defnoreturn`(带返回值路径以触发
   `neko.rs:229` 的 `round.id` 铸 id),实体 B 有 `procedures_2_callnoreturn` 调用它;
   断言 `entity_concurrency = 1` 与 `8` 下 `serde_json::to_string` **逐字节相同**、
   `report.warnings()` **逐条同序**。
2. **反向**:自造 KN 文档 —— `procedures.proceduresDict` ≥2 条定义(含形参与调用点)+ ≥2 实体
   (至少一个角色承接宿主实体);断言同上,并额外逐条比对 `RemintedId`/`DroppedProperty`。
3. 两条都必须**不依赖 `download/`**,能在 CI 跑。

> **进度(2026-09-25)**:已落地**一条**合并守门测试
> `reverse_tests::multi_entity_with_procedures_is_deterministic`(自造 KN 文档:2 实体 +
> `proceduresDict` 定义,不依赖样本)。它断言:① 反向两次转换**逐字节一致** + 告警**逐条同序**;
> ② 程序集定义根确实挂到宿主实体(`procedures_2_defnoreturn` 出现在角色积木里);
> ③ 反向产物再走**正向闭环**,正向也逐字节可重复、告警同序,且源里的定义被抽成
> `procedures.proceduresDict`(⇒ 正向 `split_procedures` 路径被真实走到)。
>
> **仍未覆盖**(执行并行前应补):正向的**调用点重写**(`rewrite_calls` 把临时 id 写进
> `fields.NAME`/`mutation`/`inputs` **键** —— 评审阻塞问题 #3 的正向场景)需要构造带
> KN 调用积木(引用某条定义)的输入;目前只有单实体层面的
> `neko::tests::rewrites_call_sites_and_leaves_unknown_calls_untouched` 部分覆盖。

**结论:执行顺序改为** —— ① 先落地 §8 两条测试(今天就能跑,且是并行的守门);② 正向并行(修 #2/#3/#6);
③ 反向按 #1 的三段重设计(§7 表);④ 告警顺序按 #4 逐段建模。**未过 §8 之前不合并并行实现。**

> **进度(2026-09-25,后续提交)**:② **正向并行已落地**,详见 §9(含实现中发现的一个评审未预见的
> 真问题:临时 id 的记账槽位必须按阶段错开)。① 的正向守门测试也一并落地(自造文档,不依赖样本)。
> ③④ 反向仍**未动**。

---

## 9. 落地记录(2026-09-25:S3a **正向**实体级并行)

范围:只做正向 `convert_kitten4_document`。反向(`convert_kn_document` / `build_kitten4_document` /
`unrewrite_calls` / `def_root_from_entry`)**一行未动**,按 §7 表 #1 另起一轮。

### 9.1 改了什么(文件 / 函数)

| 文件 | 内容 |
| ---- | ---- |
| `src/core/convert/translate/model.rs` | `MintKind`(uuid/short)、`TEMP_ID_PREFIX`(哨兵 `\u{1}`)、`is_temp_id_char`;`IdSource::recording(slot)` / `into_log()`(记录模式:产临时 id、记账、不推进全局计数)。**串行模式代码路径逐字未动** |
| `src/core/convert/translate/remint.rs`(**新**) | `run_items`(贪心装箱 + `thread::scope`,按项序返回)、`workers`(夹取请求/项数/可用核数)、`remap_tree` / `remap_entry` / `remap_json` / `remap_text`(单遍改写:值 + BTreeMap 键 + mutation/shadow XML)、`merge_report`(局部报告按项序并入,告警串同表改写) |
| `src/core/convert/translate/mod.rs` | 新增 `TranslateOptions::entity_concurrency(usize)`(默认 1,公开 API 只做加法)与 `fold_entity_concurrency`;`TranslateReport::take_warnings`;`convert_kitten4_document` 改五段(§9.2);`collect_forward_items` / `restore_forward_items`(阶段 0 取走 `block_data_json`,结束后原样放回) |
| `src/core/convert/mod.rs` | `translate_works` 折算两级并发预算(§7 阻塞 #6):`实体级 = min(请求, 可用核数 / 有效作品并发)` |
| `tests/convert_bench.rs` | 每个样本加一行 `实体并发=8` + `1 vs 8 同 SHA256` 门 + 加速比打印(第二职责) |
| `src/core/convert/translate/mod.rs`(测试) | `forward_parallel_tests`:自造 1 场景 + 2 角色(跨实体程序集调用)文档,三方逐字节对照(1 / 8 / 测试内串行参考实现)+ 告警逐条同序 + 哨兵清零 + uuid 计数连续;非确定性模式"合法且唯一 + 铸造次数不变" |

### 9.2 管线(五段)

```
阶段 0(串行)  按官方顺序(scenes → actors,各自按 id 排序)拆工作项,取走 block_data_json
阶段 1(并行)  逐项:parse → mapping → split_procedures   (每项自己的 recording id 源 + 局部报告)
阶段 2(并行)  按项序拼全局 procedures → 逐项 rewrite_calls(槽位 [项数, 2·项数))
阶段 3(串行)  账本拼接:「阶段 1:全项 → 阶段 2:全项」→ 一个 IdSource 兑现最终 id
               → 程序集条目 / 实体树 / 告警串按表改写(值 + 键 + XML)
阶段 4(并行)  逐项 tree_to_json(与旧实现同一编码入口)
装配          build_document(未动)
```

- **铸造序列可复现**:全局第 k 次铸造 = 串行顺序下的第 k 次铸造 ⇒ 确定性模式下 id 逐字节相同;
- **告警顺序**:阶段 1 全项 → 阶段 2 全项,与旧实现两次循环的 push 顺序逐条对应;
- **错误**:`.collect::<Result<Vec<_>,_>>()` 按项序取首个错误(与串行一致);
- **并发 1**:三个阶段都在当前线程按项序跑,但走同一条临时 id 路径 ⇒ 基线 SHA256 直接守默认行为。

### 9.3 实现中发现的真问题(评审未预见)

**临时 id 的记账槽位必须按"阶段"错开。** 第一版里阶段 1 与阶段 2 都用"实体序号"当槽位,
于是同一实体在阶段 1 的第 `seq` 次铸造与阶段 2 的第 `seq` 次铸造撞成**同一个临时 id**
(`\u{1}prov:{槽位}:{seq}:{形态}`),账本 `insert` 时后者覆盖前者 ⇒ 产物 id 静默错位。
**基线门当场抓住**(默认并发 1 也走同一条临时 id 路径,所以这步没被并行掩盖):

- 10.8 MB 正向样本:SHA256 `bafeb50c…` → `f412dd47…`;
- 0.3 MB 正向样本:`d653a8a5…` → `fe02c032…`;
- 两个反向样本**逐字未变**(证明差异确实来自正向改动)。

定位手法(已固化为测试):测试内保留一份**实体级并行前的串行参考实现**,对真作品跑两份产物、
比较 uuid 集合 —— 第一版报 `only new {02d,030,…}` / `only ref {001,004,…}`(同一铸造点被搬到
晚 44/24 个计数处),修完两集合完全相等。

**修法**:阶段 2 用 `[项数, 2·项数)` 的槽位;并在兑现处加 `debug_assert!`(临时 id 撞车即抓),
`IdSource::recording` 的文档里写清"槽位在同一份文档内必须唯一"。

### 9.4 验收证据

见 §9.5 的命令与输出摘录。三条门:

1. `cargo test` 全绿(含新增的 `forward_parallel_tests`、`remint::tests`、`IdSource` 记录模式单测;
   新增测试**一条都没有 ignore**,真作品那条缺样本时自跳过);
2. `taskset -c 2 cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`:
   四个样本 SHA256 **与基线一致**,且每个样本 `实体并发=1` 与 `=8` 的 SHA256 **相同**;
3. 加速比(绑核、5 轮取最小)记录在 §9.6。

**防"空门"**:并发门最容易被"两边都在串行跑"骗过(见 §9.5 的坑)。为此
`TranslateReport::entity_workers`(公开只加一个字段)把实际线程数暴露出来:

- 单测:请求并发 8(3 个工作项)时断言 `entity_workers = min(请求, 项数, 可用核数)` 且
  可用核数 ≥ 2 时必须 > 1;
- 基准:打印每行的实际线程数,且 `available_parallelism ≥ 2` 时正向样本若没开起多线程**直接 panic**;
- 真作品差分门(新 vs 串行参考实现 vs 并发 8)同样带这条守卫。

### 9.5 命令与输出摘录

**绑核口径的坑(踩到了,记下来)**:`taskset -c 2` 把进程钉在 **1 个核**上,而
`std::thread::available_parallelism()` 是**按亲和掩码**算的 ⇒ 返回 1 ⇒ `remint::workers`
夹到 1 ⇒ 并发退化成串行,于是"1 vs 8 同 SHA256"成了**空门**(第一轮就有这个现象:
输出里写着 `可用核数 1`,加速比 0.63×)。所以验收要分两跑:

- **基线守门**(默认并发 1,不需要多核):`taskset -c 2`,噪声最小;
- **并发对照 + 加速比**:**必须钉在多核核集上**(`taskset -c 0-3`),否则是空门。
  基准现在自带守卫:`available_parallelism ≥ 2` 时,正向样本的"实体并发=8"必须真的
  开起多线程,否则直接 panic(`tests/convert_bench.rs` 的"空门守卫");
  `TranslateReport::entity_workers`(公开只加一个字段)把"实际开了几个线程"暴露成
  可观测事实,基准逐行打印、单测逐条断言。

测量口径:先用 `cargo test --profile bench_perf --test convert_bench --no-run` 构建一次
(不绑核),再把同一二进制绑核跑两次 —— 与 `taskset -c N cargo test …` 等价,只是不让
编译也吃单核(单核编译这台机器要 10+ 分钟)。

```text
$ taskset -c 2 target/bench_perf/deps/convert_bench-<hash> --ignored --nocapture
实体级并发对照(方案 25 S3a):可用核数 1,请求并发 8
  10.8 MB 正向:core 281 → 290 ms(0.97×),实际线程 1 → 1,SHA256 bafeb50c0a8e4eeb…(与基线一致)
  0.3 MB 正向:SHA256 d653a8a5…;反向 9.4 MB:SHA256 e7680dcc…;反向 3.7 MB:SHA256 bfde1fc1…
  —— 四个样本逐字与基线一致,且每样本「实体并发 1」与「=8」的 SHA256 相同 ✅
[convert_bench] 产物 SHA256 与基线一致 ✅;实体级并发 1 vs 8 同 SHA256 ✅
(此环境核数=1,并发对照退化为串行 —— 见上面的坑;真正验并行的输出见下一条)

$ taskset -c 0-3 target/bench_perf/deps/convert_bench-<hash> --ignored --nocapture
实体级并发对照(方案 25 S3a):可用核数 4,请求并发 8
  10.8 MB 正向:core 265 → 171 ms(1.55×),e2e 566 → 581 ms(0.97×),实际线程 1 → 4,SHA256 与基线相同 ✅
  0.3 MB 正向:core 7 → 7 ms(1.00×),实际线程 1 → 4,SHA256 与基线相同 ✅
  反向 9.4 MB:core 203 → 213 ms(0.95×),实际线程 1 → 1,SHA256 与基线相同 ✅
  反向 3.7 MB:core 57 → 59 ms(0.97×),实际线程 1 → 1,SHA256 与基线相同 ✅
[convert_bench] 产物 SHA256 与基线一致 ✅;实体级并发 1 vs 8 同 SHA256 ✅
```

### 9.6 实测加速比(绑核、5 轮取最小)与"为什么不是 3×"

**装箱均衡性**(10.8 MB 正向样本,209 个工作项、权重 = 源 `blocks` 条数 = 12 764、
最大项占 4.35%):按 `run_items` 同口径的贪心装箱,4 线程与 8 线程都做到**完全均衡**
(各线程权重 `[3191,3191,3191,3191]` / `[1596,1596,1596,1596,1596,1595,1595,1594]`,
均衡率 = 理想值/最大值 = 1.000),所以速度上限由"并行段的线程缩放效率"决定
(实测见下:本机受 SMT/内存带宽限制),不由负载倾斜决定。

**本机硬件口径(必须先说)**:测试机是 Intel i5-5200U —— **2 物理核 / 4 逻辑核**、
2.2–2.7 GHz ULV 笔记本 CPU,`available_parallelism = 4`。所以"请求并发 8"在本机实际线程数是 4,
方案 §4 的"8 线程下 ≥3×"在这台机器上**无法验证**;能给的是一条实测缩放曲线(同一二进制,
只改 `taskset` 核集):

| 实际线程 | 10.8 MB 正向 `core_ms` | 相对 1 线程 | `e2e_ms` | 产物 SHA256 |
| -------- | --------------------- | ----------- | -------- | ----------- |
| 1(`taskset -c 0`) | 290 | 1.00× | 704 | = 基线 ✅ |
| 2(`taskset -c 0-1`) | 225 | **1.28×** | 668 | = 基线 ✅ |
| 4(`taskset -c 0-3`) | 171 | **1.55×** | 581 | = 基线 ✅ |

- 反向两个样本在 4 线程下 0.95–1.03×、**逐字与基线一致**(它们没走并行路径,符合预期);
- 0.3 MB 正向样本 1.00×(工作太小,线程调度开销吃掉了收益);
- 产物 SHA256 在**所有**绑定 × 并发组合下都等于基线 `bafeb50c0a8e4eeb…`。

**为什么 4 线程只有 1.55×**:临时插桩逐段计时(跑完已还原插桩,`cmp` 校验回滚逐字节一致)。
同一次运行内比较 1 线程与 4 线程(row 1 与 row 2 同热状态,各 6 次取最小;10.8 MB 正向):

| 阶段 | 1 线程 | 4 线程 | 加速 |
| ---- | ------ | ------ | ---- |
| 0 拆工作项(串行) | 0.58 ms | 0.58 ms | 1.0×(设计如此) |
| 1 `parse` + `mapping` + `split_procedures`(并行) | 176.0 ms | **101.7 ms** | **1.73×** |
| 2 `rewrite_calls`(并行) | 1.7 ms | 1.5 ms | —(该样本 0 个程序集) |
| 3 兑现最终 id + 程序集改写(串行) | 2.0 ms | 1.9 ms | 1.0×(设计如此) |
| 4 临时 id 改写 + `tree_to_json`(并行) | 105.3 ms | **67.4 ms** | **1.56×** |
| 装配 + 并报告(串行) | 0.11 ms | 0.09 ms | — |
| 合计(≈ 基准的 `core_ms`) | 283 ms | 171 ms | **1.65×** |

- 串行段(阶段 0/3 + 并报告)**只有 ~2.7 ms**,即内核里 ~99% 的时间在两段并行代码里
  ——不存在"并行段占比太低"的问题;
- 每段在 4 逻辑核(= **2 物理核**)上跑到 1.7×/1.6×,即物理核理想值的 **83%**;
  剩下的是 SMT 共享执行资源 + 内存带宽 + 树在阶段间跨线程搬运的分配开销
  (14 k 节点、每节点多次 `Value`/`Map` 分配);
- 所以"1.55×"是**本机 2 物理核的上限**问题,不是并行设计的问题:瓶颈层与方案 23 的
  P1-6/P1-7(减少分配与遍历)重合,与本轮"产物逐字节不变 + 与并发数无关"无关。

插桩口径:6 个 `Instant::now()` 打在阶段边界上,`eprintln!` 每转换一行;插桩前后
`cmp /tmp/mod.rs.before_profiling` 校验逐字节还原。
