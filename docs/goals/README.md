# 目标库(要做什么)

> 这里只放**还没做完的事**:待决策、待实现、待核验、已决不做。**事实与结论**放 `../knowledge/`。
> 历史过程放 `../rounds/`(保真,不改写)。

## 当前主线(按优先级)

1. **等你拍板**:`pending-decisions.md` —— **A 组**只剩 **A4**(NEMO 上传是否真机验证;A1/A2/A3/A5 都已处理);
   与 **B 组** 8 项大方向。**D 组已全部落定**(D1–D5,不再需要拍板)。
   `convert-backlog.md` §1 另有 4 项待决策(B2 / A4 / `entity_concurrency` / 反向并行重开)。
2. **不等决策就能做**:
   - `convert-backlog.md` §2 第 **2 / 4 / 5 / 6 / 7 / 8** 条(0b 只剩"把词表重导 + 整体替换写成可复跑小流程"这一子项;
     0c 的标记量门 `MARKER_BUDGET` 已落地);
   - `convert-backlog.md` §2:反向保真缺口的历史研究路线(已收口到"只剩语义降级"这一步,见 §2 第 0 条);
   - `platform-backlog.md` §1/§2:C9 两项真机实测 + 三条待核验(便宜、能消掉假设);
   - `infra-backlog.md` §2:小改批量(49 处 `unwrap` 硬化、P2 收尾;原先列的 `DecompilerError` 包装已不成立,见 `pending-decisions.md` C2)。
3. **要立轮的大方向**:`pending-decisions.md` B 组(DTO 类型化、kn→nemo、断线退避重连 —— ~~`unused` 告警恢复~~ 已由第 40 轮 R2 完成);
   ~~**`src/main.rs` 改成 `use backend::…`(走库 crate)= 立轮**~~ —— **已落地(2026-10-02)**:
   bin 不再自带 `mod api/core/utils`(公共 API 零改动),bin 侧 `unused` 噪声 **896 → 0**(**896 是重写前**旧 `src/main.rs` 的读数;重写后现行 `--bins` 诊断 = 0,见 `infra-backlog.md` §1.1 的"两棵树"说明)、bin 单测目标 **121 tests/214 s → 0 tests/0.00 s**;
   **`unused` 告警已整体恢复 `warn`**(R2 三阶段,终态三选择诊断 **0/0/0**;口径与逐条处置见 `infra-backlog.md` §1.1)。
4. **已决但暂缓(登记,待重新立项)**:`convert-backlog.md` §5 的 **W13**(`wrap_arithmetic` 移动不重铸)——
   收益是"我们自己的往返 id 台账更干净 / 往返 id 更稳",**不是用户可见差异**;代价是偏离官方 + 改产物字节 + 需重做实体机与刷基线(证据:`rounds/38 §8`、`rounds/39` §W13)。

## 第 40 轮目标(2026-10-02 立)

> 来源:CI 防线落地(`f68c2e6`)后的盘点。**逐项状态在此维护**;落地后回填提交号。
> 本轮范围只含下面 6 条;**明确不动**见表末。

