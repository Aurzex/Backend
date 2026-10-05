# 转换域待做(Kitten/KN/NEMO 作品互转)

> 目标库。**已完成**的项见 `../knowledge/convert-*` 与 `../rounds/20–29` 的落地记录;这里只留**没做完/待核验**的。
> 决策类见 `./pending-decisions.md`。

> **重构方案**:`../rounds/37-convert-architecture-refactor-plan.md`(架构归位 + 合并清单 + 性能工作单 P1–P11;
> 已含**先决阻塞**:`convert_bench` 的 SHA 基线在 34–36 轮后未刷新,因此无法证明"输出不变")。
>
> **在案的后续方案(2026-10-01)**:`../rounds/39-convert-architecture-refinement-plan.md`
> —— 职责错位收口 + 门/仪器补洞 + 效果与性能的可证化;含 W1–W13 工作流、明确不做清单、与独立评审的逐条结论。
> **拍板与逐项状态见该文 §0.3 与 §5**;**它取代本节 §5 里已被其吸收的候选条目**(见 §5 的标注)。

> **后续优化方向盘点**(2026-09-26):`../rounds/37` §11 —— 按"证据 × 收益 × 风险"排,
> 含 profiler 证据(P6/少建中间 Value)、低成本的防复发项(gitignore 卫生规则、性能门、`unused` 告警)、
> 覆盖度卡点(编辑格式抓包、实机门进 CI)与相邻待做。

## 1. 待决(需人拍板)

| 项 | 出处 | 说明 |
| --- | --- | --- |
| ~~A1 大作品上传必失败~~ **已完成(修复,2026-09-26)**:上传路径改用请求级超时(`MewRequestBuilder::with_timeout`);**实测读数与常量值见 `../knowledge/nemo-runtime-and-upload.md` §6** | `../rounds/21` §8.4 N1 | 剩下的是**单包上限**(qiniu 413),**已完成(收尾,2026-09-26,决策见 `./pending-decisions.md` A5/D5;读数见 `../knowledge/platform-and-protocol.md` §5bis)** |
| ~~A2 `keep_source` 上传的是**反编译重建的编辑版**~~ **已完成(已决:方案1)  文档化;2026-09-26)**:`TranslateOptions::keep_source` 的 rustdoc 写明偏差,不改反编译侧 | `../rounds/21` §8.4 N2 | 无(名实不符已写进文档) |
| B2 KN 到 NEMO 是否立轮 | `../rounds/24` §12.3、`../rounds/27` §1 | 建议不做(平台无对照;要自建 NEMO 编码器) |
| **A4 NEMO 是否真机验证"上传到账号"** | `../rounds/30` §4、`../rounds/24` §13.4 | 链路已就绪(渠道 + 编排 + 选项);只差是否建立一份擦不掉的 NEMO 草稿(KN 侧已端到端验证) |
| `entity_concurrency` 是否对**大作品自动开** | `../rounds/25` §9 | 默认值为 1,由使用者显式传入;自动开需定阈值 |
| 反向(KN 到 Kitten4)并行是否择机重开 | `../rounds/25` §7 #1 / §10 | 重开的前提是**三段重设计**(拆 `unrewrite_calls` 的全局依赖);数据不支持直接做 |

## 2. 待做(有方案,不等决策)

0. **反向(KN 到 Kitten4)保真的量化口径(2026-09-26 口径)**:
   - 编辑器不认识的类型**必须落成编辑器认识的形态**:块**就地改成「未收录积木」标记**、影子**清空**
     (否则整份工作区加载失败,rounds/34 §4nonies;**rounds/38 §7bis 起块不再被剔除**),因此
     "未映射/不认识"的含义是**内容恢复不出来**(块本身还在画布上);
     两个真作品实测(rounds/36 **当时的口径是"剔块"**):`Node VM v3` 429 / 清影子 4、`now` 286 / 清影子 46;
   - 定义体侧现有门是**定义体缺口预算**(只许变小;数值与口径见 `../knowledge/convert-semantics.md` §6 硬门表;此前 0/0 的口径已被"剔除/标记"机制推翻);
   - 实体侧 `pure_list_get` 影子丢失**已修**(rounds/32 §3.4);列表影子那 86+192 是代理指标造成的假象
     (rounds/34 §4ter/§4quinquies,引用零丢失);
   - 因此剩下的减损空间**只剩"语义降级"**这一条路(见 `./pending-decisions.md` D1)。

