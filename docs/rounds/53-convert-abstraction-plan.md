# 第五十三轮记录 — 转换域抽象与职责提取方案(先记录,未动代码)

## 0. 一句话
按 2026-10-07 的授权("可以引入适当的抽象"),把转换域**可做的抽象与职责提取**逐项写成方案,每项附现状读数、做法、收益判据、风险与验收门;本轮**不改一行代码**。

## 1. 范围与门槛

- **授权与边界**:允许引入**适当**抽象;仍受 `../knowledge/repo-conventions.md` §2/§5 约束 —— 不引宏、不引新依赖、一个文件一个职责、测试放文件末尾、删除/合并需零调用点证据。
- **动代码前的三条门槛**(逐项自检,写入提交信息):
  1. **不是"为对称而抽"**:能说出具名收益(编译期约束、单一实现、可回归),而不是"两边看起来一样";
  2. **有量化收益或风险降低**:纯搬迁/提取的收益也要能说清("少一处重复实现""门只此一份"),否则不动;
  3. **不改产物字节**:纯搬迁与提取必须做到产物 SHA 与分配计数**逐位不变**。
- **统一验收门**:产物 SHA256 + `#meta` 全绿;改动只搬代码时分配计数逐位不变;`cargo fmt --check`、`clippy --all-targets -- -D warnings`、离线测试(`--lib` / `repo_hygiene` / `convert_bench`);涉及 NEMO 装配的改动另跑 `nemo_tests` 的字节等价门。

## 2. 现状底账(2026-10-07 实测)

| 文件 | 总行 | 本体 | 内联测试 | 规则对照(`repo-conventions` §5) |
| --- | --- | --- | --- | --- |
| `translate/model.rs` | 3154 | ≈2285 | 869(7 个测试模块) | 本体在 ≈2500 软上限内;**总行 >3000 而测试内联 ⇒ 应外移为 `model_tests.rs`**;且 `mod streamed_writer_tests` 夹在代码中间 ⇒ 违反"测试放文件末尾" |
| `translate/assembly.rs` | 2678 | 1978 | 700(1 个模块) | 合规 |
| `translate/nemo_mapping.rs` | 2618 | 2618 | — | 超软上限,**已按 C2 在文件头记账**(不拆) |
| `translate/mapping.rs` | 2018 | 1689 | 329 | 合规 |
| `translate/pipeline.rs` | 2281 | 1252 | 1029 | 合规 |
| `translate/nemo.rs` | 1659 | 1659 | — | 合规;**但含域内最长函数 `assemble_nemo` = 657 行** |

界定口径:本体 = 总行 − 内联测试模块行数;测试块按"`#[cfg(test)]` + 紧随 `mod X {` + 顶格 `}` 收尾"识别;只标 `#[cfg(test)]` 的函数/use 单独计 2 处(`model.rs`)。

## 3. 候选(逐项)

### A. `translate/model.rs` 的职责与测试分区(**组织模式,不引入新抽象**)

- 现状:本体 ≈2285 行里混了四类职责 —— ① 树模型与类型(`BlockJson`/`BlockTree`/`type_name`/`MintKind`);② id 机制(`IdSource`/临时 id);③ **两套**解析(值形态 `parse_block_data_json`/`parse_parts`/`build_node` 与 typed 形态 `parse_block_data_json_typed`/`parse_parts_typed`/`build_node_typed`);④ 编码与写出(`encode_*`/`EncodedBlocks`/`write_encoded_blocks`/`write_kitten4_block`/`KITTEN4_DEFAULTS`);⑤ 程序集(`split_procedures`/`rewrite_calls`/`rewrite_call`/`parse_kn_procedures*`/`call_targets`/`unrewrite_calls`/`def_root_from_entry`/`procedure_entries`/`procedures_to_json`)。
- **Step A1(零风险,先做)**:测试外移 + 归位 —— 把 7 个测试模块并入 `translate/model_tests.rs`(§5 已要求,`reverse_tests.rs`/`nemo_tests.rs` 是同类先例)。判据:纯搬迁,产物与分配逐位不变。
- **Step A2(可选)**:按上述五类在 `translate/model/` 下拆 3 个文件(解析 / 编码写出 / 程序集),中核(树模型 + id)留 `model.rs`;每步单独提交。
- 收益判据:A1 是**回归规则**(undo W12b 的回退,让"测试在哪"重新可预期);A2 的收益是"改一个方向不必读另外三类",属可读性收益,若 A1 完成后仍认为不值,可只做 A1 + 文件头分区注释。

