# 第五十二轮记录 — 三库复核与修整(live 文档;历史正文不改写)

## 0. 一句话
按 `docs-tidy` 的顺序对 `docs/` 的三库做一次只读测绘,把 live 文档的漂移(计数、状态矛盾、失效指针、重复展开、表格断行、体例不一)修掉,再换新会话做两轮独立复验并修复验报出的项;`docs/rounds/**` 的正文按"历史保真"不动,其缺陷集中登记进 `../knowledge/errata.md`。

## 1. 任务背景
`knowledge/`、`goals/`、`rounds/` 三库长期增量编辑后出现典型漂移:同一事实多处各写一遍、手工维护的计数失准、一处"已完成"另一处仍"待决"、指针指向已改名/不存在的目标、表格被空行或缩进断成两段、体例(时间表述/连接词/状态词)各行其是。判据全部取成文条文:`AGENTS.md` 第三节(体例 9 条红线)与第六节(正文语体)、`docs/README.md` 的「文档体例」与「例外」块、"单一事实源"(同一事实只在一处展开,其余用指针)。

范围纪律:只动 `docs/`;**历史轮次正文不改写**(`../rounds/20` §6.1 的"历史保真";写法的格式与语体清洗已在第五轮之外的 2026-10-02 那批完成),历史正文里的缺陷按 `AGENTS.md` 第四节写进勘误。

## 2. 只读测绘
按 `docs-tidy` 的七类做,派独立只读 agent,不边读边改:文件清单、重复事实、硬编码计数、状态矛盾、失效指针、索引准确性、格式不一。命中项即下面三批修复清单所覆盖的内容;每一条都在修复时用工具核出实际值(读源码 / `ls` / `git grep -c` / `git log`),不采信文档里的另一个数字。

## 3. 第一批:准确性 —— `e279eee`
- `../knowledge/repo-conventions.md` §5:`core/convert/` 的当前布局计数 **22 改为 23**(补 `translate/source.rs`,该文件由 `rounds/50` 新增;原枚举漏了它),并把 `## 文档规范` 移到 `## 依据` 之前(依据按红线 6 是篇末节);
- `../knowledge/errata.md` 前言:"最晚一节为 2026-10-02"改为 **2026-10-06**(文内已有 49/50/51 三轮的节);
- `../knowledge/convert-semantics.md` §1:跨库链接 `../../rounds/48-…` 改为 `../rounds/48-…`(原写法解析到不存在的仓库根目录);
- `../goals/pending-decisions.md`:B1 的"建议 1) 分批"改为 **判不做**(2026-10-02 起已列入"明确不做",以 `./README.md` 为准 —— 那次只改了入口页,决策库与 infra-backlog 留在旧口径);D6 的归因"第 40 轮 R2 与 T2 报出"改为"R2 报出、其后追加第 4 项"(`rounds/40` 无 T2);
- `../goals/infra-backlog.md` §1:B1/B7/B8 三行合并为指向 `./pending-decisions.md` B 组的指针(消掉重复清单);§4 的 C8 行补"唯一权威落 `./pending-decisions.md` C8";
- `../goals/convert-backlog.md`:正文锚点 `§11` 改为 **§2 第 11 条**(该文只有 §1–§7);依据行的"本库 §1(D1–D5)"改为 `./pending-decisions.md` D 组;§6.3 的 G0 行补出缺失的"验收"列(表头 4 列);
- `../goals/README.md`:第 41/46 轮两段的易漂数字(文件数、编辑行数等)改为指针(该页自己已声明"不复制理由与读数");
- `../docs/README.md`:开头"内容不改写"与体例节"事实不改写,写法随规范清洗"统一口径;「例外」第 4 条补明**索引列允许的短标记**(`已落地`/`已收口`/`已实施`/`基本落地`/`大部分已落地`)。

## 4. 第二批:去重与格式 —— `133a29c`
- `../knowledge/convert-performance.md` §3:表格中间的空行删掉(它把"已完成的优化"断成两张表,后半段没有表头);
- `../goals/convert-backlog.md` §2 第 0 条:两个真作品的读数改为指向 `../knowledge/convert-semantics.md` §5(读数唯一展开在知识库);
- `../knowledge/errata.md`:新增「第五十二轮的三库复核(2026-10-06)」,登记**历史轮次正文**里的三处缺陷 —— `rounds/41`「锚点解析」段自称"58 个失效引用"与实表 **45 行**不一致;`rounds/39` 被语体清洗挤坏的锚点(`§3.5 2)` 变 `§3.52)`、"评审 §4 5)" 变 `§45)`);`rounds/37` §5 验收矩阵的 Phase 2 行只有 3 个单元格(表头 4 列,缺"回退")。

