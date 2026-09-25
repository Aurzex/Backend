# 平台接口与实时协议(权威事实)

> 知识库条目:与**平台服务**打交道时必须遵守的约定 —— 身份/连接参数/帧格式/服务语义差异/端点族。
> 这些是踩过坑换来的结论,违反其中任一条都表现为"看似正常但静默失败"。出处见文末。

## 1. 身份与鉴权

- 三种身份(普通 / 教育 / 评审)共用**一个全局身份槽**;登录与请求必须来自**同一身份源**,所以多账号自动举报本质是"切槽",不能把身份做成请求级参数。
- 云存储 WebSocket 的 `401` **几乎总是连接参数不匹配**,不是鉴权失败:

  | 作品编辑器 | `authorization_type` | `stag` |
  | ---------- | -------------------- | ------ |
  | Kitten / Coco | 1 | 1 |
  | Nemo | 5 | 2 |
  | KittenN | 5 | 3 |

  用 KittenN 参数连 `ide_type=KITTEN` 的作品 ⇒ 401;改成 (1,1) 立刻 101。**正确做法是连接前查 work API 的 `ide_type` 自动推断**(本库已实现)。
- **分层归因法则**:能收到 HTTP 状态码响应 ⇒ TLS 已成功、请求已被解析 ⇒ 问题必在业务层(URL 参数/凭证)。JA3/TLS 指纹拒绝只会表现为**握手失败或 TCP 断开**,不可能返回 401。

## 2. 实时通道(Socket.IO over WebSocket)

| 约定 | 内容 |
| ---- | ---- |
| 升级成功判定 | `101 Switching Protocols` 是 **1xx**,不能用 `is_success()`;统一写 `status() == StatusCode::SwitchingProtocols` |
| 事件帧解析 | `42["event","<json 字符串>"]` —— 第二个元素**可能是被 JSON 编码过的字符串**,需二次解析;解析失败保持原串恰好满足 `update_vars_done` 的 `"fail"` 语义 |
| 帧格式因服务而异 | **AI 对话端**必须 `42 ["join"]`(42 后有空格、无 payload),否则回 `code=10000000` 非法操作;**云存储端**当前用无空格的 `42[…]` 且工作正常 ⇒ 不要"统一",以真机为准 |
| 重复 `40` | 服务器会对客户端的 `40` 再回一个 `40` ⇒ 必须用 `join_sent` 防重,否则第二次 JOIN 被判非法并发 `41` 断开;重连后要重置 `join_sent` |
| `41`(ServerClose)语义 | **云存储** = 要求断开 + 清理 + 重连;**AI 对话** = 服务器自动重建会话(随后重发 `40`/`on_connect_ack`),应忽略、不清理连接资源 |
| AI 就绪信号 | 是 `on_connect_ack`,**不是** `40`;收到后再发 join |
| id 类型 | `join_ack.data.user_id` 是**字符串**;数字优先、回退字符串解析 |
| 配额耗尽 | 服务器对超配额 chat 请求**静默无视**(不回任何帧),唯一信号是超时;体检字段 `chat_count`/`remaining_times` 在 `on_connect_ack`/`join_ack` 里 |

## 3. 同步 tungstenite 的三条硬限制(与对策)

1. `read()` **无超时**、2. 无 `try_read()`、3. 不能 `split()`。
   ⇒ 单线程"事件循环 + mpsc"下,`read()` 无限阻塞会**饿死发送通道**(服务器静默期 chat 帧卡在 channel)。
   **对策**:对底层流 `set_read_timeout(200ms)`(Plain 走 `TcpStream`、Rustls 走 `owned.sock`),把 `WouldBlock` 从致命错误改判为"回去处理发送"。待发帧最坏 ~300ms(`recv_timeout 100ms` + `read 200ms`)内必发出。
   **安全性**:不会丢半包 —— tungstenite 先把原始字节读进内部 `ReadBuffer` 再解析;rustls `StreamOwned` 已解密剩余数据留在 `conn` 内;`WouldBlock` 只是"暂时没数据"。

## 4. 并发与回调(踩坑换来的铁律)

- **Condvar 铁律**:标志写入与 `notify` 持同一把锁,标志检查与等待也持同一把锁。本库统一 `Notify::notify_with`(持锁设标志再 `notify_all`)+ `wait_flag`(持锁检查 → `wait_timeout`)。
- **不要用布尔状态做等待条件**:会漏掉"已发生过又变回去"的事件(AI 快速回复使 `receiving` 已回 false)。要用**回合计数**(`completed_round >= target`)。
- **回调必须在锁外执行**(std `Mutex` 不可重入 ⇒ 回调里再调 get/set 会死锁;回调 panic 会污染锁)。标准姿势:锁内 `mem::take` 取出 → 释放锁 → 锁外执行 → 重新加锁 `extend` 放回。
- **`release` profile 是 `panic="abort"`** ⇒ `catch_unwind` 无效 ⇒ "回调不应 panic"是**契约**,必须写进 rustdoc。

## 5. 作品/资源端点族(已核实)

| 族 | 端点 | 备注 |
| -- | ---- | ---- |
| 作品详情(编辑器类型判定) | work API 的 `ide_type` / `work_type`(KN=15) | 决定 WS 连接参数与云变量参数 |
| NEMO 建作品 | `POST /nemo/v2/works`(**form**,`orientation`) | **不需要资源字节**(决定性证据),返回作品 id/previewUrl;本库 `create_nemo_work` 已真机验证 |
| KN 建作品 | KN 编辑器的 create 族 | 本库 `create_kn_work` 已真机验证(建出草稿并过官方校验器) |
| 资源上传 | Qiniu(`upload.qiniup.com` / `up.qiniup.com`) | 抓包实测 288 KB 上行,全是小文件 |
| 时间校准 | `/coconut/clouddb/currentTime` | **返回形态/单位未实测**(毫秒则会误当秒 ⇒ 见目标库) |

抓包量级参照(156.4 s / 386 连接 / 16 582 包):上下行 **1.99 MB / 22.40 MB**,`api.codemao.cn` 独占 **249 条连接** 1.46 MB,下行大头是 `creation.codemao.cn` 12.8 MB ⇒ **控制面连接数**才是 NEMO 反编译慢的根源,不是字节量。

## 依据

- `docs/rounds/01-websocket-pitfalls.md`(25 条坑与调试方法论;其中路径/版本号已过时,勘误见 `errata.md`)。
- `docs/rounds/10-ai-chat-cloudvar-test.md`(真机 AI 对话 + 云变量观察)。
- `docs/rounds/08/09-protocol-compliance*.md`(六协议;`LoginSession` 等表述已失效)。
- `docs/rounds/24-nemo-upload-route-and-apis.md` §1/§2/§12(抓包与建作品证据)、`docs/rounds/13`(端点面)。
- 代码锚点:`src/utils/socketio.rs`(parse_frame / set_stream_read_timeout / Notify / wait_flag)、`src/core/cloudvar.rs`、`src/core/converse.rs`、`src/api/work.rs`。
