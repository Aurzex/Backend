# NEMO 作品的运行与上传(知识)

> 知识库条目:**NEMO 作品在平台上是怎么被打开/创建/存储的**,以及我们能不能绕过"下载 40 MB"。
> 方案与取证过程见 `docs/rounds/22-*` §5/§6、`docs/rounds/24-*`、`docs/rounds/27-*` §9–§12。

## 1. NEMO 是什么

- NEMO 是 KN 的前身编辑器;作品文件由**资源包(`resourceZip`)+ 积木文档**组成,体积可达 40 MB 级。
- 本库对 NEMO 的反编译是**明文 JSON 直取**(fetcher),**没有解密步骤**;慢的原因是**资源要逐个下载**(见 `convert-performance.md` §2)。
- 平台侧 `work_type` 判别:Kitten 系 = 1、Nemo = 3、CodeGame = 5、KN = 15(创作侧)。

**官方的完整"保存/新建"链路是四步**(第三份抓包逐字段解出):

```text
1. GET  /cdn/qi-niu/tokens/uploading?projectName=nemo_android_ios&cdnName=qiniu&filePaths=<b64 文件名>   → 上传凭证
2. POST <七牛 upload_url>(.bcm 字节) → work_url;封面另走 upload.qiniup.com/putb64/-1/key/<b64>.cover
3. POST /nemo/v3/works/upload/<1|2>  {name, work_url, preview, bcm_version, …}   → **返回新作品 id**
4. POST /nemo/qiniu/upload/business/bind  {business_id: <新 id>, url_list: [cover]}  → 绑封面
```

⇒ **作品 id 由第 3 步返回**(第 4 步只是绑封面)。本库已实现第 3 步(`NemoWorkManager::create_nemo_work`)与第 4 步;
第 1/2 步现由 `UploadChannel::Nemo` 提供(`projectName=nemo_android_ios`),**但没有真机验证过**
(NEMO 侧删除端点未知 ⇒ 建了草稿擦不掉;见 `docs/rounds/30` §4)。

## 2. 决定性证据:**建作品不需要资源字节**

第三份抓包(nemo App + KN 编辑器混合)里找到了 **NEMO 建作品**调用:`POST /nemo/v2/works` 是**表单**提交(`orientation` 等),**不含资源字节**,返回作品 id / previewUrl。
⇒ "把作品建起来"与"作品里的资源"在协议上是**两件事**;资源由平台侧按 URL 拉取或后续上传补齐。

> 本库已据此实现 `create_nemo_work`(真机验证建出草稿并回读成功)。KN 侧对应的是 `create_kn_work`(同样真机验证)。

## 3. 资源上传走 Qiniu

抓包实测:上行总共 1.99 MB,其中上传渠道是 `upload.qiniup.com`(14 条 / 249 KB)+ `up.qiniup.com`(1 条 / 39 KB)—— **全是小文件**。控制面 `api.codemao.cn` 反而占 **249 条连接** / 1.46 MB。
⇒ 慢的是**请求数**,不是带宽。这也是"资源下载并发封顶 16"的依据。

## 4. 两条路线的现实形态

| 路线 | 形态 | 状态 |
| ---- | ---- | ---- |
| **A. 下载 + 本地重编译** | 反编译 NEMO(含资源)→ 转 KN → 上传产物建 KN 作品 | ✅ 已落地(`convert_live` 真机通过) |
| **B. 不下载,直接建作品** | 上传**已有的** `.bcm`/资源引用 + `create_nemo_work(work_url, bcm_version)` | ⚠️ 建作品端点已备好;但**要产出合法 NEMO 文件**才能把我们的产物塞回去 ⇒ 依赖 KN→NEMO(**平台无对照实现,不做**) |

⇒ 路线 B 的现实用法是**把 NEMO 作品转出来**(NEMO→KN→Kitten4),而不是把产物塞回 NEMO。

## 5. 编译耗时与上传无关

官方 App 打开同一作品时的编译发生在**设备侧**;我们的反编译耗时(NEMO 2m42s → 13s)全部来自"取字节 + 找资源 + 下载资源",与上传/编译无关。**不要**用"减少编译"的思路去优化上传路线。

## 6. 与 KN 作品的关系(KN 侧的事实)

- KN 作品的 `.bcmkn` 只有通过 **NEKO 播放器详情接口**取回时才加密(见 `work-file-formats.md` §5);平台存储的产物本身是明文。
- 本库建的 KN 草稿已实测:平台回读 `work_type=15`、`bcm_version=0.16.2`、`work_url=…bcmkn`,该文件交官方 `validateBcm` 通过。
- 上传**大作品曾必失败**:≈9 MB 产物在全局 30 s 超时下超时(实测 31.2 s / 35.5 s)。**已修**(2026-09-26):
  上传请求改用请求级超时 `UPLOAD_TIMEOUT = 600 s`(`MewRequestBuilder::with_timeout`,ureq 3 per-request config),
  其余请求仍走全局 30 s。
- 下载侧同类问题**已修**(2026-10-02,`afca96c`):转换域取作品/资源的唯一通路 `CodeMaoHttpClient`
  (`src/core/convert/shared.rs`)三个方法都带请求级超时 `DOWNLOAD_TIMEOUT = 900 s`;普通接口请求仍走全局 30 s。
- 另修一层**更硬的**下载限制:`ureq::Body::read_to_vec`/`read_to_string` 默认**只吃 10 MB**
  (`MAX_BODY_SIZE`),大作品会在**超时之前**先 `BodyExceedsLimit` 失败。下载侧改走显式有界的
  `response_to_{binary,string,json}_large`(`MAX_DOWNLOAD_BODY_BYTES = 256 MiB` 内存护栏,
  超限报带 URL / 上限 / 已读字节的 `MewError::ResponseTooLarge`);普通 API 响应仍守 10 MB 护栏。
- ⚠ 实测细节:`ureq` 的 `timeout_global` 只覆盖到**响应头**(40 s 才回的响应在 30 s 被掐),
  **不覆盖 body 流式读取**(40 B 分 40 s 才送完,30 s 全局照样成功)。所以上面两处改动解决的是
  "**首字节/响应头 > 30 s**"与"**单个响应体 > 10 MB**"两类失败;真正慢的 body 传输本就不受全局超时约束,
  属**另一类"无 body 读超时"问题**(未修)。详见 `docs/rounds/40-download-timeout-and-body-cap.md`。

## 依据

- `docs/rounds/22-nemo-decompile-performance.md` §5(这条路)、§6(抓包里的 API 清单)。
- `docs/rounds/24-nemo-upload-route-and-apis.md` §1(抓包量级)、§2(决定性证据)、§8–§13(前端 bundle 反编译 + 端点落地清点)。
- `docs/rounds/27-nemo-to-kn-conversion-plan.md` §9(官方 12 步管线)、§11/§12(落地与方向表)。
- 代码锚点:`src/api/work.rs`(`create_nemo_work` / `create_kn_work` / `delete_work`)、`src/core/convert/mod.rs`(translate + 上传编排)。
