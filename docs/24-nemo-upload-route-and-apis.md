# 第二十四轮方案 — NEMO 作品「上传而非下载」与可补充的 API 清单(只出方案)

日期:2026-09-25 · 基线:`5c7df4c` · 上游:`docs/22-nemo-decompile-performance.md`(瓶颈与已落地优化)
本轮**不写代码**:给出上传方案、API 清单、端点取法与验收口径。

---

## 1. 从新抓包读到的(`temp/PCAPdroid_25_9月_13_59_21.pcap`)

解析器:`temp/pcap_v4.py`(自动识别 Ethernet/Raw IP)、`temp/pcap_upload.py`(聚焦上传链)。

| 项 | 值 |
| -- | -- |
| 时长 / 连接 / 包 | 156.4 s / 386 / 16 582 |
| 上行 / 下行 | **1.99 MB / 22.40 MB** |
| 下行大头 | `creation.codemao.cn` 12.8 MB(15 连接)、`cdn-community.codemao.cn` 3.4 MB、`creation.bcmcdn.com` 2.6 MB、`static.codemao.cn` 1.75 MB |
| 控制面 | `api.codemao.cn` **249 条连接** / 1.46 MB、`api-creation.codemao.cn` 10 条 / 0.10 MB |
| 上传 | `upload.qiniup.com` 14 条 / **249 KB↑** + `up.qiniup.com` 1 条 / **39 KB↑**(合计 288 KB) |
| 协议 | 全部 **HTTPS + ALPN h2**;无 QUIC(UDP/443 = 0);无明文 HTTP |

### 1.1 七牛上传链的时序(5 次会话,同一模式)

```text
t+67.4s  uc.qbox.me          0.7 KB↑   2.4 KB↓   ← 取上传区域配置(七牛 SDK 自己的那步)
t+67.8s  up.qiniup.com      38.9 KB↑   2.1 KB↓   ← 载荷上行
t+68.7s  upload.qiniup.com   4.6 KB↑             ← 收尾/小块
t+69.5s  upload.qiniup.com   5.9 KB↑
t+70.2s  upload.qiniup.com   2.2 KB↑
  …  t+86~89s / 117~119s / 131~132s / 152s 又各一组(45.9 / 46.4 / 61.3 / 50.5 / 11.2 / 3.1 KB)
```

两次会话的载荷是 **5–61 KB 的小文件**(不是 3.5 MB 的 `.bcm`,更不是 48 MB 资源)→ **这份抓包不是 NEMO 作品上传**,而是 App 在正常使用(发布/改资料这类小文件上传)。

它仍然给了两条结论:

1. **上传通道 = 七牛**(`uc.qbox.me` 取配置 → `up.qiniup.com` 传载荷 → `upload.qiniup.com` 收尾),与我们已有实现同族:
   - 我们:`GET /cdn/qi-niu/tokens/uploading`(`BaseKey::OpenService`,即 `open-service.codemao.cn`)取凭证 → `POST <upload_url>`(服务端下发,退化值 `https://upload.qiniup.com`)→ URL = `bucket_url + key`(`src/utils/requests.rs:1573/1645/1597`);
   - 官方 App 只是多了一步直接用七牛 SDK 的 `uc.qbox.me`。
   - **顺带解答了上一轮"`open-service.codemao.cn` 用途不明"**:它就是**上传凭证**服务。
2. **路径不可从抓包取得**(全 h2/TLS)—— 要拿 create 端点只能走 bundle 反编译(§5)。

## 2. 决定性证据:建作品根本不需要资源字节

上一轮实测的 NEMO 作品(work 194684070),本地产物里的原始 `.bcm`(3.5 MB,`user_works/<id>/<id>.bcm`):

```text
styles 数量:2101;其中 2040 个的 url 是 https CDN URL,例如
  https://creation.bcmcdn.com/490/YW5kXzIwMDJfMTg2NDI4MDJfMTk0…
  https://creation.codemao.cn/490/aW9zXzIwMDFfMTg2NDI4MDJfMF8x…
```

也就是说:

