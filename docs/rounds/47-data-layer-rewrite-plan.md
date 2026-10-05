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

### Step 2+3(已按评审建议合并)— 流式写出:从"造整棵树再序列化"改成"直接写"

> **2026-10-05 已试并回退**:把 `tree_to_json` 由"`BlockJson::to_value` 物化 + `fill_shield` 再 DFS 一趟"改成"引用式一趟写出"(`NodeRef`,按字典序写、就地补 `shield`),等价测试通过、产物 SHA 全绿,但**同轮 A/B(`git worktree` 检出 Step 1 状态、两棵树的 bench 交替跑)显示:时间中性(kitten4-10.8 `core` 332 vs 343、kn-9.4 141 vs 142)、分配次数 +1.5%**(1 538 778 → 1 562 371)。
> 根因:新实现**每节点多一个 `Vec<(&str, 值)>`**(+1 次分配/节点),而 serde 那点被省掉的机械开销本就可忽略 —— 与 `rounds/37` P6(手写 `to_value`/`from_value`、逐字节等价但**零收益**)是**同一结论**:**瓶颈不在"怎么序列化",而在"有没有先把整棵树造成 `Value`"**。
> 因此该写法**回退,不再重试**;Step 2/3 直接合并为下面这一步(评审 P1-2 也建议合并)。

改动:新增可序列化的产物视图 + 文档级流式写出 —— `assembly` 边装配边写进 `BufWriter`,实体对象里的 `nekoBlockJsonList` 由 `BlockTree` **直接写出**(不经 `Vec<Value>`),`ConvertedEntity` 因此持 `BlockTree` 而非 `Vec<Value>`。

必须遵守(评审 P1-2/P1-3):
- 公开面 `TranslateDocument.document: Value` **不动**:内存型调用方(含 `tests/convert_facade_bench.rs`)仍走 `.to_value()`,代价与今天相同;流式只服务"文件→文件"链路(`translate_file`、`translate_work_in`)。是否把该字段换成流式产物类型**另立决策**。
- 键序按 **`serde_json::Map` 的字典序**(不是字段序、不是装配序);`extra` 的键要与已知键**合流后按字典序**输出,`shield` 落在它的字典序位置上;`NodeRef` 那种"每节点一个 `Vec`"的做法不再用(见上)。
- 三端默认键策略不同(§3.3),写出必须按方向参数化 —— 本步只做正向,反向/NEMO 见 Step 4。

交付物:①`BlockTree` 的 writer(无中间 `Value`);②`ProductDocument`(持各段 + 实体 `BlockTree`)的 `write_to` 与 `to_value`;③永久等价测试:同一批树"流式写出"与"`to_value` + `to_writer`"**逐字节相同**(覆盖 `extra`、`shield`、嵌套 `inputs`/`statements`/`next`、非 ASCII/转义)。

当步期望:`e2e` −10% 以上、分配次数 −10%~−20%(省掉整棵产物积木 `Value` 的构造、序列化遍历与析构);`core` 同向下降(正向两行目标是 `core` ≤ 200 ms)。

### Step 4 — 反向与 NEMO 对齐同一套写出(必做,承载 §2.2 的三行目标)

改动:反向 `model::build_block_data_json`(相邻表 `Value`)与 `assembly::build_kitten4_document`、NEMO `nemo::tree_to_json` 与 `convert_nemo_document` 的逐段装配,改用与 Step 2/3 同族的流式写出。

风险(评审补充):三端**默认键策略不同**(见 §3.3)⇒ 写出必须**按方向参数化**,不能强行统一;反向 `mark_unknown_blocks` 需要先有整份连接表;NEMO 的 `normalize_integral_numbers` 必须仍在装配之后。反向还有**形状 id 现铸**(见 §5 第 6 条)。

当步期望:按 §2.2 的三行目标。

### Step 5(候选,需先过 spike)— 源侧 `block_data_json` 不再先落成 `Value`

**为什么不能"从 `Value` 里取 `RawValue`"**:serde_json 在 `Value` 上反序列化 `Box<RawValue>` 时走 `OwnedRawDeserializer { raw_value: Some(self.to_string()) }`,会把整棵子树**重新序列化成 String** —— 此时那份待省的 `Value` 早已构造完毕,净收益为零。

要真省掉,必须让源文件直接 `serde_json::from_str::<骨架类型>`(文档级 `Value` 不再存在,`block_data_json` 以 `Box<RawValue>` 承载),而这会牵动所有读源文档的装配代码(`assembly::build_document` 的 `src`/`theatre`、`build_audios`、`stage_size` 等)。因此该步**先做一次性 spike**:同一批语料下,骨架解析 + `RawValue → BlockTree` 与现状 `Value → BlockTree` 得到**逐字段相同**的树(`forward_parallel_tests` 的串行参考 + SHA 门交叉验证),证明后再决定是否落地。**该步不计入 §2.2 的目标**。

**字节陷阱**:语料源文件**不是紧凑 JSON**(实测 `download/compile/k4edit/174408420-0.bcm4` 含 14 580 个空白字符)。`RawValue` 只做"解析入口",**绝不能原样透传到产物**;产物一律由同一套序列化器重新写出。

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
