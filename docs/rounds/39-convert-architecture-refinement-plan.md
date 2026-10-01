# 第三十九轮方案 — convert 域架构精进(职责错位收口 + 门/仪器补洞 + 效果与性能的可证化)

> 日期 2026-10-01 · **只读调研 + 独立评审** 后的方案;**本轮不改 `src/` 与 `tests/`**,只出文档。
>
> 范围:`src/core/convert/**`(作品文件转换 `translate/` 与反编译 `decompile/`),含 convert 与 `core`/`api` 的边界。
> 依据:`docs/knowledge/{convert-semantics,convert-performance,repo-conventions,work-file-formats}.md`、
> `docs/rounds/{25,26,29,30,31,33,34,35,36,37,38}`、`docs/goals/{convert-backlog,pending-decisions}.md`、`CONTRIBUTING.md`。
> 承接:`docs/rounds/37`(架构归位与性能实测)、`docs/rounds/38`(id 口径与「未收录积木」)。**不重开**两文已决的条目(清单见 §4)。
>
> **锚点口径**:本文所有行号是 **2026-10-01 实读快照**。`src/core/convert/translate/{mapping,model,xml}.rs`
> 正在被并行修改(本方案定稿时 `model.rs` 已由 2221 → 2237 行)⇒ 落地前**按行号复核一次**;
> 老轮次里的路径/类型名漂移一律以 `docs/knowledge/errata.md` 为准。

---

## 0. 结论摘要

**总评(评审与作者一致)**:rounds/37 之后域内**大结构是健康的**(`mod.rs` 门面 → `decompile`/`translate` 两子域 → `shared` 地基,子域互不依赖)。剩下的是三类**可逐个落地**的问题:

1. **职责错位还没修完**:地基 `shared.rs`(1372 行)里混着**上传编排**(依赖 `api::work`,还反向依赖 `translate::tables_gen`)与**反编译侧私有的影子大表**;`model ⇄ xml` 这第三处环还在。
2. **门的可靠性有洞(三处,评审补充了后两处)**:① `convert_bench` 默认模式下**基线缺失会静默重建**;② **部分样本缺失**时只打印一行警告就继续,`#meta`/SHA 那一档**静默不设防**;③ `BACKEND_BENCH_REFRESH=1` 写的是 `fresh`,**样本缺失时重刷会把那几个键从基线里永久删掉**;此外报告把**中文文案当协议**解析 —— 改文案就会让预算门静默失效。
3. **NEMO 侧**:无字节门(且加门时**必须能传 `source_version`**,否则锁死"未迁移"口径);前端有可指认的**重复解析**。

**性能**:维持 rounds/37 的实测结论(自家函数无热点、瓶颈是 `Value` 中间树)。本轮**不再提 CPU 微优化**,只提一条**分配计数门**(把"能不能证"变成"能证")。

### 0.1 工作流总览(标记:🔴 动公共 API · 🟠 跨模块 · 🟡 碰产物字节/基线)

| # | 工作流 | 类别 | 优先级 | 工作量 | 标记 | 文件组 |
|---|--------|------|--------|--------|------|--------|
| W1 | 断第三处环 `model ⇄ xml` | 架构 | **P0** | 1 h | 🟠 | G-model |
| W2 | 地基重划线:上传编排 / decompile 私有件出 `shared` | 架构 | **P1** | 1 天(2 提交) | 🟠 | G-shared |
| W3 | 门与仪器加固(5 个子项) | 架构+仪器 | **P0** | 1 天 | 🟡 | G-tests |
| W4 | 报告类型化:中文文案协议 → 结构字段 | 架构+功能 | **P1** | 1 天 | 🔴🟠 | G-report |
| W5 | `pub(crate)` 面收窄 + 死重量清理 | 架构 | **P0** | 半天 | 🔴🟠 | G-translate |
| W6 | NEMO 进门(带 `source_version`)+ 不再静默吞错 | 功能+仪器 | **P1** | 半天–1 天 | 🟡 | G-tests/G-nemo |
| W7 | **分配计数门**(决定性判据) | 性能仪器 | **P1** | 半天 | 🟡 | G-tests |
| W8 | NEMO 前端去重复解析 | 性能 | P2(前置 W6/W7) | 半天 | 🟡 | G-nemo |
| W9 | 编码端少一趟全树遍历(`fill_shield`) | 性能 | P2(前置 W7) | 1 天 | 🟡 | G-model |
| W10 | 容错:内联对象形态的影子 | 功能 | P2 | 1 天 | 🟡 | G-pipeline |
| W11 | 资源下载:失败清单结构化(承接 rounds/29 P3) | 架构健壮性 | P2 | 3 h | 🟠 | G-decompile |
| W12 | 结构小节:测试位置 / 失效注释家族 / 文件体量记账 / 文档勘误 | 架构 | P2 | 半天 | — | 多组 |
| W13 | 保真:`wrap_arithmetic` 移动不重铸(**暂缓并登记**,待立项;rounds/38 §8 残留) | 保真 | 暂缓 | 1–2 天 | 🔴🟡 | G-model |

> **落地状态(以 §0.3 为准)** —— ✅ W1 / W2 / W3a / W3b / W3c / W3d / W3e / W4 / W5 / W6 / W7 / W10 / W11 / W12b / W12c / W12d;
> ◐ W8(只做②);❌ W9(2026-10-02 实测判不做,见 §W9 落地段);⏸ W13(暂缓并登记)。

### 0.2 需你拍板(详见 §5)

| # | 事项 | 评审结论 | 我的建议 |
|---|------|----------|----------|
| **C1** | W4 动公共枚举(`TranslateWarning` 加字段)是否照原设计做 | **接受 W4 原方案**,不退化为"只改标签" | 照原设计做(同批改生产端 + 消费端) |
| **C2** | W12a 是否把 `nemo_mapping.rs` 的 551 行表拆成新文件 | **否决**(rounds/31 §3.5② 已决且已执行) | **不拆**:文件头记账 + 修两条断链注释 |
| **C3** | W2 `shared.rs` 重划线是否执行 | **建议做**(两条硬性分层反转),但按 §2 的两处修正 | 做,分 `W2a`/`W2b` 两个提交 |

> **2026-10-02 已拍板**:C1 **做** / C2 **不拆** / C3 **做**(逐条结论与理由见 §5)。

### 0.3 落地状态(2026-10-01 ~ 10-02,提交后回填)

> §1 的"缺 / 有洞"是**定稿时的快照**(尤其 §1.5);下表是当日实际落地的部分,其余按原计划仍未做。
> 回填只标状态,正文分析与锚点不动。

| # | 落地 | 提交 / 说明 |
|---|------|-------------|
| W1 断 `model ⇄ xml` 环(`xml` 变叶子层) | ✅ | `b0af619` |
| W3a 基线缺失**默认模式也失败** | ✅ | `f62aec7` |
| W3b 夹具严格开关(含"部分样本缺失") | ✅ | `305a108`(开关与主要出口)+ `f01a6a8`(域内余量:`translate/mod.rs` 2 处 + `pipeline.rs` 1 处统一走同一份 `missing_fixture`,`reverse_tests` 的重复定义删除) |
| W3c 删死 allow-list 副本(**不**新增集合门) | ✅ | `e8e19d6` |
| W3d 反向 id 台账 `LOST_ID_BUDGET_REVERSE` | ✅ | `d10d0cb`(首版)、`ef37978`(口径收窄到真正的积木节点) |
| W3e `BACKEND_BENCH_REFRESH=1` 拒绝丢键 | ✅ | `f62aec7` |
| W5 可见性收窄 + 死重量 | ✅ | `6fe2ee0`:156 处收窄(136 `pub(super)` / 10 `pub(in crate::core::convert)` / 10 私有)、删 4 个零调用点 helper、`is_name_char` 改名;② 的 `UnsupportedType` ✅ 已删(`fef30e7`,公共面破坏性变更,已授权)。④ `unused` **维持 `allow`**(试跑 943 条,一轮清不完 ⇒ 清单进 `docs/goals/infra-backlog.md` §1)。**2026-10-02 回填**:④ 的口径已修正并重测(旧「943 = lib 55 + bin 898」不可复现),④ 的前置「bin 去第二个 crate root」已落地 ⇒ bin 侧噪声 **896 → 0**,剩余仅 lib 侧 50 条;见 `docs/goals/infra-backlog.md` §1/§1.1 |
| W6 NEMO 进门(带 `source_version`)+ 不再静默吞错 | ✅ | `cc320f0`、`c7c40d5`(老版本迁移样本)、`0103d7c`(编码失败进报告、计有损) |
| W7 分配计数门(**只记不判**) | ✅ | `22897ef`:只统计的 `#[global_allocator]` + `#meta` 四个 `alloc_*` 键(窗口 = 一次 `translate_file`);同机 4 跑读数逐位相同;跨机不可比 ⇒ 断言时剔除 |
| W8 NEMO 前端去重复解析 | ◐ | `6057a88`:**② 做**(虚拟包装根 ⇒ 每实体省一次整串拷贝:NEMO 样本 −0.19%/0.21% 次数、−3.37%/3.11% 字节),**① 不做**(上界仅 −0.11% 次数、−0.05% 字节,且要换峰值内存);详见 §W8 落地段 |
| W10 容错:内联对象形态的影子 | ✅ | `66c0b6b`:对象影子就地改写成平台同款影子 XML,拒收分支删除;真实作品 `A28社区-开幕_174408420` 由"报错拒收"变**转换成功**(源积木 6029 / 告警 19 / `validateBcm` = VALID),语料扫描的 `[跳过]` 消失(**全语料 0 条跳过**);既有 6 样本产物字节未变;详见 §W10 落地段 |
| W11 资源下载失败清单结构化(`(url, error)`) | ✅ | `2d61959` |
| W12b `model.rs` 测试归位文件末尾 | ✅ | `e8e19d6` |
| W12c 失效注释 / 断链清理 | ✅ | 两条断链 rustdoc(`e8e19d6`);面包屑家族按"**只改指向不存在模块 / 自相矛盾**的、保留有意溯源标注"复核(`f01a6a8`):改 3 处(`api/auth.rs` 的 `editors/simple.rs`、`decompile/editors.rs` 头部的"本目录…"旧布局、`xml.rs` 的 `neko.rs`)、**保留**约 30 处"来自 src/…"/"原 x.rs"溯源 |
| W12d `repo-conventions.md` §4 勘误 | ✅ | 本次文档巡检(§1.3 的"已无 `Io/Json/Http`") |
| W9 编码端少一趟全树遍历(`fill_shield`) | ❌ **判不做** | 2026-10-02 三探针实测(§W9 落地段):**整趟删掉**的上界只 −1.37%/−1.40% 分配(且仅 2/6 样本);**合并能省的那部分(遍历)实测 0 分配**;不写序列化器的唯一路线(去 `skip_serializing_if`)还会改 4/6 样本字节 ⇒ 判不做,`src/` 零改动 |
| W2 地基重划线(C3 已拍板**做**) | ✅ | W2a `6027b18`(上传编排 → 新 `upload.rs`)+ W2b `fb799b6`(反编译私有件 → `decompile/{config,shadow,work}.rs`);**`shared.rs` 1372 → 503 行**;两处 grep 归零。详见 §W2 落地段 |
| W4 报告类型化(C1 已拍板**做**) | ✅ | `eea82bf`:`UnmappedBlock { kind, marked, cleared_shadows }`;`marker_counts` 改读结构字段(不再反解中文);改前/改后 `[标记量]` 读数逐行相同、活性自检(**人为 +1 ⇒ 门变红**)已做。详见 §W4 落地段 |
| W13 `wrap_arithmetic` 移动不重铸 | ⏸ **暂缓并登记** | 2026-10-02 拍板:**不列入执行队列**。收益主要是"我们自己的往返 id 台账更干净 / 往返 id 更稳",**不是用户可见差异**;代价是**偏离官方实现 + 改产物字节 + 需重做实体机与刷基线** ⇒ 待重新立项。证据:`rounds/38 §8`、§W13 |

