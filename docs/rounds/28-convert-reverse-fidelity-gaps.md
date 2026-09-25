# 第二十八轮记录 — KN→Kitten4 往返的已知保真缺口(留置)

日期:2026-09-25 · 发现者:新增的"纯程序集库"真作品测试
(`reverse_tests::procedure_library_reverses_to_kitten4_and_is_deterministic`)

> **状态:已定位、已量化、已加回归预算,未修。** 按"难以解决先留置"的约定,本文只记录证据与研究路线。

## 1. 触发样例

两份平台真作品(本地 `download/compile/`,gitignored):

| 代号 | 内容(`projectName`) | 规模 |
| ---- | ------------------- | ---- |
| 文件 A | `Node VM v3 - 全猫最强解释器` | 3 角色 / 1 场景 / **52 条程序集定义** / 3.4 MB |
| 文件 B | `now` | **101 角色** / 9 场景 / 29 条定义 / 3.8 MB |

> 两者都是用户提供的平台作品文件,落在本机 `download/compile/`(gitignored),文件名形如
> `FjSwU2i…bcmkn` / `FjDQB0v0geuG4y9BaTVyi_WcZ1jc.bcmkn`(由常量 `PROCEDURE_LIBRARIES` 引用)。

## 2. 症状(实测,`deterministic_ids(true)`,KN → Kitten4 → KN 往返)

| 观察 | 文件 A | 文件 B |
| ---- | ------------ | ----- |
| 实体侧类型差异 | `calculate ⇄ 占位积木`(已知 1:1)+ 横屏包装(成对) | 同上,**外加 `pure_list_get: 186 → 162`(净少 24)** |
| 定义体类型差异 | **6 / 51 条定义有差异,合计净减 133 块** | 0 / 29,净减 0 |
| 最严重的一例 | 某条定义体几乎整体消失(`controls_if 2→0`、`list_append 4→0`、`callnoreturn 6→0`、`temporary_list 8→1`、`parameter 40→2` …) | — |
| 次严重 | `procedures_2_parameter: 60 → 58`(形参块少 2)、`script_variables 2→1 + script_variables_param 5→3 + variables_set 5→4`(脚本变量子树少一截) | — |

**保持成立的**:定义 id 不丢(往返后 `before` 的每个定义 id 都在 `after` 里 ✓)、
重复转换逐字节一致 ✓、告警逐条同序 ✓ —— 也就是说这不是"随机丢",而是**特定形态被改写**。

## 3. 假设(未验证,按可信度排序)

1. **深层嵌套的 `next` 栈编码**:KN 正向编码器对大深度积木树有"顶层积木当作 `next` 栈"的形态
   (bundle 里 `TOP_BLOCKS_AS_NEXT_STACK`、`R=["inputs","statements","next"]`);
   我们反向可能只认其中一种,于是深树被截断 ⇒ 与"整条定义体几乎消失"吻合。
2. **inline `pure_list_get` 影子**:`list_append` 带
   `<shadow type="pure_list_get" inline="true">` 时,反向 `fold_pure_list_get` 与正向云列表特例
   之间没有对齐口径 ⇒ 实体侧净少 24 个 `pure_list_get`(定义体里同样出现)。
3. `script_variables` / `script_variables_param` 子树:可能与 `temporary_list`/`variables_set`
   的互转特例有关(官方在 Kitten 侧用"脚本变量"表达,反向没有完整还原)。

## 4. 后续研究路线(建议,不要直接猜着改)

1. 先把官方 JS 跑起来(**harness 方法已在本仓验证过**,见 `docs/rounds/20` §9):
   从 `creation.bcmcdn.com/neko/web/release/static/js/main-vendors.9b801394.js`
   里取模块 41888 的导出,`nemoBcmToNekoBcmUtils` / `kittenBcmToNekoBcmUtils` 都能直接调;
   对同一份 KN,分别跑官方 `KN→K4`(如果有反向)与我们的实现做**逐字段 diff**。
2. 对"最严重的一例定义体",用官方编辑器实际打开该作品、观察它渲染出的积木数量,
   判断到底是**我们丢**还是**官方也这么丢**(后者则属于已文档化的偏差,只需更新 allow-list)。
3. 修完后把本测试 §2 的预算(6 / 133)收紧到 0,并把 `pure_list_get` 从 allow-list 移除。

## 5. 当前回归防线

`procedure_library_reverses_to_kitten4_and_is_deterministic` 断言:
- 反向成功、场景数守恒、定义根落地、定义 id 不丢、两次转换逐字节一致、告警同序;
- 缺口**只许变小**:受影响定义 ≤ 6、净减块 ≤ 133(`now` 天然满足 0/0)。
