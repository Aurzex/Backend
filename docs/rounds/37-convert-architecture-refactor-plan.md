# 第三十七轮方案 — convert 域重构:架构归位 + 适当合并 + 性能(约束:产物逐字节不变)

> 日期 2026-09-26 · 只读调研(5 路并行侦察 + 实机基准)后的**方案**,本轮不改代码。
> 事实依据:`docs/knowledge/convert-semantics.md`、`docs/rounds/31`(上一轮编排合并与"不合并"清单)、
> `docs/rounds/36`(最近一轮)、`docs/goals/convert-backlog.md`;**代码锚点全部为本次复核的现行行号**。

## 0. 结论摘要

| 判断 | 依据(要点) |
| ---- | ---------- |
| **分层是干净的,不需要"拆架构"** | `convert/mod.rs`(257 行)= 门面 + 跨子域编排 + 上传编排;`shared.rs` 不对子域反向依赖;`translate/` 与 `decompile/` 互不依赖(规则写在 `convert/mod.rs:5-13`) |
| **真正的架构问题是"职责错位 + 两处真环"** | ① `translate/mod.rs` 一个文件装了**五件事**(选项/错误、报告层、正/反管线、入口、诊断),导致兄弟模块的 `use super::{TranslateReport,…}` **向上依赖**(实测:`mapping.rs:52`、`model.rs:2`、`nemo.rs:3-4`、`nemo_mapping.rs:1-2` 是 `use super::{…}` 形态;`assembly.rs` 经另一路径引用);② `model ⇄ mapping` 环(model.rs:1 ↔ mapping.rs:46-47);③ `nemo.rs ⇄ nemo_mapping.rs` 环(nemo.rs:6-11 ↔ nemo_mapping.rs:3-4) |
| **合并空间不大,但有两处"真职责重叠"** | 正向管线在 `translate/mod.rs:319-680`、反向管线在 `assembly.rs:804-912`,二者共用的**并行机在 assembly.rs:2152-2534 且排在 `assembly_tests`(1678-2168)之后** ⇒ 违反本仓"测试在文件末尾"规则,也是文件超 2500 行的主因 |
| **死重量/样板可确定删除** | `FileService.config` 字段全仓零读取(shared.rs:1028-1031)+ 它污染的 3 个 `file_service` 字段;`TOP_BLOCKS`/`KN_TYPES`(tables_gen.rs:1203/1216)零生产调用点;5 份 Fetcher 壳 + 3 份 `save_result` + 5 份变体错误分支(editors.rs);11 份 `LazyLock` 索引样板;9 个 `pub(crate)` 读取器(translate/mod.rs:187-247) |
| **性能上"分配"比"算法"更值得动** | 反向装配有 3~4 次整份积木 JSON 深拷(assembly.rs:847、996/999/1027/1054);`translate_work` 每次白写+删一份与源同量级的 raw JSON(decompile 侧 `save_raw` 默认 true);正向入口有一次**无条件**全文档扫描 + 二次 parse(mod.rs:523/1075-1114) |
| **先决阻塞:现在无法证明"输出不变"** | `cargo test --profile bench_perf --test convert_bench -- --ignored` 实测:两个**反向**样本 SHA 与基线不一致(正向两个一致)——与 34–36 轮刻意改反向输出吻合,但**基线没有同步**:任何重构现在都拿不到"输出不变"的机器证据(详见 §6.2) |

**做法**:4 个阶段,每阶段独立提交、独立回退;先修门(Phase 0),再纯搬迁(Phase 1),再样板/死重量(Phase 2),
最后性能(Phase 3),收尾(Phase 4)。**不动公开面**(`rounds/31 §3`)、不重开已决事项(§3.3 列出清单)。

---

## 1. Phase 0 — 先决条件(不做完不进入任何重构)

| # | 项 | 为什么必须先做 | 具体做法 |
| - | -- | -------------- | -------- |
| 0.1 | **重刷并加固 SHA 基线** | 现在基线红(§6.2):反向两样本不一致。若不先"有据地"重刷,后面每次改动都会撞一个"本来就红"的门,分不清是自己改坏的还是历史遗留 | ①在提交信息/diff 里写明"本次重刷的原因 = rounds/34–36 的**刻意**改动(词表挑名、影子判据、`theatre.groups` 合成)";②同时把 `#meta`(blocks_total/converted/warnings/bytes,**tests/convert_bench.rs:336-341**)从"只记录"升格为断言;③`load_baseline()` 解析失败改成**显式 panic**(:370-375 现在会静默重建);④基线键加**源文件 SHA**(现在只记 `source_bytes`,输入被换掉看不出来) |
| 0.2 | **严格模式开关** | `convert_bench` 在 debug / 缺样本 / 未加 `--ignored` 时**静默 return 却显示 pass**(:216-230),CI 上等于没有这条门 | 照 `BACKEND_REQUIRE_LIVE=1` 的先例加 `BACKEND_REQUIRE_BENCH=1`:缺样本或非 bench 构建时直接 panic |
| 0.3 | **冻结三条"协议"** | 重构不得改变它们,否则门会以假象通过或误报 | ①`strip_counts` 反解的中文 marker(`已剔除 N`/`已清空 N`,reverse_tests.rs:777-795);②告警**产生顺序**(procedure_library 测试比 `report.warnings()` 逐条 :1556-1559);③**id 铸造顺序**(deterministic 下产物 id 是铸造序号的纯函数,model.rs:443-444) |
| 0.4 | **补 decompile 侧离机单测** | `src/core/convert/decompile/` **零** `#[cfg(test)]`(1844+1661 行生产代码无单测),它的回归只能靠真机门 ⇒ 改这里的风险被系统性低估 | 用 `nemo_tests.rs:794` 那种自造 `DecompilerContext` + `OfflineHttp` 的写法,先给 `XmlBlockWriter::write_blocks`、`referenced_ids`、`child_input_name` 这类纯函数加 1–2 条断言 |
| 0.5 | **给 NEMO 方向补门(口径已更正)** | 更正:说"NEMO 没有门"**过强** —— `nemo_tests` 有 **6 条默认跑的单测**(`nemo_tests.rs:107/148/200/283/720/793`,走内存文档,覆盖值槽/双形态/QC-YC 迁移/占位降级/元素计数/内存反编译入口);真正缺的是 ① **SHA 字节基线** ② `translate_file` 真文件路径 + 真作品这一层的默认门(现状只有 `#[ignore]` 的 `nemo_real_samples_match_official_products:335`)③ 性能观测(`elapsed_ms` **恒 0**) | ①`nemo.rs:65` 补 `let started = Instant::now();`、`:570` 前 `report.elapsed_ms = …`;②把 `download/compile/蛋仔派对2-…/194684070.bcm`(3.4 MB,NEMO 0.16.2)加进 `convert_bench` 的 `SAMPLES`(`target: KittenN`、`slug: "kn"`;`detect_editor` **按内容**判定,扩展名无关);③可选:用手写极小 NEMO 文档(照 `nemo_tests.rs:11-84`)加一条默认跑的 `translate_file` 门 |
| 0.6 | **确认产物链无 `HashMap` 迭代** | 有的话 SHA 会随机红 | 本次静态排查未发现(产物键序由 `serde_json::Map`=BTreeMap 保证,Cargo 未开 `preserve_order`);Phase 1 之后再用扫描器复验一次 |

