# 第 40 轮:夹具目录纪律 —— bench 样本不得放在输出目录里

> 提交:`9290ede`。一次"真机门把 bench 输入夹具覆盖掉"的事故、修法与基线处置。

## 1. 事故:跑一次真机门,把 bench 输入夹具覆盖了

`tests/convert_bench.rs` 的样本 `kn-9.4MB` 原先放在 `download/convert/`:

- **读**:`tests/convert_bench.rs:238` 与 `tests/convert_facade_bench.rs:42` 都读
  `download/convert/Phigros 自制谱模拟器_195038626.kn.bcmkn`(两个 bench 都只读不写)。
- **写**:`download/convert/` 就是 `PathConfig::convert_file_path()`(`src/utils/filedata.rs:67-69`),
  而它是**转化域门面的默认输出目录**(`translate/mod.rs::product_path`:未给 `output_dir` 时回退到它);
  另外 `translate_work` 的中间目录 `staging_dir`(`src/core/convert/mod.rs`)**恒**在
  `download/convert/staging/` 下,**不受 `TranslateOptions::output_dir` 影响**。
- **触发**:真机门 `tests/convert_live.rs::translate_work_creates_draft_when_ignored` 调的
  `translate_work(..., TranslateOptions::new().upload(true))` 没给 `output_dir` ⇒ 产物按
  `<源文件名主干>.<slug>.<ext>` 口径落成同名同路径,**整体覆盖夹具**。

即:**输入夹具目录 = 库/门面的默认输出目录**。这是一条**必然**撞车的结构,不是偶然。
旧字节在 `.gitignore` 的 `/download` 下,**不可恢复**(已在全盘按 size=9357732 找过,无副本)。

## 2. 修法(选中):把夹具挪出输出目录

候选 (a) 让真机门写独立输出目录 vs (b) 把夹具挪到只读目录。**选 (b)**,因为 (a) **不足以**满足判据
「夹具目录里不允许有任何测试会写」:

- `staging_dir` 恒在 `download/convert/staging/` 下,夹具只要还在 `download/convert/`,该目录就仍被写;
- 更根本:`download/convert/` 是**库的默认输出目录**,任何调用方不给 `output_dir` 都会往里写。

落地:

1. 夹具移到 **`download/fixtures/`**(= **只读夹具目录**,仓库里没有任何写点;仍 gitignored)。
2. `tests/convert_bench.rs` 与 `tests/convert_facade_bench.rs` 的样本路径指到新位置(仅两个读点);
   在样本处加注释写明"夹具目录纪律"。
3. 顺带修真机门自身与文件头约定的不一致:`convert_live.rs` 头部写「输出写系统临时目录,不污染仓库」,
   而该 ignored 门没给 `output_dir`(另一个 ignored 门本来给了临时目录)⇒ 现在补 `.output_dir(&work_dir("draft"))`。
   **判据依据是 (b)**,这条只是让该门遵守自己声明的约定。

没动 `PathConfig` / 任何公共签名。

## 3. 基线处置:有据刷新 `kn-9.4MB` 的两个键

### 3.1 先更正一个说法:产物 SHA **变了**

任务前提里的"产物 SHA256 仍然与基线一致"**不成立** —— 那份对照只截了 `#meta`,而产物 SHA 那一行
**确实是红的**:

| 基线键 | 旧(刷新前) | 新(刷新后) |
| --- | --- | --- |
| `kn-9.4MB-kitten4`(产物 SHA256) | `dac08917…` | `0e873b2b…` |
| `#meta.source_bytes` | 9357732 | 9357804 |
| `#meta.source_sha256` | `b4af1e83…` | `c5881f55…` |
| `#meta.warnings` | 1827 | 1828 |
| `#meta.output_bytes` / `blocks_total` / `blocks_converted` | 9775778 / 5235 / 4624 | 同左(不变) |

**为什么之前只看到 `#meta` 红**:`tests/convert_bench.rs` 的断言顺序是 `meta_mismatched` 先于
`mismatched` panic ⇒ `#meta` 一变,产物 SHA 的门**根本来不及报**。只看输出会以为"产物没变"。
(该断言顺序**已修** —— `636127f`,见本文 §6。)

