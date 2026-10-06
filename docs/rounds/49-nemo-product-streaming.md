# NEMO 产物侧流式写出(2026-10-06)

范围:`src/core/convert/translate/{mod.rs,nemo.rs,nemo_tests.rs,assembly.rs,model.rs}`。

结论:NEMO → KN 的文件入口改吃**流式产物** —— 三处 `nekoBlockJsonList` 不再建整份 `Value`,而是与正向(Step 2/3)、
反向(Step 4)共用**同一份** `assembly::ProductDocument`,两条路径的差别只剩"块表编码的 `shield` 补键口径"这一个参数。
`convert_bench` 的产物 SHA256 与 `#meta` 全绿(该基线写于流式实现之前)⇒ 端到端字节不变;
同轮 A/B(绑核、每侧两轮、同侧读数无交叠):NEMO 两样本的**分配次数各降 14%**、`core` 各降约 18%。
`../rounds/47` §2.2 六行硬指标里,NEMO 两行由本轮接手(`nemo-old-1.5MB` 三行达标;`nemo-3.4MB` 的 `e2e` 与分配贴线,见 §5)。

## 1. 背景与拍板依据

该项原为 `../goals/convert-backlog.md` §1 的**待决**项,拍板点原文:

> NEMO 的**产物侧上界只有 17~20%**……且它的块表是 NEMO 自己的 `nekoBlockJsonList` 形态、程序集在解析器内就位
> ⇒ 要做就得写**第三套**写出器。拍板点:按"4% 差距 vs 第三套实现的维护面"决定做/不做

本轮的复核结论:**"第三套写出器"这个前提不成立**,依据是三条代码事实:

| 事实 | 证据 |
| --- | --- |
| NEMO 的 KN 产物与正向的 KN 产物**同形** | 三处挂点与键名完全一致:`actors.actorsDict.<id>`、`scenes.scenesDict.<id>`、`procedures.proceduresDict.<id>`,块表键都是 `nekoBlockJsonList` |
| 唯一差别是块表的 `shield` 补键口径 | 改动前:正向 `model::tree_to_json` + `fill_shield`(**恒写**,官方 `jC.parseBlock` 每个节点都写);NEMO `nemo::tree_to_json` 直吃 `BlockJson::to_value`(**假值不写**)。本轮把前者并入 `model::tree_to_value`(见 §2.1,符号漂移已登记 `../knowledge/errata.md`) |
| 该方向不在任何"判不做"清单里 | `../goals/convert-backlog.md` 末尾清单 + `../knowledge/convert-performance.md` §4 逐条核对:它只是没做 |

⇒ 落地方式定为"按口径参数化、复用同一份写出与挂点机制",维护面从"第三套"降为"一个枚举 + 一处分叉"。

## 2. 方案

### 2.1 块表编码口径参数化(`model.rs`)

新增 `ShieldPolicy{Always, OnlyWhenTrue}`;`write_block_tree(tree, shield, w)` 与 `write_block(node, shield, w)` 透传它。
`shield` 键的出现条件由"恒真"改为 `shield == Always || node.shield`;值分支不变(真写 `true`、假写 `false`,而 NEMO 口径下假值根本不出现该键)。

`tree_to_json(tree)` 并入 `tree_to_value(tree, ShieldPolicy)`:`Always` 即旧 `tree_to_json`(物化后 `fill_shield`),
`OnlyWhenTrue` 即 NEMO 旧口径(物化后不补)。两套物化器的**唯一**区别是这一个补键动作。

### 2.2 产物机制复用(`assembly.rs`)

| 改动 | 说明 |
| --- | --- |
| 挂点类型 `BlockHook` → `pub(super) BlockPlacement` | NEMO 装配侧同样登记它(NEMO 的实体循环在 `nemo.rs`,不能再用私有类型) |
| `ProductDocument` 增 `shield: model::ShieldPolicy` | 正向构造点传 `Always`;新增 `ProductDocument::new_nemo(doc, hooks)` 传 `OnlyWhenTrue` |
| 写出链(`write_to`→`write_section`→`write_entries`→`write_entry_with_blocks`)透传 `shield` | 这是"两份写出器"与"一份"的全部差别 |