### B. `nemo::assemble_nemo`(657 行)→ 提取**装配阶段对象**

- 现成先例(本仓已接受的形态,不引入 trait):`assembly::ProductDocument` / `assembly::Kitten4ProductDocument` —— 结构持状态 + 方法驱阶段。
- 做法:`pub(super) struct NemoAssembly<'a> { source, options, report, placements, .. }`,把现有函数体按 `rounds/49` §3 已写明的三分显式化:`register_block_tables()`(块表登记 / `put_block_table`)、`normalize()`(`normalize_tree_numbers`)、`finish()`(`finish_document`);两条薄包装(`convert_nemo_document` / `convert_nemo_document_product`,各 12 行)保持不变。
- 收益判据:装配阶段**顺序**成为代码形状(现在藏在 657 行的线性体里);改动面只在 `nemo.rs` 内部,公开面不动。
- 风险与门:NEMO 内存/流式两条路径共用这份装配 ⇒ 必须过 `nemo_tests::nemo_product_path_is_byte_identical_to_value_path`、`nemo_tests::tree_normalization_covers_every_value_field`、`file_path_tests::nemo_source_uses_streaming_product_and_matches_memory_path` 与 `convert_bench` 的 NEMO 两样本。
- 工作量:半天级(纯提取,无行为改动)。

### C. 形状/口径抽象的**推广规则**(写进知识库,不是代码)

- 已有两例:`source::SourceShape`(编译期挂点表 `const TARGETS` / `const LEAF` + `PhantomData`,一套三层 `Visitor` 服务两个文档形状)、`model::ShieldPolicy`(策略枚举,让 NEMO 复用正向 `assembly::ProductDocument` 而不写第三套写出器)。
- 规则建议(正式写进 `repo-conventions` §5):**仅当两条路径"同深"、差异只是键名或某个补键口径时**,用"编译期挂点表 / 策略枚举"参数化;形态不同(如反向邻接表 `{blocks, connections}` vs 正向数组)则各写一份 —— 这是 `rounds/47`/`50` 的既有实践,现只是把它写成条文。
- **待量(不先做)**:把**写出侧**也参数化以合并 `ProductDocument` / `Kitten4ProductDocument`。事实:`assembly.rs` 目前**没有任何 trait 声明**,两个结构在挂点数(3 处 vs 2 处)与块表形态上不同;要抽先量收益,否则违反门槛①。

### D. 测试侧共用件:`tests/common/`

- 事实:`sha256_hex` 在 `tests/convert_bench.rs` / `convert_facade_bench.rs` / `tests/convert_work_bench.rs` **各有一份**(3 份);基线的读、写与 `BACKEND_BENCH_REFRESH` 判定只在 `convert_bench.rs`。
- 做法:新增 `tests/common/mod.rs`(或 `tests/common/bench_support.rs`),提供 `sha256_hex`、基线读写与 REFRESH 判定,三个基准 `mod common;` 引入。
- 收益判据:纯工具、**无口径差异**(区别于被判不做的"测试里 6 份 `fn walk` 合并" —— 那些口径不同,局部性 > DRY);同时把"基线损坏/缺失一律失败、只有 REFRESH 才写"的口径收敛到一处。

### E. 类型层:`WorkId` 试点的收益量法(B7 的前置)