**Phase 0 出门条件**:`fmt/clippy/test` 全绿 + `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored` **全绿**。

---

## 2. 架构目标(`translate/` 的模块图)

### 2.1 改前 → 改后(**行数=生产码,不含内联测试**;测试另计)

现况统计(实测):`translate/` 共 **13** 个 `.rs`(含 `reverse_tests.rs`、`nemo_tests.rs` 两个测试文件),
`convert/` 共 **14** 个文件;`translate/mod.rs` 2192 = 生产 **1116** + 测试 **1076**(`diff_tests` 456 + `forward_parallel_tests` 620);
`translate/assembly.rs` 2644 = 生产头段 **1677** + `assembly_tests` **491** + 生产尾段 **366**(并行机 + remint,排在测试之后)+ `remint_tests` **110**。

```
改后(新增 4 个文件:options/report/pipeline/xml ⇒ 14 → 18;测试文件数不变)
options.rs     ≈ 255  公开面:TargetEditor/StageOrientation/TranslateOptions/TranslateOutcome/TranslateError
report.rs      ≈ 200  横切:TranslateWarning/TranslateReport
pipeline.rs    ≈ 830  正/反文档级编排(原 mod.rs 生产段 ~362 + assembly 反向段 ~110)+ 并行机(~97)+ remint(~256)
mod.rs         ≈ 500  仅门面/入口(translate_value/file、源引用、detect_editor、模块声明)+ 测试 456(留)
mapping.rs     ≈ 1990 语义表 + 双向映射(仅移出 XML 手术)
model.rs       ≈ 2100 中核模型 + id + 编解码 + 程序集(仅移出 XML 借用)
assembly.rs    ≈ 1930 装配器(正/反实体与资源字典)+ assembly_tests 491
xml.rs         ≈ 950  XML DOM + 字符串手术 + 影子渲染/转义(断两个环)
nemo.rs        ≈ 1180 NEMO 管线 + 版本迁移 + 前置改写(nemo_nxml_tests 371 随 XML 层迁走)
nemo_mapping   ≈ 2580 NEMO 解析 + 映射 + 表
```

> **注意**:`assembly.rs` 只搬并行机(**≈97 行**)仍剩 **2547 > 2500**(仓库软上限)。
> 要真正回到上限内,必须**连 remint 段(≈256 行)一起搬进 `pipeline.rs`**(2547 − 256 ≈ 2291 ✓)——
> 这会**重新决定 rounds/31 §3.5(c)\"remint 并入 assembly\"那条**:当时的动因是"少一个文件、且临时 id 改写的唯一调用方就是管线",
> 而现在 `pipeline.rs` 存在 ⇒ 管线的机器(并行 + 临时 id 兑现 + remap)同处一文件更顺。
> 这条算**重开已决事项**,所以列为 §9 的 Q2,由你拍板;不拍板则维持 2547(超软上限 47 行,可接受但要在文件头记账)。

### 2.2 每次搬迁:代价与验证

| 搬迁 | 收益 | 代价(机械但必须一次做完) | 验证 |
| ---- | ---- | ------------------------ | ---- |
| 报告层 → `report.rs` | 消掉 5 条向上依赖;报告成为可独立引用的横切面 | 5 个兄弟文件的 `use super::{TranslateReport, TranslateWarning}` 改 `use super::report::…` | 编译 + `cargo test --lib` + 扫描器 |
| 选项/错误 → `options.rs` | mod.rs 从"什么都装"变回门面;与 `DecompileOptions` 同层同义 | `mod.rs` 必须 `pub use options::{…}` 再导出(**公开路径不变**) | `tests/convert_live.rs`、`convert_bench.rs` 用的就是公开路径 ⇒ 编译即证 |
| 正/反管线 + 并行机 → `pipeline.rs` | **本次唯一"真职责重叠"的修复**:正/反编排同层可对照;`assembly.rs` 恢复单一职责且测试回到末尾 | `assembly.rs` 内 9 处引用(`assembly::workers/run_items/IdRemap/remap_*/merge_report`,translate/mod.rs:531/546/568/622/595/616/623/649/652)+ 2 条 doc 注释 | 编译 + `forward_parallel_tests`(不依赖 download/)+ `convert_bench` 1 vs 8 同 SHA |
| XML 层 → `xml.rs`(两处环) | 断 `model ⇄ mapping` 与 `nemo ⇄ nemo_mapping`;**这是分层修复,不是省行数**(净增 1 文件) | `mapping.rs` 的字符串手术(467-582)+ 影子构造(587/667)+ `nemo.rs:1452-2204` DOM 与 `:2205-2575` XML 单测整体搬;`model.rs:1071` 的 `VALUE_SHADOW_XML` 是**逐字节照搬官方**形态(含换行缩进)⇒ 不得与 `math_number_shadow` 统一格式化 | 逐字节比 5 处影子模板产物 + `nemo_tests` 的 `value_slot_keeps_shadow_xml_and_override_block` |
| 通用调度器 → `pipeline.rs` | 2644 → **2547**(仍超软上限;要 ≤2500 必须连 remint 段一起搬 ⇒ 见 §2.1 注与 Q2) | `workers`(2179-2190)/`run_items`(2191-2275)≈97 行 + 3 条调度用例 | 同上 |

