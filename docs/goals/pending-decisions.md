# 待决策事项(需要你拍板)

> 目标库:所有**悬着的决策**集中在这里。每项给:问题 / 可选项 / 我的建议 / 不定的后果。
> 决策完就把条目移进对应的 backlog 文件并标上结论。**优先看 A 组** —— 它们挡着当前工作。

## A. 阻塞当前工作(建议先回这四条)

| # | 问题 | 选项 | 建议 | 不定的后果 |
| - | ---- | ---- | ---- | ---------- |
| A1 | **大作品上传必失败**:≈9 MB 作品在全局 30 s 超时下超时(实测 31.2 s / 35.5 s) | ① 上传走**独立超时**(只放宽上传端点) ② 分片上传 ③ 维持现状 + 文档标注"大作品上传不可用" | **①**:小改动、风险低;分片留待真需要 | `translate_work(upload=true)` 在慢网/大作品下继续不可用;`keep_source` 的源文件上传同受影响(它失败只降级为告警) |
| A2 | **`keep_source` 上传的是"反编译重建的编辑版",不是原始文件字节** | ① 保留现状 + 文档说明 ② 反编译侧额外保留原始字节(改动跨域) ③ 放弃 `keep_source` | **①**:官方语义是"保留原件",我们手里只有重建版;文档说明偏差即可 | 平台的"保留原件"打不开原件(功能名实不符) |
| A3 | **`cloudvar.rs` 的 `detect_editor` 仍用全局 `WorkDataFetcher::new()`**(接 WS 时自动识别编辑器类型) | ① 让 `CloudBuilder` 持有 `CodeMaoClient` 完成注入 ② 维持全局 | **①**:与 14–17 轮注入纪律一致;设计已清楚(WS 持有 HTTP 客户端) | 剩余一处全局依赖,纯注入式使用方无法完全隔离 |
| A4 | **NEMO 作品上传链路缺一环**:建作品接口 ✅ 已实现(`POST /nemo/v3/works/upload/<n>`),封面绑定 ✅ 已实现;缺的是第 1/2 步 —— 上传凭证要 `projectName=nemo_android_ios`,而 `UploadChannel` 只有 `Pgaot|Codegame|Codemao`(后者的凭证写死 `community_frontend`) | ① 加 NEMO 渠道(或把 `projectName` 参数化)+ 编排四步,再真机验证 ② 只做"复用源作品 `work_url`"的零上传路线 B′(仍需真机验证) ③ 维持现状(只有底层 API) | **①**,但**验证前要先解决清理**:NEMO 侧删除接口未知(KN 侧有 `delete_kn_draft`),跑一次会在你账号下留一份删不掉的 NEMO 草稿 | 转换域仍只能"转出去"(NEMO→KN→Kitten4),不能"传回去";`docs/rounds/24` §13.4 的验证配方继续搁置 |

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
