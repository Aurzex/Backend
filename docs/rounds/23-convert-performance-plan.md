# 第二十三轮方案 — convert 域性能优化(只出方案)

日期:2026-09-25 · 基线:`c98c3a2` · 范围:`src/core/convert/**`(重点是 `translate/` 与 `decompile/` 的共用面)
本轮**只给方案**。目标:在保持"与官方产物对齐"(差分门)与往返不变式的前提下,把 10 MB 级作品的双向转化从**秒级**压到**亚秒级**,并让批处理能真正并行。

---

## 1. 现状量级(实测,debug 构建)

样本是仓库里已有的真作品产物(`download/compile/`、`download/convert/`),`translate_file` 直接跑:

| 方向 | 样本 | 源 → 产物 | 墙钟 | `report.elapsed_ms`(转化内) | 积木 | 告警 |
| ---- | ---- | --------- | ---- | ---------------------------- | ---- | ---- |
| Kitten4 → KN | `原气骑士 且听风吟_136021231.bcm4` | 10.8 → 9.7 MB | 4.8 s | **1747 ms** | 12 764 → 14 024 | 209 |
| Kitten4 → KN | `几何对战-联机_215246857.bcm4` | 0.3 → 0.3 MB | 133 ms | 32 ms | 374 → 439 | 0 |
| KN → Kitten4 | `Phigros 自制谱模拟器_195038626.kn.bcmkn` | 9.4 → 9.8 MB | 2.1 s | **722 ms** | 5 235 → 4 624 | 1 817 |

单独量 JSON 出入(同一批文件):

| 文件 | 大小 | `from_str` | `to_string` |
| ---- | ---- | ---------- | ----------- |
| `原气骑士…bcm4` | 10.8 MB | **781 ms** | **1000 ms** |
| `Phigros….kn.bcmkn` | 9.4 MB | **301 ms** | **854 ms** |

> ⚠️ 全部是 **debug** 构建(项目现在没有 release 基准)。绝对值会随构建模式变化,但**占比**是可用的:
> JSON 出入占正向墙钟 **37%**、反向 **55%**;转化核心按块摊薄 ≈ **137 µs/块**(正向 1747 ms/12 764、反向 722 ms/5 235 —— 两个方向惊人地一致,说明瓶颈在共用内核而非某一侧)。

## 2. 时间去哪了(按证据拆)

| # | 位置 | 证据 | 判断 |
| - | ---- | ---- | ---- |
| A | **整份文档 JSON 三进三出** | 上表 parse/serialize;`translate_file` 先 `read_to_string`+`from_str`,产物再 `to_string`+`write` | 最大单项。**该判断已被预研否证**(见 `docs/rounds/26` §6):`theatre`(含 `block_data_json`)占文档 **99.2–99.5%** 且**全部重写**,`styles`/`audios`/`variables`/`broadcasts` 也都改名/重算/包裹 ⇒ **没有「只透传」的大块**,A 项只能靠「少一轮往返 + 去克隆」来削(已在 P0 做完) |
| B | **`translate_work` 多一轮落盘+读回** | `decompile_with_options` 把编辑版 **写盘** → `translate_file(&source_path)` 再 **读回并解析** | 10 MB 级:白付一次 serialize + 一次 parse(≈0.3–1.0 s) |
| C | **逐块克隆** | `BlockJson::from_value`:`serde_json::from_value(Value::Object(obj.clone()))` | 每个块复制一次 JSON Map(12 764 块);`extra`/`fields`/`inputs`/`shadows` 全在里面 |
| D | **多趟遍历** | `parse_node` → `route_children` → `gc_deep` → `wrap_arithmetic` … 正反向各 3~5 次 walk | 每趟都要匹配 `kind`、改 map;树越大越贵 |
| E | **字符串手术** | `transform_shadow_xml`(每影子一次)、`mutation_xml`/`increment_items`/`decrement_items`、`map_field_name(..).into_owned()` | 每块平均带 1–2 个影子 → 每块至少一次字符串重建 + 一次键分配 |
| F | **告警成本** | 反向 1 817 条告警 / 5 235 块(35%):每条 `format!` 出 `String` 路径,`TranslateReport` 用 `BTreeMap<(u8,String), …>` 聚合(键克隆) | 与"块"同数量级,不能忽略;`to_markdown` 才需要人类可读文本 |
| G | **实体级串行** | 正反向都是"逐角色/逐场景"顺序处理;`batch_concurrency` 只在**作品**粒度并行 | 10 MB 作品动辄几十~几百实体,天然可并行(需保确定性 id) |
| H | **表查找** | 已在第二十一轮改 `LazyLock<HashMap>`(正向) | 已解决,保留 |