**不做**:按方向把 `assembly.rs`/`mapping.rs` 切成两份 —— 双向共用工具集中在一处是**防漂移设计**(assembly.rs:60-62 注释),
且 rounds/32 的 `pure_list_get` 影子 bug 正是"正向漏了、反向有"的不对称 ⇒ 同居是刻意保留的可对照性。

---

## 3. 合并清单

### 3.1 做(收益确定)

| # | 合并 | 省/得 | 风险 |
| - | ---- | ----- | ---- |
| M1 | `editors.rs` 的 5 份 Fetcher 壳(56-68/782-794/1104-1116/1331-1343/1440-1452)→ 一个 `HttpFetchCtx` 内嵌 | ≈ −40 行样板 | 低:`fetch` 体一行不动(rounds/31 §3.6 N3 已判"URL 拼装不可合并") |
| M2 | 3 份 `save_result`(272-285/1311-1324/1409-1422)→ 一个 helper + 3 行调用 | ≈ −26 行 | 低:三份函数体**逐字节相同**,只差编译器名字面量 |
| M3 | 5 份 `RawWorkData` 变体错误分支(90-96/855-861/1299-1305/1397-1403/1495-1501)→ `expect_variant(name)` | ≈ −25 行 | 低:错误文案逐字保留 |
| M4 | `NemoResourceConfig`(772-780)与 `WoodResourceConfig`(1430-1438)→ 一个 `ResourceConfig`(顺带删死字段) | ≈ −9 行 | 低:字段逐项相同 |
| M5 | **删 `FileService` 的死 `config` 字段 + 3 个 `file_service` 字段**(shared.rs:1028-1031、decompile/mod.rs:490/731、editors.rs:774/835/1432/1480、nemo_tests.rs:814) | 删掉一个 Arc 深拷贝链与 4 个死字段 | 低:全仓**零读取点**(已 grep 确认)。改动只在 `pub(crate)` 面 |
| M6 | 11 份 `LazyLock<HashMap>` 索引样板(mapping.rs:288-376、nemo_mapping.rs:128-184)→ `flat_index/nested_index` 两个泛型自由函数 | ≈ −55 行 | 低:不用宏、语义(首命中优先)逐键等价 |
| M7 | `tables_gen.rs` 删 `TOP_BLOCKS`(1203-1213)+ `KN_TYPES`(1216-1426),**并同步改生成器** `src/bin/gen_translate_tables.rs` | ≈ −234 行死数据 | 低:零生产调用点;必须改生成器否则下次重跑复活;顺带改写 2 条提到 `KN_TYPES` 的注释(**实测锚点**:`translate/mod.rs:33`、`mapping.rs:916`) |
| M8 | `ParsedEntity`(model.rs:570-573)单字段 newtype → 直接返回 `BlockTree` | ≈ −3 行 + 消噪声 | 低:全部调用点都立刻 `.tree` 拆包 |
| M9 | `parse_int_prefix` 两份(nemo.rs:1260 / nemo_mapping.rs:495)→ `xml.rs`;整数化助手按 rounds/31 D5 合一 | ≈ −60 行 | 低:`parse_int_prefix` 两份逐字同构 |

### 3.2 有争议、需要你一句话拍板

| # | 项 | 我的建议 | 触发条件 |
| - | -- | -------- | -------- |
| Q1 | `kitten4_vocab.rs`(118 行)并进 `mapping.rs` | **保持独立**:它是"整体替换"的工作流锚点(knowledge §5bis、rounds/34/36、backlog 0b 都引用它),合并只省 1 个文件却要改 5 处测试路径 + 3 处文档锚点 | 只在"必须减文件"时做 |
| Q2 | `assembly.rs` 的调度器搬到 `pipeline.rs`(M-搬迁) | **做**:2644 → ≤2500 是硬规则;不新增文件、不动 `remap_*` | 与 rounds/31"合并而非拆分"不冲突(这不是按方向拆) |
| Q3 | `decompile/mod.rs` 的 8 个专用 `BlockDecompiler` impl(1356-1830)用函数指针表替掉 | **先不做**:能把"每个特殊块的差异点"藏进表里,可读性未必更好;仓库明令"不过度抽象、不加宏"。先补 Phase 0.4 的单测再评估 | 若后续 decompile 侧要频繁加块类型,再议 |
| Q4 | `tree_to_json` 两份(nemo.rs:1445 vs model.rs:1779) | **不合并**:model 那份会补 `shield:false`(官方有、我们 NEMO 产物没有)⇒ 合并 = 改字节,而 NEMO 现在**没有 SHA 门**。先做两件低风险事:nemo 那份 `filter_map(ok())` 改成进报告;`fill_shield` 显式化为 `ShieldMode` 参数 | Phase 0.5 之后 |
| Q5 | `escape_text`(nemo_mapping.rs:2098)在**属性位置**漏转 `"` | **本轮不改**:补转义 = 改产物字节,须先确认是"照抄官方"还是疏漏(bundle 锚点)+ NEMO SHA 门 | 作为**独立裁决**记录,不塞进重构 |

