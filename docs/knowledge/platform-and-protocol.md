# 平台接口与实时协议(权威事实)

> 知识库条目:与**平台服务**打交道时必须遵守的约定 —— 身份/连接参数/帧格式/服务语义差异/端点族。
> 这些是由实践教训得出的结论,违反其中任一条都表现为"看似正常但静默失败"。出处见文末。

## 1. 身份与鉴权

- 三种身份(普通 / 教育 / 评审)共用**一个全局身份槽**;登录与请求必须来自**同一身份源**,所以多账号自动举报本质是"切槽",不能把身份做成请求级参数。
- 云存储 WebSocket 的 `401` **几乎总是连接参数不匹配**,不是鉴权失败:

  | 作品编辑器 | `authorization_type` | `stag` |
| --- | --- | --- |
  | Kitten / Coco | 1 | 1 |
  | Nemo | 5 | 2 |
  | KittenN | 5 | 3 |

  用 KittenN 参数连 `ide_type=KITTEN` 的作品则报 401;改成 (1,1) 立刻 101。**正确做法是连接前查 work API 的 `ide_type` 自动推断**(本库已实现)。
- **分层归因法则**:能收到 HTTP 状态码响应,说明 TLS 已成功、请求已被解析,故问题必在业务层(URL 参数/凭证)。JA3/TLS 指纹拒绝只会表现为**握手失败或 TCP 断开**,不可能返回 401。

## 2. 实时通道(Socket.IO over WebSocket)

| 约定 | 内容 |
| --- | --- |
| 升级成功判定 | `101 Switching Protocols` 是 **1xx**,不能用 `is_success()`;统一写 `status() == StatusCode::SwitchingProtocols` |
| 事件帧解析 | `42["event","<json 字符串>"]` —— 第二个元素**可能是被 JSON 编码过的字符串**,需二次解析;解析失败保持原串恰好满足 `update_vars_done` 的 `"fail"` 语义 |
| 帧格式因服务而异 | **AI 对话端**必须 `42 ["join"]`(42 后有空格、无 payload),否则回 `code=10000000` 非法操作;**云存储端**使用无空格的 `42[…]` 且工作正常,故不应"统一",以真机为准 |
| 重复 `40` | 服务器会对客户端的 `40` 再回一个 `40`,故必须用 `join_sent` 防重,否则第二次 JOIN 被判非法并发 `41` 断开;重连后要重置 `join_sent` |
| `41`(ServerClose)语义 | **云存储**:要求断开、清理并重连;**AI 对话**:服务器自动重建会话(随后重发 `40`/`on_connect_ack`),应忽略、不清理连接资源 |
| AI 就绪信号 | 是 `on_connect_ack`,**不是** `40`;收到后再发 join |
| id 类型 | `join_ack.data.user_id` 是**字符串**;数字优先、回退字符串解析 |
| 配额耗尽 | 服务器对超配额 chat 请求**静默无视**(不回任何帧),唯一信号是超时;体检字段 `chat_count`/`remaining_times` 在 `on_connect_ack`/`join_ack` 里 |

## 3. 同步 tungstenite 的三条硬限制(与对策)

1. `read()` **无超时**、2. 无 `try_read()`、3. 不能 `split()`。
   因此单线程"事件循环与 mpsc"下,`read()` 无限阻塞会让发送通道得不到调度(服务器静默期 chat 帧卡在 channel)。
   **对策**:对底层流 `set_read_timeout(200ms)`(Plain 走 `TcpStream`、Rustls 走 `owned.sock`),把 `WouldBlock` 从致命错误改判为"回去处理发送"。待发帧最坏 ~300ms(`recv_timeout 100ms` + `read 200ms`)内必发出。
   **安全性**:不会丢半包 —— tungstenite 先把原始字节读进内部 `ReadBuffer` 再解析;rustls `StreamOwned` 已解密剩余数据留在 `conn` 内;`WouldBlock` 只是"暂时没数据"。

## 4. 并发与回调(踩坑换来的铁律)

- **Condvar 铁律**:标志写入与 `notify` 持同一把锁,标志检查与等待也持同一把锁。本库统一 `Notify::notify_with`(持锁设标志再 `notify_all`)与 `wait_flag`(持锁检查后 `wait_timeout`)。
- **不要用布尔状态做等待条件**:会漏掉"已发生过又变回去"的事件(AI 快速回复使 `receiving` 已回 false)。要用**回合计数**(`completed_round >= target`)。
- **回调必须在锁外执行**(std `Mutex` 不可重入,故回调里再调 get/set 会死锁;回调 panic 会污染锁)。标准做法:锁内 `mem::take` 取出,释放锁后于锁外执行,再重新加锁 `extend` 放回。
- **`release` profile 是 `panic="abort"`**,故 `catch_unwind` 无效,因此"回调不应 panic"是**契约**,必须写进 rustdoc。

