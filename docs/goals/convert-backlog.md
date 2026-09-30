# 转换域待办(Kitten/KN/NEMO 作品互转)

> 目标库。**已完成**的项见 `docs/knowledge/convert-*` 与 `docs/rounds/20–29` 的落地记录;这里只留**没做完/待核验**的。
> 决策类见 `pending-decisions.md`。

> **重构方案**:`docs/rounds/37-convert-architecture-refactor-plan.md`(架构归位 + 合并清单 + 性能工作单 P1–P11;
> 已含**先决阻塞**:`convert_bench` 的 SHA 基线在 34–36 轮后未刷新 ⇒ 现在无法证明"输出不变")。

> **后续优化方向盘点**(2026-09-26):`docs/rounds/37` §11 —— 按"证据 × 收益 × 风险"排,
> 含 profiler 证据(P6/少建中间 Value)、便宜防复发项(gitignore 卫生规则、性能门、`unused` 告警)、
> 覆盖度卡点(编辑格式抓包、实机门进 CI)与相邻待办。

## 1. 待你做决策才能动的

| 项 | 出处 | 说明 |
| -- | ---- | ---- |
| ~~A1 大作品上传必失败~~ ✅ **已修(2026-09-26)**:`MewRequestBuilder::with_timeout` + 上传路径 `UPLOAD_TIMEOUT = 600 s`(实测 30 MB 请求跑 228 s 未被掐断) | `docs/rounds/21` §8.4 N1 | 剩下的是**单包上限**(qiniu 413,落在 9.3~30 MB 之间)⇒ 见 `pending-decisions.md` A5 |
| ~~A2 `keep_source` 上传的是**反编译重建的编辑版**~~ ✅ **已决(2026-09-26,方案① 文档化)**:`TranslateOptions::keep_source` 的 rustdoc 写明偏差,不改反编译侧 | `docs/rounds/21` §8.4 N2 | 无(名实不符已写进文档) |
| B2 KN → NEMO 是否立轮 | `docs/rounds/24` §12.3、`docs/rounds/27` §1 | 建议不做(平台无对照;要自建 NEMO 编码器) |
| **A4 NEMO 是否真机验证"上传到账号"** | `docs/rounds/30` §4、`docs/rounds/24` §13.4 | 链路已就绪(渠道 + 编排 + 选项);只差"敢不敢建一份擦不掉的 NEMO 草稿"(KN 侧已端到端验证) |
| `entity_concurrency` 是否对**大作品自动开** | `docs/rounds/25` §9 | 现在默认 1,由使用者显式传;自动开需定阈值 |
| 反向(KN→Kitten4)并行将来是否重开 | `docs/rounds/25` §7 #1 / §10 | 重开的前提是**三段重设计**(拆 `unrewrite_calls` 的全局依赖);数据不支持直接做 |

## 2. 待做(有方案,不等决策)

0. **反向(KN → Kitten4)保真的量化口径(现行,2026-09-26)**:
   - 编辑器不认识的类型**必须剔除/清空**(否则整份工作区加载失败,rounds/34 §4nonies)⇒
     "未映射/不认识"现在是**真的少了**;两个真作品实测(rounds/36):`Node VM v3` 剔块 429 / 清影子 4、
     `now` 剔块 286 / 清影子 46;
   - 定义体侧现行门是**预算 `≤3193`**(rounds/36;此前 0/0 的口径已被剔块机制推翻);
   - 实体侧 `pure_list_get` 影子丢失**已修**(rounds/32 §3.4);列表影子那 86+192 是代理指标造成的假象
     (rounds/34 §4ter/§4quinquies,引用零丢失);
   - ⇒ 剩下的减损空间**只剩"语义降级"**这一条路(见 `pending-decisions.md` D1)。

