# 第六十一轮记录 — 2026-11-08 代码逻辑与架构优化(审计后第一批)

## 0. 一句话

两路只读审计(`算法/分配` 与 `设计/写法`)给出 22 条候选;本轮落地 5 组改动,明确判不做 6 条,
并保持六个基准样本的产物 **SHA256 全部不变**、`cargo test --lib` 165 项全绿。

## 1. 审计范围与方法

- 范围:`translate/{mapping,assembly,pipeline,model}`、`decompile/**`、`core/retrieve.rs`、
  `core/registry.rs`、`utils/requests.rs`。
- 方法:两个只读子代理分别按"重复线性扫描 / O(n²) / 多遍遍历 / 热路径分配"与"重复实现 / 职责错位 /
  stringly-typed 表"出候选并给 `文件:符号` 证据;逐条对照 `docs/goals` 的"判不做"清单去重。
- 判据:分配计数当结论、毫秒只记方向(`../knowledge/convert-performance.md` §2bis 的纪律);
  任何产物字节变化都必须由 `convert_bench` 的 SHA 门解释。

## 2. 分配读数(`00fc981`)

窗口 = 一轮 `translate_file`,取串行腿;同轮 A/B(`convert_bench` 内置 `CountingAllocator`):

| 样本 | 前 | 后 | 变化 |
| --- | --- | --- | --- |
| kitten4-10.8MB | 912 164 | 898 214 | −1.53% |
| kitten4-0.3MB | 28 917 | 28 531 | −1.33% |
| kn-9.4MB | 442 291 | 439 079 | −0.73% |
| kn-3.7MB | 170 563 | 169 082 | −0.87% |
| nemo-3.4MB | 1 322 583 | 1 316 364 | −0.47% |
| nemo-old-1.5MB | 405 053 | 403 263 | −0.44% |

来源(逐项):
- `write_block`/`write_kitten4_block`:常见路径(`extra` 为空)键表改栈上数组,每块省一次 `Vec` 分配与排序;
- 源树构建的祖先链改借 `&str`,去掉每节点一次 `String` 分配;
- `reverse_field_name` 去掉每字段一次 `Vec` clone;`truncate_width` 未截断路径从两次分配降为一次。

## 3. 逻辑与单一来源(`78b5c8c`)

| 改动 | 之前 | 之后 |
| --- | --- | --- |
| 方向分派 | `translate_value`/`_product`/`translate_text` 各写一份 `match (from,target)` | `convert_document(mode)` 一份 |
| strict 门 | 6 处 `if options.is_strict() && report.is_lossy()` | `finish_report` 一份 |
| 源格式错误 | 3 份相同文案 | `unknown_source` 一份 |
| 影子字段 | JSON / XML 两种方言各算一遍 | `ShadowBuilder::effective_fields` 一份 |
| 编辑版扩展名 | `HashMap` + 空串哨兵 + 三处 `trim_start_matches` | `EditorType::file_extension()` 穷尽 match |
| 设备认证错误 | 同一消息拼两遍、错误包自身一层 | `ResultExt::with_context` |
| 写出挂点 | 每实体线性扫挂点(实体 × 挂点) | 按 id 建一次索引(实体 + 挂点) |

## 4. 结构拆分

| 拆分 | 结果 | 依据 |
| --- | --- | --- |
| `translate/product.rs`(`58d7bb5`) | 流式产物文档与写出器整体迁出:`assembly.rs` 2726 → 2356 行,`product.rs` 424 行;装配侧只保留构造/登记 | 单文件软上限 ≈ 2500(`../knowledge/repo-conventions.md` §5) |
| `nemo_tables.rs`(`febf7fc`) | 7 张纯数据表迁出:`nemo_mapping.rs` 2617 → 2073 行,`nemo_tables.rs` 556 行;原路径 `pub(super) use` 再导出 | 同上;表与逻辑分文件便于逐条核对 |
| 举报动作(`75dc7a0`) | `ReportAction::label()` / `resolution()` 成为唯一映射;`action_name` 与 `ActionConfig::simple` 改为薄适配 | 同一规则原有三份实现 |

## 5. 判不做的候选(附证据,避免下轮重提)

| 候选 | 判据 |
| --- | --- |
| 抽公共 `TranslateOptions`/`DecompileOptions` 结构 | 会动两个**公开**结构体的字段形状,收益仅省 6 个一行方法;4 个 bool 无互斥/非法组合 |
| `is_kitten_side` 的 6 次成员检查合成一张表 | 仅"未映射类型"分支调用(罕见),不在热路径 |
| `PaginatedIter::build_params` 复用参数 `Vec` | 每页几十字节,相对一次 RTT 可忽略 |
| `resolve_callee` 建程序集索引 | 全语料实测上限 476 个调用块 × 52 个程序集 ≈ 2.5 万次短串比较,低于可测门 |
| `variable_entries` 去整表深拷 | 变量表普遍很小(单位数到几十条),先留档不动 |
| 影子三张表(`shadow_types`/`shadow_fields`/`shadow_templates`)统一 | **集合语义本就不同**:`variables_get`/`math_angle` 有模板/字段但不是 shadow 类型;强行统一会改变反编译分类 |
| 正向与 NEMO 的 KN 顶层键集统一 | NEMO 侧少写哪些键是官方行为,统一会改 NEMO 产物字节;须先对平台原件取证(与 N4 布局常量无关) |

## 6. 验证

| 门 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test --lib` | 165 passed / 2 ignored |
| `convert_bench`(六样本,串行 + 并发 8) | 产物 SHA256 与 `#meta` 基线一致;实体并发 1 vs 8 同 SHA |
| `repo_hygiene`(pre-commit) | 每次提交通过 |

## 依据

- 候选与证据:2026-11-08 两路只读审计(子代理 transcript 不入库);
- 性能口径与基线:`../knowledge/convert-performance.md` §2bis、`../knowledge/repo-conventions.md` §6;
- 结构规则:`../knowledge/repo-conventions.md` §5;
- 提交:`00fc981`、`78b5c8c`、`58d7bb5`、`75dc7a0`、`febf7fc`。
