# 待决事项(需人拍板)

> 目标库:所有**悬着的决策**集中在这里。每项给出:问题 / 选项 / 建议 / 不定的后果。
> 决策完成后把条目移进对应的 backlog 文件并标上结论。**优先看 A 组**(阻塞进行中的工作)与 **D 组**(转换域最近四轮新出)。

## A. 阻塞进行中的工作(A1/A2/A3/A5 已处理;剩 A4 待决)

| # | 问题 | 选项 | 建议 | 不定的后果 |
| --- | --- | --- | --- | --- |
| A1 **已完成(2026-09-26:修复 + 实测)** | 大作品上传必失败:≈9 MB 作品在全局 30 s 超时下超时(实测 31.2 s / 35.5 s) | 1)  上传路径改用**请求级超时覆盖**(`MewRequestBuilder::with_timeout`) 2)  分片上传 | **已按方案 1) 落地**;**实测读数与常量值只在 `../knowledge/nemo-runtime-and-upload.md` §6 展开**(超时口径见 `../knowledge/platform-and-protocol.md` §5bis) | 分片上传仍留待真需要;下载侧大文件(单请求 63 598 143 B)的同类风险**已修**(2026-10-02,`afca96c`,见 `./platform-backlog.md`) |
| A2 **已完成(2026-09-26;已决:方案 1,文档化)** | **`keep_source` 上传的是"反编译重建的编辑版",不是原始文件字节** | 1)  保留现状 + 文档说明 2)  反编译侧额外保留原始字节(改动跨域) 3)  放弃 `keep_source` | **1) **:官方语义是"保留原件",本库只有重建版;文档说明偏差即可 | 平台的"保留原件"打不开原件(功能名实不符) |
| A3 **已完成(修复,2026-09-26)** | **`cloudvar.rs` 的 `detect_editor` 仍用全局 `WorkDataFetcher::new()`**(接 WS 时自动识别编辑器类型) | 1)  让 `CloudBuilder` 持有 `CodeMaoClient` 完成注入 2)  维持全局 | **1) **:与 14–17 轮注入纪律一致;设计已清楚(WS 持有 HTTP 客户端) | 剩余一处全局依赖,纯注入式使用方无法完全隔离 |
| A4 | **NEMO 作品上传链路**(已完成(上传渠道与编排):`UploadChannel::Nemo` + `DecompileOptions::upload_to_account`) | 剩余待决:**是否真机验证 NEMO 上传**(建一份 NEMO 草稿) | **可择机**:KN 侧已端到端验证通过(`../rounds/30` §5:反编译、建草稿 330852319、回读、自删);NEMO 只差是否实际建立 | NEMO 草稿**擦不掉**(删除端点未知;`empty_kn_trash` 只清 KN),所以跑一次会在使用者账号下留一份 NEMO 草稿;另:产物只带 `.bcm`,资源仍指向源 CDN(不重传) |
| A5 **已完成(定位 + 收尾,2026-09-26)** | **单包上传大小上限(与超时无关)**,已实测收尾:**超过上限提前报错**(`shared::ensure_single_package_fits`,避免用户白等几分钟再吃 413);**阈值与逐档读数只在 `../knowledge/platform-and-protocol.md` §5bis 展开** | 1)  做实验定位上限归属 / 2)  分片上传 / 3)  标注上限并接受(原记录见 D5) | **已完成(结论写进 `../knowledge/platform-and-protocol.md` §5bis)** | 超过上限的作品 `translate_work(upload=true)` 仍会失败(真实 KN 产物多在 3~9 MB,暂不阻塞日常使用) |

## B. 大方向(决定"要不要立轮")

