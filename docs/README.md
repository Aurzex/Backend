# docs 导航

本目录按**用途**分成两块库 + 一块历史记录。迁移前已有的 `NN-*.md`(32 篇:`01`–`32`)演进式轮次文档**原样保留**在 `rounds/`,
只做搬迁与交叉引用重写,内容不改写(约定见 `rounds/20` §6.1「历史保真」)。

## 知识库 `knowledge/`(是什么 / 为什么)

关于平台、文件格式、协议与仓库约定的**耐久事实**,每条带出处。改代码前先查这里。

| 文件 | 内容 |
| ---- | ---- |
| `work-file-formats.md` | `.bcm` / `.bcm4` / `.bcmkn` 三种作品文件的顶层结构、积木 JSON 模型、程序集、加密矩阵、实现陷阱 |
| `convert-semantics.md` | 编辑器间转换做了什么:支持矩阵、官方两阶段管线、改名/影子/程序集语义、官方自带的坑、硬门与不变量、**Kitten4 编辑器的隐性契约(§5bis)**、**编辑格式的读写端点(§9)** |
| `convert-performance.md` | 实测基线、瓶颈归因(请求数 × RTT)、已落地优化与数字、判定不做的优化、基准方法 |
| `nemo-runtime-and-upload.md` | NEMO 作品如何被创建/存储:建作品不需要资源字节、Qiniu 上传、两条路线的现实形态 |
| `platform-and-protocol.md` | 身份/鉴权归因、WebSocket 连接参数矩阵、帧格式与 `41` 语义、tungstenite 三限制与对策、并发与回调铁律、端点族 |
| `repo-conventions.md` | 六协议(API 形态标准)、编码风格、依赖注入与全局状态纪律、错误类型、命名与文件组织、测试门、提交习惯 |
| `errata.md` | **历史文档勘误**:`docs/rounds/` 里已过时或写错的表述及其正确值(路径/类型/行号漂移、被推翻的形态) |

## 目标库 `goals/`(要做什么)

还没做完的事:待决策、待实现、待核验、已决不做。入口见 `goals/README.md`。

| 文件 | 内容 |
| ---- | ---- |
| `goals/README.md` | 当前主线与优先级、条目统计、维护约定 |
| `goals/pending-decisions.md` | 需要你拍板的事(阻塞 3 / 大方向 8 / 小项 10 / 已决) |
| `goals/convert-backlog.md` | 转换域待办 |
| `goals/platform-backlog.md` | 平台/接口域待办 |
| `goals/infra-backlog.md` | 仓库结构/工程待办 |

## 历史记录 `rounds/`(过程)

`rounds/01-websocket-pitfalls.md` … `rounds/40-bench-fixture-discipline.md`(第 40 轮有两篇)—— 每轮一份的方案/评审/整改记录。
**入口先看 [`rounds/README.md`](rounds/README.md)**(28–40 轮的索引 + "该看哪几篇"),再看本篇。
价值在**证据链**(真机实测、抓包、官方 bundle 逆向、逐条评审),以及"为什么当初这么决定"。
读它们时先看 `knowledge/errata.md`:早期文档里的文件名/类型名/行号多数已经漂移。

**文件分布**:知识库 7 篇(`knowledge/`)、目标库 5 篇(`goals/`)、轮次记录 41 篇编号文档(`rounds/`,另有 1 篇 `README.md`;第 38–40 轮是转换域最近的工作)。

> **读老轮次前先做的两件事**:① 查 `knowledge/errata.md`(它集中列出已过时/写错的表述及正确值,
> 含"第三十三至三十六轮的结论变更"一节);② 转换相关的问题先读 `knowledge/convert-semantics.md` 的
> **§5bis「Kitten4 编辑器的隐性契约」** —— 那是"产物能不能在编辑器里用"的判据与证据方法。