- **平台侧的作品文件(`.bcm`)靠 URL 引用资源** —— 建一个新作品只要给它 `.bcm`(或其 URL),资源由平台/CDN 自己解决;
- 我们反编译时下载的 1390 个 `user_material/*.webp` 与 `<id>.userimg`(内容是 `user_material/<sha>.webp` 的**本地路径**)属于**编辑器本地缓存格式**,与"把作品放到平台账号里"无关;
- 因此"避免 48 MB 资源开销"有两条**正交**手段:①本地侧 `skip_resources`(已实现,7.6 s);②平台侧**引用关系**建作品(资源零下载零上传)。

## 3. 上传方案(三条路线 + 组合建议)

| 路线 | 请求数 | 前置条件 | 现状 | 适用 |
| ---- | ------ | -------- | ---- | ---- |
| **A. 再创作 `fork`** | **1**(`POST /nemo/v2/works/{id}/fork`) | 作品 `fork_enable=true` | ✅ 已有 `WorkDataFetcher::fork_work`。实测:Kitten4 `Phigros` = true;本次 NEMO 慢样本 = **false**,另一个 Kitten4 = false | 允许再创作的作品,一键进账号 |
| **B. 建一个指向原 `bcm_url` 的 NEMO 作品**(推荐) | **1–2** | 需 create 端点(❌ 未知) | 入参已齐:`work_name`/`bcm_url`/`bcm_version`/`preview` 都在 `fetch_work_details` 的返回里(只读探查确认键集) | 任何公开作品:资源零下载、零上传 |
| **B′. 上传我们手里的 `.bcm` 再建作品** | **2–3** | 同 B 的 create 端点 | `.bcm` 只有 **3.5 MB**(`NemoFetcher` 本来就下载它);上传通道 ✅ 已有(`file_uploader().upload()`),`POST /nemo/qiniu/upload/business/bind` ✅ 已有 | 平台若拒绝复用他人文件 URL 时退化用 |
| **C. 本地只要文档(`skip_resources`)** | 0(资源) | 无 | ✅ 已实现(7.6 s) | 备份、离线编辑缓存、只要积木结构 |

**组合建议**:

1. 默认走 **C**(本地要结构/备份时);
2. "把作品弄进我的账号"走 **B**,被服务端拒绝时退化 **B′**,再不行退 **A**(若允许再创作);
3. B/B′ 都成功后,`translate_work` 的上传分支可以复用同一套"建作品"编排(与 Kitten/Neko 的 `create_draft_work` 并列)。

### 3.1 路线 B 的参数形状(照现有 create 写,等端点确认)

参照 `CreateKittenWorkArgs`(`src/api/work.rs:393`)、`CreateKnWorkArgs`(`:559`)与官方字段命名习惯:

```rust
CreateNemoWorkArgs {
    name: &str,            // 来自源作品 work_name(或自定义)
    bcm_url: &str,         // 路线 B 用原件 URL;路线 B′ 用刚上传得到的 URL
    bcm_version: &str,     // 来自源作品详情
    preview_url: &str,     // 来自源作品 preview
    save_type: Option<i32>,// 与 Neko/Kitten 一致(2)
    // 其余字段按端点实测补
}
```

## 4. 可补充的 API 清单

证据等级:**✅** 源码/抓包已确认 · **⚠️** 由同族端点推断 · **❌** 未知