## 5. 作品/资源端点族(已核实)

| 族 | 端点 | 备注 |
| --- | --- | --- |
| 作品详情(编辑器类型判定) | work API 的 `ide_type` / `work_type`(KN=15) | 决定 WS 连接参数与云变量参数 |
| NEMO 建作品 | `POST /nemo/v3/works/upload/<orientation>`(JSON;**作品 id 由它返回**) | **不需要资源字节**;但要有一个**合法 NEMO `.bcm` 的 `work_url`**。接口已实现(`create_nemo_work`),上传渠道已就绪(`UploadChannel::Nemo` 的凭证项目名为 `nemo_android_ios`),**尚未真机验证**(NEMO 侧删除端点未知,避免留草稿) |
| KN 建作品 | `POST /neko/works` | 本库 `create_kn_work` 已真机验证(建出草稿并过官方校验器);**反编译可选上传**也在此端点验证通过(`DecompileOptions::upload_to_account`,自建自删) |
| 资源上传 | 七牛 `upload.qiniup.com` / `up.qiniup.com`,凭证走 `GET /cdn/qi-niu/tokens/uploading?projectName=…` | 凭证**按渠道区分**:社区前端 `community_frontend`、NEMO `nemo_android_ios`(见 `UploadChannel`)。抓包实测 288 KB 上行,全是小文件 |
| 超时口径 | 客户端全局 30 s(`ClientConfig::timeout`);**上传**请求用请求级覆盖 | 9 MB 产物在慢网上要 31~35 s,故全局 30 s 下必失败(A1,已修);常量值/实测读数见 `nemo-runtime-and-upload.md` §6,下载侧大文件同类风险**已修**(见该文同节) |
| 时间校准 | `/coconut/clouddb/currentTime` | **实测(2026-10-03):`data` 是 10 位秒级时间戳**,与本地 `now_s` 同值(1791040701 vs 1791040701),**不是毫秒**;故不需要 `/1000`,现有"经 `value_to_i64` 直接当秒用"的处置正确 |
| 换绑手机号 | `PATCH /tiger/v3/web/accounts/phone/change` | **请求字段名实测为 `phone_number`**(不是 OpenAPI 写的 `phone`):只发 `captcha` 或改发 `phone` 时服务端均答 `400 phone_number: 不能为空/不能为null`;发 `phone_number` 才进入验证码校验(`403 验证码错误或已被使用`)。现有实现正确 |

- **统一错误语义(实测 2026-10-03)**:走本库统一路径(`send_checked`,即 `ClientAccess` 的 `send_and_parse` / `check_status` / `send_maybe_parse`)时,4xx/5xx 会读下服务端错误体并包进 `MewError::HttpStatus { status, body }` —— 实测 `AccountManager::update_phone_number` 的失败消息带完整 `403` 服务端 JSON。**直接 `MewRequestBuilder::send()` 则不会**(ureq 只给 `HTTP status: 404`);要带体必须显式 `.with_error_body()`。
- **换绑手机号的副作用边界**:验证码无效时服务端在进入校验前即拒绝,因此用"无效验证码"探测字段名不会改动账号手机号(2026-10-03 实测,两次探测后手机号未变)。

抓包量级参照(156.4 s / 386 连接 / 16 582 包):上下行 **1.99 MB / 22.40 MB**,`api.codemao.cn` 独占 **249 条连接** 1.46 MB,下行大头是 `creation.codemao.cn` 12.8 MB,故**控制面连接数**才是 NEMO 反编译慢的根源,不是字节量。

## 5bis. 上传:单包大小上限与速率(实测 2026-09-26)

| 项 | 实测值 |
| --- | --- |
| 单包上限 | **20 MB 可传、24 MB 被 qiniu 拒 `413`**,故落在 **20~24 MB** 之间(5/9/10/12/14/16/20 MB 全部成功) |
| 上传速率 | 约 **200 KB/s**(5 MB ≈ 24 s;20 MB ≈ 105 s) |
| 渠道 | 社区前端 `UploadChannel::Codemao`(`save_path = "convert-source"`);NEMO 走 `nemo_android_ios` |

