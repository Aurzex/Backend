# 第三十轮记录 — 反编译可选「上传到当前账号」(备份 / 搬家)

日期:2026-09-25 · 基线:`c637f18` · 上游:`docs/rounds/24`(NEMO/KN 建作品端点)、`docs/rounds/22`(NEMO 上传路线)

> **需求(用户提出)**:既然 KN 与 NEMO 的建作品途径都摸清了,就在**反编译**里加一个可选项:
> 把反编译出来的作品原样建到用户自己账号下(备份、搬家),而不是"转成别的编辑器"。
>
> **落点**:`DecompileOptions::upload_to_account(bool)`,默认 **false**;开 = 替用户在平台落一份草稿。

## 1. 设计

| 层 | 内容 |
| --- | --- |
| 选项 | `DecompileOptions::upload_to_account(bool)`(默认 false,需调用方明确授权:建作品等同发布动作) |
| 结果 | 新增 `DecompileOutcome { artifact, work_id: Option<i64>, editor }` + `decompile_outcome` / `decompile_batch_outcomes`;`decompile_with_options` / `decompile_batch` 行为不变(只返回路径) |
| 共用实现 | 新增 `convert/shared/upload.rs`:`DraftUpload` + `create_draft(client, spec)` —— **反编译侧与转化侧走同一份**上传+建作品代码(`convert/mod.rs::create_draft_work` 改为委托) |
| 客户端 | 走注入的客户端(`new_with_client`)—— 顺带修掉转化侧原先直接 `NekoWorkManager::new()`(全局)的隐患 |

草稿命名沿用既有口径:**`反编译副本 KittenN <- <源作品 id>(可删)`**(直接展示在用户草稿列表,标「可删」)。

## 2. 支持矩阵(只有"有已知建作品端点"的编辑器才能上传)

| 编辑器 | 建作品端点 | 支持 | 依据 |
| --- | --- | --- | --- |
| KittenN(`.bcmkn`) | `POST /neko/works` | 已完成 | 本轮真机验证(建草稿 + 回读 + 自删) |
| Kitten4(`.bcm4`) | `POST /kitten/r2/work` | 已完成 | 端点与参数照既有实现;未单独真机验证 |
| NEMO(`.bcm`) | `POST /nemo/v3/works/upload/<orientation>` | 注意: 实现到位、**未真机验证** | 见 §4 的两条限制 |
| Kitten2 / Kitten3 / Coco / Wood | 无 | 判不做 明确报错(不静默跳过) | 编译版 Kitten 无 create;Wood 是"建工程 + 写文件"两步且产物是资源形态 |

## 3. 上传渠道

`UploadChannel` 新增 **`Nemo`** 变体:七牛凭证项目名 `nemo_android_ios`(官方 App 抓包口径,`docs/rounds/24` §12);
其余编辑器沿用 `Codemao`(`community_frontend`)。实现上把 `upload_codemao` / `upload_nemo` 合并为
`upload_qiniu_file(file, save_path, project_name)`,`get_codemao_token` 改为 `get_qiniu_token(file, project_name)`(去重)。

## 4. NEMO 的两条已知限制(所以标"未验证")

1. **草稿不可删**:NEMO 侧的删除端点至今未知(KN 侧有 `delete_kn_draft`)=> 一旦真机建了草稿就**擦不掉**,
   所以本库**不跑**NEMO 的真机上传用例(配方留在 `docs/rounds/24` §13.4)。
2. **不重传资源**:产物是反编译重建的 `.bcm`,其造型/音频仍指向**源 CDN URL**;新作品若要求资源归属自己,
   需要"逐资源上传 + 文档内 URL 改写",本轮**不做**(官方 App 保存时会上传 ~1390 个文件,是另一件事)。

`bcm_version` 取自**源作品详情**新增的 `WorkInfo::bcm_version`(空则回落到本库常量);此前 `WorkInfo` 丢掉了这个元信息。

## 5. 验证

- 单测(3 条,`shared::upload`):支持矩阵只含 KN/Kitten4/NEMO;Coco/Wood/Kitten2/Kitten3 必须显式不支持;
  草稿名含行为/编辑器/来源/「可删」;NEMO 必须走 `nemo_android_ios` 渠道。
- 全量:`cargo test --lib` 97 passed / 1 ignored;`cargo check --all-targets` 通过。
- 真机(`#[ignore]`,自清理):`tests/convert_live::decompile_uploads_backup_draft_and_cleans_up_when_ignored`
  —— 反编译真 KN 作品 -> 上传 -> `create_kn_work` -> 回读 `work_id`/`work_url` -> **删除自建草稿**。
- **真机结果(2026-09-25)**:真作品 `325806995`(`now但是1080P`)-> 产物 `now但是1080P_325806995.bcmkn`
  -> 建草稿 **330852319** -> 回读 `work_url` 非空 -> 草稿删除成功。
  => **KN 侧"反编译可选上传"端到端通过**。
- 回收站清理口径:KN 的 `delete_kn_draft` 是**软删除**(进回收站,草稿列表即时消失);
  彻底清空要调 `NekoWorkManager::empty_kn_trash()` —— 它**清空整个回收站**,测试里**不自动调用**,
  只在人工确认回收站里只有自检产物时手动执行。

## 6. 与其它轮次的关系

- 复用 `docs/rounds/24` §12 的 NEMO 建作品端点与封面 bind(本轮不动 bind);
- 与 `docs/rounds/22` §5 的"不下载就建作品"路线 B 正交:本轮是"**已经下载并反编译完**,顺手建一份";
- 不影响 `translate` 的既有行为(只把它的上传/建作品代码换成共用实现)。