| # | API | 用途 | 状态 | 证据 | 优先级 |
| - | --- | ---- | ---- | ---- | ------ |
| 1 | `GET /cdn/qi-niu/tokens/uploading`(OpenService) | 上传凭证 | ✅ 已有 | `requests.rs:1650`;抓包确认上传走七牛 | — |
| 2 | `POST <upload_url>`(七牛表单,`upload.qiniup.com`) | 文件上传 | ✅ 已有 | `requests.rs:1597`;抓包 `upload.qiniup.com` 14 条 | — |
| 3 | `POST /nemo/qiniu/upload/business/bind` | 把已上传文件绑到上传业务 | ✅ 已有 | `api/work.rs:1893` | — |
| 4 | `POST /nemo/v2/works/{id}/fork` | 再创作(1 请求进账号) | ✅ 已有 | `api/work.rs:169` | — |
| 5 | `GET /creation-tools/.../work-details`(含 `bcm_url`/`bcm_version`/`fork_enable`/`n_brick`) | 建作品入参来源 | ✅ 已有 | 只读探查(本轮) | — |
| 6 | 三个 create 模板:`POST /kitten/r2/work`、`/neko/works`、`/wood/project` | 写 create 的形状参照 | ✅ 已有 | `api/work.rs:459/624/803` | — |
| 7 | **`create_nemo_work`(建 NEMO 作品)** | 路线 B/B′ 的必需项 | ❌ 端点未知 | 三个 create 都在 `BaseKey::Creation` 下 → ⚠️ 同族推断 | **P0** |
| 8 | `create_qiniu_upload_business`(若 bind 前需先建 business id) | 上传业务初始化 | ❌ 未知 | `bind` 需要 `business_id`,来源未定 | P1(取决于 §5 发现) |
| 9 | 我的作品文件/列表校验(NEMO 侧) | 上传后自检 | ❌ 未知 | NEMO 侧只有 `works/list/user/published`(公共列表) | P2(可选) |
| 10 | 删除草稿(清理失败产物) | 失败回滚 | ⚠️ 未在 NEMO 侧实现 | Kitten/Neko 侧有草稿概念 | P2(可选) |
| 11 | `open-service` 其它上传相关端点(`dev-cdn-common.codemao.cn` 等) | CDN 相关 | ❌ 未知 | 抓包出现但流量极小 | P3 |

> 抓包能提供的上限就是"**哪些主机、多少连接、多少字节、什么顺序**";URL 路径与参数一律要另找来源(§5)。

## 5. 端点取法(照 `docs/20` 附录 C 的老流程)

1. **抓前端 bundle**:`kn.codemao.cn`(KN 编辑器,NEMO/KN 同页有入口)或 NEMO Web 入口的主 bundle + chunk,全文搜
   `works`、`create`、`import`、`bcm_url`、`bcm_version`、`business_id`、`upload`、`fork`;
2. **对照已知三例**:`/kitten/r2/work`、`/neko/works`、`/wood/project` 的 payload 字段名,先猜 NEMO 的字段(§3.1);
3. **抓包当辅助**:对同一次"建作品"操作跑 PCAPdroid,看那一刻新增的连接落在 `api.codemao.cn` 还是 `api-creation.codemao.cn`(能确认主机,不能确认路径),缩小猜测范围;
4. **真机最小验证**:建一个草稿 → 回读 `work-details` 断言 `bcm_url`/`n_brick`/资源可加载 → 删除;
5. 通过后按现有风格落成 `NemoWorkManager::create_nemo_work`,并接进 `convert::translate_work`(与 `create_draft_work` 并列)。

## 6. 验收口径与风险

**验收**

| 项 | 目标 |
| -- | ---- |
| 路线 B 的请求数 | ≤2(可含一次详情/一次创建),**资源请求 0** |
| 端到端耗时 | 秒级(对照:当前反编译完整资源 402 s / 并发 8 为 103 s) |
| 建出的作品 | 平台可打开、`brick` 数与源一致、造型/音频能加载 |
| 失败可清理 | 有删除手段或被明确标注为草稿 |

**风险**

| 风险 | 说明 | 缓解 |
| ---- | ---- | ---- |
| 平台拒绝复用他人 `bcm_url` | 路线 B 的前提;也可能是"必须上传到自己的 bucket" | 退化 B′,成本仅 3.5 MB |
| 建作品是写操作 | 会在账号下留作品 | 默认关闭(与现有 `upload` 选项一致)、名称标注可删、成功后返回 id |
| `bcm_version` 不匹配被拒 | 平台可能校验版本与 `bcm` 内容一致 | 直接取源作品的 `bcm_version`;不匹配时回退报错但不重试 |
| 端点猜错(⚠️ 推断项) | 服务端可能返回 404/参数错误 | 只在 §5 的真机验证通过后才落代码 |

## 7. 与 `docs/22` 的关系

- `docs/22` 给出瓶颈(请求数 × RTT)与已落地优化(并发 3.9×、`skip_resources` 53×);
- 本文补上"**不下载**"的完整路线与 API 缺口:平台侧只要一个 create 端点,就能把"40 MB 下行 + 数分钟"换成"1–2 次请求";
- 两件都做完后,`convert` 域的 NEMO 相关耗时将从"分钟级"降到"秒级 + 一次小请求"。

