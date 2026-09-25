# 转换域待办(Kitten/KN/NEMO 作品互转)

> 目标库。**已完成**的项见 `docs/knowledge/convert-*` 与 `docs/rounds/20–29` 的落地记录;这里只留**没做完/待核验**的。
> 决策类见 `pending-decisions.md`。

## 1. 待你做决策才能动的

| 项 | 出处 | 说明 |
| -- | ---- | ---- |
| A1 大作品上传必失败(≈9 MB 超全局 30 s 超时,实测 31.2/35.5 s) | `docs/rounds/21` §8.4 N1 | 上传需独立超时或分片;挡着 `translate_work(upload=true)` 的可用性 |
| A2 `keep_source` 上传的是**反编译重建的编辑版**,非原始字节 | `docs/rounds/21` §8.4 N2 | 平台的"保留原件"因此打不开原件 |
| B2 KN → NEMO 是否立轮 | `docs/rounds/24` §12.3、`docs/rounds/27` §1 | 建议不做(平台无对照;要自建 NEMO 编码器) |
| **A4 NEMO 是否真机验证"上传到账号"** | `docs/rounds/30` §4、`docs/rounds/24` §13.4 | 链路已就绪(渠道 + 编排 + 选项);只差"敢不敢建一份擦不掉的 NEMO 草稿"(KN 侧已端到端验证) |
| `entity_concurrency` 是否对**大作品自动开** | `docs/rounds/25` §9 | 现在默认 1,由使用者显式传;自动开需定阈值 |
| 反向(KN→Kitten4)并行将来是否重开 | `docs/rounds/25` §7 #1 / §10 | 重开的前提是**三段重设计**(拆 `unrewrite_calls` 的全局依赖);数据不支持直接做 |

## 2. 待做(有方案,不等决策)

0. **反向(KN → Kitten4)保真的量化口径**(真作品 `325806995`):`未映射积木 643 · 丢弃实体属性 3301 · 丢弃字段 12`
   (`docs/knowledge/convert-semantics.md` §5)。
   ⚠️ **两个口径都要小心**(数字为拆分类别后的最终值):① 643 个未映射积木**不是缺口**(全是 KN 扩展能力,保留原类型名,不丢积木);
   ② 丢弃类 3 313 条 = **类型歧义 2 959 + 实体属性 342 + 字段 12**(类别已拆分,见第 12 条)。
   ⇒ **反向的产物丢失实际很小**;剩下真正可评估的是 round 28 的定义体类型差(6/133)。
1. **反向保真缺口的研究路线**(`docs/rounds/28` §4;**进展见 `docs/rounds/32`**):
   - 已复现并细化:净减 133 里 **118 来自同一条定义**(`bfa2f83c-…`,定义体几乎整体消失);
     其余为零碎(3~5 块),另有一条定义是**净增**(非损失)。
   - 已排除一个"看着像 bug"的结构性因素:该作品里 `type=List` 的**程序集参数**被丢 97 次 ——
     Kitten4(Scratch 派生)没有 list 参数 ⇒ 属格式能力差,不是实现缺陷。
   - **下一步(必须先拿中间态再动手)**:
     ① 在同一测试里对**中间态 K4 文档**也做定义体 census ⇒ 判定"是反向丢还是正向丢";
     ② 把那条定义 + 宿主实体切成 `tests/fixtures/` 夹具,让修复有秒级反馈;
     ③ 定位后:是缺陷就修并把预算收紧到 0;是结构性的就从预算里剔除并写明理由。
   - 原步骤(供参考):
   ① 把官方 JS 跑起来(harness 方法已在本仓验证过,见 `docs/rounds/20` §9),对同一份 KN 做**逐字段 diff**;
   ② 对"最严重的定义体"用官方编辑器打开,判断是**我们丢**还是**官方也这么丢**(若是后者,只需更新 allow-list);
   ③ 修完把回归预算从 **受影响定义 ≤ 6 / 净减块 ≤ 133 收紧到 0**,并把 `pure_list_get` 从 allow-list 移除。
   已知症状:深层 `next` 栈编码(假设一)、inline `pure_list_get` 影子(假设二)、`script_variables` 子树(假设三)。
2. **NEMO 侧内存入口**:按 `docs/rounds/27` §2,应像 KN 侧一样把编辑版 `Value` 直接交给 translate,避免"落盘→读回"。落地情况 **[待核验]**。
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
9. **生成物里的零消费者常量**:`translate/tables_gen.rs` 的 `TOP_BLOCKS` / `KN_TYPES` 全仓无使用点。
   注意它们是 `src/bin/gen_translate_tables.rs` **整文件生成**的 ⇒ 要删得改**生成器**再重新生成,
   否则下次生成又回来(顺带核对生成器与手工表的分工)。
10. **6 条零调用私有项**(审计 §3.6「零调用点私有项」):`BlockJson::count_types` / `BlockTree::count_types`(仅测试用)、
    `nemo::parse`(仅测试用)、`DecompilerContextBuilder`(已随骨架瘦身删除)、`TOP_BLOCKS` / `KN_TYPES`。
    处理口径:仅测试用 ⇒ 标 `#[cfg(test)]` 或保留并注明;完全不用的 ⇒ 删(删除前按仓库约定确证零调用)。
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
- `reverse_tests` 里的往返多重集守恒 + 缺口预算断言(只许变小)。
- 官方校验器 `BcmHelpers.validateBcm`(headless 可跑)是"产物能否被编辑器加载"的硬门。