1. **反向保真缺口的"研究史"(已收口,留档防止重开)** —— 现有结论见上面第 0 条:
   - **口径三次修正**:1)  旧口径把 `proceduresDict` 里的**调用树**当定义体比,误判出 118 块假缺口(rounds/32 §3.2);
     2)  残块(无人挂接的 `callreturn`/`repeat_n_times`/`script_variables`/`callnoreturn` 簇)会在反向重建树时自然消失,
     属**正确行为**,因此 `def_census` 改成只数**定义根子树**,预算一度由 `≤6/≤21` 变为 `0/0`(rounds/33 §3bis);
     3)  rounds/34 起"编辑器不认识的类型必须剔除",因此 0/0 不再成立,现有预算改为**只许变小的缺口预算**(数值与口径见 `../knowledge/convert-semantics.md` §6;rounds/36;**rounds/38 §7bis 起机制为"就地改成「未收录积木」标记",预算口径不变**)。
   - **实体侧的两处疑似缺陷都已定性**:inline `pure_list_get` 影子**真丢 −24**(`now` 由 186 变为 162)已修
     (根因在**正向**的列表影子步骤写在子块循环体内,只处理有连接的块;rounds/32 §3.4 提交 `0dce9d6`);
     列表影子 86+192 是代理指标 + id 重铸造成的假象,列表 id 三态一致 24/24/24,因此**引用零丢失**
     (rounds/34 §4ter/§4quinquies)。
   - **已知结构性(不是缺陷)**:Kitten4 没有 **list 类型的程序集参数**,因此 `param(type=List)` 必丢;
     `script_variables` 子树等 Neko 专有能力无对应概念(因此见 `./pending-decisions.md` D1)。
   - **仪器**:`kn_corpus_round_trip_sweep`(反向,读取任意 `download/compile/*.bcmkn`)、
     `k4_corpus_round_trip_sweep`(正向,读取 **`download/compile/*.bcm4`** —— 早前文档写的 `k4raw/*.json` 是漂移)、采集器
     `tests/convert_corpus_harvest.rs`(`#[ignore]`)。
   - **已完成(解决,2026-09-26,rounds/37 §12)**:正向扫描器要的**编辑格式**已能拿到 ——
  逆向编辑器 bundle 得读端点 `GET /kitten/work/ide/load/{id}`,其 `source_urls` 就是编辑器写出的编辑格式文件;
  采集工具 `tests/convert_edit_harvest.rs`,语料落 `download/compile/k4edit/`,扫描器已并入该目录。
  原记录(卡住的一步):正向扫描器要的**编辑格式**平台拿不到 —— `player/load` 是编译态(以此输入会静默产出空 KN,
     实测由 654 变为 13),`kitten/r2/work/edit/load/*` 与 `kitten/work/ide/load/*` 都 404,`source/public` 对 Kitten 报 422;
     编辑格式只存在于**编辑器保存时的载荷**里,因此需要浏览器会话抓包(形态与两次误判见 rounds/33 §2;
     扫描器已加"缺 `block_data_json` 即跳过"的形态守卫)。
2. **已完成** —— **NEMO 侧内存入口**:按 `../rounds/27` §2,应像 KN 侧一样把编辑版 `Value` 直接交给 translate,避免"落盘后再读回"。实现:NEMO 走 `WorkDecompiler::editable_document` 直接给明文编辑版(不落盘、不下资源),域门面再交 `translate_value`(见 `src/core/convert/decompile/mod.rs` 与 `src/core/convert/mod.rs` 的"方案 23 P0-2"注释;rounds/37 §10.1 P10)。
2a. **已完成(2026-09-26)**:单包上传上限已实测并加了
   `shared::ensure_single_package_fits` 提前报错闸(>20 MB 直接给出"上限 + 实测值"的错误);
   逐档读数/速率/渠道见 `../knowledge/platform-and-protocol.md` §5bis。
   要传更大作品需**分片上传**(qiniu 支持),暂无此需求(真实 KN 产物 3~9 MB)。
2b. **已完成(2026-09-26)**:convert 上传前取 preview 仍走全局客户端:`src/core/convert/mod.rs` 用 `WorkDataFetcher::new()`
   取作品 `preview` —— 已改为:`DecompiledArtifact::Document` 随产物带出 `preview`(反编译阶段本就拿到),
   建草稿不再重拉详情,省一个 RTT,那处全局客户端依赖随之消失(`915c8ff`,rounds/37 P10)。
3. **已完成(2026-10-01,`2d61959`)**:P3 结构化失败记录 —— 资源下载的失败清单已是 `(url, error)` 元组,重试直接用元组里的 url(原记录:`decompile/mod.rs` 用 `line.split(": ").next()` 从错误串反解 URL,因此 **URL 含 `": "` 会切错**、该文件不会被重试;错误文本本身不影响 —— `split` 取第一个 `": "` 之前的段。见 `../rounds/29`(P3 待做)、`../rounds/39` §W11)。
4. **已完成(2026-10-03)**:重连放弃的文档化 —— 云变量重连超过上限后仅 warn 并永久放弃、且**不再发事件**(仅初始 `Closed`),故调用方可能等到自己的超时;已写进 `CloudBuilder::max_reconnect_attempts` 与 `ConnectionEvent` 的 rustdoc(见 `../rounds/43` §6;原始记录 `../rounds/29` §4)。
5. **已完成(2026-10-03)**:转换域 P2 —— `simple.rs` 的 `Arc<Value>` 已改 `Arc::try_unwrap` 免拷(拿不到唯一所有权才回退克隆;见 `../rounds/45`)。
6. **已核销(2026-10-03,见 `../rounds/45`)** —— `../rounds/21` 的遗留项逐条过完。注意:该文的"明确遗留"在 **§8.6 的 L1–L5**(§6 是"风险与不做清单",§4 无字面勾选框,按 §8 实施记录与 §8.3 偏差表反推):
   - **已落实**:`§4` 的 P0-1(`staging_dir` 唯一化 + 回归测试)、P0-2/P0-3/P0-4、P1-5/7/8/10/11、P2-12/14/15/16/18;`§5` 的 S1–S4 全做完(§8.1/§8.2/§8.5)。
   - **已按"偏差"定案**:P1-6(D2 强转族只合一 truthy/num)、P1-9(不加 Kitten2 判据)、P2-13 的 C8(保持两层 `pub use`)、D8(坐标两式对照不合一)。
   - **L2** 上传超时:已由后续轮次覆盖(`convert/upload.rs` 的 `UPLOAD_TIMEOUT`);**L3** `keep_source` 传重建版:已按方案 1) 文档化; **L5** 测试抽取:**判不做**(§8.3 偏差已改成"测试模块放文件末尾"规则)。
   - **仍开放(一条,已登记)**:**L1** —— `translate_works` 的并发端到端**真机**上传用例(`tests/` 无并发上传用例;其根因——staging 目录唯一性——已有单测覆盖)⇒ 属实机测试扩充,需账号与可删草稿,列为**待决**。**L4 已完成(2026-10-03)**:`DecompilerError` 已改名 `ConvertError`(`TranslateError` 的对应变体也由 `Decompiler` 改为 `Convert`),见 `../rounds/46` 与 `../knowledge/errata.md`。
