# 第二十七轮方案 — NEMO → KN 转化(并入 convert 域)

日期:2026-09-25 · 基线:`4c87dd2` · 相关:`docs/rounds/20`(Kitten↔KN 转化)、`docs/rounds/24 §12`(NEMO 建作品端点)、`docs/rounds/28`(反向保真缺口)

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
  (还要能过 NEMO App 的校验),而且它唯一的现实用途是"把产物塞回 NEMO"(见 `docs/rounds/24 §12.3`)。

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
`Value` 直接给 translate,避免"落盘→读回"——这也是 `docs/rounds/23` P0-2 的同一套做法,属**加法**。

---

## 3. 与现有不变量的关系

1. **产物对齐**:照 `docs/rounds/20 §9` 的口径,不对齐官方字节,只做**语义 diff**(忽略 id/location/uuid);
   官方校验器 `BcmHelpers.validateBcm`(bundle 模块 87123)作为"产物能不能被编辑器加载"的**硬门**。
2. **确定性**:沿用 `IdSource` + `TranslateOptions::deterministic_ids`;正向并行(方案 25 S3a)的
   "临时 id + 串行兑现"机制**不用改**,因为新前端只产出 `BlockJson` 树,不理解 id 策略。
3. **不变量**:`docs/rounds/28` 记录的往返保真缺口**与本轮正交**(那是 KN→Kitten4 反向的问题);
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
- 把产物上传回 NEMO 建作品(`docs/rounds/24 §12` 的端点已备好,但同样依赖 KN→NEMO);
- 不顺手改 `docs/rounds/28` 的反向保真缺口(那是独立问题,已在预算断言里守住)。

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
| 7 | **"NEMO→Kitten4 免费获得"是过度陈述** | §1 的说法与 §3.3 自认的"KN→Kitten4 是自建有损反向(`docs/rounds/28`)"冲突;损失是**叠加**的 | 降级为"附带可得但需单独保真评估",不与 NEMO→KN 的验收混谈 |

**评审补充的必查项(已并入执行前置)**:① 语义 diff 的字段级归一化 + 端到端守恒断言;
② **NEMO 块类型总量与可映射比例**(决定整个工作量,含 micro:bit/传感器/AI 等 KN 无对应的降级策略);
③ `validateBcm` 离线可行性;④ R2 夹具要覆盖函数定义/调用/参数/返回/云变量/列表,**单一简单作品不足**;
⑤ **更正**:NEMO 编辑版是 fetcher 直接取明文 JSON(`work_id.url`),**没有解密步骤** —— 原文措辞已修正。

### 8.1 修正后的规模判断

原方案把 NEMO→KN 描述成"换前端 + 换映射表"。评审后应表述为:

> **新增一条完整正向路径**:NEMO 前端(解析 + 形状归一化)+ 程序集结构合成(函数/参数/返回 →
> `procedures_2_*`)+ 映射表 + 与 Kitten 路径并列的装配(或外壳合成)+ 门面层两处改动。

⇒ 这不是"顺手加一个方向",而是一个**独立轮次**(与 `docs/rounds/20` 当年做 Kitten→KN 的体量相当)。
因此本轮的执行边界调整为:**R1–R5 研究先把"官方产物夹具 + 表 + 结构合成规则 + validateBcm 可行性"备齐**,
Rust 侧实现留到研究结论到手后再开工(见 §5 的 S1–S4,顺序不变)。

---

## 9. 前置研究 R1–R4 结论(2026-09-25,已达成;工件在 `temp/harness/`,不入库)

### 9.1 已跑通的东西

| 项 | 结果 |
| -- | ---- |
| harness(契约) | `temp/harness/harness.js`:`require(id)` 首次调用自动 boot(幂等);`require(87123)` → 39 个导出含 `validateBcm`;`require(41888)` → `nemoBcmToNekoBcmUtils`/`kittenBcmToNekoBcmUtils`。**本仓契约测试已真跑并通过** |
| 真作品 1 | `蛋仔派对2…_194684070`(NEMO 0.16.2;847 角色 / 38 场景 / 2101 造型):输入 15 482 块 → 官方 KN 13 637 块,**`validateBcm → true`**,623 个实体与官方解析器**交叉核对 0 处不一致** |
| 真作品 2 | 公开作品 103791894「无限战争 2021」(0.11.0;280 角色):4 862 → 4 421 块,**`validateBcm → true`**,241 实体 0 不一致 |
| 我们的产物 | 本仓 Rust 的 Kitten4→KN 产物经同一 `validateBcm` 判定 **VALID**(契约测试通过;这是本仓最强的门,以前因缺 harness 一直跳过) |