`into_value()` 同步按口径物化(`tree_to_value(&hook.tree, self.shield)`),因此**公开面口径不变**:`TranslateDocument.document: Value`。

### 2.3 装配共用一份(`nemo.rs`)

`convert_nemo_document` 拆成:装配(`assemble_nemo(source, options, report, placements)`)+ 收尾(`finish_document`)+ 两条薄包装:

| 入口 | `placements` | 块表去向 |
| --- | --- | --- |
| `convert_nemo_document`(内存路径) | `None` | 每处当场 `tree_to_json`(`push_root` 的"单根失败记告警 + 丢该根"语义原样保留) |
| `convert_nemo_document_product`(文件路径,新) | `Some` | 只登记 `BlockPlacement`,树按值搬出,由写出侧在占位处直写 |

分叉只在 `put_block_table` 一处。实体并行段(阶段 3)按 `product` 布尔决定"编码"还是"把树搬出",阶段 4 **串行**登记
—— 登记顺序与并行完成顺序无关(容器是 `BTreeMap`,键序由字典序决定),与 `../rounds/48` 的键序纪律一致。

文件入口(`mod.rs::translate_text`)按 `translate_value` 同一判据分派:`target == KittenN` 且 `detect_editor` 认 NEMO ⇒ 走
`ConvertedDocument::Product`(复用正向的枚举变体)。NEMO **不走源侧骨架**(它的文档里没有 `block_data_json`,判据天然排除)。

### 2.4 会改变产物字节的两条口径(必须守住)

1. **`shield` 必须是 `OnlyWhenTrue`** —— 若误用 `Always`,产物会给每个 NEMO 节点多写 `"shield": false`(官方不写);
2. **数字归一的位置** —— 旧口径在整份产物 `Value` 上跑 `normalize_integral_numbers`(官方 `JSON.stringify` 把"整数值的浮点"打印成整数,
   `stageSize`/`timerPosition`/变量坐标/实体坐标因此都是整数形态)。流式路径**没有**那份 `Value`,故拆成两半:
   非块表部分照旧整份归一;块表部分在 **typed 树**上就地归一(`normalize_tree_numbers`),覆盖 `BlockJson` 的四个 `Value` 字段
   (`location`/`fields`/`field_constraints`/`extra`)并递归 `next`/`inputs`/`statements` —— 其余字段是字符串或布尔,不含数字。

## 3. 落地(改动面)

| 文件 | 改动 |
| --- | --- |
| `translate/model.rs` | `ShieldPolicy`;`write_block_tree`/`write_block` 加口径参数;`tree_to_json` 并入 `tree_to_value`(旧的 `Always` 语义成为其中一支) |
| `translate/assembly.rs` | `BlockHook`→`pub(super) BlockPlacement`(字段放开);`ProductDocument` 加 `shield` 字段与 `new_nemo`;写出链透传 |
| `translate/nemo.rs` | `assemble_nemo` + 两条包装;`put_block_table`/`finish_document`/`normalize_tree_numbers`;`NemoEncoded` 带出待登记的树 |
| `translate/mod.rs` | 文件入口的 NEMO 分支(复用 `ConvertedDocument::Product`);模块表与走线注释 |
| `translate/nemo_tests.rs` | 新增真样本三口径字节等价门 + 树上归一覆盖面门 |
| `translate/mod.rs`(`file_path_tests`) | 新增文件入口走线门 |
| `translate/model.rs`(tests) | `streamed_block_tree_equals_value_path` 扩成两口径;`..._handles_empty_and_minimal` 补 NEMO 口径断言 |

## 4. 读数(2026-10-06;同轮 A/B,`taskset -c 0-3`,可用核数 4,每侧两轮取最小)

A = 本轮改动前(`git stash` 回到 `../rounds/48` 的落地状态),B = 本轮改动后;分配窗口 = 一轮 `translate_file`(串行腿)。

| 样本 | `core` A → B | `e2e` A → B | 分配次数 A → B | 分配 MiB A → B | 产物 SHA256 |
| --- | --- | --- | --- | --- | --- |
| nemo-3.4MB | 255–260 → **207–209**(−18.8%) | 352–357 → **307–310**(−12.8%) | 1 541 425 → **1 322 584**(**−14.2%**) | 178.3 → 155.3 | 与基线一致 |
| nemo-old-1.5MB | 77–84 → **63–65**(−18.2%) | 107–115 → **99–101**(−7.5%) | 472 850 → **405 054**(**−14.3%**) | 59.3 → 52.4 | 与基线一致 |