| Q6 ✅ **已核实(2026-09-26):不合并** | **两套批处理执行器**是否同构:`assembly::run_items`(assembly.rs:2191)与 `shared::batch_map`(shared.rs:1100) | 逐行比对后**四项语义不同**:① 装箱方式 —— `run_items` 是**加权贪心装箱**(LPT,重活优先)vs `batch_map` 是**按并发等分 chunk**;② panic —— `run_items` 让 worker panic **冒泡**(`thread::scope` 默认)vs `batch_map` 用 `on_panic` **折成调用方错误**;③ 入参所有权 —— `Vec<I>` 按值 vs `&[T]` 借用;④ 返回 —— 直接 `Vec<T>` vs `Vec<Result<R,E>>`。合并只能二选一丢功能(丢掉装箱均衡会伤大作品并行度)⇒ 违"不过度抽象"约定 | **不做**(结论已记入 §3.3) |

### 3.3 明确不做(防重开,已决/已论证)

- `mapping.rs` + `model.rs` 整体合一(4287 行,超上限 71%)、`model|assembly`(4839)、`nemo+nemo_mapping`(5244);
- `kitten4_vocab.rs` 并进 `tables_gen.rs`(生成器整文件覆盖 ⇒ 会静默删掉快照);
- 七处遍历实现强行合一(语义不同;rounds/21 §6 / rounds/31 §3.6 已判);
- **`BTreeMap` → `HashMap`**(会改产物:反例 mapping.rs:888-889 在 `for` 循环里反复覆盖 `node.next`,字典序决定胜出者);
- `RawValue` 顶层透传 / 单遍遍历合并(rounds/26 §6:透传占比 ≈0%);
- 反向(KN→Kitten4)实体级并行(rounds/25 §10:Amdahl 上限 1.9×);
- `decompile::download_resources_parallel` 并入 `shared::batch_map`(语义不同:带预过滤 + 失败重试);
- `nemo_tests.rs` 与其它测试归并(rounds/31 §2.2 已否决:要包一层 mod、`super::` 语义会变);
- 两套 options 合并(破坏公开面;`upload` vs `upload_to_account` 语义不同)。
- **两套批处理执行器**(`assembly::run_items` vs `shared::batch_map`)合并 —— 本轮已逐行核实**四项语义不同**(加权贪心装箱 / panic 折叠 vs 冒泡 / 所有权 / 返回类型),
  合并必丢功能(见 §3.2 Q6);同理 `decompile::download_resources_parallel` 也不并入(带预过滤 + 失败重试)。

---

## 4. 性能工作单(按"收益/风险"排序)

> 排序依据:§6.1 的实测分阶段占比(反向 `core` 214 ms / e2e 474 ms;正向 `core` 226 ms / e2e 516 ms)
> + 每条的**证据强度**(能指认到具体深拷贝的排在前面)。

