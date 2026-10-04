# 平台/接口域待做(HTTP · WebSocket · 端点)

> 目标库:与**平台服务**相关的未做完的事。协议事实见 `../knowledge/platform-and-protocol.md`。

## 1. 真机实测(便宜,做完就能消掉假设)

| 项 | 要做的事 | 出处 |
| --- | --- | --- |
| ~~`/coconut/clouddb/currentTime` 返回形态与单位~~ **已完成(2026-10-03)**:实测 `data` 为 **10 位秒级**时间戳(与本地秒同值),故**不需要** `/1000`,维持现状 | 原计划:若返回毫秒数字(>1e12)=> `get_calibrated_timestamp` 先 `/1000`;否则维持 | `../rounds/04` Assumptions + Phase 5;读数见 `../rounds/43` §2、`../knowledge/platform-and-protocol.md` §5 |
| `update_phone_number` 请求字段名 **已完成(2026-10-03)**:实测请求字段是 **`phone_number`**(OpenAPI 写的 `phone` 被服务端忽略),**维持现码** | 原计划:OpenAPI 说 `phone`,现码发 `phone_number` => 实测确认一个 | `../rounds/04` Phase 6-1;证据见 `../rounds/43` §2 |
| 错误信息是否含服务端 body **已完成(2026-10-03)**:统一路径(`send_checked`)**带完整服务端错误体**;裸 `MewRequestBuilder::send()` 不带(需显式 `.with_error_body()`) | 原计划:跑一次真实失败请求,人工确认 `6-6` 的统一错误语义 | `../rounds/04` Phase 6;证据见 `../rounds/43` §2 |
| 云存储事件帧是否容忍**无空格** `42[…]` **已完成(2026-10-03)**:`parse_frame` 只剥 `"42"` 前缀、其余交给 `serde_json`(跳过前导空白)⇒ **两者都接受**,无需改动 | 原计划:现实现无空格且工作正常,但 `../rounds/01` 坑 5 要求"统一带空格" => 以真机为准定论(不要盲改) | `../rounds/01` 坑 5;判定见 `../rounds/43` §2 |
| WS 连接行为 **已完成(2026-10-03)**:两线程并发 `connect` 均 `Ok` 且数据就绪(串行化成立);`close` 的事件序列为 `[Opened, Closed { was_connected: true }]`,无虚假 `Error` | 原计划:并发 `connect` 串行化、断连**不发虚假 `Error`** 需真实 WS 服务人工验证 | `../rounds/04` Phase 4;证据见 `../rounds/43` §2 |

## 2. 已核验(先前怀疑的"证据不足"已消;逐条状态见下表)

1.  **已完成(核对;早已办妥)**:`fetch_organization_ids` 的绝对 URL 调用点只有一处且传 `None` —— `src/api/education.rs` 的 `build_request(..., None)`;全仓绝对 URL 调用点均为 `None`(`../rounds/13` §P3-5)。
2.  **已完成(核对;早已办妥)**:`fetch_7day_hot_posts_iter`(`src/api/forum.rs`)端点固定为 `"/web/forums/boards/posts/7dayHot"`,`board_id` 走 `with_iter_param`(`../rounds/13` §P3-6)。
3.  **已完成(`60d5358`)**:`src/core/converse.rs` 补注释「chat 事件无字符串化载荷,刻意不二次解析(与 cloudvar 不同),勿改」(`../rounds/11` §P4-3),零行为改动。

## 3. 待方案 + 评审(已落地项与剩余项见下表)

| 项 | 说明 | 出处 |
| --- | --- | --- |
| AI 对话**指数退避重连** | 现在断线只置 `connected=false` + emit 错误;`send_and_wait` 只能等 `Timeout`,必须手动 `connect()`。云变量有退避 => 对齐前**先定会话/历史重建语义** | `../rounds/29` §2-3 |
| 举报"每类型 100 条"上限 | 现默认移除上限;是否保留取决于产品语义 | `../rounds/04` Assumptions |
| ~~大作品上传超时~~ |  已完成(修复,2026-09-26):上传请求用**请求级超时覆盖**(常量值与实测读数见 `../knowledge/nemo-runtime-and-upload.md` §6) | `../rounds/21` §8.4 N1 |
| ~~单包上传大小上限(413)~~ |  **已完成(收尾,2026-09-26)**:已加 `shared::ensure_single_package_fits` 提前报错;**阈值/速率/渠道与逐档读数只在 `../knowledge/platform-and-protocol.md` §5bis 展开**。要传更大作品需分片上传(暂无需求) | `pending-decisions.md` A5/D5 |
| ~~下载侧大文件风险~~ |  **已完成(修复,2026-10-02,`afca96c`)**:两层 —— 请求级超时 + 显式有界的大体通路(普通 API 响应仍守 10 MB 护栏)。**子项"无读超时"已结案(2026-10-03)**:原诊断经实测推翻(`timeout_global` **覆盖**响应体读取),改为**分段预算** —— 下载路径抬总预算与响应体预算,而"等响应头"仍按客户端 30 s 失败;ureq 3 无逐次读的空闲超时,判不做。常量值/逐路径枚举见 `../rounds/40-gates-cleanup-and-real-defects.md` §7,复测与落点见 `../rounds/43` §3 | `../rounds/40-gates-cleanup-and-real-defects.md` §7;`../rounds/43` §3 |

## 4. P2 性能小项(择机清)

1. `requests.rs` 的 **Bearer 头构造**(`format!("Bearer {token}")`)与 `Arc<str> -> String`:每次请求各克隆一次。
2. `requests.rs::PaginatedIter::build_params`:每翻一页克隆全部 `base_params` + 两个键串。
3. `cloudvar.rs` 的 flush 循环用固定 100 ms `sleep` 轮询(空闲也醒)=> 可换 `Condvar`/`Notify` 按需唤醒。

> 本节条目一律**按符号定位**(不写 `文件:行`);原始行号见 `../rounds/29` §3。

## 5. 公开面清理(需版本策略)

`../rounds/29` §3-3 的公开面死代码已被评审删除(`FileContent`、`CodeMaoFile::{file_write,write_json,write_lines,write_text}`、`PathConfig::{fiction_file_path,token_file_path,ensure_directories}`、孤立的 `FileError::Json`;**`write_bytes` 保留**)。
遗留问题:**破坏性 API 变更的版本策略** —— 本库尚未 1.0,当前约定是"不留兼容别名直接删";若将来要保兼容,这批就是需要 `#[deprecated]` 的先例。

## 6. 已核对并放弃的

- `requests.rs::is_header_overridden` 每个默认头线性扫描:常量小数组 × 常量,收益 ≈ 0(`../rounds/29` §4)。