## 3. 优化清单

每条:**问题 → 方案 → 预计收益 → 风险 → 验证**。收益按 debug 实测外推,标注为量级而非承诺。

### P0(改动小、收益确定)

1. **A:流式输出,去掉中间 `String`**
   → 产物改 `serde_json::to_writer(BufWriter::new(File::create(path)?), &doc)`;`translate_work` 路径同理。
   → 收益:省掉"9.7 MB String + 一次整块拷贝"(serialize 时间的 15–25%)与峰值内存(≈文档大小 ×2)。
   → 风险:无(序列化结果逐字节相同)。
   → 验证:官方差分门 + 现有往返测试;对同一输入比较产物 SHA256(应完全相同)。

2. **B:消除 `translate_work` 的中间落盘/读回(内存直通)**
   → 反编译层暴露"给我编辑版 `Value`"(现在只有落盘路径 `DecompileResult::Json/Path`);`translate` 增加 `translate_value(Value, target, options)` 入口,`translate_file` 变成它的壳(读盘 → 调用 → 写盘)。
   → 收益:10 MB 级省一次 serialize+parse(≈0.3–1.0 s,墙钟 10–20%);顺带不再依赖 staging 目录(第二十一轮的竞态问题从根上少一处)。
   → 风险:低。要保证两条入口共用同一段管线(否则会出现"文件路径"与"内存"两条分叉)。
   → 验证:`translate_file` 与 `translate_value` 对同一输入产物逐字节相同(加一条回归).

3. **C:去掉 `BlockJson::from_value` 的整块克隆**
   → `from_value` 改成消费 `Value`(`from_value_owned`)或在 `parse_parts` 阶段就把块**移动**进去(`blocks.remove(id)` 后 move);必要处用 `serde_json::from_value` 直接接收 `Value`(它支持 move)。
   → 收益:每块省一次 Map 克隆;按 12 764 块、每块 0.3–1 KB 估,数十 MB 级内存流量。
   → 风险:低(借用顺序需调整)。
   → 验证:全部单测 + 差分门 + 正向真机门(产物字节相同)。

4. **F:告警轻量化**
   → 分类键改成"枚举 + `&'static str`/`u32` id"(现在每条 `String`),`path` 用 `Cow<'static, str>` 或延迟到 `to_markdown` 时才 `format!`;聚合键用 `(u8, u32)` 计数表。
   → 收益:反向 1 817 条告警的场景省掉等量 `String` 分配与 BTreeMap 键克隆(估 5–15% 的反向时间)。
   → 风险:低(`TranslateWarning` 是公开枚举,改内部聚合不影响其形状)。
   → 验证:报告类单测(计数与类别)+ `to_markdown` 快照。

### P1(收益大,但要认真做)

