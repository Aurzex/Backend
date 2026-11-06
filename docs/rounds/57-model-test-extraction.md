# 第五十七轮记录 — 2026-11-07 转换域职责收口(第 53 轮方案 A)

## 0. 一句话

`model.rs` 的 7 个测试模块外移到 `model/model_tests.rs`:文件由 3204 行降到 2331 行(回到 2500 行软上限内),
测试不再夹在代码中间。方案 A2(再按职责拆 3 个文件)按既定门槛判暂不做,以文件头五区注释替代。
纯搬迁:产物字节、报告与分配计数基线均不变。

## 1. 背景与判据

- `../knowledge/repo-conventions.md` §5:测试默认内联在文件末尾,「本体 + 测试 > 3 000 行」时独立成 `*_tests.rs`;
  单文件上限 ≈2 500 行。
- 2026-10-07 的方案(`../rounds/53` §3A)把这条工作拆成 **A1**(测试外移 + 归位)与 **A2**(按职责拆文件,可选):
  A2 的判据写明"若 A1 完成后仍认为不值,可只做 A1 + 文件头分区注释"。
- 2026-11-07 的读数:`model.rs` 3204 行(本体 ≈2 285 + 内联测试 ≈869,7 个模块),
  且 `mod streamed_writer_tests` 位于代码中部(rounds/39 W12b 的回退)。

## 2. 改动

### 2.1 测试外移(A1)

| 项 | 改前 | 改后 |
| --- | --- | --- |
| `model.rs` | 3204 行(含 7 个测试模块) | 2331 行(无测试) |
| 测试文件 | — | `model/model_tests.rs`,880 行 |
| 测试路径 | `model::<模块>::…` | `model::model_tests::<模块>::…` |

- **位置**:`model_tests` 必须是 `model` 的**子模块**(文件放 `model/model_tests.rs`,由 `model.rs` 内的
  `#[cfg(test)] mod model_tests;` 引入)。原因:测试用到 `model` 的私有项(`params_index` / `field_text` /
  `VALUE_SHADOW_XML` 等),只有子模块能访问;放在 `translate/` 的同级文件会因可见性编译失败(实测)。
- **内层模块改名**:`mod model_tests` 改名 `block_json_tests`,否则路径成为
  `model::model_tests::model_tests`,触发 `clippy::module_inception`(`-D warnings` 下红)。

### 2.2 A2 判暂不做

- 依据:`../rounds/53` §3A 的门槛(不是"为对称而抽",收益要能说出来)。A1 后本体 2331 行回到软上限内,
  再拆 3 个文件只剩"改一个方向不必读另外三类"的可读性收益;而要把跨子模块的可见性从 `pub(super)` 调到
  `pub(in …)`,改动面与风险不成比。
- 替代:文件头按五区标注(树模型与类型 / id / 源侧解析 / 编码写出 / 程序集),每区给出符号清单。
- 重开条件:文件再次超过 2500 行,或某一区(如程序集)的修改频率显著高于其余几区。

## 3. 验证

| 门 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test --lib` | 164 passed / 0 failed / 2 ignored |
| `cargo test --test repo_hygiene` | 2 passed |
| `cargo test --test convert_bench` | 3 passed / 1 ignored |
| `cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`(基线门) | 通过 |

- 纯搬迁,不涉及产物字节与分配计数;机器时序噪声大(同一天同一命令测得 128 s 与 276 s 两档),不作性能结论。
- 全量测试计数与改前一致(164 / 2),说明 7 个模块的测试都还在跑;`model::model_tests` 过滤下 26 个通过。

## 4. 实施记录

| 序 | 内容 | 验证 |
| --- | --- | --- |
| 1 | 7 个测试模块外移到 `model/model_tests.rs`;内层 `mod model_tests` 改名 `block_json_tests` | `cargo test --lib model_tests` 26 passed;全量 164 passed |
| 2 | `model.rs` 文件头五区标注;A2 判暂不做 | 见 §2.2 |
| 3 | 文档:本文件、`../knowledge/repo-conventions.md` §5、`../goals/convert-backlog.md` §2 第 15 条、两个索引 | 本文件 |

代码落地提交:`679cbbf`;文档为后续提交。

## 依据

- 方案:`../rounds/53-convert-abstraction-plan.md` §3A / §5 / §6。
- 规则:`../knowledge/repo-conventions.md` §5(测试内联/外移、2500 行软上限、文件组织)。
- 读数:2026-11-07 实测(`wc -l`;可见性失败与 `module_inception` 报错见 §2)。
- 语料与基线门:`../rounds/56` 的命令表与 `tests/fixtures/translate/convert_bench_baseline.json`。
