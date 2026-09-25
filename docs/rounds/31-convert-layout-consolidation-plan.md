# 第三十一轮方案 — convert 域文件编排合并与架构收敛

日期:2026-09-26 · 基线:`4415234` · 上游:`docs/rounds/21`(上一轮域化,定下 16 个生产文件与「不合并」清单)

> **需求(用户提出)**:合并 convert 的文件、别太碎,顺带优化整体架构;允许大规模重构。
>
> **现状**:`src/core/convert/` **27 个 .rs / 22 685 行**,占 `src/core/` 的 73%。上一轮定的是 16 个文件,
> 之后 NEMO 方向、反向、并行、上传等轮次各自新增文件,长到 27 —— 碎在**同一职责被拆成 3~4 个文件**
> (映射表 / 引擎 / 测试各一份),不是碎在职责本身。

## 1. 现状(实测)

| 子域 | 文件 | 行数 | 碎在哪 |
| ---- | ---- | ---- | ------ |
| `mod.rs` | 1 | 262 | — |
| `shared/` | 6 | 1 270 | `mod/error/model/infra/upload` 全是 20~260 行的碎片,且靠 `shared/mod.rs` 一长串 `pub(crate) use` 汇总再导出 |
| `decompile/` | 8 | 3 927 | `editors/{mod,kitten,nemo,simple}` 4 个文件装 7 种编辑器 |
| `translate/` | 13 | 18 663 | 同一方向被切成 `引擎 + 映射 + 表 + 测试` 四份(`kitten+mapping+tables_gen`、`nemo+nemo_mapping+nemo_xml+tables_gen_nemo+nemo_tests`、`assembly+remint+reverse_tests`) |

## 2. 目标结构(12 个文件,按**单一职责**而不是按"层"切)

```
src/core/convert/                    行数(估)   来源
  mod.rs                              ~300       同现状(门面 + 跨子域编排 + 上传编排)
  shared.rs                         ~1 294       shared/{mod,error,model,infra,upload} + shared/config(配置 + 影子模板表)
  decompile/mod.rs                  ~1 983       decompile/mod(选项 + 门面 + 主流程)+ decompile/blocks(编译版积木层)
  decompile/editors.rs              ~1 692       decompile/editors/{mod,kitten,nemo,simple} —— 7 种编辑器
  translate/mod.rs                  ~1 500       同现状(选项 + 报告 + 分派;去测试)
  translate/model.rs                ~2 224       translate/{model,ids} + 双向适配器{kitten,neko}
  translate/mapping.rs               2 018       同现状(语义映射双向 + 手写表)
  translate/assembly.rs             ~2 364       同现状 + remint(装配双向 + id 重铸)
  translate/nemo.rs                 ~2 511       translate/{nemo,nemo_xml}
  translate/nemo_mapping.rs         ~2 675       translate/nemo_mapping + tables_gen_nemo(NEMO 映射表 + **人工转录**的表)
  translate/tables_gen.rs            1 664       **生成物单独一处**(`src/bin/gen_translate_tables.rs` 整文件覆盖,不能与手写表混放)
  translate/reverse_tests.rs         1 400       反向测试(**保留独立文件**:见 §2.2)
  translate/nemo_tests.rs              826       NEMO 测试(同上)
```

**规模**:27 → **13**(−52%);最大单文件 ~2 675 行,与现状的 `nemo_mapping.rs`(2 116)/`mapping.rs`(2 018)同量级 —— **不新增巨石**。

### 2.2 为什么两个测试文件仍独立

把 `reverse_tests.rs` / `nemo_tests.rs` 并成一个文件,必须把各自内容包进 `mod X { … }` 才能避免同名夹具冲突;
而两个文件内部大量使用 `use super::…`(指向 `translate`)—— 多包一层后 `super` 的含义会变,断的不只是路径,还有测试对生产模块的可见性。
收益(少 1 个文件)远小于风险 ⇒ **保留独立**(正好符合 §2.1 规则 3 的阈值条款:它们本身就是「本体 + 测试 > 3 000 行」的产物)。
`decompile/` 的测试已随 `editors.rs` 内联,`translate/mod.rs` 的 `diff_tests`/`forward_parallel_tests` 保持内联。

### 2.3 组织规则(写进本方案,后续新增按它走)