| # | 问题 | 选项 | 建议 |
| --- | --- | --- | --- |
| B1 | **api 层类型化 DTO**:351 处 `MewResult<Value>` 改为逐端点 DTO(需核对响应形态) | 1)  按域分批立轮 2)  维持 `Value` 3)  只对新端点强制 DTO | **判不做(2026-10-02 起列入"明确不做",以 `./README.md` 的"明确不做"一节为准)**:原建议是"1) 分批",该判定已覆盖它 —— 逐端点核对 351 处响应形态的成本与回归面不可控 |
| B2 | **KN 到 NEMO 方向**是否需要 | 1) 做(要自建 NEMO 编码器并过 NEMO App 校验) 2) 不做 | **2) 判不做**(建议):平台无对照实现,唯一用途是把产物塞回 NEMO(路线 B 的定义见 `../rounds/24` §3.1),成本/风险远高于收益 |
| B3 | **把建作品接进 convert 域**?(`translate_work` 自动 `create_kn_work`/`create_nemo_work`) | 1)  保持"转换只出文件,建作品由调用方显式调" 2)  加**显式开关**自动建作品 | **1) **:**建作品等同发布行为**,必须由调用方显式授权;已有独立 API 够用 |
| B4 | **`converse` 断线重连**:AI 对话断线后只能等 `Timeout`,需手动 `connect()`;云变量有指数退避 | 1)  加指数退避重连(要先定会话/历史重建语义) 2)  至少把可见行为写进文档 | **1) **:与云变量行为对齐,但**必须先定 session 语义** |
| B6 | **举报"每类型 100 条"上限**是否保留 | 1)  移除上限(现默认) 2)  保留 + 报告剩余 | 需**产品语义**拍板;技术侧无阻塞 |
| B7 | **api 层 newtype ID 推广**(`UserId` 等,13 个 Manager 数百签名) | 1)  先看 `WorkId` 试点收益再定 2)  直接推广 3)  不做 | **1) **:`WorkId` 已在反编译链试点 |
| B8 | **`work.rs`(2533 行)再切 `WorkDataFetcher`** | 1)  切(纯搬迁,re-export 保路径) 2)  不做 | **1) 按需**:纯搬迁无风险,但暂无痛点 |

## C. 小项(可以一次性批量拍板)

| # | 项 | 建议 | 说明 |
| --- | --- | --- | --- |
| C3 **已完成(2026-10-03;逐项见 `../rounds/45`)** | P2 死代码/收尾:`simple.rs` 的 `Arc::clone`、`nemo.rs::get_sha` 的 64 字节 clone、`cloudvar` flush 的 100 ms 轮询改 `Condvar` | 逐项:`simple.rs` 已改 `Arc::try_unwrap`;**`get_sha` 不可行**(`RefCell` 借用无法带出函数边界;改 `Rc/Arc` 或 `&mut self` 的改动面远超收益);flush 已改按需唤醒(`flush_pending` + `Notify`,`flush_interval` 保留超时兜底) | 删除前需零调用点证据 |
| C4 **已完成(2026-10-03,`9ddacbb`)** | `auth.rs::AccountStatus` 与 `requests.rs::Identity` 平行枚举合并 | 已合并为单一 `Identity`(删 `AccountStatus` 与 `to_identity`;`Average→Fluffy`/`Edu→Scholar`/`Judgement→Judge` 一一对应,默认身份不变) | 已确认无外部依赖 |
| C5 **已完成(2026-10-03,`9ddacbb`)** | `MessageHandler` / `ChatEventHandler` 两个 trait 改自由函数 | 两个私有 trait 与其单元结构体已改为自由函数(分派结构保留) | 可读性收尾 |
| C6 **已完成(2026-10-03,`9ddacbb`)** | `registry.rs` 错位工具函数归位 | 四个与举报无关的工具(`value_to_string`/`timestamp_to_string`/`html_to_text`/`bytes_to_human`)已移到 `utils::filedata`(与既在那里的 `value_to_i64` 同处) | 模块组织问题 |
| C7 **判不做(已核实,2026-10-03)** | `decompile_work` 拆成 `decompile_to_json` / `decompile_to_file`(去掉返回类型重载) | **前提不存在**:现签名 `decompile_work(work_id, output_dir: Option<&Path>) -> Result<PathBuf>` 里 `None` 只表示"写默认目录",没有"返回 JSON 串"的分支(`../rounds/17` §3 已确认该重载模式从未实现且当时就被删除);内存产物已由 `CodemaoDecompiler::decompile_artifact_with` 提供 ⇒ 强拆只会造出不存在的返回模式 | 无需动作 |
| C8 | `core` 向 `api` 的依赖倒置(pipeline/registry/services/retrieve 反向依赖 api) | **暂缓(待重新立项)** | 架构级重构,收益不明确;记在案 |
| C9 **已完成(2026-10-03 实测;`c5bbb13`)** | 真机实测两项:`/coconut/clouddb/currentTime` 单位、`update_phone_number` 字段名(`phone` vs `phone_number`) | 实测结论:`data` 为 **10 位秒级**时间戳(与本地秒同值);字段名是 **`phone_number`**,改发 `phone` 会被服务端忽略并报 `phone_number 不能为空` | 两处假设均消掉,现有实现两处都正确;读数与探针见 `../rounds/43` §2、耐久事实见 `../knowledge/platform-and-protocol.md` §5 |
| C10 **已完成(2026-10-03 实测;`c5bbb13`)** | 人工验证项:错误信息含服务端 body、并发 `connect` 串行化、断连不发虚假 `Error`、云存储事件帧是否容忍无空格 `42[…]` | 实测:统一路径(`send_checked`)**带完整服务端错误体**,裸 `send()` 不带;两线程并发 `connect` 均成功且数据就绪;`close` 后事件序列为 `[Opened, Closed { was_connected: true }]`,无虚假 `Error`;`parse_frame` 只剥 `42` 前缀交给 `serde_json`(跳前导空白)⇒ 有无空格都接受 | 四项全部有实测或代码级证据;读数见 `../rounds/43` §2 |