5. **G:实体级并行(角色 / 场景)** —— ✅ **正向已落地(2026-09-25)**,见
   `docs/rounds/25-convert-entity-parallelism-plan.md` §9(反向仍待按该文 §7 #1 三段重设计);
   结论摘要:临时 id + 串行兑现/改写,`entity_concurrency` 默认 1,
   **并发 1 产物与基线逐字节一致**、**1 vs N 同 SHA256**,公开 API 只做加法。
   → 正反向都对"每个实体的积木树"独立处理,天然可并行:先串行扫一遍收集工作项(实体 → 树),再用 `thread::scope` + 固定并发消费,最后**按原顺序**装配(顺序敏感:官方产物里实体顺序影响 `actors_order`/`scenes_order`)。
   → **确定性 id 必须保**:`IdSource` 现在是 `&mut` 串行计数器。方案:并行前按实体预分配 id 段(每个实体一个起始偏移 + 长度上界),或每实体一个独立 `IdSource`(前缀区分),使产物与串行一致。
   → 收益:多实体作品近似线性加速(10 MB 作品实体多 → 2–4× 可期)。
   → 风险:**中高** —— 确定性 id 一旦分段算错,产物会与官方基线/往返测试不一致;并行下的错误聚合要显式。
   → 验证:同一输入在"并发 1 / 并发 8"下产物 **SHA256 相同**;差分门 + 往返类型多重集守恒。

6. **A:未触碰子树零拷贝透传(`serde_json::value::RawValue`)**
   → 打开 `serde_json` 的 `raw_value` feature(**不是新依赖**,只是既有依赖开 feature),把顶层文档里"只透传"的部分(`styles`/`audios`/`variables`/`cloud_variables`/`broadcasts`/`scenes_order`…)保留为 `Box<RawValue>`(切片,不解析),只对 `theatre.actors[*].block_data_json` 与必须重写的字段做真解析。
   → 收益:A 项里最大的那块 —— 期望把 10 MB 文档的 parse/serialize 成本砍到"只覆盖真正要改的子树"(文档越大越划算;官方 40 MB 级作品收益更明显)。
   → **结论:不做**(`docs/rounds/26` §6 评审):装配阶段「只透传不改」的顶层字段占比 **≈0%**(theatre 99.2–99.5% 全部重写;styles/audios/variables/broadcasts 均被变换;cloud_variables/size/scenes_order 是**只读输入**不输出),收益上限 ≈0%,远低于 §2 预研 A 的 30% 门槛。
   → 验证:差分门 + 往返;对同一输入与旧实现产物做**逐字节**比较(允许差异必须先解释清楚)。
   → 建议:**单独一轮**做,作为本轮之后的"大重构"。

7. **D:合并遍历(单遍后序)**
   → 现在 `parse_node`/`route_children`/`gc_deep`/`wrap_arithmetic`/`unwrap_*` 各自一遍树。可以合并成一次后序遍历 + 每节点按固定顺序执行子步骤(把"看完再改"的步骤显式标注)。
   → 收益:~~遍历次数 3–5 → 1–2~~ **结论:不做**(`docs/rounds/26` §6 评审):正向实为 **2 趟**全遍历(`parse_node` 内含 `route_children` 再 `gc_deep`),反向本已单遍;合并只省遍历与缓存未命中,**不省逐块 `kind` 匹配 / `transform_shadow_xml` / `map_field_name`** 这些主要成本;且 `gc_node` 依赖子树**已 parse**、`shadow_number` 还自递归 ⇒ 改后序会双重应用。
   → 风险:**中高** —— 顺序耦合(某些步骤依赖前一步已经改过的子树/字段)。必须先写出步骤依赖表,再按依赖分层合并。
   → 验证:差分门 + 往返 + 真机门;合并过程中每一步都保持产物字节不变。

8. **E:减少字符串手术与键分配** —— ✅ **部分落地(2026-09-25,`mapping.rs`)**:
   - 字段/槽位改名:`map_field_name` / `get_mapped_name` 的结果**按值**判定,未改名时
     **复用消费进来的原键**(`orig_fields` / `orig_shadows` 本来就是按值消费),少一次 `String`;
   - `transform_shadow_xml` 改**按值**收 XML(解析路径搬出来的 `String`),不需要改写时原样返回;
     逐字段改为直接 `push_str` 原切片,不再各建一个临时 `String`;
   - `mapped_field_text` 返回 `Option<&'static str>`(表值本就静态),不再每次 `to_string`。
   - **坑(踩过一次,记下来)**:`Cow::Borrowed` **不等于**未改名 —— 改到静态表名
     (`"variable"`/`"list"`/槽位表)同样是 `Borrowed`,必须 `if mapped == 原值` 才算未改;
     否则静默漏改名(产物变小,6 个单测 + 基准 SHA256 同时报错)。
   - 验收:四样本产物 SHA256 **逐字节不变**、67 项单测全绿;但**加速在噪声内**
     (绑核 5 轮取最小,`core` 259 vs 261 ms)——按本方案"无数据不做"的口径,保留的理由是
     "构造上必然更少分配",不是"已测出更快"。

   原条目:

   → `transform_shadow_xml` 先用 `memchr`-风格扫描判断"是否需要改"(无需改时**直接借用原串**返回 `Cow::Borrowed`);`mutation` 只在确实有 `items`/属性变化时才重建;`map_field_name` 的 `into_owned()` 改为"未改名则沿用原键"(先在旧 map 上原地 rename:取出 value 再以旧键插回)。
   → 收益:典型作品里大多数影子/字段名并不改名 → 省掉大部分分配(估 10–25%)。
   → 风险:低。
   → 验证:单测 + 差分门。

### P2(结构性/收尾)

9. **id 与键的内存布局**:`BTreeMap<String, _>` → `Box<str>` 或复用缓冲(避免 `String` 的 24 字节 header);`BlockJson.id: Option<String>` 在树内可改 `Arc<str>`(并行时共享)。
10. **映射结果缓存**:`translate_type`/`map_field_name` 已是 O(1) 查表;再进一步可对"（kind, slot, field) → 结果"做 memo(命中率高时省表查找与 `format!`)。
11. **批处理并发**：`translate_works` 的 `batch_concurrency` 默认 1 偏保守;在内存直通(2 号)与实体级并行(5 号)落地后,建议给"每作品并发 × 每作品内实体并发"两级预算,避免线程数爆掉。
    → ✅ **已落地(2026-09-25)**:`translate_works` 折算 `entity_concurrency =
    min(请求, 可用核数 / 有效作品并发)`(`docs/rounds/25` §7 阻塞 #6),折算只改并行度不碰产物。
12. **反编译侧复用同一并发器**:`decompile_batch` 现在按作品并发;NEMO/WOOD 资源下载已并发(见 `docs/rounds/22`);Kitten/NEKO 的实体反编译也可实体级并行(同 5 号的 id 问题)。

## 4. 阶段与验收

| 阶段 | 内容 | 验收 |
| ---- | ---- | ---- |
| **S1** | P0 四项(流式输出 / 内存直通 / 去克隆 / 告警轻量化) | 产物**字节相同**(与旧实现比 SHA256)+ 全部单测 + 差分门 + 真机门 |
| **S2** | 建基准:release 下跑 §1 的三个样本 + 官方夹具,**打印分阶段耗时**(解析 / 映射 / 编码 / 装配 / 序列化)与"每块 µs";基准以 `#[test] #[ignore]` 形式进仓(不引 criterion) | 基线数字写进文档;后续每阶段对比 |
| **S3** | P1 的 5(实体级并行)与 8(字符串手术) | 并发 1 vs 并发 8 产物 SHA256 相同;基准加速比记录 |
| **S4** | ~~P1 的 6 与 7~~ **预研后判不做**(`docs/rounds/26` §6:透传占比≈0%、正向本已 2 趟) | 预研证据留档即可,不动代码 |
| **S5** | P2 收尾 + 文档 | — |

> S1 与 S2 建议**先做**,因为"先有基准再谈优化"——否则第 6/7 项这种大改无法判断是否真的变快。

## 5. 基准方法(避免自欺)

- **必须 release**:debug 下 serde_json 与字符串操作被拖得不成比例,会误导优化方向;基准测试固定 `--release` 跑。
- 样本固定:上面三个真作品产物(10.8 MB / 0.3 MB / 9.4 MB)+ `tests/fixtures/translate/geoduel_scene_actor.json`(官方基线切片)。
- 每次都打印:`parse` / `映射` / `编码` / `装配` / `serialize` 各阶段 + 每块 µs + 产物字节数 + 告警数,并**断言产物 SHA256 与基线一致**(性能改了但产物不一样 = 失败)。
- 至少跑 3 遍取中位数;报告机器与构建模式。
- 并发相关的基准要额外跑"并发 1"作对照,证明加速来自并行而不是"少干了活"。

## 6. 风险与不做

**风险**

| 风险 | 缓解 |
| ---- | ---- |
| 大改(6/7 号)改变产物字节 | 6/7 号已按预研判**不做**(`docs/rounds/26` §6);若将来重开:唯一的字节门是**自有** SHA256 基线(`tests/convert_bench.rs`),官方差分门只做语义比较(官方产物按键插入序,从未逐字节对齐) |
| 实体级并行破坏确定性 id | 先做 id 分段设计 + 单测("并发 1/8 产物相同"),再落地 |
| 为性能牺牲可读性 | 每个优化都要有基准数据支撑;无数据不做 |
| `RawValue` 与"键排序"冲突 | 先确认官方产物是否按键排序(现有实现是);若冲突,保留原始字节序并更新对齐口径 |

**不做**

- 不引入新依赖(criterion/rayon 等一概不加;并发用 `std::thread::scope`,计数用 `std` 原语);
- 不改公开 API 形状(`translate_file`/`TranslateOptions`/`TranslateOutcome`/`TranslateReport` 的字段不变;新增入口只做**加法**);
- 不为"看起来更快"牺牲对齐:任何产物字节变化都必须先解释再接受;
- 不在本轮改 NEMO 的抓包未能证实的东西(见 `docs/rounds/22` §5/§6)。

---

## 7. 落地记录(2026-09-25:S1 + S2 已完成)

### 7.1 已实现(`d355cbe`)

| 项 | 内容 | 关键位置 |
| -- | ---- | -------- |
| P0-1 流式落盘 | `FileService::write_json` 改 `to_writer` + `BufWriter`(与 `to_string` 逐字节相同);`translate_file` 与 `set_source_reference` 都走它 | `shared/infra.rs`、`translate/mod.rs` |
| P0-2 内存直通 | 新增 `translate::translate_value` / `TranslateDocument` 与 `decompile::decompile_artifact_with` / `DecompiledArtifact`;`translate_work_in` 全程内存(源引用直接改内存文档,只有"要上传源文件"那一路才落一次盘,文件名与旧路径逐字一致);`set_source_reference_in`(内存)+ 文件版保留为壳 | `translate/mod.rs`、`decompile/mod.rs`、`convert/mod.rs` |
| P0-3 去整块克隆 | `BlockJson::from_value` 改**借用**反序列化(旧:`Value::Object(obj.clone())` 每块深拷贝一次);`neko::parse_kn_entity` 字符串态移动数组(旧:`as_array().cloned()`);正向取走 `block_data_json` 再克隆实体、装配后**原样放回**(源文档不变);反向 `build_kitten4_document` 改按值消费 `entities` / `blocks_by_entity` | `translate/model.rs`、`neko.rs`、`mod.rs`、`assembly.rs` |
| P0-4 告警轻量化 | `counts()` 改借用键聚合,最后只对去重后的类别/主体字符串化;删掉 `TranslateWarning::key()`(旧实现每条告警 3 次 `String` 分配,且 `key()` 的结果被 `let _ =` 丢弃) | `translate/mod.rs` |
| 死代码 | 删 `scene_order`(克隆一份 `scenes_order` 后被 `let _ =` 丢弃) | `translate/mod.rs` |
| S2 基准 | 新档 `profile bench_perf`(不动发布档:`opt-level="z"`+`panic="abort"` 会污染量测);`tests/convert_bench.rs`(分阶段耗时 + 四样本产物 SHA256 基线守门)、`tests/convert_facade_bench.rs`(同轮内并排比"旧盘→盘流程"与"内存直通");基线 `tests/fixtures/translate/convert_bench_baseline.json` | `Cargo.toml`、`tests/` |

### 7.2 验收证据

- 单测 **67 项全绿**,含差分门(`diff_tests`)、往返类型多重集守恒(`reverse_tests`)、
  "确定性 id 下两次转换逐字节一致"的门。
- 四个样本产物 SHA256 与基线**逐字节一致**(`bafeb50c…` / `d653a8a5…` / `e7680dcc…` / `bfde1fc1…`)。
- 绑核(`taskset -c 2`)A/B,同估计量(5 轮取最小)。样本代号:
  ① 正向 10.8 MB(`download/compile/原气骑士 且听风吟_136021231.bcm4`)、
  ② 正向 0.3 MB(`download/compile/几何对战-联机_215246857.bcm4`)、
  ③ 反向 9.4 MB(`download/convert/Phigros 自制谱模拟器_195038626.kn.bcmkn`)、
  ④ 反向 3.7 MB(`download/compile/HEX Editor_317683843.bcmkn`)。

| 样本代号 | 旧 `core`/`e2e` ms | 新 `core`/`e2e` ms | 判断 |
| -------- | ------------------ | ------------------ | ---- |
| ① 正向 10.8 MB | 243 / 644 | 261 / 658 | **在噪声内**:该样本的对照指标 `ser_ms` 自身 58→81(+40%),无法判定 |
| ② 正向 0.3 MB | 8 / 23 | 6 / 16 | 小幅变快 |
| ③ 反向 9.4 MB | 217 / 424 | 184 / 323 | **-15% / -24%** |
| ④ 反向 3.7 MB | 57 / 114 | 53 / 91 | -7% / -20% |

- 域门面路径(`translate_work` 实际走的"内存直通")**同一轮内**并排比旧流程:1.34–1.51×
  (旧流程 = `translate_file` 盘→盘 + `set_source_reference` 读回-改写-写回,两者产物 SHA256 相同)。
- **测量纪律**(已写进 `tests/convert_bench.rs` 头注释):这台机器上绝对毫秒会漂(同一二进制两次跑
  `core` 差 20–40%),跨轮只能看量级,"谁更快"必须在**同一轮内并排比**;每样本 5 轮取**最小值**,
  首个样本前加一轮预热(否则冷缓存/调频爬升会污染它,10.8 MB 正向样本正是首样本)。

### 7.3 未做 / 转下一轮

- **P1-8 字符串手术减负** → ✅ **本会话已落地**(见 §3 P1-8 条目):按值判定后复用原键、
  影子 XML 按值改写、`mapped_field_text` 返回静态借用;产物逐字节不变、67 项单测全绿,
  但**加速在噪声内**(绑核 `core` 259 vs 261 ms)——保留依据是"构造上更少分配"。
- **P1-5 实体级并行** → ✅ **正向已落地**(`docs/rounds/25` §9:临时 id + 串行兑现/改写、
  `entity_concurrency` 默认 1、两级预算折算、1 vs N 同 SHA256、基线逐字节不变);**反向一行未动**。
  - **主代理独立复核**(`taskset -c 0-3`、5 轮取最小、本机 2 物理核/4 逻辑核):
    10.8 MB 正向 `core` 285 → 184 ms(**1.55×**)、`e2e` 696 → 590 ms(1.18×);
    四样本产物 SHA256 与基线**逐字节一致**、并发 1 vs 8 **同 SHA256**;反向两样本 1.00–1.02×(零差异)。
    方案 §4 写的"≥3×"是 8 线程口径,**本机物理核不够,无法验证**。
  - **默认(并发 1)的串行开销:本机测不出来**。同一基准 10.8 MB 正向 `core` 三次复跑
    254 / 285 / 300 ms(S3a 之前同口径是 261),e2e 593–710 ms(之前 658)——
    散布盖过了差异 ⇒ **不下结论**;机制上会多"阶段 0 取走 + 兑现改写一趟",量级应在个位数百分比。
    真正需要拍板的是**默认值**:现在默认 1(可预测、行为不变),调用方可显式
    `entity_concurrency(available_parallelism())` 换并行;是否把默认改成"大作品自动开"由使用者定。
  - 原方案与评审记录仍在 `docs/rounds/25`(评审判定**有条件可行**,反向需三段重设计,见该文 §7 #1;
    前置守门测试已落地一条,见 §8)。设计依据:正向 10.8 MB 有 209 个实体、最大实体仅占 4%。
  - **反向(S3b):数据判定不做**(`docs/rounds/25` §10):63 个工作项、最大一项占 **36.7%**,
    且两段必须串行(`unrewrite_calls` 依赖全局 `call_targets`、`def_root_from_entry` 把定义根
    挂进宿主实体)⇒ Amdahl 上限 1.9×(4 线程**理论上限**),实际远低于 1.5×,而反向 `core` 只有 ~200 ms。
    **并行的收益面集中在正向大实体作品**(已落地并验证)。
- **P1-6 `RawValue` / P1-7 单遍遍历** → `docs/rounds/26-convert-rawvalue-single-pass-plan.md`
  (先做预研 A 透传占比 / B 趟数占比 / C 键序口径,再决定是否动顶层读写路径)。
- **P2-9 / P2-10 / P2-11 / P2-12** 未动。
