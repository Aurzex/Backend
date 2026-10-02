# 第 40 轮 —— CI 防线 / `unused` 放开 / 根块定案 / 反编译补测 / 性能上界 / 可见性与公共面;下载侧大文件与夹具纪律

> 日期 2026-10-02。第 40 轮**动代码**,一个目标一个提交;本文件把原先拆成两篇的
> 「下载侧大文件」与「夹具目录纪律」**全文并入**,并把只存在于 `docs/goals/README.md`
> 第 40 轮目标表单元格里的 R1–R7 证据一并收录 —— 合并后,**本文件是第 40 轮唯一的展开陈述处**,
> 目标库只留一行状态 + 提交号 + 指向本篇的指针。

## 0. 概览

**范围**:R1–R6 加 R7(R2 顺带报出的「对外形状」;决策账在 `docs/goals/pending-decisions.md` **D6**),
外加同批的两件事(T1 下载侧大文件、T3 夹具目录纪律)与一批「无需决策的清理」(非锁 `unwrap` 硬化、注释补写)。

**一句话结论**:收益在**防线与可证性** —— CI 不再空跑通过、`unused` 全面转 `warn`、
基准比较能一次列全、夹具不再被默认输出路径覆盖、反编译域补上离线错误路径测试;
收益不在性能 —— 同批实测的性能方向上界虽显著(`core` 占 52–79%),两个最小切片收益均为 0,已**判不立项**(§5)。

**提交清单**(按时间顺序):

| 提交 | 一句话 |
| --- | --- |
| `f68c2e6` | R1:删 artifact 上传步 + 新增 `offline-gate` job |
| `6414b97` / `30216c5` / `8da596d` | R2:`unused` 分三阶段放开(依次为机械族、`dead_code`、收口) |
| `47a8c5e` | `src/main.rs` 改走库 crate(去掉第二个 crate root;R2「两棵树」的分界点) |
| `4072846` | R3:根块 `0 vs 80` 定案(`model.rs` 注释改准,数值不动) |
| `c076918` | R4:七家编辑器错误路径补离线测试 + 删只写不读的 `BlockContext.variable_map` |
| `104964f` | R73) :四个可达 `pub` 类型的字段放宽到 `pub`(27 个字段) |
| `cbb167a` | 非锁裸 `unwrap` 硬化(50 处重枚举,硬化 49、保留 1)—— 见 §9 |
| `60d5358` | `converse.rs` 补「chat 事件无字符串化载荷」注释(零行为改动)—— 见 §9 |
| `afca96c` | T1:下载侧请求级超时 + 大体积通路 —— 见 §7 |
| `9290ede` + `636127f` | T3:夹具迁到只读目录 + 基准比较一次列全 —— 见 §8 |

**未做 / 留给下一步**:R5(加载门离线化进 CI:`validateBcm` headless + 入库最小夹具)仍**待决**,见 §9.3。

## 1. R1 —— CI 产物步不再可能空跑通过 + 离线门进 CI(`f68c2e6`)

**状态**:已完成(2026-10-02,`f68c2e6`)

**目标**:**CI 产物步不再可能空跑通过 + 离线门进 CI**

**说明 / 判据**(第 40 轮目标表的原文):

删掉 artifact 上传步及其矩阵 `artifact:`/`libname:` 键(本仓只有 rlib、bin 是需账号的交互式控制台、无消费方,故没有可分发产物;旧步期待 `.so/.dll/.dylib`,`if-no-files-found` 默认 `warn`,故一直静默通过);新增 `offline-gate`:`fmt --check` + `clippy --all-targets -D warnings` + **逐目标点名**的离线测试(`--lib`/`--test repo_hygiene`/`--test convert_bench`)。**刻意不跑**真机门、不设 `BACKEND_REQUIRE_LIVE`(理由见 `../knowledge/repo-conventions.md` §6)

**权威落点**:CI 跑什么 / 刻意不跑什么,耐久口径见 `../knowledge/repo-conventions.md` §6;根因线索见 `../knowledge/errata.md` 的「CI 产物口径与 `Cargo.toml` 不符」节。

## 2. R2 —— `unused` 告警分阶段放开(`6414b97` / `30216c5` / `8da596d`)

**状态**:已完成(2026-10-02,三阶段 `6414b97` 机械族 / `30216c5` `dead_code` / `8da596d` 收口)

**目标**:**`unused` 告警分阶段放开**