## D. 转换域 · 最近四轮(33–36)新出的决策点(**D1–D5 已按建议落定**;**D6 为第 40 轮 R2 新登记:1)  `c076918`、3)  `104964f` 已落定,2)  与 4)  已完成(2026-10-03,见 `../rounds/43`) ⇒ 四项全部落定**;**D7/D8 已于 2026-10-05 落在"已决"(见下)**)

> 背景:rounds/34–36 把"产物能在真编辑器里打开"这条线走通了(实机验证),
> 代价与剩余缺口都量化过了。事实与证据见 `../knowledge/convert-semantics.md` §5bis 与 `../rounds/34–36`。

| # | 问题 | 选项 | 建议 | 不定的后果 |
| --- | --- | --- | --- | --- |
| D1 **已完成(2026-09-26;已决:方案 1,不做降级)** | **Neko 专有块族要不要做"语义降级"** —— 反向把 Kitten4 没有对应概念的块**剔除 + 逐类报告**:`temporary_list`(临时列表)、`script_variables`/`_param`/`_value`(脚本变量)、`traverse_number*`、`self_listen*`/`self_broadcast_with_param`、`procedure_boolean`、`self_text_effect_color`、`color_size_slider`。两个真作品里合计约占剔除量的一半(`Node VM v3` 285+97+21+19 …;`now` 90+36+33…)。**注:`../rounds/38 §7bis` 起块不再被剔除 —— 就地改成「未收录积木」标记(内容不可恢复,但块与位置保住);影子仍清空** | 1)  **不做**(维持剔除 + 报告) 2)  做全量降级 3)  只做低风险子集(先逐类列出候选映射再挑) | **1) **:平台 40 件 Kitten4 语料里这些名字**0 次出现**、编辑器注册表里也没有(条目数与单一事实源见 `../knowledge/convert-semantics.md` §5bis 第 3 条),因此没有可信的等价物;硬做等于**改语义**(例如用列表变量模拟临时列表),而"降级后行为等价"暂无判据 | 用这些功能的 KN 作品转成 Kitten4 后会**少掉这部分积木**(编辑器能打开、其余功能正常) |
| D2 **已完成(2026-09-26;已决:方案 1,不做)** | **槽默认影子回写**(可选打磨):源在同一槽里同时保留「槽的默认影子」与真子块,只保留真子块,因此编辑器里那个槽显示**空白**而不是默认值(功能无差、不丢引用) | 1)  不做 2)  做 | **1) **:纯视觉差异,量级 ~86+192(rounds/34 §4quinquies) | 少数槽在编辑器里显示空白 |
| D3 **已完成(2026-09-26;已决:方案 1,维持手动)** | **"编辑器能打开"这条门要不要进 CI**:按轮次手动跑(无头 Chromium + 线上 Kitten4 + 对照组) | 1)  维持手动 2)  进 CI | **1) **:依赖真编辑器与网络,CI 里容易 flaky;进 CI 需要先做"离线可复现"的替身(例如把 `validateBcm` 硬门接进 CI,那条已经 headless 可跑) | 编辑器回归只能靠轮次里的人工实机验证兜 |
| D4 **已完成(2026-09-26;rounds/38 起由"剔除量门"改成 `MARKER_BUDGET`)** | **标记量的预算门**:只有**定义体侧**有预算断言(数值与口径见 `../knowledge/convert-semantics.md` §6);**块/影子的标记量**另有门(rounds/36 的由 942 变为 715、由 398 变为 50 是人工 A/B 比的,rounds/38 起口径为"改成「未收录积木」标记的块数 + 清空影子数") | 1)  做(对全语料测一遍记基线,写进 `reverse_tests`) 2)  不做 | **1) **:成本低且能防止"标记量悄然增长";口径可复用现成的扫描器 | 标记量回退时没人拦 |
| D5 **已完成(2026-09-26)** | **单包上传上限**(原 A5)是否继续,已按方案 1)  收尾(**与 A5 是同一件事的两条记录**:A 组记"已收尾",D 组这里留决策轨迹;阈值与读数见 `../knowledge/platform-and-protocol.md` §5bis) | 1)  做低成本的定位实验(取凭证带 `fsize`;或试 KN 官方口径 `projectName=neko` + `insertOnly=true`) 2)  分片上传 3)  标注上限并接受 | **已完成(已按 1)  收尾)** | 无(超出上限的作品现为**明确报错**,而不是白等之后收到 413) |
| **D6** **对外形状 / 公共面四项**(第 40 轮 R2 报出、其后追加第 4 项,2026-10-02 登记) | 1)  `BlockContext.variable_map` **只写不读**(每角色一份 UUID 到变量名,由 `with_capacity` 注入;**已于 `c076918` 删除**);2)  `ActionRegistry.client` / `ReportFetcher.client` **存而不用** —— 请求实际走**方法参数**上的 client,`new_with_client` 收下的那个被丢弃(不是错客户端 bug,但注入形状**名不副实**)。**已完成(2026-10-03)**:只删两个只写不读的字段,**保留 `new_with_client`** —— 实测构造器参数确被内部注册表闭包消费,原建议"删字段 + 删构造器"会删掉 `services.rs`/`pipeline.rs` 依赖的真注入路径;证据见 `../rounds/43` §5;3)  四个 `pub` 类型(`AdminReportStatistics` / `FanByLikesStatistics` / `RankingData` / `UserInfo`)的字段**曾是 `pub(crate)`**,外部拿到类型也读不到字段(**已放宽**,`104964f`);4)  `ProcessorUi::input(&mut self, &str) -> String`(公共 trait)在 **stdin 读失败**时**无处返回错误**,现为 `expect` fail-fast(避免空串让 `choose/menu` 死循环)。**已完成(2026-10-03)**:`input`/`choose`/`menu` 改为返回 `MewResult`,并把 **EOF(`Ok(0)`)也算错误** ⇒ 既不再死循环,也不再走 `panic = "abort"`;逐调用点已传播,`main` 失败退出码 1;见 `../rounds/43` §4 | 1)  并入 R4 一并做 / 留 / 删;2)  **删字段 + 删 `new_with_client`**(注入改由方法参数承担,干净切齐)/ 保留现状;3)  先查这些类型是否出现在 `backend::` 的**公开签名**里再定(对外数据则放宽字段;内部类型则把类型收 `pub(crate)`);4)  **改签名 `MewResult<String>` 并逐调用点传播**(破坏性公共面,但本库未 1.0、约定是"直接改")/ 维持 `expect` fail-fast + 文档写明 | 1)  **已完成**(`c076918`,随 R4 落地:字段 + `with_capacity` 参数 + 调用点已删,产物字节不变),不必在此再决策;2)  **已完成(2026-10-03)**:按实测口径只删字段、**保留构造器**(原建议"删构造器"会破坏 `services.rs`/`pipeline.rs` 的真注入路径);3)  **已完成**(`104964f`:**4 个可达类型 + 3 张嵌套元素表 / 27 个字段**放宽到 `pub`;同步撤掉 4 处 `#[allow(dead_code)]`,当时全仓剩 **6 处**属性 + 1 处仅注释提及(提交信息里的"7 处"含那一处);其中两个 `client` 字段随后随本行 2) 删除,**当前为 4 处**属性));4)  **已完成(2026-10-03)**:按"改签名 + 逐调用点传播"落地(`input`/`choose`/`menu` 返回 `MewResult`,EOF 也当错误) | 2) 不定:使用者以为 `new_with_client` 生效,实际被丢;3) 不定:外部拿到类型但零可读字段(名不副实);4) 不定:`expect` 会让"stdin 关闭 / 管道断开"变成 **panic**(`release` 是 `panic="abort"`,直接终止进程) |

