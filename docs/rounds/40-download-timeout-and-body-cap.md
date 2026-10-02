# 第 40 轮:下载侧大文件 —— 请求级超时 + 10 MB 隐性上限

> 提交:`afca96c`。承接 `../goals/platform-backlog.md` §3「下载侧大文件风险」。

## 1. 背景(backlog §3「下载侧大文件风险」)

全局请求超时 30 s(`ClientConfig::default`);上传侧早在 `rounds/21` §8.4 N1 用请求级
`UPLOAD_TIMEOUT = 600 s` 修过,**下载侧未修**。

## 2. 两个问题(不是一个)

1. **超时**:下载请求受全局 30 s 限制。
2. **体量(更硬)**:`ureq::Body::read_to_vec` / `read_to_string` 自带 **10 MB** 上限
   (`MAX_BODY_SIZE`,ureq 3.4.0)—— 大作品**在超时之前**就 `BodyExceedsLimit` 失败,只放宽超时没用。
   证据:本机语料 `download/compile/raw/原气骑士 且听风吟-编辑版.bcm4` = **61 MB**。

## 3. 改动(`afca96c`)

- 唯一下载通路 = `src/core/convert/shared.rs::CodeMaoHttpClient`(Kitten / NEMO / NEKO / Coco / WOOD
  五种抓取 + 资源批量下载 + `fetch_kitten_source_document` 全走它)。三个方法统一:
  `.with_timeout(DOWNLOAD_TIMEOUT)` + 大体读取。
- `src/utils/requests.rs` 新增:
  - `pub const DOWNLOAD_TIMEOUT: Duration` = 900 s(换算:30 MB 上传 228 s ⇒ ~130 KB/s;
    63 MB 按 100 KB/s ≈ 645 s ⇒ 900 s 留 ~40% 余量)。
  - `pub const MAX_DOWNLOAD_BODY_BYTES: u64` = 256 MiB(内存护栏,**不是**协议限制)。
  - `CodeMaoClient::response_to_{binary,string,json}_large(response, url)`:显式自管上限读取,
    超限报可操作的 `MewError::ResponseTooLarge { url, limit, received }`。
  - 普通 `response_to_*`(10 MB 护栏)**保持原样**给所有普通 API 响应;上传侧两处未动;全局默认未动。
- `tests/convert_edit_harvest.rs::fetch_bytes` 同修(它拉平台编辑格式 `.bcm4`,是下载路径之一)。

## 4. 一次性证明(本地 HTTP 服务,跑完已删)

- 40 s 慢响应:全局 30 s ⇒ `Err("timeout: global")` @30.9 s;新下载路径 ⇒ `Ok(2)` @46.1 s。
- 12 MB 响应体:新大体通路 ⇒ `Ok(12582912)`;旧 10 MB 助手 ⇒
  `Err("the response body is larger than request limit: 10485760")`。
- **诊断(顺带发现,已写进知识库)**:body 分片慢送(40 B / 40 s)在全局 30 s 下**成功** ⇒
  `timeout_global` **只管到响应头**,不覆盖 body 流式读取。所以本轮解决的是
  "**首字节/响应头 > 30 s**"与"**单个响应体 > 10 MB**"两类失败;真正慢的 body 传输是
  **另一类**("无读超时")问题,未修。

## 5. 下载路径枚举(改造前后)

| # | 路径(文件:函数) | 下载内容 | 改前 | 改后 |
| - | ---------------- | -------- | ---- | ---- |
| — | `core/convert/shared.rs` `CodeMaoHttpClient::{get_json,get_binary,get_text}` | **全部下载的单一咽喉** | 全局 30 s + 10 MB | `DOWNLOAD_TIMEOUT` 900 s + 256 MiB |
| 1 | `decompile/editors.rs` `KittenFetcher::fetch` | Kitten `player/load` 元信息 | 30 s | 900 s |
| 2 | 同上(作品文档) | **Kitten 编译版作品文档(MB 级)** | 30 s + 10 MB | 900 s + 256 MiB |
| 3 | `NemoFetcher::fetch` | NEMO `source/public` 元信息 | 30 s | 900 s |
| 4 | 同上(作品 JSON) | **NEMO `.bcm` 作品 JSON** | 30 s + 10 MB | 900 s + 256 MiB |
| 5 | `NemoResourceManager` 封面 | NEMO `.cover` | 30 s + 10 MB | 900 s + 256 MiB |
| 6 | `CocoFetcher::fetch` | Coco `load` 元信息 | 30 s | 900 s |
| 7 | 同上(作品文档) | **Coco 编译版作品文档** | 30 s + 10 MB | 900 s + 256 MiB |
| 8 | `NekoFetcher::fetch` | NEKO `published-work-detail` 元信息 | 30 s | 900 s |
| 9 | 同上(密文体) | **NEKO 密文作品体(base64)** | 30 s + 10 MB | 900 s + 256 MiB |
| 10 | `WoodFetcher::fetch` | **WOOD 发布体(整份作品)** | 30 s + 10 MB | 900 s + 256 MiB |
| 11 | `decompile/mod.rs::download_resources_parallel` | **资源字节(NEMO/WOOD 造型/素材;单作品实测 1390 个 / 49 MB)** | 30 s + 10 MB/请求 | 900 s + 256 MiB |
| 12 | `decompile/mod.rs::fetch_work_info` | 作品元信息 | 30 s | 900 s |
| 13 | `decompile/mod.rs::fetch_kitten_source_document` | 元信息 + **Kitten 编译版文档(公共 API)** | 30 s + 10 MB | 900 s + 256 MiB |
| 14 | 入口 `core/convert/mod.rs::{translate_work,translate_work_in}` | 走 1–13(fetcher);`keep_source` 只影响**上传**源文件,不新增下载路径 | — | — |
| 15 | `tests/convert_edit_harvest.rs::fetch_bytes` | **平台编辑格式 `.bcm4`(采集工具)** | 30 s + 10 MB | `DOWNLOAD_TIMEOUT` + `response_to_binary_large` |
| — | `api/auth.rs::fetch_admin_captcha` | 管理员验证码图片(几 KB) | 30 s + 10 MB(足够,**未改**) | 同前 |
| — | 上传侧 `utils/requests.rs` | 上传(已 A1 修过) | `UPLOAD_TIMEOUT` 600 s(**未动**) | 同前 |

**哪些当前就可能超过 10 MB**:#2 / #4 / #7 / #9 / #10 / #13(作品文档)、#11(单个大素材)、
#15(编辑格式 `.bcm4`,语料实测 61 MB)。修复前它们会先撞 10 MB 上限。

## 6. 未做

- 未给 body 流式读取设 `timeout_recv_body`(ureq 3 未配置,属另一类"无读超时"问题)。
- 未动普通 API 响应的 10 MB 护栏(那是**内存护栏**,刻意保留)。