工件:`map-tables.json` / `tables-output.txt`(映射表 + 产品交叉核对)、`doc-transforms-output.txt`(40 项文档级变换检查)、`nemo_parser.js`(官方 NEMO XML 解析器探针)、`fetch_nemo.js`。

### 9.2 必须修正的方案假设(研究直接推翻的)

| 原方案说法 | 实际(研究证据) | 修正 |
| ---------- | --------------- | ---- |
| 映射表是"块类型 → 块类型" | **部分是取值驱动的**:`mobile__get`(按 `attribute` 0/1/2/3/5 分成 `coordinate_of_sprite`/`style_of_sprite`/`appearance_of_sprite`)、`self_stress_animation`(按 `appear` 取值)、`get_styles`(无 `currentActor` 时 `style_id → NUM`) | 表要**带谓词**,不能平铺 |
| 前端"产出 `BlockJson` 树"即可 | 官方产物**保留原始 `blocksXML` 原样**,同时**并行**生成 camelCase(`position`/`currentStyleId`/`nekoBlockJsonList`/`workspaceScrollXy`/`actorIds`/`centerPoint`)⇒ **双形态** | 前端必须同时保留 snake_case 原字段 |
| 未映射块进 `TranslateReport` | 官方**降级成占位积木**(`bcm_translator_text_execution_block`/`_return_value_block`,两份作品共 16 处) | 与官方一致产占位(同时记报告) |
| 未提版本迁移 | `bcm_version < 0.9.4`(**QC**:角色 rotation 取反、旧音频块 XML 重写、变量坐标按舞台中心平移)与 `< 0.15.0`(**YC**:旧音频块 XML 重写) | 列为显式任务(两份样本只覆盖 YC,QC 未测到) |
| 资源"重传再改写" | `WN` **只改写 url**:造型 → `https://static.codemao.cn/nemo/22/` 前缀 + `.webp` 追加 `?imageView2/0/format/png`;音频同理且 `ext=mid` 置空。**重传是编辑器外围流程**(`main.js` 调用点)不是 `WN` | 沿用"离线近似保留 url","重传"当可选步骤 |
| 未提槽位语义 | `<value>` 里 shadow/empty 与覆盖它的 `<block>` 同时存在时:`inputs[key]` = 块节点,`shadows[key]` = **重新序列化**的 shadow XML(`xmlns=xhtml`、**新 uuid**) | 前端按此实现 |
| 确定性 | 官方 blockJson id 是**随机 uuid**、缺失 `createTime` 用 `Date.now()` ⇒ 官方 diff **必须忽略 id/location/createTime**(`docs/rounds/20 §9` 早已如此) | 我们仍走 `deterministic_ids`,但差异门按语义比 |

### 9.3 官方管线的 12 步(移植顺序,`main-vendors` ~6000071 的 `gI`)

版本迁移(可选)→ 新建 KN 骨架 → `procedure_dict` → `proceduresDict` → 逐 actor/scene `parseBlocksXML`
→ 场景命名/排序 → 音频 url 规则 → 造型 url 规则 → broadcast 按场景重组 → 变量重定心与类型映射
→ `stageSize` 归一化 → `timerPosition`/`extension`/`projectName`。

**KN 侧由转换器合成的默认值**(研究已逐条验证):`self_appear.value=appear`、`self_disappear → self_appear+value=disappear`、
`self_gradually_*.show_hide`、`self_change_coordinate_*/.increase=increase`、`mouse_down.sprite=--screen`、
`stamp.align`、`self_listen/self_broadcast` 的 `inputs.message`(broadcast_input shadow)、列表块的 `inputs.list`(pure_list_get shadow)、
`procedures_2_*` 的 mutation 字符串与 `PROCEDURES_2_DEFRETURN_RETURN/VALUE` shadow 槽位。

