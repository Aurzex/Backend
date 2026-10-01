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
- **老形态的"内联对象影子"**(`shadows: {槽: {type, id, visible, editable, fields}}`)也吃:
  正向入口把它就地改写成**平台同款影子 XML**再解析(`pipeline::normalize_object_shadows`)。
  全语料只有一件(`A28社区-开幕_174408420`,第三十三轮曾因此被拒收),形态依据是**同一件作品的
  平台原件**(`download/compile/k4edit/174408420-*.bcm4`):字段名取对象的 `fields.name`、字段文本取
  `fields.text`、其余字段键(`constraints`/`allow_text`/`has_been_edited`…)当**字段元素的属性**;
  `editable=false` 的占位影子写成 `<empty … editable="false">`(平台就是这么写的)。对象里没有的
  渲染属性(`inline`/`deletable`)会丢 —— 对象形态本身不携带它们。
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
- **保真缺口(当前口径,2026-10-01 起)**:见 §5bis —— 反向的可见损失来自"**写出的名字必须是编辑器注册表里的**"
  这条硬约束:不认识的**块**就地改成「未收录积木」标记(`rounds/38` §7bis 起;此前是"整块剔除"),
  **影子**清空;`def_census` 口径修正(只数定义根子树)后,定义体侧的"残块假缺口"已归零(rounds/33 §3bis),
  现行门是**预算断言 `≤3193`**(rounds/36,只许变小;每次跑都会打印读数)。历史研究路线见 `docs/rounds/28`、`docs/rounds/32`。
- **整作品实测(2026-09-26,真作品 `325806995` = `now但是1080P`,KN → Kitten4)**:
  告警 3 973 条按类别 = `未映射积木 643 · 丢弃实体属性 3 301 · 丢弃字段 12 · 重铸 id 17`
  (由 `tests/convert_live::kn_work_to_kitten4_file` 按类别打印)。
    - ⚠️ **口径已在 rounds/34 变更**:早期结论是"未映射/不认识的类型**保留 KN 原名 + 告警、不丢积木**" ——
    **这会让编辑器整份加载失败**(实测:80 种类型里 20 种 Kitten4 不认识 ⇒ 画布一块都不显示,rounds/34 §4nonies)。
    现行行为:写出阶段把编辑器不认识的**块**就地改成「未收录积木」标记(§5bis 第 5 条)、
    **清空**这类**影子**,逐类型计入报告(`rounds/38` 起;此前是"整块剔除",那时积木会真的消失)。
  - 因此"未映射积木"这个类别现在的含义是**内容恢复不出来**(但块本身还在画布上、看得见):
    两个真作品实测(rounds/36,A/B 对照)——`Node VM v3`(328981781)曾剔块 527 → **429**、
    清影子 174 → **4**;`now`(273988379)曾剔块 415 → **286**、清影子 224 → **46**
    (这两组数是 rounds/36 当时的口径);`get_split_options`(Kitten3 口径名)的块与影子已**全部消失**
    (改回编辑器认识的 `text`)。
  - 会走到"标记"这一步的是**Kitten4 没有对应概念**的 Neko 专有块族:`temporary_list`、
    `script_variables*`、`traverse_number*`、`self_listen*`/`self_broadcast_with_param`、
    `procedure_boolean`、`self_text_effect_color`、`color_size_slider`,以及反向反查不到原类型的
    **文本占位积木**(证据:平台 40 件 Kitten4 语料里这些名字**0 次出现**,编辑器注册表 349 条里也没有)。
    要继续减损只能做"语义降级"(改语义,见 `goals/pending-decisions.md` D1)。
- 运行时实测:KN→Kitten4 产物有 `theatre`/`size`/`block_data_json`,可被平台接受。

## 5bis. Kitten4 **编辑器的隐性契约**(实机验证得出,改写出器前必读)

这一节是 rounds/34–36 的产物:**积木数对得上 ≠ 编辑器能打开**。四条,每条都有实机证据。

1. **顶层平台骨架键必须齐**(15 个):`toolbox` / `toolbox_order` / `last_toolbox_order` / `ai_lab` / `matrix` /
   `models` / `midi_order` / `midimusic` / `is_partial` / `sample_id` / `codemao_value` / `work_source_label` /
   `device_widget_type` / `hardware_type` / `hidden_toolbox`(反编译产物只差 `painter`,同档即"能读")。
   缺了 ⇒ 编辑器读不出积木(rounds/34 §4octies)。
