# 轮次索引(rounds/)

> 轮次记录是**过程与证据链**(真机实测、抓包、官方 bundle 逆向、逐条评审),**不改写**(约定见 `rounds/20` §6.1)。
> **读之前先查** `../knowledge/errata.md` —— 它集中列出已过时/写错的表述及正确值(含"第三十三至三十六轮的结论变更")。

## 转换域近十轮(28–40,2026-09-25 ~ 10-02)

| 轮次 | 一句话 | 状态 |
| ---- | ------ | ---- |
| [28](28-convert-reverse-fidelity-gaps.md) | KN→Kitten4 往返的**已知保真缺口**首份记录(留置) | 已被 32/33/36 收口(口径三次修正) |
| [29](29-optimization-scan-ledger.md) | 全仓优化扫描(子代理)与处置台账 | 大部分已落地,剩余在 `../goals/*-backlog.md` |
| [30](30-decompile-upload-option.md) | 反编译可选「上传到当前账号」(备份/搬家) | 已落地;NEMO 侧真机验证待决策 |
| [31](31-convert-layout-consolidation-plan.md) | convert 域**文件编排合并**与架构收敛(27 → 13 文件) | 已落地(读老轮次注意路径对照) |
| [32](32-reverse-definition-body-investigation.md) | 反向定义体缺口(6/21)调查:① 口径假象 ② 实体侧真丢 −24 已修 | 已收口 |
| [33](33-corpus-sweeps-and-format-split.md) | 语料扩容 + 双向往返扫描 + 两个**形态陷阱**;残块归一化定性 | 已收口;编辑格式仍缺(需浏览器抓包) |
| [34](34-forward-corpus-and-guard-fixes.md) | 正向吃真语料;判据误伤修复;**§4octies** 平台骨架键 15 个、**§4nonies** 编辑器词汇表 + 实机方法 | 已落地 |
| [35](35-kitten4-groups-fix.md) | `theatre.groups` + 场景 `group_order`:**角色不显示/画布 0 块**的根因与修法(实机验证) | 已落地 |
| [36](36-editor-vocabulary-single-candidate.md) | 块与影子的取名**统一过编辑器词汇判据**;剔除量 942→715 / 398→50 | 已落地 |
| [37](37-convert-architecture-refactor-plan.md) | **convert 域重构**:架构归位(`options`/`report`/`pipeline`/`xml`)、样板/死重量清理、性能 P1–P10 与实测结论、**抓到平台原始编辑格式语料**、差异口径与"三个块数口径"方法教训 | 已实施(Phase 0–4);性能收益实测 ≈0,结构与安全是真收益 |
| [38](38-convert-fidelity-id-caliber.md) | **保真:id 口径**定案 rounds/37 §13 的两条悬案(`lists_get` 大减 = 归一化;`get_midis` 整块消失 = **真缺陷**);反向对"反查不到原类型的占位块"改**保留**(「未收录积木」`incompatible_*`);正向扫描器加 **id 台账门**(只许变小) | 已落地 |
| [39](39-convert-architecture-refinement-plan.md) | **convert 域架构精进方案**(只读调研 + 独立评审):职责错位收口(`model ⇄ xml` 环、地基里的上传编排/影子大表)、门与仪器补洞(**基线缺失/部分样本缺失/REFRESH 丢键**/中文文案协议/反向 id 台账)、NEMO 进门(带 `source_version`)与**分配计数门**;含 W1–W13、不做清单、**C1/C2/C3 已拍板**、评审与修订记录 | **基本落地**(2026-10-02,以该文 §0.3 为准):✅ W1/W2/W3a/W3b/W3c/W3d/W3e/W4/W5/W6/W7/W10/W11/W12b/W12c/W12d;◐ W8(只做②);❌ W9(实测判不做);⏸ W13(暂缓并登记);C1 做(`eea82bf`)/C2 不拆/C3 做(`6027b18`+`fb799b6`) |

| [40a](40-download-timeout-and-body-cap.md) | **下载侧大文件**:请求级超时 `DOWNLOAD_TIMEOUT = 900 s` + 修 `ureq` 的 **10 MB 隐性体量上限**(`BodyExceedsLimit`)—— 改走显式有界的大体通路,超限报可操作的 `MewError::ResponseTooLarge`;顺带查明 `timeout_global` **只管到响应头** | 已落地(`afca96c`) |
| [40b](40-bench-fixture-discipline.md) | **夹具目录纪律**:真机门按默认输出路径把 bench 输入夹具整体覆盖的事故;修法 = 夹具迁到只读 `download/fixtures/`(而非只改门的输出目录);`kn-9.4MB` 基线有据刷新(它是**快照冻结门**,不是可复现门);顺带记 `#meta` 与产物 SHA 的**语义区分**,并修掉"`#meta` 先 panic 遮住产物 SHA 红"的断言顺序(§6) | 已落地(`9290ede` + `636127f`) |

## 更早的轮次(01–27)

`01-websocket-pitfalls.md` … `27-nemo-to-kn-conversion-plan.md`:WebSocket 坑位、四轮评审与整改、
协议合规、API 类型化、风格统一、客户端注入(14–17)、架构评审(18)、命名统一(19)、
Kitten↔KN 转换方案与落地(20–21)、NEMO 路线(22–24)、性能(25–26)、NEMO→KN(27)。
**路径/类型名多数已漂移**,以 `../knowledge/` 与 `../knowledge/errata.md` 为准。

## 转换域"该看哪几篇"

1. 想理解**产物为什么打不开/看不到东西** → `34` §4octies/§4nonies、`35`;
2. 想知道**判据怎么定** → `../knowledge/convert-semantics.md` §5bis(五条隐性契约 + 三种证据方法);
3. 想知道**编辑格式语料/编辑器端点** → `37` §12(逆向 bundle 得到的读写端点 + 采集工具);
4. 想知道**还剩什么没做** → `../goals/convert-backlog.md` 与 `../goals/pending-decisions.md`(A/D 两组);
5. 想知道**"丢没丢积木"怎么量、怎么定案** → `38`(id 口径)与 `37` §13(三个块数口径的教训);
6. 想知道**测试夹具/基线怎么摆、哪些是"快照冻结门"** → `40b`(夹具不得放输出目录)`../knowledge/convert-performance.md`(基线说明);
7. 想知道**接下来要动什么、为什么** → `39`(当前唯一在案的 convert 域精进方案:门/边界的补洞清单 + 独立评审的逐条结论;**C1/C3 待拍板**(C2 已定"不拆"),已交付项见其 §0.3)。