| # | 项 | 证据(锚点) | 预期收益 | 验证 | 风险 |
| - | -- | ---------- | -------- | ---- | ---- |
| **P1** ✅ 已落地(`c55dce7`) | `translate_work` 关掉 `save_raw` | `convert/mod.rs:67-69` 用 `DecompileOptions::new()`(默认 `save_raw = true`,decompile/mod.rs:35/63)⇒ 每次白写一份与源同量级的 JSON(NEMO 两份)再被 `remove_dir_all` 删掉(:45-47) | 每次 translate_work **≈20 ms 的无产出 I/O**(实测,3.8 MB 源;见 §6.2ter)—— 收益真实但小一个量级 | 一行改动 + "staging 内无 raw" 断言 + SHA 不变 | **最低**:raw 不是产物 |
| **P2** ✅ 已落地(`df92e03`,实测 ≈2–3%) | `KnEntity.source` 不再深拷 `nekoBlockJsonList` | `assembly.rs:847` `entity.as_object().cloned()`,而装配侧读 `source` 的位置(1135-1146、1336-1373、1376-1445、1476-1515)**从不读这个键** | 反向最大的一笔分配(9.4 MB 样本 ≈10⁵ 节点) | convert_bench `core` 列 + SHA | 低:过滤式克隆或改借用 `&'a Map` |
| **P3** ✅ 已落地(`df92e03`) | `strip_unknown_blocks` 改就地消费 | assembly.rs:996 `cloned()`、999 `blocks.cloned()`、1027 `connections.cloned()`、1054/1057 逐块 `shadows` 双拷(即使无未知类型) | 同一份积木数据**3~4 次深拷 → 0** | 同上 | 低:`root.remove` 取所有权;**保持 BTreeMap 键序** ⇒ 字节不变 |
| ~~**P4**~~ ❌ **判定不做(2026-09-26,执行时重新裁决)** | `find_object_shadow` 惰性化 | `translate/mod.rs:523` **无条件**全文档扫描 + 字符串形态再做一次 `from_str`(:1080-1082);触发形态实测只 1 例(:518-521) | 正向入口省 3–10% e2e(≈ 一次 read+parse 量级) | SHA 不变 + 错误消息文本兼容(有测试引用) | **不做**:已 grep 确认错误文案无测试断言,但惰性化会把"内联对象影子作品"从**拒绝**变成**先尝试解析**(可能被接受)⇒ 改变了对外行为,而收益只是"3–10% 正向 e2e"的**推断值** ⇒ 按仓库"无数据不做 + 不悄悄改行为"的纪律不做 |
| **P5** | 逐块线性扫描 → `LazyLock` 索引 | `rc_plain`(mapping.rs:418)在**每个块**上扫 180 条且未命中走满(:730);`is_kitten_side`(:971-983)扫 367×2(表长实测:`KITTEN_TO_KN` ≈367、`KITTEN_MUTATION_TEXT` ≈180、`_SELECT` 粗计 ≈41 —— 侦察报 17,动手前精确数一次) | 正向 ~2.5M、反向 ~3.8M 次短串比较 ⇒ 换成哈希(≈ core 的 1–3%) | bench `core` 列(必须超出噪声才算) | 低:`or_insert` 保持"首命中优先" |
| **P6** | `#[serde(flatten)] extra` 手写 | model.rs:94-97 的 `extra` 被 `from_value`/`to_value` **逐节点**调用(:133/146,调用点 translate/mod.rs:428、model.rs:796/1783) | 逐节点 serde 成本 **1.5–3×** ⇒ `core` 的大头 | 先跑 `model_tests/null_tolerance_tests` + 四样本 SHA | **中高**:`extra` 键序与 null 容错必须逐字节复现;且 `extra` 有**生产消费者**——`model.rs:1184/1751`(`def.extra.insert`/`body.extra`)与 **`assembly.rs:2445`**(remint 的 `remap_object(&mut node.extra)`)必须原样可用(只换反序列化机制,field 语义不动) |
| **P7** | 正向装配解构移动 + `duplicate_ids` 借用 + `count()` 复用 | `assembly.rs:128` `entity.source.clone()`(按值收却仍克隆,与 :1110-1113 注释自相矛盾);:914-926 每节点 `to_string()`;:841/884 同一棵树数两遍 | 每实体一次深拷 + ~5k 次 String + 一趟 DFS | SHA 不变 | 低(注意 `blocks_total` 取点在映射**前**、`converted` 在编码后,语义不可互换) |
| **P8** | 告警 String 延迟构造 | 反向 5235 块产 1817 条告警(rounds/23 §2 F);产生点 13 处(mapping.rs:735/1332/1350/1427-1430…;assembly.rs:347/374/878/890/1082/1091…) | 反向 `core` 的 5–15% | 告警**逐条同序**断言(procedure_library) | 低:不改公开枚举形状 |
| **P9** | NEMO:去重复解析 + 分配 | `nemo.rs:624`/`nemo_mapping.rs:709` 用 `format!("<root>{xml}</root>")` 再解析;`has_return_blocks`(:738-742)把同一段**再包一次再解析**;含返回的条目共 3 次解析 + 1 次深拷;`text_content`(:1570)每次 2 次堆分配(万级 `<field>`);三趟 `replace_*` 递归(:641-646) | NEMO 前端解析时间**可省一半到三分之二** | **先补 NEMO SHA 门**(Phase 0.5) | 中:畸形输入下 `<root>` 包装与裸 `Parser::run` 行为不同 ⇒ 必须保持同样严格 |
| **P10** | `create_draft_work` 复用已取到的 `preview` | `convert/mod.rs:212-224` 为拿 preview **重新拉一次详情**;而反编译侧 `WorkInfo.preview` 早就有(decompile/mod.rs:538-546、shared.rs:188-192),只是 `DecompiledArtifact::Document` 没带出来 | 每次带上传的 translate_work 省一个 RTT;顺带消掉 backlog 2b 的全局客户端依赖 | 编译 + 真机上传门 | 低 |
| **P11** | `parent_id` 改 `Arc<str>` / 临时 id clone | mapping.rs:832/836/841 每子节点一次 String;translate/mod.rs:600 每铸造点 clone 一次临时 id | 万级分配,占 `core` 3–8% / <2% | SHA 不变 | 中:动 `BlockJson` 字段类型,排后面 |

**明确不做的性能项**:`BTreeMap`→`HashMap`(改字节)、`kitten4_editor_knows` 换 `HashSet`(实测 ~1.3×10⁵ 次比较,亚毫秒级)、
`to_value` 后的 `fill_shield` 与 `flatten` 改造**同批**(两个都会动字节,出问题无法归因)。

---

## 5. 阶段与验收矩阵

| 阶段 | 内容 | 出门条件(全绿才算过) | 回退 |
| ---- | ---- | -------------------- | ---- |
| **Phase 0** ✅ **已完成(2026-09-26)** | 门加固 + 基线有据重刷 + 三条协议冻结 + decompile 侧首批离线单测 | `fmt`/`clippy` 干净;`cargo test` **109 过**(104+5);`BACKEND_REQUIRE_BENCH=1 … convert_bench` **绿** | — |
| **Phase 1** ✅ **已完成(2026-09-26)** | `options.rs`(262)/`report.rs`(180)/`pipeline.rs`(1655,含测试)/`xml.rs`(1278,含测试)四个新文件 + 断两环 + 模块文档纠错 | 同上 + **四样本 SHA256 与 `#meta` 逐字节不变**(纯搬迁的硬证明);`assembly.rs` 2644 → **2024**(回到 2500 上限内) | 一次提交 `14ba14d` |
| **Phase 3** 🕓 **进行中**:P1/M5/§0.5(`c55dce7`)、**P2/P3(`df92e03`)** 已落地 | `translate_work` 关 `save_raw`;删 `FileService` 死字段;NEMO 补 `elapsed_ms` | 同上(SHA 不变) | 提交 `c55dce7` |
| **Phase 0** | §1 的 6 项(门 + 冻结协议 + 补 decompile/NEMO/translate_work 证据) | `fmt` / `clippy -D warnings` / `cargo test`(含两条扫描器) / `BACKEND_REQUIRE_BENCH=1 … convert_bench` | 逐条独立,单条 revert |
| **Phase 1** | 纯搬迁:报告层、选项、`pipeline.rs`(含调度器回迁)、`xml.rs`(断两环) | 同上 + `forward_parallel_tests`(不依赖 download/)+ `cargo test --lib` 的 46 份正向 + 10 份反向扫描器 | 每个搬迁一次提交,单独 revert |
| **Phase 2** | 样板与死重量:M1–M9 + Q1/Q2 的裁定 | 同上 + decompile 侧新单测(0.4 先补) | 同上 |
| **Phase 3** | 性能:P1→P11 每条**独立提交** | 同上 + **同轮 A/B**(绑核、5 轮取最小,记 `read/parse/core/ser/e2e`) + SHA 逐样本比对 | 单条 revert;若 SHA 变则**先解释再决定**是否接受 |
| **Phase 4** | 收尾:文档锚点失效(4 处指向已删模块)、`convert-backlog` 的 `k4raw` 漂移、`BlockJson::walk` 零调用处理、`#meta` 升级后的基线维护说明 | 文档自检(引用可达)+ 门全绿 | — |

