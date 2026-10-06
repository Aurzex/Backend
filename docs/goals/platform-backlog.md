# 平台/接口域待做(HTTP · WebSocket · 端点)

> 目标库:与**平台服务**相关的未做完的事。协议事实见 `../knowledge/platform-and-protocol.md`。

## 1. 真机实测(便宜,做完就能消掉假设)

| 项 | 要做的事 | 出处 |
| --- | --- | --- |
| ~~`/coconut/clouddb/currentTime` 返回形态与单位~~ **已完成(2026-10-03 实测;`c5bbb13`)**:实测结论(时间戳单位与本地秒同值)⇒ **不需要**给 `get_calibrated_timestamp` 加任何缩放,维持现状 | 原计划:返回毫秒数字时需要先做一次 `/1000`;否则维持现状 | `../rounds/04` Assumptions + Phase 5;读数见 `../rounds/43` §2、`../knowledge/platform-and-protocol.md` §5 |
| `update_phone_number` 请求字段名 **已完成(2026-10-03 实测;`c5bbb13`)**:实测确认现码发的字段名被服务端接受(OpenAPI 写的是另一个名字、被忽略),**维持现码** | 原计划:OpenAPI 与现码的字段名不一致,需实测确定一个 | `../rounds/04` Phase 6-1;证据见 `../rounds/43` §2、字段名见 `../knowledge/platform-and-protocol.md` §5 |
| 错误信息是否含服务端 body **已完成(2026-10-03 实测;`c5bbb13`)**:统一路径带完整服务端错误体、裸 `send()` 不带 —— 逐条结论见 `../knowledge/platform-and-protocol.md` §5 的「统一错误语义」段 | 原计划:跑一次真实失败请求,人工确认 `6-6` 的统一错误语义 | `../rounds/04` Phase 6;证据见 `../rounds/43` §2 |
| 云存储事件帧是否容忍**无空格** `42[…]` **已完成(2026-10-03 实测;`c5bbb13`)**:`parse_frame` 的实现使两种写法都接受,无需改动 | 原计划:现实现无空格且工作正常,但 `../rounds/01` 坑 5 要求"统一带空格",故以真机为准定论(不要盲改) | `../rounds/01` 坑 5;判定见 `../rounds/43` §2 |
| WS 连接行为 **已完成(2026-10-03 实测;`c5bbb13`)**:并发 `connect` 串行化成立;`close` 的事件序列与"无虚假 `Error`"的结论见 `../knowledge/platform-and-protocol.md` §5 | 原计划:并发 `connect` 串行化、断连**不发虚假 `Error`** 需真实 WS 服务人工验证 | `../rounds/04` Phase 4;证据见 `../rounds/43` §2 |

## 2. 已核验(先前怀疑的"证据不足"已消;逐条状态见下)

1.  **已完成(2026-10-03 核实;出处见右列)**:`fetch_organization_ids` 的绝对 URL 调用点只有一处且传 `None` —— `src/api/education.rs` 的 `build_request(..., None)`;全仓绝对 URL 调用点均为 `None`(`../rounds/13` §P3-5)。
2.  **已完成(2026-10-03 核实;出处见右列)**:`fetch_7day_hot_posts_iter`(`src/api/forum.rs`)端点固定为 `"/web/forums/boards/posts/7dayHot"`,`board_id` 走 `with_iter_param`(`../rounds/13` §P3-6)。
3.  **已完成(`60d5358`)**:`src/core/converse.rs` 补注释「chat 事件无字符串化载荷,刻意不二次解析(与 cloudvar 不同),勿改」(`../rounds/11` §P4-3),零行为改动。

## 3. 待方案 + 评审(已完成项与剩余项见下表)