**对照(本轮未改动的方向,用来把"改动只落在 NEMO"钉死)** —— 分配计数逐位相同:

| 样本(未改动) | 分配次数 A → B | `core`/`e2e` A → B |
| --- | --- | --- |
| kitten4-10.8MB(正向) | 909 987 → 909 987 | 178/300 → 198/313 |
| kitten4-0.3MB(正向) | 28 864 → 28 864 | 6/10 → 7/10 |
| kn-9.4MB(反向) | 602 799 → 602 799 | 65/160 → 101/243 |
| kn-3.7MB(反向) | 141 085 → 141 085 | 23/62 → 25/66 |

### 读法与结论

1. **分配是硬信号**:该装置里分配计数是逐位可复现的量 —— 未改动方向两侧差值恒为 0,且 A 侧 `nemo-3.4MB` 的 1 541 425
   与 `../rounds/48` §4 的记录值**逐位相同**。故"−14.2% / −14.3%"是结论级证据。
2. **毫秒只作同向佐证**:同轮里未改动的 `kn-9.4MB` 的 `core` 两侧差 55%(65 → 101)、`kitten4-10.8MB` 差 11%(178 → 198),
   而 NEMO 两侧各自内部一致(A 侧 255/260、B 侧 207/209,无交叠)⇒ 时间收益方向明确,但绝对值不可当收益量。
   注意漂移方向:未改动方向在 B 轮**变慢**,若把这条漂移一并计入,NEMO 的净收益只会更大。
3. `ser` 列(NEMO 12 / 5 ms)不变 ✓ —— 它量的是内存路径的 `Value → JSON`,本步不动内存路径。
4. 产物 SHA256 与 `#meta` 全绿,且**基线写于流式实现之前** ⇒ 这是"文件入口端到端字节不变"的直接证据(不是自证)。

## 5. 与 `../rounds/47` §2.2 硬指标的对账

| 样本 | 指标 | 目标 | 本轮后 | 结论 |
| --- | --- | --- | --- | --- |
| nemo-3.4MB | `core` | ≤ 230 ms | **207–209** | 达标 |
| nemo-3.4MB | `e2e` | ≤ 300 ms | 307–310 | 贴线(差 2.3%~3.3%;该列同轮漂移带见 §4 第 2 条) |
| nemo-3.4MB | 分配次数 | ≤ 1 300 000 | **1 322 584** | 差 1.7%(本轮前 1 541 425,差 18.6%) |
| nemo-old-1.5MB | `core` | ≤ 75 ms | **63–65** | 达标 |
| nemo-old-1.5MB | `e2e` | ≤ 100 ms | 99–101 | 贴线 |
| nemo-old-1.5MB | 分配次数 | ≤ 420 000 | **405 054** | 达标 |

⇒ 六行目标里 **`nemo-old-1.5MB` 三行全部达标**;`nemo-3.4MB` 剩 `e2e`(差 2.3%~3.3%)与分配(差 1.7%)两处贴线,
两者都已从"18.6% 的缺口"缩到个位数百分比。剩余带宽的来源不在产物侧(见 §7)。

## 6. 守门(新增三条 + 既有门)

| 门 | 内容 | 位置 |
| --- | --- | --- |
| 三口径字节等价(真样本) | 内存 `Value` / 产物物化 / 流式字节**逐字节相同**,且块数与告警条数逐条相同。样本为 847 演员 + 38 场景 + 4 程序集 ⇒ 三处挂点全覆盖(不依赖网络:`download/` 本地件) | `nemo_tests::nemo_product_path_is_byte_identical_to_value_path` |
| 树上归一的覆盖面 | 四个 `Value` 字段 + 三处子槽都放"整数值的浮点",逐字段比"树上归一"与"整树归一";并断言样例**真的**含整数值浮点(防空门) | `nemo_tests::tree_normalization_covers_every_value_field` |
| 文件入口**走线** | 钉住 `translate_text` 对 NEMO 源返回 `ConvertedDocument::Product`(字节一致证不了走线 —— 回落路径的字节也对),再比字节。与 `nemo_tests::nemo_entity_parallelism_...`(钉"真的开了线程")同一路数 | `file_path_tests::nemo_source_uses_streaming_product_and_matches_memory_path` |
| 块表口径的字节等价 | 两条 `ShieldPolicy` **各自**与它的 `to_value` 路径逐字节等价;NEMO 口径下假 `shield` 不写键、真 `shield` 照写 | `model::tests::streamed_block_tree_equals_value_path`、`..._handles_empty_and_minimal` |
| 端到端产物门 | `convert_bench`:产物 SHA256 + `#meta`(源 SHA/字节/块数/告警数)与基线一致 | `tests/convert_bench.rs` |

