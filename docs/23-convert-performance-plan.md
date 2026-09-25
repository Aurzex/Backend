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
| A | **整份文档 JSON 三进三出** | 上表 parse/serialize;`translate_file` 先 `read_to_string`+`from_str`,产物再 `to_string`+`write` | 最大单项。文档里 90% 的字节(造型/音频/变量/云变量)我们**只透传不改**,却付了完整的解析与重编码代价 |
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

5. **G:实体级并行(角色 / 场景)**
   → 正反向都对"每个实体的积木树"独立处理,天然可并行:先串行扫一遍收集工作项(实体 → 树),再用 `thread::scope` + 固定并发消费,最后**按原顺序**装配(顺序敏感:官方产物里实体顺序影响 `actors_order`/`scenes_order`)。
   → **确定性 id 必须保**:`IdSource` 现在是 `&mut` 串行计数器。方案:并行前按实体预分配 id 段(每个实体一个起始偏移 + 长度上界),或每实体一个独立 `IdSource`(前缀区分),使产物与串行一致。
   → 收益:多实体作品近似线性加速(10 MB 作品实体多 → 2–4× 可期)。
   → 风险:**中高** —— 确定性 id 一旦分段算错,产物会与官方基线/往返测试不一致;并行下的错误聚合要显式。
   → 验证:同一输入在"并发 1 / 并发 8"下产物 **SHA256 相同**;差分门 + 往返类型多重集守恒。

6. **A:未触碰子树零拷贝透传(`serde_json::value::RawValue`)**
   → 打开 `serde_json` 的 `raw_value` feature(**不是新依赖**,只是既有依赖开 feature),把顶层文档里"只透传"的部分(`styles`/`audios`/`variables`/`cloud_variables`/`broadcasts`/`scenes_order`…)保留为 `Box<RawValue>`(切片,不解析),只对 `theatre.actors[*].block_data_json` 与必须重写的字段做真解析。
   → 收益:A 项里最大的那块 —— 期望把 10 MB 文档的 parse/serialize 成本砍到"只覆盖真正要改的子树"(文档越大越划算;官方 40 MB 级作品收益更明显)。
   → 风险:**高** —— 需要重构顶层文档的读写路径(typed shell + raw 字段),且与"逐字节对齐官方"的差分门必须逐项复核;`RawValue` 的键序/空白由原始字节决定,落盘时如果要求规范化排序会冲突(现在 `serde_json` 默认按键排序 → 需确认官方产物是否也排序;不排序就得保留原始字节序,反而更保真)。
   → 验证:差分门 + 往返;对同一输入与旧实现产物做**逐字节**比较(允许差异必须先解释清楚)。
   → 建议:**单独一轮**做,作为本轮之后的"大重构"。

7. **D:合并遍历(单遍后序)**
   → 现在 `parse_node`/`route_children`/`gc_deep`/`wrap_arithmetic`/`unwrap_*` 各自一遍树。可以合并成一次后序遍历 + 每节点按固定顺序执行子步骤(把"看完再改"的步骤显式标注)。
   → 收益:遍历次数 3–5 → 1–2,理论 20–40% 的内核时间。
   → 风险:**中高** —— 顺序耦合(某些步骤依赖前一步已经改过的子树/字段)。必须先写出步骤依赖表,再按依赖分层合并。
   → 验证:差分门 + 往返 + 真机门;合并过程中每一步都保持产物字节不变。

8. **E:减少字符串手术与键分配**
   → `transform_shadow_xml` 先用 `memchr`-风格扫描判断"是否需要改"(无需改时**直接借用原串**返回 `Cow::Borrowed`);`mutation` 只在确实有 `items`/属性变化时才重建;`map_field_name` 的 `into_owned()` 改为"未改名则沿用原键"(先在旧 map 上原地 rename:取出 value 再以旧键插回)。
   → 收益:典型作品里大多数影子/字段名并不改名 → 省掉大部分分配(估 10–25%)。
   → 风险:低。
   → 验证:单测 + 差分门。

### P2(结构性/收尾)

9. **id 与键的内存布局**:`BTreeMap<String, _>` → `Box<str>` 或复用缓冲(避免 `String` 的 24 字节 header);`BlockJson.id: Option<String>` 在树内可改 `Arc<str>`(并行时共享)。
10. **映射结果缓存**:`translate_type`/`map_field_name` 已是 O(1) 查表;再进一步可对"（kind, slot, field) → 结果"做 memo(命中率高时省表查找与 `format!`)。
11. **批处理并发**：`translate_works` 的 `batch_concurrency` 默认 1 偏保守;在内存直通(2 号)与实体级并行(5 号)落地后,建议给"每作品并发 × 每作品内实体并发"两级预算,避免线程数爆掉。
12. **反编译侧复用同一并发器**:`decompile_batch` 现在按作品并发;NEMO/WOOD 资源下载已并发(见 `docs/22`);Kitten/NEKO 的实体反编译也可实体级并行(同 5 号的 id 问题)。

## 4. 阶段与验收

| 阶段 | 内容 | 验收 |
| ---- | ---- | ---- |
| **S1** | P0 四项(流式输出 / 内存直通 / 去克隆 / 告警轻量化) | 产物**字节相同**(与旧实现比 SHA256)+ 全部单测 + 差分门 + 真机门 |
| **S2** | 建基准:release 下跑 §1 的三个样本 + 官方夹具,**打印分阶段耗时**(解析 / 映射 / 编码 / 装配 / 序列化)与"每块 µs";基准以 `#[test] #[ignore]` 形式进仓(不引 criterion) | 基线数字写进文档;后续每阶段对比 |
| **S3** | P1 的 5(实体级并行)与 8(字符串手术) | 并发 1 vs 并发 8 产物 SHA256 相同;基准加速比记录 |
| **S4** | P1 的 6(`RawValue` 透传)与 7(单遍遍历),按收益/风险单独提交 | 逐字节对比旧实现,差异逐条解释;往返不变式 |
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
| 大改(6/7 号)偏离官方的逐字节对齐 | 差分门 + 往返 + 字节级对比;每步单独提交,可回滚 |
| 实体级并行破坏确定性 id | 先做 id 分段设计 + 单测("并发 1/8 产物相同"),再落地 |
| 为性能牺牲可读性 | 每个优化都要有基准数据支撑;无数据不做 |
| `RawValue` 与"键排序"冲突 | 先确认官方产物是否按键排序(现有实现是);若冲突,保留原始字节序并更新对齐口径 |

**不做**

- 不引入新依赖(criterion/rayon 等一概不加;并发用 `std::thread::scope`,计数用 `std` 原语);
- 不改公开 API 形状(`translate_file`/`TranslateOptions`/`TranslateOutcome`/`TranslateReport` 的字段不变;新增入口只做**加法**);
- 不为"看起来更快"牺牲对齐:任何产物字节变化都必须先解释再接受;
- 不在本轮改 NEMO 的抓包未能证实的东西(见 `docs/22` §5/§6)。