**说明 / 判据**(第 40 轮目标表的原文):

终态 `[lints.rust] unused = "warn"`,三选择(`--lib`/`--bins`/`--tests`)诊断 **0/0/0**;独立验证在 HEAD `8da596d`(树干净)上实测:`clippy --all-targets -- -D warnings` **无诊断**、`cargo test` 120 通过、基准门 **6/6 SHA 与基线一致 + 并发/串行同哈希**。**计数(注意:两棵树不可混用)**:R2 开工时(`47a8c5e` 之后)`--lib` 50 / `--bins` **50**(全部来自被顺带重编的 **lib 依赖单元**,bin 自身 **0**)/ `--tests` 98;另有一个 **`47a8c5e` 之前**(旧 `src/main.rs` 还是第二个 crate root)的旧读数 `--bins` **946**(其中 `main.rs` **896**)/ `--tests` **551**(其中 `main.rs` 453)——那批 bin 侧噪声是**重写**消除的、**不是** R2 消除的,也不能在当前树复现(现行 `--bins` = 0)。阶段 1 清理机械族 19 条(+ test 目标侧 8 条),阶段 2 处置 `dead_code` 31 条(**删 13 / `#[cfg(test)]` 10 / `#[allow]`+理由 8**)。两条坑:**组的 `level` 要配 `priority = -1`**(否则 clippy `lint_groups_priority` 直接报错)、**显式 `warn` 挡不住 `-D warnings`**。口径与逐条处置表见 `infra-backlog.md` §1.1;**四处对外形状 / 公共面项已按分库纪律移入 `pending-decisions.md` D6**

**读数与逐条处置表**:只在 `../goals/infra-backlog.md` §1.1 展开(两棵树 / 三阶段 / `#[allow(dead_code)]` 的 8 项处置与现存 6 处属性 / 两条实测坑);两套旧读数为何不可复现见 `../knowledge/errata.md` 的「第 40 轮 R2 的 `unused` 读数」节。

## 3. R3 —— 根块纵向布局 `0 vs 80` 是否真 bug(`4072846`)

**状态**:已完成(2026-10-02,`4072846`)

**目标**:**根块纵向布局 `0 vs 80` 是否真 bug**

**说明 / 判据**(第 40 轮目标表的原文):

结论:**不是行为缺陷** —— 两者不是同一约定(**同名不同物**),错的是 `model.rs` 那句自称"一致"的注释(已改准)。平台取证(711 个含根块实体):编辑器自排网格是 `0 + 80·k`(起点 0 出现 257/711,起点 80 **零次**),平台步长 80 与本库的 220 也不是一套,而根块坐标**非语义**(在语义 diff 的 allow-list 里);A/B 实测把 80 改 0 只让 `kn-3.7MB` 一个基线键变 SHA,故零行为收益,**数值刻意保持 80/220**。见 `convert-backlog.md` §2 第 8 条

**平台取数、A/B 实测与「为何不共享常量」**见 `../goals/convert-backlog.md` §2 第 8 条;`rounds/31` §3.6 N4 的审计结论与锚点漂移见 `../knowledge/errata.md`。

## 4. R4 —— 反编译域补离线测试(`c076918`)

**状态**:已完成(2026-10-02,`c076918`)

**目标**:**反编译域补离线测试**

**说明 / 判据**(第 40 轮目标表的原文):

七家编辑器的畸形/边界输入补**类型化错误**断言(Kitten4 缺/坏 `compile_result`、Kitten2/3 的 blocksXML 分支各断言一遍、Coco 屏数据漂移、Neko 密文损坏、Nemo/Wood 输出路径不可写且不留半成品)、抓取器缺 `source_urls`/`work_urls`/`bcmc_url` 的 `InvalidResponse`(全走注入的 `HttpClient` 桩,不碰全局单例);**固定两条隐性契约**:未知**块**类型走兜底分支**原样保留**、未知**影子**类型**告警回退 `logic_empty`**(JSON/XML 两形态)—— 后者若把未知类型名原样写出会让 Kitten4 **整份工作区加载失败**(`../knowledge/convert-semantics.md` §5bis 第 3 条);`XmlBlockWriter` 根块布局门(**两两不重叠 + 首根在 (0,0)**),**刻意不固定**"首根 80 / 步长 220"这类自定值(坐标非语义,在语义 diff 的 allow-list 里)。**测试具备判别力**:做了变异验证(把 `y += 220.0` 改成 `+= 0.0`、把影子回退类型改成 `math_number`,对应测试各自 FAILED,再还原)。另删掉只写不读的 `BlockContext.variable_map`(连带字段+参数+两处调用点+map 构造共 **6 处**;零读取方,故产物字节/块数/告警不变,**`convert_bench` 6/6 SHA 与基线一致、无需重刷**),对应 D61) 

