# 第三十二轮记录 — 反向定义体缺口(6/133)的调查进展

日期:2026-09-26 · 基线:`bc9eb5d` · 上游:`docs/rounds/28`(该缺口的首份记录)
样例:真作品 `download/compile/FjSwU2iKY6bLe6fexZJTvsX3X7AI.bcmkn`(3.4 MB,`Node VM v3 - 全猫最强解释器`)

> **状态:缺口已复现并细化,尚未修。** 本轮只做证据采集,按 `docs/rounds/28` §4 的三步走,
> 但把"猜根因"换成了"先拿到中间态"。**不修改历史轮次**(`docs/rounds/28` 保持原样)。

## 1. 复现(基线未变)

`cargo test --lib procedure_library -- --nocapture` ⇒ 6 个定义受影响、净减 133 块(**与 round 28 记录一致**):

| 定义(key) | 差异 | 净减 |
| ---------- | ---- | ---- |
| `bfa2f83c-…` | `break 1→0 · controls_if 2→0 · list_append 4→0 · list_item 11→0 · logic_compare 3→0 · logic_empty 1→0 · procedures_2_callnoreturn 6→0 · procedures_2_parameter 40→2 · procedures_2_return_value 2→1 · repeat_forever_until 1→0 · replace_list_item 3→0 · temporary_list 8→1 · variables_get 1→0 · warp 1→0` | **118** |
| `6be6ac61-…` | `script_variables 2→1 · script_variables_param 5→3 · variables_set 5→4` | 5 |
| `71540544-…` | `procedures_2_parameter 60→58` | 4 |
| `b9858f0c-…` | `logic_compare 2→1 · procedures_2_parameter 2→1` | 3 |
| `c50c29e1-…` | `break 1→0 · repeat_n_times 1→0` | 3 |
| `923e1ade-…` | `text 36→35`(该定义整体**净增** 2 ⇒ 非损失) | 0 |

⇒ **一条定义吃掉 118/133(89%)**,其余是零碎;**"整条定义体几乎消失"就是它**。

## 2. 反向报告分布(本轮新增的观测)

给测试加了按需打印(`--nocapture` 时输出),该作品的反向报告:

| 类别 | 条数 | 主体样例 |
| ---- | ---- | -------- |
| 类型歧义 | 1855 | `controls_if`、`delete_list_item`、`list_append`… |
| 未映射积木(保留原类型名) | 465 | `calculate`、`get_timer`、`logic_empty`、`script_variables`… |
| **丢弃字段** | **97** | **全是 `procedures.<id>.param.<name>(type=List)`** |
| 丢弃实体属性 | 17 | KN 顶层键 `aiImageUrls`/`courseMaterials`/`guideUrl`/`previewUrl`… |

⇒ `type=List` 的**程序集参数**被丢弃 97 次,与 `procedures_2_parameter 40→2` 这类减少对得上。
Kitten4 是 Scratch 派生的参数体系(字符串/标签),**没有 list 类型参数** ⇒ 这部分属**结构性差异**,
不是"我们写错了"(与 `docs/rounds/31` §3.6 的 D2 族同理:看着像 bug,其实是格式能力差)。

## 3. 定义体的真实形态(直接数源文件)

该作品的 `proceduresDict` 里定义块的参数形态很不寻常:

- 参数块是 **无类型、输入槽全空**的 `procedures_2_parameter`(字段只有 `param_name`);
- 单条定义的参数块数量可达 **336 / 230 / 227 / 149 / 109**;最大一条 **1665 个节点**;
- 与 `docs/rounds/28` 的**假设①**(官方把深树编成 "`next` 栈",`TOP_BLOCKS_AS_NEXT_STACK`)吻合。

⚠️ **口径警告**:测试里 `def_census` 是按**定义块 `fields.NAME` 聚合整棵树**;我按"定义块自身子树"扫描时,
没有任何定义块的参数块数恰好是 40。⇒ **两边口径不同,不能凭这个数字猜根因**(这正是本轮不信"假设①"就动手的原因)。

## 4. 下一步(必须先拿中间态,再谈修)

1. **拆开往返**:在同一测试里对**中间态 K4 文档**也做一次定义体 census(按定义 `NAME` 匹配),
   对比 KN → K4 与 K4 → KN 两段 ⇒ 一句话回答"是反向丢的,还是正向丢的"。
   (正向在真 Kitten4 作品上是零未映射,但定义体路径与实体路径不同,不能直接外推。)
2. **最小复现**:把那条定义(及其宿主实体)从作品里切出来做成夹具(`tests/fixtures/`),
   让修复有秒级反馈(现在每次都要跑 3.4 MB 真作品)。
3. 定位后再决定:若确为实现缺陷 ⇒ 修 + 把预算从 `affected<=6 && deficit<=133` **收紧到 0**
   (`docs/rounds/28` §4.3);若属结构性(如 List 参数)⇒ 从预算里剔除并写明理由。

> **不做**:不动 `docs/rounds/28`(历史保真);不在没有中间态证据前改反向映射(那是本域风险最高的代码)。
