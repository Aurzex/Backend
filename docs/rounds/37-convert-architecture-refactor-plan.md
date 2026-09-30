# 第三十七轮方案 — convert 域重构:架构归位 + 适当合并 + 性能(约束:产物逐字节不变)

> 日期 2026-09-26 · 只读调研(5 路并行侦察 + 实机基准)后的**方案**,本轮不改代码。
> 事实依据:`docs/knowledge/convert-semantics.md`、`docs/rounds/31`(上一轮编排合并与"不合并"清单)、
> `docs/rounds/36`(最近一轮)、`docs/goals/convert-backlog.md`;**代码锚点全部为本次复核的现行行号**。

## 0. 结论摘要

| 判断 | 依据(要点) |
| ---- | ---------- |
| **分层是干净的,不需要"拆架构"** | `convert/mod.rs`(257 行)= 门面 + 跨子域编排 + 上传编排;`shared.rs` 不对子域反向依赖;`translate/` 与 `decompile/` 互不依赖(规则写在 `convert/mod.rs:5-13`) |
| **真正的架构问题是"职责错位 + 两处真环"** | ① `translate/mod.rs` 一个文件装了**五件事**(选项/错误、报告层、正/反管线、入口、诊断),导致 5 个兄弟模块 `use super::{TranslateReport,…}` 的**向上依赖**(mapping.rs:52、model.rs:2、assembly.rs:6-8、nemo.rs:3-4、nemo_mapping.rs:1-2);② `model ⇄ mapping` 环(model.rs:1 ↔ mapping.rs:46-47);③ `nemo.rs ⇄ nemo_mapping.rs` 环(nemo.rs:6-11 ↔ nemo_mapping.rs:3-4) |
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
| 0.5 | **给 NEMO 方向补门** | NEMO 既无 SHA 基线也无默认扫描器,唯一强门靠不入库的 `temp/harness` + 2 份夹具(`nemo_tests.rs:317-337`,默认 ignore) | ①把现成 NEMO 样本加进 `convert_bench` 的 `SAMPLES`;②给 NEMO 分支补 `report.elapsed_ms`(现在**恒 0**,translate/mod.rs:712-719) |
| 0.6 | **确认产物链无 `HashMap` 迭代** | 有的话 SHA 会随机红 | 本次静态排查未发现(产物键序由 `serde_json::Map`=BTreeMap 保证,Cargo 未开 `preserve_order`);Phase 1 之后再用扫描器复验一次 |

**Phase 0 出门条件**:`fmt/clippy/test` 全绿 + `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored` **全绿**。

---

## 2. 架构目标(`translate/` 的模块图)

### 2.1 改前 → 改后

```
改前(13 文件,translate/*.rs 生产 ≈ 9.3k 行)          改后(14 文件,行数全部回到 ≤2500)
mod.rs        2192  ← 选项/错误 + 报告 + 正反管线 + 入口 + 诊断    mod.rs        ≈ 460  仅保留:模块声明 + 入口/门面(translate_value/file、源引用、detect_editor)
assembly.rs   2644  ← 装配器 + 反向管线 + 并行机 + remint + 测试    options.rs    ≈ 255  公开面(TargetEditor/StageOrientation/TranslateOptions/Outcome/Error)
mapping.rs    2092                                                  report.rs     ≈ 200  横切(TranslateWarning/TranslateReport)
model.rs      2195                                                  pipeline.rs   ≈ 720  正/反文档级编排 + 并行机 + 临时 id 改写(remap_*)
nemo.rs       2575                                                  mapping.rs    ≈ 1990 语义表 + 双向映射
nemo_mapping  2669                                                  model.rs      ≈ 2100 中核模型 + id + 编解码 + 程序集
assembly.rs …                                                       assembly.rs   ≈ 2200 装配器(正/反实体与资源字典)
                                                                    xml.rs        ≈ 950  XML DOM + 字符串手术 + 影子渲染/转义(断两个环)
                                                                    nemo.rs       ≈ 1180 NEMO 管线 + 版本迁移 + 前置改写
                                                                    nemo_mapping  ≈ 2580 NEMO 解析 + 映射 + 表
```

### 2.2 每次搬迁:代价与验证