---

## 8. 本轮实测补充(2026-09-25 续:前端 bundle 反编译 + 抓包解密尝试)

### 8.1 前端 bundle 反编译:Web 端**没有**建 NEMO 作品的接口

| 目标 | 做法 | 结果 |
| ---- | ---- | ---- |
| `nemo.codemao.cn`(NEMO 分享/落地页) | 拉入口 + `main.js` + 全部 **23 个懒加载 chunk**(`kn-cdn.codemao.cn/nemoy/`) | 只有**读**接口与活动接口:见 §8.3 的增量清单;`get_upload_data` 是 **native bridge** 调用(上传发生在 App 侧) |
| `tools-entry.codemao.cn`(创作入口 SDK) | 拉 `tools-sdk.*.js`,抽取编辑器 → URL 映射 | 映射里是 `Roki` / `IntlRoki` / `Neko`(= `kn.codemao.cn/editor/`)等,**没有 NEMO** |
| 官方 App(APK) | 从平台自己的 `GET /nemo/v2/config/apk` 拿到地址,下载 109 MB APK | **代码被加固**:`classes.dex` 只有壳入口,真实代码在 `assets/classes0.jar` / `assets/classes.dgc`(**加密**,非 zip/dex);assets 里的 H5(workspace/helps bundle)无接口字符串 → **静态挖不动** |

结论:**NEMO 作品的创建/上传只存在于 App 内**,Web 侧不存在该入口;要拿端点只能 (a) 解密抓包,或 (b) 运行时 dump(需 root/frida)。

### 8.2 接口探测:候选路径全部不存在(方法有效,结论为负)

探测法可靠:**已知的 POST-only 路由**(`/neko/works`、`/kitten/r2/work`)用 GET 打 → **405 `40000005 请求方式不支持`**;不存在的路径 → **404**,且 404 响应体暴露服务名(`api-nemo-app`、`creation_tools…`)。

候选(`/nemo/works`、`/nemo/work`、`/nemo/v2/works`、`/nemo/v2/work`、`/nemo/v3/works`、`/nemo/project`、`/nemo/works/create` × `api-creation.codemao.cn` / `api.codemao.cn` / `nemo.codemao.cn`)→ **全部 404**。

### 8.3 增量 API 清单(全部有 bundle/响应证据)

| 端点 | 用途 | 证据 |
| ---- | ---- | ---- |
| `POST nemo/v2/user/submit/work` | **活动投稿**(body 为 `{user_phone, work_id}`,不是建作品) | `nemoy` chunk `4.5eec4bb3.js` |
| `GET nemo/v2/config/apk` | 返回官方 APK 地址(实测:`https://static.codemao.cn/nemo/apk/编程猫Nemo_4.5.0_…apk`) | 实测 + `nemoy/main.js` |
| `GET nemo/v2/activity/work/{total}` | 活动作品列表/总数 | `nemoy/main.js` |
| `GET nemo/v2/works/web/` | 作品推荐/详情(Web) | 同上 |
| `GET api/work/info/`、`api/work/praise/` | 作品信息 / 点赞 | 同上 |
| `GET tiger/user`、`POST oauth/{wechat,qq}/login`、`tiger/wechat/config/js_sdk` | 用户信息 / 第三方登录 / 微信 JS-SDK | 同上 |

### 8.4 抓包解密:已排除到"只剩 keylog 与记录不匹配"

准备工作与已排除项(**都有量化证据**):

