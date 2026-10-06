# 轮次索引(rounds/)

> 轮次记录是**过程与证据链**(真机实测、抓包、官方 bundle 逆向、逐条评审),**不改写**(约定见 `rounds/20` §6.1)。
> **查阅前须先核对** `../knowledge/errata.md` —— 该文件集中列出已过时或写错的表述及正确值(含"第三十三至三十六轮的结论变更")。

## 转换域轮次(28–40,2026-09-25 ~ 10-02)

| 轮次 | 一句话 | 状态 |
| --- | --- | --- |
| [28](28-convert-reverse-fidelity-gaps.md) | KN 与 Kitten4 往返的**已知保真缺口**首份记录(留置) | 已收口(被 32/33/36 三次修正口径) |
| [29](29-optimization-scan-ledger.md) | 全仓优化扫描(子代理)与处置台账 | 大部分已落地(剩余在 `../goals/` 的 backlog) |
| [30](30-decompile-upload-option.md) | 反编译可选「上传到当前账号」(备份/搬家) | 已落地(NEMO 侧真机验证仍待决) |
| [31](31-convert-layout-consolidation-plan.md) | convert 域**文件编排合并**与架构收敛(27 个文件收敛为 13 个) | 已落地(读老轮次注意路径对照) |
| [32](32-reverse-definition-body-investigation.md) | 反向定义体缺口(6/21)调查:1)  口径假象 2)  实体侧真丢 −24 已修 | 已收口 |
| [33](33-corpus-sweeps-and-format-split.md) | 语料扩容 + 双向往返扫描 + 两个**形态陷阱**;残块归一化定性 | 已收口(编辑格式缺口后由 37 §12 补齐) |
| [34](34-forward-corpus-and-guard-fixes.md) | 正向使用真语料;判据误伤修复;**§4octies** 平台骨架键 15 个、**§4nonies** 编辑器词汇表 + 实机方法 | 已落地 |
| [35](35-kitten4-groups-fix.md) | `theatre.groups` + 场景 `group_order`:**角色不显示/画布 0 块**的根因与修法(实机验证) | 已落地 |
| [36](36-editor-vocabulary-single-candidate.md) | 块与影子的取名**统一过编辑器词汇判据**(剔除量读数见 `../goals/pending-decisions.md` D4) | 已落地 |
| [37](37-convert-architecture-refactor-plan.md) | **convert 域重构**:架构归位(`options`/`report`/`pipeline`/`xml`)、样板与冗余清理、性能 P1–P11 与实测结论、**取得平台原始编辑格式语料**、差异口径与"三个块数口径"方法教训 | 已实施(Phase 0–4;性能收益实测 ≈0) |
| [38](38-convert-fidelity-id-caliber.md) | **保真:id 口径**定案 rounds/37 §13 的两条悬案(`lists_get` 大减 = 归一化;`get_midis` 整块消失 = **真缺陷**);反向对"反查不到原类型的占位块"改**保留**(「未收录积木」`incompatible_*`);正向扫描器加 **id 台账门**(只许变小) | 已落地 |
| [39](39-convert-architecture-refinement-plan.md) | **convert 域架构精进方案**(只读调研 + 独立评审):职责错位收口(`model` 与 `xml` 之间的环、地基里的上传编排/影子大表)、门与仪器补洞(**基线缺失/部分样本缺失/REFRESH 丢键**/中文文案协议/反向 id 台账)、NEMO 进门(带 `source_version`)与**分配计数门**;含 W1–W13、不做清单、**C1/C2/C3 已拍板**、评审与修订记录 | 基本落地(2026-10-02,以该文 §0.3 为准) |
| [40](40-gates-cleanup-and-real-defects.md) | **CI 防线 + `unused` 放开 + 根块定案 + 反编译补测 + 性能上界 + 可见性/公共面;下载侧请求级超时与 `ureq` 10 MB 隐性上限;夹具目录纪律 + 断言顺序**(R1–R6、R7、T1/T3 与同批清理的合并稿) | 已落地(`f68c2e6` / `6414b97` / `30216c5` / `8da596d` / `47a8c5e` / `4072846` / `c076918` / `104964f` / `cbb167a` / `60d5358` / `afca96c` / `9290ede` + `636127f`) |

## 工程与文档轮次(41–46,2026-10-02 ~ 10-03)

