# 第二十七轮方案 — NEMO → KN 转化(并入 convert 域)

日期:2026-09-25 · 基线:`4c87dd2` · 相关:`docs/20`(Kitten↔KN 转化)、`docs/24 §12`(NEMO 建作品端点)、`docs/28`(反向保真缺口)

> **状态:只出方案 + 第一步前置研究已派出。** 按约定"先写文档 → 子代理评审 → 执行",
> 本文是那份文档;执行前的硬前置是 §4。

---

## 1. 范围:先弄清平台到底支持哪些方向

编辑器 bundle(`creation.bcmcdn.com/neko/web/release/static/js/*.js`)里**只有两个转化入口**:

| 官方符号 | 位置 | 方向 |
| -------- | ---- | ---- |
| `kittenBcmToNekoBcmUtils`(导出 `GN`) | `main-vendors.9b801394.js`(模块 41888) | Kitten → KN(**已实现**) |
| **`nemoBcmToNekoBcmUtils`(导出 `WN`)** | 同上 | **NEMO → KN(本轮目标)** |

**没有** `nekoBcmToNemoBcm` / `nekoBcmToKittenBcm` 之类(全 bundle 搜过)⇒ 平台的语义是
**"万物入 KN"**,KN 是枢纽。因此:

- **做**:NEMO → KN。做完之后 NEMO → Kitten4 **附带可得**(走已有 KN→Kitten4 反向),
  但那是**自建有损反向**,损失会叠加 ⇒ 需单独做保真评估,不与 NEMO→KN 的验收混谈(评审 §8 #7)。
- **不做(留置)**:KN → NEMO。平台没有对照实现,自建一套 NEMO 编码器成本高、风险大
  (还要能过 NEMO App 的校验),而且它唯一的现实用途是"把产物塞回 NEMO"(见 `docs/24 §12.3`)。

官方调用点(`main.14802dc2.js`)的完整流程,就是我们要复刻的骨架:

```text
下载 NEMO 作品文件(.bcm) → WN(data, bcmVersion) → KN 文档
  → 重传 styles 里的资源(url 换成本站) → 写 source = 原文引用
  → validateBcm 校验 → 上传成 .bcmkn → resetWork(建 KN 作品)
```

---

## 2. 架构落点(复用现有 translate 四层,不造第二套)

现有正向管线是「前端(pure 图↔树)→ 语义映射(`mapping`)→ 后端(编码)→ 装配(`assembly`)」。
NEMO 只需要换**前端**与**映射表**,后端与装配**完全复用**:

| 层 | Kitten4 → KN(现有) | NEMO → KN(新增) |
| -- | ------------------ | ---------------- |
| 前端 | `kitten::parse_block_data_json` | **`nemo::parse_blocks`(新)** |
| 语义 | `mapping::translate_kitten_to_kn` | **`mapping::translate_nemo_to_kn`(新,表驱动)** |
| 后端 | `neko::split_procedures` / `rewrite_calls` / `tree_to_json` | **同一套** |
| 装配 | `assembly::build_document` | **同一套** |

接入点:`translate_value` / `translate_file` 的 match 里加一条
`(Some(EditorType::Nemo), TargetEditor::KittenN)` 分支(`EditorType::Nemo` 枚举已存在)。
`TranslateReport` 里已有的 `UnmappedBlock` / `DegradedToText` / `DroppedField` 直接承载
"NEMO 有、KN 没有"的语义,不新增报告形状。

**源文档从哪来**:反编译侧 `editors/nemo.rs` 现在产 `DecompileResult::Path`(资源目录 + 编辑版;
注意:NEMO 编辑版是 fetcher 直接取明文 JSON,**没有解密步骤** —— 评审更正)。需要一条**内存入口**(与 S1 已落地的 `DecompiledArtifact` 同构):把 NEMO 的编辑版
`Value` 直接给 translate,避免"落盘→读回"——这也是 `docs/23` P0-2 的同一套做法,属**加法**。

---

## 3. 与现有不变量的关系

1. **产物对齐**:照 `docs/20 §618` 的口径,不对齐官方字节,只做**语义 diff**(忽略 id/location/uuid);
   官方校验器 `BcmHelpers.validateBcm`(bundle 模块 87123)作为"产物能不能被编辑器加载"的**硬门**。
2. **确定性**:沿用 `IdSource` + `TranslateOptions::deterministic_ids`;正向并行(方案 25 S3a)的
   "临时 id + 串行兑现"机制**不用改**,因为新前端只产出 `BlockJson` 树,不理解 id 策略。
3. **不变量**:`docs/28` 记录的往返保真缺口**与本轮正交**(那是 KN→Kitten4 反向的问题);
   但 NEMO→KN 的产物如果也走一次 KN→Kitten4,得先确认不引入**新的**缺口 —— 验收要覆盖。

---

## 4. 前置研究(第一步,已派出;不完成不动 Rust 代码)

| 编号 | 任务 | 产出 |
| ---- | ---- | ---- |
| R1 | 把官方 `WN` 跑起来:从 `main-vendors.9b801394.js` 取 webpack 模块 41888,包成 Node 可调(`nemoBcmToNekoBcmUtils(data, bcmVersion)`),并顺带暴露 `kittenBcmToNekoBcmUtils` 与我们已实现的 Kitten 正向做**交叉验证** | `temp/harness/*.js`(不入库,与既有 `temp/harness/harness.js` 约定一致) |
| R2 | 拿 ≥2 个真 NEMO 作品跑 `WN`,把官方 KN 产物存成差分夹具 | `temp/harness/out-*.json` + 形状说明 |
| R3 | 抽出 `WN` 用到的映射表:NEMO 块类型 → KN 类型、字段/槽位、特例(与 `tables_gen.rs` 同口径整理) | 表清单(建议后续生成 `tables_gen_nemo.rs`) |
| R4 | 说清 NEMO 编辑版的**文档形状**(`project.json`/资源引用方式/坐标系),以及 `WN` 对资源的处理 | 短报告 |

真作品来源(本地已有或可现取):`download/compile/春风得意_324995084.bcm`、
`download/compile/蛋仔派对…_194684070/`(反编译产物目录),或用 NEMO 反编译接口按作品 id 现取
(`data/test-config.json` 里有 NEMO 作品 id)。

## 5. 执行阶段(研究过关后)

| 阶段 | 内容 | 验收 |
| ---- | ---- | ---- |
| S1 | NEMO 编辑版内存入口(反编译侧加法)+ `nemo::parse_blocks` | 单测:真 NEMO 作品 → `BlockTree` 可解析、块数与非空实体守恒 |
| S2 | `translate_nemo_to_kn` 表驱动映射(先覆盖 `WN` 里出现的全部类型;未覆盖的进报告) | 与 R2 的官方产物做**语义 diff**;`validateBcm` 通过 |
| S3 | 资源引用策略(造型/音频 url:沿用官方"重传"做法) | 官方 diff 里 url 字段可解释 |
| S4 | 接入 `translate_value`/`translate_file`/`translate_work`,补真作品测试与文档 | 全量测试绿 + 至少两份 NEMO 真作品端到端 |

## 6. 风险

| 风险 | 缓解 |
| ---- | ---- |
| NEMO 的**坐标/画布**与 KN 不同(它是 JS 风格、屏幕像素) | R2 的官方产物直接给出换算结果,按它对齐;不一致处进报告 |
| NEMO 资源是**打包内**引用(`.userimg`/bundle),KN 是 CDN url | 沿用官方"重传资源再改写 url"的做法(我们已有上传先例) |
| `WN` 是压缩代码,表可能内联在函数里 | R1 的 harness 能**跑**,表可以"跑出来的产物"反推(不必逐字读懂压缩码) |
| 引入第二套前端后 API 面变大 | 只加 `pub(crate)` 前端/映射;公开面只多一个 `TargetEditor` 语义不变的分支 |

## 7. 明确不做

- KN → NEMO(平台无对照,且要先自建 NEMO 编码器);
- 把产物上传回 NEMO 建作品(`docs/24 §12` 的端点已备好,但同样依赖 KN→NEMO);
- 不顺手改 `docs/28` 的反向保真缺口(那是独立问题,已在预算断言里守住)。

---

## 8. 子代理评审结论(2026-09-25,ReviewNemoPlan):**有条件可行**,且工作量比我原估的大

**原方案的三处乐观估计被代码事实推翻**,逐条记证据与修法:

| # | 阻塞问题 | 证据(`文件:行号`) | 修法 |
| - | -------- | ----------------- | ---- |
| 1 | **"装配层完全复用"不成立**:`assembly::build_document` 强依赖 Kitten4 文档形状(NEMO `.bcm` 没有那些顶层键) | `assembly.rs:119` 读 `theatre`、`:497-498` 读 `size`、`:531` 读 `project_name`、`:536` 读 `broadcasts`、`:361-368` 读 `audio`、`:546-551` 读 `variables`/`cloud_variables`;`actor_entry` 还施加 Kitten 专属变换(横屏 10/13 缩放、`lock→locked`、坐标中心原点)。而 NEMO 编辑版按 `actors.actors_dict` 判定(`translate/mod.rs:772-778`)、资源在 `styles.styles_dict`(`decompile/editors/nemo.rs`) | 方案 §2 改为两条可选路线:**(a)** 新增 NEMO 专用装配 `build_document_nemo`;**(b)** 前端额外合成一份 Kitten4 形状的 `theatre/size/project_name/variables/broadcasts/audio` 外壳。二者都算**独立工作量**,不再写"同一套装配" |
| 2 | **并行脚手架被硬编码**,不能"只在 match 加分支" | `translate/mod.rs:369-373` `parse_forward_item` 写死 `kitten::parse_block_data_json` + `translate_kitten_to_kn` + `EditorType::Kitten4`;S3a 的四阶段并行直接调它 | 明确新增 `convert_nemo_document`,**或**把四阶段脚手架参数化(前端/映射作为参数) |
| 3 | **KN 后端程序集机制硬编码 Kitten 字段布局**,不是"改名"能复用 | `neko.rs:74-84` 的 `procedures_2_defnoreturn/_stable_parameter/_parameter/_return_value/_callnoreturn/_callreturn`;`split_procedures(:134-148)` 按 `DEF_ROOT` 摘根、`collect_params` 读 `fields.param_name` + `PARAMS<n>` 槽;`rewrite_calls` 重建 `<mutation def_id name>` 与 `ARG<i-1>` | R3 增加产出:**NEMO 函数定义/调用/参数/返回值 → Kitten `procedures_2_*` 结构与字段布局的合成规则**;§6 风险表补"程序集结构合成"风险 |
| 4 | **`validateBcm` 硬门没有落地途径** | §3.1 把它当硬门,但 §4 的 R1 只覆盖 `WN`/`GN` | 新增 **R5**:把 `validateBcm`(bundle 模块 87123)也抽成 Node 可调 harness,**先证明离线能跑**;跑不起来就把它降级为"可选门"并在 §3 说明 |
| 5 | **语义 diff 的归一化规则未定义**,且"重传资源"与"复用装配"自相矛盾 | §3.1 只说"忽略 id/location",没定义 `createTime`/`Date.now()`/`sortList` 顺序/资源 url/递归深度;而 `assembly::build_styles(assembly.rs:320)` 是**刻意不上传**、保留源 url 的离线近似 | §3.1 补**字段级归一化清单**;资源策略二选一并写死(建议沿用"离线近似保留源 url",与既有 Kitten 路径一致) |
| 6 | **门面层还有两处必须改** | `convert/mod.rs:95-99` 对 `DecompiledArtifact::Path` 直接报错("NEMO / WOOD 请用反编译接口另行处理");`:107-110` `needs_source_upload` 只覆盖 Kitten2/3/4,而官方 NEMO→KN 也写 `source` | 方案列出这两处改动(接受 NEMO 的内存文档产物;把 Nemo 纳入 `needs_source_upload`) |
| 7 | **"NEMO→Kitten4 免费获得"是过度陈述** | §1 的说法与 §3.3 自认的"KN→Kitten4 是自建有损反向(`docs/28`)"冲突;损失是**叠加**的 | 降级为"附带可得但需单独保真评估",不与 NEMO→KN 的验收混谈 |

**评审补充的必查项(已并入执行前置)**:① 语义 diff 的字段级归一化 + 端到端守恒断言;
② **NEMO 块类型总量与可映射比例**(决定整个工作量,含 micro:bit/传感器/AI 等 KN 无对应的降级策略);
③ `validateBcm` 离线可行性;④ R2 夹具要覆盖函数定义/调用/参数/返回/云变量/列表,**单一简单作品不足**;
⑤ **更正**:NEMO 编辑版是 fetcher 直接取明文 JSON(`work_id.url`),**没有解密步骤** —— 原文措辞已修正。

### 8.1 修正后的规模判断

原方案把 NEMO→KN 描述成"换前端 + 换映射表"。评审后应表述为:

> **新增一条完整正向路径**:NEMO 前端(解析 + 形状归一化)+ 程序集结构合成(函数/参数/返回 →
> `procedures_2_*`)+ 映射表 + 与 Kitten 路径并列的装配(或外壳合成)+ 门面层两处改动。

⇒ 这不是"顺手加一个方向",而是一个**独立轮次**(与 `docs/20` 当年做 Kitten→KN 的体量相当)。
因此本轮的执行边界调整为:**R1–R5 研究先把"官方产物夹具 + 表 + 结构合成规则 + validateBcm 可行性"备齐**,
Rust 侧实现留到研究结论到手后再开工(见 §5 的 S1–S4,顺序不变)。