- 实测方法:用与生产路径**同一条渠道**逐档上传临时文件(先登录,再 `file_uploader().upload(...)`),
  记录每档的成功/失败与耗时。结论对 `translate_work(upload=true)` 与反编译"上传到账号"都成立。
- => 产物超过上限时**提前报错**(`shared::ensure_single_package_fits`,别让用户白等几分钟再吃 413);
  上传请求必须单独放宽超时 —— **常量值与实测读数见 `nemo-runtime-and-upload.md` §6**(A1)。
- 要传更大的作品只能做**分片上传**(见 `../goals/convert-backlog.md`);真实 KN 产物多在 3~9 MB。

## 5ter. HTTP 超时是三段预算(实测 2026-10-03)

ureq 3 的超时不是"一个数",而是三个互不覆盖的预算:

| 旋钮 | 覆盖范围 | 本库配置 |
| --- | --- | --- |
| `timeout_global` | **整通调用**(DNS 解析到读完响应体) | `ClientConfig::timeout`,默认 30 s |
| `timeout_recv_response` | 连接建立到**响应头**收到 | 同上 |
| `timeout_recv_body` | **响应体**读取(总预算,不按次重置) | 同上 |

实测(本机慢服务器 + 真实 `ureq` agent 与 `CodeMaoClient`,2026-10-03):

| 场景 | 配置 | 结果 |
| --- | --- | --- |
| 响应头立刻发、响应体延迟 6 s | 只设 `timeout_global` 2 s | 2.0 s 失败 `timeout: global` |
| 同上 | `timeout_global` 60 s + `timeout_recv_body` 2 s | 2.0 s 失败 `timeout: receive body` |
| 响应体每 0.4 s 发 1 字节(共 20 字节) | `timeout_global` 2 s | 2.0 s 失败 `timeout: global` |
| 请求级 `timeout_global` 覆盖到 60 s | agent 另设 `timeout_recv_response` 2 s | 2.1 s 失败 `timeout: receive response` |
| 死服务端(accept 后不响应)+ 请求级覆盖 60 s | 客户端 `timeout` 2 s | 2.1 s 失败 `timeout: receive response`(未等满 60 s) |
| 响应体延迟 6 s + 请求级覆盖 60 s | 客户端 `timeout` 2 s | 6.0 s 成功返回 8 字节 |

三条结论:

1. **`timeout_global` 覆盖响应体读取**,不只是响应头。故 `../rounds/40` §7.4 记录的"global 仅覆盖响应头、body 属无读超时"不成立,勘误见 `errata.md`(2026-10-03 条)。
2. **请求级覆盖会继承 agent 的其余超时旋钮**:只改 `timeout_global` 不会把 `timeout_recv_response` 一起抬走。
3. ureq 3 **没有逐次读的"空闲超时"**:响应体中途卡住只会吃掉总预算(不按次重置),这是旋钮自身的边界,不是漏配。

=> 本库据此分段:普通接口三段一致(默认各 30 s);下载侧(`with_timeout(DOWNLOAD_TIMEOUT)`,900 s)
只抬"总预算 + 响应体预算",**等响应头仍按客户端 `timeout` 失败** —— 死连接不会在下载路径上白等 15 min,
大响应体的 900 s 预算不受影响。=> `src/utils/requests.rs` 的 `KittyCore::new` 与 `apply_request_config`。

## 依据

- `../rounds/01-websocket-pitfalls.md`(25 条坑与调试方法论;其中路径/版本号已过时,勘误见 `errata.md`)。
- `../rounds/10-ai-chat-cloudvar-test.md`(真机 AI 对话 + 云变量观察)。
- 上传上限/速率:2026-09-26 逐档实测(同渠道),记录见 `../goals/pending-decisions.md` A5。
- HTTP 三段超时:`§5ter`,2026-10-03 本机慢服务器 + 真实 `ureq` agent/`CodeMaoClient` 实测(探针为一次性集成测试,跑完已删;读数与场景表见该节)。
- `../rounds/08/09-protocol-compliance*.md`(六协议;`LoginSession` 等表述已失效)。
- `../rounds/24-nemo-upload-route-and-apis.md` §1/§2/§12(抓包与建作品证据)、`../rounds/13`(端点面)。
- 代码锚点:`src/utils/socketio.rs`(parse_frame / set_stream_read_timeout / Notify / wait_flag)、`src/core/cloudvar.rs`、`src/core/converse.rs`、`src/api/work.rs`。
