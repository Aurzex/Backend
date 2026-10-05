# 第四十七轮记录(方案 v2) — 数据表示层重写:减少中间 `serde_json::Value` 的物化

> v1 经独立评审(2026-10-05)后修订:5 处 P1、4 处 P2、1 处 P3 全部处置,逐条见 §9。

## 0. 一句话

在"产物逐字节不变"这条硬门之下,把转换域里**同一份数据被物化成 `serde_json::Value` 的次数**降下来:先清掉 `json!` 造成的整份深拷贝(Step 1,三方向都有),再把写出改成从强类型直接流式写(Step 2/3),然后反向与 NEMO 对齐(Step 4);源侧解析绕过 `Value` 作为**候选**(Step 5),需先过 spike。

## 1. 背景与读数(2026-10-05 剖析,`../knowledge/convert-performance.md` §2bis)

进程级 CPU 归属(self 占比):**libc 分配/释放 36.8%**、`Value` 树反序列化 9.2%、`Value`/`BTreeMap` 析构 9.6%、`BTreeMap<String, Value>` 插入遍历 8.4%、序列化与转义 8.4%,而**本库转换逻辑合计只有 8.5% 且无单点超过 1%**。分配量:产物节点摊到 **114–215 次分配/节点**,分配字节是源文件的 **17–59 倍**。

单点切片(rounds/37 P6、rounds/39 W9)实测收益均为 **0**,不是因为方向错,而是因为它们只削掉这条链上的一小块。本轮改为按"**同一份数据被物化的次数**"来削:正向现状是"源 `Value` → 强类型 → 产物 `Value`(+ 装配期整段深拷)",同一块积木至少被物化两次、装配期再深拷一次。

## 2. 目标与硬指标(可观测、可复跑)

### 2.1 口径(与装置一致)

- **时间**:`cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`,`taskset -c 0-3`。该基准 `RUNS = 5`,**时间列取 5 轮最小值**;判断"哪一步更快"用 `git worktree` 双树**同轮交替**跑(仓库纪律:`../knowledge/repo-conventions.md` §3ter)。
- **产物门**:`tests/fixtures/translate/convert_bench_baseline.json` 的**产物 SHA256 逐字节相同**,且 `#meta` 中除 `alloc_*` 四个键外全部相同(那四个键在 `META_RECORD_ONLY` 名单里,**不参与断言**,只作读数);并发 1 与并发 8 的产物 SHA 必须相同。
- **分配读数**:`#meta` 的 `alloc_count_serial` / `alloc_bytes_serial`(窗口 = 一次 `translate_file`);现状取**已提交基线文件**的读数,验收取当轮打印值。
- **参照点**:§2.2 的表是**相对基线**的累计目标;每步的**当步期望**写在该步里(相对上一步)。

### 2.2 累计目标(按方向归属到步骤)

| 样本 | 方向/归属 | `core` 现状 → 目标 | `e2e` 现状 → 目标 | 分配次数现状 → 目标 |
| --- | --- | --- | --- | --- |
| kitten4-10.8MB | 正向 · Step 1–3 | 285–308 → ≤ 200 ms | 696–720 → ≤ 520 ms | 2 125 796 → ≤ 1 200 000 |
| kitten4-0.3MB | 正向 · Step 1–3 | 6–7 → ≤ 5 ms | 18–20 → ≤ 15 ms | 68 665 → ≤ 50 000 |
| kn-9.4MB | 反向 · Step 4 | 191–199 → ≤ 150 ms | 330–341 → ≤ 270 ms | 995 789 → ≤ 750 000 |
| kn-3.7MB | 反向 · Step 4 | 57–60 → ≤ 45 ms | 101–107 → ≤ 85 ms | 264 400 → ≤ 210 000 |
| nemo-3.4MB | NEMO · Step 4 | 281–287 → ≤ 230 ms | 360–371 → ≤ 300 ms | 1 626 347 → ≤ 1 300 000 |
| nemo-old-1.5MB | NEMO · Step 4 | 88–92 → ≤ 75 ms | 115–124 → ≤ 100 ms | 505 321 → ≤ 420 000 |

现状取自已提交基线(正向两行与本轮同机读数一致;反向两行用基线值,与 §2bis.2 的同机读数差 0.3–0.5%,属构建差异)。

**Step 5 已落地后的实测(2026-10-05)**:`kitten4-10.8MB` 分配 1 538 778 → 1 329 054、`e2e` 595/580 → 546/545 ms;`kitten4-0.3MB` 分配 48 436 → 42 277。

**Step 2/3 已落地后的实测(2026-10-05,`61e2c33`)**:`kitten4-10.8MB` 分配 → **909 987**、`e2e` → **421 ms**、`core` → 257 ms;`kitten4-0.3MB` 分配 → 28 864、`e2e` → 9 ms ⇒ **正向两行达标**。剩余未达标的是反向两行与 NEMO 两行的 `e2e`/分配(Step 4 的目标,但 Step 4 的"对齐同一套写出"机制本身要先按 Step 2/3 的结论重估:那四个方向的 `e2e` 未归类占比只有 4–16%,大头在 `core`,见 §2bis.1 与 §4 Step 4)。

## 3. 现状数据流(符号级)

### 3.1 正向(Kitten4 → KittenN)