7. **NEMO"完整搬家"还缺资源重传**:"上传到账号"只传 `.bcm`,造型/音频仍指向源 CDN URL;
   要让新作品自带资源,需"逐资源上传 + 文档内 URL 改写"(官方 App 保存时上传 ~1390 个文件)。
   KN 侧无此问题(积木引用的是造型 id,资源在作品文件内/平台侧)。见 `../rounds/30` §4。

8. **已完成(核实并修注释,2026-10-02,`4072846`)**:结论 —— **"0 与 80"根本不是同一约定的两边**,`translate/model.rs` 那句自称"与 `XmlBlockWriter` 的约定一致"是错的(已改准);**数值 80/220 刻意不动**(坐标非语义,改数值零行为收益却要动基准产物字节)。
   - **平台取证(编辑器亲手写出的编辑格式:Kitten4 JSON 侧共 711 个含根块的实体,另有 1 个 Kitten3 blocksXML 场景)**:

     | 语料 | 实体数 | 根块 `location` 观测 |
| --- | --- | --- |
     | `download/compile/k4edit/215246857-{0..9}.bcm4`(几何对战-联机) | 4/版 | 每实体恰为 `x=0, y=0,80,160,…`(0 版 4 实体全网格;拖动过的版本里起点仍是 0) |
     | `download/compile/raw/几何对战-联机.bcm4` | 4 | 同上:`0 + 80·k` |
     | `download/compile/k4edit/174408420-{0..9}.bcm4`(A28社区-开幕) | 47/版 | 多数是用户拖出的任意值(`−131.777…`、`648.502…`、`2050` 等);每版另有 12–15 个实体是干净的 `x=0, y=0,80,…` |
     | `download/compile/raw/原气骑士 且听风吟-编辑版.bcm4` | 197 | 177 个任意坐标(`−178.9`…`696.7`);19 个 `x=0` 列起点 0;1 个起点 332 |
     | `download/compile/raw/春风得意-编辑版.bcm`(Kitten3 blocksXML) | 1 场景 | 根块 `(0,0)` 与 `(0,180)` |
     | `download/compile/*.bcm4`(本库的反编译产物) | — | 根块 `y = 50 + 70·k`(反编译侧自定,与本条无关) |

   - **判据**:1)  平台把根块坐标当**用户拖出来的位置**(含负数/小数),从不构成协议;2)  编辑器自己排出来的网格是 **0 + 80·k** —— 起点 **0** 出现在 **257/711** 个实体里,起点 **80 一次都没有**,因此若问"平台起点",答案是 **0**,但平台的**步长(80)**与 `XmlBlockWriter`/`model.rs` 的 220 **也不是一套**;3)  两处唯一的硬要求只是"**根块互不重叠**",而坐标本身**非语义**(`location` 在 `nemo_mapping` 语义 diff 的 allow-list 里,注释原话"根的位置是编辑器布局,不是语义")。
   - **为何不把 80 改成 0(实测数字)**:同选项(与 `convert_bench` 的 `kn-3.7MB` 样本完全一致:`deterministic_ids(true)` / `keep_source(false)` / `entity_concurrency(1)`)下把常量临时改成 0,产物 SHA256 由 `0d3cf2e3…`(与基线一致)变成 `15d7d050…`,字节数由 4117977 变为 4117976;`kn-9.4MB`(`Phigros`)则**逐字节不变**(`dac08917…`,与基线一致)。因此改数值零行为收益,却要重刷 `kn-3.7MB-kitten4` 这一个基线键。2026-10-02 的取证按"非语义字段不换来字节漂移"处理。(A/B 用的是一次性探针测试,跑完已删,未入库。)
   - **为何不共享常量**:两处是**同名不同物** —— `decompile/editors.rs` 的 `XmlBlockWriter` 写 Kitten2/3 blocksXML(`y=0.0`,0+220·i),`translate/model.rs` 写 Kitten4 `block_data_json` 的 `location`(`[0, 80+220·i]`,只在 KN 侧缺 `location` 时兜底;常量名 `ROOT_LAYOUT_Y`/`ROOT_LAYOUT_STEP`);格式、编辑器、方向三者皆不同,合并还要跨子域并统一 `f64`/`i64`,与 §11 已归档的"不可合并"定性一致。
   - 出处:第三十一轮只读审计 `../rounds/31-convert-layout-consolidation-plan.md` §3.6 N4。