| 步骤 | 结果 |
| ---- | ---- |
| keylog ↔ pcap 配对 | ✅ **`PCAPdroid_25_9月_13_59_21` 388 个 ClientHello 中 386 个能在 keylog 里找到密钥**(339 个 TLS1.2 `CLIENT_RANDOM` + 47 个 TLS1.3);另一个 pcap 0 命中(与文件名相符) |
| 协商套件 | TLS1.2 `0xc02f`(AES128-GCM)339 条,`0x1302`(TLS1.3 AES256-GCM)47 条;ServerHello.random 末 8 字节为 `DOWNGRD\x01`(TLS1.3 降级哨兵,属正常) |
| 记录切分 | 客户端序:`22:174`(ClientHello)→ `22:37`(ClientKeyExchange)→ `20:1`(CCS)→ `22:40`(**加密 Finished**)→ `23:1123`(app data)→ `21:26`(加密 alert) |
| 我的实现 | 自造测试向量(同 PRF/AAD/nonce 路径)加密→解密 **通过**;TLS1.2 按 CCS 划分加密 epoch、序号从 0 起、AAD=`seq‖type‖ver‖ptLen`、nonce=`fixed_iv‖explicit` 均为 RFC 写法 |
| 穷举尝试 | 对"加密 Finished"与首条 app data 共 **约 12 万组参数组合**(密钥/IV 交换、seed 顺序、AAD 版本 {记录, 0x0303, 0x0301}、AAD 长度 {16,24,40}、nonce 4 种构造、seq {0,1,2})→ **全部认证失败** |

**现状**:未能解密;现象是"client_random 匹配、但 master secret 解不开记录"这一自相矛盾的局面(自测证明实现无误)。需要一个第三方实现来裁决 —— 见 §8.5。

### 8.5 下一步(需要你的一条命令,我这边 sudo 要密码)

```bash
sudo pacman -S wireshark-cli                      # 提供 tshark
# 1) 先验证能否解密(能出 h2 帧 = keylog 可用)
tshark -r temp/PCAPdroid_25_9月_13_59_21.pcap \
       -o tls.keylog_file:temp/PCAPdroid_25_9月_13_59_21.keylog \
       -Y http2 -c 5
# 2) 端点清单(这才是我们要的)
tshark -r temp/PCAPdroid_25_9月_13_59_21.pcap \
       -o tls.keylog_file:temp/PCAPdroid_25_9月_13_59_21.keylog \
       -Y 'http2.headers.path' -T fields -e http2.headers.path | sort -u
# 3) 找建作品/上传相关(带 SNI 与请求体)
tshark -r temp/PCAPdroid_25_9月_13_59_21.pcap \
       -o tls.keylog_file:temp/PCAPdroid_25_9月_13_59_21.keylog \
       -Y 'http2.headers.method == "POST"' \
       -T fields -e tls.handshake.extensions_server_name -e http2.headers.path -e http2.data.data
```

- 若 tshark **能解密**:把 2)/3) 的输出贴给我(或你自己看),我据此把 `create_nemo_work` 的路径与 payload 字段一次写进 §3.1,并落成 API;
- 若 tshark **也解不开**:即证明这份 keylog 与抓包虽同源(随机数匹配)但不配套,本轮解密路线到此为止 —— 回到 §3 的 A/C 路线(或等一侧新的抓包 + keylog 同批导出)。

### 8.6 我的临时工具(都在 `temp/`,已 gitignore)

| 文件 | 用途 |
| ---- | ---- |
| `temp/pcap_v4.py` / `pcap_upload.py` / `pcap_conn3.py` | 纯标准库 pcap 解析:主机/连接/时序/七牛上传链 |
| `temp/dec/decrypt.js` + `match.js` | TLS 解密尝试(keylog → 记录解密 → HPACK 解 h2);`match.js` 做 keylog↔pcap 配对量化 |
| `temp/web_dig.py` | 前端入口 bundle 抓取 + 关键词抽取(本轮用它挖了 nemoy 与 tools-entry) |

---

## 9. 抓包解密成功后的结论(2026-09-25 续二)

用 `tshark -o tls.keylog_file:…` 解开 `temp/PCAPdroid_25_9月_13_59_21.pcap`(1729 个 HTTP/2 帧):

