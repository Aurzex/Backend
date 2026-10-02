# 第二十六轮方案 — convert 文档级零拷贝透传与单遍遍历(方案 23 S4 拆分)

日期:2026-09-25 · 基线:`d355cbe` · 是 `docs/rounds/23` §3 P1-6 / P1-7 的子方案

> **状态:判不做。方案阶段只出方案并先做成本低的可行性预研;两项均属"大改",执行前需要先决定对齐口径(评审结论见 §6)。**

---

## 1. 两块要解决的问题

### 1.1 P1-6 `RawValue` 零拷贝透传(方案 23 §3 P1-6)

现状(`translate/mod.rs` `translate_value` 的输入形态):整份编辑版 JSON 一进来就被
`serde_json::from_str` 解析成 `Value`,装配时再把整份文档序列化回去。文档里**大部分字段
只透传不修改**(`styles` / `audios` / `variables` / `cloud_variables` / `broadcasts` /
`scenes_order` / `size` / `stageSize` / `aiImageUrls` …),却付出完整的解析与重编码代价。

方案:打开 `serde_json` 的 `raw_value` feature(**不是新依赖**,既有依赖加 feature),
顶层改"类型化外壳与原始切片":

```text
RootShell { theatre: TheatreShell, styles: Box<RawValue>, audios: Box<RawValue>, … }
```

只对必须重写的部分(`theatre.actors[*].block_data_json` / `theatre.scenes[*]…`)真解析。

### 1.2 P1-7 单遍后序遍历(方案 23 §3 P1-7)

正向每棵树现在要经过 `parse_node`、`route_children`、`gc_deep` 三趟以上,反向另有
`unwrap_arithmetic_wrappers`、`fold_pure_list_get`;且部分步骤(如 `gc_node`)依赖"子树已被前一步改过"。

方案:先写**步骤依赖表**(哪一步读哪些字段、依赖哪些子步骤已完成),再按依赖分层合并成
1–2 趟后序遍历。属于"改对了更快、改错了产物就变"的高风险项。

---

## 2. 先做预研(成本低,且决定是否实施)

**预研 A:透传字段占比**(决定 1.1 的上限)。对四个样本统计"始终不触碰的顶层字段"
占文档字节的比例;若低于 30%,`RawValue` 的收益不足以支撑"顶层读写路径重写"的风险,
第二十六轮标为"不做"。

**预研 B:遍历趟数占比**(决定 1.2 的上限)。给 `mapping` 的每个遍历步骤套上计时
(`#[cfg(feature="bench")]` 或基准里用 `Instant` 包裹调用点),量出各趟占 `core_ms` 的比例;
只有"合并后能节省不低于 15% 的 `core_ms`"才值得实施。

**预研 C:键序口径**(决定 1.1 是否可接受)。现状:输出用 `serde_json` 默认(BTreeMap,
即**键按字典序排列**,`to_string`/`to_writer` 都是)。`RawValue` 保留**源字节序**,即引入
"有的子树排序、有的不排序"的混合口径。必须先回答:

- 官方产物是否排序?现有差分门夹具(`tests/fixtures/translate/geoduel_scene_actor.json`)与
  官方基线切片能否回答?
- 若官方**排序**,则 `RawValue` 会把"与官方逐字节对齐"改成"与源文件逐字节一致" —— 这是
  口径变更,需要明确批准,并同步更新差分门基线;
- 若官方**不排序**,则现状的排序实为"自定义规范化",`RawValue` 反而更保真 —— 但
  现状产物会变(所有下游基线都要重录)。

---

## 3. 验收(任一项落地时)

1. `tests/convert_bench.rs` 四个样本 SHA256 与基线**逐字节相同**;若口径变更(§2 预研 C),
   则改为"与**新的**基线逐字节相同、差异逐条解释、差分门同步更新";
2. 差分门与往返多重集守恒、真机门(`tests/convert_live.rs`)全部通过;
3. 绑核基准:`core_ms` 与 `e2e_ms` 都必须可复现地下降(单遍遍历按趟数预期 20–40%;
   `RawValue` 按预研 A 的占比外推),否则回滚;
4. `RawValue` 路径需要一条"**未触碰字段原样透传**"的单测(构造含奇怪键序/空白的输入,
   断言产物里该字段逐字节等于输入切片)。

## 4. 风险

| 风险 | 缓解 |
| --- | --- |
| 口径变更(键序/空白) | §2 预研 C 先定论;不允许"无解释的产物变化" |
| `RawValue` 与"必须重写字段"混用时的插入顺序 | 顶层外壳固定键序,`RawValue` 只放在"整块透传"的位置 |
| 单遍合并顺序耦合写错 | 先写步骤依赖表(§1.2),按依赖分层;每层单独提交、每层都验产物不变 |
| 两条大改互相影响 | 顺序执行:先 1.2(纯内部遍历合并,产物应完全不变),再 1.1(口径敏感,单独一轮) |