**验收矩阵(改动类型 → 需要跑什么)**:

| 改动类型 | SHA 基线 | 全语料扫描器 | `forward_parallel_tests` | 真机门 |
| -------- | -------- | ------------ | ------------------------ | ------ |
| 纯搬迁(文件/函数位置) | 必须 | 必须 | 必须 | 可选 |
| 死重量删除 | 必须 | 必须 | 可选 | 不需要 |
| 分配/所有权重构 | 必须 | 必须 | 可选 | 不需要 |
| 涉及 id 铸造 / 键序 | 必须 + **逐字段解释** | 必须 | 必须 | 建议 |
| 涉及 NEMO 语义 | **先补 NEMO 基线** | NEMO 无扫描器 | — | 需要 harness |
| 实机能否打开 | 不覆盖 | 不覆盖 | 不覆盖 | **必须**(rounds/35 的手法) |

---

## 6. 度量(本次实测)

### 6.1 分阶段耗时(`--profile bench_perf`,本机 4 核)

| 样本 | 源 MB | read | parse | **core** | ser | e2e | 产物 MB | 块(源→产物) | 告警 |
| ---- | ----- | ---- | ----- | -------- | --- | --- | ------- | ------------ | ---- |
| kitten4-10.8MB(正向) | 10.8 | 4 | 73 | **226** | 61 | 516 | 9.7 | 12764→14024 | 209 | <!-- hygiene-allow:基准样本键,非凭据 -->
| kitten4-0.3MB(正向) | 0.3 | 0 | 2 | 8 | 2 | 22 | 0.3 | 374→439 | 0 | <!-- hygiene-allow:基准样本键,非凭据 -->
| kn-9.4MB(反向) | 9.4 | 5 | 33 | **214** | 47 | 474 | 9.8 | 5235→4624 | 1817 |
| kn-3.7MB(反向) | 3.7 | 2 | 6 | 57 | 11 | 145 | 4.0 | 1597→1597 | 508 |

读法:`core` 占 e2e 的 **44%**(正向 226/516)、**45%**(反向 214/474);`ser` ≈ 10–12%;`parse` 6–14%;
还有 **≈30% 未归类**(写盘 + 报告 + 装配 + 计划外克隆)⇒ 性能工作单里 P2/P3/P7(整份深拷)正落在这 30% 里。
实体级并发实测只拿到 **core 1.36× / e2e 1.06×**(正向),反向无并行 —— 与 rounds/25 的 Amdahl 结论一致。

### 6.2 "输出不变"这门现在的状态(**红**)

```
[convert_bench] 产物与基线不一致(性能改了但产物变了 = 失败):
  kn-9.4MB-kitten4     基线 e7680dcc…  现在 97053e47…
  kn-3.7MB-kitten4     基线 bfde1fc1…  现在 eb29bd52…
```

- 不一致的**只有两个反向样本**;两个正向样本 SHA 一致 ⇒ 与 rounds/34–36"只改反向输出"完全吻合;
- 但基线文件(`tests/fixtures/translate/convert_bench_baseline.json`)**没有同步** ⇒ 结论:
  **基线是历史遗留的红,不是新缺陷**;它必须在 Phase 0 有据地重刷(§1 0.1),否则后面分不清因果;
- 附带发现:`#meta` 只记录不断言、基线缺失会被静默重建、CI 上这条门静默跳过(`download/` 被 gitignore)——
  这三条都在 Phase 0 一并堵掉。

### 6.2bis 同轮 A/B 实测(P2/P3,2026-09-26)

方法:同一台机器、同一时段,**`git worktree` 双树交替跑**(改动前 `14ba14d` ↔ 改动后),
每侧两轮、每样本取**最小值**(规避跨轮漂移;`convert_bench` 的绝对毫秒会漂 20–40%)。

| 样本 | 侧 | core ms | e2e ms | 产物 SHA256 |
| ---- | -- | ------- | ------ | ----------- |
| kitten4-10.8MB(正向,不受 P2/P3 影响) | 前 | 212 | 673 | 与后一致 ✓ | <!-- hygiene-allow:基准样本键,非凭据 -->
| 同上 | 后 | 245 | 685 | — |
| kn-9.4MB(反向,**P2/P3 的目标**) | 前 | 227 | 481 | 与后一致 ✓ |
| 同上 | 后 | **221** | **474** | — |
| kn-3.7MB(反向) | 前 | 64 | 143 | 与后一致 ✓ |
| 同上 | 后 | **62** | 143 | — |

**结论(诚实)**:
1. **P2/P3 的收益只有 ≈2–3%(反向 `core` 227→221、`e2e` 481→474),落在本机噪声内** ⇒
   方案把它们排在"收益最大"是**估错了** —— 那几笔深拷贝在 9.4 MB 文档上只值几毫秒;
2. 正向样本"变慢"(212→245)是同轮噪声的证明:它**不经过** P2/P3 改的代码路径 ⇒ 该差值只能来自机器波动;
3. **真正贵的是"逐块"工作**:反向 5235 块花 221 ms(**≈42 µs/块**),正向 12764 块花 212–245 ms(**≈17 µs/块**)
   ⇒ 反向每块贵 2.5×。⇒ **队列要改**:把 P6(`#[serde(flatten)]` 逐节点 serde)、P5(逐块线性扫描)、
   P8(告警 String)提到前面;P7/P9–P11 次之;已被否的 P4 不回头。

### 6.2ter P1 的量,与"方案的收益估计普遍偏乐观"这个教训

