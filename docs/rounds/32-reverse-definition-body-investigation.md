# 第三十二轮记录 — 反向定义体缺口(6/21)的调查进展

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

## 3.1 第 1 步已完成:三分定性 ⇒ **丢在反向这一腿**(2026-09-26)

给测试加了受 `DUMP_K4` 环境变量控制的**按作品分开落盘**的中间态 + K4 侧定义体 census(顺 `connections` 递归)。

> ⚠️ 过程记录:第一版 dump 用了固定文件名,**被循环里第二个作品覆盖** ⇒ 头几轮分析看错了文件。
> 现在按作品名分文件(`/tmp/k4-dump-<作品名>.json`),下面的数字是**对着正确文件**重做的。

| 观测(作品 A `Node VM v3`) | 结果 |
| -------- | ---- |
| 源 `proceduresDict` | 52 条条目 → **51 个唯一定义名** |
| 中间态 K4 的 def 块 | 47 个(**按块 id 兜底计数**;按 `fields.NAME` 取到 0 个 ⇒ 命名口径尚未对齐,别据此下结论) |
| 5 条受影响定义(128 / 126 / 209 / 9 / 1171 块) | 中间态里**没有同尺寸的定义体**(最近的 119/141/204/1163 都是别的定义)⇒ **反向确实没把它们写出去** |
| 第 6 条(`b9858f0c`,10 块) | 中间态有多个 10 块定义 ⇒ 尺寸撞车,**存疑**(需按调用点/双条目对齐再判) |
| ⇒ 定性 | **缺口出在反向(KN → Kitten4)的输出路径**,不是正向 |

**已排除的猜想**(都验过,不要再走):
- "零调用定义挂不上宿主会被丢" —— 读代码确认 **每个定义都会挂到宿主实体**(`assembly.rs`:`host` = 第一个非角色实体,
  没有实体才退化成告警);而且该作品 51 个定义里 **42 个零调用**,但中间态只缺 ~4~5 个 ⇒ **零调用不是判别条件**。
- "定义体是结构性带不走(list 参数等)" —— 那只解释参数块减少,解释不了整条体消失。

**⚠️ 未对齐的口径(下一步先解决)**:K4 中间态里的 `procedures_2_def*` 块按 `fields.NAME` 取不到名字
(上一版的"47 个"是块 id 兜底)。但**尺寸指纹结论与名字无关** ⇒ "5 条定义的体没进中间态"这一条仍成立。
先对齐命名口径(看反向写出的 def 根到底把名字放在 `fields.NAME` / `mutation` / 还是都没有),
否则后续的"判别轴"对比会拿错集合。

**下一步的判别轴**(还没做):对比"丢失的这几条"与"活下来的 46 条"在
`NORMAL`/`ROUND` 双条目、调用点数量、体量、宿主实体、是否有 `warp`/`temporary_list` 这些特征上的差异 ——
一次脚本枚举即可定位判别条件,再去读反向对应的那一段。

**当前可用的调试手段**(已入库、受环境变量控制):
`DUMP_K4=1` 把每个作品的中间态落盘(`/tmp/k4-dump-<作品名>.json`)并打印每实体的 def 分布;
`DUMP_K4=full` 额外打印一个 `procedures_2_defnoreturn` 块的完整 JSON。

## 3.2 口径修正:缺口 **133 → 21**(2026-09-26,同一轮内)

按 `block.id`(而非名字)对齐中间态后,查出**测试口径本身在造假**:

| 源里同名的两条 `proceduresDict` 条目 | type | 体量 | **首个块** |
| ------------------------------------ | ---- | ---- | ---------- |
| `bfa2f83c-…` | ROUND | 16 块 | `procedures_2_defnoreturn` ← **真正的定义根** |
| `08dc6ce9-…` | ROUND | **128 块** | `procedures_2_callreturn` ← **调用子树,不是定义体** |

旧 `def_census` 按**名字**聚合、取较大者 ⇒ 把那条 128 块的**调用树**当成"定义体"去比,凭空造出 118 块的缺口。

**修正**:只把"首块是定义块(`procedures_2_def*`)"的条目算作定义体(`def_census` 里已加该过滤 + 注释)。重测:

