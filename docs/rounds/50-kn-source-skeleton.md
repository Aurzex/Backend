# 反向(KN → Kitten4)源侧骨架(2026-10-06)

范围:`src/core/convert/translate/{source.rs,model.rs,pipeline.rs,mod.rs,reverse_tests.rs}`。

结论:反向的文件入口也吃**源侧骨架** —— 三处 `nekoBlockJsonList`(源文档里最大的字段)留成原文、
直喂强类型反序列化,不再为整份 KN 文档建 `Value`。骨架解析按**文档形状**参数化,Kitten4(正向,
`rounds/47` Step 5)与 KittenN(反向,本轮)共用同一套三层 `Visitor`。
读数(同轮 A/B,绑核):`kn-9.4MB` 分配 **602 799 → 442 291(−26.6%)**、`kn-3.7MB` 141 085 → **108 224(−23.3%)**;
产物 SHA256 与 `#meta` 全绿(基线写于本步之前);正向两样本的分配读数逐位不变 ⇒ 骨架重构没有动正向。

## 1. 背景

- 本步是 `../rounds/49` §7 登记的两条"与本轮正交、仍开着"里的第一条;`../rounds/47` 的 §2.2 六行硬指标
  (正向 / 反向 / NEMO 各两行)与"源侧骨架"是同一族里的最后一块:正向(Step 5)与反向(Step 4)的**产物侧**
  都已流式写出,但**源侧**只有正向吃过骨架。
- 基座读数(A 侧,本轮实测):`kn-9.4MB` 的 `parse` 列 38 ms(即整份 9.4 MB 文档建 `Value` 的那一趟)、
  `e2e` 269 ms、分配 602 799;`kn-3.7MB` 同列 6 ms、分配 141 085。
- 正向 Step 5 的同法实测:分配 −13.6%(10.8 MB 样本)。反向的块表更大(占源文档比例更高)⇒ 预期不低。

## 2. 方案

### 2.1 骨架解析按"文档形状"参数化(`source.rs`)

原实现把路径硬编码在三个 `Visitor` 里(`theatre` → `scenes`/`actors` → `block_data_json`)。两个方向的路径其实是
**同一种形状**(文档 → 段 → 字典 → 实体 → 叶子键,四层同深),只是键名不同 ⇒ 把键名提成一张挂点表:

| | 段 | 字典 | 叶子键 | 旁表标签 |
| --- | --- | --- | --- | --- |
| `Kitten4Shape` | `theatre` | `scenes` / `actors` | `block_data_json` | `scenes` / `actors` |
| `KnShape` | `actors` / `scenes` / `procedures` | `actorsDict` / `scenesDict` / `proceduresDict` | `nekoBlockJsonList` | `actors` / `scenes` / `procedures` |

- 形状是**编译期**参数(`trait SourceShape { const TARGETS; const LEAF }` + `PhantomData`),
  因此三层 `Visitor` 仍是固定类型、不需要 `DeserializeSeed` 那套运行时样板;
- 判据只查"字典名"的依据:段名与字典名在**同一形状内唯一**(Kitten4 的两个字典都在 `theatre` 下、
  KittenN 的三个字典名互不相同)⇒ 第二层不必知道"自己在哪个段里"(见该模块文档);
- 旁表里的容器名在**构造期**就跟着字典带出来(`RawDict { label, entities }`),回收时不反查、也不留兜底默认值;
- `parse(text)`(Kitten4)与 `parse_kn(text)`(KN)是同一条 `parse_with::<M>` 的两次实例化;
  `Skeleton` 类型与**Kitten4 的旁表键集**(`scenes`/`actors`, 实体 id)逐字不变 ⇒ 正向调用点零改动。

### 2.2 强类型入口(`model.rs`)

`parse_kn_entity_typed(raw)`:数组直接按 `BlockJson` 反序列化;字符串形态(少数链路把该字段存成 JSON 字符串)
在 `visit_str` 里解析内层数组;`null`/缺失 = 空树;`type == ""` 的垃圾节点照旧过滤 —— 与 `parse_kn_entity`
**逐项等价**(守门:`kn_typed_tests::typed_matches_value_entry_for_all_shapes` 对四种形态逐一对拍)。

`parse_kn_procedures_with(dict, tree_of)`:定义体的树来源由调用方给(内存路径读条目自己的键,骨架路径按 id 取原文);
`parse_kn_procedures` 成为它的薄包装。回调收的是**字典键**(骨架旁表的键就是它;`entry.id` 可能是另一个值)。

### 2.3 管线与接线(`pipeline.rs` / `mod.rs`)

- `convert_kn_document_impl(source, raw_blocks, options, report)` 一条实现,两条入口:
  `convert_kn_document_product`(内存路径,`None`)与 `convert_kn_document_raw_product`(文件路径,`Some`);