## 5. 建议顺序

1. 预研 A/B/C(各不超过半天,只读代码与统计,不改行为);
2. 若 1.2 的预研 B 达到门槛,则先做单遍遍历(产物不变,风险可控);
3. 1.1 仅在预研 A 占比高且口径获批后才做;
4. 两项均**不阻塞** `docs/rounds/25`(实体级并行):并行是"同代码多线程",与文档级重构正交,
   先并行收益更大(方案 23 §4 顺序建议一致)。

---

## 6. 子代理评审结论(2026-09-25,ReviewS4RawValue):两项都**判不做**

评审实际完成了三项预研,结论推翻本方案的两个前提,也推翻 `docs/rounds/23` §1 的一处数字:

### 6.1 预研 A(透传占比)约 **0%**,故 `RawValue` 上限约 0%

| 字段 | 四样本字节占比 | 装配期实际处理 |
| --- | --- | --- |
| `theatre`(含 `block_data_json`) | **99.2%–99.5%** | 每个实体全部重写并重新编码 |
| `styles` | 0.05%–0.60% | `build_styles` 把 `center_point`/`rotate_center` 改为 `centerPoint`、`cdn_url` 改为 `url` |
| `audios` | 0.05%–0.13% | `build_audios` 把 `cdn_url` 改为 `url` |
| `variables` / `cloud_variables` | 0.12%–0.80% | `build_variables` 重算 `position`/`style`/`type`/`createTime` |
| `broadcasts` | 不超过 0.02% | `build_broadcasts` 包 `broadcastsDict` |
| `size` / `scenes_order` | 0.00% | **只读输入**,不输出 |
| `stageSize` / `aiImageUrls` / `resourceZip` | — | **输出期计算/硬编码**的字段,不是输入 |
| 反向唯一真透传 | `toolbox`/`hidden_toolbox` 不超过 0.01% | `build_kitten4_document` |

因此本方案 §2 预研 A 的门槛为 30%,实测约 0%,**判不做**(方案自身的规则:无数据不做)。

### 6.2 预研 C:不存在"与官方逐字节对齐"的门,口径变更的对象是**自有基线**

- `serde_json` 未开 `preserve_order`(BTreeMap,输出按键排序);
- 差分门(`translate/mod.rs` `diff_tests`)只做**语义**比较(类型计数、逐 id 的 type/fields、槽名集合),
  文档里明确写了"为什么不做整段 JSON diff";官方夹具的块键序是**插入序**(`type,id,location,shield,mutation,next`),
  即官方产物**不排序**,本实现**从未**与之逐字节对齐;
- 唯一字节门是 `tests/convert_bench.rs` 对**自有** `convert_bench_baseline.json` 的 SHA256。

因此本方案 §2 预研 C 的二分前提("官方若排序…")不成立;`docs/rounds/23` §1 的"90% 字节只透传"与
"逐字节对齐官方"两处措辞已在 `docs/rounds/23` 就地更正。

### 6.3 预研 B:正向是 **2 趟**全遍历,合并收益低于门槛

- `translate_kitten_to_kn` 由 `parse_node`(内含 `route_children` 内联)与 `gc_deep` 组成;
- `gc_node` 读的是"子树已 **parse**"的状态(`shadow_number` 拆包、`list_append` 降级、
  横屏包装读子节点 `location`),**不能**与 `parse_node` 同向前序合并;`shadow_number` 还自递归调用
  `gc_node`,改为后序会**双重应用**;
- 反向 `translate_kn_to_kitten` 本已单遍(`reverse_node` 先 `unwrap_arithmetic_wrappers`/`fold_pure_list_get` 再递归);
- 合并遍历不减少逐块 `kind` 匹配 / `transform_shadow_xml` / `map_field_name` 这些**主要**成本,
  达不到本方案 §2 预研 B 的 15% 门槛,**判不做**。

### 6.4 结论

`docs/rounds/26` 两项(P1-6 / P1-7)**均不执行**;若将来需要再评估,必须先有新的证据(例如某类作品
的装配期确实存在大块透传字段,或遍历占比量测达到门槛)。量测办法(评审给出的可行做法,不改产物路径):
在 `mapping.rs` 用 `#[cfg(test)]` 的 `Instant` 分别包住 `translate_kitten_to_kn` 里 `parse_node` 与 `gc_deep`
两个调用点,先量占比再决定。
