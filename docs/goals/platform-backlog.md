# 平台/接口域待办(HTTP · WebSocket · 端点)

> 目标库:与**平台服务**相关的未完成项。协议事实见 `docs/knowledge/platform-and-protocol.md`。

## 1. 真机实测(便宜,做完就能消掉假设)

| 项 | 要做的事 | 出处 |
| -- | -------- | ---- |
| `/coconut/clouddb/currentTime` 返回形态与单位 | 若返回毫秒数字(>1e12)⇒ `get_calibrated_timestamp` 先 `/1000`;否则维持 | `docs/rounds/04` Assumptions + Phase 5 |
| `update_phone_number` 请求字段名 | OpenAPI 说 `phone`,现码发 `phone_number` ⇒ 实测确认一个 | `docs/rounds/04` Phase 6-1 |
| 错误信息是否含服务端 body | 跑一次真实失败请求,人工确认 `6-6` 的统一错误语义 | `docs/rounds/04` Phase 6 |
| 云存储事件帧是否容忍**无空格** `42[…]` | 现实现无空格且工作正常,但 `docs/rounds/01` 坑 5 要求"统一带空格" ⇒ 以真机为准定论(不要盲改) | `docs/rounds/01` 坑 5 |
| WS 连接行为 | 并发 `connect` 串行化、断连**不发虚假 `Error`** 需真实 WS 服务人工验证 | `docs/rounds/04` Phase 4 |

## 2. 已核验(3 条全 ✅;原先怀疑的"证据不足"已消)

1. ✅ **已核对,早已办妥**:`fetch_organization_ids` 的绝对 URL 调用点只有一处且传 `None` —— `src/api/education.rs` 的 `build_request(..., None)`;全仓绝对 URL 调用点均为 `None`(`docs/rounds/13` §P3-5)。
2. ✅ **已核对,早已办妥**:`fetch_7day_hot_posts_iter`(`src/api/forum.rs`)端点固定为 `"/web/forums/boards/posts/7dayHot"`,`board_id` 走 `with_iter_param`(`docs/rounds/13` §P3-6)。
3. ✅ **已完成(`60d5358`)**:`src/core/converse.rs` 补注释「chat 事件无字符串化载荷,刻意不二次解析(与 cloudvar 不同),勿改」(`docs/rounds/11` §P4-3),零行为改动。

## 3. 待方案 + 评审(5 行中 3 行已 ✅,剩 2 项待方案)

| 项 | 说明 | 出处 |
| -- | ---- | ---- |
| AI 对话**指数退避重连** | 现在断线只置 `connected=false` + emit 错误;`send_and_wait` 只能等 `Timeout`,必须手动 `connect()`。云变量有退避 ⇒ 对齐前**先定会话/历史重建语义** | `docs/rounds/29` §2-3 |
| 举报"每类型 100 条"上限 | 现默认移除上限;是否保留取决于产品语义 | `docs/rounds/04` Assumptions |
| ~~大作品上传超时~~ | ✅ 已修(2026-09-26):上传请求用 `UPLOAD_TIMEOUT=600s`(请求级超时覆盖) | `docs/rounds/21` §8.4 N1 |
| ~~单包上传大小上限(413)~~ | ✅ **已收尾(2026-09-26)**:同渠道逐档实测,上限落在 **20~24 MB**(20 MB 成功 / 24 MB 413);已加 `shared::ensure_single_package_fits` 提前报错,>20 MB 直接给出"上限 + 实测值"的错误。要传更大作品需分片上传(暂无需求) | `pending-decisions.md` A5/D5 |
| ~~下载侧大文件风险~~ | ✅ **已修(2026-10-02,`afca96c`)**,两层:① **超时** —— 下载请求加请求级超时 `DOWNLOAD_TIMEOUT = 900 s`(`core/convert/shared.rs` 的 `CodeMaoHttpClient` 三个方法,覆盖作品文档与资源两条下载通路);② **体量(更硬)** —— `ureq` 的 `Body::read_to_vec`/`read_to_string` 自带 **10 MB** 上限,大作品会在**超时之前**先 `BodyExceedsLimit`,故新增显式有界的大体通路 `response_to_{binary,string,json}_large`(`MAX_DOWNLOAD_BODY_BYTES = 256 MiB` 内存护栏;超限报带 URL/上限/已读字节的 `MewError::ResponseTooLarge`),只给下载路径用,普通 API 响应仍守 10 MB 护栏。**未做**:body 流式读取的"无读超时"是另一类问题 | `docs/rounds/40-download-timeout-and-body-cap.md` |

## 4. P2 性能小项(择机清)

1. `requests.rs:340-342` / `:978-980`:每次请求 `format!("Bearer {token}")` 与 `Arc<str> → String` 各克隆一次。
2. `requests.rs:1237-1260`:`PaginatedIter::build_params` 每翻一页克隆全部 `base_params` + 两个键串。
3. `cloudvar.rs:2410`:flush 用固定 100 ms `sleep` 轮询(空闲也醒)⇒ 可换 `Condvar`/`Notify` 按需唤醒。

## 5. 公开面清理(需版本策略)

`docs/rounds/29` §3-3 的公开面死代码已被评审删除(`FileContent`、`CodeMaoFile::{file_write,write_json,write_lines,write_text}`、`PathConfig::{fiction_file_path,token_file_path,ensure_directories}`、孤立的 `FileError::Json`;**`write_bytes` 保留**)。
遗留问题:**破坏性 API 变更的版本策略** —— 本库尚未 1.0,当前约定是"不留兼容别名直接删";若将来要保兼容,这批就是需要 `#[deprecated]` 的先例。

## 6. 已核对并放弃的

- `requests.rs::is_header_overridden` 每个默认头线性扫描:常量小数组 × 常量,收益 ≈ 0(`docs/rounds/29` §4)。