> 相应地把 §0.1 的 ✅ 项(W1/W2/W3a/W3b/W3c/W3d/W3e/W4/W5/W6/W7/W10/W11/W12b/W12c/W12d)视为**已交付**,W8 为**部分交付**(◐);§1.5 里"反向 id 台账 = 缺""NEMO 字节门 = 缺""基线缺失静默重建""`allowed_entity` 死副本"四条已被上述提交消解。
> 2026-10-01 第二批(本轮):W5 `6fe2ee0`、W7 `22897ef`、W8 `6057a88`、清理(W3b 域内余量 / W12c 复核)`f01a6a8`。
> 2026-10-02:W9 **判不做**(`06036a7`,见 §W9 落地段)、**W10 落地**(`66c0b6b`,见 §W10 落地段);
> **C1/C2/C3 与 W5②/W13 已拍板并全部落地**(见 §5):W2(`6027b18`+`fb799b6`)、W4(`eea82bf`)、W5②(`fef30e7`)均 ✅;
> C2 不拆(文件头已加体量记账)、W13 暂缓并登记。另 G4 的收尾(`d3f617e`,词表新鲜度读数 + 导出基准单一事实源,只打印不判)也已落地。

---

## 1. 现状事实(锚点为实读快照)

> ⚠️ 本节是 **2026-10-01 定稿时**的快照;当日落地情况见 §0.3。

### 1.1 文件与职责

| 文件 | 行数 | 职责 | 备注 |
|------|------|------|------|
| `convert/mod.rs` | 259 | 域门面 + 跨子域编排(`translate_work*` / 上传编排) | 干净 |
| `convert/shared.rs` | 1372 | "地基":错误 / 模型 / 配置(影子大表)/ 加密 / HTTP / 文件 / JSON 扩展 / 批量执行 / **上传编排** | **混合体** |
| `translate/mod.rs` | 749 | 门面 + `translate_value/file` + `detect_editor` + `diff_tests` | 干净 |
| `translate/model.rs` | 2221 | 中核树模型 + `IdSource` + 邻接表编解码 + 程序集拆分/调用点重写 | 测试模块分散(§1.6) |
| `translate/mapping.rs` | 1970 | 官方正向映射移植 + 自建反向 | 唯一 >150 行函数 `parse_node` |
| `translate/assembly.rs` | 2133 | 双向装配(`build_document` / `build_kitten4_document` / `mark_unknown_blocks`) | |
| `translate/pipeline.rs` | 1665 | 正/反文档级编排 + 并行调度 + 临时 id 改写 | |
| `translate/xml.rs` | 1297 | 最小 XML DOM + 属性手术 + 影子/mutation 模板 | 环的另一端(§1.2) |
| `translate/{options,report,tables_gen,kitten4_vocab}.rs` | 262/184/1439/150 | 选项 / 报告 / 生成表 / 编辑器词表 | |
| `translate/nemo.rs` | 1421 | NEMO 文档级管线(骨架 / 版本迁移 / 9 个前置改写 / 归一) | `convert_nemo_document:64-573` ≈510 行 |
| `translate/nemo_mapping.rs` | 2616 | NEMO 映射(前端+映射一体)+ 尾部 ≈551 行手写表 | 超 ≈2500 软上限,**按 rounds/31 §2 属容忍带**(§5-C2) |
| `translate/{reverse_tests,nemo_tests}.rs` | 2671/832 | 反向保真门 + 两台语料扫描器 / NEMO 测试 | 门所在地 |
| `decompile/{mod,editors}.rs` | 1950/1777 | 反编译门面 + 引擎 / 7 家编辑器实现 + 资源管理器 | 5 条离线单测 |

> 评审已逐条 `wc -l` 复核:上表 18 个数字**一个不差**。

### 1.2 依赖、环与分层反转

- ✅ 已断(rounds/37 Phase 1):`model ⇄ mapping`、`nemo ⇄ nemo_mapping`。
- ❌ **仍在的环**:`model ⇄ xml` —— `model.rs:2` `use super::xml::{math_number_node, math_number_shadow, xml_attr_value}`;`xml.rs:8` `use super::model::BlockJson`。
  `BlockJson` 在 `xml.rs` 只出现两次:`:8`(import)与 `:117`(`math_number_node` 内)⇒ **`xml.rs` 的 model 依赖只有那一个函数**。
- ❌ **地基 → api 反向依赖**:`shared.rs:1138` `use crate::api::work::{CreateKittenWorkArgs, …, KittenWorkManager, NekoWorkManager, NemoWorkManager}`(只服务 `create_draft`)。
- ❌ **地基 → translate 反向依赖**:`shared.rs:1238`(在 1237-1241 的兜底分支里引用 `translate::tables_gen::BCM_VERSION`)。
- ⚠️ 测试级泄漏:`nemo_tests.rs` import `decompile::{DecompilerContext, WorkDecompiler}`(仅测试;W2b 可顺带迁走)。

### 1.3 错误模型:同一个域里三种风格

| 层 | 类型 | 证据 |
|----|------|------|
| 公共面(decompile) | `DecompilerError`(8 变体,`pub` 且在 `convert/mod.rs:21` 再导出) | `shared.rs:38-61` |
| 公共面(translate) | `TranslateError`(5 变体,含 `Decompiler(#[from] DecompilerError)`) | `options.rs:235-245` |
| translate **内部** | 复用 `DecompilerError` + `shared::Result` 别名 | `assembly.rs:6`、`model.rs:4`、`xml.rs:9` |
| 映射层 | **不可失败**(只发 `TranslateReport`) | `mapping.rs:192/825` |

- 死变体:`DecompilerError::UnsupportedType`(`shared.rs:46-47`)全仓 **0 调用点**(但删它 = **动公共枚举**,见 W5②)。
- **文档已过时**:`docs/knowledge/repo-conventions.md` §4 的"`DecompilerError` 仍自带 `Io/Json/Http`,与 `MewError` 重复(待改)" —— 实测已无这三个变体,改用 `From<io::Error>` / `From<serde_json::Error>` 折进 `Mew`(`shared.rs:62-72`)。⇒ W12d。

### 1.4 可见性

`translate/*` 里绝大多数项是 `pub(crate)`(例:`xml.rs` 的 10 个 helper、`mapping::{truthy,is_text_placeholder}`、`model` 的几乎全部项),但调用点经全 `src` + `tests` grep **全部落在 `translate` 子树内** ⇒ 这些项对整个 crate 可见,是**过宽面**(W5)。

### 1.5 门与仪器(现状 + 本轮新发现的三处洞)

| 门/仪器 | 位置 | 状态 |
|---------|------|------|
| `convert_bench` SHA256 + `#meta` | `tests/convert_bench.rs:88/259/343/358` | 有洞,见下三行 |
| ⤷ **基线缺失静默重建** | `:395-399`(`baseline.is_empty()` ⇒ 写盘)+ `:420-427`(NotFound 非严格 ⇒ 返回空 map) | W3a |
| ⤷ **部分样本缺失只打印** | `:239`(`missing.len() == SAMPLES.len()` 才 panic)+ `:244-246`(否则只 `eprintln!`) | W3b |
| ⤷ **REFRESH 丢键** | `:381` 写的是 `fresh`(只由**存在**的样本构成) | W3e |
| 正向扫描器 + `[id台账]` + `LOST_ID_BUDGET` | `reverse_tests.rs:1017-1051`(表)、`:1281`(台账)、`:1443-1451`(断言) | 已有,好 |
| `MARKER_BUDGET` | `reverse_tests.rs:1568`(表),读数靠 `marker_counts:787-804` | 已有,但**读中文文案** |
| 定义体预算 `DEFICIT_BUDGET=3193` | `reverse_tests.rs:2009`(常量)、`:2015-2020`(断言) | 已有 |
| 扫描器"差异类别" | 正向 `:1276-1279/1405-1420/1448`;反向 `:1599-1602/1689-1718/1721` | **只打印不断言**(W3c) |
| `allowed_entity` 死副本 | `reverse_tests.rs:1825-1835`(`:1832 let _ = &allowed_entity;`) | **死代码**;活的那份在 `:2192-2225`(真过滤 + 断言) |
| 反向 id 台账 | — | **缺**(rounds/38 §8 自记) |
| NEMO 字节门 | `SAMPLES`(`:61`)只有 4 个 Kitten 样本 | **缺**,且 `Sample` 无法传 `source_version` |
| 分配/内存门 | — | **缺**(rounds/37 §6.3 自记的 gap) |
| `[lints.rust] unused = "allow"` | `Cargo.toml` | 死代码不报(W5④ 建议改 `warn`) |

### 1.6 其他已核实偏差

- `model.rs` 的 5 个测试模块有 **4 个夹在生产代码中间**(`:221`、`:280`、`:481`、`:821`;第 5 个 `neko_tests:1881` 在末尾)⇒ 违反 `repo-conventions` §5"测试在文件末尾"(W12b)。
- 死 helper(被 `unused=allow` 掩盖):`nemo.rs:1132 find_descendant`、`nemo.rs:1200 find_value_shadow`(两者只剩自递归)。
- 命名撞车:`is_name_char` 两处语义无关(`xml.rs:428` XML 名称字符 vs `assembly.rs:653` 官方显示名净化字符集)。
- **失效注释是一个家族(约 20 处,不是 2 处)**:`来自 src/…` 面包屑仍在 `decompile/editors.rs:23/126/834/1152`、`decompile/mod.rs:26/844`、`shared.rs:230/244/932/990/1025`、`translate/assembly.rs:10`、`model.rs:12/572/1004`、`nemo.rs:13`、`nemo_mapping.rs:13/2057`、`pipeline.rs:551`、`xml.rs:145`;另有"原 `*.rs`"式 `decompile/editors.rs:1368/1453`、`decompile/mod.rs:720/737`。**两条已断的 rustdoc 链接**:`nemo.rs:21`(`[`super::neko`]`,模块不存在)、`nemo_mapping.rs:17`(`[`super::tables_gen_nemo`]`,文件不存在)。
- `nemo.rs:1416 tree_to_json`(`:1419 filter_map(ok())`)静默吞错;`model.rs:1776` 同名函数返回 `Result`(W6②)。

