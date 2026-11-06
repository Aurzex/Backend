# 第五十五轮记录 — 2026-11-06 「未收录积木」原内容写入平台注释(可恢复)

## 0. 一句话

反向(KN 到 Kitten4)把编辑器不认识的块改成「未收录积木」标记时,原 KN 节点的标量内容(类型/字段/影子/变异)
与槽名映射写进平台**原生注释**(`block_data_json.comments`);再转回 KN 时逐字段还原。
10 件 KN 语料的往返差异类别由 52 降到 4、定义体差异行由 64 降到 10,标记量与 id 台账不变,实机加载通过。

## 1. 背景与判据

- `../rounds/36`–`../rounds/38` 的既定策略:编辑器不认识的**块**就地改成「未收录积木」
  (`incompatible_block` / `incompatible_output_block`),位置与连接保住,内容(类型/字段/影子/变异)丢弃;
  语义降级在 D1 判定不做(`../goals/pending-decisions.md`)。
- 2026-11-06 核对线上 Kitten4 编辑器 bundle(`creation.codemao.cn/kitten/build/kitten.fc848ad0879ae6a93ee0.js`,
  4.5 MB):两个标记块的定义为
  `{type, message0:"%{BKY_INCOMPATIBLE_BLOCK}", args0:[], previousStatement/nextStatement}`(值型无 `output`),
  **块上没有任何字段** —— 原内容不能放在块里。
- 平台文件本身用**注释**表达说明:`block_data_json.comments`(`id → {id, parent_id, text, pinned, size,
  auto_layout, color_theme, location}`),块上的 `comment` 指向它。本仓 Kitten4 语料(62 个 `.bcm4`)实测:
  块级 `comment` 非空 93 处、`comments` 表非空 63 处 ⇒ 注释是平台原生、可存可读的通道。
- 因此本轮不动 D1/D2:标记仍是标记;新增的是**内容承载与还原**。

## 2. 方案

### 2.1 载荷形态

- `KnOrigin { node, slots }`:`node` 为原 KN 节点的**标量** JSON(去 `inputs`/`statements`/`next`,
  保留 `type`/`fields`/`shadows`/`mutation`/`is_shadow`/`is_output`/渲染键);`slots` 为
  `(KN 槽名, 写出去的 Kitten4 槽名)`。
- 注释文本:第一行给人看(`未收录积木:<中文名>(<KN 类型>)。原积木内容已保存在本注释里,转换回 KittenN 时自动还原。`),
  第二行 `#convert-kn#` 加紧凑 JSON。哨兵同时是"这份块表有没有本转换器写的注释"的廉价判据。
- 注释对象键名照平台原件:`pinned=false`(未置顶,与平台用户注释同形)、`size=[214,126]`、`auto_layout=true`、
  `color_theme="BLUE"`、`location` = 积木坐标右移 220。

### 2.2 写出(KN 到 Kitten4)

- `mapping::reverse_node` 在类型反演之后、字段/影子/槽名改写**之前**抓快照(只有"编辑器不认识"的块才抓,
  判据与写出阶段同一谓词 `kitten4_editor_knows`)。
- `assembly::mark_unknown_blocks_encoded` 收下快照;注释 id 在装配**末尾**统一铸(排在组/变量/音频之后,
  既有产物 id 一个都不偏移),`EncodedBlocks.comments` 随 `block_data_json` 在**物化与流式两条写出路径**一起写。
- 报告里的标记类型名改用**原 KN 类型**:此前用反向挑出的 Kitten 侧候选名(`timer` 被报成
  `mobile__timer_value`,后者从未在源作品里出现过)。

### 2.3 读回(Kitten4 到 KN)

- `pipeline::parse_forward_item` 从 `block_data_json.comments` 取 `id → 文本`;原文(Raw)路径只在出现哨兵时
  才多解析一次(平台原件没有哨兵,骨架快速通道成本不变)。
- 语义映射**之后**调 `origin::restore_marked_blocks`:标记块整块换成载荷里的原节点,子块按载荷槽名放回原槽;
  报告记 `TranslateWarning::RestoredMarkedBlock { blocks }`(非损失)。
- 放在映射之后的原因:映射会把标记改写成占位块并动过字段/影子/槽名;载荷是 KN 口径,整块替换只保留连接与子块。

## 3. 代码锚点

- 新增 `src/core/convert/translate/origin.rs`(载荷捕获/文本序列化/还原);
- `model.rs`:`BlockJson.origin`(`#[serde(skip)]`)、`EncodedBlocks.comments`、`add_origin_comment`;
  `encoded_to_value` / `write_encoded_blocks` 写注释;
- `mapping.rs`:`reverse_node` 的 3bis 抓快照;`reverse_slot_name` 提为 `pub(super)`;
- `assembly.rs`:`mark_unknown_blocks_encoded` 收集待挂注释;`Kitten4Hook.pending_comments`;装配末尾铸注释 id;
- `pipeline.rs`:`comments_of`(Value 与 Raw 两条路径)+ 映射后还原;
- `report.rs`:`RestoredMarkedBlock`(非损失类别);
- `reverse_tests.rs`:语料扫描新增**标记可恢复门** + 合成文档的端到端单测。

## 4. 验收

### 4.1 单测

- `origin::tests` 三条:载荷往返、还原槽映射、没有本转换器注释的标记不动。
- `reverse_tests::marker_content_round_trips_through_platform_comment`:合成 KN 文档到 Kitten4
  (断言标记块挂注释、文本含哨兵、`pinned=false`)再回 KN(断言类型/字段/影子/变异/子块逐项回来、报告有还原读数)。
