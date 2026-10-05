# 第四十六轮记录 — 公共面收尾:类型改名、悬空入口删除与映射记法定案

## 0. 一句话
把第 43 轮盘点里剩下的三条"不用拍板"的尾巴做完:`DecompilerError` 改名 `ConvertError`(连带 `TranslateError` 的变体),删掉两处零调用的全局门面并把"对外可配"的假签名收窄,最后把悬了很久的映射记法口径定成判据并清掉 live 文档里的 10 行正文连接词。

## 1. 任务背景
2026-10-03 的指令("先做 13")指向三项:`infra-backlog.md` §6 剩的两条公共面项、`../rounds/21` §8.6 的 **L4**(类型改名)、以及 `../rounds/41` §8 第 2 条的**映射记法待决口径**。三项都在目标库里有出处,且都不改动行为语义。

## 2. L4:`DecompilerError` 改名 `ConvertError`

| 面 | 改前 | 改后 |
| --- | --- | --- |
| 类型 | `core/convert/shared.rs::DecompilerError` | `ConvertError`(公开路径仍是 `core::convert::ConvertError`) |
| `TranslateError` 的载体变体 | `Decompiler(#[from] DecompilerError)`,文本"作品文件解析失败" | `Convert(#[from] ConvertError)`,文本"转换域错误" |
| 引用点 | 11 个文件、136 处 | 同名替换后 `cargo build` 直接通过(无别名、无 `#[deprecated]`) |

为何改名:该类型实际承载**反编译 + 转换 + 上传**整域的错误(`decompile/`、`translate/`、`upload.rs` 都在用),`Decompiler` 这个名字窄于职责(原始记录:`../rounds/21` §8.6 的 L4)。历史轮次里的旧名不回改,已登记到 `../knowledge/errata.md`,并把 `../knowledge/repo-conventions.md` §4 的口径锚点改成新名。

## 3. infra §6 剩两条

### 3.1 `CheckConfig` 的"悬空公共面"→ 收窄并删除零调用构造器

`CheckConfig` 的 7 个字段全是 `pub(crate)`、没有公开构造器,因此 `ReportProcessor::new_with_config(config)` / `new_with_config_and_client(config, client)` 对外**只能传 `Default`** —— 签名承诺的"自定义配置"不可达。取"收窄"这一边:

- 两个构造器收 `pub(crate)`;
- 收窄后 `new_with_config` 零调用 ⇒ **直接删除**(`unused` 门当场抓到);`new_with_config_and_client` 仍被 `new_with_client` 使用;
- 对外只留 `ReportProcessor::{new, new_with_client}`;`CheckConfig` 类型本身留在 crate 内由 `ViolationChecker` 消费。

判据:全仓 `ReportProcessor::` 的调用点只有 `new()`(`src/main.rs`)与两个静态助手(`core/terminal.rs`)⇒ 无外部使用者受损。

### 3.2 `api` 层两处零调用全局入口 → 删除

删除 `api/auth.rs` 的 `global_auth_manager()`、`GLOBAL_AUTH_MANAGER`(static)与 `fetch_current_timestamp()`;保留在用的 `fetch_current_timestamp_with_provider(&provider)`。

判据:这三项在全仓(含 `tests/`)只出现在定义处。删除后 `clippy -D warnings` 反过来抓到两个因此变空的 import(`Arc`/`OnceLock`),已一并清掉 —— 这正是"`pub` 项 `unused` 抓不到、只能人工 grep"那条提醒的实证。此次删除同时消掉 `../knowledge/repo-conventions.md` §3 里最后一处"看似可用的全局登录入口"。

## 4. 映射记法口径定案

原待决(`../rounds/41` §8 第 2 条)是"表格单元格里 `A -> B` 要不要统一书面化"。定案前先量了规模(定案前状态):非 `rounds` 文档里**表格内**含 ` -> ` 的 13 行、**表格外**含 ` -> ` 的 27 行;`../rounds/**` 里表格外有 233 行。逐行看下来,这些 `->` 绝大多数是**不能改的**东西:Rust 签名(`fn f(..) -> Result<..>`)、grep 表达式(`grep X -> 0 命中`)、数值/节点数变化(`285 -> 184 ms`)、以及协议字典描述。

因此口径按"表达的是什么"划分,而不是按文件或按写法:

- **表格单元格内**:`->` 是数据记法(量值、节点/条目数、流程、类型签名、旧名到新名的映射对),**保留不改写**;
- **正文(表格外)**:只允许出现在①代码/命令片段、②数值或键值变化、③类型签名里;其余(自然语言连接、流程链、改名叙述)一律写书面语,如"由 A 改为 B";
- **`../rounds/**` 属历史**:按"引用与代码保真优先"**不回改**(233 行里绝大多数命中第①②③类白名单,逐行判定风险高、收益为零;要动就得单独立项并逐行人工过)。

按该口径**编辑了 10 行正文(2026-10-03)**(`errata.md` 8 行:"`utils/acquire.rs` 搬迁链"、"类型改名"、"同类型改 i32"、重命名叙述、"登录/举报/恢复身份"流程链、参数校验改名、反向方向描述;`repo-conventions.md` 1 行:契约指针;`work-file-formats.md` 1 行:字段字典描述)。清理后非 `rounds` 文档的表格外 `->` 行由 27 降到 18(余下含 2026-10-03 新增的口径条文示例两行),其余命中全部落在白名单。口径写进 `docs/README.md` 的"文档体例"节与 `../goals/README.md` 第 41 轮小节。引用原文里的 `->`(如 errata 里标注"改写前原文"的那句)照旧保留。

## 5. 验证

| 项 | 读数 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过,零告警(含上一步抓到并清掉的两处空 import) |
| `cargo test` | 退出码 0:14 个结果行合计 **144 通过 / 0 失败 / 12 忽略**;忽略者按约定全是网络写操作/采集/基准(`convert_bench`、`convert_work*`、`compile_live`、两腿语料采集),`live_features` 的三例真机用例在跑并全过 |
| 公共面残留 | 工作区 `DecompilerError` 零命中(改名后 `ConvertError` 136 处,与改前计数一致);`global_auth_manager` / `GLOBAL_AUTH_MANAGER` / `fetch_current_timestamp()` 零命中;`ReportProcessor::new_with_config` 已不存在 |

## 6. 未做
- `../rounds/21` §8.6 的 **L1**(`translate_works` 并发端到端真机上传用例)仍待决:需账号与可删草稿。
- 大件三项仍在目标库:分片上传、NEMO"完整搬家"的资源重传、夹具与输出目录分离。
- `../rounds/**` 的 233 行历史 `->` 按 §4 的口径不回改。

## 依据
- 代码提交:`264026f`(`refactor(public-api)!`:类型改名 + 悬空入口删除)。
- 目标库:`../goals/infra-backlog.md` §6(两条转已完成)、`../goals/convert-backlog.md` §2 第 6 条的 L4、`../goals/README.md` 第 41 轮小节的待决口径转已定。
- 知识库:`../knowledge/errata.md`(新增两条:改名、入口删除)、`../knowledge/repo-conventions.md` §4(口径锚点改名)、`../knowledge/work-file-formats.md` §1(字段字典表述)。
- 体例判据与计数:`docs/README.md` "文档体例"节;计数命令见本文 §4 的描述(按"表格内/表格外"分别统计)。