1. **一个文件一个职责**:模型与适配器(`model`)、语义映射(`mapping`)、装配(`assembly`)、生成表(`tables`)、NEMO 侧(`nemo`/`nemo_mapping`)各归一处;正反两个方向**共享**的文件(映射/装配/模型)保持共享,不按方向硬切。
2. **生成物集中**:所有生成表进 `translate/tables.rs`(与手写代码物理隔离,但只有一处)。
3. **测试**:默认内联在**被测文件末尾**;当「本体 + 测试 > 3 000 行」时,测试独立成 `*_tests.rs`(本轮只有 `translate/tests.rs` 触发)。
4. **文件上限 ≈ 2 500 行**:超过就按职责再切,不靠"再拆一层目录"。
5. **合并会改模块路径 ⇒ 必须同步改写调用点**:`shared::X` 与 `decompile::editors::X` 这两个合并没有改名代价
   (调用方用的正是目标路径);但 `translate::{kitten,neko,remint,nemo_xml,tables_gen_nemo}` 与被并入
   `decompile/mod.rs` 的 `decompile::blocks`,合并后**原路径会断** ⇒ 必须改引用,清单见 §3.5。
   > 评审 `PlanReview31` 纠正了本方案早期版本的一处方向性错误:把"调用方都用完整路径"当成"路径不会失效"的理由 —— 恰好相反,正因为用完整路径,删模块名才会断。

## 3. 架构收敛(与本方案同批做,均为**内部**改动)

| # | 问题 | 改法 | 为什么不破坏公开面 |
| - | ---- | ---- | ------------------ |
| A1 | `shared/mod.rs` 的长 `pub(crate) use` 汇总层(=每个符号两处名字) | 删掉汇总层,调用方直接 `crate::core::convert::shared::X` | `shared` 本就不对外暴露 |
| A2 | `WorkProcessorRegistry`(`HashMap<EditorType, Box<dyn Fn(…)->Box<dyn …>>>` + 两类型别名)为 7 种固定编辑器引入动态分派 | 换成 `match EditorType` 直接构造 fetcher/decompiler;删两个工厂类型别名与注册表结构 | `pub(crate)`,无人从外部注册 |
| A3 | 同一助手在正/反向各抄一份(坐标换算、主题表、`procedures` 名字↔id) | 收敛到 `translate/model.rs`(仅助手,不动两方向的控制流 —— 尊重 `docs/rounds/21` §6「不为对称强行合函数」) | 内部 |
| A4 | `decompile/` 的 `EditorType` 与 `translate/` 的 `TargetEditor` 概念重叠 | 保留两者(公开面),但在 `model.rs` 给 `TargetEditor ⇄ EditorType` 一对内部转换,消除散落的 `match` | 公开类型不动 |
| A5 | 测试组织两套并存(内联 + 独立) | 统一按 §2.1 规则 3 | 内部 |
| A6 | `translate/mod.rs`(175-198)与 `decompile/mod.rs`(324-341)**各有一套**同构的「chunk + `thread::scope` + 保序收集」批处理 | 收敛成一个内部批处理执行器(工作项 + 并发上限 → 保序结果),两处改为调用它 | 内部函数;并发语义与结果顺序必须逐字保持 |

**明确不做**(沿用 `docs/rounds/21` §6,并说明为何仍适用):

- **不改公开路径与类型名**:`convert::{EditorType, WorkId, DecompilerError}`、`translate::{TargetEditor, TranslateOptions, TranslateReport, translate_file}` 一律原地保留;
  本轮全是搬迁,`convert/mod.rs` / `translate/mod.rs` 继续 re-export 原路径 ⇒ 下游 `use` 不变。
  `DecompilerError` 与 `TranslateError` **不合并**(公开面,留单独评审)。
- 不引入语义 IR、不新增第三方依赖、不重写 decompile 算法、不为对称合并正反向控制流。
- 不拆出新的子目录(目录层级已经够深)。

### 3.5 合并的硬前置(评审 `PlanReview31` 对着代码实测出的三类卡点)

**(a) 测试模块同名冲突(E0428)** —— 直接拼接编译不过,必须显式改名:

| 合并组 | 冲突位置 | 改名 |
| ------ | -------- | ---- |
| `translate/model.rs` ← {model,kitten,neko} | `model.rs:215`、`kitten.rs:287`、`neko.rs:883` 各一个 `mod tests` | `mod model_tests` / `mod kitten_tests` / `mod neko_tests` |
| `translate/assembly.rs` ← {assembly,remint} | `assembly.rs:1407`、`remint.rs:375` | `mod assembly_tests` / `mod remint_tests` |
| `translate/nemo.rs` ← {nemo,nemo_xml} | 两者各一个 `mod tests` | 分别改名 |
| `shared.rs`(S1,已完成) | `upload.rs:150` 的 `mod tests` | 已改 `mod upload_tests` |

**(b) 生成物不可与手写表混放** —— `src/bin/gen_translate_tables.rs` 用 `std::fs::write` **整文件覆盖** `translate/tables_gen.rs`。
因此 `tables_gen_nemo.rs`(**人工转录**官方 bundle,无生成器)并入 **`nemo_mapping.rs`**,**绝不并入** `tables_gen.rs`;
否则哪天重跑生成器,全部 `NEMO_*` 常量会被静默删掉。