两条隐性契约的**判据与证据方法**见 `../knowledge/convert-semantics.md` §5bis(第 3、5 条)。

## 5. R6 —— 先量「少建中间 `Value` 树」的上界(判定类任务,零落地)

**状态**:已完成(2026-10-02;判定类任务,零落地)

**目标**:**先量"少建中间 `Value` 树"的上界**

**说明 / 判据**(第 40 轮目标表的原文):

用 W7 的 `alloc_*` 计数口径把一次 `translate_file` 的窗口拆成三段(6 样本;一次性探针,跑完即删):**A** = 整窗、**A_parse** = 单独解析源文本、**A_mat** = 把**产物文本**物化成 `Value`(即"产物自己的地板")。读数:产物 **79–98%** 的节点就是积木树;`A_mat` = **1.49–1.59 次分配/节点**;`core = A − A_parse − A_mat` = **52–79% 的 A**(kitten4-10.8MB 1 113 686/2 125 929 = 52%;nemo-3.4MB 1 281 697/1 626 408 = 79%),故 **上界不是噪声级(远超 ±2%)**。但**可达性**已被两次实测排除:P6(这条路上最小的一步:手写装配替 `#[serde(flatten)]`)产物逐字节等价、**无可测收益**、+150 行(`rounds/37` §10.6);W9(合并编码两趟)分配收益 **0**(`rounds/39` §W9)。故**判定不立项**;若要取得那 52–79%,只能把 `BlockJson` 中间表示整个拿掉(即 parse、mapping、**直接写流**)。**重开条件**:1)  出现硬性墙钟/内存指标;2)  有人认领完整流式重写并自带门(产物字节不变 + 分配门下降 + profiler 前后)。注意:读法提醒,NEMO 两行的 `core` 里含它的输入 **XML(DOM)构建**,**不能全按"中间 `Value` 冗余"读**。见 `convert-backlog.md` §5 第 2 条

**读数与「判定不立项」的完整理由**见 `../goals/convert-backlog.md` §5 第 2 行;前置计量口径(W7 分配计数门)见 `rounds/39` §W7;两个失败切片见 `rounds/37` §10.6 与 `rounds/39` §W9。

## 6. R7 —— 可见性与公共面(1)  随 R4 销账 / 2)  待决 / 3)  `104964f`)

**状态**:部分(1)  已完成(`c076918`,随 R4 销账);2)  待决;3)  已完成(`104964f`))

**目标**:**对外形状三项**(R2 顺带报出;决策账在 `pending-decisions.md` **D6**)

**说明 / 判据**(第 40 轮目标表的原文):

1)  `BlockContext.variable_map` 只写不读,**已删除**(随 R4);2)  `ActionRegistry.client` / `ReportFetcher.client` 存而不用且 `new_with_client` 收下即丢(请求走**方法参数**上的 client),故建议**删字段 + 删 `new_with_client`**(属**公共面**,**待决**);3)  四个可达 `pub` 类型的字段**放宽到 `pub`**(`104964f`:**4 个可达类型 + 3 张嵌套元素表,共 27 个字段**;同步撤掉 4 处 `#[allow(dead_code)]`,全仓现剩 **6 处**属性 + `kitten4_vocab.rs` 中 1 处仅注释提及(提交信息里的"7 处"含那一处))。逐项口径见 D6

**逐项口径与状态只在 `../goals/pending-decisions.md` D6 展开**(D6 = 四项:1) 2) 3) 4) ,比 R7 多其后登记的 4)  `ProcessorUi::input`)。

## 7. T1 —— 下载侧大文件:请求级超时 + `ureq` 10 MB 隐性上限(`afca96c`)

> 提交:`afca96c`。承接 `../goals/platform-backlog.md` §3「下载侧大文件风险」。

### 7.1 背景(backlog §3「下载侧大文件风险」)

全局请求超时 30 s(`ClientConfig::default`);上传侧早在 `rounds/21` §8.4 N1 用请求级
`UPLOAD_TIMEOUT = 600 s` 修过,**下载侧未修**。