9. **已完成(2026-09-26,`b7d4e07`)**:生成物里的零消费者常量 `translate/tables_gen.rs` 的 `TOP_BLOCKS` / `KN_TYPES` 已删(含生成器同步;核对时发现 `.gitignore` 的 `bin/` 通配误伤 `src/bin/`,生成器此前**从未入库**,已修)。原记录:全仓无使用点。
   注意它们是 `src/bin/gen_translate_tables.rs` **整文件生成**的,因此要删得改**生成器**再重新生成,
   否则下次生成又回来(同时核对生成器与手工表的分工)。
10. **零调用私有项**(审计 §3.6;2026-09-26 复核后收口):`BlockJson::count_types` / `BlockTree::count_types` ——
    复核结论:二者**仅测试/仪器用**(`reverse_tests` 的 census),`BlockJson::walk` 与 `BlockTree::walk` 是**递归核心**、
    有生产调用(`assembly::duplicate_ids`),因此按仓库口径"**保留并注明**",不删。原记录:
    `nemo::parse`(仅测试用)、`DecompilerContextBuilder`(已随骨架瘦身删除)、`TOP_BLOCKS` / `KN_TYPES`。
    处理口径:仅测试用则标 `#[cfg(test)]` 或保留并注明;完全不用的则删(删除前按仓库约定确证零调用)。
    **已完成(2026-10-02,rounds/40 R2,`30216c5`)**:两处 `count_types` 都标了 `#[cfg(test)]` —— 从生产构建里彻底移出、函数体保留,`reverse_tests` 的 census 仍可用(1 参版本调用 2 参版本,两处必须同标)。
0b. **已完成(2026-10-03 收尾)**:词汇表新鲜度:`kitten4_vocab.rs` 里的编辑器类型快照(条目数与导出日期以该文件的 `KITTEN4_VOCAB_EXPORTED` 为**单一事实源**,见 `../knowledge/convert-semantics.md` §5bis 第 3 条);
   编辑器升级后名字会漂移(名字认错 = 整份打不开)。原先只记日期、没有可复跑流程的缺口**已补**:文件头注释给出三步(无头浏览器打开 Kitten4 并等 `window.Blockly.Blocks` 就绪 → 控制台 `Object.keys(window.Blockly.Blocks).sort()` 逐行打印可直接粘进 Rust 数组的形式 → 整体替换数组并更新 `KITTEN4_VOCAB_EXPORTED` 的 `(日期, 条目数)`),`cargo test --lib editor_type_list_` 是验收门。
0c. **已完成(2026-09-26;rounds/38 起由"剔除量门"改成 `MARKER_BUDGET`)**:标记量的预算门:原先只有定义体侧有预算断言;
   块/影子的"剔除量"没有门(rounds/36 的人工 A/B 读数见 `./pending-decisions.md` D4)。现有口径:守"每件作品
   **改成「未收录积木」标记的块数 + 清空影子数**"(只许变小,已对 `download/compile/*.bcmkn` 全语料记基线)。
11. **剩余重复项的定性结论已归档**(`../rounds/31` §3.6):`D2` 族 JS 值强转、`N3` Fetcher/ResourceManager 样板、
    `N4` 布局常量**已核实并定性**(同名不同物、不可合并,见 §2 第 8 条);`D3`/`N1` 已合并。不再重新提"把这些也合一"。

12. **已完成(2026-09-26)**:`类型歧义` 借用 `DroppedProperty` 上报(`mapping.rs` 的"Kitten 原类型有 N 个…"分支),
    导致类别标签「丢弃实体属性」与事实不符(它并非丢属性,而是"KN 一个类型对应 Kitten 多个源类型")。
    —— 已按方案 1)  落地:新增 `TranslateWarning::AmbiguousType { kind, candidates, chosen }`(公开枚举加变体,
    预 1.0 可接受),并计入 `is_lossy`(Kitten 原类型名不可恢复)。
    报告类别因此从「丢弃实体属性 3301」修正为「类型歧义 2959 + 丢弃实体属性 342」。