| 发现 | 证据 |
| ---- | ---- |
| **KN 建作品 payload 与我们的实现逐字段一致** | `POST api-creation.codemao.cn/neko/works` 实体(`{"bcm_version":"0.27.4","save_type":1,"name":"我的作品","work_url":"…/922/user-files/*.bcmkn","preview_url":"…jpeg","stage_type":1,"work_classify":0,"n_blocks":0,"n_roles":2,"n_scenes":1}`),带 `work_id` 时为**更新**;`pic_need_check_file_url` 指向一个 `.json` |
| **KN 删除草稿** | `DELETE /neko/works/330803731?force=2`(我们已有 `delete_kn_draft`) |
| **上传 token 的项目名按编辑器区分** | KN:`projectName=neko` + `insertOnly=true` + `filePaths=922%2Fuser-files%2F*.bcmkn\|*.json`;NEMO:`projectName=nemo_android_ios` + `filePaths=<b64>.bcm` 与 `<b64>.cover`(封面走 `upload.qiniup.com/putb64/-1/key/<b64 路径>.cover`) |
| **平台自己的"用本地文件打开编辑器"入口** | `GET tools-entry.codemao.cn/?fileUrl=<作品文件 URL>&appId=1&signature=123456&api_env=production&toolType=KN`(先上传作品文件,再把 URL 交给该入口) |
| **统一作品列表(跨编辑器)** | `GET api.codemao.cn/creation-tools/v1/works/list/user?offset&limit` 一次性返回 84 条,字段 `work_id`/`work_type`/`bcm_version`/`work_url`/`preview`/`publish_time`/`update_time`;实测:`work_type=8` 全是 NEMO(`.bcm` + `.cover`,如 `work_id=330803896` 的 `work_url` 尾部与本机抓到的 NEMO 上传文件同名 `…_8nHvxlvm.bcm`)、`work_type=1` 是 Kitten(预览 URL 走 `120/kitten/…`)、KN 自己的列表返回 `type=15` |
| **这份抓包里没有 NEMO 建作品调用** | 全量 POST 清单只有 `shence/collection/qiniup/sentry/**neko/works**`;搜 `330803896`(NEMO 新作品 id)在**全部解密流量里不存在**;两个 pcap 均无 QUIC(`udp.port==443` 计数 0),keylog 也只有标准 TLS 标签 ⇒ 该作品的注册调用发生在**另一次会话**(很可能就是 `PCAPdroid_25_9月_12_11_14.pcap`,而它**没有配套 keylog**,只有 SNI 可读:主要是 `api.codemao.cn`,符合 `/nemo/**`、`/creation-tools/**` 被网关路由到 `api-nemo-app` 服务) |
| **NEMO 建作品路由扫描:0 命中** | 先校准判别器:`GET` 已知 POST-only 路由(`/neko/works`)= **405 "请求方式不支持"**;`api.codemao.cn` 上方法不符 = **404 + `error_code 40103015`**,路由不存在 = **404 + `Path-Not-Found@Common`**,未带鉴权的已知路由 = **406 + 40100004**。随后扫 408 个候选(`/creation-tools/v1/**`、`/nemo/v2|v3/**` × `/works|/work|/user/works|…` × `/create|/save|/upload|/submit|…`),**全部 `Path-Not-Found@Common`**,对照组正常 |

**结论**:NEMO 侧"建作品"这一个端点仍缺,且**只能从运行时拿**——要么给 `PCAPdroid_25_9月_12_11_14.pcap` 配一份 keylog
(用 PCAPdroid 的 TLS-keylog 导出,和上次同样的方式),要么在 NEMO App 里"保存/发布作品"时重新抓一次包 + 导出 keylog;
拿到后 `tshark -o tls.keylog_file:… -Y http2 -T fields -e http2.headers.authority -e http2.headers.method -e http2.headers.path`
即可一眼看出那条 POST。**不需要**再花时间在 Web bundle / APK 静态挖(App 代码已加固:加密的 `classes0.jar`/`classes.dgc`)。

---

## 11. 第三份抓包(17:12,NEMO App + KN 编辑器混合)的结论

用 `temp/PCAPdroid_25_9月_17_12_46.keylog` 解密后,拿到了 **NEMO 侧"保存已有作品"的完整链路**,
以及一个此前完全没有的端点。**新建作品那一步仍然没有被抓到**(理由见 §11.3)。

### 11.1 NEMO 保存(已有作品)的完整链路

| 步 | 调用 | 证据 |
| -- | ---- | ---- |
| 1 | `GET open-service.codemao.cn/cdn/qi-niu/tokens/uploading?projectName=nemo_android_ios&cdnName=qiniu&filePaths=<b64 文件名>` | 4 次 token(2 次 `.bcm` + 2 次 `.cover`) |

