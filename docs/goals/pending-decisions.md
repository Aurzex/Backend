# 待决策事项(需要你拍板)

> 目标库:所有**悬着的决策**集中在这里。每项给:问题 / 可选项 / 我的建议 / 不定的后果。
> 决策完就把条目移进对应的 backlog 文件并标上结论。**优先看 A 组**(挡着当前工作)与 **D 组**(转换域最近四轮新出)。

## A. 阻塞当前工作(A1/A2/A3/A5 已处理;剩 A4 待你拍板)

| # | 问题 | 选项 | 建议 | 不定的后果 |
| - | ---- | ---- | ---- | ---------- |
| A1 ✅ **已修 + 已实测证明(2026-09-26)** | 大作品上传必失败:≈9 MB 作品在全局 30 s 超时下超时(实测 31.2 s / 35.5 s)。修后实测:一次性 30 MB 请求**跑了 228 s 未被超时掐断**(返回的是服务器的 413,见 A5)⇒ 超时放宽确实生效 | 已按方案 ① 落地:`MewRequestBuilder::with_timeout(Duration)`(ureq 3 请求级 config)+ 上传路径用 `UPLOAD_TIMEOUT = 600 s`(七牛表单与 pgaot 表单两处);其余请求仍走全局 30 s,行为不变 | — | 分片上传仍留待真需要;下载侧大文件(63 MB 单请求)是同类风险,尚未处理(见 `platform-backlog`) |
| A2 ✅ **已决(2026-09-26,方案① 文档化)** | **`keep_source` 上传的是"反编译重建的编辑版",不是原始文件字节** | ① 保留现状 + 文档说明 ② 反编译侧额外保留原始字节(改动跨域) ③ 放弃 `keep_source` | **①**:官方语义是"保留原件",我们手里只有重建版;文档说明偏差即可 | 平台的"保留原件"打不开原件(功能名实不符) |
| A3 ✅ **已修(2026-09-26)** | **`cloudvar.rs` 的 `detect_editor` 仍用全局 `WorkDataFetcher::new()`**(接 WS 时自动识别编辑器类型) | ① 让 `CloudBuilder` 持有 `CodeMaoClient` 完成注入 ② 维持全局 | **①**:与 14–17 轮注入纪律一致;设计已清楚(WS 持有 HTTP 客户端) | 剩余一处全局依赖,纯注入式使用方无法完全隔离 |
| A4 | **NEMO 作品上传链路**(✅ 上传渠道与编排已落地:`UploadChannel::Nemo` + `DecompileOptions::upload_to_account`) | 剩余待决:**是否真机验证 NEMO 上传**(建一份 NEMO 草稿) | **可择机**:KN 侧已端到端验证通过(`docs/rounds/30` §5:反编译 → 建草稿 330852319 → 回读 → 自删);NEMO 只差"敢不敢建" | NEMO 草稿**擦不掉**(删除端点未知;`empty_kn_trash` 只清 KN),所以跑一次会在你账号下留一份 NEMO 草稿;另:产物只带 `.bcm`,资源仍指向源 CDN(不重传) |

| A5 ✅ **已定位(2026-09-26)** | **单包上传大小上限(与超时无关)**:30 MB 单包被 qiniu 拒 **413**;而 9.3 MB 的平台产物写入是成功的(`docs/rounds/21` §11.2)⇒ 实测**上限落在 20~24 MB**(同渠道逐档传:20 MB 成功、24 MB 413) | 已做第 ④ 项并**加了提前报错闸**(`shared::ensure_single_package_fits`,>20 MB 直接报错并说明上限);分片上传留待真需要 | ✅ 结论写进 `knowledge/platform-and-protocol.md` §5bis | 超过上限的作品 `translate_work(upload=true)` 仍会失败(真实 KN 产物多在 3~9 MB,暂不阻塞日常使用) |

## B. 大方向(决定"要不要立轮")

