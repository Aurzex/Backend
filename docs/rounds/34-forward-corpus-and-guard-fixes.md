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

## 4ter. **更正 §4bis 的结论**:那些"丢"绝大多数是口径/归一化,没有确认未修的缺陷

§4bis 用"`pure_list_get` 节点数"当代理指标,得出"腿 a 丢 86、腿 b 丢 192"。逐实例复核后**必须更正**:

1. **正向会重铸 id** ⇒ 我早先"中间态里找不到这个父块"是假象(块在,id 变了);
2. 抽两个真实父块核对:**完全正常** —— 源里那个 `lists_get` 影子**子块**被改名成 `pure_list_get`
   并留在 `list` 槽 ✓(`VAR → list` 槽名也确实被映射了 ✓);
3. 剩下"没有 `list` 节点"的父块,逐类拆开看:
   - **49 个**是 `inputs.list` 被**计算型积木**占着(实测 `text_split` = "拆分文本成列表")⇒ 本来就不该有影子节点 ✓
     (`shadows.list` 只是槽的**默认影子** ✓);
   - **7 个**是**退化影子**(`<field name="VAR">?</field>`,列表名就是 `?`)⇒ 已定性为归一化(rounds/33 §3.5)✓;
   - 其余是块总数口径差 ×我的匹配方式错 ✗。

**结论**:正向侧**没有确认未修的缺陷**。残留差异逐条归入**已文档化族**(带测试或带注释):

| 残留 | 性质 |
| --- | --- |
| `input ⇄ next` 此消彼长(几乎每件) | 占位块的**表述差异**,唯一还没写进文档的一条 ⇒ 本轮已记 |
| `stop → terminate`、`shadow_number → …` | 反向的 `GC` 特例 / 官方拆包降级(`mapping.rs:1304`、`gc_node`,均有专门断言) |
| `x/y/width/height/below/start` 小幅增减 | `GC` 横屏坐标包装(已文档化) |
| `controls_if`/`logic_compare`/`default_value` 小幅 +n | 同一包装/拆分的连带 |
| `procedures_2_defnoreturn` 翻倍 | 正向 `zC` 拆 `NORMAL`+`ROUND`(已文档化) |
| `lists_get` 类减少 | 见上:计算型列表槽 + 退化影子 |

> **教训(写进方法)**:扫描器打印的差异行很长,我用 `cut -c1-230` 截断看输出,结果把"替代品那一半"
> 截掉了 ⇒ 差点把归一化读成丢失。**看差异要看整行**;代理指标(节点数)必须用实例复核才能下结论。

## 4quater. 口径又抓到一层噪声:`connections` 里的"连接描述符"被当成积木

给扫描器加"只数**带 `id`** 的真块"之后,`input ⇄ next` 这族**几乎每件作品都有**的差异**整族消失**,
有差异的作品从 **19/33 降到 12/33**。

根因:`block_data_json.connections` 里存的是
`{"input_name":"message","input_type":"value","type":"input"}` 这类**连接描述符**(描述槽位连到哪),
不是积木 —— 我的计数把其中的 `type` 当成了块类型(实测某件作品虚增 1 万多个"input 块")。

修正后的残留(12/33),逐条定性:

| 残留 | 件数 | 性质 |
| --- | --- | --- |
| `stop → terminate`、`shadow_number → …` | 6 | 反向 `GC` 特例 / 官方拆包降级(`mapping.rs:1304`、`gc_node`,均有专门断言) |
| `controls_if`/`logic_compare`/`logic_operation`/`default_value` 小幅 +n | 3 | `GC` 横屏坐标包装的连带 |
| `procedures_2_*` 翻倍 | 1 | 正向 `zC` 拆 `NORMAL`+`ROUND`(已文档化) |
| `lists_get: N → M` | 4 | **待最后一查**(计算型列表槽 + 退化影子已解释一部分) |
| `change_cloud_variable`/`cloud_lists_*` 小幅 +n | 1 | **待查**:怀疑反向在"云/本地"歧义里选了云 —— 代码注释写的是"非云优先" |

> 最后两行是下一段唯一还没收口的东西;其余都是已文档化族(带测试或带注释)。

## 4quinquies. `lists_get` 那 4 件收口:**列表引用一个没丢**,少的是「槽的默认影子」

用两条**与 id 重铸无关**的证据定案:

1. **列表 id 集合**:源 / 中间(KN)/ 往返三态各引用 **24 / 24 / 24** 个不同的列表 id,
   交集也都是全部(源∩中间 = 源∩往返 = 24)⇒ **没有任何列表引用丢失**;
2. **接线**从 `block_data_json.connections` 看得很清楚,格式是
   `connections[父块 id] = { 子块 id: {"input_name": "VAR", "input_type": "value"} }`:
   源里 768 个 `lists_get` 影子**全部**挂在某个槽上(未接线 **0** 个)⇒ 不是残块。

⇒ 那 768 → 682 → 490 少的是什么?结合前面那 49 个「`inputs.list` 被计算型积木(`text_split`)占着」的父块:
**源在同一个槽里同时保留「槽的默认影子」和「真正接上的子块」**(Blockly 的 fallback 写法,影子带 `is_shadow`),
我们的产物只保留真正接上的子块,不再回写那层默认影子。

**定性**:表示差异(编辑器里那个槽会显示空白,而不是默认影子),**不影响功能、不丢任何引用**;
量级 ~86(正向)+ ~192(反向)。若要完全对齐,需要在「槽被真块占着」时也回写该槽的默认影子 —— 列为**可选打磨项**。

