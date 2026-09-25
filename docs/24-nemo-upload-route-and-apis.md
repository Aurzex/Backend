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
