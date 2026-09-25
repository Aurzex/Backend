# NEMO 反编译性能:瓶颈分析与优化(2026-09-25)

范围:`NemoDecompiler` / `NemoResourceManager`(现 `src/core/convert/decompile/editors/nemo.rs`)与同型的 WOOD。
结论:慢在**请求数 × RTT**(1 390 次串行 GET),不在带宽;改并发后 **402 s → 103 s**,`skip_resources` 后 **7.6 s**。

---

## 1. 现象与量级

| 观测 | 值 | 来源 |
| ---- | -- | ---- |
| 真机反编译一个 NEMO 作品(work 194684070) | **~525 s**(旧代码,串行) | 第二十一轮实测(`compile_live::decompile_nemo_works`) |
| 该作品的资源文件数 | **1 390 个 / 48.1 MB** | 上次反编译产物 `download/compile/蛋仔派对2…_194684070/user_material/` |
| 文档本体 | `.bcm` + `.userimg` + `.meta` + `.cover` ≈ **4.1 MB** | 同目录 `user_works/` |
| 平均每请求耗时(串行) | **~0.29 s**(402 s ÷ 1 390) | 本轮 A/B 实测 |

即:**99% 的产物体积是造型资源,而 98% 的耗时是这 1 390 次请求的往返延迟**。文档本体只有 4 MB。

## 2. 抓包侧证(`temp/PCAPdroid_25_9月_12_11_14.pcap`,官方 App 打开同一作品)

自写解析器(`temp/pcap_conn3.py`,纯标准库;PCAPdroid 的 VPN 隧道里目的地址是网关,所以按归一化五元组 + TLS ClientHello 定客户端方向):

| 项 | 值 |
| -- | -- |
| 时长 / 连接数 | 264.7 s / 342 条 |
| 上行 / 下行 | 2.06 MB / **43.91 MB** |
| `creation.bcmcdn.com` | **31.2 MB↓**,22 条连接(最大单连接 18.8 MB / 20 s) |
| `cdn-community.bcmcdn.com` | **8.1 MB↓**,6 条连接 |
| `creation.codemao.cn` / `static.codemao.cn` | 2.19 MB / 0.62 MB |
| `api.codemao.cn` | 218 条连接,但只有 0.85 MB↑ / 0.80 MB↓(控制面,碎但小) |
| 上传 | `upload.qiniup.com` 0.34 MB↑ + `api.qiniu.com`(七牛签名) |

两条可直接用的结论:

1. **资源下载就是那个量级**(官方 40 MB ≈ 我们的 48 MB),绕过它只能靠"不下载"(见 §5);
2. **官方把 40 MB 摊在 28 条 keep-alive 连接上**(≈1.4 MB/连接),不是每文件一条连接 —— 说明瓶颈是"我们没有并发",不是"连接没复用"。

## 3. 代码侧根因

- `NemoResourceManager::download_resources` 遍历 `styles.styles_dict[*].url`,**逐个 `get_binary` 串行下载**;
- 连接复用没问题:`KittyCore` 只建一次 `ureq::Agent`(`src/utils/requests.rs:624`),keep-alive 在;
- 没有去重(同一 url 在多造型里复用会重复下)、没有"跳过已存在"(重跑等于重下)、失败只 `warn!`;
- `save_core_files` 还会单独下 1 个封面(可忽略);
- WOOD 的 `download_images` 是同一个形状(逐个下载,无并发)。

**所以慢的根因是:串行 × 1 390 次往返。**

## 4. 优化与实测(A/B,同一作品、同一网络)

改动(`c98c3a2`):