2. **`theatre.groups` + 场景 `group_order` 必须自洽非空**:平台用"每组一个角色、组上带 `scene` 归属"表达
   "谁在场景里";两张表都空 ⇒ **角色一个都不列、画布 0 块**(积木一块没少也照样黑屏)。KN 侧没有分组概念,
   反向必须**合成**(rounds/35)。
3. **所有积木类型名(含影子 XML 的 `type`)**必须是编辑器**注册表里有的名字** —— 不认识的名字会让
   **整份工作区加载失败**。注册表 = 线上编辑器 `Object.keys(window.Blockly.Blocks)`(349 条,导出快照在
   `src/core/convert/translate/kitten4_vocab.rs`)⇒ 写出阶段必须落成编辑器认识的形态(不认识的就地改成「未收录积木」标记、
   影子清空)+ 逐类报告(rounds/34 §4nonies、36、38 §7bis)。
4. **表(`REVERSE_TYPES`/`KITTEN_TO_KN`)是 Kitten3 口径** ⇒ 凡是要往 Kitten4 写名字的地方(块、影子,
   以及将来任何新写出点)**都要过词汇判据**:候选认识取候选;否则 KN 名认识就保留 KN 名;否则交给写出阶段落成标记并报告。
   反例(已修):KN `text` 的候选只有 `get_split_options`,而平台 40 件 Kitten4 语料里 `get_split_options`
   出现 **0** 次、`text` **1338** 次(rounds/36)。
5. **"编辑器不认识"的块:不能原样写出去、也不能删掉,要就地改成「未收录积木」标记**(rounds/38):
   - 原样写出去 ⇒ 编辑器**整份工作区加载失败**(第 3 条);
   - 直接删掉 ⇒ 积木**真的消失**(id 口径实测:某件作品丢了 4 个可达块;而且 187 个占位映射里
     **43 个**没有 `RC` 标题,正向写不出 mutation ⇒ 反向必然走到这一步);
   - 现行:写出阶段 `assembly::mark_unknown_blocks` 把类型**就地换成**平台自己的
     `incompatible_block`(语句位)/ `incompatible_output_block`(值位 —— 判据是 `connections` 里
     `input_type == "value"`,或块自己的 `is_output`),并清掉字段/影子/变异;**位置与连接保持**。
     实机(线上 Kitten4 + 「打开本地作品」):语句型**原地**渲染成「未收录积木」并留在语句链里;
     值型因平台自己的块定义**没有 `output` 连接**而落成**孤立块**(槽位空,但块还在画布上)。
   - **影子是例外**:不换标记块,仍是**清空** —— 影子是槽位的默认值,换成标记块没有意义(没有字段
     可表达默认值)。见 `rounds/34` §4quinquies 的 D2。

**判据与证据来源**(都可复用):

| 判据 | 怎么做 |
| ---- | ------ |
| 名字/字段是否合法 | 对着**平台真实文件**(`download/compile/*.bcm4`)做**归一化 schema 差集**(id 段折掉、块字段聚合),看"平台有、我们没有"的字段路径 |
| 名字该叫什么 | **平台语料计数**(某名字在 40 件真作品里出现几次)比"表里怎么写的"更权威 |
| 产物能不能用 | **无头 Chromium + 线上编辑器的「打开本地作品」**(隐藏 `input[accept=".bcm, .bcm4"]`)⇒ 数画布积木 + 看角色列表;**必须带一个已知能读的对照组**(见 `docs/rounds/35` 的方法留档) |

## 5ter. 已知的**文档化偏差**(改代码前先看,别重复怀疑)