**(c) 引用改写清单**(合并后原路径失效,必须同步改;全部是机械替换):

| 原路径 | 新位置 | 需要改的调用点 |
| ------ | ------ | -------------- |
| `translate::kitten::{parse_block_data_json, build_block_data_json, …}` | `translate::model::…` | `translate/mod.rs`(正向 + 反向) |
| `translate::neko::{split_procedures, rewrite_calls, tree_to_json, parse_kn_entity, parse_kn_procedures, unrewrite_calls, def_root_from_entry, ProcedureEntry}` | `translate::model::…` | `translate/mod.rs`、`assembly.rs:1412`(`super::neko::split_procedures`)、`remint.rs:26`(`super::neko::ProcedureEntry`)、`reverse_tests.rs` |
| `translate::remint::{run_items, remap_*}` | `translate::assembly::…` | `translate/mod.rs`(8 处) |
| `translate::nemo_xml::{parse_fragment, XmlNode, …}` | `translate::nemo::…` | `nemo.rs:56/612`、`nemo_mapping.rs:56/732/762`、`nemo_tests.rs:725/733` |
| `translate::tables_gen_nemo::NEMO_*` | `translate::nemo_mapping::…` | `nemo.rs:57`、`nemo_mapping.rs:57` |
| `translate::mapping::truthy` | `translate::model::truthy`(A3 顺手收敛) | `assembly.rs:60` |
| `decompile::blocks::{…}` | `decompile::…`(并入 `decompile/mod.rs`) | `decompile/editors/kitten.rs:11` |

> `translate/tables_gen.rs` **保持独立文件名** ⇒ 它自己的路径(`translate::tables_gen::BCM_VERSION`)不变。

### 3.6 重复实现审计(`DupAudit`,2026-09-26)

> 目的:架构收敛(A3)前先把「真重复 / 假重复」分清。审计是只读的,逐条带 `file:line`。

| 项 | 判定 | 位置 | 结论 |
| -- | ---- | ---- | ---- |
| D2 | divergent | translate/mapping.rs:302-311 · translate/mapping.rs:314-320 · translate/mapping.rs:327-334 · translate/nemo.rs:1276-1283 · translate/nemo.rs:1287-1291 · translate/assembly.rs:734-736 · translate/assembly.rs:739-745 · translate/assembly.rs:748-754 · translate/model.rs:1836-1842 · translate/nemo_mapping.rs:538-542 | **不可合并**(会改行为) |
| D3 | exact | translate/mapping.rs:70 · translate/nemo_mapping.rs:51 | 可合并 |
| D4 | near | translate/mapping.rs:584-588 · translate/mapping.rs:591-599 · translate/model.rs:1430-1433 · translate/nemo_mapping.rs:952-956 | 可合并 |
| D5 | near | translate/assembly.rs:725-731 · translate/nemo.rs:1344-1352 · translate/nemo.rs:1319-1341 | 可合并 |
| D6 | divergent | translate/assembly.rs:518-523 · translate/nemo.rs:559-564 | **不可合并**(会改行为) |
| D7 | near | translate/assembly.rs:776-782 · translate/assembly.rs:596-606 · translate/assembly.rs:785-792 · translate/assembly.rs:482 | 可合并 |
| D8 | divergent | translate/assembly.rs:573-593 · translate/assembly.rs:1387-1403 · translate/nemo.rs:459-462 | **不可合并**(会改行为) |
| D9 | exact | shared.rs:1129 · translate/assembly.rs:768 | 可合并 |
| D11 | exact | decompile/mod.rs:996-1039 | 可合并 |
| D12 | exact | decompile/mod.rs:907-916 | 可合并 |
| N1 | exact | translate/nemo_mapping.rs:220-225 · translate/tables_gen.rs:1652-1657 · translate/mapping.rs:290-292 · translate/nemo_mapping.rs:243-245 | 可合并 |
| N2 | near | mod.rs (convert):215-218 · translate/mod.rs:717-720 · translate/mod.rs:721-724 | 可合并 |
| N3 | near | decompile/editors.rs:56-86 · decompile/editors.rs:782-814 · decompile/editors.rs:1104-1133 · decompile/editors.rs:1331-1376 · decompile/editors.rs:1440-1463 · decompile/editors.rs:897-1100 · decompile/editors.rs:1517-1660 | **不可合并**(会改行为) |
| N4 | near | translate/model.rs:707-708 · decompile/editors.rs:625-626 · decompile/editors.rs:333-334 | **不可合并**(会改行为) |

**逐条要点**