`translate_work` 的端到端实测**不可用**:那条路上抓取要 3~8 s,抖动比本地开销大一个量级
(实测两轮:前 6901 ms / 后 6463 ms,但第二轮后侧 15.8 s ⇒ 纯网络噪声),而且**产物 SHA 前后完全一致**
(`d6978f5b…` ⇒ P1 确实不动产物 ✓)。于是改成**直接量 P1 消掉的那件事**(本地探针 `raw_write_cost_probe`):

| 项 | 方案里的估计 | **实测** | 结论 |
| -- | ------------ | -------- | ---- |
| P1 关 `save_raw` | "数百 ms~1 s / 次" | **20 ms**(3.8 MB 源,release;一次 `to_string` + 写盘;NEMO 两份 ⇒ ≈两倍) | 真实但是**纯浪费的 I/O**,墙钟收益**小一个数量级** |
| P2/P3 深拷消除 | "反向最大的一笔分配" | **≈2–3%**(§6.2bis,且落在噪声内) | 收益可忽略 |
| §0.5 / M5 | 观测与死代码 | 无耗时影响 | 判断正确 |

**教训(写进方法)**:方案里凡是没有**同轮 A/B 或本地探针**支撑的收益估计,一律按"待量"处理 ——
本轮两条"看似最大"的项都低估/高估了一个量级,根因是估计来自 **debug 口径或纯推断**。
⇒ 从现在起:**先量后改**(队列按 §6.1/§6.2ter 的实测重排,而不是按直觉排序)。

**下一步该量什么**(据 §6.2bis 的每块成本):反向 `core` **≈42 µs/块**(5235 块 / 221 ms)、
正向 **≈17 µs/块**(12764 块 / 212–245 ms)⇒ 嫌疑集中在**逐块路径**:
`#[serde(flatten)]` 的逐节点 serde(P6)、逐块线性扫描(P5)、告警 String(P8)。
在动它们之前,先各加一条**本地探针**(像 `raw_write_cost_probe` 那样,脱离网络与噪声)。

### 6.3 证据强弱纪律

- SHA 是**绊线**(能证"变了"),不是**等价证明**(不能证"语义没变")⇒ 每次 SHA 变化必须给逐字段解释;
- 没有分配/内存门 ⇒ 凡"减少 clone"的优化,一律**必须**用同轮 A/B 的 `core`/`e2e` 数字作证(单轮绝对毫秒会漂 20–40%);
- 没有性能回归门 ⇒ 人工取"5 轮最小、同机同档";Phase 3 可顺手加一条粗门(core/e2e 超基线 1.5× 才失败)以减少人眼负担。

---

## 7. 风险与对策

| 风险 | 对策 |
| ---- | ---- |
| 重构期间基线红 ⇒ 无法判因 | Phase 0 先修门;**Phase 3 每条性能改动单独提交** |
| 两个"会动字节"的改动同批(如 `flatten` + `fill_shield`) | 明确排成两条独立提交,分两次归因 |
| 公开面被无意改动(rounds/31 §3) | 只搬迁 + `pub use` 再导出;`tests/convert_live.rs`/`convert_bench.rs` 用的是公开路径 ⇒ 编译即证 |
| 与 rounds/31 的"不合并"清单冲突 | §3.3 逐条列出"不重开";Q2/Q3 两条有争议项单列等你拍板 |
| NEMO 无门却被"顺手"改动 | Phase 0.5 补门之前**不碰 NEMO 语义**(只允许 §4 Q4/P9 的非语义部分) |
| 语料目录可变(`download/` 由采集器写入) | STRIP_BUDGET/PROCEDURE_LIBRARIES 按**文件名**索引 ⇒ 新语料落"表内最大值"兜底;方案要求新样本同时补表 |

---

## 8. 自检记录(方案结论的逐条复核)

**复核方式**:本轮在代码里逐条对照(不依赖侦察报告),命令 = grep/读锚点;`⚖️` 标记的是"未能证实、只能推断"的项。