`translate_file` → `serde_json::from_str::<Value>`(整份文档一次物化)→ `translate_value`(按值持有)→ `pipeline::convert_kitten4_document(&mut Value)` → `collect_forward_items`(取走每实体 `block_data_json`;**另把整份实体元数据 `entity.clone()` 进 `ForwardItem.source`**)→ `parse_forward_item` →(`normalize_object_shadows` 仅对象影子作品才 `bdj.clone()`)→ `model::parse_block_data_json(&Value)` → `model::parse_parts`(**`connections` 整份 `cloned()`**)→ `build_node`/`BlockJson::from_value`(借用反序列化)→ `mapping::translate_kitten_to_kn`(原地改写强类型树)→ `split_procedures`/`rewrite_calls` → 串行 `IdRemap` 铸造与 `remap_tree` → `run_items`(阶段 4)→ `model::tree_to_json`(逐节点 `BlockJson::to_value` = **整棵树重新物化**)→ `fill_shield` → `assembly::build_document`(**整份产物 `Value` 的构造点**)→ `restore_forward_items` → `FileService::write_json`。

**装配期的整份深拷贝**(`json!(expr)` 展开为 `serde_json::to_value(&expr)`,已核 serde_json 1.0.151 的 `macros.rs`):

| 位置 | 被再物化一次的东西 |
| --- | --- |
| `assembly::build_document`: `json!({ "scenesDict": scenes, … })` | 全部场景 |
| 同上: `json!({ "actorsDict": actors })` | **全部角色 + 每个角色的 `nekoBlockJsonList`(即整份产物积木)** |
| 同上: `json!({ "proceduresDict": … })` | 全部程序集(含其积木树) |
| 同上: `styles` / `variables` / `audios` / 单键包装若干 | 各段 |
| `model::procedures_to_json`: `json!({ … "nekoBlockJsonList": tree_to_json(..)? })` | 每个程序集的整棵积木树 |

### 3.2 反向(KittenN → Kitten4)/ NEMO → KittenN

- 反向:KN 的 `nekoBlockJsonList` → `model::parse_kn_entity`/`BlockJson::from_value` 借用反序列化 → `mapping::translate_kn_to_kitten` 反演 → `model::unrewrite_calls`(依赖**全局 `call_targets`**)→ `def_root_from_entry`(定义根挂进宿主实体)→ `model::build_block_data_json`(**每根 `root.clone()` + 每节点 `to_value()`**)→ `assembly::build_kitten4_document`(**整份产物 `Value`**;含 `json!` 单键包装)→ `write_json`。
- NEMO:`xml::parse_fragment` → `XmlNode` DOM(迁移 + 若干 transform)→ `nemo_mapping::translate_nemo_to_kn`(边解析边映射成强类型)→ `nemo::tree_to_json` → **逐段装配**(`json!({ "actorsDict": Value::Object(actors) })`、`proceduresDict`、`stylesDict`、`variablesDict`、`scenesDict`、`audiosDict` 各处都是整段再物化一次)→ `normalize_integral_numbers`(**必须在装配之后**)→ 整份产物 `Value` → `write_json`。

### 3.3 三端编码策略互不相同(Step 3/4 必须按方向参数化)

| 端 | 编码入口 | 补键策略 |
| --- | --- | --- |
| 正向 | `model::tree_to_json` + `fill_shield` | 只递归补 `shield: false`(仅 `inputs`/`statements`/`next`,不递归 `extra`) |
| 反向 | `model::encode_block` + `KITTEN4_DEFAULTS` | 补 **12 个**默认键(含 `is_shadow`/`is_output`/`disabled`),并强制 `parent_id`/`mutation`/`field_constraints` |
| NEMO | `nemo::tree_to_json` | **不调 `fill_shield`** |

## 4. 分步计划

### Step 1 — 清掉 `json!` 造成的整份深拷贝(三方向同一类问题,字节等价论证最强)

改动(全部是"把已构造好的 `Value`/`Map` 直接放进 `Map`,不经过 `json!`"):

- `assembly::build_document`:`scenes`/`actors`/`procedures`/`styles`/`variables`/`audios` 的单键包装改成 `Map::insert`;
- `model::procedures_to_json`:同上(`entry.params` 的小 `json!` 可保留或一并改);
- `nemo::convert_nemo_document`:`scenesDict`/`actorsDict`/`proceduresDict`/`stylesDict`/`variablesDict`/`audiosDict`/`broadcastsDict` 的单键包装同上;
- `model::parse_parts`:`connections.and_then(Value::as_object).cloned()` 改借用(`Option<&Map>` + 空表兜底),`build_node` 已收 `&Map`。

**不做**:`pipeline::collect_forward_items` 的 `entity.clone()` —— 装配阶段(`actor_entry`/`scene_entry`)对**按值**拿到的实体做有损改写(`remove("x"/"y"/"user_change_r_c")`、`insert("nekoBlockJsonList"/"currentStyleId"/"name")`),而 `forward_parallel_tests` 在成功与失败两路都断言"源文档逐字节不变"⇒ 这份克隆是结构必需。该结论与 `../goals/convert-backlog.md` §5 第 5 行既有裁决一致,本轮**不重开**。

字节等价论证:`json!(expr)` ≡ `to_value(&expr)`;对已是 `Value` 的表达式,`to_value` 是**恒等深拷贝**;`serde_json::Map` 是 BTreeMap(编译特性无 `preserve_order`),对象键按**字典序**输出 ⇒ 同一次序、同一序列化器 ⇒ 字节相同。**仍需 SHA 门证实**,不靠论证放行。

当步期望:分配次数 −20%~−30%(正向;削掉产物几何级的一次再物化),`core` −8%~−15%;反向/NEMO 同量级。

