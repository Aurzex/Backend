# 第四十三轮记录 — A 组落地:两处协议假设、分段超时、UI 输入错误传播

## 0. 一句话
目标库 A 组的六条全部结案:两处协议假设由真机实测消掉(`currentTime` 为秒级、换绑手机号字段名是 `phone_number`),下载侧"body 无读超时"的原诊断被实测**推翻**并改为分段超时预算,UI 输入改为可传播错误(消掉 EOF 死循环与 `panic=abort` 下的进程终结),两处存而不用的注入字段删除,云变量重连放弃的行为写进 rustdoc。

## 1. 任务背景
2026-10-03 的指令为"先完成 A 组,再盘点架构性与性能方向"。A 组指 `../goals/` 里"不需拍板、成本低"的一批:两处真机实测(C9)、WS 四项人工验证(C10)、下载体超时、云变量重连文档化,以及两项公共面决定(D6 的 2) 与 4) )。

## 2. 真机实测(2026-10-03)

全部经一次性集成测试打真实服务(探针跑完即删,未入库),账号取自本机 `data/test-config.json`。

| 项 | 探针 | 结果 |
| --- | --- | --- |
| `currentTime` 单位 | 登录后 `GET /coconut/clouddb/currentTime` | `data = 1791040701`,同刻本地 `now_s = 1791040701`、`now_ms = 1791040701977` ⇒ **10 位秒级**,不是毫秒 |
| 换绑手机号字段名 | `PATCH /tiger/v3/web/accounts/phone/change` 三种载荷 | 只发 `captcha`:400 `phone_number: 不能为空, phone_number: 不能为null`;发 `phone`:同样的 400 ⇒ **`phone` 被忽略**;发 `phone_number`:进入校验,403 `验证码错误或已被使用` ⇒ 字段名正确 |
| 统一错误语义 | `AccountManager::update_phone_number`(走 `send_and_parse` → `send_checked`) | `Err = HTTP 403: {"error_category":"AC3", … "验证码错误或已被使用" …}` ⇒ **服务端错误体完整带出** |
| 裸 `send()` 的对照 | 直接 `MewRequestBuilder::send()` 打不存在的路由 | `Err = HTTP error: http status: 404`,**不带体** ⇒ 要带体必须显式 `.with_error_body()`(统一路径内部已加) |
| 并发 `connect` | 两线程各调一次 `CloudConnection::connect`(同一实例,屏障对齐) | 两次均 `Ok`、`is_connected = true`、数据就绪、私有变量 3 条 ⇒ 串行化成立,无双读线程、无竞态报错 |
| 断连事件序列 | `on_connection` 订阅后 `connect_and_wait` → `close` | 事件序列 `[Opened, Closed { was_connected: true }]` ⇒ **无虚假 `Error`**(主动关闭路径;网络断开路径由 `on_connection_lost` 发同类 `Closed`) |
| 无空格 `42[…]` 帧 | 读 `socketio::parse_frame` | 只剥 `"42"` 前缀,其余交给 `serde_json::from_str`,该函数会跳过前导空白 ⇒ **有无空格都接受**;真机 `cloud_variables` 通过可交叉印证 |

副作用边界:换绑手机号的三次探测都用了无效验证码,服务端在进入校验前即拒绝,账号手机号未变动(实测后手机号仍为原值)。

## 3. 分段超时:推翻第 40 轮的"body 无读超时"诊断

`../rounds/40` §7.6 记的"`timeout_global` 仅覆盖响应头、body 属无读超时"不成立。本机慢服务器 + 真实 `ureq` 3.4.2 agent 复测六种形态:

| 场景 | 配置 | 结果 |
| --- | --- | --- |
| 响应体延迟 6 s(响应头立刻发) | 仅 `timeout_global` 2 s | 2.0 s 失败 `timeout: global` |
| 同上 | `global` 60 s + `recv_body` 2 s | 2.0 s 失败 `timeout: receive body` |
| 响应体每 0.4 s 发 1 字节 | `global` 2 s | 2.0 s 失败 `timeout: global` |
| 响应头延迟 6 s + 请求级覆盖 `global` 60 s | agent 另设 `recv_response` 2 s | 2.1 s 失败 `timeout: receive response` ⇒ 请求级覆盖**继承**其余旋钮 |
| 死服务端 + 请求级覆盖 60 s | 客户端 `timeout` 2 s | **改前**会等满 60 s;**改后** 2.1 s 失败 |
| 响应体 6 s + 请求级覆盖 60 s | 客户端 `timeout` 2 s | 6.0 s 成功返回 8 字节(大响应体预算不受影响) |

结论:ureq 3 的超时是**三段预算**(总调用 / 到响应头 / 读响应体),请求级覆盖只改显式设置的旋钮;真正的边界是**没有逐次读的空闲超时**(响应体中途卡住只会吃掉总预算)。

代码落点(`src/utils/requests.rs`):
- `KittyCore::new`:三段都按 `ClientConfig::timeout` 配置 ⇒ 普通接口语义不变。
- `apply_request_config`:请求级覆盖同时设 `timeout_global` 与 `timeout_recv_body`,**不**动 `timeout_recv_response` ⇒ 下载路径(`DOWNLOAD_TIMEOUT`,900 s)拿到大响应体预算,而"等响应头"仍按客户端 30 s 失败,死连接不再在下载路径上白等 15 min。