### 9.4 对 §5 阶段的影响

- S1(前端)要加"XML→`BlockJson`(含 shadow/empty/覆盖语义)+ 双形态文档"两件事,工作量比原估大;
- S2(映射)要带谓词 + 合成默认值 + 占位降级;
- **新增 S1.5**:版本迁移(QC/YC)—— 老作品不做迁移会直接产出与官方不同的产物;
- S3(资源)按 §9.2 的 url 规则即可,不再需要上传;
- 验收换成本文 §9.1 的两份真作品 + `validateBcm` 硬门 + 与 `temp/harness/out-*.json` 的语义 diff(忽略 id/location/createTime)。

---

## 10. 里程碑:仓库的"官方校验器"硬门**首次真正生效**(2026-09-25)

`src/core/convert/translate/mod.rs::generated_bcmkn_passes_official_validator_when_harness_present`
自加入仓库起,只要 `temp/harness/harness.js` 不存在就**静默跳过**(打一行提示后 `return`,测试仍然算 pass)
—— 也就是说这条"最强门"在过去的会话里**从未真正验过任何东西**。

今天 harness 按契约补齐(`require(id)` 首次调用自动 boot ✓),这条测试**真跑了**:

| 检查 | 命令 | 结果 |
| ---- | ---- | ---- |
| 我们 Rust 的 Kitten4→KN 产物过官方校验 | `cargo test --lib generated_bcmkn_passes_official_validator -- --nocapture` | **通过**(断言 `stdout` 含 `VALID`) |
| 两份官方 NEMO→KN 产物过校验(独立复验) | `node temp/harness/harness.js --validate temp/harness/out-eggparty-194684070.json` / `…out-wuxian-103791894.json` | 两次 `validateBcm -> true` |
| 官方解析器交叉核对(独立复验) | `node temp/harness/map_tables.js` | `XML slots dropped: {}`、`lost next chains: {}`,仅 `self_broadcast:73`/`self_listen:94` 的 message 按官方规则搬进 KN 输入槽 |

⇒ 结论:**"产物能被官方编辑器加载"从"声称"变成了"可执行的门"**;NEMO→KN 的验收也因此有了硬依据。

### 10.1 harness 的保真注意事项(用之前先看)

1. **DOM 是手写的**(`temp/harness/dom.js`),不是浏览器:XML 严格性有差异(容忍裸 `&`;结构错误产出
   `<html><body><parsererror>` 形态);选择器只实现转换器用到的那一小撮(`:scope`、子/后代、标签、`[attr]`、
   `[attr="v"]`、逗号列表),超出即显式抛错。
2. **曾经因为缺一个 DOM 成员静默降级**:没有 `Element.nextElementSibling` 时,官方解析器走了"只看 shadow"的
   退化分支,**悄悄丢掉覆盖 shadow 的块**(产物 11.8/4.05 MB,而且**照样过 validateBcm**);补上后是 14.1/4.59 MB。
   ⇒ 教训:**只有在 `map_tables.js` 报 0 交叉核对不一致时,harness 产物才可信**;bundle 升级后必须重跑它。
3. harness 是**逆向工具、不入库**(`temp/` 已 gitignore);仓库测试在缺少它时跳过,CI 不受影响。

---

## 11. 落地记录:NEMO → KN 已实现(`5755791`,主代理已独立复核)

### 11.1 改动(9 个文件,+5 938 行)