| 搬迁 | 收益 | 代价(机械但必须一次做完) | 验证 |
| ---- | ---- | ------------------------ | ---- |
| 报告层 → `report.rs` | 消掉 5 条向上依赖;报告成为可独立引用的横切面 | 5 个兄弟文件的 `use super::{TranslateReport, TranslateWarning}` 改 `use super::report::…` | 编译 + `cargo test --lib` + 扫描器 |
| 选项/错误 → `options.rs` | mod.rs 从"什么都装"变回门面;与 `DecompileOptions` 同层同义 | `mod.rs` 必须 `pub use options::{…}` 再导出(**公开路径不变**) | `tests/convert_live.rs`、`convert_bench.rs` 用的就是公开路径 ⇒ 编译即证 |
| 正/反管线 + 并行机 → `pipeline.rs` | **本次唯一"真职责重叠"的修复**:正/反编排同层可对照;`assembly.rs` 恢复单一职责且测试回到末尾 | `assembly.rs` 内 9 处引用(`assembly::workers/run_items/IdRemap/remap_*/merge_report`,translate/mod.rs:531/546/568/622/595/616/623/649/652)+ 2 条 doc 注释 | 编译 + `forward_parallel_tests`(不依赖 download/)+ `convert_bench` 1 vs 8 同 SHA |
| XML 层 → `xml.rs`(两处环) | 断 `model ⇄ mapping` 与 `nemo ⇄ nemo_mapping`;**这是分层修复,不是省行数**(净增 1 文件) | `mapping.rs` 的字符串手术(467-582)+ 影子构造(587/667)+ `nemo.rs:1452-2204` DOM 与 `:2205-2575` XML 单测整体搬;`model.rs:1071` 的 `VALUE_SHADOW_XML` 是**逐字节照搬官方**形态(含换行缩进)⇒ 不得与 `math_number_shadow` 统一格式化 | 逐字节比 5 处影子模板产物 + `nemo_tests` 的 `value_slot_keeps_shadow_xml_and_override_block` |
| 通用调度器 → `pipeline.rs` | `assembly.rs` 2644 → ≤2500(回到仓库上限内);调度是管线原语,放装配文件里职责不顺 | `workers`(2179)/`run_items`(2191)+ 3 条调度用例;`remap_*`(**留在 assembly**,守住 rounds/31"remint 并入 assembly"的结论) | 同上 |

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
| M7 | `tables_gen.rs` 删 `TOP_BLOCKS`(1203-1213)+ `KN_TYPES`(1216-1426),**并同步改生成器** `src/bin/gen_translate_tables.rs` | ≈ −234 行死数据 | 低:零生产调用点;必须改生成器否则下次重跑复活;顺带改写 2 条提到 `KN_TYPES` 的注释(translate/mod.rs:33、mapping.rs:909) |
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

---

## 4. 性能工作单(按"收益/风险"排序)

> 排序依据:§6.1 的实测分阶段占比(反向 `core` 214 ms / e2e 474 ms;正向 `core` 226 ms / e2e 516 ms)
> + 每条的**证据强度**(能指认到具体深拷贝的排在前面)。

| # | 项 | 证据(锚点) | 预期收益 | 验证 | 风险 |
| - | -- | ---------- | -------- | ---- | ---- |
| **P1** | `translate_work` 关掉 `save_raw` | `convert/mod.rs:67-69` 用 `DecompileOptions::new()`(默认 `save_raw = true`,decompile/mod.rs:35/63)⇒ 每次白写一份与源同量级的 JSON(NEMO 两份)再被 `remove_dir_all` 删掉(:45-47) | 每次 translate_work **数百 ms~1 s** 的无产出开销 | 一行改动 + "staging 内无 raw" 断言 + SHA 不变 | **最低**:raw 不是产物 |
| **P2** | `KnEntity.source` 不再深拷 `nekoBlockJsonList` | `assembly.rs:847` `entity.as_object().cloned()`,而装配侧读 `source` 的位置(1135-1146、1336-1373、1376-1445、1476-1515)**从不读这个键** | 反向最大的一笔分配(9.4 MB 样本 ≈10⁵ 节点) | convert_bench `core` 列 + SHA | 低:过滤式克隆或改借用 `&'a Map` |
| **P3** | `strip_unknown_blocks` 改就地消费 | assembly.rs:996 `cloned()`、999 `blocks.cloned()`、1027 `connections.cloned()`、1054/1057 逐块 `shadows` 双拷(即使无未知类型) | 同一份积木数据**3~4 次深拷 → 0** | 同上 | 低:`root.remove` 取所有权;**保持 BTreeMap 键序** ⇒ 字节不变 |
| **P4** | `find_object_shadow` 惰性化 | `translate/mod.rs:523` **无条件**全文档扫描 + 字符串形态再做一次 `from_str`(:1080-1082);触发形态实测只 1 例(:518-521) | 正向入口省 3–10% e2e(≈ 一次 read+parse 量级) | SHA 不变 + 错误消息文本兼容(有测试引用) | 中:错误**触发时机**变化,需 grep 引用该文本的断言 |
| **P5** | 逐块线性扫描 → `LazyLock` 索引 | `rc_plain`(mapping.rs:418)在**每个块**上扫 180 条且未命中走满(:730);`is_kitten_side`(:971-983)扫 367×2 | 正向 ~2.5M、反向 ~3.8M 次短串比较 ⇒ 换成哈希(≈ core 的 1–3%) | bench `core` 列(必须超出噪声才算) | 低:`or_insert` 保持"首命中优先" |
| **P6** | `#[serde(flatten)] extra` 手写 | model.rs:94-97 的 `extra` 被 `from_value`/`to_value` **逐节点**调用(:133/146,调用点 translate/mod.rs:428、model.rs:796/1783) | 逐节点 serde 成本 **1.5–3×** ⇒ `core` 的大头 | 先跑 `model_tests/null_tolerance_tests` + 四样本 SHA | **中高**:`extra` 键序与 null 容错必须逐字节复现 |
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

## 8. 待你拍板的三点

1. **是否按本方案执行**(Phase 0 → 1 → 2 → 3 → 4),还是只做其中某几段(例如"只要性能,不要搬迁")?
2. **Q2**:`assembly.rs` 的调度器搬到 `pipeline.rs`(让文件回到 2500 行上限内)—— 做 or 不做?
3. **Q1**:`kitten4_vocab.rs` 是否并进 `mapping.rs`(少一个文件 vs 丢掉"整体替换"工作流锚点)—— 我的建议是保持独立。