### 7.2 两个问题(不是一个)

1. **超时**:下载请求受全局 30 s 限制。
2. **体量(更硬)**:`ureq::Body::read_to_vec` / `read_to_string` 自带 **10 MB** 上限
   (`MAX_BODY_SIZE`,ureq 3.4.0)—— 大作品**在超时之前**就 `BodyExceedsLimit` 失败,只放宽超时没用。
   证据:本机语料 `download/compile/raw/原气骑士 且听风吟-编辑版.bcm4` = **63 598 143 B(约 60.6 MiB)**。

### 7.3 改动(`afca96c`)

- 唯一下载通路为 `src/core/convert/shared.rs::CodeMaoHttpClient`(Kitten / NEMO / NEKO / Coco / WOOD
  五种抓取 + 资源批量下载 + `fetch_kitten_source_document` 全走它)。三个方法统一:
  `.with_timeout(DOWNLOAD_TIMEOUT)` + 大体读取。
- `src/utils/requests.rs` 新增:
  - `pub const DOWNLOAD_TIMEOUT: Duration` = 900 s(换算:30 MB 上传 228 s,约 130 KB/s;
    63 598 143 B 按 100 KB/s 约 636 s,故 900 s 留约 40% 余量)。
  - `pub const MAX_DOWNLOAD_BODY_BYTES: u64` = 256 MiB(内存护栏,**不是**协议限制)。
  - `CodeMaoClient::response_to_{binary,string,json}_large(response, url)`:显式自管上限读取,
    超限报可操作的 `MewError::ResponseTooLarge { url, limit, received }`。
  - 普通 `response_to_*`(10 MB 护栏)**保持原样**给所有普通 API 响应;上传侧两处未动;全局默认未动。
- `tests/convert_edit_harvest.rs::fetch_bytes` 同修(它拉平台编辑格式 `.bcm4`,是下载路径之一)。

### 7.4 一次性证明(本地 HTTP 服务,跑完已删)

- 40 s 慢响应:全局 30 s 超时,在 30.9 s 返回 `Err("timeout: global")`;新下载路径在 46.1 s 返回 `Ok(2)`。
- 12 MB 响应体:新大体通路返回 `Ok(12582912)`;旧 10 MB 助手返回
  `Err("the response body is larger than request limit: 10485760")`。
- **诊断(顺带发现,已写进知识库)**:body 分片慢送(40 B / 40 s)在全局 30 s 下**成功**,故
  `timeout_global` **只管到响应头**,不覆盖 body 流式读取。所以第 40 轮解决的是
  "**首字节/响应头 > 30 s**"与"**单个响应体 > 10 MB**"两类失败;真正慢的 body 传输是
  **另一类**("无读超时")问题,未修。

### 7.5 下载路径枚举(改造前后)

| # | 路径(文件:函数) | 下载内容 | 改前 | 改后 |
| --- | --- | --- | --- | --- |
| — | `core/convert/shared.rs` `CodeMaoHttpClient::{get_json,get_binary,get_text}` | **全部下载的唯一入口** | 全局 30 s + 10 MB | `DOWNLOAD_TIMEOUT` 900 s + 256 MiB |
| 1 | `decompile/editors.rs` `KittenFetcher::fetch` | Kitten `player/load` 元信息 | 30 s | 900 s |
| 2 | 同上(作品文档) | **Kitten 编译版作品文档(MB 级)** | 30 s + 10 MB | 900 s + 256 MiB |
| 3 | `NemoFetcher::fetch` | NEMO `source/public` 元信息 | 30 s | 900 s |
| 4 | 同上(作品 JSON) | **NEMO `.bcm` 作品 JSON** | 30 s + 10 MB | 900 s + 256 MiB |
| 5 | `NemoResourceManager` 封面 | NEMO `.cover` | 30 s + 10 MB | 900 s + 256 MiB |
| 6 | `CocoFetcher::fetch` | Coco `load` 元信息 | 30 s | 900 s |
| 7 | 同上(作品文档) | **Coco 编译版作品文档** | 30 s + 10 MB | 900 s + 256 MiB |
| 8 | `NekoFetcher::fetch` | NEKO `published-work-detail` 元信息 | 30 s | 900 s |
| 9 | 同上(密文体) | **NEKO 密文作品体(base64)** | 30 s + 10 MB | 900 s + 256 MiB |
| 10 | `WoodFetcher::fetch` | **WOOD 发布体(整份作品)** | 30 s + 10 MB | 900 s + 256 MiB |
| 11 | `decompile/mod.rs::download_resources_parallel` | **资源字节(NEMO/WOOD 造型/素材;单作品实测 1390 个 / 49 MB)** | 30 s + 10 MB/请求 | 900 s + 256 MiB |
| 12 | `decompile/mod.rs::fetch_work_info` | 作品元信息 | 30 s | 900 s |
| 13 | `decompile/mod.rs::fetch_kitten_source_document` | 元信息 + **Kitten 编译版文档(公共 API)** | 30 s + 10 MB | 900 s + 256 MiB |
| 14 | 入口 `core/convert/mod.rs::{translate_work,translate_work_in}` | 走 1–13(fetcher);`keep_source` 只影响**上传**源文件,不新增下载路径 | — | — |
| 15 | `tests/convert_edit_harvest.rs::fetch_bytes` | **平台编辑格式 `.bcm4`(采集工具)** | 30 s + 10 MB | `DOWNLOAD_TIMEOUT` + `response_to_binary_large` |
| — | `api/auth.rs::fetch_admin_captcha` | 管理员验证码图片(几 KB) | 30 s + 10 MB(足够,**未改**) | 同前 |
| — | 上传侧 `utils/requests.rs` | 上传(已 A1 修过) | `UPLOAD_TIMEOUT` 600 s(**未动**) | 同前 |