| 文件 | 作用 |
| ---- | ---- |
| `translate/nemo_xml.rs`(新,1 093) | Scratch 风格 XML 前端:块/字段/值槽/语句/next、`shadow`/`empty`/覆盖语义 |
| `translate/nemo.rs`(新,1 418) | NEMO 编辑版文档 → KN 文档:双形态(原 snake_case + `blocksXML` 原样保留 + camelCase)、装配、资源 url 规则 |
| `translate/nemo_mapping.rs`(新,2 116) | 语义映射(表驱动 + 取值驱动特例)+ KN 侧合成默认值 + 占位降级 + QC/YC 版本迁移 |
| `translate/tables_gen_nemo.rs`(新,333) | 从 harness 交叉核对过的表生成 |
| `translate/nemo_tests.rs`(新,825) | 12 条单测(槽位覆盖、双形态、版本迁移、占位、内存入口…)+ 两条真样例门(默认 `#[ignore]`) |
| `translate/mod.rs` | `translate_value` 加 `(Nemo, KittenN)` 分支;`detect_editor` 识别 NEMO;`source_version` 透传 |
| `decompile/{mod.rs,editors/nemo.rs}` | 反编译侧新增**内存编辑版**入口(NEMO 不再只能落盘) |
| `convert/mod.rs` | 域门面接受 NEMO 文档产物;Nemo 纳入"需要上传源引用"的判定 |

### 11.2 验收(主代理逐条复跑,证据为真实输出)

| 门 | 结果 |
| -- | ---- |
| 我们 Rust 产物过**官方 `validateBcm`** | 蛋仔派对(0.16.2)→ `VALID`;无限战争 2021(0.11.0)→ `VALID` |
| 与**官方产物**语义 diff(忽略 id/location/createTime/parent_id、空=缺省、影子 id 归一) | 蛋仔派对:**0 处**;无限战争:**4 处,全部在 allow-list 内**(唯一条目 = `.audios.sortList`/`.broadcasts.<场景>` 的**源字典顺序**:官方按 JS 插入序,我们是 `BTreeMap` 按键序;两边**集合严格相等**) |
| **块数**对照 | 蛋仔派对 15 482 源元素 → 我们 **13 637** = 官方 **13 637**;无限战争 4 862 → 我们 **4 421** = 官方 **4 421** |
| **Kitten 路径零回归** | 四样本产物 SHA256 与基线**逐字节一致**、并发 1 vs 8 同哈希、正向并行仍 **1.70×**;基线文件未改 |
| 测试 | `cargo test`:94 项单测 + 集成目标(含真机 live_features 3/3)全绿;新文件 `clippy` 0 告警、无 `unsafe`/`unwrap`、无新依赖 |

### 11.3 已知偏差与留白(故意、且有据)

1. **源字典顺序**(唯一 allow-list):`BTreeMap` 与 JS 插入序的差异;集合相等,块数另有 `assert_eq` 守。同源还有"重名变量去重后缀",两份样本无重名(已核)。
2. **官方 bug 有意不复刻**:`procedures_2_return_value`(ROUND)把一个漏调用的函数对象拼进影子 id;我们铸真 id(模块文档已记)。
3. `<block type="mobile__text">` 官方走字符串替换,自闭合写法会插成兄弟节点;我们实现为插成首个子节点(真实作品不自闭合)。
4. QC 的旧音频块重写:官方是字符串正则,我们按 DOM 等价写法(真实作品上等价)。
5. 多字段影子的字段顺序差异(同样源于键序)**未触发**:两份样本影子均为单字段。
6. `translate_value`/`translate_file` 的 NEMO 分支要求调用方给 `source_version`(不给=不迁移,与官方 `WN(data, bcmVersion)` 同义);**域门面已自动从作品元信息带上**。
7. **域门面端到端(需要网络取作品)本轮未跑**:与仓库既有真机测试同约定(默认 `#[ignore]`);内存入口本身有单测直接覆盖。

### 11.4 现在的方向矩阵

| 源 → 目标 | 状态 |
| --------- | ---- |
| Kitten4 → KN | ✅ 已有(过官方校验器 + 实体级并行) |
| KN → Kitten4 | ✅ 已有(自建有损反向,保真缺口见 `docs/rounds/28`,已加预算断言) |
| **NEMO → KN** | ✅ **本轮落地**(与官方逐数一致) |
| NEMO → Kitten4 | ✅ 附带可得(= NEMO→KN ∘ KN→Kitten4;损失叠加 ⇒ 需单独保真评估) |
| KN → NEMO | ⛔ 不做(平台无对照;`docs/rounds/24 §12` 的建作品端点已备好,但要先能产出合法 NEMO 文件) |