新增两条**永久回归测试**(在 `requests.rs` 的 `#[cfg(test)]` 内):`agent_carries_all_three_timeout_budgets`(三段预算都落在 agent 上)与 `request_level_override_keeps_header_budget`(本地死服务端上,抬总预算后仍必须在 20 s 内失败)。语义与读数同时写进 `../knowledge/platform-and-protocol.md` §5ter,原诊断按勘误登记。

## 4. UI 输入的错误传播(原 D6 的 4) )

- **原问题**:`ProcessorUi::input/choose/menu` 返回 `String`/`Option<usize>`,读入失败无处表达 ⇒ 旧实现用 `expect`(release 是 `panic = "abort"`,直接终结进程);而 stdin 返回 `Ok(0)`(EOF)时旧实现把空串当正常输入,`choose`/`menu` 会**立刻再次读取并拿到空串** ⇒ 死循环刷屏。
- **改法**:三个方法改为 `MewResult<…>`;`read_line` 把 **EOF 也当错误**(`UnexpectedEof`,带中文说明);`ReportConsole::{run, process_flow, view_done, pick_type, pick_status, ask_action}` 随之改为返回 `MewResult` 并逐调用点 `?`;`src/main.rs` 的两处输入按 `String` 错误映射,登录失败与控制台失败都以退出码 1 结束。
- **证据**:管道喂空 stdin 跑真二进制 —— 旧行为是 `expect` 或死循环,现为 `管理员用户名: 管理员登录失败: 读取输入失败: I/O error: stdin 已到末尾(输入被关闭或重定向为空)` 并退出(退出码 1)。`choose`/`menu` 的循环体已由 `?` 保证不再重读。

## 5. 两处存而不用的注入字段(原 D6 的 2) )

`core::registry::ReportFetcher` 与 `core::pipeline::ActionRegistry` 各有一个只写不读的 `client` 字段(带 `#[allow(dead_code)]` 与"不删否则成假注入"的注释)。实测两处构造器都**真的**用参数去喂内部闭包(`let c = client.clone()` 进注册表 / `register_report_handler!` 宏),因此:

- **删除字段**(连带 `#[allow(dead_code)]` 与注释),**保留** `new_with_client` —— 原建议"删字段 + 删 `new_with_client`"会删掉 `services.rs::new_with_config_and_client` 与 `pipeline.rs::apply_with_client` 依赖的真注入路径,退化成全局客户端。此口径修正记在第 42 轮之后的 D6 条目里。
- 结果是纯删除:`ReportFetcher` 只剩 `registry` 字段,`ActionRegistry` 只剩 `handlers`。

## 6. 云变量重连放弃的文档化
`CloudBuilder::max_reconnect_attempts` 与 `ConnectionEvent` 的 rustdoc 写明:退避为 `reconnect_interval * 2^(n-1)`(上限 5 分钟),超过次数后**永久放弃**且**不再发连接事件**,调用方看到的最后一个事件是断线时的 `Closed`,`wait_for_data` 会一直等到自己的超时 ⇒ 需要"已放弃"信号时按超时判定(对应 `convert-backlog.md` §2 第 4 条)。

## 7. 未做与判不做
- **逐次读的空闲超时**:ureq 3 无此旋钮(响应体卡住只吃总预算)。要更细的上界得自建读循环与看门狗线程,收益是"卡住的下载提前失败",代价是多一层并发结构 ⇒ 本轮登记为**判不做**,语义与边界写进知识库。
- A 组之外的方向(架构与性能)本轮只做盘点,结论按目标库纪律登记到 `../goals/`(架构四条见 `../goals/infra-backlog.md` §6,性能五条见 `../goals/convert-backlog.md` §7),不在本文展开。

## 8. 验证

| 项 | 读数 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过,零告警 |
| `cargo test` | 退出码 0:14 个目标 **144 通过 / 0 失败 / 11 忽略**;其中 `--lib` 目标 134 项(含本轮新增的两条超时门 + 两腿语料扫描器全绿) |
| 真机目标 | `compile_live` 1 通过 1 忽略、`convert_live` 2 通过 3 忽略、`live_features` 3 通过、`repo_hygiene` 2 通过 |
| 空 stdin 冒烟(真二进制) | `管理员用户名: 管理员登录失败: 读取输入失败: I/O error: stdin 已到末尾(输入被关闭或重定向为空)`,**退出码 1** |

一次性探针(真机六项 + 本地慢服务器九种形态)跑完即删,未入库;可复跑部分已固化为 §3 的两条永久回归测试。

## 依据
- 代码提交:`d24b781`(`fix(timeout,ui)`:分段超时预算 + UI 输入错误传播 + 删两处存而不用的注入字段 + 重连放弃 rustdoc)。
- 实测与探针:2026-10-03 一次性集成测试(已删),读数见本文 §2/§3/§4。
- 知识库:`../knowledge/platform-and-protocol.md` §5(端点族新增两行与两条实测)与 §5ter(三段超时);`../knowledge/errata.md`(第 40 轮 §7.4 诊断勘误)。
- 目标库:A 组条目与 D6 的 2)/4) 的处置见 `../goals/pending-decisions.md`、`../goals/platform-backlog.md` §1/§3、`../goals/convert-backlog.md` §2。