13. **待方案**(未修 + 有风险):`download/compile/` 同时是**输出目录**与**夹具/语料目录**(`../rounds/40` 记的
    "夹具同名冲突"在同一结构上的第二个实例)——
    - **写点**:反编译默认输出目录(`decompile/config.rs` 的 `default_output_dir`)、语料采集器
      `tests/convert_corpus_harvest.rs`、编辑格式采集器 `tests/convert_edit_harvest.rs`;
    - **读点(byte-baselined 夹具)**:`convert_bench` 5 个样本、`convert_facade_bench` 4 个样本、
      `convert_work_bench` 的 `RAW_SAMPLE`,以及 `reverse_tests.rs` / `nemo_tests.rs` / `translate/mod.rs` /
      `pipeline.rs` 的多条真作品门 + 两个**默认跑**的往返扫描器(遍历整个目录)。
    采集器是"增量写新文件、不覆盖"故**至今未出事故**;但**默认输出通路仍在**:以默认选项反编译恰为夹具的那几件
    作品会按同名口径**覆盖夹具**(`../rounds/40` 的 `kn-9.4MB` 就是这么被真机门覆盖的,已按"夹具迁到只读目录"
    临时处置)。**彻底修 = 把"语料/夹具目录"与"输出目录"分离**(较大的布局约定变更),需先出方案。
    出处:`../rounds/40-gates-cleanup-and-real-defects.md` §8.4。

## 3. 已在案、不做的(不再重开)

- `RawValue` 顶层只透传、单遍遍历合并(`../rounds/26` §6):透传占比 ≈0%、正向本已 2 趟。
- 反向实体级并行(`../rounds/25` §10):Amdahl 上限 1.9×,反向 `core` 仅 ~200 ms。
- 产物**逐字节对齐官方**:官方按键插入序、id 随机,因此只做语义 diff + `validateBcm` 硬门。
- 编译版块引用**不做**字符串容错(`../rounds/21` §7-1:真样本 2236 处采样全是内联对象;遇字符串显式报错)。

## 4. 需要留意的既有防线(改动前先看)

- `tests/convert_live.rs`(真机,默认 `#[ignore]`;写平台的用例会建"可删"草稿)。
- `tests/convert_bench.rs`(自有 SHA256 基线:任何产物字节变化都必须先解释再接受;样本清单与"两种门"的说明见 `../knowledge/convert-performance.md` §5)。
- `reverse_tests` 里的往返多重集守恒 + 缺口预算断言(只许变小;每次跑打印 `[预算]` 读数)+ **两腿 id 口径台账**
  (正向 `[id台账]`/`LOST_ID_BUDGET`、反向 `[id台账·反向]`/`LOST_ID_BUDGET_REVERSE`)+ 反向标记量门 `[标记量]`/`MARKER_BUDGET`。
- **实机门(最强)**:无头 Chromium + 线上 Kitten4 的「打开本地作品」—— 数画布积木 + 看角色列表,
  必须带一个已知能读的对照组(方法留档见 `../rounds/35`;配方也可从 `../knowledge/convert-semantics.md` §5bis 反查)。
- 官方校验器 `BcmHelpers.validateBcm`(headless 可跑)是"产物能否被编辑器加载"的硬门。

---

## 5. rounds/37 之后的下一步

| # | 候选 | 前置条件 | 说明 |
| --- | --- | --- | --- |
| 1 | ~~**把两条扫描器的"差异类别"升级成基线门(只许变少)**~~,**经 rounds/39 W3c 改判**:差异类别**不升格为门**(保留"只打印 + 人工分诊"),只**删掉** `reverse_tests.rs` 那份死 allow-list 副本(活的那份仍按名引用)。**已完成(死副本已删,2026-10-01,`e8e19d6`)**。理由:`download/` 是 gitignored 的**增量**语料,集合门必然假红;与仓库"按文件名索引 + 表外取表内最大值"的门口径不合 | — | 见 `../rounds/39` §W3c/§4 |
| 2 | 性能:P8–P11 | — | **实测判定不再做**:P2/P3/P5/P6 同轮 A/B 都落在噪声内(±2%,rounds/37 §6.2bis/§10.6);要真收益只有"少建中间 `Value` 树"那条重写装配层的路,风险极高,除非有硬性指标。**R6 已量(2026-10-02,判定类任务,零落地)**:把一次 `translate_file` 的分配拆成「源解析 `A_parse` / 产物物化 `A_mat` / 转换自己 `core`」三段(`convert_bench` 的同一批样本,一次性探针)= `A_mat` **1.49–1.59 次/节点**、`core` **52–79% 的 A**、产物 **79–98%** 节点是积木,因此**上界显著(远超 ±2%)**,但同方向两个最小切片(P6 `../rounds/37` §10.6、W9 `../rounds/39` §W9)实测收益均为 **0**,因此只能靠"拿掉 `BlockJson` 中间表示、由 parse 到 mapping 直接写流"的**完整重写**才可能吃到,且**必须先有硬指标**。因此该方向在"有硬指标 + 认领完整重写"之前**关闭**,连同 P8–P11 一起不再单独提起(注意:NEMO 两行的 `core` 含输入 **XML(DOM)构建**,不能全按"中间 `Value` 冗余"读)。**2026-10-05 补充(平坦热点 + 分配器 A/B,读数见 `../knowledge/convert-performance.md` §2bis)**:进程 CPU 里 `libc` 分配/释放占 **36.8%**、`Value` 树解析/析构加 `BTreeMap` 与序列化转义合计约 **35%**,而本库转换逻辑合计只有 **8.5%** 且无单点超过 1% —— 与该判断一致;另:只换分配器(jemalloc)即省 `core` 14%–26%、整套墙钟 12%–16% 且产物逐字节不变,已登记为 `./pending-decisions.md` **D7** |
| 3 | ~~NEMO 方向:补 SHA 基线~~ **已完成(2026-10-01,见 §6.3 G6)**:样本进 `SAMPLES`(`source_version` 逐样本透传 + `#meta` 记该字段),两件 NEMO 作品入基线 | — | 加样本时 `Sample` **必须能传 `source_version`**(否则基线锁死"不迁移"口径,门绿但证错东西)。**W8 已处置(2026-10-02)**:2)  已做(`6057a88`:虚拟包装根,每实体省一次整串拷贝,NEMO 两样本分配字节 −3.4%/−3.1%、产物字节不变);1)  量到上界过小(≤ 0.11% 次数)且要换峰值内存,因此**不做**(见 `../rounds/39` §W8 落地段) |
| 4 | 实机门进 CI | — | 离线替身:官方 `validateBcm` 可 headless 跑。**CI 现状(2026-10-02 起,`f68c2e6`)**:`build` 矩阵 + `hygiene` + **`offline-gate`**(fmt --check / clippy -D warnings / 逐目标点名的离线测试);**刻意不跑**真机门与吃 `download/` 的语料基准(口径见 `../knowledge/repo-conventions.md` §6),因此任何 CI 门都得先有**入库的**夹具。已挂到**第 40 轮 R5**(见 `./README.md`) |
| 5 | `wrap_arithmetic` 移动不重铸(`../rounds/39` W13) | — | **暂缓(待重新立项)**(2026-10-02 拍板登记):**不列入执行队列**。收益主要是"本库往返 id 台账更干净 / 往返 id 更稳",**不是用户可见差异**;代价是**偏离官方实现 + 改产物字节 + 需重做实体机与刷基线**。证据:`../rounds/38` §8、`../rounds/39` §W13 |