| # | 问题 | 选项 | 建议 |
| - | ---- | ---- | ---- |
| B1 | **api 层类型化 DTO**:351 处 `MewResult<Value>` → 逐端点 DTO(需核对响应形态) | ① 按域分批立轮 ② 维持 `Value` ③ 只对新端点强制 DTO | **①分批**:收益是编译期校验 + 文档化;一次性做完不现实 |
| B2 | **KN → NEMO 方向**是否需要 | ① 做(要自建 NEMO 编码器并过 NEMO App 校验) ② 不做 | **②不做**:平台无对照实现,唯一用途是把产物塞回 NEMO(见 `pending-decisions` 之外的 route B),成本/风险远高于收益 |
| B3 | **把建作品接进 convert 域**?(`translate_work` 自动 `create_kn_work`/`create_nemo_work`) | ① 保持"转换只出文件,建作品由调用方显式调" ② 加**显式开关**自动建作品 | **①**:**建作品等同发布行为**,必须由调用方显式授权;已有独立 API 够用 |
| B4 | **`converse` 断线重连**:AI 对话断线后只能等 `Timeout`,需手动 `connect()`;云变量有指数退避 | ① 加指数退避重连(要先定会话/历史重建语义) ② 至少把可见行为写进文档 | **①**:与云变量行为对齐,但**必须先定 session 语义** |
| B6 | **举报"每类型 100 条"上限**是否保留 | ① 移除上限(现默认) ② 保留 + 报告剩余 | 需**产品语义**拍板;技术侧无阻塞 |
| B7 | **api 层 newtype ID 推广**(`UserId` 等,13 个 Manager 数百签名) | ① 先看 `WorkId` 试点收益再定 ② 直接推广 ③ 不做 | **①**:`WorkId` 已在反编译链试点 |
| B8 | **`work.rs`(2533 行)再切 `WorkDataFetcher`** | ① 切(纯搬迁,re-export 保路径) ② 不做 | **①按需**:纯搬迁无风险,但目前没有痛点 |

## C. 小项(可以一次性批量拍板)

| # | 项 | 建议 | 说明 |
| - | -- | ---- | ---- |
| C1 | 49 处**非锁 `unwrap`** 硬化(`time_difference` / `active.as_mut` / 模板 `unwrap` / 10 处 `write!().unwrap()`) | 做 | 机械改动,零行为变化 |
| C3 | P2 死代码/收尾:`simple.rs` 的 `Arc::clone`、`nemo.rs::get_sha` 的 64 字节 clone、`cloudvar` flush 的 100 ms 轮询改 `Condvar` | 做 | 都是局部小改,**删除前需零调用点证据** |
| C4 | `auth.rs::AccountStatus` 与 `requests.rs::Identity` 平行枚举合并 | 做(不紧急) | 易漂移,合并前先确认无外部依赖 |
| C5 | `MessageHandler` / `ChatEventHandler` 两个 trait 改自由函数 | 做 | 可读性收尾,非必须 |
| C6 | `registry.rs` 错位工具函数归位 | 做 | 模块组织问题 |
| C7 | `decompile_work` 拆成 `decompile_to_json` / `decompile_to_file`(去掉返回类型重载) | 做 | API 语义清晰化 |
| C8 | `core → api` 依赖倒置(pipeline/registry/services/retrieve 反向依赖 api) | **暂缓** | 架构级重构,收益不明确;记在案 |
| C9 | 真机实测两项:`/coconut/clouddb/currentTime` 单位、`update_phone_number` 字段名(`phone` vs `phone_number`) | 做 | 便宜,能消掉两处假设 |
| C10 | 人工验证项:错误信息含服务端 body、并发 `connect` 串行化、断连不发虚假 `Error`、云存储事件帧是否容忍无空格 `42[…]` | 做 | 都要真实服务;可以在跑真机测试时顺带确认 |

## D. 转换域 · 最近四轮(33–36)新出的决策点(**D1–D5 已按建议落定**;**D6 为第 40 轮 R2 新登记:① `c076918`、③ `104964f` 已落定,仅 ② 待拍板**)