| 偏差 | 说明 | 处置 |
| ---- | ---- | ---- |
| `keep_source` 上传的是**重建版**而非原始字节 | 原始 `.bcm*` 字节在反编译阶段就被解开,本库手上只有重建的编辑版 ⇒ 平台侧"保留原件"打开的是重建版(可读、可再转换,语义不受影响)。见 `docs/rounds/21` §8.4 N2 | **文档化**(rustdoc 已写明),不改反编译侧 |
| KN 侧"不认识"的类型在 Kitten4 产物里**被改成「未收录积木」标记** | 不是偷懒:Kitten4 编辑器不认识的名字会让**整份工作区加载失败**(§5bis 第 3 条);而**删掉**会让积木真的消失(rounds/38 的 id 口径定案) | 已按预算记账(`MARKER_BUDGET`,只许变小)+ 逐类打印报告 |
| Kitten4 没有 **list 类型的程序集参数** | `param(type=List)` 必丢;属格式能力差 | 已定性(rounds/32) |

## 6. 硬门与不变量(实现任何新方向都必须满足)

| 门 | 内容 |
| -- | ---- |
| 官方校验器 | `BcmHelpers.validateBcm`(bundle module 87123)**可 headless 运行** —— "产物能否被编辑器加载"的硬门 |
| 语义 diff | 与官方产物比较**忽略 id/location/uuid**(`docs/rounds/20` §9);官方产物按键插入序,**从不逐字节对齐官方** |
| 确定性 | `IdSource` + `TranslateOptions::deterministic_ids`;并发 1 与并发 N 产物 **SHA256 相同** |
| **字节基线** | `convert_bench` 的产物 SHA256 + `#meta`(源 SHA / 字节 / 块数 / 告警数 / `source_version`):**6 样本 = 4 Kitten + 2 NEMO**(NEMO 一件含 YC 版本迁移);产物字节或报告退化(块变少、告警变多)都必须先解释再接受 |
| 往返守恒 | KN→Kitten4→KN 的积木类型**多重集**一致(差异仅白名单降级项 + 预算断言) |
| 有损记账 | 一切有损进 `TranslateReport`;官方重传资源不算有损(`ReuploadedOnImport`) |
| **编辑器能否打开** | 实机硬门:无头 Chromium + 线上 Kitten4 的「打开本地作品」,数画布积木并看角色列表(带对照组) |
| **标记量预算** | 编辑器不认识的块被改成「未收录积木」的数量、以及被清空的影子数,必须 ≤ 记录值(只许变小,`MARKER_BUDGET`);定义体缺口预算见 §5bis / rounds/36 |
| **id 口径台账** | 正向扫描器(`k4_corpus_round_trip_sweep`):源里"带类型的块节点 id"在往返产物里缺失的 **真块 / 影子** 数必须 ≤ 记录基线(`LOST_ID_BUDGET`,只许变小;每次打印 `[id台账]`)。**反向同口径**:反向扫描器(`kn_corpus_round_trip_sweep`)的 `LOST_ID_BUDGET_REVERSE`(逐件打印 `[id台账·反向]`;源侧只认积木容器节点,见 rounds/39 §W3d)。这是"块有没有被搬过去"的**直接**证据 —— 积木计数/告警/树可达都不是(rounds/37 §13 三次翻车) |

## 7. 判定"不做"的两项(有证据,别再重开)

| 项 | 判定 | 证据 |
| -- | ---- | ---- |
| `RawValue` 顶层只透传 | **不做** | 装配期"只透传不改"的顶层字段占比 **≈0%**:`theatre`(含 `block_data_json`)占文档 99.2–99.5% 且全部重写;`styles`/`audios`/`variables`/`broadcasts` 都要变换 ⇒ 收益上限 ≈0%,远低于 30% 门槛 |
| 正/反向遍历合并成单遍 | **不做** | 正向实为 2 趟(`parse_node` 内含 `route_children` 再 `gc_deep`),反向本已单遍;合并只省遍历,不省逐块 `kind` 匹配/`transform_shadow_xml`/`map_field_name` 这些主要成本,且 `gc_node` 依赖子树已 parse |

## 8. 文本层(低成本高价值的调试通道)

- `knBcmToText` / `textToBlock`:`.bcmkn` ⇄ 中文积木 Markdown(`# 场景 / ## 角色 / ### 属性 / ### 代码`)。
- 实测 3.7 MB 作品 → 4.86M 字符,1.4 s;**全树积木 359 → 359,55 种类型多重集完全一致**(差异仅元数据/XML 归一化/新 UUID)。
- 大文件要有大小上限;文本层不是产物的必需环节,是**人工校验**手段。

## 9. **编辑格式**在平台上的读写端点(逆向编辑器 bundle 得到,2026-09-26)