---

## 6. 转换**效果**(保真)提升:方向与目标(2026-10-01 立)

> 与"性能"分开:性能在 `../rounds/37` 已收口(同轮 A/B 全部落在噪声内,判不再做)。本节只谈**转换质量** ——
> "编辑器能打开"已达成之后,产物里**少没少东西、少得对不对**。
> 依据:`../rounds/37` §11/§13、`../knowledge/convert-semantics.md` §5/§5bis/§9bis、本库 §1(D1–D5)。

### 6.1 方向(一句话)

**从「能打开」推进到「打开后与原作品对得上,且这份一致性由可复跑的门守住」。**
三步走:1)  用 **(c) 口径(id 是否出现在产物)** 把两台扫描器上剩余的每一条差异**定案**(真丢还是归一化);
2)  对定案为真丢的项做减损:能保住的**保住**(rounds/38 §7bis 起:不认识的块**就地改成「未收录积木」标记**,不再剔除;
以实机加载门为界,**不做语义降级** —— D1 结论不变);
3)  把差异类别 / **标记量预算**(`MARKER_BUDGET`)/ id 台账(`LOST_ID_BUDGET` + `LOST_ID_BUDGET_REVERSE`,两腿)/ 词汇表 / 加载门固化成"只许变小、只能变好"的门
(差异类别按 `../rounds/39` §W3c 判**不做**门:增量语料下会假红)。

### 6.2 现状(证据,2026-10-01 复核)

| 面 | 状态 | 出处 |
| --- | --- | --- |
| 正向(Kitten4 到 KN) | 真原件语料 24 条差异**全部**归因于改名/等价类 + 已文档化降级与包装 + 槽默认影子不回写,因此**无已确认缺陷** | `../rounds/37` §13.4/§13.8 |
| 反向(KN 到 Kitten4) | 自建、有损;实机门已过(角色列表 + 画布块数);损失已量化 = **改成「未收录积木」标记**(位置与存在保住、内容不可恢复)+ 槽默认影子不回写 + 类型歧义 + id 重铸 | `../rounds/34/35/36/38`、`../knowledge/convert-semantics.md` §5bis |
| 既有门 | **门的定义与数值以 `../knowledge/convert-semantics.md` §6 为准**;本表只记落地读数:**三处预算门(都只许变小)** —— id 台账门 `LOST_ID_BUDGET`(正向扫描器,`reverse_tests` 的 `k4_corpus_round_trip_sweep`)+ id 台账门 `LOST_ID_BUDGET_REVERSE`(反向扫描器,`kn_corpus_round_trip_sweep`;逐件打印 `[id台账·反向]`,口径见 `../rounds/39` §W3d)+ 标记量门 `MARKER_BUDGET`(反向扫描器,**"改成「未收录积木」标记的块数 + 清空影子数"**,rounds/38 起由"剔除量门"改名改语义);`convert_bench` SHA + `#meta` 基线(Phase 0 已重刷加固,`ae41361`);往返多重集守恒 | `reverse_tests` 的扫描器/预算函数(按名引用;行号会漂)、`tests/convert_bench.rs` |
| 仪器 | `kn_corpus_round_trip_sweep` / `k4_corpus_round_trip_sweep`(默认跑,读 `download/compile/`,缺语料则跳过)+ 平台真原件编辑格式语料 `download/compile/k4edit/`(2 件作品 × 10 版)+ 实机门(手动) | `reverse_tests` 的同名扫描器、`tests/convert_edit_harvest.rs` |
| NEMO | 与官方逐数一致;**已有 SHA + `#meta` 字节门**(`convert_bench` 的 `nemo-3.4MB-kn` / `nemo-old-1.5MB-kn`,后者让版本迁移真的进基线;`#meta` 带 `source_version`);`report.elapsed_ms` 实测非 0(3.5 MB 源:core 365–442 ms),原先记的"恒 0"已过时;`tree_to_json` 编码失败不再静默丢块(进报告,计有损) | `tests/convert_bench.rs`、`src/core/convert/translate/nemo.rs`(2026-10-01) |

