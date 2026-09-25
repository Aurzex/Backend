# 待决策事项(需要你拍板)

> 目标库:所有**悬着的决策**集中在这里。每项给:问题 / 可选项 / 我的建议 / 不定的后果。
> 决策完就把条目移进对应的 backlog 文件并标上结论。**优先看 A 组** —— 它们挡着当前工作。

## A. 阻塞当前工作(A1 已修,剩下三条待你拍板)

| # | 问题 | 选项 | 建议 | 不定的后果 |
| - | ---- | ---- | ---- | ---------- |
| A1 ✅ **已修 + 已实测证明(2026-09-26)** | 大作品上传必失败:≈9 MB 作品在全局 30 s 超时下超时(实测 31.2 s / 35.5 s)。修后实测:一次性 30 MB 请求**跑了 228 s 未被超时掐断**(返回的是服务器的 413,见 A5)⇒ 超时放宽确实生效 | 已按方案 ① 落地:`MewRequestBuilder::with_timeout(Duration)`(ureq 3 请求级 config)+ 上传路径用 `UPLOAD_TIMEOUT = 600 s`(七牛表单与 pgaot 表单两处);其余请求仍走全局 30 s,行为不变 | — | 分片上传仍留待真需要;下载侧大文件(63 MB 单请求)是同类风险,尚未处理(见 `platform-backlog`) |
| A2 | **`keep_source` 上传的是"反编译重建的编辑版",不是原始文件字节** | ① 保留现状 + 文档说明 ② 反编译侧额外保留原始字节(改动跨域) ③ 放弃 `keep_source` | **①**:官方语义是"保留原件",我们手里只有重建版;文档说明偏差即可 | 平台的"保留原件"打不开原件(功能名实不符) |
| A3 | **`cloudvar.rs` 的 `detect_editor` 仍用全局 `WorkDataFetcher::new()`**(接 WS 时自动识别编辑器类型) | ① 让 `CloudBuilder` 持有 `CodeMaoClient` 完成注入 ② 维持全局 | **①**:与 14–17 轮注入纪律一致;设计已清楚(WS 持有 HTTP 客户端) | 剩余一处全局依赖,纯注入式使用方无法完全隔离 |
| A4 | **NEMO 作品上传链路**(✅ 上传渠道与编排已落地:`UploadChannel::Nemo` + `DecompileOptions::upload_to_account`) | 剩余待决:**是否真机验证 NEMO 上传**(建一份 NEMO 草稿) | **可择机**:KN 侧已端到端验证通过(`docs/rounds/30` §5:反编译 → 建草稿 330852319 → 回读 → 自删);NEMO 只差"敢不敢建" | NEMO 草稿**擦不掉**(删除端点未知;`empty_kn_trash` 只清 KN),所以跑一次会在你账号下留一份 NEMO 草稿;另:产物只带 `.bcm`,资源仍指向源 CDN(不重传) |

| A5 | **单包上传大小上限(新发现,与超时无关)**:30 MB 单包被 qiniu 拒 **413**;而 9.3 MB 的平台产物写入是成功的(`docs/rounds/21` §11.2)⇒ 上限落在 9.3~30 MB 之间 | ① 取凭证时带上文件大小(`fsize`/`fileSize`,若平台支持) ② 换 KN 官方口径的 `projectName=neko`+`insertOnly=true` 试上限 ③ 分片上传(qiniu 支持) ④ 文档标注上限并接受 | **先做 ①/② 的便宜实验**定位上限归属;分片留待真需要 | 超过上限的作品 `translate_work(upload=true)` 仍会失败(真实 KN 产物多在 3~9 MB,暂不阻塞日常使用) |

## B. 大方向(决定"要不要立轮")