| 项 | 说明 | 出处 |
| --- | --- | --- |
| AI 对话**指数退避重连** | 现在断线只置 `connected=false` + emit 错误;`send_and_wait` 只能等 `Timeout`,必须手动 `connect()`。云变量有退避 => 对齐前**先定会话/历史重建语义** | `../rounds/29` §2-3 |
| 举报"每类型 100 条"上限 | 现默认移除上限;是否保留取决于产品语义 | `../rounds/04` Assumptions |
| ~~大作品上传超时~~ |  已完成(修复,2026-09-26):上传请求用**请求级超时覆盖**(常量值与实测读数见 `../knowledge/nemo-runtime-and-upload.md` §6) | `../rounds/21` §8.4 N1 |
| ~~单包上传大小上限(413)~~ |  **已完成(收尾,2026-09-26)**:已加 `shared::ensure_single_package_fits` 提前报错;**阈值/速率/渠道与逐档读数只在 `../knowledge/platform-and-protocol.md` §5bis 展开**。要传更大作品需分片上传(暂无需求) | `pending-decisions.md` A5/D5 |
| ~~下载侧大文件风险~~ |  **已完成(修复,2026-10-02,`afca96c`)**:两层 —— 请求级超时 + 显式有界的大体通路(普通 API 响应仍守 10 MB 护栏)。**子项"无读超时"已结案(2026-10-03)**:原诊断经实测推翻(`timeout_global` **覆盖**响应体读取),改为**分段预算** —— 下载路径抬总预算与响应体预算,而"等响应头"仍按客户端 30 s 失败;ureq 3 无逐次读的空闲超时,判不做。常量值/逐路径枚举见 `../rounds/40-gates-cleanup-and-real-defects.md` §7,复测与落点见 `../rounds/43` §3 | `../rounds/40-gates-cleanup-and-real-defects.md` §7;`../rounds/43` §3 |

## 4. P2 性能小项(择机清)

1. **已完成(2026-10-03)**:`requests.rs` 的 Bearer 头不再逐请求构造 —— 身份槽在 `set_token` 时预计算 `"Bearer {token}"`(`IdentitySlot`),`AuthProvider::auth_header` 改为返回 `Option<(&'static str, Arc<str>)>`,请求路径只做一次引用计数克隆(内置实现全部覆写;trait 默认实现保留给外部实现者)。见 `../rounds/45`。
2. **判不做(已核实,2026-10-03)**:`PaginatedIter::build_params` 的克隆要真正省掉,必须让 `MewRequestBuilder::with_params` 改收 `&[(String, String)]`(公共面破坏)或把整条链改成 `&mut self`——而 `with_params` 按值消费 `Vec` 的现状下,`&mut self` 也省不掉克隆;`base_params` 通常只有个位数条目 ⇒ **收益 < 改动面**,判不做。
3. **已完成(2026-10-03)**:`cloudvar.rs` 的 flush 循环不再固定 `sleep(100 ms)` 轮询 —— 入队走新增的 `flush_pending` 标记 + `Notify::notify_with` 按需唤醒,`flush_loop` 用 `wait_flag(.., flush_interval, ..)` 等待;**保留超时兜底**(断线回退的批次仍会定期重试),标记在 `commands` 锁内清除以消除丢失唤醒窗口。见 `../rounds/45`。

> 本节条目一律**按符号定位**(不写 `文件:行`);原始行号见 `../rounds/29` §3。

## 5. 公开面清理(需版本策略)

`../rounds/29` §3-3 的公开面死代码已被评审删除(`FileContent`、`CodeMaoFile::{file_write,write_json,write_lines,write_text}`、`PathConfig::{fiction_file_path,token_file_path,ensure_directories}`、孤立的 `FileError::Json`;**`write_bytes` 保留**)。
**后续(2026-10-03,`../rounds/44`)**:`FileError` 类型本身也已删除,`CodeMaoFile::write_bytes` 返回 `MewResult<()>`。
遗留问题:**破坏性 API 变更的版本策略** —— 本库尚未 1.0,当前约定是"不留兼容别名直接删";若将来要保兼容,这批就是需要 `#[deprecated]` 的先例。

## 6. 已核对并放弃的

- `requests.rs::is_header_overridden` 每个默认头线性扫描:常量小数组 × 常量,收益 ≈ 0(`../rounds/29` §4)。