### 6.3 目标

| # | 目标 | 验收(可观测) | 前置/成本 |
| --- | --- | --- | --- |
| **G0** **已完成(2026-10-03)** | 先把门跑绿并留读数 —— `cargo test --lib`(含两台语料扫描器)与 `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored` 均绿;读数(定义体缺口预算 / `LOST_ID_BUDGET` / `LOST_ID_BUDGET_REVERSE` / `MARKER_BUDGET`)已入 `../knowledge/convert-semantics.md` §6 的硬门表,扫描器每次跑都打印 `[预算]`/`[id台账]`/`[标记量]` | 无(常态门;2026-10-03 复跑见 `../rounds/45` §3) |
| **G1** **已完成(2026-10-01,rounds/38)** | **反向腿 id 口径定案**:两条悬案判「真丢 / 归一化」,各附 (c) 口径证据或最小复现 | 1)  `lists_get`(等价类代表 `pure_list_get`)大减 = **归一化**(丢的逐个都是 KN 侧 `is_shadow` 的 `pure_list_get`,折回父块 `fields`);2)  `bcm_translator_text_return_value_block` 由 4 变为 0 = **真缺陷**(187 个占位映射里 43 个没有 `RC` 标题,因此正向不写 mutation,反向反查失败,被写出阶段整块剔除) | 已出结论 + 证据(`../rounds/38` §3) |
| **G2** **已完成(2026-10-01,rounds/38 §7bis)** | **减损:把「不认识就剔除」在信息层降级为「保留为编辑器认识的不可用标记」** | 写出阶段统一处理:**所有**编辑器不认识的块就地改成 `incompatible_block`(语句位)/ `incompatible_output_block`(值位),位置与连接保持、字段/影子/变异清空;影子仍是清空。覆盖占位积木(43 类型)+ D1 的 Neko 专有块族(`temporary_list`/`script_variables*` …)。正向补了由 `incompatible_*` 到 `bcm_translator_text_*` 的手工映射,往返稳定 | 已落地 + 单测 + 全语料 + 实机(`HEX Editor` 182 个标记,角色 `raw` 4/4 对上) |
| **G3** 部分(两腿 id 台账均已落地;类别集合门改判不做,2026-10-01) | **id 口径台账**(正/反两腿)+ 差异类别集合门 | 已完成(正向扫描器 `k4_corpus_round_trip_sweep`):**`LOST_ID_BUDGET`**(只许变小,每次打印 `[id台账]`;基线 = 59 件里 33 件非零,构成已逐类查过)。**已完成(反向 id 台账已建)**(`../rounds/39` §W3d):反向扫描器 `kn_corpus_round_trip_sweep` 的 **`LOST_ID_BUDGET_REVERSE`**(正向同口径,逐件打印 `[id台账·反向]`;`d10d0cb` 首版、`ef37978` 把口径收窄到真正的积木节点)。基线读数:多数作品 **0/0**,`FjDQB…bcmkn` 真块 0 / 影子 120,`FjSw…bcmkn` 真块 110 / 影子 32(全名见 `reverse_tests` 的 `LOST_ID_BUDGET_REVERSE`)。**差异类别集合门已改判不做**(gitignored 增量语料,必然假红,`../rounds/39` §W3c;那份死 allow-list 副本已随 `e8e19d6` 删除) | 成本低;口径已稳 |
| **G4** **已完成(2026-10-02)** | **覆盖度**:真原件语料常态化(并入常规扫描)+ 词表新鲜度检查 | 已完成(语料常态化):`download/compile/k4edit/` 的平台原件已由正向扫描器常规吃到(`k4_corpus_round_trip_sweep` 扫 `k4edit/*.bcm4`,默认 `cargo test` 即覆盖)。已完成(词表侧):`kitten4_vocab::tests::editor_type_list_is_sorted_and_queryable`(严格升序 + 抽样查询,含两个「未收录积木」标记)+ **新鲜度读数** `editor_type_list_freshness_is_reported_not_enforced`(打印条目数 / 导出日期 / 距今天数,条目数与记录不一致或 >180 天时醒目提醒;单一事实源 = `kitten4_vocab::KITTEN4_VOCAB_EXPORTED`)。**刻意不做按挂钟时间失败**(如"导出超 N 天就 panic"):那会让整套测试在某个日期之后**自动变红**、门沦为噪音而被忽略(仓库的门是 `-D warnings` + 全绿)—— 新鲜度**只打印,不参与断言** | 成本低;扩语料需登录采样 |
| **G5** 待决(已挂到第 40 轮 R5) | **加载门离线化进 CI**:`validateBcm` headless 接进 CI(实机浏览器门仍手动) | CI 上一条"产物可被编辑器加载"的校验;故意写一个编辑器不认识的类型,预期被拦下 | 重:`download/` 与官方 bundle 都不入库,因此要先决定**入库最小夹具**。CI 现状(2026-10-02,`f68c2e6`)= `build` 矩阵 + `hygiene` + `offline-gate`;真机门与语料基准**刻意不进**(见 `../knowledge/repo-conventions.md` §6) |
| **G6** **已完成(2026-10-01)** | **NEMO 补门**:加 SHA 字节基线 + 耗时读数 | NEMO 样本进了 `convert_bench` 的 `SAMPLES` 并有 `#meta`:1)  `nemo-3.4MB`(`download/compile/` 的真作品,`source_version` = 0.16.2 = 迁移目标版本,迁移为 no-op;与官方产物**语义 diff 0 处**、`validateBcm` 判定为 VALID);2)  `nemo-old-1.5MB`(公开作品 `103791894`,0.11.0 < 0.15.0,因此**YC 迁移生效**;文件已挪进语料目录 `download/compile/`(`1d0be93`;R2 前在 `temp/harness/`),缺失仍只跳过不假红)。`elapsed_ms` 非 0(实测)。另:`tree_to_json` 的编码失败不再静默丢块(进报告、计有损) | 已完成(含基线刷新) |