**哪些当前就可能超过 10 MB**:#2 / #4 / #7 / #9 / #10 / #13(作品文档)、#11(单个大素材)、
#15(编辑格式 `.bcm4`,语料实测约 60.6 MiB / 63 598 143 B)。修复前它们会先撞 10 MB 上限。

### 7.6 未做

- 未给 body 流式读取设 `timeout_recv_body`(ureq 3 未配置,属另一类"无读超时"问题)。
- 未动普通 API 响应的 10 MB 护栏(那是**内存护栏**,刻意保留)。

## 8. T3 —— 夹具目录纪律 + 断言顺序(`9290ede` / `636127f`)

> 提交:`9290ede`。一次"真机门把 bench 输入夹具覆盖掉"的事故、修法与基线处置。

### 8.1 事故:跑一次真机门,把 bench 输入夹具覆盖了

`tests/convert_bench.rs` 的样本 `kn-9.4MB` 原先放在 `download/convert/`:

- **读**:`tests/convert_bench.rs` 与 `tests/convert_facade_bench.rs` 的样本清单都读
  `download/convert/Phigros 自制谱模拟器_195038626.kn.bcmkn`(两个 bench 都只读不写)。
- **写**:`download/convert/` 就是 `PathConfig::convert_file_path()`(`src/utils/filedata.rs`),
  而它是**转化域门面的默认输出目录**(`translate/mod.rs::product_path`:未给 `output_dir` 时回退到它);
  另外 `translate_work` 的中间目录 `staging_dir`(`src/core/convert/mod.rs`)**恒**在
  `download/convert/staging/` 下,**不受 `TranslateOptions::output_dir` 影响**。
- **触发**:真机门 `tests/convert_live.rs::translate_work_creates_draft_when_ignored` 调的
  `translate_work(..., TranslateOptions::new().upload(true))` 没给 `output_dir`,故产物按
  `<源文件名主干>.<slug>.<ext>` 口径落成同名同路径,**整体覆盖夹具**。

即:**输入夹具目录与库/门面的默认输出目录相同**。这是一条**必然**冲突的结构,不是偶然。
旧字节在 `.gitignore` 的 `/download` 下,**不可恢复**(已在全盘按 size=9357732 找过,无副本)。

### 8.2 修法(选中):把夹具挪出输出目录

候选 (a) 为让真机门写独立输出目录,(b) 为把夹具挪到只读目录。**选 (b)**,因为 (a) **不足以**满足判据
「夹具目录里不允许有任何测试会写」:

- `staging_dir` 恒在 `download/convert/staging/` 下,夹具只要还在 `download/convert/`,该目录就仍被写;
- 更根本:`download/convert/` 是**库的默认输出目录**,任何调用方不给 `output_dir` 都会往里写。

落地:

1. 夹具移到 **`download/fixtures/`**(= **只读夹具目录**,仓库里没有任何写点;仍 gitignored)。
2. `tests/convert_bench.rs` 与 `tests/convert_facade_bench.rs` 的样本路径指到新位置(仅两个读点);
   在样本处加注释写明"夹具目录纪律"。