**落地读数(2026-10-05,提交见本轮记录;新构建 3 轮取最小 vs 改前同机 3 轮取最小)**:产物 SHA 与 `#meta` 全部与基线一致(六样本、两条腿)。

| 样本 | `core` 前 → 后 | `e2e` 前 → 后 | 分配次数 前 → 后 |
| --- | --- | --- | --- |
| kitten4-10.8MB | 292 → 277(−5.1%) | 697 → 545(−21.8%) | 2 125 796 → 1 538 778(−27.6%) |
| kitten4-0.3MB | 6 → 6 | 18 → 14(−22.2%) | 68 665 → 48 436(−29.5%) |
| kn-9.4MB | 189 → 125(−33.9%) | 330 → 265(−19.7%) | 995 789 → 798 341(−19.8%) |
| kn-3.7MB | 57 → 35(−38.6%) | 101 → 76(−24.8%) | 264 400 → 196 002(−25.9%) |
| nemo-3.4MB | 281 → 202(−28.1%) | 360 → 282(−21.7%) | 1 626 347 → 1 353 316(−16.8%) |
| nemo-old-1.5MB | 88 → 59(−33.0%) | 115 → 91(−20.9%) | 505 321 → 415 798(−17.7%) |

**读数只一条:**正向的 `core` 只降 5%(它取 `report.elapsed_ms`,而深拷贝发生在 `assembly` **之外**),但同一样本的 `e2e` 降 22% —— 把 §2bis.5 第 5 条那条 [INFERENCE] 坐实了:**`e2e` 未归类那一段(原先 227 ms / 32%)正是产物物化与深拷贝**,现降至约 71 ms(−69%)。**§2.2 里反向两行与 NEMO 两行的目标已达成**(kn-9.4 125 ≤ 150、kn-3.7 35 ≤ 45、nemo-3.4 202 ≤ 230、nemo-old 59 ≤ 75);正向两行仍待 Step 2/3。

### Step 2+3(2026-10-05 已落地,`61e2c33`)— 流式写出:从"造整棵树再序列化"改成"直接写"

**落地内容**(按下方实测上界与实现路径定):

1. `model::write_block_tree` / `write_block`:块树 → JSON 文本的**字节等价**写出。键序 = `serde_json::Map`(= `BTreeMap`)的字节序;`shield` **恒写**(旧路径靠 `fill_shield` 补,这里无条件写 `node.shield`);`extra` 与已知键撞名时 **`extra` 胜**(与 `to_value` 的"已知字段先写、flatten 后写覆盖"同口径);键名、字符串、`Value` 一律交给 `serde_json::to_writer` 转义。**不建任何中间 `Value`**。
2. `assembly::ProductDocument`:文档里三处 `nekoBlockJsonList`(角色 / 场景 / 程序集定义体)只留 `null` **占位**(位置由 `Map` 的字典序决定),真正的树挂 `BlockHook`。`into_value()` 把树物化回 `Value` 填占位(公开面 `TranslateDocument.document: Value` 口径不变),`write_to()` 在占位处**直接流写**块树。
3. `ConvertedEntity.blocks: BlockTree`(不再是 `Vec<Value>`);阶段 4 只改写树、不再编码;`model::procedure_entries` 把"程序集条目表"与"定义体树"拆开(树按值搬出,占位只作位置锚点)。
4. `translate_file` 走 `ConvertedDocument::Product` → 新增的 `FileService::write_json_with`(同一套 `BufWriter` 口径,序列化交给调用方);内存路径(`translate_value`)与所有回落路径仍是整份 `Value`。
5. 常驻等价门 3 条(块写出含 `extra` 撞名/`shield` 真假/三槽嵌套/转义与非 ASCII;空树与最小块;角色+场景+程序集**三处挂点**的整份文档),见 `model::streamed_writer_tests` 与 `assembly_tests::streamed_product_matches_value_product`。

**验证**(装置同 §2.1;A = `d937cda`):

| 样本 / 指标 | A(改动前) | B(本步) |
| --- | --- | --- |
| kitten4-10.8MB `e2e` | 541 ms | **421 ms(−22%)** |
| kitten4-10.8MB `core` | 373 ms | **257 ms(−31%)** |
| kitten4-10.8MB 分配(次数 / 字节) | 1 329 054 / 231.2 MiB | **909 987 / 184.6 MiB(−31.5% / −20%)** |
| kitten4-0.3MB `e2e` / 分配 | 15 ms / 42 277 | **9 ms / 28 864(−31.7%)** |
| kitten4-10.8MB 并发 8(`e2e` / `core`) | 417 / 260 ms | **316 / 156 ms** |
| kn-9.4 / kn-3.7 / nemo / nemo-old 分配 | — | **逐位不变**(本步只改正向) |