> 背景:rounds/34–36 把"产物能在真编辑器里打开"这条线走通了(实机验证),
> 代价与剩余缺口都量化过了。事实与证据见 `docs/knowledge/convert-semantics.md` §5bis 与 `docs/rounds/34–36`。

| # | 问题 | 选项 | 建议 | 不定的后果 |
| - | ---- | ---- | ---- | ---------- |
| D1 ✅ **已决(2026-09-26):方案①,不做降级** | **Neko 专有块族要不要做"语义降级"** —— 反向现在把 Kitten4 没有对应概念的块**剔除 + 逐类报告**:`temporary_list`(临时列表)、`script_variables`/`_param`/`_value`(脚本变量)、`traverse_number*`、`self_listen*`/`self_broadcast_with_param`、`procedure_boolean`、`self_text_effect_color`、`color_size_slider`。两个真作品里合计约占剔除量的一半(`Node VM v3` 285+97+21+19 …;`now` 90+36+33…)。**注:`rounds/38 §7bis` 起块不再被剔除 —— 就地改成「未收录积木」标记(内容不可恢复,但块与位置保住);影子仍清空** | ① **不做**(维持剔除 + 报告) ② 做全量降级 ③ 只做低风险子集(先逐类列出候选映射再挑) | **①**:平台 40 件 Kitten4 语料里这些名字**0 次出现**、编辑器注册表 349 条里也没有 ⇒ 没有可信的等价物;硬做等于**改语义**(例如用列表变量模拟临时列表),而"降级后行为等价"目前没有判据 | 用这些功能的 KN 作品转成 Kitten4 后会**少掉这部分积木**(编辑器能打开、其余功能正常) |
| D2 ✅ **已决(2026-09-26):方案①,不做** | **槽默认影子回写**(可选打磨):源在同一槽里同时保留「槽的默认影子」与真子块,我们只留真子块 ⇒ 编辑器里那个槽显示**空白**而不是默认值(功能无差、不丢引用) | ① 不做 ② 做 | **①**:纯视觉差异,量级 ~86+192(rounds/34 §4quinquies) | 少数槽在编辑器里显示空白 |
| D3 ✅ **已决(2026-09-26):方案①,维持手动** | **"编辑器能打开"这条门要不要进 CI**:现在是我按轮次手动跑(无头 Chromium + 线上 Kitten4 + 对照组) | ① 维持手动 ② 进 CI | **①**:依赖真编辑器与网络,CI 里容易 flaky;进 CI 需要先做"离线可复现"的替身(例如把 `validateBcm` 硬门接进 CI,那条已经 headless 可跑) | 编辑器回归只能靠轮次里的人工实机验证兜 |
| D4 ✅ **已落地(2026-09-26;rounds/38 起由"剔除量门"改成 `MARKER_BUDGET`)** | **标记量的预算门**:现在只有**定义体侧**有预算断言(`≤3193`);**块/影子的标记量**另有门(rounds/36 的 942→715 / 398→50 是人工 A/B 比的,rounds/38 起口径为"改成「未收录积木」标记的块数 + 清空影子数") | ① 做(对全语料测一遍记基线,写进 `reverse_tests`) ② 不做 | **①**:便宜且能防"悄悄又标记多了";口径可复用现成的扫描器 | 标记量回退时没人拦 |
| D5 ✅ **已完成(2026-09-26)** | **单包上传上限**(原 A5)是否继续:30 MB 单包被 qiniu 拒 **413**,平台自己 9.3 MB 产物写入成功 ⇒ 上限落在 9.3~30 MB 之间 | ① 做便宜实验定位上限归属(取凭证带 `fsize`;或试 KN 官方口径 `projectName=neko` + `insertOnly=true`) ② 分片上传 ③ 标注上限并接受 | 已按 ① 收尾(上限 20~24 MB + 提前报错) | 无(超出上限的作品现在是**明确报错**而不是白等后 413) |
| **D6** **对外形状三项**(第 40 轮 R2 报出,2026-10-02 登记) | ① `BlockContext.variable_map` **只写不读**(每角色一份 UUID→变量名,由 `with_capacity` 注入;**已于 `c076918` 删除**);② `ActionRegistry.client` / `ReportFetcher.client` **存而不用** —— 请求实际走**方法参数**上的 client,`new_with_client` 收下的那个被丢弃(不是错客户端 bug,但注入形状**名不副实**);③ 四个 `pub` 类型(`AdminReportStatistics` / `FanByLikesStatistics` / `RankingData` / `UserInfo`)的字段**曾是 `pub(crate)`** ⇒ 外部拿到类型也读不到字段(**已放宽**,`104964f`) | ① 并入 R4 一并做 / 留 / 删;② **删字段 + 删 `new_with_client`**(注入改由方法参数承担,干净切齐)/ 保留现状;③ 先查这些类型是否出现在 `backend::` 的**公开签名**里再定(对外数据 ⇒ 放宽字段;内部类型 ⇒ 把类型收 `pub(crate)`) | ① ✅ **已完成**(`c076918`,随 R4 落地:字段 + `with_capacity` 参数 + 调用点已删,产物字节不变)⇒ 不必在此再决策;② **待你拍板**(删字段/方法属**公共面**改动);③ ✅ **已完成**(`104964f`:**7 个类型 / 27 个字段**放宽到 `pub`,含 4 张嵌套表;同步撤掉 4 处 `#[allow(dead_code)]`,全仓现剩 **7 处**) | ②不定:使用者以为 `new_with_client` 生效,实际被丢;③不定:外部拿到类型但零可读字段(名不副实) |