> **与第 40 轮 R7 的关系**:R7 是那次盘点里的**三项(1) 2) 3) ,见 `./README.md` 第 40 轮 R7 行)**;本 D6 是**四项(1) 2) 3) 4) )**—— 4) (`ProcessorUi::input`)是其后新登记的公共面项,故两处数量不同、不是同一份清单。


## 已决(不用再问)

> **2026-10-05 由 D 组移入**(按用户拍板落地;读数与命令见 `../knowledge/convert-performance.md` §2bis):
> - ~~D8 **发布档 `opt-level` 由 `"z"` 改为 `3`**~~ **已完成(2026-10-05)**:`Cargo.toml` 的 `[profile.release]` 改为 `opt-level = 3`(其余 `lto`/`strip`/`panic` 不变),注释里写明读数与适用范围(只作用于本仓作为顶层的构建)。实测 `core` 快 12.5%–28.5%、`e2e` 快 19%–31%、整套 test 墙钟 19.6%,代价是 bin 体积 2.20 → 2.90 MiB。
> - ~~D7 **是否指定全局分配器**~~ **已决(建议口径)**:本库**不指定**全局分配器(避免强加给所有使用者);改为把"换更快分配器可省 `core` 14%–26%"写成建议,写进 `README.md` 的构建节与 `../knowledge/convert-performance.md` §2bis,由使用者在自己的工程里用 `#[global_allocator]` 或 `LD_PRELOAD` 选择。