### 3.2 产物 SHA 为什么会变 —— 输入快照换了,不是行为变了(已证)

钉住的事实:

1. 夹具里实体/积木 id 是**随机 UUID**(实测 3430 处;`TranslateOptions::deterministic_ids` 默认
   **false**)。真机门用默认选项 ⇒ 每次产出的快照 id 都不同。
2. 反向 KN→Kitten4 会把源实体 id 带进产物(`assembly.rs` 的 `"id": source.get("id")`)
   ⇒ **产物 SHA 是输入快照的函数**。验证(临时探针,已删):把夹具里 1 个 actor id 改成别的值
   (同长度),产物 SHA 立刻从 `0e873b2b…` 变成 `21b2195a…`。
3. `source` 字段**不影响**产物:夹具与其"只删掉 `source` 成员"的变体(72 字节)都得到同一产物
   `0e873b2b…`(后者 warning 少 1:反向把 `source` 当 `DroppedProperty` 丢掉)。
4. **关键对照**:在**最后一次刷新基线的那次提交 `6057a88`** 上重新编译跑同一探针 ⇒ 同一输入
   同样得到 `0e873b2b…`(与 HEAD 逐字节相同)⇒ 自 `6057a88` 起,反向产物代码**没有**改变过
   该输入的结果;`dac08917…` 只能是**旧快照**(换了整套随机 id)的产物。

结论:**变的是输入快照,不是转换行为**。新旧夹具是同一作品(195038626)的两次转化快照
(块数 5235→4624、产物字节 9775778、toolType KN / version 0.16.2、55 角色 / 4 场景都对得上),
差异在随机 id(以及多出的 `source` 引用)。

### 3.3 刷了什么、代价是什么

- 只改 `kn-9.4MB-kitten4` 与 `kn-9.4MB-kitten4#meta` **两个键**;其余 5 个样本逐字节不动
  (用 `BACKEND_BENCH_REFRESH=1` 跑出正确新值后,把其它样本的 record-only `alloc_*` 读数还原成原值,
  保持最小 diff)。
- **代价(必须记住)**:`kn-9.4MB` 的基线钉死在**"真机门从作品 195038626 生成的、带随机 UUID id
  与 `source` 引用的一次性快照"**上,而不是"某次刻意采集、可复现的文件"。它的产物 SHA 是
  **快照冻结门**,不是"可复现性门":任何重新采集(或再跑一次真机门)都会得到不同的 id ⇒
  产物 SHA 必变 ⇒ 必须再走一次**有据刷新**。这与刷新前的情形同类(旧夹具也是这种快照),
  区别只是换了一份快照。其余 5 个样本仍来自可控采集(`download/compile/**`),不受影响。
  (该"两种门的区别"已写进 `../knowledge/convert-performance.md`。)

## 4. 同类撞车(本次**只报不改**)

**`download/compile/` 是同一类结构问题的另一个实例**:它既是

- **写点**:反编译默认输出目录(`decompile/config.rs` 的 `default_output_dir`)、语料采集器
  `tests/convert_corpus_harvest.rs`(含 `player-load/`)、编辑格式采集器 `tests/convert_edit_harvest.rs`(`k4edit/`);
- 又是 **byte-baselined 夹具/语料**:`convert_bench` 5 个样本、`convert_facade_bench` 4 个样本、
  `convert_work_bench` 的 `RAW_SAMPLE`,以及 `reverse_tests.rs` / `nemo_tests.rs` /
  `translate/mod.rs` / `pipeline.rs` 的多条真作品门,还有两个**默认跑**的往返扫描器遍历整个目录。

采集器是**按发现流增量写新文件**、不覆盖既有夹具,所以当前没发生事故;但"默认输出目录"这条通路仍在:
以默认选项反编译**恰为夹具那几件作品**的调用会按同名口径覆盖夹具。彻底消灭得把
"语料/夹具目录"与"输出目录"分开(与本次 (b) 同思路),属较大的布局约定变更 ⇒
已登记为待办(`../goals/convert-backlog.md` §2,标"未修、有风险、需方案")。

## 5. 验证

提交 `9290ede`(只含 4 个文件:`tests/convert_bench.rs`、`tests/convert_facade_bench.rs`、
`tests/convert_live.rs`、`tests/fixtures/translate/convert_bench_baseline.json`)。