| 改动 | 说明 |
| ---- | ---- |
| `download_resources_parallel(client, tasks, concurrency)`(`decompile/mod.rs`) | 固定线程数消费共享队列(`std::thread::scope`,**不新增依赖**);按 url 去重;跳过"已存在且非空"的文件(内容寻址命名 → 同名即同内容,重跑近零成本);每 200 个打进度 |
| 失败重试 | 主轮失败的**串行重试一轮**。实测 32 并发会被 CDN 限流丢文件,重试补回(比"一律降并发"更划算) |
| `DecompileOptions::resource_concurrency`(默认 **8**) | 资源下载并发数 |
| `DecompileOptions::skip_resources`(默认 false) | 只产出文档与元数据(`.bcm`/`.userimg`/`.meta`/`.cover`),**1390 次请求 → 0** |
| NEMO / WOOD 共用 | WOOD 的 `download_images` 一并改造 |

实测(work 194684070,资源数/字节数逐项核对):

| 模式 | 用时 | 资源文件 |
| ---- | ---- | -------- |
| 串行 `resource_concurrency(1)` | **402.1 s** | 1 390 / 48.1 MB |
| 并发 8(**新默认**) | **103.3 s** | 1 390 / 48.1 MB |
| 并发 16 | 91.7 s | 1 390 / 48.1 MB |
| 并发 32 | 126.8 s | **1 388**(被限流丢 2) |
| 并发 32 + 重试 | 106.2 s | **1 390** ✓ |
| `skip_resources(true)` | **7.6 s** | 0 |

- 默认 8:**3.9× 提速**,且留有余量(16 只快 11%,32 反而更慢且会丢文件);
- `skip_resources`:**53× 提速**;
- 只读文档本体的场景(转化、备份元数据、上传前处理)应该用它。

## 5. 「上传编译好的 NEMO 文件,而不是下载资源」这条路

用户提的思路是对的,而且**所需的一半能力已经在手里**:

| 需要的东西 | 现状 |
| ---------- | ---- |
| NEMO 作品的本机文件集(`.bcm` / `.userimg` / `.meta` / `.cover`) | ✅ `skip_resources(true)` **7.6 s** 就产出(不再下 48 MB) |
| 文件上传通道(七牛) | ✅ `CodeMaoClient::file_uploader().upload(path, UploadChannel::Codemao, save_path)`;抓包也确认官方走 `upload.qiniup.com` + `api.qiniu.com` 签名 |
| 平台侧「用这个文件建一个 NEMO 作品」的 API | ❌ **未知**(`src/api/work.rs` 有 Kitten/Neko/Wood/Coco 的 create,没有 NEMO) |
| 建作品所需的入参 | ✅ 作品详情里直接给了 **`bcm_url` + `bcm_version`**(只读探查 `WorkDataFetcher::fetch_work_details`,键集含 `bcm_url`/`bcm_version`/`work_name`/`preview`/`fork_enable`…) |

### 5.1 三条可行路线(按"要写平台"的程度排序)

| 路线 | 请求数 | 前置条件 | 现状 |
| ---- | ------ | -------- | ---- |
| **A. 再创作(fork)** | **1** | 作品**允许**再创作(`fork_enable=true`) | ✅ 已有 `WorkDataFetcher::fork_work`(`POST /nemo/v2/works/{id}/fork`)。实测:配置里的 Kitten4 `Phigros` = true,而本次的慢样本 NEMO `蛋仔派对2…` = **false**,另两个 = false → 只能对部分作品用 |
| **B. 建一个指向同一 `bcm_url` 的新 NEMO 作品** | **1–2** | 需要 NEMO 的 create 端点 | ❌ 端点未知;入参已具备(`bcm_url` + `bcm_version` + `work_name` + `preview`)。参数形状可照 `CreateKnWorkArgs`/`CreateKittenWorkArgs`(`api/work.rs:442`/`608`) |
| **C. 本地只要文档(`skip_resources`)** | 0(资源) | 无 | ✅ 本轮已实现,7.6 s |

**B 是用户那条思路的最短实现**:资源由平台侧自行处理(它们本来就在 CDN 上),我们既不下载 48 MB,也不需要上传 48 MB。缺的只是一个端点。

### 5.2 取端点的具体做法