| | 旧口径 | **新口径** |
| --- | --- | --- |
| 受影响定义 | 6 | **6**(同一批) |
| 净减块 | 133 | **21** |
| 逐条 | `bfa2f83c` 118 / `6be6ac61` 5 / `71540544` 4 / `b9858f0c` 3 / `c50c29e1` 3 | `bfa2f83c` **6** / `6be6ac61` 5 / `71540544` 4 / `b9858f0c` 3 / `c50c29e1` 3 / `923e1ade` 0(净增 2) |

⇒ **86% 的"缺口"是测量假象**。真实剩余的 21 块里,`procedures_2_parameter` 的减少(6 块)已确认是**结构性**
(Kitten4 参数体系没有 list 类型,见 §2);其余 ~15 块分散在 `script_variables`/`break`/`repeat_n_times`/
`logic_compare`/`callnoreturn`/`temporary_list` 上。

**预算已按 round 28 §4.3 收紧**:`affected <= 6 && deficit <= 21`(原为 6 / 133),并在断言里写明口径变更的原因。

## 3.3 顺带验了另一条:inline `pure_list_get` 缺口**是真的**(2026-09-26)

round 28 还记了一条"inline `pure_list_get` 影子在往返里丢失"。用同一套方法(落盘源/往返文档 + 分别数
**节点**与 **shadows XML 串**里的 `pure_list_get`)验:

| 作品 | 源(节点/影子串/合计) | 往返后 | 结论 |
| ---- | --------------------- | ------ | ---- |
| `now`(`FjDQB0v0…`,该缺口的持有者) | 233 / 270 / **503** | 206 / 245 / **451** | **真的少了 52 处提及** ⇒ 不是口径假象 |
| `Node VM v3`(`FjSwU2i…`) | 1 / 1974 / 1975 | 0 / **2094** / 2094 | 反而**多** 120 ⇒ 形态差异(影子在两种序列化间来回) |

⇒ 两条结论:
1. **`pure_list_get` 的丢失是真的**(节点与影子串**同时**下降),量级以测试自己的实体侧 census 为准:**−24**(`186 → 162`);
2. **"影子串计数"不能单独当损失指标** —— 它随序列化形态变化(同一条信息可以既是节点、又是一段 XML),
   所以判断损失要看**节点数的 census**(测试里用的就是这个),不要用文本提及次数。
   (本轮实体侧打印同时暴露了另两处**已文档化**的差异:横屏包装 `math_arithmetic`/`math_number` 成对增加,`calculate ⇄ 占位积木` 1:1。)

⇒ 因此 `pure_list_get` 那一项**保留在 allow-list 里是对的**,但它对应的是一个**待修的实现缺口**
(round 28 的假设:`fold_pure_list_get`(反向)与云列表特例(正向)口径不一致),不是测量问题。

## 3.4 修复:正向「列表影子」步骤写错了位置(2026-09-26,同一轮内)

§3.3 把缺口定位成"实体侧 −24,节点与影子串同降"。**根因后来找到了,而且不在反向**:

正向 `mapping.rs::route_children` 的 (9) 步(把 `fields.list` 变成 `inputs.list` + 影子节点 + 影子 XML)
原先写在 `for (from_value, slot, child) in inputs.chain(statements)` **循环体内** —— 而它的条件
(`LIST_INPUT_TYPES.contains(kind)` 且 `fields.list` 存在)与"有没有已连接的子块"无关。于是:
**没有任何子块连接的块永远进不了循环**,既不转换字段、也不造影子;而反向(自家实现)对所有块都折叠
⇒ 两者不对称 ⇒ 往返一圈丢掉这些影子。

**证据**(落盘源 / 中间态 / 往返三态,按块类型分组):

| 阶段 | 39 个 `delete_list_item` 的形态 |
| --- | --- |
| 中间态(Kitten4) | 全部 `fields=(TYPE,VAR)`、无影子节点(反向折叠是对的 ⇒ 反向无过) |
| 往返后(KN) | **11 个** `fields=(item,)` + `pure_list_get` 节点(有子块,正向补回来了);**28 个** `fields=(item,list)`、无节点(无子块,正向从没进过循环) |