### 6.4 建议顺序与需拍板处

**顺序**:`G0`、`G1`(定案,决定后面是否还有真正可做的工作)、`G3` 与 `G4`(成本低、防复发)、`G2`(真收益,依赖 G1 结论 + 载体研究),最后 `G5`/`G6`。

**G2 的载体路线已决(2026-10-01,rounds/38 §7bis,用户拍板)**:「不认识就保留为编辑器认识的标记」
(`incompatible_block` / `incompatible_output_block`)已落地并实机验证 —— 它不是 D1 的语义降级(不声称等价、
不冒充可用),而是"把丢失变成可看见的损失"。**该问句已关闭,不再挂起**。

**拍板处与逐项状态见 `../rounds/39` §5 / §0.3**(2026-10-02 已按证据全部拍板并落地:C1 = 做、C2 = 不拆、C3 = 做;另 W52)  已完成、W13 暂缓)—— **本节不复制结论与提交号**。

## 7. 性能盘点(2026-10-03;1/2 已落地,3/4/5 结论见下)

> 结论先行:转换域的性能大方向早已关闭(见 §5 第 2 行与 `../rounds/40` §5)。本节的五条小改已逐条处置,判据仍是"逐键/逐字节等价 + 产物 SHA 不变";落地的两条**不宣称提速**(读数在噪声内,见 `../rounds/45` §3)。

| # | 项 | 结论 |
| --- | --- | --- |
| 1 | **反向半边不走已有索引** | **已落地(2026-10-03)**:`mapping.rs::{is_renamed_lc_key,unmapped_field_text,reverse_slot_name}` 改用新索引(`group_index`,**不去重**——反向靠"命中数唯一"判歧义);另补 `zh_title` 的键索引 |
| 2 | **反向值向反查缺索引** | **已落地(2026-10-03)**:`reverse_field_name` 的两趟线性扫换成 `flat_reverse_index`(值 → 全部命中,保序);`reverse_appearance_attribute` 换成 `pair_reverse_index` |
| 3 | **`resolve_callee` 逐调用块扫 `procedures`** | **不立项**:量级上限 = 调用块数 × 定义数 × ≤3,典型作品为万级短串比较(比 P5 那次的百万级低两个量级),且要为此把索引穿进 `rewrite_node`/`unrewrite_node` 递归链;除非某件作品实测显示它进热路径,否则不动 |
| 4 | **NEMO `parse_block` 的 `<field>` 分支克隆整份 fields** | **已核实不可行**:该 `fields` 在 `extend` 之后还要被读两次(`message` 与 `handle_procedure_fields`),而 `extend` 需要所有权 ⇒ 这次克隆是结构必需;换 `&Map` 也仍要逐条克隆,省不掉 |
| 5 | **正向 `collect_forward_items` 每实体深拷 `source`** | **不立项**:消费方(`assembly` 的 `kitten4_scene/actor`)只读 `&source`,但要免掉这次深拷只能把整份实体搬出源文档再搬回(动"唯一接触源文档"的跨阶段不变量),而同族的反向 P2 实测收益只有 2~3% ⇒ 收益与风险不成比 |

**已判不做(不要重开)**:`find_object_shadow` 惰性化(P4,`../rounds/37` §4;W10 起已由 `normalize_object_shadows` 的廉价探测替代)、P6 手写去 `#[serde(flatten)]`、W8 1)、W9、W13、单遍遍历合并、`RawValue` 透传、`BTreeMap` 改 `HashMap`、合并两份 `tree_to_json`、NEMO 每实体多趟全 DOM 遍历(属"单遍遍历合并"家族)。