- `kn_tree_from_raw`:原文优先;**形态不合即在该实体粒度回落** `Value`(把这一份原文物化成 `Value` 再走老路径)——
  与正向 `parse_forward_item` 同一套约定;**缺键 = 空树**(源里本来就没有该键);
- 文件入口(`translate_text`):反向也先试骨架,判据三段式 = 定长键 `"nekoBlockJsonList"` 的字节扫描
  + 骨架解析成立 + `detect_editor` 认 KN。第三段复用**现成**的 `detect_editor` 即可 —— 它的 KN 判据只看
  `actors.actorsDict` 在不在,与块表无关 ⇒ 摘掉块表不影响识别(这一点是本步落地前先核过的事实)。

## 3. 落地(改动面)

| 文件 | 改动 |
| --- | --- |
| `translate/source.rs` | `SourceShape` / `Target` / `Kitten4Shape` / `KnShape`;三层 `Visitor` 改为按表驱动;新增 `parse_kn`;`RawDict` 带出旁表标签;新增 KN 等价门与拒绝门 |
| `translate/model.rs` | 新增 `parse_kn_entity_typed`;`parse_kn_procedures` 拆出 `parse_kn_procedures_with`;新增 `kn_typed_tests` |
| `translate/pipeline.rs` | `convert_kn_document_impl` 收 `raw_blocks`;新增 `convert_kn_document_raw_product` 与 `kn_tree_from_raw` |
| `translate/mod.rs` | 文件入口的反向骨架分支;`translate_text` 的文档改写 |
| `translate/reverse_tests.rs` | 新增真样本三口径字节等价门 |
| `translate/mod.rs`(`file_path_tests`) | 新增反向走线门(前置条件显式断言) |

## 4. 读数(2026-10-06;同轮 A/B,`taskset -c 0-3`,可用核数 4)

A = 本步改动前(`git stash` 回到 `9cf1616` 的落地状态),B = 改动后;B 侧两轮都跑了,分配读数两轮逐位相同:

| 样本 | 分配次数 A → B | 分配 MiB A → B | `e2e` A → B | `core` A → B | 产物 SHA256 |
| --- | --- | --- | --- | --- | --- |
| kn-9.4MB | 602 799 → **442 291**(**−26.6%**) | 141.0 → 124.9 | 269 → 221 / 134 | 109 → 113 / 70 | 与基线一致 |
| kn-3.7MB | 141 085 → **108 224**(**−23.3%**) | 43.8 → 41.2 | 71 → 62 / 36 | 27 → 30 / 17 | 与基线一致 |
| kitten4-10.8MB(未改动对照) | 909 987 → 909 986(−1) | 184.6 → 184.6 | 435 → 316 / 453 | 270 → 212 / 294 | 与基线一致 |
| nemo-3.4MB(未改动对照) | 1 322 584 → 1 322 584(逐位相同) | 155.3 → 155.3 | 348 → 336 / 319 | 237 → 232 / 189 | 与基线一致 |

### 读法与结论

1. **分配是精确信号**:它在本装置里逐位可复现(未改动的 NEMO 两行两轮逐位相同;B 侧两轮同为 442 291 / 108 224)
   ⇒ "−26.6% / −23.3%"是结论级证据。省下的是**整份源文档的 `Value` 中间树**(9.4 MB 文档 ≈ 4 600 块 + 三张字典),
   与 `../rounds/47` Step 5 的机制同一套(那边只省 −13.6%,因为 Kitten4 的块表在源文档里占比更小)。
2. **毫秒列本轮不可用**:同一轮里未改动的 `kitten4-10.8MB` 的 `e2e` 三次读数 435 / 316 / 453(±40%),
   而 NEMO 侧几乎不动 ⇒ 本会话后半段机器进入过快的状态,B 的两轮 `e2e`(221 / 134)无法与 A(269)在同一漂移带里比。
   与 `../knowledge/convert-performance.md` §5 的漂移纪律一致:**只给分配结论**,毫秒列仅记方向
   (两次 B 都低于 A,但这不足以支撑百分比)。
3. **`core` 列涨了是口径漂移,不是变慢**:`report.elapsed_ms` 自管线入口起算,而骨架路径把"实体块表的 JSON 解析"
   从 `translate_file` 里那次 `from_str`(`core` 之外)**挪进了**管线内 ⇒ `core` 9.4 MB 上 109 → 113/70、
   3.7 MB 上 27 → 30/17。这条与 `../rounds/47` §4 Step 5 复盘里记的同名现象一字不差(判收益看 `e2e`/分配)。
