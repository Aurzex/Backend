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