- 语料扫描:标记块在往返后按类型**全部回来**(`kn_corpus_round_trip_sweep` 内按类型求和断言)。

### 4.2 语料读数(10 件 KN 作品,`cargo test --lib kn_corpus_round_trip_sweep`)

| 指标 | 改前 | 改后 |
| --- | --- | --- |
| 标记块总数(全语料) | 1237 | 1237 |
| 往返有差异的作品 | 8 / 10 | 4 / 10 |
| 名字层面差异类别 | 52 | 4 |
| 定义体差异行 | 64 | 10 |

- 余下的 4 类差异逐条有因:横屏坐标壳(`math_arithmetic` 与 `math_number` 成对增加)与
  `calculate` 到 `bcm_translator_text_return_value_block` 的 1:1 降级(rounds/38 已定案)。
- 标记量与 id 台账(`[标记量]`、`[id台账·反向]`)与改前逐件相同(`FjDQB` 604/62、`FjSw` 429/4、`HEX` 182/17)。

### 4.3 字节与报告基线(`convert_bench`)

| 样本 | 产物字节 | 产物 SHA256 | 说明 |
| --- | --- | --- | --- |
| `kn-3.7MB`(HEX Editor) | 4 117 977 → **4 354 446** | `0d3cf2e3…` → **`b2c39727…`** | 182 条标记注释;块数 1597、告警 508、源 SHA 不变 |
| `kn-9.4MB`(Phigros) | 9 775 778 不变 | 不变 | 该样本没有标记块 ⇒ 零注释 |
| `kitten4-0.3MB` / `kitten4-10.8MB` / `nemo-3.4MB` / `nemo-old-1.5MB` | 不变 | 不变 | 不经过标记路径 |

- `#meta` 的分配计数同批重刷,但**基线自 `9290ede`(2026-10-02)起未重刷** ⇒ 它们的下降来自
  rounds/47、49、50、54 的既有优化,**不归因本轮**(例:`kitten4-0.3MB` 串行腿 68 665 次 → 28 918 次)。
  本轮在 `kn-3.7MB` 上新增的是 182 条注释的构造;该样本的串行腿与并发腿读数仍逐位相同。
- 重刷命令:`BACKEND_BENCH_REFRESH=1 cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`;
  门(`--ignored`,不带 `REFRESH`)随后复跑通过。

### 4.4 实机验证(无头 Chromium + 线上 Kitten4 的「打开本地作品」)

- `算乘法_287002279`(1 个标记):加载成功,8 个角色全部列出;切到含标记的角色(`开心编程猫`)后画布 10 块,
  标记以「未收录积木」渲染。
- `HEX Editor_317683843`(182 个标记,4.3 MB):加载成功,默认角色 62 块,4 个标记块在语句链里原位渲染;8 个角色。
- 注释在本版编辑器里**未观察到渲染**:标记块的 SVG 只有 `blocklyPath`/`blocklyText`,全页没有 comment 类元素
  ⇒ 注释是**数据通道**,不是可见 UI。
- 保存后注释是否保留:**待核验** —— 无头环境下「保存到电脑」未触发下载,保存到账号需要登录。

## 5. 与既定决策的边界

- D1(不做语义降级)不变:标记不冒充可用、不声称等价。
- D2(影子清空)不变:影子仍清空,不写标记。
- 新增的是"内容承载与还原",不是"把未收录积木变成可执行积木"。

## 6. 未覆盖 / 后续

- 正向(Kitten4 到 KN)的对称损失未处理:Kitten4 专有块降级成 `bcm_translator_text_*` 时类型同样不可恢复;
  对称做法是在 KN 实体级 `comments` 写载荷(KN 的块注释同样指向实体注释表),需另立一项。
- 编辑器保存后的注释存活未验证(见 §4.4 待核验项)。
- 注释不渲染 ⇒ 用户在画布上仍只看到「未收录积木」;若要在画布上直接看到原类型,需要另设可见载体(未立项)。

## 7. 实施记录

| 序 | 内容 | 验证 |
| --- | --- | --- |
| 1 | `origin.rs` 载荷(捕获/文本/还原)与单测 | `cargo test --lib origin::` 3 passed |
| 2 | `model.rs` 注释写出(两条路径)+ `report.rs` 新类别 | `cargo test --lib` 164 passed |
| 3 | 反向抓快照 + 装配末铸注释 id;正向解析还原 | 同上的合成文档端到端单测 |
| 4 | 语料扫描加标记可恢复门;`convert_bench` 基线重刷 | `kn_corpus_round_trip_sweep` 通过;基线见 §4.3 |
| 5 | 实机(算乘法 / HEX Editor) | 见 §4.4 |
| 6 | 文档(rounds/55、`../knowledge/convert-semantics.md` §5/§5bis/§6、`../goals/convert-backlog.md` §6) | 本文件 |

代码落地提交:`b64941b`(实现 + 单测 + 语料门 + `convert_bench` 基线);本文件为后续文档提交。

## 依据

- 标记机制与 D1/D2:`../rounds/34` §4nonies、`../rounds/36`、`../rounds/38` §7bis、`../goals/pending-decisions.md` D1/D2。
- 注释格式与实机方法:`../knowledge/convert-semantics.md` §5bis(隐性契约)与平台语料 `download/compile/**/*.bcm4`。
- 编辑器 bundle:`creation.codemao.cn/kitten/build/kitten.fc848ad0879ae6a93ee0.js`(2026-11-06 拉取,4.5 MB)。
- 读数来源:`cargo test --lib kn_corpus_round_trip_sweep`、`convert_bench` 基线(`tests/fixtures/translate/convert_bench_baseline.json`)。