3. 顺带修真机门自身与文件头约定的不一致:`convert_live.rs` 头部写「输出写系统临时目录,不污染仓库」,
   而该 ignored 门没给 `output_dir`(另一个 ignored 门本来给了临时目录),故现在补 `.output_dir(&work_dir("draft"))`。
   **判据依据是 (b)**,这条只是让该门遵守自己声明的约定。

没动 `PathConfig` / 任何公共签名。

### 8.3 基线处置:有据刷新 `kn-9.4MB` 的两个键

#### 8.3.1 先更正一个说法:产物 SHA **变了**

任务前提里的"产物 SHA256 仍然与基线一致"**不成立** —— 那份对照只截了 `#meta`,而产物 SHA 那一行
**确实不一致**:

| 基线键 | 旧(刷新前) | 新(刷新后) |
| --- | --- | --- |
| `kn-9.4MB-kitten4`(产物 SHA256) | `dac08917…` | `0e873b2b…` |
| `#meta.source_bytes` | 9357732 | 9357804 |
| `#meta.source_sha256` | `b4af1e83…` | `c5881f55…` |
| `#meta.warnings` | 1827 | 1828 |
| `#meta.output_bytes` / `blocks_total` / `blocks_converted` | 9775778 / 5235 / 4624 | 同左(不变) |

**为什么之前只看到 `#meta` 不一致**:`tests/convert_bench.rs` 的断言顺序是先 `meta_mismatched`
后 `mismatched` panic,故 `#meta` 一变,产物 SHA 的门**根本来不及报告**。只看输出会以为"产物没变"。
(该断言顺序**已修** —— `636127f`,见本文 §8.6。)

#### 8.3.2 产物 SHA 为什么会变 —— 输入快照换了,不是行为变了(已证)

已固定的事实:

1. 夹具里实体/积木 id 是**随机 UUID**(实测 3430 处;`TranslateOptions::deterministic_ids` 默认
   **false**)。真机门用默认选项,故每次产出的快照 id 都不同。
2. 反向 KN 到 Kitten4 会把源实体 id 带进产物(`assembly.rs` 的 `"id": source.get("id")`),
   故**产物 SHA 是输入快照的函数**。验证(临时探针,已删):把夹具里 1 个 actor id 改成别的值
   (同长度),产物 SHA 立刻从 `0e873b2b…` 变成 `21b2195a…`。
3. `source` 字段**不影响**产物:夹具与其"只删掉 `source` 成员"的变体(72 字节)都得到同一产物
   `0e873b2b…`(后者 warning 少 1:反向把 `source` 当 `DroppedProperty` 丢掉)。
4. **关键对照**:在**最后一次刷新基线的那次提交 `6057a88`** 上重新编译跑同一探针,同一输入
   同样得到 `0e873b2b…`(与 HEAD 逐字节相同),故自 `6057a88` 起,反向产物代码**没有**改变过
   该输入的结果;`dac08917…` 只能是**旧快照**(换了整套随机 id)的产物。

结论:**变的是输入快照,不是转换行为**。新旧夹具是同一作品(195038626)的两次转化快照
(块数自 5235 变为 4624、产物字节 9775778、toolType KN / version 0.16.2、55 角色 / 4 场景都对得上),
差异在随机 id(以及多出的 `source` 引用)。

#### 8.3.3 刷了什么、代价是什么

- 只改 `kn-9.4MB-kitten4` 与 `kn-9.4MB-kitten4#meta` **两个键**;其余 5 个样本逐字节不动
  (用 `BACKEND_BENCH_REFRESH=1` 跑出正确新值后,把其它样本的 record-only `alloc_*` 读数还原成原值,
  保持最小 diff)。
- **代价(必须记录)**:`kn-9.4MB` 的基线固定为**"真机门从作品 195038626 生成的、带随机 UUID id
  与 `source` 引用的一次性快照"**,而不是"某次刻意采集、可复现的文件"。它的产物 SHA 是
  **快照冻结门**,不是"可复现性门":任何重新采集(或再跑一次真机门)都会得到不同的 id,故
  产物 SHA 必变,必须再走一次**有据刷新**。这与刷新前的情形同类(旧夹具也是这种快照),
  区别只是换了一份快照。其余 5 个样本仍来自可控采集(`download/compile/**`),不受影响。
  (该"两种门的区别"已写进 `../knowledge/convert-performance.md`。)