- 六个字节基线样本 × 串行/并发 8 两条腿:产物 **SHA256 与 `#meta` 全绿**;并发 1 与 8 同 SHA;
- `cargo test --lib` 138 项全绿(含两条语料往返扫描器 —— 它们走 `translate_file`,即这条新路径);`cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 全绿。
- **§2.2 正向两行由此达标**:10.8MB 分配 909 987 ≤ 1 200 000、`e2e` 421 ≤ 520;0.3MB 分配 28 864 ≤ 50 000(`core` 一列因 Step 5 的口径变化已不可比)。

> **2026-10-05 已试并回退**:把 `tree_to_json` 由"`BlockJson::to_value` 物化 + `fill_shield` 再 DFS 一趟"改成"引用式一趟写出"(`NodeRef`,按字典序写、就地补 `shield`),等价测试通过、产物 SHA 全绿,但**同轮 A/B(`git worktree` 检出 Step 1 状态、两棵树的 bench 交替跑)显示:时间中性(kitten4-10.8 `core` 332 vs 343、kn-9.4 141 vs 142)、分配次数 +1.5%**(1 538 778 → 1 562 371)。
> 根因:新实现**每节点多一个 `Vec<(&str, 值)>`**(+1 次分配/节点),而 serde 那点被省掉的机械开销本就可忽略 —— 与 `rounds/37` P6(手写 `to_value`/`from_value`、逐字节等价但**零收益**)是**同一结论**:**瓶颈不在"怎么序列化",而在"有没有先把整棵树造成 `Value`"**。
> 因此该写法**回退,不再重试**;Step 2/3 直接合并为下面这一步(评审 P1-2 也建议合并)。

改动:新增可序列化的产物视图 + 文档级流式写出 —— `assembly` 边装配边写进 `BufWriter`,实体对象里的 `nekoBlockJsonList` 由 `BlockTree` **直接写出**(不经 `Vec<Value>`),`ConvertedEntity` 因此持 `BlockTree` 而非 `Vec<Value>`。

> **产物侧上界实测(2026-10-05,一次性探针,跑完即删)** —— 为什么必须做**文档级**,而不是再试一次"引用式写出":
>
> | 项 | ms | 分配/轮 |
> | --- | --- | --- |
> | `translate_file`(`e2e`) | 568 | |
> | `report.elapsed_ms`(`core`) | 397 | |
> | 读源 | 6 | |
> | 源侧骨架(Step 5 实测区间) | 19~30 | |
> | **产物序列化到 sink**(`to_writer`,Vec 预分配) | **36.3** | **1** |
> | 产物解析(仅作参照) | 89.4 | |
>
> `e2e − core` = 171 ms;扣掉读源 6、源侧骨架 ~25、序列化 36 ⇒ **约 104 ms 落在"产物 `Value` 构造 + 落盘 + 析构"**。也就是说:**序列化本身只有 36 ms(预分配 sink 下仅 1 次分配),大头是那份产物 `Value` 本身的构造与析构** —— 与已回退的那版(只换写出方式、每节点多一个 `Vec`,量到零收益)正好互证:**收益只能来自"根本不做这份产物 `Value`",不能来自"换一种写法"**。
> **实现路径(2026-10-05,按上表定)**:必须在写出路径上**根本不创建每块的 `Value`**,而不是"换一种构造/序列化方式"。已回退的那版之所以零收益,是因为 `tree_to_json` 的返回类型就是 `Value` —— 它只能在"造出这份 `Value`"的前提下做等价改写,块 `Value` 照造不误(还多了每节点一个 `Vec`)。因此本步的落地形态是:
> 1. `model::BlockJson`/`BlockTree` 增**字节等价的文本写出**(键序 = `serde_json::Map` 字典序;`extra` 与已知键**合流后排序**;`shield` 落在其字典序位置;字符串转义交给 `serde_json::to_writer` 写键/值,不手写转义);
> 2. 文档信封用一个 `write_document`:逐键 `serde_json::to_writer` 写键与值,并在 `theatre.{actors,scenes}.<id>.nekoBlockJsonList` 处挂钩子 —— 块的 `Value` 一个都不建,直接流写;
> 3. 公开面 `TranslateDocument.document: Value` **不动**(内存路径继续走 `to_value()`);
> 4. 永久等价测试:同一批树"流式写出"与"`to_value()` + `to_writer`"**逐字节相同**(覆盖 `extra`、`shield`、嵌套 `inputs`/`statements`/`next`、非 ASCII 与转义)。

必须遵守(评审 P1-2/P1-3):
- 公开面 `TranslateDocument.document: Value` **不动**:内存型调用方(含 `tests/convert_facade_bench.rs`)仍走 `.to_value()`,代价与今天相同;流式只服务"文件→文件"链路(`translate_file`、`translate_work_in`)。是否把该字段换成流式产物类型**另立决策**。
- 键序按 **`serde_json::Map` 的字典序**(不是字段序、不是装配序);`extra` 的键要与已知键**合流后按字典序**输出,`shield` 落在它的字典序位置上;`NodeRef` 那种"每节点一个 `Vec`"的做法不再用(见上)。
- 三端默认键策略不同(§3.3),写出必须按方向参数化 —— 本步只做正向,反向/NEMO 见 Step 4。

交付物:①`BlockTree` 的 writer(无中间 `Value`);②`ProductDocument`(持各段 + 实体 `BlockTree`)的 `write_to` 与 `to_value`;③永久等价测试:同一批树"流式写出"与"`to_value` + `to_writer`"**逐字节相同**(覆盖 `extra`、`shield`、嵌套 `inputs`/`statements`/`next`、非 ASCII/转义)。

当步期望:`e2e` −10% 以上、分配次数 −10%~−20%(省掉整棵产物积木 `Value` 的构造、序列化遍历与析构);`core` 同向下降(正向两行目标是 `core` ≤ 200 ms)。

> **顺序修订(2026-10-05,按读数)**:§2.2 的约束项是**正向 `core`**(277 → ≤200),而 Step 2/3 主要打在 `assembly`/`ser` 一侧(对 `e2e` 大、对 `core` 小)。正向 `core` 里当前最大的单块是**源侧**:`parse`(整份文档 → `Value`)约 104 ms + `parse_block_data_json` 把 `Value` 再翻成强类型树 —— 即 `block_data_json` 这份**最大的 `Value` 子树**被完整物化了一次,随后又被翻译成 `BlockTree`、最后随文档一起析构。
> 因此把 **Step 5(源侧)提前到 Step 2/3 之前**先做 spike:若 spike 成立(`RawValue` 直喂强类型 + 旧形态回落),先落 Step 5,再落 Step 2/3;若 spike 不成立(旧形态回落比例高、或字节/行为出现偏差),回到 Step 2/3 并按 §2.2 重新界定正向 `core` 目标(记录原因,不静默降级)。

### Step 4 — 反向与 NEMO 对齐同一套写出(必做,承载 §2.2 的三行目标)

改动:反向 `model::build_block_data_json`(相邻表 `Value`)与 `assembly::build_kitten4_document`、NEMO `nemo::tree_to_json` 与 `convert_nemo_document` 的逐段装配,改用与 Step 2/3 同族的流式写出。

> **机制需先重估(2026-10-05,按 Step 2/3 的读数与归因)**:Step 4 的原机制是"反向 / NEMO 对齐同一套写出",但 §2bis.1 显示那四个方向的 `e2e` **未归类只占 4–16%**(NEMO 5%、反向 16%),而 Step 2/3 在正向拿到的 −22% `e2e` 正是**未归类那段(32%)**;反向 / NEMO 的大头在 `core`(kn-9.4 137 ms、nemo-3.4 217 ms,而它们的 `e2e` 分别是 289 / 313 ms)⇒ 对它们"只换写出方式"的预期收益小。**落地前先按方向量一次产物侧上界**(做法同 Step 2/3 的探针),再决定是"只对齐写出"还是"改查 `core` 的热点"——后者的候选见 `../rounds/37` §11.1 #3(NEMO 同一段 XML 包 `<root>` 解析 3 次 + `has_return_blocks` 再解析一次)。
>
> **按方向的上界实测(2026-10-05,一次性探针,跑完即删)**:口径与 Step 2/3 的探针相同(产物侧 = `e2e − core − 读源 − 源解析 − 产物序列化到 sink`):
>
> | 样本 | `e2e` | `core` | 读源 | 源解析 | 产物序列化到 sink | 产物 | **产物侧(推)** | 占比 |
> | --- | --- | --- | --- | --- | --- | --- | --- | --- |
> | kn-9.4(反向) | 313 | 151 | 4 | 39 | 21.7(41 011 次分配) | 9.3 MiB | **97 ms** | 31% |
> | kn-3.7(反向) | 89 | 44 | 3 | 7 | 5.2(13 203 次) | 3.9 MiB | **31 ms** | 35% |
> | nemo-3.4 | 326 | 233 | 2 | 11 | 24.7(58 816 次) | 7.4 MiB | **55 ms** | 17% |
> | nemo-old-1.5 | 104 | 71 | 1 | 5 | 6.2(18 483 次) | 2.3 MiB | **21 ms** | 20% |
>
> ⇒ **反向方向的产物侧占比与正向(Step 2/3 前的 30%)同量级**,所以"对齐同一套写出"在**反向先做**是有依据的(上界约 −20% `e2e`);NEMO 只有 17~20%,预期收益约为其一半。**注意**反向的产物块表是**邻接表形态**(`build_block_data_json` 出 `{blocks, connections}`),不能直接复用 `model::write_block_tree`(那是数组形态),要另写一份字节等价的邻接表写出 + 对应的常驻等价门。


风险(评审补充):三端**默认键策略不同**(见 §3.3)⇒ 写出必须**按方向参数化**,不能强行统一;反向 `mark_unknown_blocks` 需要先有整份连接表;NEMO 的 `normalize_integral_numbers` 必须仍在装配之后。反向还有**形状 id 现铸**(见 §5 第 6 条)。

当步期望:按 §2.2 的三行目标。

### Step 5 — 源侧 `block_data_json` 不再先落成 `Value`(2026-10-05 已落地,`d9652f2`)

> **Spike 记录(2026-10-05,一次性探针,跑完即删)**:骨架读法(把 `block_data_json` 记为 `IgnoredAny`,只扫描不建节点)对"整份 → `Value`"的读数(10.3 MiB 样本):110.8 ms / 555 517 次 / 61.3 MiB → 24.7 ms / 29 302 次 / 4.6 MiB(0.3 MiB 样本:2.7 ms / 16 148 次 → 0.6 ms / 697 次);覆盖性核过:两样本各 209 / 4 个实体**全部**落在 `theatre.{scenes,actors}` 下。**这是上界,不是实际收益** —— 实际收益 −13.6%,差额归因见 `../knowledge/convert-performance.md` §2bis.8。由此定下的四条落地约束(骨架不用 `flatten` 收其余字段、`RawValue` 只做解析入口且**绝不透传进产物**(源语料非紧凑 JSON)、旧形态回落、公开面 `translate_value` 不动)均已落在实现里。

**为什么不能"从 `Value` 里取 `RawValue`"**:serde_json 在 `Value` 上反序列化 `Box<RawValue>` 时走 `OwnedRawDeserializer { raw_value: Some(self.to_string()) }`,会把整棵子树**重新序列化成 String** —— 此时那份待省的 `Value` 早已构造完毕,净收益为零。

**落地内容**(与初版设计的两处偏离都标在括注里):

1. `serde_json` 开 `raw_value`(`Cargo.toml`);
2. 新增 `src/core/convert/translate/source.rs`:对"文档 / `theatre` / 实体"三层各写一个**流式 `Visitor`** 读源文本,`theatre.{scenes,actors}.*.block_data_json` 留成 `Box<RawValue>`(旁表,键 = (容器, id)),其余仍是 `Value`;形状不合即 `Err`,调用方回落整份 `Value` 解析。**不用 `#[serde(flatten)]` 收"其余字段"** —— 那会把整份文档重新缓冲成 `Content`(见 `../knowledge/convert-performance.md` §2bis.8 结论 3,读数 29 302 次 vs 454 次);
3. `translate_file` 改走 `translate_text`:先 `text.contains("\"block_data_json\"")` 设闸,骨架成立、`detect_editor`(以旁表里"演员是否带该键"为准)判为 Kitten4、且目标为 KN 时走快速通道;其余一切(骨架不成立 / Kitten2·3 / NEMO·Neko / 反向目标)回落成整份 `Value` + `translate_value`,与旧口径逐字一致;
4. `pipeline` 新增 `enum BlockData { Raw(Box<RawValue>), Value(Value) }`(**偏离初版设计**:初版写的是 `ForwardItem.block_data_json: Option<Box<RawValue>>`,但公开面 `translate_value(Value)` 没有原文,保留一个 `Value` 变体才不用在内存路径上反向序列化);`collect_forward_items` 增旁表形参,`restore_forward_items` 只放回 `Value` 变体;装箱权重在原文路径改用**原文字节数**(权重只影响并行均衡,不进产物);
5. `model::parse_block_data_json_typed` / `parse_parts_typed` / `build_node_typed`:与 `Value` 版**逐句对齐**(祖先环检查、缺 id 兜底、连接类型与错误文案);`TypedBdj` 反序列化失败(字符串化 `blocks`、内联对象影子)即回落成"物化 `Value` 走老路径"。