4. **`parse` 列不变是预期的**:它量的是基准自己那次 `serde_json::from_str::<Value>`,与被测入口无关。
5. **正向逐位不变(重构的回归门)**:`kitten4-10.8MB` 分配只差 **−1**、`kitten4-0.3MB` 同样 −1,
   成因可解释:新实现在回收时**搬移**段名 `String`(旧实现写 `"theatre".to_string()` ⇒ 多一次分配)。
   产物 SHA256 与 `#meta` 全绿 ⇒ 正向行为未变;这是 `source.rs` 从"硬编码一个形状"改成"表驱动两个形状"的直接证据。

## 5. 守门(新增四条 + 既有门)

| 门 | 内容 | 位置 |
| --- | --- | --- |
| KN 骨架等价 | 骨架读出的文档 == 整份 `Value` 解析再摘掉三处 `nekoBlockJsonList`(逐字段);旁表键 = (段名, id),三段都命中;段/字典之外的键仍走 `Value` | `source::tests::kn_skeleton_doc_matches_plain_value_minus_block_tables` |
| 骨架拒绝 | 非对象 / 段非对象 / 字典非对象 / 实体非对象 / 尾部多余内容一律失败(调用方据此回落) | `source::tests::skeleton_rejects_non_object_and_trailing_garbage`(两个形状都覆盖) |
| 强类型入口等价 | 数组 / JSON 字符串 / `null` / 空数组四种形态上,`parse_kn_entity_typed` 与 `parse_kn_entity` 产出同一棵树;`type == ""` 过滤;形状不合失败 | `model::kn_typed_tests`(三条) |
| 三口径字节等价(真样本) | 内存 `Value` / 骨架路径流式字节 / 骨架路径物化 —— 三者在真作品上逐字节相同,源块数与告警条数逐条相同 | `reverse_tests::kn_source_skeleton_is_byte_identical_to_value_path` |
| 走线 | 证明三段落判断据对这份文本成立(键扫描命中 + 骨架成立 + `detect_editor` 认 KN),再比字节 | `file_path_tests::kn_source_uses_skeleton_fast_path` |
| 端到端产物门 | `convert_bench`:产物 SHA256 + `#meta` 与基线一致(该基线写于本步之前) | `tests/convert_bench.rs` |

自查过的口径风险(逐条给结论):

- **骨架摘掉的键被别处读到**:装配侧只读实体的标量/小数组(名字、坐标、造型、`actorIds`),从不读块表
  (`rounds/37` P2 起就是既有约定,`pipeline` 的克隆循环也跳过它)⇒ 摘掉不影响;
- **判据失效**:`detect_editor` 认 KN 只看 `actors.actorsDict` 在不在 ⇒ 摘表不影响(已在走线门里显式断言);
- **回落粒度**:形态不合只回落该实体(把这一份原文物化),不回落整份文档;缺键 = 空树(与内存路径同解);
- **旁表键的一致性**:字典名 ⇒ 标签的映射在构造期固化(`RawDict.label`),回收期不做反查、没有兜底默认值;
- **正向回归**:分配 −1 且可解释、SHA/`#meta` 全绿(见 §4 第 5 条)。

**评审状态**:本轮**未做**独立评审(改动面:一个模块的表驱动化 + 三处接线,风险点已在上表逐条自查)。

## 6. 未做与遗留

- `kn_tree_from_raw` 每次查旁表现构两个 `String`(847 实体 ≈ 1.7k 次小分配):相对本步省下的整份源 `Value` 可忽略,
  本轮不做;若要做,需把旁表键换成单一 `String` 或改成游标消费(`../rounds/47` §2.2 的"先量化再排期"纪律);
- `../rounds/49` §7 的第二条(`entity_concurrency` 是否对大作品自动开)仍**待决**;
- 反向的**实体级并行**仍不做(`../rounds/25` §10:Amdahl 上限 1.9×)。

## 依据

- 代码:`src/core/convert/translate/{source.rs,model.rs,pipeline.rs,mod.rs}`(符号:`source::SourceShape`/`parse_kn`、
  `model::parse_kn_entity_typed`/`parse_kn_procedures_with`、`pipeline::convert_kn_document_raw_product`/`kn_tree_from_raw`);
  测试:`source::tests`、`model::kn_typed_tests`、`reverse_tests::kn_source_skeleton_is_byte_identical_to_value_path`、
  `mod::file_path_tests::kn_source_uses_skeleton_fast_path`。
- 读数:`tests/convert_bench.rs`(同轮 A/B:改动前一轮、改动后两轮,均 `taskset -c 0-3`;分配计数与
  `convert_bench_baseline.json` 的 `#meta` 同口径)。
- 同族前例:`../rounds/47`(Step 5 正向源侧骨架的 spike、设计与"`core` 口径漂移"复盘)、`../rounds/49`(§7 登记本步未做)。
- 落地提交:`845b2c2`(代码 + 六条守门);本篇记录与三个库的同步在其后单独提交。
