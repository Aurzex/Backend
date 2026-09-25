# NEMO 作品的运行与上传(知识)

> 知识库条目:**NEMO 作品在平台上是怎么被打开/创建/存储的**,以及我们能不能绕过"下载 40 MB"。
> 方案与取证过程见 `docs/rounds/22-*` §5/§6、`docs/rounds/24-*`、`docs/rounds/27-*` §9–§12。

## 1. NEMO 是什么

- NEMO 是 KN 的前身编辑器;作品文件由**资源包(`resourceZip`)+ 积木文档**组成,体积可达 40 MB 级。
- 本库对 NEMO 的反编译是**明文 JSON 直取**(fetcher),**没有解密步骤**;慢的原因是**资源要逐个下载**(见 `convert-performance.md` §2)。
- 平台侧 `work_type` 判别:Kitten 系 = 1、Nemo = 3、CodeGame = 5、KN = 15(创作侧)。

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
- 上传**大作品(≈9 MB)在全局 30 s 超时下会失败**(实测 31.2 s / 35.5 s 超时)⇒ 见目标库(上传需独立超时或分片)。

## 依据

- `docs/rounds/22-nemo-decompile-performance.md` §5(这条路)、§6(抓包里的 API 清单)。
- `docs/rounds/24-nemo-upload-route-and-apis.md` §1(抓包量级)、§2(决定性证据)、§8–§13(前端 bundle 反编译 + 端点落地清点)。
- `docs/rounds/27-nemo-to-kn-conversion-plan.md` §9(官方 12 步管线)、§11/§12(落地与方向表)。
- 代码锚点:`src/api/work.rs`(`create_nemo_work` / `create_kn_work` / `delete_work`)、`src/core/convert/mod.rs`(translate + 上传编排)。
