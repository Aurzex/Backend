# 第五十八轮记录 — 2026-11-07 转换域抽象与职责提取(第 53 轮方案 D / B,兼 E / F 量法)

## 0. 一句话

两道完成的落地:**D**(三个基准目标的重复工具收进 `tests/common/`)与 **B**(`nemo::assemble_nemo` 由 657 行
线性函数改为 `NemoAssembly` 阶段对象,装配顺序只在 `assemble()` 一处写出)。**E** 给出 `WorkId` 试点的量法结论:
试点已存在,但"被编译期拦下的误传 / 因此删掉的校验"一处也数不出 ⇒ B7 的推广理由不成立。**F** 维持登记、不推进。

## 1. 判据

- 方案:`../rounds/53-convert-abstraction-plan.md` §3B / §3D / §3E / §3F。
- 门槛(同上文 §1):不是"为对称而抽";纯搬迁/提取不改产物字节;每步单独提交并说明过了哪条门。
- 统一验收:产物 SHA256 与 `#meta` 基线、`nemo_tests` 的字节等价门、`cargo fmt --check`、
  `clippy --all-targets -- -D warnings`、`cargo test --lib`。

## 2. D:测试侧共用件(`tests/common/`)

| 项 | 改前 | 改后 |
| --- | --- | --- |
| `sha256_hex` | `convert_bench.rs` / `convert_facade_bench.rs` / `convert_work_bench.rs` 各一份 | `tests/common::sha256_hex` |
| `require_fixtures` / `best` / `ms` / `bench_dir` | `convert_bench.rs` 与 `convert_facade_bench.rs` 各一份 | `tests/common::*` |

- 只收**口径无差异**的工具;基线读写与 `BACKEND_BENCH_REFRESH` 判定本来就只在 `convert_bench.rs` 一处,无重复可收。
- 各测试目标只取所需函数 ⇒ `tests/common/mod.rs` 顶层 `allow(dead_code)` 并写明理由。
- 读数:`convert_bench.rs` 1016 -> 980 行、`convert_facade_bench.rs` 216 -> 181 行、
  `convert_work_bench.rs` 211 -> 199 行;新增 `tests/common/mod.rs` 52 行。

## 3. B:`NemoAssembly` 阶段对象

| 项 | 改前 | 改后 |
| --- | --- | --- |
| `assemble_nemo` | 657 行单函数 | 12 行(构造 + `assemble()`) |
| 阶段顺序 | 藏在 657 行线性体里 | `assemble()` 十行调用 |
| 状态 | 局部变量 | `NemoAssembly { source, options, report, placements, qc, yc, product, now_ms, document, ids, ctx, stage_width, stage_height }` |
| 最大方法 | — | `assemble_entities` 306 行(并行四阶段,原样保留) |

`assemble()` 的顺序(官方步骤):

```text
assemble_procedures -> assemble_entities(并行四阶段) -> assemble_audios -> assemble_styles
-> assemble_broadcasts -> assemble_variables -> apply_stage_size -> apply_timer
-> apply_extension -> apply_project_name
```

- 纯提取:每个方法开头把字段重借为局部(`let source: &Value = self.source;` 等),函数体与原函数对应段
  **逐字一致**;两条薄包装(`convert_nemo_document` / `convert_nemo_document_product`)未动。
- 有意改动仅三处:`now_ms` 字段类型取 `u128`(与 `current_epoch_ms` 一致);`nemo_parse_procedures(dict, ctx, ids, report)`
  与 `kind.mint(ids)` 去掉多余的 `&mut`(绑定本身就是可变借用);`stage_width/height` 存入字段供后两阶段读。
- 核对法:把改前文件的 12 个代码段做成"空白归一化字符串",逐段断言其出现在新文件中;对 §3 的三处有意改动
  先做同样替换再断言 —— 12 段全部通过。
- 读数:`nemo.rs` 1658 -> 1791 行(方法开头的字段重借与文档注释 +133 行);`assemble_nemo` 657 -> 12 行。

## 4. E:`WorkId` 试点的收益量法(结论:B7 推广理由不成立)

- 事实:`WorkId` 在 convert 域内已是实参类型 —— `decompile/{mod,editors,work}.rs`、`upload.rs`、`staging_dir`、
  `translate_work` 等约 30 处签名;`shared.rs` 提供 `new` / `get` 与 `From<i64>` / `Into<i64>`。
- 量法(方案 §3E):试点应能数出"被编译期拦下的误传"或"因此删掉的调用点校验"。
  实测:convert 域内没有任何 `work_id` 的合法性/范围校验(改动前也没有)⇒ **一处也数不出**;
  而 `From<i64>` 又把"裸 i64 转 newtype"压成一次 `.into()`。
- 结论:B7(全量推广 newtype)的推广理由**不成立**;保留现状(域内继续用,公共 Manager 签名不动),
  结论写回 `../goals/pending-decisions.md` B7。

## 5. F:`translate_work` 的骨架读 —— 登记、不推进

`translate_work` 的源文档来自 `http_client.get_json`,整份响应仍建成 `Value` ⇒ 内存直通吃不到源侧骨架的收益。
方案 §3F 要求"先量收益(响应体量级)与接口面(通用层读法会不会外溢成公共面)再决定";2026-11-07 未量,保持登记,
位置见 `../goals/convert-backlog.md` 前言的「覆盖面缺口」。

## 6. 验证

| 门 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test --lib nemo_tests` | 10 passed / 1 ignored |
| `cargo test --lib` | 164 passed / 0 failed / 2 ignored |
| `cargo test --test convert_bench` | 3 passed / 1 ignored |
| `cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`(基线门) | 通过(NEMO 两样本产物 SHA 与 `#meta` 不变) |
| 三个基准目标(facade / work) | 通过 |

- D 只动测试侧;B 是纯提取,产物与分配基线不变(基线门直接验证)。
- 全量测试墙钟在同一台机器上仍受负载影响明显(同日两档 128 s / 281 s),不作性能结论。

## 7. 实施记录

| 序 | 内容 | 验证 |
| --- | --- | --- |
| 1 | D:新增 `tests/common/mod.rs`,三个基准改用 | 三个基准目标 + fmt/clippy 全绿 |
| 2 | B:`NemoAssembly` 阶段对象(10 个方法) | `nemo_tests` 10 passed;全量 164 passed |
| 3 | B 的逐段核对 | 12 段空白归一化后逐段命中(有意改动先替换) |
| 4 | E 量法、F 登记 | 见 §4 / §5 |
| 5 | 文档:本文件、两个索引、`../goals/convert-backlog.md` §2 第 15 条、`../goals/pending-decisions.md` B7 | 本文件 |

代码提交:`ec18260`(D)、`d2c1e0d`(B);文档为后续提交。

## 依据

- 方案:`../rounds/53-convert-abstraction-plan.md` §3B / §3D / §3E / §3F、§5。
- 门与口径:`../knowledge/repo-conventions.md` §5/§6;`../rounds/56`(命令表)。
- NEMO 的字节门与三口径:`../rounds/49-nemo-product-streaming.md` §3 与 `translate/nemo_tests.rs`。
- 读数:2026-11-07 实测(`wc -l`;`assemble_nemo` 与各方法的行数按函数边界统计)。
