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

- **做**:NEMO → KN。做完之后 **NEMO → Kitten4 免费获得**(走我们已有的 KN→Kitten4 反向),
  这样"nemo / kitten / kn 三者互转"里**两个方向**(NEMO→KN→{KN,Kitten4}、Kitten→KN)都齐了。
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

**源文档从哪来**:反编译侧 `editors/nemo.rs` 现在产 `DecompileResult::Path`(资源目录 + 解密后的
编辑版)。需要一条**内存入口**(与 S1 已落地的 `DecompiledArtifact` 同构):把 NEMO 的编辑版
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