1. **反向保真缺口的"研究史"(已收口,留档防止重开)** —— 现行结论见上面第 0 条:
   - **口径三次修正**:① 旧口径把 `proceduresDict` 里的**调用树**当定义体比 ⇒ 凭空 118 块假缺口(rounds/32 §3.2);
     ② 残块(没人挂的 `callreturn`/`repeat_n_times`/`script_variables`/`callnoreturn` 簇)会在反向重建树时自然消失,
     属**正确行为** ⇒ `def_census` 改成只数**定义根子树**,预算一度 `≤6/≤21 → 0/0`(rounds/33 §3bis);
     ③ rounds/34 起"编辑器不认识的类型必须剔除"⇒ 0/0 不再成立,现行预算 `≤3193`(rounds/36)。
   - **实体侧的两处"看着像 bug"都已定性**:inline `pure_list_get` 影子**真丢 −24**(`now` 186→162)已修
     (根因在**正向**的列表影子步骤写在子块循环体内,只处理有连接的块;rounds/32 §3.4 提交 `0dce9d6`);
     列表影子 86+192 是代理指标 + id 重铸造成的假象,列表 id 三态一致 24/24/24 ⇒ **引用零丢失**
     (rounds/34 §4ter/§4quinquies)。
   - **已知结构性(不是缺陷)**:Kitten4 没有 **list 类型的程序集参数** ⇒ `param(type=List)` 必丢;
     `script_variables` 子树等 Neko 专有能力无对应概念(⇒ 见 `pending-decisions.md` D1)。
   - **仪器**:`kn_corpus_round_trip_sweep`(反向,吃任意 `download/compile/*.bcmkn`)、
     `k4_corpus_round_trip_sweep`(正向,吃 **`download/compile/*.bcm4`** —— 早前文档写的 `k4raw/*.json` 是漂移)、采集器
     `tests/convert_corpus_harvest.rs`(`#[ignore]`)。
   - **仍然卡住的一步**:正向扫描器要的**编辑格式**平台拿不到 —— `player/load` 是编译态(喂进去会静默产出空 KN,
     实测 654→13),`kitten/r2/work/edit/load/*` 与 `kitten/work/ide/load/*` 都 404,`source/public` 对 Kitten 报 422;
     编辑格式只存在于**编辑器保存时的载荷**里 ⇒ 需要浏览器会话抓包(形态与两次误判见 rounds/33 §2;
     扫描器已加"缺 `block_data_json` 即跳过"的形态守卫)。
2. **NEMO 侧内存入口**:按 `docs/rounds/27` §2,应像 KN 侧一样把编辑版 `Value` 直接交给 translate,避免"落盘→读回"。落地情况 **[待核验]**。
2a. ✅ **已完成(2026-09-26)**:单包上传上限实测 **20 MB 可传 / 24 MB 413**(同渠道逐档),并加了
   `shared::ensure_single_package_fits` 提前报错闸(>20 MB 直接给出"上限 + 实测值"的错误)。
   要传更大作品需**分片上传**(qiniu 支持),目前无此需求(真实 KN 产物 3~9 MB)。
2b. ✅ **已完成(2026-09-26)**:convert 上传前取 preview 仍走全局客户端:`src/core/convert/mod.rs` 用 `WorkDataFetcher::new()`
   取作品 `preview` —— 已改为:`DecompiledArtifact::Document` 随产物带出 `preview`(反编译阶段本就拿到),
   建草稿不再重拉详情 ⇒ 省一个 RTT,那处全局客户端依赖随之消失(`915c8ff`,rounds/37 P10)。
3. **P3 结构化失败记录**:`decompile/mod.rs` 的资源下载失败重试用 `line.split(": ").next()` 从错误串反解 URL(URL 或文本含 `": "` 会截断)⇒ 改成结构化 `(url, error)` 记录,直接消掉反解(`docs/rounds/29` §4)。
4. **重连放弃的文档化**:云变量重连 5 次后仅 warn 并永久放弃,且**不再发事件**(仅初始 `Closed`)⇒ 调用方可能无限等待,需在 rustdoc 写清(`docs/rounds/29` §4)。
5. **转换域 P2**:`simple.rs` 的 `Arc<Value>` 可 `Arc::try_unwrap` 免拷(`docs/rounds/29` §3-4)。
6. `docs/rounds/21` §6「明确遗留(不在本轮)」与原方案 §4/§5 的未勾选项 —— 以该文为准逐条过一遍(本轮未逐条核验)。
7. **NEMO"完整搬家"还缺资源重传**:当前"上传到账号"只传 `.bcm`,造型/音频仍指向源 CDN URL;
   要让新作品自带资源,需"逐资源上传 + 文档内 URL 改写"(官方 App 保存时上传 ~1390 个文件)。
   KN 侧无此问题(积木引用的是造型 id,资源在作品文件内/平台侧)。见 `docs/rounds/30` §4。

8. **根块纵向布局的 0 vs 80 疑似漂移(可能是个真 bug)**——反编译侧 `XmlBlockWriter` 根块从 `y=0.0` 起、步长 220;
   `translate/model.rs` 另定义 `ROOT_LAYOUT_Y=80 / STEP=220` 并注释自称"与 `XmlBlockWriter` 的约定一致",
   但起点不一致。**先核实 0 与 80 哪个是对的**(对官方产物取样),再决定共享常量或修一边。
   出处:第三十一轮只读审计(`docs/rounds/31` §3.6 N4)。
