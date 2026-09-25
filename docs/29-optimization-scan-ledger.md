# 第二十九轮记录 — 全仓优化扫描(子代理)与处置

日期:2026-09-25 · 基线:`78f49ae` · 来源:只读扫描 `OptimizationScan`(证据均为 `文件:行号` + 代码原文)

> 处置原则:两条 **P0 已执行并验证**(见 §1);**P1/P2 先方案后执行**(§2/§3),
> 「待验证」项(§4)在没有证据前不动代码。

---

## 1. P0(已执行)

| # | 位置 | 问题 | 处置 | 验证 |
| - | ---- | ---- | ---- | ---- |
| 1 | `src/core/cloudvar.rs:2439` | `merge_commands(batch.clone())`:每个 flush 周期(默认 100ms)把整批命令的 `Value` 深拷贝一次,只为失败回退还能用原 `batch` | 改为**借用式分组** `plan_uploads(&batch) -> UploadPlan<'_>`(只记 `&Value`/`&[Value]`),失败回退直接用原 batch;公有/私有判定口径**刻意保持旧样**(只看 `data.action == "set"`,不用 `Variable.private`)⇒ 帧字节不变 | 新增单测 `plan_uploads_tests`(分组/同 cvid 合并/帧序列化逐字节);**真机** `live_features::cloud_variables` 通过(平台接受了新分组的帧) |
| 2 | `src/api/auth.rs:1187-1192` | `CloudAuthenticator` 在**每次云变量重连**与**每份 NEKO 作品取件**都重建(`cloudvar.rs:2253`、`decompile/editors/simple.rs:271`),而时差缓存在实例字段里 ⇒ 每次白付一个串行 `currentTime` RTT | 时差提到**进程级缓存**(`time_difference_cache`,10 分钟 TTL 兜底休眠/NTP 漂移;实例字段保留、行为不变) | 新增单测覆盖命中/过期(绝对过期断言,避开并行测试竞态);82 项单测 + 真机 live_features 3/3 通过 |

## 2. P1(需方案 + 评审)

| # | 位置 | 问题 | 建议 | 风险/备注 |
| - | ---- | ---- | ---- | --------- |
| 1 | `decompile/mod.rs:236-258` | 反编译侧两级并发不折算:batch × `resource_concurrency`(默认 8)无上限 | ✅ **已落地**:`decompile_batch` 把"作品级 × 单作品资源级"的**总线程封顶 16**(常量 `RESOURCE_DOWNLOAD_BUDGET`)。**口径按评审纠正**:下载是 I/O 密集,不能用 `available_parallelism` 折算(docs/22 实测 8=103s / 16=92s / **32=127s 且 CDN 限流丢文件**),否则低核机器折到近串行、高核机器放宽到限流区 | 默认 `batch_concurrency = 1` 时行为完全不变(仍是 8) |
| 2 | `tests/live_features.rs`、`tests/convert_live.rs` | **真机门在 CI 永不真跑且静默放行**:配置缺失时 `eprintln` + `return` ⇒ 显示 pass 却没验任何东西(它们不是 `#[ignore]`) | ✅ **已落地(评审采纳方案 b)**:新增 `BACKEND_REQUIRE_LIVE=1` 严格模式,判据**不止"配置文件不存在"**,还覆盖"解析失败 / accounts 为空 / 没有所需 kind 的作品 / 登录失败"(评审补充要求);默认不设 ⇒ 行为与之前完全一致 | 验证:默认缺配置仍跳过式通过;严格模式 + 完整配置通过;**严格模式 + 缺配置 ⇒ 明确失败**并打印原因(三个方向都实测) |
| 3 | `converse.rs:846-856` | AI 对话断线后只置 `connected=false` + emit 错误,没有 cloudvar 那样的指数退避重连;断连后 `send_and_wait` 只能等 Timeout,必须手动 `connect()` | 评估加退避重连(session/历史重建语义要想清);至少把可见行为写进文档 | 中:产品语义 |

## 3. P2(可选,建议择机清)

1. `requests.rs:340-342` / `:978-980`:每次请求 `format!("Bearer {token}")` 与 `Arc<str>→String` 克隆各一次。
2. `requests.rs:1237-1260`:`PaginatedIter::build_params` 每翻一页克隆全部 base_params + 两个键串。
3. ✅ **已删除**(评审自行 `grep` 复核过零调用方):`filedata.rs` 的 `FileContent` 枚举、`CodeMaoFile::{file_write,write_json,write_lines,write_text}`、`PathConfig::{fiction_file_path,token_file_path,ensure_directories}`、孤立的 `FileError::Json`;`write_bytes` 保留(auth 的 captcha 仍在用)。~~死代码(已核实零调用方,`grep` 全仓)~~:
   - `filedata.rs` 的 `FileContent` 枚举、`CodeMaoFile::{file_write,write_json,write_lines,write_text}`
     (其中 `write_json` 用 `to_string_pretty` 整串中间分配,与 convert 域 `FileService::write_json` 的**流式**实现重复 ——
     正是方案 23 P0-1 淘汰掉的老写法;13 处 `write_json` 调用全是 `FileService` 的);
   - `PathConfig::{fiction_file_path,token_file_path,ensure_directories}`(零调用方)。
   - ⚠️ 这些是**公开面**上的工具函数,删除属破坏性变更 ⇒ 需评审确认后再删(或先标注 `#[deprecated]`)。
4. `simple.rs:219`:`CocoDecompiler::decompile` 对唯一的 `Arc<Value>` 做 `(*data).clone()`,可 `Arc::try_unwrap` 免拷。
5. `auth.rs:108-139` 的 `AccountStatus` 与 `requests.rs` 的 `Identity` 是同一"身份"概念的平行枚举(靠 `to_identity()` 转换)—— `docs/19` 之后的新裂缝,易漂移。
6. `nemo.rs::get_sha` 每次 `clone()` 64 字节 hex;可返回 `&str`。
7. `cloudvar.rs:2410` flush 用固定 100ms `sleep` 轮询(空闲也醒);可换 `Condvar`/`Notify` 按需唤醒。

## 4. 待验证(无证据不动)

- `decompile/mod.rs:513-519` 下载失败串行重试用 `line.split(": ").next()` 从错误串反解 URL —— 若 URL 或 error 文本含 `": "` 会截断失配(未见真实 URL 含该子串)。
- `cloudvar.rs:2380-2399` 重连达 5 次后仅 warn 并永久放弃(是否应降频继续,取决于产品意图)。
- `requests.rs::is_header_overridden` 每个默认头做一次线性扫描(多数请求 `extra_headers` 为空,未实测占比)。

## 5. 与其它轮次的关系

- P1-1(反编译侧并发折算)与 `docs/23` §3 P2-11「两级预算」同源 ⇒ 可并到那一轮;
- P2-3(死代码)与「删死重量」的仓库约定一致,但涉及公开面 ⇒ 需要评审与版本策略。
