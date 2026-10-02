# 真机冒烟测试 — AI 对话与作品云变量(2026-08-12)

> 目的:验证 `core/converse.rs`(AI 对话)与 `core/cloudvar.rs`(作品云变量)在真实环境下的可用性,并核验同期完成的「云变量编辑器类型自动识别」。
> 基线:HEAD `70a6b44`。方法:当时的临时示例 `examples/smoke_test.rs`。
> **该示例从未入库**(`examples/` 目录当前不存在,`git log` 里也没有它的痕迹),当时仅在本机运行;
> 不要把它连同凭据提交进来。当前可复用的真机入口:`tests/live_features.rs`(登录/AI 对话/云变量)
> 与 `tests/convert_live.rs`(作品转化),凭据统一从 `data/test-config.json` 读。

## 测试账号(脱敏)

> **凭据不入库**:真实账号与密码只放在 `data/test-config.json`(`data/` 已被 `.gitignore` 忽略,
> 集成测试从这里读取);本文档只记录脱敏后的账号别名与测试结论。

| 账号(脱敏) | 角色     | AI 对话结果     |
| --- | --- | --- |
| 账号 A      | 普通用户 | 已完成 完整流式回复 |
| 账号 B      | 普通用户 | 已完成 完整流式回复 |
| 账号 C      | 普通用户 | 已完成 完整流式回复 |

三个账号均以 `PasswordV2` 登录成功。登录接口偶发 `HTTP error: timeout: global`(网络抖动),重试即成功,非代码问题。

## 测试作品(云变量)

| 作品 ID   | 类型    | 编辑器识别 | 私有变量             | 公有变量                              | 云列表            | 结果 |
| --- | --- | --- | --- | --- | --- | --- |
| 195038626 | KITTEN4 | Kitten     | 3(设置/成绩存档/rks) | 0                                     | 0                 | 已完成   |
| 194684070 | NEMO    | Nemo       | 4                    | 5(奥姆/第五人格/活跃度/云数据库/玩家) | 0                 | 已完成   |
| 140290349 | KITTEN4 | Kitten     | 1                    | 9(a房间1-9)                           | 2(排行榜 170+ 条) | 已完成   |
| 103791894 | NEMO    | Nemo       | 4                    | 5(t/q/w/r/e 聊天室变量)               | 0                 | 已完成   |
| 325806995 | NEKO    | KittenN    | 0                    | 1(宜刻乐代打总成绩)                   | 0                 | 已完成   |

5/5 全部连接成功、数据就绪、变量/列表完整读取。

## 发现并修复的 Bug:`connect()` 未等待 Socket.IO 就绪

**严重度:。AI 对话"超时未开始回复"的直接原因。**

### 现象

首次测试,账号 A 的 `send_and_wait` 稳定报 `Timeout("AI 未开始回复")`,三个账号均出现同一现象。

### 根因(日志证据)

```text
[INFO] AI 对话 WebSocket 已建立
[INFO] 聊天消息已发送: 你好,请用一句话介绍你自己   ← 过早!
[INFO] 握手成功,发送连接请求                        ← 服务器 0 帧此刻才到
[INFO] Socket.IO 连接成功
[INFO] 连接确认 - 剩余对话次数: 0
```

`connect()` 原实现只等 WebSocket 层连接(`connected` 标志,establish 后立即置位),`send_and_wait` 随即发送 chat 帧——此时 Socket.IO 握手(`0` 帧转为 `40`)与 JOIN 均未完成,服务器静默丢弃,`join_ack` 也收不到。Python 参考实现(`deepser.py`)以 `connect()` 后 `sleep(2)` 规避该问题;Rust 端缺少这一等待。

### 修复

`src/core/converse.rs`:

1. `ChatInner` 新增 `io_ready`(收到 `40` 置位)与 `joined`(收到 `join_ack` 置位)两个原子标志;
2. `connect()` 改为等待 `joined`(而非仅 WebSocket 层),以 `join_ack` 为准,比 Python 的 `sleep(2)` 更可靠;
3. `send_message` 以 `joined` 为守卫,握手/JOIN 未完成返回 `NotConnected`;
4. `establish()` / `close()` / 断连路径均重置两个标志。

修复后日志顺序正确:`握手成功 -> Socket.IO 连接成功 -> 加入成功 -> 聊天消息已发送 -> 完整流式回复`。

## 新功能:云变量编辑器类型自动识别

**背景**:云存储 WS 连接参数 `authorization_type`/`stag` 因编辑器而异(Kitten=`1/1`,Nemo=`5/2`,KittenN=`5/3`,Coco=`1/1`)。此前 `CloudBuilder` 默认 Kitten,遇到 NEMO/KN 作品直接返回 401(实测 194684070/103791894/325806995 均受影响)。

**实现**(`src/core/cloudvar.rs`):

- `CloudInner.editor` 由 `EditorType` 改为 `Mutex<Option<EditorType>>`:`None` = 连接时自动识别;
- 新增 `detect_editor(work_id)`:查询作品详情(`/creation-tools/v1/works/{id}` 的 `type` 字段)映射编辑器,失败回退 Kitten;
- 识别结果缓存,自动重连不重复请求;`CloudBuilder::editor()` 仍可显式覆盖。

**用法**:`CloudBuilder::new(work_id).build()` 即可,无需传参。

## 有价值的经验

1. **`chat_count: 0` 是账号配额,不是代码 bug**。服务器对配额不足的 chat 请求**静默无视**(不回任何帧),超时是唯一信号——与 `docs/rounds/01-websocket-pitfalls.md` 坑 9 完全一致。测试中该字段在多次连接间由 0 增至 6,疑为配额刷新滞后。
2. **"服务器静默"不等于"代码 bug"**:先查看 `on_connect_ack` 的 `chat_count`/`remaining_times` 诊断字段,再查协议。
3. **时序问题优先检查日志顺序**:chat 帧在握手前发出,依据日志中 `聊天消息已发送` 与 `握手成功` 的先后顺序即可定位,无需抓包。
4. **NEMO/KN 作品的云变量必须用对应编辑器参数**,否则 401;现在库内自动识别,调用方无需感知。
5. **登录接口偶发全局超时**:重试即可,非代码缺陷;批量测试账号时逐个执行比一次性并发更稳定。
6. **测试工具设计**:当时的 `examples/smoke_test.rs` 支持多账号与自动识别作品类型,可一键重新运行。
   **注意**:它把账号密码当命令行参数传,这类工具不要入库(本仓库里它从未入库);
   现在的回归入口是 `tests/live_features.rs` / `tests/convert_live.rs`,凭据只从
   `data/test-config.json`(gitignored)读。

## 变更文件

- `src/core/converse.rs` — AI 对话连接时序修复(io_ready/joined 标志)
- `src/core/cloudvar.rs` — 云变量编辑器类型自动识别
- `examples/smoke_test.rs`(本机临时示例,**从未入库**;含凭据,不要再提交)
- `README.md` — 云变量示例补充自动识别说明
- `.gitignore` — 忽略 `/examples`

`cargo test`:3 passed,0 failed。