**验证**(装置:`--profile bench_perf`,`git worktree` 双树同轮交替;A = `dcb23f4`):

| 样本 / 指标 | A(改动前) | B(本步) |
| --- | --- | --- |
| kitten4-10.8MB `e2e` | 595 / 580 ms | **546 / 545 ms** |
| kitten4-10.8MB 分配(串行腿) | 1 538 778 | **1 329 054(−13.6%)** |
| kitten4-0.3MB `e2e` / 分配 | 17 / 18 ms;48 436 | 12 / 15 ms;**42 277** |
| nemo-3.4MB 分配 | 1 353 316 | 1 353 318(持平) |
| kn-9.4MB 分配 | 798 341 | 798 343(持平) |

- **产物 SHA256:六个样本 × 串行/并发 8 两条腿全绿**;`#meta` 除 `alloc_*` 四个记录键外全等;并发 1 与 8 同 SHA;
- `cargo test`:lib 135 项全过(其中 `k4_corpus_round_trip_sweep` 走的就是这条新路径)+ 集成测试全过;
- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` 全绿。

**两个必须记住的坑**:

1. **`core` 的口径漂了**:`report.elapsed_ms` 自管线入口起算,而原文路径把"bdj 解析"从 `translate_file` 里那次 `from_str`(core 之外)挪进了管线内 ⇒ 本步 `core` 在 10.8MB 上 302 → 379 ms **是口径变化,不是变慢**;判收益看 `e2e`(−40 ms)。这条只影响读数解释,不影响产物。
2. **快速通道必须按格式设闸**:无条件试骨架时,KN/NEMO 文本会白付一次全量骨架解析(实测 nemo-3.4MB `e2e` +26 ms、分配 +50 638 次 / +3.7%)。已改为先做定长键的字节扫描。

**归因(为什么只省 13.6%,而 spike 的上界是 36%)**:见 `../knowledge/convert-performance.md` §2bis.8 —— 强类型流式解析(596 110 次分配)比先建 `Value`(532 116 次)还贵,本步真正省掉的是 `Value → BlockJson` 的 `from_value` 那一趟;根因是 `BlockJson` 的 `#[serde(flatten)] extra` 让派生实现把整块积木先缓冲成 `Content` 再逐字段转换。