- 做法:只在反编译 → 转换链(域内)把 `work_id` 换成 newtype `WorkId`;**不动 Manager 公共签名**(全量推广是 B7,仍待拍板)。
- 判据:能数出"被编译期拦下的误传 / 被删掉的调用点校验"几处;若试点内一处也没有,则 B7 的推广理由不成立(如实记结论)。
- 收益:编译期约束是本仓唯一未判死的抽象方向;代价是调用点改造面,故先试点。

### F. 分层归属:`translate_work` 的"按骨架读"(`convert-backlog.md` 前言「覆盖面缺口」)

- 决策点:给通用层 `utils/requests` 加"骨架读"能力 vs 转换域自留解析。
- 判据:通用层不能只服务转换域 ⇒ 先量收益(该 GET 的响应体量级)与接口面(通用层的读法抽象会不会外溢成公共面),再决定。**本轮只登记,不推进。**

## 4. 判不做(本轮复核过,不重开)

宏化(`impl_api_manager!` 类)、god file 拆分、类型级 WS 状态机、公开 `HttpClient` trait、`read_to_string` → `from_reader`、`BTreeMap` → `HashMap`、七处遍历合一、两份 `tree_to_json` 合并、`Fetcher::new` 壳 / 测试 `fn walk` 合并、`download_resources_parallel` 并入 `batch_map`、两套 options 合并、`mapping+model` 合一、`nemo_mapping` 表拆出(C2)、`decompile_work` 拆两入口(C7)、`cloudvar.rs` 深拆、`compiler.rs` 切子模块、手写序列化器(`rounds/39` §W9 评审禁用)、`find_object_shadow` 惰性化。判据与出处见 `../rounds/37` §3.3、`../rounds/39` §4、`../goals/convert-backlog.md` §3、`../goals/pending-decisions.md`。

## 5. 建议顺序

1. **A1**(测试外移 + 归位):半小时级,纯搬迁,过字节门;
2. **D**(测试共用件):纯测试侧,不影响产物;
3. **B**(装配阶段对象):有 NEMO 三条字节等价门兜底;
4. **A2**(model.rs 拆文件):视 A1 后的观感决定,可只做文件头分区注释;
5. **E**(`WorkId` 试点):先量收益再谈 B7;
6. **F**:先量后决。

每步**单独提交**,每步在提交信息里写清"过了哪条门槛、用哪个门验的"。

## 6. 未决(需评审/拍板)

- A2 拆到几个文件(3 个还是仅做注释分区);
- C 的"待量"项:是否给写出侧引入 trait(现无 trait,先量);
- C 的规则是否写进 `repo-conventions` §5(建议写);
- E 的试点范围(只 `WorkId`,还是连带 `UserId`)。

## 依据
- 授权:2026-10-07 的用户指令("可以引入适当的抽象";先记录方案、先不改代码)。
- 现状读数:2026-10-07 实测(`wc -l`;测试块按"`#[cfg(test)]` + `mod` + 顶格 `}`"界定;最长函数按花括号配平)。据此纠正了当日先前一处误记(`model.rs` 的本体**未超**软上限,真问题是"总行 >3000 而测试内联 + 测试模块夹在代码中间"),勘误落在 `../knowledge/errata.md`「转换域体量与拆分记录的两处漂移(2026-10-07)」。
- 纪律与先例:`../knowledge/repo-conventions.md` §2/§5;`../rounds/31`(一个文件一个职责)、`../rounds/37` §3.3、`../rounds/39` §0.3/§4/§5(W9 禁用与"收益是一处定义")、`../rounds/47`(共用一个变换是硬要求)、`../rounds/49` §3/§5/§6(三分与三条守门)、`../rounds/50`(`SourceShape` 参数化)。
- 待办落点:`../goals/convert-backlog.md` §2 第 15 条;`../goals/pending-decisions.md` B7。