| 方案结论 | 复核证据 | 结果 |
| -------- | -------- | ---- |
| `save_raw` 默认 true,且 `translate_work` 没关它 | `decompile/mod.rs:35/63/476`、`convert/mod.rs:82`(无 `.save_raw(false)`)、`:48` 写完即 `remove_dir_all` | ✅ 成立(P1 有效) |
| 反向 `KnEntity.source` 连 `nekoBlockJsonList` 一起深拷,装配侧不读它 | `assembly.rs:847` + 全仓 `nekoBlockJsonList` 读点(只 `:840` 的 parse 与测试) | ✅ 成立(P2 有效) |
| `strip_unknown_blocks` 3~4 次深拷 | `assembly.rs:996/999/1027` 三处 `cloned()` + `:1054` 逐块 `shadows` 克隆 | ✅ 成立(P3 有效) |
| 正向入口**无条件**扫全文档 + 二次 parse | `translate/mod.rs:523`(调用即条件)+ 实现 `:1075`;罕见形态实测只 1 例 | ✅ 成立(P4 有效) |
| 逐块线性扫描 | `mapping.rs:418`(`rc_plain`)、`:730`(逐块两扫)、`is_kitten_side:971-983`(367×2) | ✅ 成立(P5 有效) |
| `#[serde(flatten)] extra` 逐节点生效 | `model.rs:96-97`;调用点 `:133/146` | ✅ 成立;**并新增风险**:`extra` 有生产消费者(`model.rs:1184/1751`、`assembly.rs:2445` remint 的 `remap_object`) |
| `FileService.config` 全仓零读取 | grep `.file_service`:只有 `editors.rs:835/1480` 把字段**转发**给别人,无人读 `.config` | ✅ 成立(M5 有效) |
| `decompile/` 零内联测试 | grep `#[cfg(test)]`:`decompile/mod.rs`、`editors.rs` 均**无** | ✅ 成立(Phase 0.4 必要) |
| `TranslateOptions` 有 9 个 `pub(crate)` 读取器 | 逐行数 `translate/mod.rs:187/191/195/199/203/207/211/240/245` | ✅ 成立(恰好 9 个) |
| 报告层被兄弟模块向上依赖 | grep `use super::{TranslateReport`:mapping.rs:52、model.rs:2、nemo.rs:3-4、nemo_mapping.rs:1-2(assembly 经另一路径) | ✅ 成立 |
| `TOP_BLOCKS`/`KN_TYPES` 零生产调用 | 全仓 grep:只有生成器 + 定义 + 2 条注释 | ✅ 成立(注释锚点实测是 `mapping.rs:916`,侦察给的 909 已修正) |
| `assembly.rs` 有生产代码排在测试之后 | `assembly_tests` 1678-2168、生产 2169-2534、`remint_tests` 2536 | ✅ 成立(⇒ 并修正了"搬到 ≤2500"的算术) |
| `kitten4_editor_knows` 不在热点(≈1.3×10⁵ 次比较) | 调用点量级估算 | ⚖️ **推断**,无 profile ⇒ 方案里已按"无数据不做"处理 |
| P5 的收益(1~3% core) | 无 profile | ⚖️ **推断** ⇒ 必须靠同轮 A/B 的数 |
| P9 的收益(NEMO 省 1/2~2/3 解析) | 无 profile(NEMO 连 `elapsed_ms` 都没有) | ⚖️ **推断** ⇒ Phase 0.5 补仪器后再量 |
| `KITTEN_MUTATION_TEXT_SELECT` 表长 | 粗计 ≈41 vs 侦察报 17,**不一致** | ⚖️ 未定 ⇒ 动手前精算(不影响方向) |
| `BlockJson::walk` 零生产调用 | grep 只命中**同名局部函数**,未精确区分方法 | ⚖️ 未定 ⇒ Phase 4 用 dead_code 复核 |
| "NEMO 方向没有离线门" | 读 `nemo_tests.rs` 的 `#[test]` 列表 | ❌ **方案原措辞过强** ⇒ 已更正:有 6 条默认跑的内存单测;缺的是 SHA 基线 + 真文件路径门 + `elapsed_ms` |
| Q6 两套批处理执行器是否同构 | 逐行读 `assembly.rs:2191-2250` 与 `shared.rs:1100-1131` | ✅ **不同构**(装箱/panic/所有权/返回四点都不同)⇒ 结论"不合并" |

**本轮复核改正的三处**:

1. **`assembly.rs` 的算术错了**:只搬并行机(≈97 行)⇒ 2547,仍超 2500 软上限;要回到上限内必须**连 remint 段(≈256 行)一起搬** ⇒ 已改正,并把"重开 rounds/31 remint 合并决策"单列为 **Q2**;
2. **文件数错了**:现 `convert/` 共 **14** 个 `.rs`(含 2 个测试文件),改后 **18**(原写"13→14");
3. 锚点/口径:注释锚点 `mapping.rs:909` → **916**;`_SELECT` 表长待精算;`extra` 的消费者(assembly.rs:2445)补进 P6 风险。

## 9. 待你拍板的三点(Q6 已核实并关闭)

1. **是否按本方案执行**(Phase 0 → 1 → 2 → 3 → 4),还是只做其中某几段(例如"只要性能,不要搬迁")?
2. **Q2**:`assembly.rs` 的调度器搬到 `pipeline.rs`(让文件回到 2500 行上限内)—— 做 or 不做?
3. **Q1**:`kitten4_vocab.rs` 是否并进 `mapping.rs`(少一个文件 vs 丢掉"整体替换"工作流锚点)—— 我的建议是保持独立。

---

## 10. 执行实况(2026-09-26 本轮)

| 阶段 | 状态 | 提交 | 硬证据 |
| ---- | ---- | ---- | ------ |
| Phase 0(门 + 协议 + decompile 测试) | ✅ 完成 | `ae41361` | 严格模式 bench 绿;`cargo test` 104 → **109**;基线重刷**逐键可审计**(正向只多 `source_sha256`;反向 SHA 与产物字节变化 = 34–36 轮刻意改动) |
| Phase 1(options/report/pipeline/xml) | ✅ 完成 | `14ba14d` | 四个新文件 + 断 `model⇄mapping`、`nemo⇄nemo_mapping` 两环;`assembly.rs` **2644 → 2024**;四样本 **SHA 逐字节不变** |
| Phase 3 第一批(P1/M5/NEMO 计时) | ✅ 完成 | `c55dce7` | 同上(SHA 不变) |

**已按判断确定的项**:Q1 = `kitten4_vocab.rs` **保持独立**;Q2 = remint **随管线搬入 `pipeline.rs`**(已执行,文件回到上限内);Q6 = 两套批处理执行器**不合并**(已核实不同构)。

**仍在队列(按方案 §4 的优先级)**:

1. **P2/P3**(反向装配的整份深拷:`KnEntity.source` 带 `nekoBlockJsonList`、`strip_unknown_blocks` 3~4 次 `cloned()`)—— 收益最大、风险低(纯所有权重构),但需要一次完整验证周期;
2. **P4**(正向入口 `find_object_shadow` 无条件全文档扫描 + 二次 parse)、**P7**(正向装配解构移动 + `duplicate_ids` 借用 + `count()` 复用);
3. **Phase 2** 的样板与死数据:M1–M4(`editors.rs` 的 5 份 Fetcher 壳/3 份 `save_result`/5 份变体错误分支)、M7(`TOP_BLOCKS`/`KN_TYPES` 死数据 + 生成器)、M5 残项(三个纯转发的 `file_service` 字段);
4. **P5/P6/P8–P11**(逐块线性扫描、`#[serde(flatten)]`、告警 String、`Arc<str>`、临时 id clone);
5. **Phase 4** 收尾(文档锚点、backlog 的 `k4raw` 漂移、`BlockJson::walk`);
6. **补一条 `translate_work` 端到端基准** —— 没有它,P1 这类改动的收益只有代码论证、给不出同轮 A/B 数字(方案 §0.5 ③)。