### Step 5b(不立项)— 去掉 `BlockJson` 的 `flatten`,手写 `Deserialize`

本次探针给出了这一处的**单点**读数:`bdj` 强类型解析由 `flatten` 版的 596 110 次 / 155.5 ms 降到手写版(未知键直接进 `Map`)531 907 次 / 136.1 ms(**−12% 分配 / −12% 时间 / −35% 分配字节**,同一份 9.1 MiB `bdj`;见 `../knowledge/convert-performance.md` §2bis.8)。

**但这条已经试过并被回退**:`../rounds/37` §10.6(P6,2026-09-26)去掉 `extra` 的 `#[serde(flatten)]`、手写 `from_value`/`to_value`(约 150 行),产物逐字节不变、`cargo test` 全绿,而**同轮 A/B 三轮交替量不到收益** ⇒ 回退。原因也写在那一节:40.7% 的 libc 大头是**整份 `Value` 树本身的构造与析构**,手写解析仍要 clone 同样多的 `String`/`Value`,省下的只是"未匹配键的缓冲机制"这一小块。

两者对得上:单点的 −12% 换算到整条流水线只有约 **−5% 分配**,低于 A/B 的可测门 —— 与 P6 的端到端结论一致。**故本步不立项(维持 `../rounds/37` §10.6 的判定)**;若将来要吃这一块,必须与 §11.1 #2 那条"少建中间 `Value` 树"的大改**同批**做,并自带同轮 A/B,单独做只会重演 P6。

