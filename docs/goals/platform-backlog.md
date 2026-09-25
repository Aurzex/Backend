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

## 2. 待核验(文档称已完成,但证据不足)

1. `fetch_organization_ids` 的绝对 URL 处 `Some(BaseKey::Education)` → `None` 是否**逐处**改完(`docs/rounds/13` §P3-5)。
2. `fetch_7day_hot_posts_gen` 的 `board_id` 是否已走 `with_iter_param`(端点已固定为 `/web/forums/boards/posts/7dayHot`)(`docs/rounds/13` §P3-6)。
3. `converse` 的 `parse_frame` 是否已补注释说明"**不二次解析**"(`docs/rounds/11` §P4-3)。

## 3. 待方案 + 评审

| 项 | 说明 | 出处 |
| -- | ---- | ---- |
| AI 对话**指数退避重连** | 现在断线只置 `connected=false` + emit 错误;`send_and_wait` 只能等 `Timeout`,必须手动 `connect()`。云变量有退避 ⇒ 对齐前**先定会话/历史重建语义** | `docs/rounds/29` §2-3 |
| 举报"每类型 100 条"上限 | 现默认移除上限;是否保留取决于产品语义 | `docs/rounds/04` Assumptions |
| ~~大作品上传超时~~ | ✅ 已修(2026-09-26):上传请求用 `UPLOAD_TIMEOUT=600s`(请求级超时覆盖) | `docs/rounds/21` §8.4 N1 |
| **单包上传大小上限(413)** | 30 MB 单包被 qiniu 拒 `413`;9.3 MB 通过 ⇒ 上限在 9.3~30 MB 之间。定位/绕开方案见 `pending-decisions.md` A5 | 2026-09-26 实测 |
| **下载侧大文件风险**(新) | 单请求下载 63 MB 级作品/资源仍受全局 30 s 限制 ⇒ 慢网必失败。同类修法(给工作文件/资源下载单独放宽超时)未做 | `docs/knowledge/nemo-runtime-and-upload.md` |

## 4. P2 性能小项(择机清)

1. `requests.rs:340-342` / `:978-980`:每次请求 `format!("Bearer {token}")` 与 `Arc<str> → String` 各克隆一次。
2. `requests.rs:1237-1260`:`PaginatedIter::build_params` 每翻一页克隆全部 `base_params` + 两个键串。
3. `cloudvar.rs:2410`:flush 用固定 100 ms `sleep` 轮询(空闲也醒)⇒ 可换 `Condvar`/`Notify` 按需唤醒。

## 5. 公开面清理(需版本策略)

`docs/rounds/29` §3-3 的公开面死代码已被评审删除(`FileContent`、`CodeMaoFile::{file_write,write_json,write_lines,write_text}`、`PathConfig::{fiction_file_path,token_file_path,ensure_directories}`、孤立的 `FileError::Json`;**`write_bytes` 保留**)。
遗留问题:**破坏性 API 变更的版本策略** —— 本库尚未 1.0,当前约定是"不留兼容别名直接删";若将来要保兼容,这批就是需要 `#[deprecated]` 的先例。

## 6. 已核对并放弃的

- `requests.rs::is_header_overridden` 每个默认头线性扫描:常量小数组 × 常量,收益 ≈ 0(`docs/rounds/29` §4)。