四道门全绿(退出码均为 0):`cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` /
`cargo test` / `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored`
(末者输出 `产物 SHA256 与元信息都与基线一致 ✅`)。

6 样本产物 SHA256(与基线逐项相同;其中 5 个自始未变):

| 样本 | SHA256 |
| --- | --- |
| `kitten4-10.8MB-kn` | `bafeb50c…` |
| `kitten4-0.3MB-kn` | `d653a8a5…` |
| `kn-9.4MB-kitten4` | `0e873b2b…` |
| `kn-3.7MB-kitten4` | `0d3cf2e3…` |
| `nemo-3.4MB-kn` | `67641a0a…` |
| `nemo-old-1.5MB-kn` | `4b6038f9…` |

撞车已消灭的旁证:移入 `download/fixtures/` 的那份夹具 = 9357804 字节、
sha256 `c5881f55…`(与刷新后的 `#meta.source_sha256` 一致)⇒ 基准跑完后**夹具字节未被动过**;
`download/convert/` 现在只剩空的 `staging/`。

## 6. 第二处缺陷:断言顺序把红遮住(已修)

> 提交 `636127f`(只改 `tests/convert_bench.rs`,+269/−28)。这是 §3.1 那个"只看输出会以为产物没变"
> 的**根因**。

**症状**:`tests/convert_bench.rs` 里 `#meta` 不一致先 panic、产物 SHA 不一致后 panic ⇒ 产物 SHA 的红
**永远看不到**。§3.1 的误判就是这么来的。

**修法**:

- 比较逻辑抽成**纯函数** `baseline_mismatches(key, 基线产物 SHA, 实跑产物 SHA, 基线 `#meta`, 实跑 `#meta`)
  -> `Vec<BaselineMismatch>`:一次返回**全部**不一致项 —— 产物 SHA 一项 + `#meta` 每个**参与断言的**键各一项
  (按字段名排序;**键被加/删也算**,缺失渲染成 `<缺>`)。
  **记录键(`alloc_*`)仍不参与断言**(与既有"只记录不判"口径一致,否则跨机抖动会假红)。
- 两类红**分开标注**(这是"可操作"的核心):
  `[产物 SHA256 | 行为/产物变了]` 与 `[#meta.<字段> | 输入夹具被换 / 元信息漂]`;
  报告开头点清"产物 SHA256 N 项 / `#meta` M 项",随后逐项给出 **样本名 → 键 → 基线值 → 现在值**。
- `render_baseline_mismatches`(纯函数)负责这份"一次列全"的报告文本,便于测试。
- **行为不变(判据等价)**:通过/失败仍只看"不一致项是否为空"(等价于原来的"任一非空即 panic");
  并发腿不一致(1 vs 8 产物不同)仍是**独立**断言,且仍在 refresh 分支**之前** panic(相对顺序未动);
  pass 分支打印的绿字不变。

**测试**(非 `#[ignore]`,全部用**合成输入**,不动真基线;该集成测试目标现有 **3 个**测试):

- `baseline_mismatches_reports_product_and_every_meta_key_together`:构造"产物 SHA 与 `#meta`
  **同时**不一致"(正是实测那次的形状),断言 ① 产物项在列表里 ② `#meta` 逐键报、记录键不进、
  按字段名排序 ③ 报告文本同时含两类标签 + 样本名 + 期望/实际 ④ 共 3 项。
- `baseline_mismatches_is_silent_without_baseline_or_when_equal`:基线缺键 ⇒ 不报(归 W3a/W3e 的
  加载与重刷守卫);完全一致 ⇒ 空列表(不误报)。

**变异验证**(证明测试真有牙齿):临时在 `baseline_mismatches` 里插入"首个错即停"
(`if !out.is_empty() { return out; }`),`cargo test --test convert_bench baseline_mismatches`
⇒ **FAILED**(exit 101,`left: []` / `right: ["source_bytes", "source_sha256"]`);还原后绿。

**门**:`fmt --check` 0 / `clippy --all-targets -- -D warnings` 0 / `cargo test` 0 /
`BACKEND_REQUIRE_BENCH=1 … convert_bench --ignored` 0(6 样本含 `kn-9.4MB` 与基线逐项一致)。