**未覆盖**:`translate_work`(域门面)这条内存直通路的源文档来自平台的 `http_client.get_json`(`decompile/editors.rs` 里 Coco 走 HTTP 拿 "compiled"),不在本步范围 —— 要吃到同一份收益,得让那条 GET 也按骨架读(记进 `../goals/convert-backlog.md`,待方案)。

## 5. 不变量与风险(重构不得打破)

1. **键序 = BTreeMap 字典序**(不是结构体字段序、不是装配顺序):`serde_json::Map` 是 BTreeMap(编译特性无 `preserve_order`)⇒ 不得换 `Map` 实现、不得改写出顺序、不得开启 `preserve_order`。
2. **id 铸造是纯函数**:正向 `model::IdSource` 的 counter 表示"第几次铸造"(阶段 1 用 `[0, n)`、阶段 2 用 `[n, 2n)`);`TEMP_ID_PREFIX` 哨兵与 `remap_*` 必须覆盖所有承载 id 的字段(fields/shadows 的键与 XML 串内),`debug_assert_eq!(unmatched, 0)` 是兜底。任何遍历顺序变化或增删一个铸造点都会整体偏移后续 id。
3. **唯一接触源文档**:只有 `collect_forward_items`/`restore_forward_items` 改源文档,且必须在成功与失败两路都把 `block_data_json` 还原(已有 `source == pristine` 断言);`normalize_object_shadows` 只在副本上改。
4. **`fill_shield` 的不对称语义**:正向只补 `shield: false`,不补 `is_shadow`/`is_output`/`disabled`;只递归 `inputs`/`statements`/`next`。NEMO **不补**;反向经 `KITTEN4_DEFAULTS` 补 12 个默认键。
5. **`extra` flatten 保真**:未识别键原样往返,合流排序后输出。
6. **反向的 id 语义**:节点/实体 id **沿用源 id**,但**形状 id 由 `IdSource::short` 现铸**,铸造点有三处 —— `model::encode_block`(缺 id / id 重复)、`model::unrewrite_call`(每个非 `Label` 形参的 `ARG<j>` 影子)、`model::def_root_from_entry`(每个形参的 `math_number` 影子);其顺序由实体遍历序、`unrewrite_calls`/`def_root_from_entry` 递归序与 `build_block_data_json` 的根序共同决定,同样落在 SHA 门上。
7. **名字唯一化依赖遍历序**:`assembly::build_document` 的角色名 / 场景 `screenName` 用共享的 `actor_used`/`scene_used` + `uniquify`(命中加后缀),结果与遍历顺序绑定。
8. **程序集与实体同表改写**:阶段 2 把程序集 id/形参 id 复制进实体树,`remap_entry` 与 `remap_tree` 必须成对覆盖。
9. **反向独有**:全局 `call_targets` 的生命周期、`def_root_from_entry` 把定义根挂进宿主实体。
10. **NEMO 的整数归一位置**:`normalize_integral_numbers` 必须在装配之后;单根失败按现有策略降级为告警(不整份失败)。

## 6. 验收矩阵(每步都必须过,且必须有量化入口)

| 改动类型 | 产物 SHA/`#meta` | 两台语料扫描器 | `forward_parallel_tests` | 同轮 A/B(量化入口) | 真机门 |
| --- | --- | --- | --- | --- | --- |
| 去 `json!` 深拷贝(Step 1) | 必须 | 必须 | 必须 | 必须(`git worktree` 交替 3 轮,记 `core`/`e2e`/分配) | 不需要 |
| 流式写出(Step 2) | 必须 | 必须 | 必须 | 必须 | 建议 |
| 节点直写(Step 3) | 必须 | 必须 | 必须 | 必须 | 建议 |
| 反向/NEMO 对齐(Step 4) | 必须 | 必须 | 必须 | 必须 | 建议 |
| 源侧骨架解析(Step 5) | 必须 + **逐字段 spike 对照** | 必须 | 必须 | 必须 | 建议 |

统一口径:`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`、`BACKEND_REQUIRE_BENCH=1 … --profile bench_perf --test convert_bench -- --ignored`(严格模式)全绿;每步单独提交并写明当步读数;**某步若 `core`/`e2e`/分配三项都没有可测变化,视为该步无效,记录原因并重排后续步**。

## 7. 不做清单(本轮不碰)

- 不换 `Map` 实现、不引入 `preserve_order`、不改 BTreeMap 字典序。
- 不改 id 铸造策略与阶段划分(不合并/拆分 `IdSource` 的铸造点)。
- 不做语义降级、不改 `TranslateWarning` 判定。
- 不改反向 `call_targets`/`def_root` 语义、不动 `mark_unknown_blocks`/`duplicate_ids` 判定。
- 不重新引入已判不做的项:`RawValue` **透传**、合并两份 `tree_to_json`、`BTreeMap`→`HashMap`、`find_object_shadow` 惰性化、W13。
- 不改公开面 `TranslateDocument.document`(见 Step 2 的处置)。
- NEMO 的 XML→DOM 层优化(rounds/37 P9)与反向实体级并行(判不做)不在本轮。

## 8. 与既有结论的对账(评审修订)

