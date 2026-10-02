# 第三十八轮记录 — 转换保真:**id 口径**定案两条悬案 + 「不认识就别删」

> 日期 2026-10-01。承接 `docs/rounds/37` §13 留下的两条悬案,与 `docs/goals/convert-backlog.md` §6(G1/G3)。
> 本轮**动代码**:目标不是"能打开"(已达成),而是"打开后**少没少东西**"。

## 1. 一句话

用 **id 口径(这块的 id 还在不在产物里)** 把两条悬案定案:`lists_get` 大减是**归一化**(槽默认影子),
`get_midis` 类整块消失是**真缺陷**;修法是反向对"反查不到原类型的占位块"**改保留、不再剔除**
(顶替成平台自己的「未收录积木」标记),并给正向扫描器加一条 **id 台账门**(只许变小)。

## 2. 为什么必须换口径(方法)

rounds/37 §13 把"块数"的三种口径踩了个遍,同一件事下了三次相反的结论:

| 口径 | 含义 | 不能拿来判什么 |
| --- | --- | --- |
| (a) 原始 JSON 遍历(census) | 数与连接形态无关 | **不能**说"丢没丢" |
| (b) 从根可达的树计数(`tree.count()`) | 转换器这一趟搬了多少 | 同上(场景那 20 块在 KN 里换了连接形态,计数为 0 但 id 都在) |
| (c) **id 是否出现在产物里** | 块有没有被搬过去 | —— **这才是"丢没丢"的**直接**证据** |

本轮全程用 (c)。做法:`k4_corpus_round_trip_sweep` 自带 `DUMP_FWD=1`,把三腿
(源 Kitten4 / KN 中间态 / 往返产物)落盘到 `/tmp/fwd-{src,kn,back}-*.json`,再逐 id 追。

## 3. 定案

### 3.1 `lists_get`(等价类代表 `pure_list_get`)`768 -> 490` = **归一化**

`原气骑士 且听风吟`(源 12 920 块):丢 278 个 `lists_get` 的 id,**逐个都是 KN 侧 `is_shadow: true` 的
`pure_list_get`**,即"列表槽里那块被折回父块 `fields.list` 的影子"(`fold_pure_list_get`)。
`跑酷_70` 的 `1 -> 0` 同因。=> 与 D2(槽默认影子不回写)一致,**内容与引用零丢失**
(旧的"引用零丢失"结论这次是用 id 口径重证的,不再依赖代理指标)。

### 3.2 `bcm_translator_text_return_value_block: 4 -> 0` = **真缺陷**

`P1拓展任务1音乐顺序_300981590.bcm4`(源 37 块)里的 4 个 `get_midis`:

| 腿 | 结果 |
| --- | --- |
| 源 | `get_midis` ×4(带 `fields.midimusic_id`,挂在 `play_midimusic_till_end` 的 `midimusic` 槽) |
| KN 中间态 | 4 个 id **都在**,类型变成占位块 `bcm_translator_text_return_value_block`(**且 `mutation` 为空**) |
| 往返产物 | 4 个 id **一个不剩**,产物文本里连 `bcm_translator` 都搜不到 |

根因链:
1. `KITTEN_TO_KN` 把 `get_midis` 降级成占位块(`LC` 一族);
2. 但正向的占位文本来自 `RC` 标题表 —— **187 个占位映射里 43 个没有标题**
   (`get_midis`/`get_any_midis`/`get_notes`/`get_whole_midis`/`ai_lab_*`/`auto_player_*`/
   `microbit_*`/`physics2_*`/`on_phone_*`/`rgb_light_*` …)=> 正向**写不出 `mutation`**;
3. 反向 `reverse_placeholder` 只能按 `mutation` 标题反查 => 必然失败 => 按旧行为**保留占位名**;
4. 写出阶段 `strip_unknown_blocks` 见 `bcm_translator_text_*` 不在编辑器注册表里 => **整块剔除**。

=> 这 4 块在"能打开"的意义上没问题,但**内容白丢**;而文档一直把它们记成"已文档化的不可逆项",
掩盖了"其实是被删掉、不是被保成占位"这一点。

## 4. 修法

反向新增 `incompatible_marker`(`mapping.rs`):占位块反查不到原类型时,**顶替成平台自己的**
「未收录积木」标记 —— 语句位 `incompatible_block`、值位 `incompatible_output_block`;
影子保持原名(它的清空是 D2 的槽默认影子,不换块)。

**这两个名字为什么可用**(都是证据,不是推断):

- 在编辑器注册表 349 条里(`kitten4_vocab.rs`);
- 编辑器 bundle 原文(`creation.codemao.cn/kitten/build/kitten.*.js`):
  `t.incompatible_block={type:"incompatible_block",message0:"%{BKY_INCOMPATIBLE_BLOCK}",args0:[],
  previousStatement:!0,nextStatement:!0,…}`、`incompatible_output_block`(同样 `args0:[]`,**且没有 `output`**);
  JS 生成器里 `incompatible_block:function(){throw Error()}` => **它本来就是"不可执行"的占位块**;