> **2026-10-02 从 B/C 组移入**(已完成,保留结论与提交号以便回溯):
> - ~~B5 **恢复 `[lints.rust] unused` 告警**~~ **已完成**:第 40 轮 R2 三阶段(`6414b97` / `30216c5` / `8da596d`),终态 `unused = "warn"`。**读数与口径见 `./infra-backlog.md` §1.1**,耐久约定见 `../knowledge/repo-conventions.md` §6。
> - ~~C1 **非锁 `unwrap` 硬化**~~ **已完成(`cbb167a`)**:处数、逐类处置与旧计数不可复现的理由**只在 `./infra-backlog.md` §2 第 2 条展开**;其中 1 处(stdin 读失败)转为待决,即 **D64) **。
> - ~~C2 **`ConvertError` 包装 `MewError`**(原记"消除自带 `Io/Json/Http` 重复";该类型 2026-10-03 前叫 `DecompilerError`)~~ **已不成立并已清零**:该重复 **2026-10-01 核实已不存在**(`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew`);剩下的零调用死变体 `UnsupportedType` **已于 `fef30e7` 删除**(破坏性公共面变更、已授权)。见 `../rounds/39` §1.3/§W52) /§W12d。

- `MewError`/`MewResult` **品牌名保留**;`terminal.rs` 留在库内;`HttpClient` trait 不公开;类型级 WS 状态机(`CloudConnection<Connected>`)不做;god file 不拆、不加宏(`impl_api_manager!` 类)。
- 转换域:**不对齐官方字节**(只语义 diff + 官方 `validateBcm` 硬门);`RawValue` 透传与单遍遍历**判不做**(读数见 `../knowledge/convert-semantics.md` §7);反向(KN 到 Kitten4)实体级并行**判不做**(判据见 `../knowledge/convert-performance.md` §4)。
- 转换域(rounds/34–38 既定原则):**"产物能在真编辑器里打开"优先于"少丢几块"**;因此编辑器不认识的**块**一律就地改成「未收录积木」标记(`incompatible_block` / `incompatible_output_block`:内容不可恢复,但块与位置保住)、**影子**清空,并逐类报告(rounds/38 §7bis 起;`../rounds/34–36` 当时是"整块剔除",那会让积木真的消失);
  表是 **Kitten3 口径**,因此凡往 Kitten4 写名字的地方(块、影子)都必须过编辑器词汇判据;KN 侧没有分组概念,因此 `theatre.groups` 由反向**合成**(机制与实机对照见 `../knowledge/convert-semantics.md` §5bis 第 2 条)。
- 未改动项按"记录理由即可"处理:flush 回退重放风险、`block_xml` 字符串 `next`、`with_page`/`with_limit` 统一等(见 `../knowledge/errata.md` 与各轮次「不做」表)。
