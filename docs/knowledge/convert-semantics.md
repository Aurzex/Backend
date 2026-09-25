# 转换语义与不变量

> 知识库条目:**编辑器间转换到底做了什么、哪些是硬约束、哪些坑官方自己也有**。
> 方案与实施过程见 `docs/rounds/20-*`(Kitten↔KN)、`docs/rounds/27-*`(NEMO→KN)、`docs/rounds/28-*`(反向保真)。

## 1. 平台/官方支持矩阵(实测)

| 方向 | 官方 | 本库 |
| ---- | ---- | ---- |
| Kitten4 `.bcm4` → KN `.bcmkn` | ✅ `kittenBcmToNekoBcmUtils` | ✅ 过官方校验器 + 实体级并行 |
| Kitten 2/3 `.bcm` → KN | ⛔ 编辑器明确拒绝(引导去 Kitten V4.0) | ⛔ 同 |
| NEMO → KN | ✅ `nemoBcmToNekoBcmUtils` | ✅ 与官方逐数一致 |
| NEMO → Kitten4 | ⛔ 官方无 | ✅ 附带可得(= NEMO→KN ∘ KN→Kitten4,**损失叠加**) |
| KN → Kitten4 | ⛔ 官方无此方向 | ✅ 本库**自建**(有损,见 §5) |
| KN → NEMO | ⛔ 平台无对照 | ⛔ 不做(唯一用途是把产物塞回 NEMO,见 `nemo-runtime-and-upload.md`) |

编辑器 UI 行为:导入 `.bcmkn*` 直接打开;`.bcm4/.bcmp4` **自动转换**(提示「已自动转为KittenN作品」);`.bcm/.bcmp` 拒绝。

## 2. 官方管线是两阶段(正向 Kitten4 → KN)

1. **阶段 1**(`GN` / `kittenBcmToNekoBcmUtils`):积木层转换。**产物还不是合法 bcmkn** —— 官方校验器会报 `actorsDict 不存在`。
2. **阶段 2**(module 68602):顶层**重排** —— `theatre.*` → `actorsDict`/`scenesDict`/`stylesDict`/`variablesDict`/`stageSize`/`projectName`。

⇒ 只做阶段 1 会得到"编辑器打不开"的文件。本库把两阶段合进一次 translate。

NEMO → KN 是另一条前端(`hI.parseBlocksXML`),官方管线共 12 步(`main-vendors` 的 `gI`),含**版本迁移**(`< 0.15.0` 补 `controls_if` 的 `else="1"`;`< 0.9.4` 角色 rotation 取反 + 旧音频块包影子)。

## 3. 积木层映射语义

- **类型改名**是主要工作:实测一次真实转换 374 个输入积木里 **151 个被改名**(`start_on_click→on_running_group_activated`、`self_disappear→self_appear`、`set_costume→set_sprite_style`、`get_audios→get_play_audio`、`math_single→math_function`、`default_value→math_number`;`get_3` 按 `fields.attribute` **分流**成 `coordinate_of_sprite`/`appearance_of_sprite`/`effect_of_sprite`)。
- **字段改名**同步:`math_arithmetic.fields.OP:"MULTIPLY"` → `fields.type:"multiply"`。
- **影子**:XML 串原样搬运,同时物化成嵌套 `inputs`(177 个积木两者都有)。
- **常量/枚举**:`fl_`/`gd_` 前缀表、`chineseNameDict` **208** 条(中文名 ↔ KN 类型)。
- **槽位覆盖语义**:`<value name="A"><shadow/><block/></value>` 时 `inputs["A"]` = 那个块、`shadows["A"]` = **重新序列化**的 shadow XML(`xmlns=xhtml`、**新 uuid**);`<empty>` 与 `<shadow>` 走不同分支。
- 已知不对称(照抄官方):云列表分支只写 `inputs.list`,并 `delete fields.list`。

## 4. 官方实现自带的坑(照抄前先知道)