### 8.4 同类冲突(只报告,不修改)

**`download/compile/` 是同一类结构问题的另一个实例**:它既是

- **写点**:反编译默认输出目录(`decompile/config.rs` 的 `default_output_dir`)、语料采集器
  `tests/convert_corpus_harvest.rs`(含 `player-load/`)、编辑格式采集器 `tests/convert_edit_harvest.rs`(`k4edit/`);
- 又是 **byte-baselined 夹具/语料**:`convert_bench` 5 个样本、`convert_facade_bench` 4 个样本、
  `convert_work_bench` 的 `RAW_SAMPLE`,以及 `reverse_tests.rs` / `nemo_tests.rs` /
  `translate/mod.rs` / `pipeline.rs` 的多条真作品门,还有两个**默认跑**的往返扫描器遍历整个目录。

采集器是**按发现流增量写新文件**、不覆盖既有夹具,所以当前没发生事故;但"默认输出目录"这条通路仍在:
以默认选项反编译**恰为夹具那几件作品**的调用会按同名口径覆盖夹具。彻底消除需要把
"语料/夹具目录"与"输出目录"分开(与 §8.2 的 (b) 同思路),属较大的布局约定变更,故
已登记为待方案(`../goals/convert-backlog.md` §2,标"未修、有风险、需方案")。

### 8.5 验证

提交 `9290ede`(只含 4 个文件:`tests/convert_bench.rs`、`tests/convert_facade_bench.rs`、
`tests/convert_live.rs`、`tests/fixtures/translate/convert_bench_baseline.json`)。

四道门全部通过(退出码均为 0):`cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` /
`cargo test` / `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored`
(末者输出 `产物 SHA256 与元信息都与基线一致 已完成`)。

6 样本产物 SHA256(与基线逐项相同;其中 5 个自始未变):

| 样本 | SHA256 |
| --- | --- |
| `kitten4-10.8MB-kn` | `bafeb50c…` |
| `kitten4-0.3MB-kn` | `d653a8a5…` |
| `kn-9.4MB-kitten4` | `0e873b2b…` |
| `kn-3.7MB-kitten4` | `0d3cf2e3…` |
| `nemo-3.4MB-kn` | `67641a0a…` |
| `nemo-old-1.5MB-kn` | `4b6038f9…` |

冲突已消除的旁证:移入 `download/fixtures/` 的那份夹具为 9357804 字节、
sha256 `c5881f55…`(与刷新后的 `#meta.source_sha256` 一致),故基准跑完后**夹具字节未被动过**;
`download/convert/` 现在只剩空的 `staging/`。

### 8.6 第二处缺陷:断言顺序把失败遮住(已修)

> 提交 `636127f`(只改 `tests/convert_bench.rs`,+269/−28)。这是 §8.3.1 那个"只看输出会以为产物没变"
> 的**根因**。

**症状**:`tests/convert_bench.rs` 里 `#meta` 不一致先 panic、产物 SHA 不一致后 panic,故产物 SHA 的不一致
**永远看不到**。§8.3.1 的误判即由此产生。

**修法**:

- 比较逻辑抽成**纯函数** `baseline_mismatches(key, 基线产物 SHA, 实跑产物 SHA, 基线 `#meta`, 实跑 `#meta`)
  -> `Vec<BaselineMismatch>`:一次返回**全部**不一致项 —— 产物 SHA 一项 + `#meta` 每个**参与断言的**键各一项
  (按字段名排序;**键被加/删也算**,缺失渲染成 `<缺>`)。
  **记录键(`alloc_*`)仍不参与断言**(与既有"只记录不判"口径一致,否则跨机抖动会假红)。
- 两类不一致**分开标注**(这是"可操作"的核心):
  `[产物 SHA256 | 行为/产物变了]` 与 `[#meta.<字段> | 输入夹具被换 / 元信息漂]`;
  报告开头点清"产物 SHA256 N 项 / `#meta` M 项",随后逐项给出 **样本名、键、基线值与现在值**。
- `render_baseline_mismatches`(纯函数)负责这份"一次列全"的报告文本,便于测试。
- **行为不变(判据等价)**:通过/失败仍只看"不一致项是否为空"(等价于原来的"任一非空即 panic");
  并发腿不一致(1 vs 8 产物不同)仍是**独立**断言,且仍在 refresh 分支**之前** panic(相对顺序未动);
  pass 分支打印的通过提示不变。