---

## 2. 工作流明细

> 每条含 **证据 / 做法 / 收益 / 风险 / 验证 / 回退 / 工作量 / 标记 / 评审处置**。
> 通用验证基准(见 rounds/37 §5 矩阵):`cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test` +
> `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored`(四样本 SHA256 + `#meta`)。

### W1【架构·P0】断第三处环:`model ⇄ xml` 🟠
**证据**:`model.rs:2` ↔ `xml.rs:8`;`BlockJson` 在 `xml.rs` 只出现于 `:8`(import)与 `:117`(`math_number_node` 内);调用点 `model.rs:1328`、`mapping.rs:527/532`(引用处 `mapping.rs:54-56`)。
**做法**:把 `math_number_node`(`xml.rs:116-129`)搬进 `model.rs`(紧挨 `BlockJson`),`mapping.rs` 改从 `model` 引;`xml.rs` 只留纯字符串模板(`math_number_shadow:109`、`pure_list_shadow:131`)。搬完 `xml.rs` 不再依赖 `model` ⇒ 依赖变成 `model → xml` 单向。
**收益**:`xml` 成为真正叶子层(只依赖 `shared`);域内第三条环消失。
**风险**:低。`model.rs:1738` 仍用 `math_number_shadow`(model → xml 单向,合法)。
**验证**:`cargo test`(`kitten_tests`/`reverse_tests` 的影子断言)+ 四样本 SHA 逐字节不变。
**回退**:单提交 revert。
**工作量**:1 小时。
**评审处置**:✅ **采纳**。补一条评审要求:同步改 `xml.rs:145-150` 附近的头注释("本模块只依赖 `shared`/`model` 的类型" ⇒ 按新事实改写)。

---

### W2【架构·P1】地基重划线:`shared.rs` 只留"两子域真正共用"的东西 🟠

**W2a——上传编排出地基**(证据:`shared.rs:1138`(api 依赖)、`:1238`(translate 依赖)、`:1147 DraftUpload`、`:1229 create_draft`、`:1308 upload_tests`;消费者:`convert/mod.rs:238`、`decompile/mod.rs:424` 与 `:419-430`)
**做法**:新建 `src/core/convert/upload.rs`(`pub(crate)`),搬 `DraftUpload` / `supports_account_upload` / `draft_name` / `channel_for` / `SINGLE_PACKAGE_LIMIT` / `ensure_single_package_fits` / `create_draft` + `upload_tests`;`create_draft` **保留**"空则常量兜底"的语义,但兜底常量在 `upload.rs` 内引用 `translate::tables_gen::BCM_VERSION`(单一定义源,不复制常量)。
**收益**:一次修掉**两处**分层反转(地基 → api、地基 → translate)。
**风险**:中低。评审指出的关键点:`bcm_version` 有**两个调用点** —— `convert/mod.rs`(可给 `tables_gen::BCM_VERSION`)与 `decompile/mod.rs:424`(给 `context.work_info.bcm_version`,可为空;`rounds/30 §4` 明写"空则回落到本库常量")。若把兜底改成"调用方传入",则要么新增 `decompile → translate` 的**跨子域**依赖(比原来更坏),要么改行为。
⇒ 本方案的选法:**把 `upload.rs` 定位为"域级工具层"**(与门面同级),允许它依赖 `translate::tables_gen`;**地基 `shared.rs` 保持零反向依赖**。这样既不动行为,也不新增 `decompile → translate`。
**验证**:编译 + `cargo test` + `grep -rn "crate::api::" src/core/convert/shared.rs` = 0 + `grep -rn "translate::" src/core/convert/shared.rs` = 0 + 四样本 SHA 不变。
**回退**:单提交 revert。
**工作量**:半天。
**评审处置**:⚠️ **部分采纳** —— 采纳"两个调用点 + 兜底语义不能动"的更正;**不采纳**评审建议的"让调用方各传各的"两种写法(前者造跨子域依赖,后者复制常量)。改用上面的"域级工具层"居中方案,并在文档里写明它的层级定位。

**W2b——decompile 私有件出地基**(证据:`shared.rs:282 ShadowTemplate`、`:306 DecompilerConfig`(影子字段 `:312-315`、构造 `:320-560`)、`:690 ShadowBuilder`;消费者 `editors.rs:438/501/783`、`decompile/mod.rs:882/894/912/1011/1287`、`nemo_tests.rs:797`)
**做法**:按**实际消费者**重定边界(评审提供实测):
- **留在地基**(translate 生产码真用):`EditorType`、`WorkId`、`XHTML`、`DecompilerError`/`Result`、`IdGenerator`、`FileService`。
- **随 decompile 走**:`DecompilerConfig` + `ShadowTemplate` + `ShadowBuilder`(→ `decompile/config.rs`、`decompile/shadow.rs`)、`WorkInfo`/`RawWorkData`/`WorkFetcher`、**`ResultExt`/`ValueExt`**(translate 生产码零使用)、**`CryptoService`/`HttpClient`/`CodeMaoHttpClient`**(translate 生产码零使用,只有 `nemo_tests.rs:767/787` 的测试桩)、**`batch_map`**(translate 零使用;消费者是门面 `convert/mod.rs` + `decompile/mod.rs:267` ⇒ 放**域级工具** `convert/batch.rs`,与 `upload.rs` 同级)。
- 顺带:`nemo_tests.rs:763-817` 那个 `OfflineHttp` + `NemoDecompiler` 用例其实是 **decompile 侧测试**(rounds/37 §0.4 就这么建议)⇒ 随本次迁进 `decompile/`,消掉 §1.2 记的"测试级泄漏"。
**收益**:`shared.rs` 1372 → **≈400 行**;"地基 = 两子域共用"从口号变成结构事实;单子域的实现细节(影子 XML 模板)回到它的子域。
**风险**:中(触点:`decompile/*`、`convert/mod.rs`、`translate/nemo_tests.rs`),全是机械替换,编译器兜底。
**验证**:编译 + `cargo test` + `grep -rn "crate::core::convert::shared" src/core/convert/translate` 只剩上面那 6 个共用名。
**回退**:单提交 revert。
**工作量**:半天。
**评审处置**:⚠️ **部分采纳** —— 采纳"保留集判据错了"的更正与 `batch_map`/`CryptoService`/`HttpClient`/`ValueExt`/`ResultExt` 的去处;保留方案的搬迁范围与验证方式。收益口径按评审统一为"W2a 去掉两条反向依赖 + W2b 修一处职责错位",不把 W2b 说成分层反转。

**落地(2026-10-02,两个提交)** —— `shared.rs` **1372 → 503 行**(W2a 后 1128 行、W2b 后 505 行、W5② 再删 2 行),两处分层反转归零:

- **W2a(`6027b18`):上传编排出地基 → 新 `src/core/convert/upload.rs`(`pub(crate)`,253 行)**。搬走 `DraftUpload` /
  `supports_account_upload` / `draft_name` / `channel_for` / `SINGLE_PACKAGE_LIMIT` / `ensure_single_package_fits` /
  `create_draft` + `upload_tests`;两个调用点(`convert/mod.rs` 门面、`decompile/mod.rs` 反编译侧上传)语义不变。
  - **为什么选"域级工具层"而不是"调用方各传"**:`DraftUpload.bcm_version` 的"空串 ⇒ 用本库常量兜底"是**刻意语义**
    (`rounds/30 §4`),而反编译侧的"上传到账号"是它的**第二个调用点**;若改成"调用方必须传",要么给
    `decompile → translate` 造一条**新的跨子域依赖**(比原来更坏),要么在**两处复制常量**。⇒ 把 `upload.rs` 定位为
    与域门面 `mod.rs` **同级**的工具层,允许它依赖 `translate::tables_gen`(单一定义源);地基 `shared.rs` 保持**零反向依赖**。
- **W2b(`fb799b6`):反编译私有件搬回 `decompile/`** —— `DecompilerConfig` + `ShadowTemplate` → `decompile/config.rs`、
  `ShadowBuilder` → `decompile/shadow.rs`、`WorkInfo`/`RawWorkData`/`WorkFetcher` → `decompile/work.rs`。保留集按
  **实际消费者**划线(留地基的:`EditorType`/`WorkId`/`XHTML`/`DecompilerError`/`Result`/`ResultExt`/`ValueExt`/
  `FileService`/`CryptoService`/`HttpClient`/`CodeMaoHttpClient`/`batch_map`/`IdGenerator`);顺带把 `nemo_tests.rs` 里
  那个其实是 decompile 侧的用例随件迁走(`rounds/37 §0.4` 的建议)。
- **搬迁做了逐字节比对**:四块(`work`/`config`/`shadow`/`upload`)与搬迁前原文 **verbatim 命中**,只新增模块文档与 import
  ⇒ 产物字节无关,由 6 样本 SHA256 + `#meta` 门复核。
- **验收**:`cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` / `cargo test` 全绿;
  `grep -rn "crate::api::" src/core/convert/shared.rs` **无输出**、`grep -rn "translate::" src/core/convert/shared.rs`
  **无输出**(两处分层反转归零);`BACKEND_REQUIRE_BENCH=1 … convert_bench --ignored` 通过(SHA256 与 `#meta` 同基线)。

---

### W3【架构+仪器·P0】门与仪器加固 🟡
五个子项,全在 `tests/` + `reverse_tests.rs`(与 W4/W6/W7 同文件组,按序)。

**W3a 基线缺失必须失败,但不能断掉"从零建基线"**(证据:`convert_bench.rs:420-427`(NotFound 非严格 ⇒ 返回空 map)、`:395-399`(空基线 ⇒ 静默写盘))
- 做法:`load_baseline()` 的 NotFound 分支改成:严格模式或**非 REFRESH** ⇒ panic;`BACKEND_BENCH_REFRESH=1` 时仍返回空 map(允许首跑建基线)。同时删掉 `:395-399` 的"首次运行自动写基线"分支(改为提示"用 `BACKEND_BENCH_REFRESH=1`")。
- 收益:默认模式下门不可能再被"基线被删/首次检出"静默绕过。
- 风险:**行为变更(期望的)**:全新检出第一次跑 `convert_bench` 不再绿,须先显式 REFRESH。
- 验证:临时重命名 fixture → 默认跑应红、`BACKEND_BENCH_REFRESH=1` 应能建首版;再验证"REFRESH 之外不会写盘"。