- **平台 60 份原件(compile 40 + k4edit 20)里这两个名字 0 次出现** => 平台自己没造过,是本项目第一次用。

标记块 `args0` 为空 => 顺手把该节点的 `fields`/`shadows`/`mutation` 清掉,形态与平台一致。

## 5. 验证(三层)

1. **单测**(`reverse_tests::placeholder_inverts_back_via_mutation_title`):可反查的照旧还原;
   反查不到的 -> `incompatible_block`(语句)/ `incompatible_output_block`(值,且仍 `is_output`);
   影子 -> 保持原名。`cargo test` 全绿(单测 110 passed / 0 failed);另给词表加了一条**有序性断言**
   (`kitten4_vocab::tests::editor_type_list_is_sorted_and_queryable` —— `binary_search` 在无序表上会
   静默答错,而词表是"重导后整体替换"的产物)。
2. **实机**(线上 Kitten4 + 隐藏 `input[accept=".bcm, .bcm4"]`「打开本地作品」,headless Chromium;
   做法同 rounds/35):
   | 文件 | 画布块数 | 渲染 |
| --- | --- | --- |
   | 对照(原文件) | 4 | `当开始被点击 -> 重复执行 -> 移动10步 -> 面向鼠标指针` |
   | 语句位换成 `incompatible_block` | **4** | 该位置变成「**未收录积木**」,**仍留在语句链里**,作品名正常 |
   | 值槽换成 `incompatible_output_block` | **5** | 该槽显示为空(0),标记块**以孤立块保留**(平台自己的块定义没有 `output` 连接) |
   | **真实修后产物**(`P1拓展任务1` 的 KN->Kitten4 产物) | 音符角色 **19** | 其中 **4 块是「未收录积木」** —— 修前这 4 块整块消失;作品名正常、不触发"整份加载失败" |
   => 不引入"整份工作区加载失败",且丢掉的块**看得见**了。
3. **id 口径**:修前那 4 个 id 在产物文本里 `False`;**修后 4/4 在,类型 = `incompatible_output_block`**。
   `convert_bench` 四样本 SHA256 + `#meta` 与基线**完全一致** => 本轮改动对基准样本无字节影响。

## 6. 新增的门:`[id台账]` + `LOST_ID_BUDGET`

`k4_corpus_round_trip_sweep` 现在逐件打印 `[id台账] <文件>: 真块丢 N / 影子丢 M`,并与
`LOST_ID_BUDGET`(只许变小)比对。**为什么是预算而不是"绝对 0"** —— 实测 59 件里 33 件非零,
构成逐类查过:

| 侧 | 机制 | 说明 |
| --- | --- | --- |
| 真块 | **横屏 `GC` 算术壳给被包住的节点重铸 id** | `wrap_arithmetic` 照官方"给原节点重铸 id 并挂到包装块"(mapping.rs:~493)=> 内容搬进 `A` 槽、id 换新。`原气骑士` 408 个真块丢失里约 342 个是它 |
| 真块 | 类型级派生(`get_3`+属性、`appearance_of_sprite` 之类) | 同属"换了编号",内容在 |
| 影子 | D2(槽默认影子不回写)+ 影子重铸 | 只留 `fields`,影子块不再写出 |

=> 这些是"编号变了"而不是"块没了"。但**真正丢块**那一类会被拦住:本轮修的缺陷在表上就是 `+4`。

## 7bis. 把「标记」推广到**所有**编辑器不认识的块(同日,用户拍板)

§1–§7 只在**占位积木**这条路上改成标记;D1 的 Neko 专有块族(`temporary_list` / `script_variables*` /
`traverse_number*` …)当时仍是**直接删掉**。用户决定统一,于是把这件事从映射层搬到**写出阶段**。

### 改动

| 位置 | 改动 |
| --- | --- |
| 写出器 `assembly.rs` | `strip_unknown_blocks` -> **`mark_unknown_blocks`**:不认识的块**就地换类型**为 `incompatible_block`(语句位)/ `incompatible_output_block`(值位 —— 判据是 `connections` 里 `input_type == "value"`,或块自己的 `is_output`),清掉 `fields`/`shadows`/`mutation`,**位置与连接保持** => 原来那段"重建 `connections` 去掉悬空引用"整段删掉(不再有悬空) |
| 映射层 `mapping.rs` | 反向兜底回到"保留占位名",交给写出阶段(单一机制,不再两处各做一遍);**影子**仍是清空(影子是槽位默认值,换成标记块没有意义) |
| 正向 `mapping.rs` | 新增**手工映射**(生成表不受影响)`incompatible_block -> bcm_translator_text_execution_block`、`incompatible_output_block -> bcm_translator_text_return_value_block`。否则把产物再正向一次,KN 文档里会留下 **KittenN 编辑器不认识**的 `incompatible_*`;映射过去后往返稳定(标记 -> 占位块无标题 -> 反向反查失败 -> 又是标记) |