## 5. 独立复验(换会话,只读,两个 agent 分片)
一轮"准确性/自洽/指针/索引"、一轮"体例/去重/格式"。两轮各自给的命中项经本次逐条复核后落入第 6 节;其中被判定为**不修**的见第 7 节。复验方法:读全量 live 文档 + `grep` 模式匹配 + 锚定正则核每张表的列数与缩进 + `git log`/`git grep` 核计数与提交号。

## 6. 第三批:修复验报出的项 —— `e404c67`
计数(以工具核出的实际值为准):
- `../knowledge/convert-performance.md` §5 与「依据」:基准样本构成由"4 Kitten + 2 NEMO"改为 **2 Kitten4 + 2 KN(Neko) + 2 NEMO**(按 `tests/convert_bench.rs` 的 `SAMPLES` 与 `source_editor`);
- `../goals/convert-backlog.md` §2 第 13 条:`convert_bench` 样本数 5 改为 **6**(`convert_facade_bench` 的 4 个正确);
- `../goals/infra-backlog.md` §1.1 与 `../goals/pending-decisions.md` D6:`#[allow(dead_code)]` 现存处数由 6 改为 **4**(两个 `client` 字段已随 D6-2 于 2026-10-03 删除;`git grep` 实测 4 处属性 + 1 处仅注释提及);
- `../knowledge/convert-performance.md` §2bis.3:表头补口径(各行归类互有重叠、八行合计 **125.6%**,只能比量级不能按比例读),并加一条读法说明。

状态(收敛到固定标记集):
- `../goals/README.md` 待决清单删去已完成项 `entity_concurrency`;"B1 与 B2 都已列入明确不做"改为 **B1 已列入、B2 仍待决**(与「明确不做」节、`pending-decisions.md` B2、`convert-backlog.md` §1 三处对齐);
- `已核实应判不做`/`已核销` 改为 `判不做(已核实,…)`/`已完成(…;提交号)`;缺提交号的补 `0ead279`(实体级并发自动)、`c5bbb13`(rounds/43 的文档同步)、`659c030`(rounds/45 的文档同步)、`9ddacbb`(rounds/45 的代码提交);正文里的 `已落地` 改为 `已完成(日期,提交号)`;
- `不做` 统一为 `判不做`(`../knowledge/convert-performance.md` §4 的标题与格、`../knowledge/convert-semantics.md` §7)。

失效/漂移指针:
- `../knowledge/repo-conventions.md`:`rules/47` 改为 `rounds/47`;`convert/shared.rs::DecompilerError` 改为 `::ConvertError`;§1 历史项里的 `KittyRequestBuilder`/`KittyConfig` 改用现名 `MewRequestBuilder`/`ClientConfig`(括注旧名);
- `../goals/pending-decisions.md` B2:自指乱码「见 `./pending-decisions.md` 之外的 route B」改为指向 `../rounds/24` §3.1;
- 表格断行:`../goals/convert-backlog.md` §2 第 8 条的取证表(分隔行在列 0)、`../knowledge/platform-and-protocol.md` §5 的作品编辑器表、`../knowledge/convert-performance.md` §1 的下载并发表,三处分隔行缩进归位。

去重(读数只在知识库展开):
- `../goals/convert-backlog.md` 前言的分步读数、§1 两行的读数、§3 的 `RawValue` 透传读数、§7 的阈值读数改为指针;
- `../goals/platform-backlog.md` §1 五行去掉与 `../knowledge/platform-and-protocol.md` §5 重复的数字与字段名,留结论 + 指针;
- `../goals/pending-decisions.md` 已决段:`RawValue` 透传与 `theatre.groups` 合成的机制改为指针;
- `../rounds/README.md`:把 47–51 的五节(标题为"最新落地 / 更早的落地")并成一张 `## 轮次 47–51(2026-10-05 ~ 10-06)` 表,行内读数改指针。