**测试**(非 `#[ignore]`,本段新增的两个用**合成输入**、不动真基线;该集成测试目标现有 **4 个** `#[test]`,其中本段新增 2 个):

- `baseline_mismatches_reports_product_and_every_meta_key_together`:构造"产物 SHA 与 `#meta`
  **同时**不一致"(正是实测那次的形状),断言 1)  产物项在列表里 2)  `#meta` 逐键报、记录键不进、
  按字段名排序 3)  报告文本同时含两类标签 + 样本名 + 期望/实际 4)  共 3 项。
- `baseline_mismatches_is_silent_without_baseline_or_when_equal`:基线缺键则不报(归 W3a/W3e 的
  加载与重刷守卫);完全一致则返回空列表(不误报)。

**变异验证**(证明测试确有判别力):临时在 `baseline_mismatches` 里插入"首个错即停"
(`if !out.is_empty() { return out; }`),`cargo test --test convert_bench baseline_mismatches`
返回 **FAILED**(exit 101,`left: []` / `right: ["source_bytes", "source_sha256"]`);还原后通过。

**门**:`fmt --check` 0 / `clippy --all-targets -- -D warnings` 0 / `cargo test` 0 /
`BACKEND_REQUIRE_BENCH=1 … convert_bench --ignored` 0(6 样本含 `kn-9.4MB` 与基线逐项一致)。

## 9. 同批的工程收尾与未做项

### 9.1 非锁裸 `unwrap` 硬化(`cbb167a`)

- 按「接收者跨行回溯 + 排除 `.lock()/.read()/.write()`」的新口径**重新枚举**,真实 **50 处**(生产 **10** / `cfg(test)` **40**):
  **硬化 49、按约定保留 1**(`utils/socketio.rs` 的 `Condvar::wait_timeout` —— 与相邻 `lock().unwrap()` 同属 Mutex 毒化);
  另 **1 处待决**(`core/terminal.rs` 的 stdin 读失败,对应 **D64) **)。
- 旧「49 处」点名的四族(`auth.rs::time_difference`、`registry.rs` 的 `active.as_mut().unwrap()`、
  `compiler.rs`(已并入 `core/convert/`)的 `template.unwrap()` 与 10 处 `write!(String).unwrap()`)在本树**已不存在**,
  故旧计数**不可复现**(旧口径还会漏掉跨行的 `lock()` 与 `.unwrap()` 链)。
- **唯一权威落点**:`../goals/infra-backlog.md` §2 第 2 条(本处只记第 40 轮结论与提交号)。

### 9.2 注释补写(`60d5358`)

- `src/core/converse.rs` 补注释「chat 事件无字符串化载荷,刻意不二次解析(与 cloudvar 不同),勿改」,
  零行为改动(口径:`rounds/11` §P4-3;登记:`../goals/platform-backlog.md` §2 第 3 条)。

### 9.3 未做:R5 加载门离线化(**待决**)

- **目标**:`validateBcm` headless 进常规测试 + 一个**入库的最小夹具**。
- **卡点**:`download/` 与官方 bundle 都不入库,故要先决定夹具形态;CI 现状(2026-10-02,`f68c2e6`)
  为 `build` 矩阵 + `hygiene` + `offline-gate`,真机门与语料基准**刻意不进**(口径见 `../knowledge/repo-conventions.md` §6)。
- 来源:原 `convert-backlog` §6.3 G5。

## 依据

- 本文件合并自(原文件已并入并删除):原「下载侧大文件」篇(§7 全文)、
  原「夹具目录纪律 + 断言顺序」篇(§8 全文)、`../goals/README.md` 第 40 轮目标表 R1–R7 的单元格(§1–§6 的证据原文);
  其余提交号(`cbb167a` / `60d5358`)见 §9。
- 相关权威落点:`../goals/infra-backlog.md` §1/§1.1(R2 读数、CI 产物、`47a8c5e`)、§2 第 2 条(unwrap 硬化);
  `../goals/convert-backlog.md` §2 第 8 条(根块)、§5 第 2 行(R6);`../goals/pending-decisions.md` D6(公共面四项);
  `../knowledge/repo-conventions.md` §6(CI 口径)、§5(文件组织);`../knowledge/convert-semantics.md` §5bis/§6;
  `../knowledge/convert-performance.md` §5(基准样本与两种门);`../knowledge/errata.md`(CI 口径、R2 读数、`rounds/31` §3.6 N4)。