只有 `delete_list_item` 掉(39 → 11),其它 7 种列表积木全等 ⇒ 与"该块没有子块"这一条件完全吻合。

**改法**:把该步骤**提到循环外**(同条件、只做一次) ⇒ 对原本正确的 11 个零行为变化。

**验证**:
- 实体侧差异里 `pure_list_get` 那行**消失**;`pure_list_get` 合计 **503 → 507**(不再减少);
- 官方基线差分门 `diff_tests` 仍通过 ⇒ 改动与官方产物不冲突;
- 实体侧 allow-list 的 `pure_list_get:` **已移除**(门真的守起来);定义体侧那一对保留,理由见 §3.5;
- 全量 `cargo test --lib` **99 passed**。

⇒ 提交 `0dce9d6`。**§3.3 的归因要改口径**:缺口在**正向**,`fold_pure_list_get` 没有嫌疑。

## 3.5 语料扩容 + 往返扫描器;定义体侧那一对的成因(2026-09-26)

**语料**:`download/compile/` 里原本还有 `HEX Editor_317683843.bcmkn`、`滑动算法_301113412.bcmkn`
两件真作品没被用上。前者满足专测前提(有程序集定义 + 有角色)⇒ 进 `PROCEDURE_LIBRARIES`;
后者是普通作品(没有定义)⇒ 专测前提不成立,改由新扫描器覆盖。

**新增扫描器 `kn_corpus_round_trip_sweep`**(通用,任意 `.bcmkn`):每件作品跑一遍 KN → Kitten4 → KN,
逐条打印实体侧、定义体侧的类型多重集差异。**不设保真断言** —— 新语料上的差异要先读懂语义再定性;
它只守两条铁律:① 每件作品都转换得动(解码 / 解析 / 两个方向都不 Err、不 panic);② 往返确定性
(同输入两遍,最终产物逐字节一致 —— 两腿任一腿不确定都会被抓住)。语料每加一件就多一组块形态组合,
而上一轮的影子缺口正是"只在某类块 + 某个分支下"才暴露。

4 件语料的扫描结果:3 件有差异,**全部落在已文档化的族里**(横屏包装 `math_arithmetic`+`math_number` 成对、
`calculate ⇄ bcm_translator_text_return_value_block` 1:1、以及下面这一对)。

**定义体侧 `pure_list_get ⇄ procedures_2_callreturn` 成对减少的成因(已查明)**:把源文档里的
`pure_list_get` 节点逐一打印,`FjSwU2i…` 里**只有一个**,形态是

```json
{"type": "pure_list_get", "fields": {"list": "?"}, "is_shadow": true, "is_output": true,
 "parent_id": "aulxRNaKf2s7zkcqe6Ys"}
```

—— 列表名是**字面量 `"?"`**,即作品自己的"未设置 / 已被删除的列表"退化影子。往返后它消失,
同一条定义体里少掉的那个 `procedures_2_callreturn` 是同一处调用点的输入。**这是退化数据的归一化,
不是数据丢失**:`"?"` 在 Kitten4 侧没有可表达的对应物(列表槽只能填列表名),带不走。

⇒ 定义体侧的豁免**保留**,理由从"待证结构性"改写为"退化影子(`fields.list="?"`)归一化",
并在 `reverse_tests.rs` 注释里写明:与实体侧那处(已修)**不同源**。预算维持 `affected <= 6 && deficit <= 21`。

## 4. 下一步

1. ✅ §3.4 已修(影子步骤提出循环)+ §3.5 已把定义体侧那对定性为退化归一化;
2. **继续扩语料**:用仓库 API 抓更多真作品(`/creation-tools/v1/works/list`、
   `/neko/works/list/user/published`,或公开发现流),反编译落盘到 `download/compile/` 后由扫描器自动纳入
   —— 语料越杂,越容易撞出"某类块的某条分支";
3. 定义体侧剩余的 `script_variables` / `break` / `repeat_n_times` / `logic_compare` / `temporary_list` 减少,
   随新语料一起复查:先分清哪些是"同一个调用点塌陷"的连带计数、哪些是真丢。

> **不做**:不动 `docs/rounds/28`(历史保真);扫描器**不加**保真断言(它的价值在暴露差异,判据留在专测)。