体例与格式:
- `docs/README.md`:「例外」由 4 项扩到 **6 项**(补"决策表与 backlog 的目标表以单元格内的 `../rounds/NN` 代替独立「出处」列";补"正文里以反引号引用的同目录文件名不写 `./`(它们不是链接)");「文档体例」补**映射记法**的等价与"同表不混用"、以及 `~~` 划线的口径;
- 正文里的 `=>`/`->`/`<->` 连接词改为书面语(含表格外的流程链,如"落盘 → 读回""大改先出方案文档,经子代理评审之后再动 Rust 代码");同一张表内的映射字形统一(§3 表内的三处 `->` 归到 `→`);
- `../knowledge/errata.md` 篇末补 `## 依据`(其余 knowledge 篇都有,原缺)。

## 7. 判不做(登记备查)
- **枚举标记 `N)` 后的双空格**(live 文档 99 处,`rounds/**` 内更多):渲染等效,纯空白的批量改动只增加 diff 噪声,不改。
- **`→` 与 `->` 不逐字统一**(live 文档 121 处 `→`、73 处 `->`,其中大量在表格与代码片段里):改为成文口径"同义、同一张表内不得混用(代码/类型签名里的 Rust 箭头不属映射记法)",只修同表混用处。
- **`rounds/**` 正文不改写**:其缺陷只登记 errata,不修原稿。
- **`errata.md` 与 `rounds/**` 里的引文照旧**(`AGENTS.md` 例外 3):引文里的"本轮/本次"与箭头不动。

## 8. 验证

| 项 | 读数 |
| --- | --- |
| 表格完整性(全部 live md,脚本:逐表核列数与缩进) | 列数/缩进不一致的表 **0** 张(修前 3 张:两处空行断表已于 `133a29c`/更早修,三处分隔行缩进在 `e404c67` 修完) |
| 时间表述扫描(脚本) | 正文里 `本轮`/`本次`/`最新`/`目前` 仅剩 3 处,全部属引文或口径条文自身(`errata.md` 引 rounds/11 原文、`docs/README.md` 例外 3 的举例) |
| 连接词扫描(脚本) | 表格外的 `=>` 仅剩 1 处,属引文(`errata.md` 引旧结论"内联对象形态影子 => 拒收该作品"已改写,现存为 `rounds/39` 变更节里的引号内描述) |
| 状态近义词 / 第一人称(脚本) | `待办`/`待定`/`待你决策`/`待你拍板`/`咱们`/`其实` **0** 命中 |
| `cargo test --test repo_hygiene` | 2 通过(文档改动不触代码;全套门在 `e404c67` 未跑,该提交只含 `docs/`) |

## 9. 未做与遗留
- **文档体例没有自动化门**:第 52 轮的复核靠一次性脚本核(表格完整性、时间表述、连接词、状态近义词),是否固化成 `docs` 侧的门,记在 `../goals/infra-backlog.md` §5 文档类。
- 复验只覆盖 live 文档的可机械核对项;**语义级**的重复(同一结论换一种说法写两遍)不做穷尽,只处理复验点名的组。
- 上游待办不受影响:`../rounds/21` §8.6 的 L1、分片上传、NEMO 完整搬家的资源重传、夹具与输出目录分离仍在目标库。

## 依据
- 提交:`e279eee`(准确性)、`133a29c`(去重与格式 + 历史正文勘误登记)、`e404c67`(按独立复验修整)。
- 体例与判据:`../AGENTS.md` 第三节/第五节/第六节;`../README.md` 的「文档体例」与「例外」块;`../rounds/20` §6.1(历史保真);`../rounds/41`(契约固化与全盘重构)、`../rounds/46` §4(映射记法口径)。
- 计数与提交号的核验命令:`tests/convert_bench.rs` 的 `SAMPLES`/`source_editor`(样本 6 个:2 Kitten4 + 2 KN + 2 NEMO);`git grep -n 'allow(dead_code)' -- src tests`(4 处属性 + 1 处仅注释提及);`ls docs/knowledge docs/goals`(7 + 5 篇);`git log -- docs/rounds/43-*.md`(`c5bbb13`)、`git log -- docs/rounds/45-*.md`(`659c030`)。
