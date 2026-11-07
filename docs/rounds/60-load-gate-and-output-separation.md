# 第六十轮记录 — 2026-11-07 加载门离线化(手动工作流)与产物/语料目录分离

## 0. 一句话

两件落地:官方 `validateBcm` 加载门有了**可复跑的手动工作流**(夹具入库、bundle 按固定 SHA256 运行时下载、
负控防空转);产物默认落 `target/convert-out`,与 `download/` 的语料目录分离。

## 1. G5 加载门(`validateBcm` 离线化)

### 1.1 形态

| 件 | 内容 |
| --- | --- |
| 夹具 | `tests/fixtures/convert/load_gate/minimal.bcm4`(4 278 B):由合成 KN(1 角色 / 1 场景 / 2 块)反向生成,不含第三方内容 |
| 测试 | `tests/load_gate.rs`(`#[ignore]`,默认 `cargo test` 跳过):夹具正向成 KN → 断言 `actors`/`scenes`/`styles`/`stageSize` 四个必填顶层键 → 官方 `validateBcm` 必须 `true`;负控(删 `actors.actorsDict`)必须 `false` |
| 工作流 | `.github/workflows/load-gate.yml`(`workflow_dispatch`):从官方 CDN 下载四个 chunk(合计 13.9 MB)到 `temp/web/`,按固定 SHA256 校验,再以 `BACKEND_REQUIRE_LOAD_GATE=1` 跑 `--ignored` |
| 主 CI | **不变**:`build` + `hygiene` + `offline-gate`;加载门不进 push CI(不联网、不下载 bundle) |
| harness | `tests/load_gate/{harness.js,dom.js,reflect_metadata.js}` 入库;`BCM_WEB_DIR` 指向 bundle 目录(默认 `tests/web`) |

固定 SHA256(2026-11-07 从官方 CDN 下载后逐一核对):

| chunk | SHA256 |
| --- | --- |
| `base.f158ddbf.js` | `724251e202e9dac074241e6296f3bbf3773c420141516764c8434dd17eb113e7` |
| `crc_libs.8196dbd1.js` | `462bb78dc75f99002dfe628d15211941469aa01d71e12890327eedcdb142f8a7` |
| `main-vendors.9b801394.js` | `23dd6052f04563c91736e60d929789964b15a48fe330e4ba301135fcd6a4d887` |
| `main.14802dc2.js`(落地名 `main.js`) | `2214a97e56be14f02a3d20639c7ffadab6ff32a1b0d570e6c572e445c1af138d` |

### 1.2 读数与判据

- 本地(2026-11-07,bundle 就位):`BCM_WEB_DIR=temp/web cargo test --test load_gate -- --ignored` → 1 passed;
  负控被官方校验器判 `false` 且 harness 以非零退出(因此负控不能按"进程成功"断言)。
- **门非空转**由负控证明:删掉一个必填结构后校验器立刻报 `actorsDict 不存在`。
- 只验 `validateBcm` 的**结构必填**,不替代真机门(编辑器仍可能因渲染/交互问题不显示);真机门继续手动
  (`../knowledge/convert-semantics.md` §5bis)。

## 2. 产物与语料目录分离(backlog §2 第 13 条)

| 项 | 变化 |
| --- | --- |
| 默认产物目录 | `PathConfig::output_dir()` = `<root>/target/convert-out`(`CARGO_TARGET_DIR` 可改 target 根) |
| 反编译/转化输出 | `compile_file_path()` / `convert_file_path()` 跟随 `output_dir()`,不再写 `download/` |
| 语料目录 | `download_dir()` 保持只读语义(夹具/语料只从这里读) |
| 显式覆盖 | 不变:`DecompileOptions::output_dir` / `TranslateOptions::output_dir` |
| 回归 | `src/utils/filedata.rs::tests::output_dirs_stay_out_of_the_fixture_root`(断言产物目录不在 `download/` 下) |

背景:`download/compile/` 曾同时是输出目录与夹具目录,默认选项反编译恰好同名作品会覆盖夹具
(`../rounds/40` §8.4 的 `kn-9.4MB`);采集器"增量写新文件"只是把事故概率压低,没有消除。

## 3. 验证与实施记录

| 序 | 内容 | 结果 |
| --- | --- | --- |
| 1 | 目录分离:单测 + 受影响测试 | `cargo test --lib` 165 passed / 2 ignored |
| 2 | `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` | 通过 |
| 3 | `repo_hygiene`(新入库存的 harness/夹具) | 2 passed |
| 4 | 加载门(本地 bundle) | 1 passed(正控 `true`、负控 `false`) |
| 5 | 官方 bundle 哈希核对 | 四个文件与固定值逐一相等 |
| 6 | 文档:本文件、repo-conventions、backlog、两个索引、errata | 本提交集 |

代码提交:`f6f6b8e`(目录分离)、`2d3321b`(加载门)。

## 依据

- 门的目标与历史:`../goals/convert-backlog.md` §6.3 G5、`../rounds/40` §9.3 R5。
- 目录分离:`../goals/convert-backlog.md` §2 第 13 条、`../rounds/40` §8.4。
- 校验器口径:`../knowledge/convert-semantics.md` §6(硬门)、§5bis 第 3 条(名字契约)。
- CI 口径:`../knowledge/repo-conventions.md` §6;bundle 为第三方产物,不入库。
- 读数:2026-11-07 本机实测(命令见 §1.2、§3)。