- `../rounds/40` §5(R6)的结论是**判定不立项**,给出两条重开条件:1) 出现硬性墙钟/内存指标;2) 有人认领**完整流式重写**并自带门(产物字节不变 + 分配门下降 + profiler 前后)。本轮**同时满足两条**:§2.2 给出硬指标与量化入口、§4 给出完整重写路径与门 ⇒ 属**按 R6 的重开条件重开**,不是"等有了指标就做"。
- 本方案重开的"已判不做"项与理由:`P6 手写去 #[serde(flatten)]`、`W9`、`RawValue` **透传**(注意:Step 5 用的是"RawValue 做解析入口",不是透传) —— 其零收益证据之所以不再适用,是因为它们各自只削一条链上的一小块(单点),而本轮削的是**整份数据的物化次数**(§3.1 的装配期深拷贝一次就是整份产物);这一点由 Step 1 的独立读数直接证伪或证实(每步都有量化入口)。
- `../goals/convert-backlog.md` §5 第 5 行的既有裁决(`collect_forward_items` 的 `entity.clone()` 不立项)本轮**尊重不重开**,已写进 Step 1 的"不做"。

## 9. 评审与修订(2026-10-05,独立评审)

评审结论:**有条件接受**;5 处 P1、4 处 P2、1 处 P3。处置如下(逐条):

| # | 级别 | 评审意见 | 处置 |
| --- | --- | --- | --- |
| 1 | P1 | Step 1 ⑤(实体深拷)不可行且会破坏"不改源文档" | **已删**(改为 Step 1 的"不做"并引用 `convert-backlog` §5 既有裁决) |
| 2 | P1 | Step 2 与 pub 字段 `TranslateDocument.document` 冲突,且与 Step 3 互相依赖 | **已修订**:Step 2 明确"不改公开面,只服务文件→文件链路";与 Step 3 的依赖在 §4 写明(Step 3 依赖 Step 2 的 writer) |
| 3 | P1 | Step 3 把键序写成"结构体字段序",实际是 BTreeMap 字典序 | **已修正**:Step 3 与 §5 第 1 条改为"字典序",并写明 `extra` 合流与 `shield` 的落位 |
| 4 | P1 | §2 目标表 3 行无归属步骤(反向/NEMO 只在"可选"Step 5);NEMO 同类深拷贝漏进 Step 1;§8 对 P9 的归属写错 | **已修订**:§2.2 增加"方向/归属"列;Step 1 纳入 NEMO 与反向的 `json!` 清理;NEMO/反向写出升为必做的 Step 4;删掉对 P9 的错误归属 |
| 5 | P1 | Step 4(源侧)按"从 `Value` 取 `RawValue`"省不掉 `Value` | **已修正**:写明 `OwnedRawDeserializer{raw_value: Some(self.to_string())}` 这条机制,Step 5 改为"文档级骨架解析 + spike 前置",且不计入目标 |
| 6 | P2 | §5 第 6 条"反向不铸造"与代码不符(三处 `ids.short()`) | **已修正**:§5 第 6 条按三处铸造点重写 |
| 7 | P2 | 验收口径与装置不符(`RUNS = 5`;`alloc_*` 在 `META_RECORD_ONLY` 不参与断言;现状分配数与基线不同) | **已修正**:§2.1 重写口径;§2.2 现状改用基线读数并标注差异来源 |
| 8 | P2 | 每步的门无法证伪"这一步没有收益";分步期望的单位未定义 | **已修正**:§6 增加"同轮 A/B"列与"三项均无可测变化即视为无效"的判定;§2.1 明确参照点(累计 vs 当步) |
| 9 | P2 | §5 漏三条:三端默认键策略不同、名字唯一化依赖遍历序、NEMO 单根失败降级 | **已补**:§3.3 新增对照表;§5 第 4/7/10 条补入 |
| 10 | P3 | §8 未与 R6 的重开条件及"已判不做"清单对账 | **已补**:§8 重写为对账段(声明满足两条重开条件 + 逐项列重开理由) |

评审未采纳项:无。评审提出的"更省力替代路":无(其结论是本方案方向成立,问题在表述与分步)。

**评审后新增的一条实测(2026-10-05)**:按"Step 3 = 引用式一趟写出"做过一版并**同轮 A/B 后回退** —— 时间中性、分配 +1.5%(根因:每节点多一个 `Vec`,而 serde 少走的那点机械开销可忽略)。这条与 `../rounds/37` P6 互证:**改动必须落在"少造 `Value` 树"上,不能落在"换一种序列化写法"上**;分步计划据此把 Step 2/3 合并(见 §4)。

## 依据

- 读数与命令:`../knowledge/convert-performance.md` §2bis(平坦热点、分配计数、分配器 A/B)与 §2bis.6(`opt-level` 档位实测)。
- 数据流地图:2026-10-05 的两份只读侦察(正向一份、反向 + NEMO 一份),符号级链路与深拷贝清单见 §3;§3.1 的 `json!` 展开语义已核对 serde_json 1.0.151 的 `macros.rs`(`($other:expr) => to_value(&$other)`)。
- 评审:2026-10-05 的独立子代理评审(逐条处置见 §9)。
- 既有方案与裁决:`../rounds/37`(P1–P11)、`../rounds/39`(W1–W13)、`../rounds/40` §5(R6 不立项与两条重开条件)、`../goals/convert-backlog.md` §5(已判不做清单与 `entity.clone()` 裁决)。
- 门与纪律:`../knowledge/repo-conventions.md` §3ter、`tests/convert_bench.rs`(SHA + `#meta` + 分配读数)。