平台对外只给**编译态**(`/kitten/r2/work/player/load/{id}`),**编辑格式只存在于"编辑器读写的那份文件"里**。
端点写在编辑器的 bundle(`creation.codemao.cn/kitten/build/kitten.*.js`,逐字见 `docs/rounds/37` §12):

| 用途 | 端点 | 备注 |
| ---- | ---- | ---- |
| **读编辑格式** | `GET {creation_api}/kitten/work/ide/load/{work_id}` | 返回 `{name, ide_type, preview, source_urls:[…/*.bcm4], …}`;`source_urls` 是**编辑器亲手写出的文件**(通常 10 个历史版本)⇒ 这就是正向语料的来源。按**权限**给源文件(无权限 ⇒ `source_urls` 空) |
| 写/保存 | 先上传 JSON 到 CDN,再 `POST {creation_api}/kitten/r2/work` | 载荷:`work_id`/`name`/**`work_url`**/`preview`/`orientation`/`sample_id`/`version`/`work_source_label`/`parent_id`/`save_type` |
| 服务端翻译 | `POST /kitten/work/translate` | 官方那条 Kitten→KN |
| 历史存档 | `GET /kitten/work/archive/{id}` | 版本列表(含 `archive_id`/`date_desc`/`save_type`) |
| `.bcm` 解码 | `POST /kitten/work/bcm/decode` | 载荷 `{code}` |

`creation_api` 是运行时注入的;本库对应 `BaseKey::Creation`(`https://api-creation.codemao.cn`)。
采集工具:`tests/convert_edit_harvest.rs`(落盘 `download/compile/k4edit/`)。

**为什么值得**:拿它喂正向扫描器就**打破了"自产自测"** —— 转换器的输入不再是"我们反编译器的产物",
而是编辑器亲手写的东西(我们归一化掉的形态才会暴露)。

## 9bis. 数"块数"有三个互不相等的口径(方法纪律)

同一份文档,"有多少块"至少有三种口径,答案**不相等**,混用就会造出**幻影缺陷**:

| 口径 | 含义 | 什么时候用 |
| ---- | ---- | ---------- |
| **(a) 原始 JSON 遍历** | 数与连接形态无关(census/扫描器用的就是这个) | 判"形态差异",但**不能**当"有没有丢" |
| **(b) 从根可达的树计数** | `parse_*_entity` + `tree.count()`:转换器实际处理的那棵树 | 判"转换器搬了多少" |
| **(c) id 是否出现在文档里** | 判"这块有没有被搬过去" | 判"丢没丢"的**唯一**直接证据 |

> 实例(rounds/37 §13.5–13.8):一次扫描报 `get_midis: 4 -> 0`,我先后用 (a)/(b) 的差值下了三次结论,
> 全被推翻;最后 (c) 证明"源场景 20 块的 id 在产物里 20/20 都在" ⇒ **块没丢**。
> **纪律**:跨口径比较前先声明口径;定案优先用**真 API 的最小复现**(把一个块喂进真函数看它变成什么),
> 而不是自己写的遍历。

## 依据

- `docs/rounds/20-kitten-kn-work-conversion-plan.md` §3(官方实现逆向)、§4(反向可行性)、§6.2(设计取舍)、§8(坑)、§9(验证)、§11(实测)、§11.2(真机端到端)。
- `docs/rounds/27-nemo-to-kn-conversion-plan.md` §9(前端/映射研究)、§11/§12(落地与方向表)。
- `docs/rounds/28-convert-reverse-fidelity-gaps.md`(缺口)、`docs/rounds/26-convert-rawvalue-single-pass-plan.md` §6(两项判不做)。
- `docs/rounds/33-corpus-sweeps-and-format-split.md`(两台扫描器 + 编辑格式缺口)、
  `docs/rounds/34-forward-corpus-and-guard-fixes.md`(§4octies 骨架键、§4nonies 词汇表与实机方法、§4sexies 云/本地收口)、
  `docs/rounds/35-kitten4-groups-fix.md`(角色不显示)、`docs/rounds/36-editor-vocabulary-single-candidate.md`(块与影子统一判据)。
- 代码锚点:`src/core/convert/translate/{mapping,nemo_mapping,assembly,neko}.rs`、`tests/convert_*`。