1. **"删字段"其实没删**:`spread({}, omit(a,[block_data_json,…]), cloneDeep(a))` —— `cloneDeep(a)` 又把它加回来 ⇒ 产物里 `block_data_json` 仍在(实测)。
2. **原地改写入参**:`GN` 会回写源 `block_data_json`(实测 12 处,如 `get_3` 补 `fields.*`)并返回同一对象引用。
3. **id 非确定**:每次运行现铸 UUID(同输入两次运行 28 个 id 不同);`KC` 复制输入时保留 shadow id ⇒ 输出出现 **49 个重复 id**。
4. **Kitten3 会静默丢数据**:强行补 `size`/`block_data_json` 后返回"0 根 0 积木 0 warning"的空结果,不报错。
5. **`UC` 横屏标志是模块级全局**(`mod41888:82820`):并发转换必须改成显式参数,否则跨作品串味。

## 5. 反向(KN → Kitten4)是自建且有损

- 逐条与正面对称反查(改名逆表、字段反查、槽位还原),但不变量只能靠**往返 + 预算断言**守。
- **已知保真缺口**:inline `pure_list_get` 影子在往返中丢失,实体侧与定义体侧都出现(受影响定义 6 / 净减块 133,已写成断言,扩大会失败)。根因与后续研究见 `docs/rounds/28-*`。
- 运行时实测:KN→Kitten4 产物有 `theatre`/`size`/`block_data_json`,可被平台接受。

## 6. 硬门与不变量(实现任何新方向都必须满足)

| 门 | 内容 |
| -- | ---- |
| 官方校验器 | `BcmHelpers.validateBcm`(bundle module 87123)**可 headless 运行** —— "产物能否被编辑器加载"的硬门 |
| 语义 diff | 与官方产物比较**忽略 id/location/uuid**(`docs/rounds/20` §9);官方产物按键插入序,**从不逐字节对齐官方** |
| 确定性 | `IdSource` + `TranslateOptions::deterministic_ids`;并发 1 与并发 N 产物 **SHA256 相同** |
| 往返守恒 | KN→Kitten4→KN 的积木类型**多重集**一致(差异仅白名单降级项 + 预算断言) |
| 有损记账 | 一切有损进 `TranslateReport`;官方重传资源不算有损(`ReuploadedOnImport`) |

## 7. 判定"不做"的两项(有证据,别再重开)

| 项 | 判定 | 证据 |
| -- | ---- | ---- |
| `RawValue` 顶层只透传 | **不做** | 装配期"只透传不改"的顶层字段占比 **≈0%**:`theatre`(含 `block_data_json`)占文档 99.2–99.5% 且全部重写;`styles`/`audios`/`variables`/`broadcasts` 都要变换 ⇒ 收益上限 ≈0%,远低于 30% 门槛 |
| 正/反向遍历合并成单遍 | **不做** | 正向实为 2 趟(`parse_node` 内含 `route_children` 再 `gc_deep`),反向本已单遍;合并只省遍历,不省逐块 `kind` 匹配/`transform_shadow_xml`/`map_field_name` 这些主要成本,且 `gc_node` 依赖子树已 parse |

## 8. 文本层(低成本高价值的调试通道)

- `knBcmToText` / `textToBlock`:`.bcmkn` ⇄ 中文积木 Markdown(`# 场景 / ## 角色 / ### 属性 / ### 代码`)。
- 实测 3.7 MB 作品 → 4.86M 字符,1.4 s;**全树积木 359 → 359,55 种类型多重集完全一致**(差异仅元数据/XML 归一化/新 UUID)。
- 大文件要有大小上限;文本层不是产物的必需环节,是**人工校验**手段。

## 依据

- `docs/rounds/20-kitten-kn-work-conversion-plan.md` §3(官方实现逆向)、§4(反向可行性)、§6.2(设计取舍)、§8(坑)、§9(验证)、§11(实测)、§11.2(真机端到端)。
- `docs/rounds/27-nemo-to-kn-conversion-plan.md` §9(前端/映射研究)、§11/§12(落地与方向表)。
- `docs/rounds/28-convert-reverse-fidelity-gaps.md`(缺口)、`docs/rounds/26-convert-rawvalue-single-pass-plan.md` §6(两项判不做)。
- 代码锚点:`src/core/convert/translate/{mapping,nemo_mapping,assembly,neko}.rs`、`tests/convert_*`。
