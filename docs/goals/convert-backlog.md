# 转换域待办(Kitten/KN/NEMO 作品互转)

> 目标库。**已完成**的项见 `docs/knowledge/convert-*` 与 `docs/rounds/20–29` 的落地记录;这里只留**没做完/待核验**的。
> 决策类见 `pending-decisions.md`。

## 1. 待你做决策才能动的

| 项 | 出处 | 说明 |
| -- | ---- | ---- |
| A1 大作品上传必失败(≈9 MB 超全局 30 s 超时,实测 31.2/35.5 s) | `docs/rounds/21` §8.4 N1 | 上传需独立超时或分片;挡着 `translate_work(upload=true)` 的可用性 |
| A2 `keep_source` 上传的是**反编译重建的编辑版**,非原始字节 | `docs/rounds/21` §8.4 N2 | 平台的"保留原件"因此打不开原件 |
| B2 KN → NEMO 是否立轮 | `docs/rounds/24` §12.3、`docs/rounds/27` §1 | 建议不做(平台无对照;要自建 NEMO 编码器) |
| `entity_concurrency` 是否对**大作品自动开** | `docs/rounds/25` §9 | 现在默认 1,由使用者显式传;自动开需定阈值 |
| 反向(KN→Kitten4)并行将来是否重开 | `docs/rounds/25` §7 #1 / §10 | 重开的前提是**三段重设计**(拆 `unrewrite_calls` 的全局依赖);数据不支持直接做 |

## 2. 待做(有方案,不等决策)

1. **反向保真缺口的研究路线**(`docs/rounds/28` §4,三步,按序做):
   ① 把官方 JS 跑起来(harness 方法已在本仓验证过,见 `docs/rounds/20` §9),对同一份 KN 做**逐字段 diff**;
   ② 对"最严重的定义体"用官方编辑器打开,判断是**我们丢**还是**官方也这么丢**(若是后者,只需更新 allow-list);
   ③ 修完把回归预算从 **受影响定义 ≤ 6 / 净减块 ≤ 133 收紧到 0**,并把 `pure_list_get` 从 allow-list 移除。
   已知症状:深层 `next` 栈编码(假设一)、inline `pure_list_get` 影子(假设二)、`script_variables` 子树(假设三)。
2. **NEMO 侧内存入口**:按 `docs/rounds/27` §2,应像 KN 侧一样把编辑版 `Value` 直接交给 translate,避免"落盘→读回"。落地情况 **[待核验]**。
3. **P3 结构化失败记录**:`decompile/mod.rs` 的资源下载失败重试用 `line.split(": ").next()` 从错误串反解 URL(URL 或文本含 `": "` 会截断)⇒ 改成结构化 `(url, error)` 记录,直接消掉反解(`docs/rounds/29` §4)。
4. **重连放弃的文档化**:云变量重连 5 次后仅 warn 并永久放弃,且**不再发事件**(仅初始 `Closed`)⇒ 调用方可能无限等待,需在 rustdoc 写清(`docs/rounds/29` §4)。
5. **转换域 P2**:`simple.rs` 的 `Arc<Value>` 可 `Arc::try_unwrap` 免拷(`docs/rounds/29` §3-4)。
6. `docs/rounds/21` §6「明确遗留(不在本轮)」与原方案 §4/§5 的未勾选项 —— 以该文为准逐条过一遍(本轮未逐条核验)。

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