**W3b 夹具/样本严格开关,并覆盖"部分缺失"**(证据:`reverse_tests.rs` 的 `real_bcmkn` 族、`:1249-1254`、`:1271-1273`、`:1584-1596`、`:1731-1733`、`:2254-2256`;`convert_facade_bench.rs:157-158` 的 `continue`;`convert_work_bench.rs:39` 自有一份 `strict_mode()`;`convert_bench.rs:239/244-246`)
- 做法:新增 `BACKEND_REQUIRE_FIXTURES=1`,把"缺夹具 return"与"**部分样本缺失只打印**"统一折成 panic;`convert_work_bench.rs` 的 `strict_mode()` 复用它(避免第三个口径)。
- 验证:`download/` 临时改名 ⇒ `BACKEND_REQUIRE_FIXTURES=1 cargo test --lib` 应红、默认应绿。

**W3c 清死代码;不新增"类别集合门"**(证据:死副本 `reverse_tests.rs:1825-1835`;活的那份 `:2192-2225`(真过滤 + `assert!`);"只打印"处 `:1276-1279/1405-1420/1448`、`:1599-1602/1689-1718/1721`)
- 做法:**删掉 `:1825-1835` 那 4 行**(活的已在 `:2199-2225`),保留"类别只打印 + 人工分诊"。
- 不新增集合门(评审"半否决"的结论):`download/` 是 **gitignored、采集器增量写入**的(rounds/37 §7),新增一件语料就可能引入新类别 ⇒ 集合门必然**假红**;仓库既有门一律用"按**文件名**索引 + 表外取表内最大值兜底"(`reverse_tests.rs:1429-1432`),集合门没有这个兜底,与既有口径不合。
- 验证:`cargo test` 全绿(删的是死代码,应有零行为变化)。

**W3d 反向 id 台账**(证据:rounds/38 §8"反向侧没有 id 台账";正向那条 `:1017-1051/1281/1443-1451`)
- 做法:在 `kn_corpus_round_trip_sweep`(`:1582`)里按 `collect_ids`(`:926`)/`block_node_ids`(`:964`)同口径统计"源块 id 在 Kitten4 产物里缺失"的真块/影子数,新增 `LOST_ID_BUDGET_REVERSE`(**只许变小**;沿用 `:1429-1432` 的"表外取表内最大值"兜底)。
- 收益:反向保真第一次有**直接证据**;`incompatible_*` 兜底从此有回归门。
- 风险:低(只加断言);新增基线须在提交信息里写明口径与实测值。

**W3e `BACKEND_BENCH_REFRESH=1` 不得丢键**(证据:`convert_bench.rs:381` 写的是 `fresh`;`fresh` 只由存在样本构成 `:343/358`)
- 做法:进入 REFRESH 分支时,若 `!missing.is_empty()`(**待补充:见 §5** —— 或)⇒ **拒绝写盘**并列出缺失样本与将被删除的键名;要强制刷新需第二个显式开关。
- 收益:堵住"重刷一次就永久少守一档、事后看不出来"的洞(评审 L1(b),这是本轮最值得修的洞)。
- 验证:删一个样本 → `BACKEND_BENCH_REFRESH=1` 应**拒绝写盘并列出键**;样本齐时刷新正常。

**工作量**:W3 合计 1 天。**回退**:五个子项各自独立 revert。
**评审处置**:W3a ⚠️ **部分采纳**(采纳"必须写成 NotFound 且非 refresh ⇒ panic");W3b ✅ **采纳**(含评审补充的"部分缺失"与 `convert_work_bench`);W3c ⚠️ **部分采纳**(采纳"直接删、不接线";**不采纳**新增集合门 —— 理由如上);W3d ✅ **采纳**(含表外兜底口径);W3e ✅ **新增采纳**(评审 L1(b) 的新发现)。

---

### W4【架构+功能·P1】报告:把"中文文案当协议"改成结构字段 🔴🟠
**证据**:`report.rs:24-26` 明写"冻结协议"(计数拼进 `kind`);`reverse_tests.rs:787-804 marker_counts` 用 `split_once("已改成未收录积木 ")` 反解中文 ⇒ **文案一改,读数静默变 0,`MARKER_BUDGET` 变成"永远通过"**;`report.rs:53` 的类别标签 "未映射积木(**保留原类型名**)" 与 rounds/38 后的语义**相反**。
**做法**:`TranslateWarning::UnmappedBlock { kind: String, marked: u64, cleared_shadows: u64 }`(`kind` 回归纯类型名);`assembly.rs:881 mark_unknown_blocks` 直接填字段;`pipeline.rs:772-808 remap_warning` 同步(穷尽 match);`mapping.rs:1322/1349` 两处**纯类型名**发射点填 `0`;`marker_counts` 改读字段;`category()` 标签改对;**把 `report.rs:24-26` 那条注释改写成新协议的结构说明(不要删,它记的教训仍有效)**。
**收益**:门不再依赖文案;报告可被程序消费(`counts()` 已是结构化聚合);消除"改文案 → 门失效"。
**风险**:**动公共 API**(`TranslateWarning` 是 `pub`)。受影响文件:`report.rs`/`assembly.rs`/`pipeline.rs`/`mapping.rs`/`reverse_tests.rs`/`tests/convert_live.rs`(类别聚合)。**必须与改门同批**;且因 `remap_warning` 是穷尽 match,**枚举本身的改动要与 W6② 串行**(评审 §2-②)。
**验证**:`cargo test` 全绿(含 `MARKER_BUDGET`)+ 四样本 SHA 不变 + **反向自检**:人为把某类型 `marked` 加 1 ⇒ 门必须红。
**回退**:单提交 revert(字段与文案要一起回)。
**工作量**:1 天。
**评审处置**:✅ **采纳原设计**(C1),并按评审要求**删掉原方案的 `to_json()` 加料**(`counts()`/`to_markdown()` 已够)。

**落地(2026-10-02,`eea82bf`)** —— 报告不再把中文文案当协议:

- **做法**:`TranslateWarning::UnmappedBlock { kind: String, marked: u64, cleared_shadows: u64 }`(`kind` 回归**纯类型名**);
  生产端同批改(`assembly.rs::mark_unknown_blocks` 两处直接填字段、`pipeline.rs::remap_warning` 透传两个字段、
  `mapping.rs` 两处纯类型名发射点填 `0`);消费端同批改(`reverse_tests.rs::marker_counts` 改成**读字段**、
  `tests/convert_live.rs` 的类别聚合取 `kind`);`report.rs` 把"冻结协议"注释改写成新协议说明(**保留**那条教训)、
  `category()` 从语义相反的"未映射积木(**保留原类型名**)"改成对的说法;**未加**方案里被评审否掉的
  `TranslateReport::to_json()`。
- **读数不变(证据)**:改前/改后各跑一遍 `cargo test --lib -- kn_corpus_round_trip_sweep --nocapture`,10 件语料的
  `[标记量] {label}: 块 X / 影子 Y` **逐行完全相同**(块合计 1237 / 影子合计 83)—— 变的只是取值来源
  (读结构字段取代反解中文)。
- **活性自检**:临时把某类型的 `marked` 人为 +1 ⇒ 读数升高且 `MARKER_BUDGET` **变红**(exit 101);复原后恢复 passed
  ⇒ 门读的是真字段,不是死数据。
- **兼容性**:`TranslateWarning` 是 `pub`,加字段会让下游穷尽 `match` 编译错(**有意**,预 1.0 可接受);本仓所有消费点已同批改完。

---