> 第 1 步的文件名形如 `and_2002_<uid>_0_<毫秒时间戳>_<随机后缀>.bcm`(由 App 生成),`.cover` 同族。
| 2 | `POST upload.qiniup.com/`(multipart,上传 `.bcm`) | 2 次 |
| 3 | `POST upload.qiniup.com/putb64/-1/key/<b64 路径>.cover`(封面 base64 上传) | 2 次 |
| 4 | **`POST https://api.codemao.cn/nemo/qiniu/upload/business/bind`** | **2 次**;完整体:`{"business_id":"330816585","url_list":["https://creation.bcmcdn.com/490/YW5kXzIwMDFfMTI3NzAxMTRfMF8xNzkwMzI3NTkxMjY5X1d2cmxqNkpL.cover"]}`(另一条 `business_id=330816598`) |

⇒ 也就是说:**"保存" = 上传 `.bcm` + 上传 `.cover` + 把封面 URL 绑到作品 id**。
`bind` 是通用绑定服务(`business_id` + `url_list`),绑的是**已存在**的作品 id。

### 11.2 探测方法自证 + 新增已验证端点

**方法自证**:用"无鉴权 GET + 错误码判别"(`40103015`=路由存在但方法不对、`Path-Not-Found@Common`=无此路由)
去探抓包**已证实**的 `POST /nemo/qiniu/upload/business/bind`,得到 `404 + 40103015` —— 与抓包一致,
⇒ §9 那套判别器可信。

| 端点 | 方法 | 结果 |
| ---- | ---- | ---- |
| `/nemo/qiniu/upload/business/bind` | POST | ✅ 存在(抓包 + 探测双证),体见 §11.1 |
| `/nemo/v2/works/list/user` | GET | ✅ 存在;无鉴权 406 `40100004 参数不能为空`(要令牌)⇒ **NEMO 草稿列表** |
| `/nemo/v2/works/list/user/published?user_id=<id>&offset=&limit=` | GET | ✅ **200 公开可用**(无需登录)⇒ 某用户已发布的 NEMO 作品列表 |
| `/nemo/v2/works/business/total?user_id=`、`/nemo/v2/home/banners`、`/nemo/v3/user/level/info`、`/nemo/v3/user/level/report/login`、`/nemo/v3/dialog/get`、`/creation-tools/v1/home/{discover,especially/course}` | GET | ✅ 同轮抓到 |
| `/nemo/qiniu/upload/business{,/create,/bind/list}`、`/nemo/v2/works/{list/user/draft,user,save,draft}`、`/nemo/v2/work` | — | ❌ `Path-Not-Found@Common`(无此路由) |

### 11.3 为什么"新建作品"还是没抓到

- 两条 `bind` 的作品 id(`330816585` / `330816598`)**在本抓包全部 body 里都搜不到**,
  也没有任何 POST 提交 `work_url`/`bcm_version`(只有 GET 列表响应里带这些字段)⇒
  这一轮是**在保存两份已存在的作品**(App 本地已知这两个 id),不是新建。
- 与 §9 的第一份抓包结论一致(那份也只有"上传",没有注册)。

⇒ **仍缺的一步**:在 NEMO App 里**新建一个空白作品**再保存/发布,同时开 PCAPdroid 的
TLS-keylog 导出。拿到后优先在这几个族里找:`/nemo/v2/**`、`/nemo/v3/**`、`/creation-tools/v1/**`
(判别器已经能区分"存在/不存在",扫一遍只要几十秒)。

### 11.4 对"方案 B"(上传建作品)的影响

- **链路已明确 3/4**:token(`nemo_android_ios`)→ 上传 `.bcm` → 传 `.cover` → `bind`。
- 缺的只有**第 0 步:作品 id 从哪来**。拿到它之后,`bind` 的参数已经逐字段清楚,
  实现是一层薄封装(现在不实现,避免造出"能上传但落不到作品"的半成品)。
- 顺带新增两个**可用于转换功能**的读接口:`/nemo/v2/works/list/user`(草稿,需令牌)、
  `/nemo/v2/works/list/user/published`(公开)—— 前者正好能用来"枚举自己的 NEMO 作品再批量转化"。
