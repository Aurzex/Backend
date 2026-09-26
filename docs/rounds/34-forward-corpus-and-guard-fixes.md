# 第三十四轮记录 — 正向方向首次吃到真语料;两个判据误伤;定义体缺口收口

## 1. 修掉两个"判据误伤"(产品侧)

| 症状 | 根因 | 处置 |
| --- | --- | --- |
| `捕鱼达人_259694808.bcm4`(Kitten4)被拒:"源作品没有 size 字段,这看起来是 Kitten2/3" | `convert_kitten4_document` 强要求顶层 `size`,而该作品只有顶层 `width`/`height` | 改用与装配**同一套回退**(`assembly::source_stage_size`:`size.*` → `width`/`height` → 562×900),`source_stage_size` 提为 `pub(super)` 复用 |
| 放松判据后 `春风得意_324995084.bcm`(Kitten3)**静默**当空作品转换(源积木=0) | 画布字段不能区分 Kitten3/Kitten4(Kitten3 也有 `width`/`height`) | 判据换成"实体里有没有 `block_data_json`",没有就报明确错误 |
| `A28社区-开幕_174408420.bcm4` 报 `invalid type: map, expected a string` | 平台上有作品的 `shadows` 是**内联对象**(实测 3653 处),本库只吃 XML 字符串形态 | 前置拦截,报"内联影子是对象形态(块 `X` 的槽 `Y`),暂不支持该作品" |

> "支持对象形态影子"(把对象转成影子 XML)列入待办 —— 需要复用 `ShadowBuilder` 那套。

## 2. 正向扫描器换上真语料(离线,21 件)

`download/compile/*.bcm4` 就是"反编译一个 Kitten4 作品"的产物 —— **正是用户会走的路径**。
实测 **21/22 件正向吃得下**(Kitten3 一件、对象影子一件按形态跳过并说明原因)。

**口径**:早期差异里绝大部分是**有意的改名**(歧义类型保留 KN 名 + 告警,见 `mod.rs` 顶部),
而这些对在表里**互为表项**(`text ⇄ get_split_options`),只映射一次会把两侧推向相反方向。
所以两侧类型都折到**等价类代表**(`canonical_kind`:传递闭包 + 取类内字典序最小)再比 ——
语义改名抵消,剩下的才是真差异(`下100层`、`障碍爬竿` 从"一堆差异"变成**零差异**)。
同时只收"像类型名"的 `type` 值,避免把影子 XML 串当积木计数。

## 3. 反向侧最后一项:定义体缺口 = 残块归一化(收口)

`def_census` 从"整条条目"改为"**定义根子树**"(`accumulate_subtree`),保留"首根不是定义块即跳过整条"的旧语义
(改它会因镜像侧 id 重铸让"定义 id 不丢"假红)。结果:三件真作品语料的定义体侧差异**归零**,
定义体侧两条豁免删除,预算按 rounds/28 §4.3 从 **`≤6/≤21`** 收紧到 **`0/0`**。
成因与证据见 rounds/33 §3bis(那些块是没人挂的残块,反向重建树时自然消失)。

## 4. 正向侧待分诊(11/21 件有差异,逐条已打印)

| # | 形态 | 例子 | 判断 |
| --- | --- | --- | --- |
| 1 | `input: N→M` 与 `next: M→N` **此消彼长**,几乎每件都有 | `1711-2`:`input 1208→1143`、`next 196→261` | 占位块(`input`/`next`)的**表述差异**,待定性 |
| 2 | `lists_get: 201 → 101` | `Plactions` 201→101;`原气骑士` 768→490 | 有块换了**语义类**(不是改名)⇒ 风险最高 |
| 3 | `get_current_scene: 20 → 186` 且 `get_screens: 166 → 0` | `原气骑士` | 疑似**大范围语义替换**⇒ 风险最高 |
| 4 | `stop ⇄ terminate`、`below: 0→12`、`x/y/width/height` 小增量、`shadow_number: 2→0` | 多件 | 疑似坐标系/占位表达 |
| 5 | `procedures_2_defnoreturn` 翻倍 + `parameter`/`return_value`/`stable_parameter` 增加 | `射箭-1`、`烂` | **已文档化**:正向 `zC` 拆 `NORMAL`+`ROUND`(rounds/32 §3.2)⇒ 预期 |

## 4bis. 正向侧最值得追的一条:列表影子的「分腿丢」(三态计数,2026-09-26)

对 `原气骑士 且听风吟_136021231.bcm4`(最严重的一件)落盘**源 / 中间(KN)/ 往返(Kitten4)**三态并计数
(只数真块,不看 `shadows` XML):

| 类型 | 源(Kitten4) | 中间(KN) | 往返(Kitten4) |
| --- | --- | --- | --- |
| `lists_get` | 768 | 0 | **490** |
| `pure_list_get` | 0 | **682** | 0 |
| `lists_get_value` / `list_item` | 505 | 480 | 505 |
| `lists_replace` / `replace_list_item` | 266 | 205 | 266 |
| `lists_append` / `list_append` | 6 | 6 | 6 |
| `input` | 10186 | 0 | 9853 |
| `next` | 2126 | 0 | 2179 |
| 块总数 | 25170 | 14485 | 24625 |

读法:

- **正向腿**:`lists_get`(768)→ `pure_list_get`(**682**)⇒ **丢 86**;同一条腿上 `lists_get_value` 505 → `list_item` 480(−25)、
  `lists_replace` 266 → `replace_list_item` 205(−61);
- **反向腿**:`pure_list_get`(682)→ `lists_get`(**490**)⇒ **再丢 192**;而 `list_item`/`replace_list_item`
  反向**原样保留**(不回改成源里的名字 —— 但同义,已被扫描器的等价类口径抵消);
- ⇒ **两条腿都在丢**(86 + 192 = 278),不是单侧问题。

**关键线索**:源(Kitten4)里的 `pure_list_get` 影子是**没有 `fields.list` 的裸影子** ——
Kitten4 侧的列表引用挂在**父块的 `fields.VAR`** 上(见 `FIELD_NAME_MAP` 的 `("VAR","list")`),
而正向的列表影子步骤读的是 `fields.list` ⇒ **这些"已经存在的影子"既不会被识别、也没被当作普通子块保住**。

**下一条(明确)**:比较"影子作为子块"与"影子作为列表槽输入"两种形态在父块槽位里的差别 ——
即正向的列表影子步骤该按**Kitten 侧字段名**(`get_mapped_name` 反查)取字段,而不是硬读 `fields.list`。

另外 `input`(10186 → 9853,**−333**)与 `next`(2126 → 2179,+53)几乎每件作品都有,是**占位块**的表述差异,待定性。

## 5. 环境:clippy 门恢复干净

`cargo clippy --all-targets -- -D warnings`(CONTRIBUTING 要求)此前被 **7 条工具链新 lint** 挡着,
且都在本轮改动范围之外的文件里 —— 一并修掉,门恢复可用:`nemo_mapping` 文档空行 ×2、
`shared` 文档列表缩进(`+` 开头被当成列表项)、`nemo_tests` 的 `&String`→`&str` 与 `filter().next_back()`、
`api/auth` 测试模块后置项、`reverse_tests` 我新写的文档注释(降级为普通注释)。

## 6. 下一步

从 §4 的第 2、3 条入手 —— 它们是**改语义**而不是改名字,风险最高:
`lists_get` 与 `get_current_scene`/`get_screens` 的成对替换说明某张反向表把积木映射到了**功能不同**的块。