## 已决(不用再问)

> **2026-10-02 从 B/C 组移入**(已完成,保留结论与提交号以便回溯):
> - ~~B5 **恢复 `[lints.rust] unused` 告警**~~ → **已完成**:旧为 `allow`;第 40 轮 R2 三阶段放开为 `warn`(`6414b97` 机械族 19 条 / `30216c5` `dead_code` 31 条 / `8da596d` 收口),**终态三选择(`--lib`/`--bins`/`--tests`)诊断 0/0/0**;口径与逐条处置见 `../goals/infra-backlog.md` §1.1,耐久约定见 `repo-conventions.md` §6。
> - ~~C2 **`DecompilerError` 包装 `MewError`**(原记"消除自带 `Io/Json/Http` 重复")~~ → **已不成立并已办净**:该重复 **2026-10-01 核实已不存在**(`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew`);剩下的零调用死变体 `UnsupportedType` **已于 `fef30e7` 删除**(破坏性公共面变更、已授权)。见 `docs/rounds/39` §1.3/§W5②/§W12d。

- `MewError`/`MewResult` **品牌名保留**;`terminal.rs` 留在库内;`HttpClient` trait 不公开;类型级 WS 状态机(`CloudConnection<Connected>`)不做;god file 不拆、不加宏(`impl_api_manager!` 类)。
- 转换域:**不对齐官方字节**(只语义 diff + 官方 `validateBcm` 硬门);`RawValue` 透传与单遍遍历**判不做**;反向(KN→Kitten4)实体级并行**判不做**。
- 转换域(rounds/34–38 既定原则):**"产物能在真编辑器里打开"优先于"少丢几块"**;因此编辑器不认识的**块**一律就地改成「未收录积木」标记(`incompatible_block` / `incompatible_output_block`:内容不可恢复,但块与位置保住)、**影子**清空,并逐类报告(rounds/38 §7bis 起;`rounds/34–36` 当时是"整块剔除",那会让积木真的消失);
  表是 **Kitten3 口径** ⇒ 凡往 Kitten4 写名字的地方(块、影子)都必须过编辑器词汇判据;KN 侧没有分组概念 ⇒ `theatre.groups` 由反向**合成**。
- 未改动项按"记录理由即可"处理:flush 回退重放风险、`block_xml` 字符串 `next`、`with_page`/`with_limit` 统一等(见 `docs/knowledge/errata.md` 与各轮次「不做」表)。