### 顺带修掉一个**漏清**(重要)

老实现里 `if dropped.is_empty() { return }` 被当成"省一次遍历"的性能优化(rounds/37 P3);
它其实是**按实体**提前返回,顺带**跳过影子扫描** => "没有任何未知块的实体"里的未知影子
**从来没被清过,直接留在产物里** —— 而未知影子正是会让编辑器整份工作区加载失败的东西。
去掉早退后:全语料里有**一件作品**的影子清空量 52 -> **62**(这 10 条是补上的漏清,不是回归;
块数一件没变)。

### 读数与门

- 报告文案:`…(Kitten4 编辑器不认识,已改成未收录积木 N 块)`;影子仍是 `…已清空 N 条影子`;
- `STRIP_BUDGET` -> `MARKER_BUDGET`(同一批块、同一批数值:604 / 429 / 182 / 8 / 4 / 1 …,
  只有上面那一件的影子列 52 -> 62);`kn_corpus_round_trip_sweep` 的门改成"跑完统一报",
  一次列出所有超 baseline 的项(不再"第一件就 panic")。

### 验证

- 单测:111 passed / 0 failed(新增 `assembly_tests::unknown_blocks_become_incompatible_markers`:
  语句位 -> 语句标记、值槽子块 -> 值标记、字段/影子/变异清空、位置与连接保持、逐类计数);
- 全语料:标记块数与原来的"剔除块数"**逐一相同** => 同一批块,只是不再删;
- `convert_bench`:正向两个样本 SHA256 + `#meta` **完全没动**;反向两个样本**按预期变化** ——
  `kn-9.4MB` 产物 9 780 218 -> **9 775 778** B(块变小了)、告警 1817 -> **1827**(+10 条影子清空);
  `kn-3.7MB` 3 963 260 -> **4 117 977** B(182 个标记块及其连接留下来)、告警不变。
  基线已按流程重刷(`BACKEND_BENCH_REFRESH=1`,提交信息里写明原因);
- **实机**(线上 Kitten4 + 「打开本地作品」,产物 = `HEX Editor_317683843` 的 KN->Kitten4 结果,
  含 **182** 个标记块):作品正常打开(项目名 `HEX Editor`)、**6 个角色都在角色列表里**、
  画布把标记块渲染成「未收录积木」;逐角色精确核对:角色 `raw` 文件里 4 个标记、画布上就是 **4** 个
  (数法:画布 `text` 里**恰好等于**「未收录积木」的标签数 —— 按 `g.blocklyDraggable` 数会把嵌套子块
  重复计入,先前的 9/8 就是这么来的)。

## 8. 残留与下一轮

- **值型标记是孤立块**:平台自己的 `incompatible_output_block` 没有 `output` 连接 => 槽位仍空,
  只是块还在(位置 + 存在保住,可被用户看见与手工删除)。要真"填回槽里"得平台改块定义。
- **被标记的类型全部不可逆**:信息(原类型与字段)确实丢了 —— 现在至少**看得见**。
  要真恢复得给正向补 `RC` 标题,但**没有可信来源**(bundle 里那批中文串是"说明文案"不是 `RC` 标题)=> **不做**。
- **id 改铸可以再压**:把 `wrap_arithmetic` 从"移动 + 重铸"改成"移动不重铸",往返 id 即稳定,
  台账能压到近 0 —— 动的是照官方的那一行,**要单独立项 + 实机/基准验证**。
- **G3(差异类别门)未做**:`[id台账]` 只覆盖"块没了"、`MARKER_BUDGET` 只覆盖"认不出多少块";
  两台扫描器的**差异类别集合**仍未冻结。
- 反向侧没有 id 台账(正向那条已建);反向"认不出"的量由 `MARKER_BUDGET` 守。

> **方法教训(第四次同类)**:跨口径比较前先声明口径;**判"丢没丢"只用 id 口径**;
> 而"id 口径"本身也要分清 **节点 id / XML 里的 id / 只是 `connections` 键上出现** ——
> 本轮就靠这个区分把"影子重铸"和"整块被删"分了开。

## 依据

- `docs/rounds/37` §13(三条悬案与三次改口)、`docs/knowledge/convert-semantics.md` §5bis(隐性契约,本轮加第 5 条)、
  §6(硬门表,本轮加"id 口径台账")。
- 代码锚点:`src/core/convert/translate/assembly.rs`(`mark_unknown_blocks`,§7bis)、
  `mapping.rs`(`reverse_placeholder` / `KITTEN_TO_KN_MANUAL`)、`reverse_tests.rs`
  (`collect_ids` / `block_node_ids` / `LOST_ID_BUDGET` / `MARKER_BUDGET` / 两台扫描器)。
- 仪器:`DUMP_FWD=1` 三腿落盘;实机门(无头 Chromium + 线上 Kitten4);`convert_bench`(SHA + `#meta`)。