- **D2**(divergent,保留两份):整族「JS 值强转」散在两套装配 + 模型 + NEMO 解析里,已按官方不同分支各自照抄,合并会改行为。逐对差异:
  - 调用点:调用点(数量/文件):js_text 5 处(仅 translate/mapping.rs:666,1282,1580,1674,1675);value_key 12 处(仅 mapping.rs:363,436,686,689,1071,1278,1289,1419,1495,1509,1596,1659);truthy 10 处(assembly.rs:224,245,304,337,338,370,610,628 + mapping.rs:689,1595);truthy_value 5 处(nemo.rs:201,210,288,445,504);is_nil 2 处(assembly
- **D3**(exact,可合一):两份 `pub(crate) const XHTML` 字符串逐字节相同(`http://www.w3.org/1999/xhtml`)。mapping.rs 的那份被 translate/model.rs `use super::mapping::{XHTML, ...}` 借用(mapping.rs:586,665 与本文件多处 `{XHTML}`);nemo_mapping.rs 自带一份,在 815,826,929,953,1044,1087,1137,1559,1586,1760,1924,1950,2053,2071 等 ~15 处使用。另有 `mapping.rs:68-69 SHADOW_TEXT_MUTATION` 把同一字面量硬编进字符串;`decompile/mod.rs` 侧则完全没用常量,~30 处内联 `xmlns="http://www.w3.org/1999
  - 调用点:合并需决定落点:若 nemo_mapping 借用 mapping 的常量,会引入 NEMO→Kitten 映射 依赖 Kitten→KN 映射模块的方向耦合(两者本就同属 translate,风险可接受);更干净是提到 shared.rs。另需一并把 decompile/mod.rs 的内联字面量收敛(SHADOW_TEXT_MUTATION 与 ~30 处),否则仍有两处来源。调用点:2 个常量声明 + ~15 处 nemo_mapping 引用 + mapping/model 若干。
- **D4**(near,可合一):`math_number_shadow` 产出 `<shadow xmlns="{XHTML}" type="math_number" id="{id}" visible="visible"><field constraints="-Infinity,Infinity,0," name="NUM">{num}</field></shadow>`。nemo_mapping.rs:953-955 与它逐字节相同(仅源码里 `\` 换行续行,展开后一致);model.rs:1431 与之只差字段多一个 `allow_text="true"`(官方 `VC` 的字面量)。`math_number_node` 是同一节点的积木形态(kind/id/is_shadow/fields.NUM),由 mapping.rs 两处与 model.rs:1330 使用。
  - 调用点:调用点:math_number_shadow 2 处(mapping.rs:639,640)+1 处(model.rs:1740);math_number_node 3 处(mapping.rs:625,630、model.rs:1330);内联两份各 1 处(model.rs:1431、nemo_mapping.rs:953)。合并需给 helper 加 `allow_text: bool`(或第二个构造函数),否则 model.rs 那份不能直接换用;保留两处时 nemo_mapping 那份应直接改调 mapping::math_number_shadow(跨模块 borrow 无方向问
- **D5**(near,可合一):两者都是「整数值的 f64 发整数」:阈值同为 `9.007_199_254_740_992e15`、判据同为 `fract()==0.0 && abs()<阈值`。**不是逐字节相同**:`num` 把 `is_finite()` 并入快分支(`value.is_finite() && ...` → `json!(value as i64)`),else 落到 `json!(value)`;`number_value` 先 `if !number.is_finite() { return Value::Null }` 再走同一判据 → `Value::from`。对 NaN/±inf,`json!(value)` 在 serde_json 里同样折成 `Value::Null`,因此**语义等价**(round21 D5 记的「逐字节相同的两份 finish/kitten4_finish
  - 调用点:调用点:num ~15 处(全在 assembly.rs:235,237,255,260,514,590,591,612,630,631,633,1051,1112,1158,1163,1182,1183,1302);number_value 2 处(nemo.rs:202,1298);normalize_integral_numbers 1 处(nemo.rs:569,含自递归)。可把 num/number_value 合成 shared 或 model 里一份;注意 nemo 侧是整棵文档的递归归一口径,合并时保留该入口。
- **D6**(divergent,保留两份):两处都实现「projectName || 兜底」,但兜底字面量不同:`assembly.rs:73 DEFAULT_PROJECT_NAME = "空白作品"`(官方 `finish` 系)vs `nemo.rs:563 "新的作品"`(官方 NEMO 建作品默认名,见 api/work.rs:948 的 `work_name.unwrap_or("新的作品")`)。语义分支不同,不是纯重复。assembly 侧另有一层正向包装 `fn project_name(src)`(≈526)只是 `project_name_at(src, "project_name")`。
  - 调用点:调用点:project_name_at 2 处(assembly.rs:154/1046 + 正向包装 527);nemo 内联 1 处。需保留两份常量并注释各自对应的官方路径。
- **D7**(near,可合一):round21 的 D7(两份正逆表)**已合一**:现在只有 assembly.rs:776 一张 `VAR_STYLE_TABLE`,theme_style/theme_of_style 是这张表的正向/反向访问器,不是重复数据(docs/rounds/21 §8.2 已记「主题表合一为 VAR_STYLE_TABLE」)。残留重复只有默认值字面量 `"default"`:assembly.rs:482 用 `VAR_STYLE_DEFAULT`(定义在 assembly.rs:77),nemo.rs:513 硬编同一字符串。
  - 调用点:调用点:theme_style 1 处(assembly.rs 变量条目构造 ≈470);theme_of_style 1 处(assembly.rs 反向变量条目 ≈1300);`"default"` 2 处(assembly.rs:482、nemo.rs:513)。只需把 nemo.rs:513 改成引用 assembly::VAR_STYLE_DEFAULT(或提到共享常量)。
- **D8**(divergent,保留两份):正向 `x' = (x + src_w/2)*target_w/src_w`、`y' = (src_h/2 - y)*target_h/src_h`;反向 `x = x'*canvas_w/kn_w - canvas_w/2`、`y = canvas_h/2 - y'*canvas_h/kn_h` —— 严格互逆,但参数形态不同(Option<&Value>+标量 vs &Value+两对画布元组),且正向经 `num()` 取整、反向交调用方取整。round21 §8.2 已决定「坐标公式不合并、同文件相邻 + 互逆说明」,该说明现位于 assembly.rs:1383-1386。nemo.rs:459 是第三条变体(源舞台中心系↔目标左上系的重定心)。
  - 调用点:调用点:stage_position 1 处(assembly.rs 正向变量坐标 ≈470);kitten4_position 1 处(assembly.rs:1301);nemo 内联 1 处(nemo.rs:461)。保持两份 + 互逆注释(已有),勿为对称强行抽象。
- **D9**(exact,可合一):两份私有常量字符串逐字节相同(`4.11.20`),分别服务两条不同管线:shared.rs:1216(建草稿作品时填 `version`,Kitten4 建作品端点)与 assembly.rs:1043(反向 `.bcm4` 装配写 `application_version`)。
  - 调用点:调用点各 1 处(shared.rs:1216、assembly.rs:1043)。合并后放 shared.rs 更合理(两条管线都已在用 shared);注意 shared.rs 的这份是私有 `const`,提升为 `pub(crate)` 即可跨模块复用。
- **D11**(exact,可合一):round21 D11 的「一处识别字符串+对象引用、一处只识别对象」**已合一**(docs/rounds/21 §8.1 S1.3「D11 `referenced_ids` 合一并改对象-only + 遇字符串显式报错」,§8.3 记录真样本 462 块 / 4256 处引用无字符串)。当前全布局只剩 decompile/mod.rs:996 一份:读 `next_block`(对象)、`child_block[*]`/`conditions[*]`(对象)、`params.*`(对象),遇字符串 `return Err(reject_string(...))`。
  - 调用点:调用点 2 处,均在同一 crate:decompile/editors.rs:330(`decompile_root_blocks`)与 decompile/editors.rs:621(`XmlBlockWriter::write_blocks`)。无重复可做;仅需注意若日后 Kitten2 真机样本出现字符串引用,应在**这一份**里恢复字符串分支(round21 §4-16 的保留条款)。
- **D12**(exact,可合一):round21 D12 的「子插槽命名规则两份」**已合一**(§8.1 S1.1「D12 插槽命名合一」)。当前全布局只剩 decompile/mod.rs:907 一份,规则 `controls_if|controls_if_no_else ⇒ index<conditions_count ? format!("DO{index}") : "ELSE"`;文档注释(≈906)自称「唯一的规则来源」。`format!("DO{}", index)` 在全仓只出现一次。
  - 调用点:调用点 2 处:decompile/mod.rs:1191(`BlockDecompilerCore` 子块展开)与 decompile/editors.rs:694(`XmlBlockWriter`,注释「与反编译重建共用同一套插槽命名规则」)。无重复可做。
- **N1**(exact,可合一):**新发现**:同名谓词 + 同内容集合各一份。tables_gen.rs:1652 是生成物里已有的 4 元素数组(`TEXT_PLACEHOLDER_BLOCKS`),mapping.rs:290 在用它;nemo_mapping.rs 却又手写了一份等价的 `TEXT_PLACEHOLDERS`(220-225,同样 4 个 `bcm_translator_text_*`)并另写一个同名 `is_text_placeholder`(243)。两个数组元素逐字节相同,两个谓词体同构。合并安全,且**不回写生成物**:改 nemo_mapping 引用 `tables_gen::TEXT_PLACEHOLDER_BLOCKS` 即可(不可反过来改 tables_gen.rs,它被 `src/bin/gen_translate_tables.rs` 整文件覆盖)。
  - 调用点:调用点:mapping::is_text_placeholder 4 处(mapping.rs:970,1272,727 隐含 + 定义;含 `PLACEHOLDERS_*` 走查;实际直接调用在 970 与 1272);nemo_mapping::is_text_placeholder 4 处(nemo_mapping.rs:924,1279,1356,并定义)。合并后 nemo 侧仅删本地常量与本地谓词,改 `use super::tables_gen::TEXT_PLACEHOLDER_BLOCKS`。
- **N2**(near,可合一):**新发现**(round31 §3 A4 点名但未做):同一个 2 变体枚举 `TargetEditor` 在 3 处各写一遍穷尽 `match`,只是产物不同(EditorType / 文件名 slug / 扩展名)。三者互不逐字节相同,但都属于「散落的 TargetEditor ⇄ 目标编辑器属性」映射,新增编辑器时要同时改多处。
  - 调用点:调用点 3 处:convert/mod.rs:215(建草稿时挑上传渠道的 editor)、translate/mod.rs:717 与 :721(同一函数 `product_path` 里的 slug/ext)。建议在 model.rs 加 `TargetEditor::{editor_type(), slug(), extension()}`(round31 A4 的落点),三处改调用;公开类型与公开面不动。
- **N3**(near,保留两份):**新发现(round21 D13 同项)**:5 个 Fetcher 的字段(dyn HttpClient + Arc<DecompilerConfig>)与 `new` 构造函数逐字节同形,`impl WorkFetcher::fetch` 的前半段(取 source_url → 二次 GET)也高度雷同;两个 `*ResourceManager`(+ 两个 `*ResourceConfig`)同样成对同构(dirs: HashMap<String,PathBuf>、create_directories 结构一致)。
  - 调用点:调用点:各 Fetcher 只在 decompile/mod.rs 的 `fetcher_for` 里构造 1 次;两个 ResourceManager 各在其 Decompiler 内构造 1 次(NemoDecompiler ≈840、WoodDecompiler ≈1485)。round21 §8.2/§3.2 明确「不建议合并:每个编辑器的差异点才是重点,可选抽公共构造」—— 若合并,只抽字段与 `new`(共同壳),URL 拼装与解密路径必须各自保留。
- **N4**(near,保留两份):**新发现**:反编译侧 `XmlBlockWriter` 的「根块纵向间距 220」被硬编为浮点 `y += 220.0`(editors.rs:626),而 translate 侧 `model.rs:707-708` 又把同一约定独立定义成 `ROOT_LAYOUT_Y=80 / ROOT_LAYOUT_STEP=220`;model.rs:706 的注释自称「`XmlBlockWriter` 的约定:首根 80、每根 +220」。但两边**首根起点不一致**:model 侧从 80 起(80 + 220·i),editors.rs 侧从 `y = 0.0` 起(0 + 220·i)。两处魔法数各写一份、且起点有 80 的漂移,改动单边会静默错位。
  - 调用点:调用点:ROOT_LAYOUT_Y/STEP 1 处(model.rs:726 布局);editors.rs 的 220.0 1 处(626)+ 50.0 两处(334、352,后者是函数体行距不是同一约定)。**先核实起点 0 vs 80 是有意还是漂移**,再决定是否共享常量;若确认是同一约定,则把 220/80 提到一处(decompile 与 translate 分属两个子域,可能需要放到 shared.rs)。在核实前不要合并。

**零调用点私有项(可删,需复核)**

- pub(crate) const TOP_BLOCKS: &[&str] —— `translate/tables_gen.rs:1203`(全仓 grep `TOP_BLOCKS` 命中 2 处:定义(tables_gen.rs:1203)与生成器写出的常量名字符串(src/bin/gen_translate_tables.rs:172)。零消费者 —— 无任何 `tables_gen::TOP_BLOCKS`/`TOP_BLOCKS` 使用点,且 `pu)
- pub(crate) const KN_TYPES: &[&str] —— `translate/tables_gen.rs:1216`(全仓 grep `KN_TYPES` 命中 4 处:定义(tables_gen.rs:1216)、生成器字符串(src/bin/gen_translate_tables.rs:184)、以及两条**注释**(translate/mod.rs:33、translate/mapping.rs:909,都是文字提到 `KN_)
- pub(crate) fn BlockJson::count_types(&self, out: &mut BTreeMap<String, usize>) —— `translate/model.rs:171`(全仓 grep `count_types` 命中 6 处:两个定义(model.rs:171 与 192)、定义间的内部调用(model.rs:195)、以及全部在 `#[cfg(test)]` 里的调用(model.rs:272、reverse_tests.rs:759)。生产路径零调用 —— 只有测试用;`pub()
- pub(crate) fn BlockTree::count_types(&self) -> BTreeMap<String, usize> —— `translate/model.rs:192`(同上一条:grep `count_types` 的 6 处里,生产调用为零,唯一非定义调用点是 model.rs:272(测试)与 reverse_tests.rs:759(测试)。与其上一条同生共死。)
- pub(crate) fn parse(xml: &str) -> Result<XmlNode, DecompilerError> —— `translate/nemo.rs:1677`(全仓 grep `parse(` 的调用点**全部**在 nemo.rs 的 `#[cfg(test)]` 模块内:nemo.rs:2211,2244,2319,2328,2395,2445,2448,2456,2548,2559,2571;生产侧只用 `parse_fragment`(nemo.rs:624、nemo)
- impl Default for DecompilerContextBuilder + 其 6 个增量 setter —— `decompile/mod.rs:716-719(Default)、750-780(setters)、782(build)`(grep `DecompilerContextBuilder` 全仓命中:定义(706)、`impl Default`(716)、唯一构造点 decompile/mod.rs:494-503、以及 use 导入与个别测试。`DecompilerContextBuilder::default()` 零调用(只有 `new)

**decompile 骨架评审(纯仪式 vs 真实语义)**

- 静态分派 `fetcher_for` / `decompiler_for`(原 WorkProcessorRegistry 注册表) —— `decompile/mod.rs:112-152(fetcher_for 121、decompiler_for 139、说明注释 114-118)`:real semantics(且注册表已删,无仪式残留)(round31 §3 A2 要删的 `HashMap<EditorType, Box<dyn Fn(..)->Box<dyn ..>>>` + 两个工厂类型别名在当前代码里**已不存在**:grep `WorkProcessorRegistry` 全仓 0 命中,只剩两个自由函数 `match EditorType` 穷尽 7 型(Kitten2/3/4→Kitten*、Neko/Nemo/Woo)
- `DecompilerContext` —— `decompile/mod.rs:690-702`:real semantics(不能动)(它本身就是 plain struct(8 个 `pub(crate)` 字段:output_dir/resource_concurrency/download_resources/work_info/http_client/file_service/id_generator/config)。携带真实状态:work_info 决定扩展名与端点、http_client/file_service 供 N)
- `DecompilerContextBuilder`(+ `impl Default`) —— `decompile/mod.rs:706-791(Default 716、build 768)`:ceremony(可换成 plain struct / 单一构造函数)(7 个 `Option` 字段 + 6 个增量 setter + 一个**零调用**的 `Default` 实现,只为**唯一一个**构造点(decompile/mod.rs:494-503)服务;该点把每个字段都显式设了值 ⇒ `build()` 里对 `work_info`/`http_client` 的两个 `ok_or_else` 在该调用点是不可达分支,增量 `mut self -> S)
- `DecompileResult` —— `decompile/mod.rs:794-797`:real semantics(不能动)(`enum { Json(Value), Path(String) }` 是**每个反编译器与保存层之间的内部契约**:Kitten/NEKO/NEMO/COCO 返回 Json 交 `save_json_result`,WOOD 返回 Path 交 `save_path_result`(853-893)。它不是可省的中转层 —— 两个 saver 都靠它对**错误形态**做断言(「应返回 JSO)
- `DecompiledArtifact` —— `decompile/mod.rs:804-820`:real semantics(公开面;但与 DecompileResult 概念重叠,可收敛)(`pub enum { Document{document,file_name,source_version}, Path(PathBuf) }` 是**对外**产物描述(方案 23 P0-2 的内存形态),被 convert/mod.rs:23 使用;`decompile_artifact_with`(≈287-372)把内部 `DecompileResult` 映射成它(Json→Docume)
- `EditableDocument` vs `DecompiledArtifact::Document` —— `decompile/mod.rs:823-828 / 804-820`:near-ceremony(可并入,但当前形态有理由)(`EditableDocument{document, source_version}` 是 `WorkDecompiler::editable_document`(830-844 默认返回 `Ok(None)`)的返回类型;`DecompiledArtifact::Document` 是它的超集(多 `file_name`,且 file_name 由门面按 work_info 现算)。两者字段重)
- `DecompileOutcome` 与 `decompile_inner`/`decompile_core` 包装 —— `decompile/mod.rs:156-165(DecompileOutcome)、432-443(decompile_core)`:partial ceremony(`DecompileOutcome{artifact, work_id, editor}` 是旧的「必落盘」入口的返回;`decompile_artifact_with` 是新的内存入口 —— 两个入口各自重复一段「确定 output_dir → save/不 save → 可选 upload」的编排(≈390-430 vs ≈287-372)。`decompile_core`(432)只是 `p)
- `save_json_result` / `save_path_result`(共享落盘助手) —— `decompile/mod.rs:854-893`:real semantics(不能动)(两个 saver 被 5 个 Decompiler 的 `WorkDecompiler::save_result` 复用,并统一命名口径(`FileService::safe_filename` + 扩展名),确保内存入口(decompile_artifact_with:305-335)与落盘入口给出**逐字一致**的文件名。不是仪式。)
- `download_resources_parallel`(第四处自建批处理) —— `decompile/mod.rs:585-680`:real semantics(与 batch_map 语义不同,不要合并)(它也是 chunk + `thread::scope` 形状(另在 shared.rs:1083 `batch_map`、assembly.rs:1919 `run_items` 各有一份),但**不是**同构重复:这里带「已存在且非空则跳过」的预过滤、失败重试一轮、`Mutex<Vec<String>>` 收集失败清单,返回值是失败列表而非保序结果。与 A6 的 `batch_map`(保序、按)

## 4. 迁移步骤(4 个提交,每步可独立回滚)

| 步 | 内容 | 状态 | 验证 |
| -- | ---- | ---- | ---- |
| **S1** | `shared/` 6 → 1(`shared.rs`,含配置与影子模板表);删掉汇总再导出层(A1) | ✅ `d3d995b` | `cargo check --all-targets` + 单测数不变(97/2/2) |
| **S2a** | `decompile/blocks.rs` 并入 `decompile/mod.rs`;`decompile/editors/*` 4 → 1(`editors.rs`) | ✅ `f8ce473` | 同上 |
| **S2b** | A2:注册表(`HashMap<Box<dyn Fn>>`)→ `match` 静态分派 | ✅ 本轮 | `cargo check --all-targets` + 全量 |
| **S3** | `translate/` 14 → 9:`model` 吸收 `kitten`+`neko`、`assembly` 吸收 `remint`、`nemo` 吸收 `nemo_xml`、`nemo_mapping` 吸收 `tables_gen_nemo`;按 §3.5(c) 改引用、§3.5(a) 改测试模块名 | ✅ `41470d9` | 同上 + **产物 SHA256 基线不变** |
| **S3b** | A3 / A4 / A6 + 骨架瘦身 | ✅ 本轮:**A2**(静态分派)、**A4**(`TargetEditor::{as_editor,file_slug,file_extension}`)、**A6**(`shared::batch_map`)、**A3 的 D3**(`XHTML` 常量唯一化)与 **N1**(`TEXT_PLACEHOLDER_BLOCKS` 复用)、**`DecompilerContextBuilder` 删除**;A3 其余项**按审计定性处理**(见 §3.6:D2 族 `unify_safe: no` ⇒ 保留两份并记档) | 同上 |
| **S4** | 文档同步(README 目录树、`docs/knowledge/repo-conventions.md`(已改)、`docs/README.md`、本方案勾选)+ 真机上传用例跑一次 | 部分 | 文档引用可解析 + 全量测试 |

每步之间不混:搬迁提交不改逻辑 ⇒ 任何回归都能一眼定位到某个提交。

> **本轮实际完成:S1 + S2a + S3 —— `src/core/convert/` 27 → 13 个文件(−52%)**,三处提交各自独立可回滚。
> 途中三个真实坑(多行 `use` 块、`mod tests` 撞名、`super::` 兄弟导入失效)与修法见 §3.5;合并脚本见 `temp/merge_rs.py`(gitignored,一次性工具)。

## 5. 验证口径(每一步都跑)

1. `cargo check --all-targets` + `cargo clippy`(零新增告警);
2. `cargo test --lib`(基线 **97 passed / 1 ignored**)+ `cargo test --test repo_hygiene`;
3. `cargo test --test convert_live`(离线真机 2 条);
4. **产物不变**:`tests/convert_bench.rs` 的自有 SHA256 基线与 `reverse_tests` 的往返多重集断言必须全绿
   —— 纯搬迁不允许改变任何产物字节;
5. 真机上传用例(`--ignored`)在 S4 后跑一次,确认端到端仍通。

## 6. 已核实(两个只读代理,2026-09-26)

- **逐文件清点**(`ConvertMap`):职责/行数/测试占比/域内依赖已用于 §2 的文件数与行数估计。
  其中发现 `translate/mod.rs` 与 `decompile/mod.rs` **各有一套同构批处理** ⇒ 列为 A6;
  `translate::TargetEditor` 与 `EditorType` 两套平行枚举 ⇒ A4。
- **方案评审**(`PlanReview31`):**方向可行,但不能当"纯搬迁 + 改 use"直接做**。三类硬卡点(测试模块同名、生成物混放、路径改写遗漏)已写入 §3.5,目标结构与步骤已按评审修正:
  ① `tables_gen_nemo` 改并入 `nemo_mapping`(**不进**生成物文件);② 每条合并都要显式改测试模块名;③ 引用改写清单逐条列出。
- 评审同时确认:各组顶层项集合互斥(除测试模块名)、无语义可见性升级需求、夹具均以 `CARGO_MANIFEST_DIR` 定位、仓库无 `include_str!`/`#[path]` 魔法 ⇒ 搬迁本身不引入行为风险。