| 轮次 | 一句话 | 状态 |
| --- | --- | --- |
| [41](41-agent-contract-and-doc-reformat.md) | 根目录 `AGENTS.md` 固化项目级契约,并据此对全部 Markdown 执行格式、状态与语体清洗 | 已落地(剩两条待决口径见该文 §8,登记在 `../goals/README.md`) |
| [42](42-dependency-refresh.md) | 依赖按 crates.io 最高稳定版刷新(直接依赖与 `Cargo.lock` 全量重解),离线门与真机测试全过 | 已落地 |
| [43](43-a-group-real-machine-and-narrowing.md) | A 组落地:**两处协议假设真机消掉**(`currentTime` 秒级 / 字段名 `phone_number`)、**分段超时**(原"body 无读超时"诊断被推翻)、UI 输入错误传播、两处存而不用字段删除 | 已落地 |
| [44](44-third-party-type-containment.md) | **公共面收窄**:`ureq` 类型移出公共契约(自有 `MewResponse` / `TransportError`),三处错误类型收敛为"底层失败一律经 `MewError`"(删 `FileError`) | 已落地(`5e602dd`) |
| [45](45-mechanical-batch-and-verification-closeout.md) | **机械批与核验收口**:认证头预计算、flush 按需唤醒、`AccountStatus` 并入 `Identity`、两个私有 trait 改自由函数、工具函数归位、反向查表索引一致化;infra §3 与 `rounds/21` 遗留核销 | 已落地 |
| [46](46-public-face-closeout-and-notation-rule.md) | **公共面收尾**:`DecompilerError` 改名 `ConvertError`(含 `TranslateError` 变体)、`ReportProcessor` 的自定义配置入口收窄并删零调用构造器、`api::auth` 三处零调用全局门面删除;**映射记法口径定案**(表格内 `A -> B` 属数据记法保留,正文只留代码/数值/签名) | 已落地(`264026f`) |

## 更早的轮次(01–27)

`01-websocket-pitfalls.md` … `27-nemo-to-kn-conversion-plan.md`:WebSocket 坑位、四轮评审与整改、
协议合规、API 类型化、风格统一、客户端注入(14–17)、架构评审(18)、命名统一(19)、
Kitten 与 KN 之间的转换方案与落地(20–21)、NEMO 路线(22–24)、性能(25–26)、NEMO 转 KN(27)。
**路径/类型名多数已漂移**,以 `../knowledge/` 与 `../knowledge/errata.md` 为准。

## 最新落地(48)

| 轮次 | 一句话 | 状态 |
| --- | --- | --- |
| [48](48-nemo-entity-parallelism.md) | **NEMO 方向的实体级并行**:与正向同构的四阶段(临时 id 记录 · 串行兑现 · 并行兑现与编码 · 串行装配);只置不清的上下文按原值播种;QC 迁移的字符串也按同一张 id 表改写 | **已落地**(同轮 1 vs 8:`core` 1.44×/1.21×、`e2e` 1.31×/1.21×;产物 SHA256 与 `#meta` 全绿;代价:分配 +13.9%,见该文 §4) |

## 进行中的方案(47)

| 轮次 | 一句话 | 状态 |
| --- | --- | --- |
| [47](47-data-layer-rewrite-plan.md) | **数据表示层重写**:减少中间 `serde_json::Value` 的物化(Step 1 去 `json!` 深拷贝 · Step 2 流式写出 · Step 3 块树直写 · Step 4 反向/NEMO 对齐 · Step 5 源侧骨架)· 含硬指标、验收矩阵、评审与修订 | **Step 1、Step 2+3、Step 4(反向)、Step 5 已落地**(正向 + 反向四行达标);Step 5b 经复核不立项;NEMO 两行未做 |

## 转换域"该看哪几篇"

1. 如需了解**产物为什么打不开或看不到内容**,见 `34` §4octies/§4nonies、`35`;
2. 如需了解**判据怎么定**,见 `../knowledge/convert-semantics.md` §5bis(五条隐性契约 + 三种证据方法);
3. 如需查阅**编辑格式语料/编辑器端点**,见 `37` §12(逆向 bundle 得到的读写端点 + 采集工具);
4. 如需了解**还剩什么没做**,见 `../goals/convert-backlog.md` 与 `../goals/pending-decisions.md`(A/D 两组);
5. 如需了解**"丢没丢积木"怎么量、怎么定案**,见 `38`(id 口径)与 `37` §13(三个块数口径的教训);
6. 如需了解**测试夹具/基线如何摆放、哪些是"快照冻结门"**,见 `40` §8(夹具不得放输出目录)与 §7(下载侧);`../knowledge/convert-performance.md` §5(基线说明);
7. 如需了解**后续要改什么、为什么**,见 `39`(当前唯一在案的 convert 域精进方案:门/边界的补洞清单 + 独立评审的逐条结论;**C1/C2/C3 已拍板并落地**(C2 = 不拆),已交付项见其 §0.3)。