| # | 问题 | 选项 | 建议 |
| - | ---- | ---- | ---- |
| B1 | **api 层类型化 DTO**:351 处 `MewResult<Value>` → 逐端点 DTO(需核对响应形态) | ① 按域分批立轮 ② 维持 `Value` ③ 只对新端点强制 DTO | **①分批**:收益是编译期校验 + 文档化;一次性做完不现实 |
| B2 | **KN → NEMO 方向**是否需要 | ① 做(要自建 NEMO 编码器并过 NEMO App 校验) ② 不做 | **②不做**:平台无对照实现,唯一用途是把产物塞回 NEMO(见 `pending-decisions` 之外的 route B),成本/风险远高于收益 |
| B3 | **把建作品接进 convert 域**?(`translate_work` 自动 `create_kn_work`/`create_nemo_work`) | ① 保持"转换只出文件,建作品由调用方显式调" ② 加**显式开关**自动建作品 | **①**:**建作品等同发布行为**,必须由调用方显式授权;已有独立 API 够用 |
| B4 | **`converse` 断线重连**:AI 对话断线后只能等 `Timeout`,需手动 `connect()`;云变量有指数退避 | ① 加指数退避重连(要先定会话/历史重建语义) ② 至少把可见行为写进文档 | **①**:与云变量行为对齐,但**必须先定 session 语义** |
| B5 | **恢复 `[lints.rust] unused` 告警**(现在 `unused = "allow"`,死代码不会被报) | ① 恢复为 `warn` + 清理暴露项 ② 维持 `allow` | **①**:先跑一次看告警数量再决定是否在本轮清 |
| B6 | **举报"每类型 100 条"上限**是否保留 | ① 移除上限(现默认) ② 保留 + 报告剩余 | 需**产品语义**拍板;技术侧无阻塞 |
| B7 | **api 层 newtype ID 推广**(`UserId` 等,13 个 Manager 数百签名) | ① 先看 `WorkId` 试点收益再定 ② 直接推广 ③ 不做 | **①**:`WorkId` 已在反编译链试点 |
| B8 | **`work.rs`(2533 行)再切 `WorkDataFetcher`** | ① 切(纯搬迁,re-export 保路径) ② 不做 | **①按需**:纯搬迁无风险,但目前没有痛点 |

## C. 小项(可以一次性批量拍板)

| # | 项 | 建议 | 说明 |
| - | -- | ---- | ---- |
| C1 | 49 处**非锁 `unwrap`** 硬化(`time_difference` / `active.as_mut` / 模板 `unwrap` / 10 处 `write!().unwrap()`) | 做 | 机械改动,零行为变化 |
| C2 | `DecompilerError` **包装 `MewError`**(消除自带 `Io/Json/Http` 重复) | 做 | 与 `ProcessorError`/`DataQueryError` 对齐 |
| C3 | P2 死代码/收尾:`simple.rs` 的 `Arc::clone`、`nemo.rs::get_sha` 的 64 字节 clone、`cloudvar` flush 的 100 ms 轮询改 `Condvar` | 做 | 都是局部小改,**删除前需零调用点证据** |
| C4 | `auth.rs::AccountStatus` 与 `requests.rs::Identity` 平行枚举合并 | 做(不紧急) | 易漂移,合并前先确认无外部依赖 |
| C5 | `MessageHandler` / `ChatEventHandler` 两个 trait 改自由函数 | 做 | 可读性收尾,非必须 |
| C6 | `registry.rs` 错位工具函数归位 | 做 | 模块组织问题 |
| C7 | `decompile_work` 拆成 `decompile_to_json` / `decompile_to_file`(去掉返回类型重载) | 做 | API 语义清晰化 |
| C8 | `core → api` 依赖倒置(pipeline/registry/services/retrieve 反向依赖 api) | **暂缓** | 架构级重构,收益不明确;记在案 |
| C9 | 真机实测两项:`/coconut/clouddb/currentTime` 单位、`update_phone_number` 字段名(`phone` vs `phone_number`) | 做 | 便宜,能消掉两处假设 |
| C10 | 人工验证项:错误信息含服务端 body、并发 `connect` 串行化、断连不发虚假 `Error`、云存储事件帧是否容忍无空格 `42[…]` | 做 | 都要真实服务;可以在跑真机测试时顺带确认 |

## 已决(不用再问)

- `MewError`/`MewResult` **品牌名保留**;`terminal.rs` 留在库内;`HttpClient` trait 不公开;类型级 WS 状态机(`CloudConnection<Connected>`)不做;god file 不拆、不加宏(`impl_api_manager!` 类)。
- 转换域:**不对齐官方字节**(只语义 diff + 官方 `validateBcm` 硬门);`RawValue` 透传与单遍遍历**判不做**;反向(KN→Kitten4)实体级并行**判不做**。
- 未改动项按"记录理由即可"处理:flush 回退重放风险、`block_xml` 字符串 `next`、`with_page`/`with_limit` 统一等(见 `docs/knowledge/errata.md` 与各轮次「不做」表)。
