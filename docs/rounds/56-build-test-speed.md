# 第五十六轮记录 — 2026-11-06 构建与测试提速(语料扫描器去掉序列化往返)

## 0. 一句话

`cargo test --lib` 全量墙钟由 202 s 降到 128 s(约 −37%),改动只在测试代码:两道语料扫描器里
「序列化再解析拿副本」的写法换成 `Value` 拷贝。编译侧的候选(lld 链接器、`[profile.test] debug = 1`、
O1/O2 优化档)实测无收益或代价更高,均已撤回或判不做。

## 1. 背景与量法

开发机的固定约束:4 核、3.8 GB 内存(zram 已占 2.4 GB);仓库 54 个源文件、5.3 万行、166 个 `#[test]`;
`target/` 9.8 GB;语料 `download/compile` 下 62 个 `.bcm4`(133 MB)与 10 个 `.bcmkn`(11 MB),其中最大一件 63.6 MB。

改前的实测(同一会话内,`date +%s%N` 包裹单条命令):

| 环节 | 墙钟 |
| --- | --- |
| 改一个核心文件后 `cargo test --lib --no-run`(编译 + 链接) | 78 s |
| `cargo check --lib`(同一文件) | 33 s |
| `cargo test --lib` 全量 | 202 s |
| ├ `k4_corpus_round_trip_sweep`(62 件) | 171 s |
| ├ `kn_corpus_round_trip_sweep`(10 件) | 32 s |
| └ 其余 162 个单测 | 38 s |
| 测试二进制 | 75 MB |

时间集中在两处:两道语料门占全量墙钟的绝大部分(两道门合计 203 s,并行后全量墙钟 202 s);
编译侧是「整 crate 一次 codegen + 链接 75 MB 二进制」,`check` 的 33 s 已是只做类型检查的下限。

量法:单机单会话、每档 1–2 次,读数取整秒;4 核机器在 zram 压力下同一档的方差约 ±10%,
10 s 以内的差异不作结论。所有对照都在同一份 `target/` 与同一台机器上做,命令写在 §7。

## 2. 改动(只动测试代码)

`src/core/convert/translate/reverse_tests.rs` 的两道扫描器里,`serde_json::from_str(&x.to_string())`
的唯一用途是拿一份独立的 `Value`,等价于 `x.clone()`,而在 O0 下对多 MB 文档是一次完整的序列化加解析:

| 位置 | 改前 | 改后 |
| --- | --- | --- |
| `k4_corpus_round_trip_sweep` 的 `round_trip` 闭包(源) | `from_str(&source.to_string())` | `source.clone()` |
| 同一闭包(正向产品) | `from_str(&kn.to_string())`(产品本就是 `Value`) | 直接使用产品 `Value` |
| `k4_corpus_round_trip_sweep` 主循环(源) | `from_str(&source.to_string())` | `source.clone()` |
| `kn_corpus_round_trip_sweep` 的中间态 | `from_str(&k4_product.to_string())` | `k4_product.clone()` |

语义不变:两份都是独立深拷贝;文档来自 JSON,不含 NaN/Inf,`float_roundtrip` 打开后数值逐位一致。
断言、打印、id 台账、注释还原门全部保留 —— 两道门本身仍然逐字节比较两遍往返结果、按类型断言标记块回来。

## 3. 读数

| 指标 | 改前 | 改后 |
| --- | --- | --- |
| `cargo test --lib` 全量 | 202 s | 128 s |
| 编译(改一个核心文件) | 78 s | 78 s |
| `cargo check --lib` | 33 s | 33 s |
| 测试二进制 | 75 MB | 75 MB |

- 改后只复测了全量;两道门单独跑的耗时未再拆分。
- 全量里的 164 个测试全部通过(2 个 `#[ignore]` 照旧),测试计数与改前一致。

## 4. 编译侧的候选:实测与处置