### W5【架构·P0】可见性收窄 + 死重量清理 🔴🟠
**证据**:§1.4(过宽的 `pub(crate)`);`shared.rs:46-47`(`UnsupportedType` 零调用);`nemo.rs:1132/1200`(自递归死 helper);`xml.rs:428` vs `assembly.rs:653`(同名不同义);`Cargo.toml` 的 `unused = "allow"`。
**做法**:
① **`pub(super)` 的正确落点**:只对**声明在 `translate/` 子目录文件**里的项(如 `model/mapping/xml/nemo/nemo_mapping/pipeline.rs`)收窄 —— 对它们 `pub(super)` = `translate`,语义正确(且 `translate::reverse_tests`/`nemo_tests` 作为子模块仍可见);**声明在 `translate/mod.rs` 自身的项不动**(对它而言 `pub(super)` = `core::convert`,那是**放宽**不是收窄)。
② 删 `DecompilerError::UnsupportedType`(`shared.rs:46-47`)—— **标 🔴:这是删 `pub` 枚举的变体**,会破坏下游穷尽 `match`("全仓 0 调用点 ≠ 外部 0 消费者");需授权(仓库允许破坏性删除,但必须显式标注)。
③ 删 `nemo.rs:1132 find_descendant`、`nemo.rs:1200 find_value_shadow`;`assembly.rs:653` 改名 `is_display_name_char`。
④ **不手写 `dead_code` 审计**:把 `Cargo.toml` 的 `[lints.rust] unused` 从 `"allow"` 改 `"warn"`,一次 `cargo clippy --all-targets` 列全仓(rounds/37 §11.2 #6 已建议;本域只处理自己清单)。
**收益**:域外可见面收窄;死代码不再被当样板抄。
**风险**:低-中(编译器兜底;②是公共面破坏,需授权)。
**验证**:`cargo build/clippy/test` 全绿 + 四样本 SHA 不变。
**工作量**:半天。
**评审处置**:⚠️ **部分采纳** —— 采纳②的 🔴 标注与"删变体 = 动公共枚举"的更正、采纳①的落点更正、采纳④的替代做法(改用 lint 开关,不手写审计);保留③。

**落地(2026-10-02)**:①③ 在 `6fe2ee0`(156 处可见性收窄、删 4 个零调用点 helper、`is_name_char` 改名);
② 的 `DecompilerError::UnsupportedType` **已删(`fef30e7`)** —— 公共面**破坏性**变更(下游穷尽 `match` 会失败),已授权,
除该变体外 `DecompilerError` 的形状与其余变体未动(删前零调用点证据:`grep -rn UnsupportedType src tests` 只有声明处本身);
④ **维持 `unused = "allow"`**(临时改 `warn` 试跑 943 条、一轮清不完 ⇒ 清单进 `docs/goals/infra-backlog.md` §1)。

**回填(2026-10-02,续测 + 前置落地)**:④ 的**前置**(bin 不再当第二个 crate root)已完成 —— `src/main.rs` 改 `use backend::…`(公共 API 零改动;bin 单元 dep-info 输入 49 → 1、bin 单测目标 121 tests → 0、`unused=warn` 下 bin 侧 896 → 0)。
- **口径修正**:旧「943 条 = `lib` 55 + `bin "backend"` 898」是用 `--message-format short` 量的,**该格式不带 target 信息**,分不出编译单元 ⇒ 不可复现(且 `943 ≠ 55+898`)。新口径:临时把 `unused` 改 `warn`,按**目标选择**分别跑并读 json —— 改前 `--lib` **50**(全 `src/lib.rs`)/ `--bins` **946**(bin 自身 **896**)/ `--tests` **551**(bin-test 453 + lib-test 98 + 集成测试 0);改后 bin 侧在两个选择里都是 **0**,只剩 lib 侧 50。
- `unused` **仍为 `allow`**:剩余 lib 侧 50 条的逐条清单在 `docs/goals/infra-backlog.md` §1.1。放开的前提已实测 —— 显式 `warn` **挡不住** `clippy --all-targets -- -D warnings`(rustc 1.98.0:`-D warnings` + `--allow=unused` 退出 0;+ `--warn=unused` 退出 1),而 Cargo 正是把 `[lints.rust]` 以 `--warn/--allow=unused` 传给 rustc(`cargo build -v` 的 rustc 命令行可见)⇒ **先清清单再按族放开**。

---

### W6【功能+仪器·P1】NEMO 进门(带 `source_version`)+ 不再静默吞错 🟡
**证据**:`convert_bench.rs:61 SAMPLES` 只有 Kitten 样本;`Sample` 结构(`:53-59`)只有 `label/path/target/slug`,`measure`/`warmup`(`:162-169/211-219`)统一构造 `TranslateOptions`,`**无任何地方能传 `source_version`**`;而 NEMO 迁移只由 `TranslateOptions::source_version` 驱动(`options.rs:126-131`、`nemo.rs:686-705`)⇒ 按原方案加样本会得到**不迁移**的产物。另:`nemo.rs:1419 filter_map(ok())`(`:1416-1420`)静默吞错;`nemo.rs:602-605` 既有降级用 `DroppedField` 记"XML 解析失败"。
**做法**:① `Sample` 增 `source_version: Option<&'static str>`,`measure`/`warmup` 透传;NEMO 样本用 `Some("0.16.2")`(该样本真值,`nemo_tests.rs:321`);基线键解释里写明"该样本带 0.16.2 迁移";重刷基线。② `nemo::tree_to_json` 不再静默丢错,把失败纳入 `TranslateReport`(要决定是否计入 `is_lossy`,这是**行为可见性变化**,写进提交信息)。③ 与 W4 串行(枚举/`remap_warning`)。
**收益**:NEMO 从"6 条内存单测、无字节门"变成"有 SHA 基线(**官方迁移口径**)+ 有损可见"。
**风险**:低-中;**会改基线**(新增键,`BACKEND_BENCH_REFRESH=1`,逐键解释)。
**验证**:`BACKEND_REQUIRE_BENCH=1` 下 NEMO 键有 SHA 且可复现(改 `source_version` 应导致该键 SHA 变 —— 反向证明参数真的生效);构造 `to_value` 失败输入的单测断言进报告。
**工作量**:半天–1 天。
**评审处置**:⚠️ **部分采纳** —— 采纳评审 L3 的必填参数更正(这是本项能否证对东西的关键);②保留;并采纳"枚举改动与 W4 串行"的约束。

---

### W7【性能仪器·P1】分配计数门 🟡
**证据**:rounds/37 §6.3 自记的 gap("没有分配/内存门 ⇒ 任何'减少 clone'的优化只能靠同轮 A/B",而 A/B 噪声 ±2% 让 P2/P3/P5/P6 全部无法判);profiler(rounds/37 §10.5)显示瓶颈是 `Value` 树构造/析构(libc 40.7% + `drop_glue<Value>` 5.5% + `BTreeMap` 插入/消耗 ≈10%)。
**做法**:在 `tests/convert_bench.rs` 挂一个**只统计**的 `#[global_allocator]`,把计数写进 `#meta`;**计量窗口必须写明**:
- 只包住**收敛后的一次** `translate_file`(在 `measure` 的单次循环内取前后快照差);`warmup` 与 `RUNS` 的其余轮**不计入**;`:271` 的产物 SHA256 读文件不计入;`println!`/`eprintln!` 不计入。
- **串行腿与并行腿分开记两个键**(`alloc_serial` / `alloc_parallel`):`thread::scope`/`spawn` 自身必分配,1 vs 8 必然不同数。
- **可复现性口径**:同机同档可复现;`entity_workers` 受 `available_parallelism`(`:254`)夹取 ⇒ **跨机不作门**,只作同机 A/B 的确定性判据。
- 首版**只记录不判定**(让它自然成为可比基线);跑稳后再考虑"只许变小"。
**收益**:此后"少建中间 `Value`/少一次深拷"的改动有**确定性、零噪声**的判据,终结"落噪声里"的循环;并给 W8/W9 一个可判前提。
**风险**:低(只在测试二进制内;不动产物)。计数分配器会让**墙钟**变慢 ⇒ 它只作为分配判据。
**验证**(活性自检,改成一行的改法):临时在 `fill_shield`(`model.rs:1792`)入口 `return;` —— 该改动会**同时改 SHA**,正好一并证明"分配门与 SHA 门都活着";跑完复原。
**工作量**:半天。**会改基线**(只新增键)。
**评审处置**:⚠️ **部分采纳** —— 采纳本项;采纳评审对"缺计量窗口定义/跨机不可复现/自检不可实施"的三条更正(判据按上面重写,自检换成 `fill_shield` 早退)。

---

### W8【性能·P2,前置 W6+W7】NEMO 前端去重复解析 🟡
**证据**:`nemo.rs:626` 与 `nemo_mapping.rs:655` 同款 `parse_fragment(&format!("<root>{xml}</root>"))`;`nemo_mapping.rs:617-631` 调用 `has_return_blocks`(`:684-688` 内 `:685` 再 `format!`+解析)⇒ **同一串被完整解析两次**。
**做法**:① `has_return_blocks` 改成吃**已解析的节点**(`&[XmlNode]`)——**风险更正**:这要把 parse **前移到扩张循环**并把 `Vec<XmlNode>` 带进 `:653` 的解析循环 ⇒ **同时持有全部条目的已解析树**(3.4 MB XML 的 DOM 显著更大),是"CPU 换峰值内存"的取舍,不是"低风险";② 评估不包 `<root>` 的入口省掉整串 `format!`(`xml.rs:388 parse` 已支持多顶层元素,`:1242` 有测试),但须逐条核对 `parse_fragment`(`:405-421`)的"文本落点/空白归属"语义。
**收益**:每条带返回值的程序集省一次完整解析;每实体省一次整串分配。**量级先量**(W7 分配计数 + W6 样本)。
**风险**:①中(峰值内存,须给数据);②中(语义一致性)。
**验证**:W7 的 `alloc_count` 下降 + NEMO 样本(带迁移口径)SHA 不变 + `nemo_tests` 7 条全绿;**测不出或内存涨得不划算 ⇒ 不做**。
**工作量**:半天。
**评审处置**:⚠️ **部分采纳** —— 采纳①的风险更正(并入"测不出就不做"口径);②不变。

**落地(2026-10-01,分配门 W7 已可用 ⇒ 按"先量后改"逐条裁决)** —— 提交 `6057a88`:

- **② 做**(收益比预期大一个量级,且是净简化):`Parser` 加**虚拟包装根**(`run_inner(Some("root"))`),
  `parse_fragment` 直接吃片段原文 ⇒ 不再构造包装串。实测(`convert_bench` 的 `alloc_*`,窗口口径不变、
  前后各一跑):`nemo-3.4MB` **1629387 → 1626347 次 / 214 586 067 → 207 344 832 B**
  (−0.19% 次数、**−7.24 MB = −3.37% 字节**);`nemo-old-1.5MB` **506399 → 505321 次 /
  70 484 092 → 68 290 438 B**(−0.21% / **−2.09 MB = −3.11%**);**4 个 Kitten 样本逐位不变**(对照)。
  产物 SHA256 逐字节不变。语义等价由新单测 `parse_fragment_equals_wrapped_parse`(12 组输入与
  "真套 `<root>`"的老写法逐字段对照,错误只比"是否 Err")+ 官方 harness 门守:
  `蛋仔派对…194684070.bcm` / `nemo-103791894.bcm` 均 `validateBcm → VALID`、语义 diff 在 allow-list 内。
  唯一契约变化:`parse_fragment` 不再要求"唯一根"(顶层多元素本来就合法),空片段 = 0 个顶层元素。
- **① 不做**(量级不划算):量到它的**上界**(临时让 `has_return_blocks` 恒 `false`,恰好等于
  "去掉那次重复解析"):`nemo-3.4MB` 1626347 → 1624506 次(**−1841 / −0.11%**)、197.7 → 197.6 MiB
  (**−105 KB / −0.05%**);`nemo-old-1.5MB` 与 4 个 Kitten 样本逐位不变(该样本没有程序集 `blocksXML`)。
  且**同一实验下产物 SHA 逐字节不变** ⇒ 语料里**没有**带返回值的程序集(`K = 0`),
  方案"每条带返回值的程序集省一次完整解析"那条收益在本语料上**不存在**;代价侧还要把解析前移到
  扩张循环(同时持有全部程序集 DOM)+ 为免 ROUND 副本深拷引入共享树(`Rc`)。
  ⇒ 拿 0.05% 字节换峰值内存 + 一处 id 铸序敏感的重排,按本项"内存涨得不划算 ⇒ 不做"判**不做**。
- 结论:W8 真正的收益全在 ②("每实体一次整串分配");① 那条"重复解析"在程序集小语料上量级过小。

---

### W9【性能·P2,前置 W7】编码端少一趟全树遍历 🟡
**证据**:`model.rs:1776 tree_to_json` 先 `BlockJson::to_value()` 逐根(serde 一整棵树),再由 `fill_shield`(`:1792`)走第二趟全树,**每个缺键节点插一个 `"shield"` 键**(=节点数级分配)。
**做法**:把"补 `shield`"并进编码那一趟(**不得引入手写序列化器** —— 那是 rounds/37 §10.6 **实测回退**的 P6 家族)。
**收益**(改写):减少 ≈ **每次转换的节点数级分配**(不是"该阶段 1/3",那个数字无来源且与 profiler 结论矛盾)。
**风险**:**高**(字节敏感:`shield` 只在真值时写、键序必须一致)。
**验证**:四样本 SHA **逐字节不变** + `alloc_count` 下降;**两者缺一即不做**(不做例外)。
**回退**:单提交 revert。
**工作量**:1 天(含判据)。
**评审处置**:⚠️ **部分采纳** —— 采纳判据与收益改写、采纳"禁止手写序列化器"的约束。

**落地(2026-10-02,三个探针实测 ⇒ 判不做)** —— 只出结论,**`src/` 零改动**(探针全部撤销,
`git diff` 为空;`BACKEND_REQUIRE_BENCH=1 … convert_bench --ignored` 复跑仍绿):

1. **整趟删掉的上界**(探针 P1:`fill_shield` 整体短路;窗口/P2 口径不变,只改这一处):
   `kitten4-10.8MB` **2125796 → 2096644 次(−29152 = −1.37%)**、`kitten4-0.3MB` **68665 → 67707(−958 = −1.40%)**;
   `kn-9.4MB` / `kn-3.7MB` / `nemo-3.4MB` / `nemo-old-1.5MB` **逐位不变** —— `fill_shield` 只被
   `tree_to_json`(正向 Kitten4→KN 的编码端)调用,反向与 NEMO 各走自己的编码器。
2. **"走的成本"与"补键的成本"分开量**(探针 P2:保留遍历、只停掉那次插入):6 个样本 × 两条腿与 P1
   **逐位相同** ⇒ **遍历本身零分配**;那 −1.37%/−1.40% **全部来自每节点一次 `"shield".to_string()` 插入**。
3. **"合并两趟"省不掉那一项——直接用实测代答**:把 `BlockJson::shield` 的
   `skip_serializing_if = "is_false"` 去掉(serde 每节点都写 `shield`,于是 `fill_shield` 可整段删掉;
   这是**不写序列化器**的唯一落地路线),探针读数(同一次跑,`fill_shield` 已短路):
   `kitten4-10.8MB` **2125796 次**(与基线**一字不差**)、`kitten4-0.3MB` **68665 次**(同样一字不差);
   `kn-9.4MB` 1009679(+13880)、`kn-3.7MB` 267626(+3226)、`nemo-3.4MB` 1652769(+26422)、
   `nemo-old-1.5MB` 514071(+8750)—— 后四个还**改了产物字节**(见下)。
   ⇒ 合并后的写法仍要为每个节点建 `"shield"` 键(serde 的 `Map<String, Value>` 以 owned `String` 为键)
   ⇒ **分配收益 = 0**(不是"小",是"零");而这一趟的**遍历**按 P2 是 0 分配,CPU 侧也与
   rounds/37 §10.5"自家函数单条 <0.4%"同向,没有可争的量级。
4. **该路线本身也不满足判据**:`BlockJson` 是**共享**结构体,`to_value()` 也被反向与 NEMO 编码器用 ⇒
   去掉 `skip_serializing_if` 后实测 **4/6 个样本产物变了**(`kn-9.4MB`、`kn-3.7MB`、`nemo-3.4MB`、
   `nemo-old-1.5MB`;只有本来就每节点带 `shield` 的两个 Kitten4 正向样本不变)⇒ 违反
   "6 样本 SHA256 逐字节不变"。要做成"只正向写",就得给正向路径包一层自定义 `Serialize`/新类型
   = **P6 家族 + 新抽象层**,两条都被明令禁止。
5. **结论:判不做**。依据不是"收益小"(上界 1.4% 分配、且集中在 2/6 个样本),而是**可合并掉的那部分
   实测为 0**:真正的成本是"每节点一个 `shield` 键",合并改不掉它,只能改掉一次零分配的遍历。
   若将来真要动这一处,前提是"允许手写序列化器(wrapper)+ 接受抽象层增加",须重新立项。

---

### W10【功能·P2】容错:内联对象形态的影子 🟡
**证据**:`pipeline.rs:221-224` 直接拒绝该类作品;`:218-220` 注释自认"支持对象形态 = 把对象转成影子 XML,列在待办里";真实样本 `A28社区-开幕_174408420`(已在 `download/compile/k4edit/` 语料)。
**做法**:写出"对象影子 → 影子 XML"的转换,替换拒绝分支;范围只做 **Kitten4 → KN 正向**。
**收益**:一类真实作品从"报错拒收"变"可转换";扫描器里"跳过:暂不支持"的计数会变小(可观测判据)。
**风险**:中(新增写出形态);必须证明**既有样本字节不变**。
**验证**:该件语料的往返 + 最小复现单测 + 四样本 SHA 不变 +(建议)实机门。
**工作量**:1 天。
**评审处置**:✅ **采纳**(无异议)。

**落地(2026-10-02,`66c0b6b`)** —— 对象形态影子**已容错**,该件作品不再被拒收:

- **做法**:正向入口(`parse_forward_item`)在解析前调 `pipeline::normalize_object_shadows`,把 `shadows` 里
  是**对象**的槽就地改写成**平台同款影子 XML**;没有对象影子时只做一次廉价探测、**零拷贝**。改写只作用在
  **解析用的副本**上(`Option<Value>` ⇒ `&mut source` 不变),源文档一字不动。范围:仅 Kitten4 → KN 正向。
- **目标形态的依据(都是平台自己的东西,不是自创写法)**:
  - 形态依据 = **同一件作品的平台原件** `download/compile/k4edit/174408420-*.bcm4`(`ide/load` 拿到的
    编辑器亲手写出的源文件),按影子 id 配对逐槽比过 **800 对**:影子元素属性集与对象键一一对应;字段名取
    `fields.name`、文本取 `fields.text`、其余字段键(`constraints`/`allow_text`/`has_been_edited`…)当**字段
    元素的属性**;空文本自闭合 `<field …/>`。来源与全语料形态一致(该对象形态全语料**仅此一件**);
  - 这套写法**也正是本管线自己合成影子**时的形态(`xml::math_number_shadow`、`nemo_mapping::render_shadow_xml`),
    与 KN 侧语料(`download/compile/*.bcmkn`)一致 ⇒ 产物里不会多出第二种影子方言;
  - **只当必要条件**:官方转换器对对象影子**根本不支持**,`BcmHelpers.validateBcm` 也**区分不了**这两种方言
    ⇒ "平台原件长这样"不足以证明写法正确,最终判据仍是产物过 `validateBcm` + `[id台账]` 读数。
- **实测**:`A28社区-开幕_174408420` 由"报错拒收"→ **转换成功**:源积木 **6029**、告警 **19** 条、
  官方 `BcmHelpers.validateBcm` = **VALID**;语料扫描里该作品的 `[跳过]` 行消失(**全语料 0 条跳过**)。
  既有 6 个基准样本的产物**字节未变**(改写只在副本上,对已是字符串形态的作品是零拷贝探测)。
- **两处如实残留**:
  ① 对象里没有 `inline`/`deletable`(平台侧由块定义/实例状态决定)⇒ 这两个渲染属性**丢**;
  ② 平台在**字符串**形态里对空槽写 `""`,而仅凭对象分不出该写 `""` 还是 `<empty>` ⇒ 统一按 `editable=false`
     写平台的 `<empty … editable="false">`(保住 id 与 `editable` 两个事实;`""` 里没有 id ⇒ 占位影子的 id
     会整批丢)。**量法与读数**(写在同一处代码注释里 = `pipeline::object_shadow_xml` 的 `!editable` 分支):
     把那一支临时改成 `String::new()`,跑 `cargo test --lib k4_corpus_round_trip_sweep -- --nocapture`,
     读该件作品的 `[id台账]` 影子列 —— **603 → 943**(+**340** 条);改回 `<empty …>` 仍是 **603**。
     ⇒ `<empty …>` 比 `""` 多保住 **340** 条占位影子 id(515 − 340 = 175 条的槽本来就被正向映射重写,
     两种写法都保不住)。
- **台账**:`LOST_ID_BUDGET` 为它新增一行 **194/603**(`A28社区-开幕_174408420.bcm4` —— W10 起不再被拒收,
  于是第一次进台账)。高读数由**这份老形态的内容**造成,不在"对象 → XML"的改写上:老形态把槽默认值
  **实体化成对象/子块**(515 个 `logic_empty` 占位对象 + 58 个 `default_value` 占位块),而正向映射对这些槽
  本来就会**重新合成**默认影子(现铸 id)⇒ 源 id 被换掉。对照(同一件作品的两份形态,"源影子 id 未出现在
  往返产物里"的条数):对象形态 3653 条里丢 **603**、平台字符串形态 2237 条里丢 **114**;同件作品的平台
  新形态(`k4edit/174408420-*`)读数**不变**(45/88)。对象 → XML 的改写只可能**保住** id(它不删也不重铸任何影子)。

---

### W11【架构健壮性·P2】资源下载:失败清单结构化 🟠
**证据**:`decompile/mod.rs:694` `let url = line.split(": ").next().unwrap_or_default();`,失败串由 `:665/672` 的 `format!("{}: {}", task.url, error)` 拼成。
**做法**:`failures: Mutex<Vec<(String, String)>>`(url, error),重试直接用元组。
**收益/真实失效模式(评审更正)**:URL **含 `": "`** 时才会失配(错误文本含不影响 —— `split` 取第一个 `": "` **之前**的段);失配的后果是**该文件不被重试**,但**仍会被 `warn!` 逐条报出、不崩溃** ⇒ **不是"静默漏文件"**,优先级按 rounds/29 §2 的裁定为低。本项只是便宜的结构化硬化 + 顺带补 `decompile` 侧离线单测。
**风险**:低。
**验证**:离线单测 —— 自造 `HttpClient` 桩(对某 url 首次 `Err`、再次 `Ok`),断言重试成功 + 失败清单成对(参考 `nemo_tests.rs:767` 的 `OfflineHttp`)。
**工作量**:3 小时。
**评审处置**:⚠️ **部分采纳** —— 采纳"承接 `rounds/29 §2` 的 P3 待办"的定性(不再自称新发现)、采纳失效模式更正与优先级下调(原 P0 → P2);保留修法与验证。

---

### W12【架构·P2】结构小节
- **12a `nemo_mapping.rs` 体量记账(按 C2:不拆)**:文件头加一行"2616 行,超 ≈2500 软上限;**intentional**,依据 `docs/rounds/31 §2`(目标表就写 `~2675`)"或按拍板结果执行。
- **12b `model.rs` 测试归位**:`:221/:280/:481/:821` 四个测试模块移到文件末尾(与 `repo-conventions` §5 对齐)。
- **12c 失效注释家族清理**(评审指出实际 ~20 处,不是 2 处):`来自 src/…` 面包屑 + `原 *.rs` + **两条断链 rustdoc** —— 清单见 §1.6;`nemo_mapping.rs:17` 与 `:2057` 指向的 `tables_gen_nemo` 是 **C2 的历史证据**,改文案时要保住这条来龙去脉。
- **12d `docs/knowledge/repo-conventions.md` §4 勘误**:删/改"`DecompilerError` 仍自带 `Io/Json/Http`"那句(实测已无,见 §1.3)。
**收益**:规矩一致、断链消失、阅读噪声下降。**风险**:极低(纯注释/文档/测试归位)。**验证**:`cargo test` + 四样本 SHA 不变。**工作量**:半天。
**评审处置**:⚠️ **部分采纳** —— 12a 由"拆文件"改为"记账"(C2 否决);12b/12c **采纳并扩清单**;12d **新增采纳**(评审 §4⑤)。

---

### W13【保真·待授权】`wrap_arithmetic` 移动不重铸 🔴🟡
**证据**:`docs/rounds/38 §8` 自己挂的残留 —— "把 `wrap_arithmetic` 从'移动 + 重铸'改成'移动不重铸',往返 id 即稳定,台账能压到近 0 —— 动的是**照官方的那一行**,要单独立项 + 实机/基准验证"。`LOST_ID_BUDGET` 基线里 `原气骑士` 408 个真块丢失中约 **342** 条来自它(rounds/38 §6)。
**做法**:待立项。要点:① 确认官方确实给被包节点重铸 id(改它就是**偏离官方**);② 会改产物字节 ⇒ 需 SHA 逐字段解释 + 实机门 + `LOST_ID_BUDGET` 重刷。
**处置**:**暂缓并登记(2026-10-02 拍板)** —— 收益主要是"我们自己的往返 id 台账更干净 / 往返 id 更稳",**不是用户可见差异**;代价是**偏离官方实现 + 改产物字节 + 需重做实体机与刷基线** ⇒ **不列入执行队列**,待重新立项。证据:`rounds/38 §8` 与本节。
**评审处置**:✅ **采纳评审 `§4④`**(原方案把这条漏了,属盘点不完整)。

---

## 3. 性能小结(诚实版)

| 项 | 判据来源 | 结论 |
|----|----------|------|
| 自家函数热点 | rounds/37 §10.5(全部 < 0.4%) | **不做**:P2/P3/P5/P6/P8–P11 实测落在 ±2% 噪声内,不重开 |
| 真正瓶颈 | `Value` 中间树构造/析构 + `BTreeMap` 插入(libc 40.7%) | 只有"少建中间 Value"有意义,而这需要**判据** ⇒ **W7** |
| 可指认的浪费 | W8(NEMO 重复解析)、W9(多一趟全树遍历 + 节点数级键分配) | **先量后改**:W8② 已做(NEMO 样本 −3.4% 字节)、W8① 判不做;**W9 已量**:①整趟删掉的上界 −1.4% 分配(仅 2/6 样本)②可合并掉的"遍历"实测 **0 分配** ⇒ 判不做(见 §W9 落地段) |
| 已消除的浪费 | `save_raw`(20 ms/次)、P10(少一个 RTT) | 已完成,不复提 |

**性能纪律(沿用 rounds/37 §6.3)**:SHA 是绊线不是等价证明;任何"少 clone/少遍历"的改动必须给**同轮 A/B 或分配计数**;单轮绝对毫秒跨轮会漂 20–40%。

---

## 4. 明确"不做"(防重开)

| 项 | 理由(锚点) |
|----|-------------|
| 手写去掉 `#[serde(flatten)]`(P6)、`BTreeMap`→`HashMap`、`RawValue` 顶层透传、单遍遍历合并、反向实体级并行、逐字节对齐官方、两套批处理执行器合并、`mapping+model` 合一、按方向拆 `assembly/mapping`、`kitten4_vocab` 并入 `mapping` | rounds/31 §3 / rounds/37 §3.3(含 §3.2 Q6 的批处理四项语义差异)、rounds/37 §10.6(P6 实测回退:逐字节等价、无收益、+150 行)、rounds/26 §6、rounds/25 §10 |
| 合并两份 `tree_to_json`(`model.rs:1776` vs `nemo.rs:1416`) | Q4 已决:model 版补 `shield`、NEMO 版不补 ⇒ 合并即改字节。**只修静默吞错**(W6②) |
| 合并三处 `math_number` 影子字面量 | **锚点更正(评审)**:第三处是 **`model.rs:1429`**(`NUM` + `allow_text`,`rewrite_call` 内),而 `model.rs:1768` 是 **`default_value`** 影子(`:1765 default_value_shadow`);第一处 `xml.rs:109`、第二处 `model.rs:1068 VALUE_SHADOW_XML`(多行、无 xmlns、字段名 `TEXT`)。各处在不同官方上下文里逐字节锚定 ⇒ 统一格式化就是改字节。rounds/31 §3.6 D4 记的就是 `model.rs:1431` |
| `translate_file` 的 `read_to_string` → `from_reader` | 取舍:serde_json 的 `from_reader` 对大文件比 `from_str` 更慢,是"峰值内存换 CPU";当前无内存压力 |
| 把 convert 公共错误统一成 `MewError` | `MewError`(`utils/requests.rs:21-35`)无通用变体 ⇒ 会丢 `Lossy{report}`/`Unsupported{from,to}` 语义,且是公共面破坏 |
| 拆 `convert_nemo_document`(`nemo.rs:64-573`,≈510 行) | **暂不做**:NEMO 现无字节门,拆无门的行为更难回归。顺序应是 W6 → W7 → 再评估 |
| **拆 `nemo_mapping.rs` 的 551 行表**(原 W12a) | **不做**(C2 已决,**2026-10-02**):`rounds/31 §3.5②` 是**已评审并执行**的决定("表并入本模块、不进生成物文件"),`repo-conventions` §5 的 ≈2500 只是**软**上限、2616 在已批准的容忍带内 ⇒ 不重开。文件头已加一行体量记账(见 §5-C2) |
| 继续合并 5 份 `Fetcher::new` 壳 / 测试里 6 份 `fn walk` | rounds/37 M1 已判("收益是一处定义");测试局部性 > DRY,且各 `walk` 口径不同 |
| `find_object_shadow` 预扫描惰性化/删除(P4) | rounds/37 §4 已判不做 —— 理由是惰性化会把"对象影子作品"从**拒绝**变成**先尝试解析、可能被接受**,即**悄悄改对外行为**。⚠️ **该语境已变**:本轮 W10 已**显式**落地这类作品的支持(`66c0b6b`,见 §W10 落地段)⇒ "接受对象影子作品"是刻意做的,不是惰性化的副作用。**P4 的惰性化本身仍未做**(现在它只是纯性能项,不再是行为改变项) |
| Coco/Wood 反编译路径的样板合并 | 这两个编辑器是死端(不支持互转、无建作品端点),投入产出比低 |
| 扫描器"差异类别集合"升格为门 | 见 W3c:gitignored 增量语料 ⇒ 必然假红;与仓库"按文件名索引 + 表外取表内最大值"的门口径不合 |

---

## 5. 待拍板:C1 / C2 / C3

> **状态(2026-10-02,已按证据拍板并落地)**:C1 = **做**(✅ `eea82bf`)、C2 = **不拆**(✅ 文件头体量记账)、C3 = **做**(✅ `6027b18`+`fb799b6`);
> 另 **W5② 删公共枚举变体 = 已授权**(✅ `fef30e7`)、**W13 = 暂缓并登记**。逐项状态见 §0.3。

### C1 —— W4 动公共枚举(`TranslateWarning` 加字段)
- **支持(作者 + 评审一致)**:① `report.rs:24-26` 的"冻结协议"注释**并未禁止**这次改动 —— 它原文是"要改先改那边",W4 正是同批改生产端 + 消费端;要做的是**把注释改写成新协议说明**。② 退化为"只改 `category()` 标签 + `marker_counts` 读数为 0 即 fail"**不解决问题**(文案再变哪怕只是空格,自检的判据同样静默失效)。
- **反对面(已考虑)**:`TranslateWarning` 是 `pub`,加字段会破坏下游穷尽 `match`;集成测试与 `tests/convert_live.rs` 的类别聚合要同步。
- **建议**:**照原设计做**(同批改消费端,并删掉原方案的 `to_json()` 加料)。消费端清单:`assembly.rs:881/1003-1010`、`pipeline.rs:772-808`、`mapping.rs:1322/1349`、`report.rs:53/65`、`reverse_tests.rs:89/99/138/797`、`convert_live.rs:257/274/280/367`。
- **已决(2026-10-02,按证据拍板):做**。理由:不改成结构字段,门的读数就得**反解中文文案**,一次改文案就会让 `MARKER_BUDGET` 静默变 0;预 1.0 给 `TranslateWarning` 加两个字段可接受;同批改消费端。落地:**W4** ✅ `eea82bf`(C1 由该提交完成;详见 §W4 落地段)。

### C2 —— W12a 是否把 `nemo_mapping.rs` 的 551 行表拆成新文件
- **支持拆分(原方案)**:2616 > ≈2500 软上限;`repo-conventions` §5 有"生成物单独一处"的规则。
- **反对(评审,更强)**:**这不是"方向相反",而是重开一条已决且已执行的条目** —— `rounds/31 §3.5②` 明写"`tables_gen_nemo` 改并入 `nemo_mapping`(**不进**生成物文件)",`§2` 的目标表就写 `translate/nemo_mapping.rs ~2 675`,并在 `§6` 记了执行(`41470d9`);`repo-conventions` §5 用的也是 "≈ 2 500" ⇒ 2616 在**已批准的容忍带**内。拆分还会让 `nemo_mapping.rs:17/2057` 那句指向 `tables_gen_nemo` 的历史注释"重新变成真的",把已否掉的布局复活。
- **建议**:**不拆**;改为在文件头加一行体量记账,并修掉两条断链注释。若你坚持拆,那是"重开 `rounds/31 §3.5②`",须按仓库纪律写明授权与理由,不当作 W12 的子项顺手做。
- **已决(2026-10-02,按证据拍板):不拆**(采纳评审:`rounds/31 §3.5②` 是**已评审并执行**的决定,`~2500` 是软上限、2616 在已批准的容忍带内)。收尾动作已做:文件头加了一行体量记账(`nemo_mapping.rs`,本次注释提交);两条断链注释此前已修(`e8e19d6`/`73b749d`)。

### C3 —— W2 `shared.rs` 重划线是否执行
- **支持(作者 + 评审)**:两条**硬性**分层反转是事实(`shared → api::work`、`shared → translate::tables_gen`),与 `convert/mod.rs` 文档里"`shared` 是两子域共用的地基"直接冲突,不是风格偏好。
- **反对面**:与 rounds/31 的"合并"方向相反(那次把 `shared/{mod,error,model,config,infra,upload}.rs` 合成一个文件),文件数会回增。
- **建议**:**做**,但按 §2 的两处修正:① W2a 里 `bcm_version` 的兜底语义**不动**(避免新增 `decompile → translate` 跨子域依赖或复制常量)——把 `upload.rs` 定位为**域级工具层**;② W2b 的保留集按**实际消费者**重定(评审已给出实测表)。若你更看重文件数,W2a 可退化为"`shared.rs` 内分段 + api 依赖改参数注入"。
- **已决(2026-10-02,按证据拍板):做**。理由:两处**硬性**分层反转是事实(`shared → api::work`、`shared → translate::tables_gen`),与"`shared` 是两子域共用的地基"直接冲突。按评审修正后的画法落地:① **以实际消费者划线**定保留集;② `bcm_version` 的兜底走**域级工具层**路线(`upload.rs` 与门面同级、允许依赖 `translate::tables_gen`,地基 `shared.rs` 保持零反向依赖)。落地:**W2** ✅ `6027b18`(W2a)+ `fb799b6`(W2b);详见 §W2 落地段。

---

## 6. 实施顺序、并行性与验证矩阵

> **状态(2026-10-02,已全部拍板并落地)**:阶段 1 全部交付(W1 `b0af619`、W3a/W3e `f62aec7`、W3b `305a108`、W3c `e8e19d6`、W5 `6fe2ee0`+`fef30e7`、W12b `e8e19d6`、W12c 部分、W12d 本次);阶段 2:W3d(`d10d0cb`/`ef37978`)、W6(`cc320f0`/`c7c40d5`/`0103d7c`)、W11(`2d61959`)、W2(`6027b18`+`fb799b6`)均已交付;阶段 3:W7 ✅ `22897ef`、W8 只做②(`6057a88`)、W9 判不做(`06036a7`);阶段 4:W10 ✅ `66c0b6b`、W4 ✅ `eea82bf`、W12a 按 C2 **不拆**;阶段 5:W13 **暂缓并登记**。详见 §0.3。

```
阶段 1(零风险,1 天内):W1 | W3a/W3b/W3c/W3e | W5 | W12b/W12c/W12d
        ├─ 文件组:G-model、G-tests、G-translate
阶段 2(边界收口,1~2 天):W2a → W2b | W6 | W3d | W11
        └─ W2a/W2b 同动 shared.rs ⇒ 串行;W6 与 W3 同动 tests/reverse_tests ⇒ 串行
阶段 3(可证性,1 天):W7 →(W8 | W9)
阶段 4 ✅(2026-10-02 全部拍板并落地):W4 ✅ `eea82bf` | W10 ✅ `66c0b6b` | W12a(按 C2 **不拆**)
阶段 5:W13 **暂缓并登记**(待立项)
```

**并行性**:`G-model`(W1/W9/W12b)与 `G-shared`(W2)、`G-decompile`(W11)、`G-nemo`(W8)动**不同文件**,可并行;
`G-tests`(W3/W6①/W7)与 `G-report`(W4)都动 `reverse_tests.rs` ⇒ 与 W3/W6/W7 **串行**;
**并且 `TranslateWarning` 枚举本身的改动(W4 与 W6②)也要串行**(评审 §2-②: `remap_warning` 是穷尽 match,两处会互踩)。

| 改动类型 | SHA 基线 | 语料扫描器 | 分配门(W7) | 实机门 | 备注 |
|----------|----------|------------|-------------|--------|------|
| 纯搬迁(W1/W2/W5③/W12b/c) | 必须 | 必须 | — | 不需要 | 编译即证路径不变 |
| 门/仪器(W3/W6/W7) | 必须(新增键要重刷并解释) | 必须 | 必须 | 不需要 | 门本身要"反向自检"(人为造红) |
| 公共枚举(W4/W5②) | 必须 | 必须 | — | 建议 | 同批改消费端;需授权 |
| 行为可见性(W6②/W10) | 必须 + 逐字段解释 | 必须 | — | **建议** | 新增/改变的形态要最小复现 |
| 性能(W8/W9) | 必须 | 必须 | **必须下降** | 不需要 | 测不出即判不做 |

---

## 7. 评审与修订记录

**评审**:独立评审者(未参与撰写),只读;逐条读代码核对锚点(未跑 `cargo`),并核对 `docs/rounds/{25,26,29,30,31,34,36,37,38}` 与 `docs/knowledge/*`。
**结论**:方案事实底子扎实(18 个文件行数逐个准确;抽查 30 条锚点:❌ 1 条、⚠️ 6 条、其余成立),**方向同意**;但指出四类会让实施走偏的问题。

### 7.1 逐条处置

| 项 | 评审 | 本方案处置 |
|----|------|-----------|
| W1 | 接受 | ✅ 采纳(补:同步 `xml.rs` 头注释) |
| W2a | 需改(`bcm_version` 有第二个调用点,`rounds/30 §4` 的兜底语义不能动) | ⚠️ 部分采纳:采纳更正;修法改用"域级工具层"居中方案(见 W2a) |
| W2b | 需改(保留集判据错:`batch_map`/`HttpClient`/`CryptoService`/`ResultExt`/`ValueExt` 在 translate 生产码零使用) | ⚠️ 部分采纳:按实际消费者重定边界并纳入评审建议(含 `nemo_tests` 的 decompile 侧用例迁走) |
| W3a | 需改(NotFound 恒 panic 会断掉 REFRESH 首跑;且没提"部分样本缺失") | ⚠️ 部分采纳:改为"NotFound 且非 REFRESH ⇒ panic";"部分缺失"并入 W3b |
| W3b | 接受 | ✅ 采纳(含评审补充:`convert_work_bench` 的第三个 `strict_mode`) |
| W3c | 需改/半否决(集合门在 gitignored 增量语料下必然假红;`allowed_entity` 直接删即可,活的那份已存在) | ⚠️ 部分采纳:删死副本、**不新增集合门**(理由写入 W3c 与 §4) |
| W3d | 接受 | ✅ 采纳(补"表外取表内最大值"兜底口径) |
| W4 | 接受但删 `to_json()`;C1 按原设计做 | ✅ 采纳(C1 定案;`to_json()` 已删) |
| W5 | 需改(删变体是动公共枚举 🔴;`pub(super)` 落点;审计改用 lint) | ⚠️ 部分采纳:三条全部采纳并改写(见 W5) |
| W6 | 需改(NEMO 样本必须能传 `source_version`) | ⚠️ 部分采纳:采纳必填参数更正(见 W6①) |
| W7 | 需改(计量窗口未定义、跨机不可复现、自检不可实施) | ⚠️ 部分采纳:判据按评审重写,自检改为 `fill_shield` 早退 |
| W8 | 接受(①须补风险) | ⚠️ 部分采纳:①的风险(峰值内存)已改写,并入"测不出就不做" |
| W9 | 需改("省 1/3"无来源且与 profiler 矛盾;自定义 Serialize 是 P6 家族) | ⚠️ 部分采纳:收益改写为"节点数级分配";**禁止手写序列化器** |
| W10 | 接受 | ✅ 采纳 |
| W11 | 需改(说反了失效模式,优先级无据) | ⚠️ 部分采纳:承接 `rounds/29 §2 P3`,失效模式与优先级已更正(P0 → P2) |
| W12 | 接受(12c 漏 ~25 处;12a 属 C2) | ⚠️ 部分采纳:12a 改记账(C2 否决)、12c 扩清单(含 2 条断链) |
| §4.1 不做清单 | 接受,一处锚点错(`model.rs:1768` 是 `default_value`) | ✅ 采纳:已更正为 `model.rs:1429`,并写明另两处的真实形态 |
| §4.2 C1/C2/C3 | 见评审 §6 | ✅ 采纳:C1 做(原设计)/ C2 否决(不拆)/ C3 做(按修正) |

### 7.2 评审新增、已并入本方案的内容

1. **W3e(新)**:`BACKEND_BENCH_REFRESH=1` 写的是 `fresh`(`convert_bench.rs:381`)⇒ 样本缺失时重刷会**永久删掉那些基线键**(评审 L1(b);这是本轮"门的可靠性"主题下最强的实例)。
2. **部分样本缺失这条真洞**(`convert_bench.rs:239/244-246`)并入 W3b。
3. **`convert_work_bench.rs:39` 的第三个 `strict_mode()` 口径**并入 W3b。
4. **失效注释家族约 20 处 + 2 条断链 rustdoc** 并入 W12c(清单见 §1.6)。
5. **`rounds/38 §8` 的 `wrap_arithmetic` 残留**登记为 **W13(待授权)**。
6. **`repo-conventions.md §4` 的过时句**并入 W12d。
7. **C2 的更强论据**(`rounds/31 §3.5②`+`§2` 的 ~2675 接受值 + 执行记录 `41470d9`)写入 §5-C2。

### 7.3 双方仍有分歧、留给拍板处理的地方

> **2026-10-02 已拍板**:下列两处**都已决**,以 §5 为准(本节保留当时的分歧记录)。
- **W2a 的修法**:评审给的是"让调用方各传各的"两条路(⇒ 要么新增 `decompile→translate` 跨子域依赖,要么复制常量);本方案改用第三条路(`upload.rs` 作域级工具层,允许它依赖 `translate::tables_gen`)。三者取一由 C3 一并拍板 ⇒ **已决(C3 = 做):取"域级工具层"路线**。
- **W12a 的拆分**:已按 C2 改为"不拆"(与评审一致);若你要拆,须显式授权 ⇒ **已决(C2 = 不拆)**。

---

## 依据

- **知识库**:`docs/knowledge/convert-semantics.md`(§5/§5bis/§6/§9bis)、`convert-performance.md`(§2/§3/§4/§5)、`repo-conventions.md`(§3ter/§5/§6)、`work-file-formats.md`、`errata.md`。
- **轮次**:`rounds/37`(架构归位 + P1–P11 与实测 + §5 验收矩阵 + §6.2ter/§10.6 的办法纪律)、`rounds/38`(id 口径、`incompatible_*`、`LOST_ID_BUDGET`/`MARKER_BUDGET`)、`rounds/31`(布局与"合并而非拆分"、§3.5② 的 `tables_gen_nemo` 并入决定、§2 的 ~2675 容忍)、`rounds/29 §2 P3`(下载失败结构化)、`rounds/30 §4`(`bcm_version` 回落常量)、`rounds/25/26`(并行与透传的判定不做)、`rounds/33/34/35/36`(语料、词汇表、groups、剔除量)。
- **代码锚点**:`src/core/convert/{mod,shared}.rs`、`src/core/convert/decompile/{mod,editors}.rs`、`src/core/convert/translate/{mod,model,mapping,assembly,pipeline,xml,options,report,nemo,nemo_mapping,reverse_tests,nemo_tests}.rs`、`tests/convert_bench.rs`、`tests/convert_facade_bench.rs`、`tests/convert_work_bench.rs`、`Cargo.toml`。
- **独立评审**:`convert 域架构精进方案 —— 独立评审`(2026-10-01,§0.1 逐条裁决、§1 锚点核对 30 条、§2/L1–L3、§5 可执行性、§6 C1/C2/C3 建议)。