1. 抓 **NEKO/NEMO 前端 bundle**(与 `docs/rounds/20` 附录 C 同法:下载主 bundle + chunk,搜 `works`/`create`/`import`/`bcm_url`/`bcm_version`),
   定位"新建/导入作品"的请求构造;
2. 与现有三个 create(`/kitten/r2/work`、`/neko/works`、`/wood/project`)对照猜路径族(都在 `BaseKey::Creation` 下);
3. 真机验证:用配置里的 NEMO 作品(或先 fork 一个可再创作的)建一次,回读详情确认 `bcm_url`/`n_brick`/资源可加载;
4. 成功后按现有风格加 `NemoWorkManager::create_nemo_work(CreateNemoWorkArgs { name, bcm_url, bcm_version, preview, save_type, … })`,
   并把它接到 `convert::translate_work`(与 Kitten/Neko 的 `create_draft_work` 并列)。

也就是说,剩下唯一缺口是那个 create 接口的**路径与参数**。抓包拿不到:PCAPdroid 里全是 TLS,只能看到主机名(`api.codemao.cn`、`api-creation.codemao.cn`、`creation.codemao.cn`、`open-service.codemao.cn`、`*.bcmcdn.com`、`upload.qiniup.com`),看不到 URL 路径。

可行的取法(本仓库已成功过一次的老办法,见 `docs/rounds/20` 附录 C):抓官方 **NEKO/KN Web 编辑器或 NEMO 入口页的前端 bundle**,搜"新建作品/导入作品"的请求构造(关键词:`works`、`create`、`bcm_version`、`work_url`、`upload_status`),用真机验证。落地后即可实现:

```text
反编译(skip_resources) → 上传 .bcm → create_nemo_work(work_url, bcm_version) → 得新作品 id
```

净效果:**把"40 MB 下行 + 8 分钟"换成"4 MB 产出 + 1 次上传"**。

## 6. 抓包里的 API 清单(能加什么、不能加什么)

| 主机 | 用途判断 | 我们是否已有 |
| ---- | -------- | ------------ |
| `api.codemao.cn` | 主 API(218 条连接,最大流量) | ✅ |
| `api-creation.codemao.cn` | 创作域 API | ⚠️ 有 `creation_base_url` 相关调用,未见独立入口 |
| `creation.codemao.cn` | 作品文件/资源 | ✅ 反编译取源 |
| `*.bcmcdn.com`(`creation` / `cdn-community`) | 资源 CDN(NEMO 造型、KN 资源) | ✅(URL 由文档给出,直接 GET) |
| `upload.qiniup.com` + `api.qiniu.com` | 文件上传 + 签名 | ✅ `file_uploader()` |
| `open-service.codemao.cn` | 不明(流量很小) | ❌ 需 bundle 侧确认 |
| `shence-data` / `bugly` / `umeng` / `aliyun log` | 官方 App 的埋点与崩溃上报 | 🚫 不该接(与业务无关) |

**诚实结论**:抓包能给出"主机 + 流量构成 + 时序",但**给不出 URL 与参数**(全 TLS),所以本轮没有直接新增 API。要新增只能走 bundle 反编译那条路(§5),这也是 `open-service.codemao.cn` 与 NEMO create 接口的下一步。

## 7. 复现方式

```bash
# A/B 基准(会写平台?否 —— 只读作品,输出到临时目录)
# 1) config 里加一条 {"id":194684070,"kind":"NEMO"}(data/ 已 gitignore)
# 2) 跑一个临时基准(本轮用后已删,按 §4 的三种模式各跑一遍即可):
#    resource_concurrency(1) / (8) / skip_resources(true)
cargo test --test compile_live decompile_nemo_works -- --ignored --nocapture

# 抓包分析(需 PCAPdroid 的 pcap;脚本在 temp/,不进仓库)
python3 temp/pcap_conn3.py temp/PCAPdroid_25_9月_12_11_14.pcap
```