| 候选 | 读数 | 处置 |
| --- | --- | --- |
| `[profile.test] debug = 1` | 测试二进制 75 MB -> 58 MB;同文件重建 86 s(改前 78 s) | 撤回:只减体积,不减时,且丢失变量级调试信息 |
| 本机 `[build] rustflags = ["-C","link-arg=-fuse-ld=lld"]` | 首次全量重建 380 s(依赖全部重编);之后同文件重建 86 s 与 95 s 各一次 | 撤回:对照不成立(`--config build.rustflags=[]` 未能让依赖重编,两组读数混在同一批指纹上),差异在噪声内 |
| `[profile.test] opt-level = 1` | 全量运行 46 s;增量重建 78 s -> 180 s;首次全量构建 373 s | 判不做:单测循环慢 2.3 倍,全周期只省约 19% |
| `[profile.test.package."*"] opt-level = 2`(只优化依赖) | 全量运行 133 s;首次构建 546 s | 判不做:与 §2 同收益、代价大一个量级 |
| 两道语料门复用中间态 / 按大小分片并行 | 未实施 | 判不做:§3 的 128 s 已低于方案设定的 150 s 阈值,留作后备 |

结论:本机编译侧的地板是「整 crate 一次 codegen + 链接」约 78 s(lld 与缩 debuginfo 不改变它);
运行侧的余量主要来自语料门本身的写法,优化档换来的提速抵不过它带来的重编译。

## 5. 验证

提交门窗(2026-11-06,改后工作树):

| 门 | 结果 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo test --lib` | 164 passed / 0 failed / 2 ignored,128 s |
| `cargo test --test repo_hygiene` | 2 passed |
| `cargo test --test convert_bench` | 3 passed / 1 ignored |
| `cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture`(基线门) | 通过 |

产物未改动(只动 `#[cfg(test)]` 模块),`convert_bench` 的产物 SHA 与分配计数基线保持不变。

## 6. 未做与后续

- 编译侧没有找到能在本机稳定生效的免费收益;若换机器(更多核、更大内存),`[profile.test] opt-level = 1`
  的账会反过来(构建增量摊薄、运行直接降到 46 s),届时应重测后再定。
- 运行侧后备两项:语料门复用已算出的中间态(每件少一次正向,预计约 −20 s);按文件大小分片并行(2 线程,
  最大一件单独跑),预计再减半,但内存峰值翻倍。两项都在 128 s 的现状下判不做。
- `cargo check` 的 33 s 与 `--skip corpus_round_trip_sweep` 的 38 s 证明:内循环应优先用「检查 + 点名测试」,
  全量语料门留到提交前(命令表见 `../knowledge/repo-conventions.md` §6)。

## 7. 实施记录

| 序 | 内容 | 验证 |
| --- | --- | --- |
| 1 | 两道扫描器去掉序列化往返(4 处) | 全量 164 passed,读数见 §3 |
| 2 | 编译侧四档实测(lld / debug=1 / O1 / 依赖 O2) | 读数见 §4,全部撤回或判不做 |
| 3 | 提交门窗与基线门 | 见 §5 |
| 4 | 文档:本文件、`../knowledge/repo-conventions.md` §6 命令表、两个索引 | 本文件 |

代码落地提交:`7f4042e`(测试代码);文档为后续提交。

## 依据

- 读数命令:`cargo test --lib`、`cargo test --lib --no-run`、`cargo check --lib`,均以 `date +%s%N` 包裹;
  优化档对照用环境变量 `CARGO_PROFILE_TEST_OPT_LEVEL`(不落盘),依赖档用临时 `[profile.test.package."*"]`(已撤回)。
- 语料规模:`download/compile` 下 62 个 `.bcm4`(133 MB)与 10 个 `.bcmkn`(11 MB),按文件字节统计。
- 测试门与 CI 口径:`../knowledge/repo-conventions.md` §6、§7。
- 语料扫描器的断言与门:`../rounds/38`(id 台账)、`../rounds/55`(标记可恢复门)。