自查过的口径风险(逐条给结论):

- **`shield` 误用 `Always`**:由上面第 4 条与第 5 条门当场抓(产物会多出 `"shield": false`);
- **漏归一某个字段**:由第 2 条门抓(覆盖不到就与整树归一不等);
- **占位没被填回**(`null` 留在产物里):由第 1、3 条门抓(字节不等);
- **块表登记顺序与内存路径不一致**:`Map` 是 `BTreeMap`,键序由字典序决定,登记顺序不影响字节(第 1 条门覆盖);
- **`elapsed_ms` 口径漂移**:两条包装各自在函数入口起算、`assemble_nemo` 不再自设,`core` 列因此可比(§4 的 A/B 是同一口径)。

**评审状态**:本轮**未做**独立评审(改动面小:一处口径参数化 + 一处装配分叉,风险点已在上表逐条自查);
若后续要按 `../rounds/47` §9 的方式收紧,评审应重点看 §2.4 两条会改产物字节的口径与 §7 的行为差异。

## 7. 已知的行为差异与未做

- **编码失败路径(收尾诚实项)**:内存路径保持旧口径(单根编码失败 ⇒ `TranslateReport` 记告警 + 丢该根,`push_root` 与其单测不变);
  文件路径在**写出时**才编码,失败即 `ConvertError` 中止 —— 不产出"少了块还写盘"的产物。真实数据构造不出该失败
  (见 `push_root` 的文档与 `encode_failure_enters_report_instead_of_dropped` 的说明),故只影响手工构造的输入;
  取舍:流式写出没有"先试编码"的位置,而**中止**比"静默少块"更保守。
- **未做(与本轮正交,仍开着)**:反向(KN → Kitten4)的**源侧骨架**(把 `nekoBlockJsonList` 留成 `RawValue` 直喂 typed 反序列化,
  与 Step 5 同族;基座读数:反向 `kn-9.4MB` 的 `parse` 列 34 ms / `e2e` 243 ms);
  `entity_concurrency` 是否对大作品自动开(`../goals/convert-backlog.md` §1,仍待决)。
- **不在本轮范围**:`BlockJson` 的形状不动 ⇒ `../rounds/37` §10.6 的 `#[serde(flatten)]` 手写(P6)仍在"判不做"。

## 依据

- 代码:`src/core/convert/translate/{model.rs,assembly.rs,nemo.rs,mod.rs}`(符号:`model::ShieldPolicy`、`model::tree_to_value`、
  `assembly::ProductDocument::{new_nemo,write_to,into_value}`、`nemo::{convert_nemo_document_product,assemble_nemo,put_block_table,normalize_tree_numbers}`、
  `mod::ConvertedDocument::Product`);测试:`nemo_tests::{nemo_product_path_is_byte_identical_to_value_path,tree_normalization_covers_every_value_field}`、
  `mod::file_path_tests::nemo_source_uses_streaming_product_and_matches_memory_path`。
- 读数:`tests/convert_bench.rs`(同轮 A/B 四次调用:改动前两轮 `git stash`、改动后两轮;分配计数与 `convert_bench_baseline.json` 的 `#meta` 同口径)。
- 方案与被推翻的前提:`../rounds/47`(§2.2 硬指标、§3.2 块表编码口径表、§4 Step 2/3 与 Step 4 的逐条结论)、
  `../rounds/48`(NEMO 分配基线 +13.9%)、`../goals/convert-backlog.md` §1(原待决行)。
- 落地提交:`9cf1616`(代码 + 三条守门);本篇记录与三个库的同步在其后单独提交。