| # | 目标 | 状态 | 说明 / 判据 |
| - | ---- | ---- | ----------- |
| **R1** | **CI 产物步不再可能"空着绿" + 离线门进 CI** | ✅ **已完成(2026-10-02,`f68c2e6`)** | 删掉 artifact 上传步及其矩阵 `artifact:`/`libname:` 键(本仓只有 rlib、bin 是需账号的交互式控制台、无消费方 ⇒ 没有可分发产物;旧步期待 `.so/.dll/.dylib`,`if-no-files-found` 默认 `warn` ⇒ 一直静默绿);新增 `offline-gate`:`fmt --check` + `clippy --all-targets -D warnings` + **逐目标点名**的离线测试(`--lib`/`--test repo_hygiene`/`--test convert_bench`)。**刻意不跑**真机门、不设 `BACKEND_REQUIRE_LIVE`(理由见 `../knowledge/repo-conventions.md` §6) |
| **R2** | **`unused` 告警分阶段放开** | ✅ **已完成(2026-10-02,三阶段 `6414b97` 机械族 / `30216c5` `dead_code` / `8da596d` 收口)** | 终态 `[lints.rust] unused = "warn"`,三选择(`--lib`/`--bins`/`--tests`)诊断 **0/0/0**;独立验证在 HEAD `8da596d`(树干净)上实测:`clippy --all-targets -- -D warnings` **无诊断**、`cargo test` 120 通过、基准门 **6/6 SHA 与基线一致 + 并发/串行同哈希**。**计数(⚠️ 两棵树别混)**:R2 开工时(`47a8c5e` 之后)`--lib` 50 / `--bins` **50**(全部来自被顺带重编的 **lib 依赖单元**,bin 自身 **0**)/ `--tests` 98;另有一个 **`47a8c5e` 之前**(旧 `src/main.rs` 还是第二个 crate root)的旧读数 `--bins` **946**(其中 `main.rs` **896**)/ `--tests` **551**(其中 `main.rs` 453)——那批 bin 侧噪声是**重写**清掉的、**不是** R2 清的,也不能在当前树复现(现行 `--bins` = 0)。阶段 1 清机械族 19 条(+ test 目标侧 8 条),阶段 2 处置 `dead_code` 31 条(**删 13 / `#[cfg(test)]` 10 / `#[allow]`+理由 8**)。两条坑:**组的 `level` 要配 `priority = -1`**(否则 clippy `lint_groups_priority` 直接报错)、**显式 `warn` 挡不住 `-D warnings`**。口径与逐条处置表见 `infra-backlog.md` §1.1;登记待决 3 项见其末(`BlockContext.variable_map` 只写不读 → 建议并入 R4;两个 `client` 注入缝"存而不用";4 个 `pub struct` 字段全是 `pub(crate)`) |
| **R3** | **根块纵向布局 `0 vs 80` 是否真 bug** | ✅ **已完成(2026-10-02,`4072846`)** | 结论:**不是行为缺陷** —— 两者不是同一约定(**同名不同物**),错的是 `model.rs` 那句自称"一致"的注释(已改准)。平台取证(711 个含根块实体):编辑器自排网格是 `0 + 80·k`(起点 0 出现 257/711,起点 80 **零次**),平台步长 80 与我们 220 也不是一套,而根块坐标**非语义**(在语义 diff 的 allow-list 里);A/B 实测把 80 改 0 只让 `kn-3.7MB` 一个基线键变 SHA ⇒ 零行为收益,**数值刻意保持 80/220**。见 `convert-backlog.md` §2 第 8 条 |
| **R4** | **反编译域补离线测试** | ⏳ 待做(第二优先域) | `decompile/` 的回归现在几乎只能靠真机门 ⇒ 给纯函数(`XmlBlockWriter::write_blocks`、`referenced_ids`、`child_input_name` …)补离机断言(`rounds/37` §0.4) |
| **R5** | **加载门离线化**(原 `convert-backlog` §6.3 G5) | ⏳ 待做(**需拍板**) | `validateBcm` headless 进常规测试 + 一个**入库的最小夹具**;`download/` 与官方 bundle 都不入库 ⇒ 先决定夹具形态 |
| **R6** | **先量"少建中间 `Value` 树"的上界** | ⏳ 待做 | 用 `alloc_*` 那把尺子先量;不显著就判不做并**关掉该方向**(避免被反复提起) |

**明确不动**(别再重开):**API DTO 类型化**(`pending-decisions.md` B1)、**W13**(`wrap_arithmetic` 移动不重铸,已暂缓并登记,见 `convert-backlog.md` §5)、以及 `rounds/31 §3` / `rounds/37 §3.3` / `rounds/39 §4` 已判不做的清单。

## 文件

| 文件 | 内容 | 条目量 |
| ---- | ---- | ------ |
| `pending-decisions.md` | 需要你决策:A 组(剩 A4)+ B 组 8 + C 组 9 + 已决清单(D 组 5 项已全部落定) | **待决 18**(共 28 行;A1–A3/A5、C2、D1–D5 已定) |
| `convert-backlog.md` | 转换域(格式/语义/性能/NEMO):§1 待决策 4、§2 待做 6、§3 不做 4 | **待决 14**(§2 待做 = 第 2/4/5/6/7/8 条) |
| `platform-backlog.md` | 平台/接口域:§1 真机实测 5、§2 待核验 3、§3 待方案 5 行(其中 2 项 ✅)、§4 P2 3、§5 公开面 1 | 17 行 |
| `infra-backlog.md` | 仓库工程:§1 待决策 3(「去第二个 crate root」「`unused` 告警」「CI 产物」三项已完成)+ §1.1 `unused` 落地记录(0 残;待决 3 项在其末)、§2 小改 3、§3 待核验 3、§4 已决 6、§5 文档 3 | 58 行 |

> 统计口径:来自 `docs/rounds/01–19` 的自动抽取(未开始 36 / 进行中 1 / 被阻塞 1 / 已放弃 16)
> 与 `docs/rounds/20–36` 的手工归并(33–36 轮的结论见 `../knowledge/errata.md` 的"第三十三至三十六轮"节);**已完成的 33 条不再列入**(它们在轮次记录里)。
>
> **本次点数口径(2026-10-01)**:按文件里现存的**表格行 / 编号条目**逐个点;标 ✅ 的计入"共 N 行"但**不计入**"待决"。
> 各文件的明细:`pending-decisions.md` = A 5 + B 8 + C 10 + D 5 = 28 行,其中 A1/A2/A3/A5、C2、D1–D5 共 10 项已定;
> `convert-backlog.md` = §1 6 行(2 项 ✅)+ §2 17 条条目(7 条 ✅ + 4 条属"现行口径/已收口"叙述,实为待做 6)+ §3 4 条"不做"。

## 维护约定

- 一项做完:在本库把它删掉,并在对应的 `../knowledge/` 或一个新轮次记录里写下**结论与证据**。
- 一项决定:把结论写进本库对应条目(标 ✅ 与日期),需要长期遵守的写进 `../knowledge/repo-conventions.md`。
- **不要**把"事实"写进目标库,也不要把"待办"写进知识库 —— 两库混了就失去意义。
