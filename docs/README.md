# docs 导航

本目录按**用途**分成两块库 + 一块历史记录。迁移前已有的 `NN-*.md`(`rounds/01`–`rounds/32`)演进式轮次文档**原样保留**在 `rounds/`,
只做搬迁与交叉引用重写,内容不改写(约定见 [`rounds/20` §6.1](rounds/20-kitten-kn-work-conversion-plan.md)「历史保真」)。

## 知识库 [`knowledge/`](knowledge/)(是什么 / 为什么)

关于平台、文件格式、协议与仓库约定的**耐久事实**,每条带出处。改代码前先查这里。

| 文件 | 内容 |
| --- | --- |
| [`knowledge/work-file-formats.md`](knowledge/work-file-formats.md) | `.bcm` / `.bcm4` / `.bcmkn` 三种作品文件的顶层结构、积木 JSON 模型、程序集、加密矩阵、实现陷阱 |
| [`knowledge/convert-semantics.md`](knowledge/convert-semantics.md) | 编辑器间转换做了什么:支持矩阵、官方两阶段管线、改名/影子/程序集语义、官方自带的坑、硬门与不变量、**Kitten4 编辑器的隐性契约(§5bis)**、**编辑格式的读写端点(§9)** |
| [`knowledge/convert-performance.md`](knowledge/convert-performance.md) | 实测基线、瓶颈归因(请求数 × RTT)、已落地优化与数字、判定不做的优化、基准方法 |
| [`knowledge/nemo-runtime-and-upload.md`](knowledge/nemo-runtime-and-upload.md) | NEMO 作品如何被创建/存储:建作品不需要资源字节、Qiniu 上传、两条路线的现实形态 |
| [`knowledge/platform-and-protocol.md`](knowledge/platform-and-protocol.md) | 身份/鉴权归因、WebSocket 连接参数矩阵、帧格式与 `41` 语义、tungstenite 三限制与对策、并发与回调铁律、端点族 |
| [`knowledge/repo-conventions.md`](knowledge/repo-conventions.md) | 六协议(API 形态标准)、编码风格、依赖注入与全局状态纪律、错误类型、命名与文件组织、测试门、提交习惯 |
| [`knowledge/errata.md`](knowledge/errata.md) | **文档与现实不符**的勘误总表:`rounds/` 里已过时或写错的表述及其正确值(路径/类型/行号漂移、被推翻的形态),另含**非轮次条目**(仓库配置、文档读数与体例) |

## 目标库 [`goals/`](goals/)(要做什么)

还没做完的事:待决、待做、待核验、已决不做。入口见 [`goals/README.md`](goals/README.md)。

| 文件 | 内容 |
| --- | --- |
| [`goals/README.md`](goals/README.md) | 指针式索引:拍板清单、不用等决策就能做的事、要立轮的方向、文件一览、维护约定 |
| [`goals/pending-decisions.md`](goals/pending-decisions.md) | 需要拍板的事(A / B / C / D 四组)+ 已决清单 |
| [`goals/convert-backlog.md`](goals/convert-backlog.md) | 转换域待做(§1 待决 / §2 待做 / §3 不做 / §4 防线 / §5 `rounds/37` 之后 / §6 效果目标) |
| [`goals/platform-backlog.md`](goals/platform-backlog.md) | 平台/接口域待做(§1 真机实测 / §2 已核验 / §3 待方案 / §4 P2 / §5 公开面) |
| [`goals/infra-backlog.md`](goals/infra-backlog.md) | 仓库结构/工程待做(§1 待决 + §1.1 `unused` 落地 / §2 小改 / §3 待核验 / §4 已决 / §5 文档) |

## 历史记录 [`rounds/`](rounds/)(过程)

`rounds/01-websocket-pitfalls.md` … `rounds/43-a-group-real-machine-and-narrowing.md` —— 每轮一份的方案/评审/整改记录。
**入口先看 [`rounds/README.md`](rounds/README.md)**(28–43 轮的索引 + "该看哪几篇"),再看本篇。
价值在**证据链**(真机实测、抓包、官方 bundle 逆向、逐条评审),以及"为什么当初这么决定"。
读它们时先看 [`knowledge/errata.md`](knowledge/errata.md):早期文档里的文件名/类型名/行号多数已经漂移。

**文件分布**:知识库 [`knowledge/`](knowledge/)、目标库 [`goals/`](goals/)、轮次记录 `rounds/01`–`rounds/43`(每轮一篇,40 号已合并为单篇,另设本目录索引 `README.md`;第 38–40 轮为转换域的近期工作,第 41 轮为契约固化与文档重构,第 42 轮为依赖刷新,第 43 轮为 A 组实测与收窄)。

> **读老轮次前先做的两件事**:1)  查 [`knowledge/errata.md`](knowledge/errata.md)(它集中列出已过时/写错的表述及正确值,
> 含"第三十三至三十六轮的结论变更"一节);2)  转换相关的问题先读 [`knowledge/convert-semantics.md`](knowledge/convert-semantics.md) 的
> **§5bis「Kitten4 编辑器的隐性契约」** —— 那是"产物能不能在编辑器里用"的判据与证据方法。

## 文档体例

规范原文见根目录 [`AGENTS.md`](../AGENTS.md) 第三节(体例 9 条红线)、第五节(输出禁忌)、第六节(正文语体)。本节不重复条文,只登记两库分工与例外。

- **两库分工**:`knowledge/` 篇末统一 `## 依据`;`goals/` 用表内"出处"列;导航页不写易漂的文件数/条目数。
- **历史稿**:`rounds/01`–`rounds/40` 的正文于 2026-10-02 按 `AGENTS.md` 完成格式与语体清洗,严重度改用文字(`高/中/低`)。此前的"历史稿不改写"约定同时改为"**事实不改写,写法随规范清洗**"。

> **例外(4 项)**:
> 1)  [`knowledge/errata.md`](knowledge/errata.md) 里被记录的 `文件:行` 属于**勘误内容本身**(记录当年的行号与漂移),保留;该文件内指向当前活代码的引用仍按符号定位。
> 2)  [`knowledge/convert-semantics.md`](knowledge/convert-semantics.md) §1 支持矩阵里的支持标记表示"该方向支持",不是完成状态。
> 3)  **引用原文/原标题/用户指令原文**照旧保留其用词("本轮 / 本次"等引文不改,改了就不是引用)。
> 4)  导航与索引页的状态列允许在固定标记之后附加至多一个短句说明(例如 `已完成(结论已收口)`),不引入其它状态词。