9. ✅ **已完成(2026-09-26,`b7d4e07`)**:生成物里的零消费者常量 `translate/tables_gen.rs` 的 `TOP_BLOCKS` / `KN_TYPES` 已删(含生成器同步;顺带发现 `.gitignore` 的 `bin/` 通配误伤 `src/bin/`,生成器此前**从未入库** ⇒ 已修)。原记录:全仓无使用点。
   注意它们是 `src/bin/gen_translate_tables.rs` **整文件生成**的 ⇒ 要删得改**生成器**再重新生成,
   否则下次生成又回来(顺带核对生成器与手工表的分工)。
10. **零调用私有项**(审计 §3.6;2026-09-26 复核后收口):`BlockJson::count_types` / `BlockTree::count_types` ——
    复核结论:二者**仅测试/仪器用**(`reverse_tests` 的 census),`BlockJson::walk` 与 `BlockTree::walk` 是**递归核心**、
    有生产调用(`assembly::duplicate_ids`)⇒ 按仓库口径"**保留并注明**",不删。原记录:
    `nemo::parse`(仅测试用)、`DecompilerContextBuilder`(已随骨架瘦身删除)、`TOP_BLOCKS` / `KN_TYPES`。
    处理口径:仅测试用 ⇒ 标 `#[cfg(test)]` 或保留并注明;完全不用的 ⇒ 删(删除前按仓库约定确证零调用)。
0b. ✅ **已完成(2026-09-26)**:词汇表新鲜度:`kitten4_vocab.rs` 的 349 条是 2026-09-26 从线上编辑器导出的快照;
   编辑器升级后名字会漂移(名字认错 = 整份打不开)。待做:把"重导 + 整体替换"写成一个可复跑的小流程
   (浏览器一句 `Object.keys(window.Blockly.Blocks).sort()`,方法见 §5bis 的"判据与证据来源"),
   并在注释里记下导出日期与命令(现状只记了日期)。
0c. ✅ **已落地(2026-09-26)**:剔除量的预算门:目前只有定义体侧有预算断言;块/影子的剔除量还没有门
   (rounds/36 的 942 → 715 / 398 → 50 是靠 A/B 人工比出来的)。待做:把"每件作品的剔除块数与影子数
   ≤ 记录值"写成断言(先对 `download/compile/*.bcmkn` 全语料测一遍记录基线)。
11. **剩余重复项的定性结论已归档**(`docs/rounds/31` §3.6):`D2` 族 JS 值强转、`N3` Fetcher/ResourceManager 样板、
    `N4` 布局常量属**不可合并或需先核实**;`D3`/`N1` 已合并。今后不要重新提"把这些也合一"。

12. ✅ **已完成(2026-09-26)**:`类型歧义` 借用 `DroppedProperty` 上报(`mapping.rs` 的"Kitten 原类型有 N 个…"分支),
    导致类别标签「丢弃实体属性」与事实不符(它其实不是丢属性,而是"KN 一个类型 ← Kitten 多个源类型")。
    —— 已按方案 ① 落地:新增 `TranslateWarning::AmbiguousType { kind, candidates, chosen }`(公开枚举加变体,
    预 1.0 可接受),并计入 `is_lossy`(Kitten 原类型名不可恢复)。
    报告类别因此从「丢弃实体属性 3301」修正为「类型歧义 2959 + 丢弃实体属性 342」。

## 3. 已在案、不做的(别再重开)

- `RawValue` 顶层只透传、单遍遍历合并(`docs/rounds/26` §6):透传占比 ≈0%、正向本已 2 趟。
- 反向实体级并行(`docs/rounds/25` §10):Amdahl 上限 1.9×,反向 `core` 仅 ~200 ms。
- 产物**逐字节对齐官方**:官方按键插入序、id 随机 ⇒ 只做语义 diff + `validateBcm` 硬门。
- 编译版块引用**不做**字符串容错(`docs/rounds/21` §7-1:真样本 2236 处采样全是内联对象;遇字符串显式报错)。

## 4. 需要留意的既有防线(改动前先看)

- `tests/convert_live.rs`(真机,默认 `#[ignore]`;写平台的用例会建"可删"草稿)。
- `tests/convert_bench.rs`(自有 SHA256 基线:任何产物字节变化都必须先解释再接受)。
- `reverse_tests` 里的往返多重集守恒 + 缺口预算断言(只许变小;每次跑打印 `[预算]` 读数)。
- **实机门(最强)**:无头 Chromium + 线上 Kitten4 的「打开本地作品」—— 数画布积木 + 看角色列表,
  必须带一个已知能读的对照组(方法留档见 `docs/rounds/35`;配方也可从 `docs/knowledge/convert-semantics.md` §5bis 反查)。
- 官方校验器 `BcmHelpers.validateBcm`(headless 可跑)是"产物能否被编辑器加载"的硬门。