## 4sexies. 最后一条"云/本地"也收口:名字合并 ≠ 语义丢失

三态**原始**类型名计数(`原气骑士 且听风吟_136021231`,只列有变化的):

| 名字 | 源(Kitten4) | 中间(KN) | 往返(Kitten4) |
| --- | --- | --- | --- |
| `change_variable`(本地) | 89 | 0 | 0 |
| `change_variables` | 0 | 86 | 89 |
| `cloud_variables_get`(云) | 11 | 0 | 0 |
| `variables_get` | 800 | 806 | 811 |
| `cloud_variables_set`(云) | 7 | 0 | 0 |
| `variables_set` | 294 | 265 | 301 |
| `stop` → `terminate` | 10 | 9 | 0 / 10 |

结论:

1. **不是"把云当成本地"**:KN 侧本来就**只有一种**变量积木(`cloud_variables_get` 在表里也映射到
   `variables_get`)—— **云 / 本地的区别在变量 id 与变量定义表里,不在积木类型名上** ⇒ 名字合并
   **不丢语义**;反向"歧义时保留 KN 名 + `AmbiguousType` 告警"是**既定策略**(`mod.rs` 顶部有说明),
   而且它其实**不选云**:真正会改名的歧义才按"非云优先"挑(`mapping.rs` 的 `reverse_kind` (f)),
   恒等歧义直接保留原 KN 名。
2. `stop → terminate` 是反向 `GC` 特例(`mapping.rs`:`fields.scope="0"` ⇒ `terminate`,有专门断言);
   `shadow_number` 被 `gc_node` 按官方行为拆包/降级(有专门断言)。
3. `lists_get` 减少是"槽的默认影子不回写"(§4quinquies)。

⇒ **正向侧已全部解释完毕:没有确认未修的缺陷。** 剩下的都是文档化的归一化 / 官方行为 / 可选打磨项。

## 4septies. 手工转换两件线上作品(工具 + 数字口径的两次踩坑)

新增工具 `tests/convert_work.rs`(`WORK_ID=<id> cargo test --test convert_work -- --ignored`):
按作品类型自动选方向(`NEKO → Kitten4`、`KITTEN4 → KittenN`),调 `translate_work`
(抓取 → 反编译到编辑版 → 重排 → 落盘),产物写 `download/converted/`(**不塞进** `download/compile/`
——那里是往返扫描器的语料,混进手工产物会破坏"每件都是真作品"的前提)。**只转换、不上传**。

实测:

| 作品 | 类型 | 产物 | 体积 / 实体 | 告警(分类) |
| --- | --- | --- | --- | --- |
| `now`(273988379) | NEKO | `now_273988379.kitten4.bcm4` | 6.56 MB / 110 | 3977:KN 顶层键 Kitten4 没有、形参类型 `Custom`/`Audio`、`stop.scope=2/3` |
| `Node VM v3 - 全猫最强解释器`(328981781) | NEKO | `…_328981781.kitten4.bcm4` | 5.38 MB / 4 | 2494:同上(含大量 `param(type=List)` —— Kitten4 没有 list 形参) |

两件都做了**回程自检**(把产物再转回 KN)⇒ 都能转,且 `Node VM v3` 的回程里
`proceduresDict` 从 **52 条**(NORMAL 10 / ROUND 39 / HEXAGONAL 3)变成 **93 条**(NORMAL 52 / ROUND 41)
—— 这就是**官方 `zC` 把带返回值的定义拆成 NORMAL + ROUND 两条**的行为(rounds/32 §3.2 已文档化,
`def_census` 会把两条合并回同一定义再比),所以回程块数看着多一截是**定义体被写了两遍**,不是丢或坏。

### 数字口径的两次踩坑(都记下来)

1. **别用 `parent_id` 是否存在当"是不是块"的判据**:官方/反编译产物里 `next` 子节点**不带** `parent_id`
   (库的 `diff_tests` 注释里就写着"官方会丢 `is_output`/`field_constraints`,且 `next` 子节点不带 `parent_id`,
   我们刻意保留"),拿它计数会把我们的产物虚增一大截;
2. **`params[]` 不是块**:`proceduresDict[..].params[] = {"id","name","type"}` 里 `type` 是**形参类型名**
   (`List`/`String`/`Audio`/`Custom`/`Label`),计数时会把它们当成积木类型(实测某作品 182 个)。
   ⇒ 计数只认**真块**,并优先用库自己的 tree census 做判据(它天然不带这两类噪声)。

## 5. 环境:clippy 门恢复干净

`cargo clippy --all-targets -- -D warnings`(CONTRIBUTING 要求)此前被 **7 条工具链新 lint** 挡着,
且都在本轮改动范围之外的文件里 —— 一并修掉,门恢复可用:`nemo_mapping` 文档空行 ×2、
`shared` 文档列表缩进(`+` 开头被当成列表项)、`nemo_tests` 的 `&String`→`&str` 与 `filter().next_back()`、
`api/auth` 测试模块后置项、`reverse_tests` 我新写的文档注释(降级为普通注释)。

## 6. 下一步

从 §4 的第 2、3 条入手 —— 它们是**改语义**而不是改名字,风险最高:
`lists_get` 与 `get_current_scene`/`get_screens` 的成对替换说明某张反向表把积木映射到了**功能不同**的块。
