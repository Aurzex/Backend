# 仓库约定(协议/风格/纪律)

> 知识库条目:本库**已经定下来的工程约定**。违反它们会让评审被打回,或让改动与全仓不一致。
> 约定来源:`CONTRIBUTING.md` 与历史轮次(05/06/07/08/09/14–19/21)。

## 1. 「六协议」(API 形态的唯一标准)

| 协议 | 要求 |
| --- | --- |
| **构造** | Builder 链式;`build()` 返回实例;**必选参数走 `new`**、可选走链式;**禁止在 Builder 上直接发网络** |
| **执行** | 网络由**实例方法**触发、返回 `MewResult<T>`、构造无副作用 |
| **分页** | 数据源实现 `Iterator<Item = MewResult<T>>`;调用方 `for` 遍历;**`Err` 不中断**;**不暴露 `next_chunk`** |
| **回调** | 统一 `on_` 前缀 |
| **等待** | 统一 `_and_wait` 后缀;**超时参数进 Builder**,`_and_wait` 本身不接收超时 |
| **配置** | 复杂可选配置进 Builder 链式,**不单独定义 `Options` 结构体传自由函数** |

> **已知例外(勘误)**:auth 域为 `LoginBuilder::execute(&mut self)` —— `LoginSession` 那套"build 后再 execute"的形态已被后续依赖注入重构推翻。见 `errata.md`。

判定过"合规"的历史项(不再重新"整改"):`MewRequestBuilder::send*`(旧名 `KittyRequestBuilder`)属请求原语;`wait_for_*` 是等待原语;`CommentQueryBuilder::stream_*` 是惰性执行;`CheckConfig`/`ClientConfig`(旧名 `KittyConfig`)/`PaginationConfig` 是内部配置结构体;`done_chunks` 是历史查询收尾 API。

## 2. 编码风格

- 错误传播:`?` 与 `ok_or_else(|| ...)`;**禁止裸 `unwrap`/`expect`**(锁除外 —— `lock().unwrap()` 全仓接受)。
- **不引入**宏/泛型抽象;不引入新第三方依赖(沿用 std 与既有依赖)。
- 函数内聚优先于"看起来对称":已经判定为**判不做**的统一项(分页构造、错误构造、`with_page(Option<i32>)` vs `with_limit(usize)`、builder bool 开关枚举化)都有记录理由,**不再重新立项**。
- 注释写"为什么"(约束/证据),不写"做了什么"。

## 3. 依赖注入与全局状态(第 14–17 轮的结论)

- 业务面**统一注入**:`src/api/**` 的 Manager、反编译器、`core` 举报引擎的 `DataQuery`/`CommentQueryBuilder`/`ReportFetcher`/`ViolationChecker`/`ReportProcessor`/`FileProcessor` 都通过 `new_with_client(client)` 注入;`client: &'static CodeMaoClient` 字段已**全仓归零**。
- **有意保留的全局**:`CodemaoDecompiler::global()` 门面;多账号身份槽(`switch_identity`)、`ActionRegistry`(`LazyLock`)、报告展示注册表 —— 因为"多账号即切全局身份槽",**不能部分注入**。
- `KittyFactory` 门面已删除(全仓 grep 零命中),不要再引入同类门面。
- **已完成(2026-09-26)**:`cloudvar.rs` 的 `detect_editor` 原用全局 `WorkDataFetcher::new()` —— 现改用
  `CloudBuilder::new_with_client(work_id, client)`,并由 `CloudConnection` 暴露 `client()`(`impl ClientAccess`),
  缺省构造仍走 `CodeMaoClient::global()`(对既有使用方行为不变)。回归测试见 `cloudvar::tests::cloud_builder_uses_injected_client`。
- **已完成(2026-09-26,`915c8ff`)**:`src/core/convert/mod.rs` 上传前取作品 `preview` 原用全局 `WorkDataFetcher::new()`
  —— 现改由 `DecompiledArtifact::Document` 随产物带出 `preview`(反编译阶段本就拿到),建草稿不再重拉详情,
  因此那处全局客户端依赖随之消失(rounds/37 §10.1 P10)。

## 3bis. `.gitignore` 的一处陷阱(已修,阅读前先了解)

仓库的 `.gitignore` 从 Python 模板继承了通配规则 `bin/`,它会**连带忽略 Rust 的 `src/bin/`**,
因此 `src/bin/gen_translate_tables.rs`(translate 表的生成器)**长期没进版本库**,而文件头与 `../rounds/20` 都误以为它已提交。
已在 `.gitignore` 里显式放行(`!/src/bin/`、`!/src/bin/**`),该文件现已入库。
**教训**:往仓库里加"生成器/工具"时,确认 `git check-ignore -v <path>` 是空的 —— 否则"改生成器"这件事本身会在不知情的情况下丢失。

## 3ter. 基准与门的环境开关(2026-09-26 起)

| 开关 | 作用 |
| --- | --- |
| `BACKEND_REQUIRE_LIVE=1` | 真机测试严格模式:缺配置/登录失败**一律失败**,不再静默 pass |
| `BACKEND_REQUIRE_BENCH=1` | **基准**严格模式:debug 构建、样本缺失一律失败(默认这两条是"打印后 return **显示 pass**",干净检出上等于没有这条门)。**基线例外**:`NotFound`/解析失败在**默认模式下也直接失败**(`f62aec7`,W3a;此前"NotFound 则返回空 map 并首次运行自动写盘"会让门在「基线被删 / 全新检出」时静默消失)——建基线的**唯一**通道是显式 `BACKEND_BENCH_REFRESH=1` |
| `BACKEND_REQUIRE_FIXTURES=1` | **夹具**严格模式:跑测试要用的夹具(语料 / 真作品样例 / 官方 harness)缺失时**失败并点名缺了什么**(默认只打印一行 `跳过:` 后 return —— 干净检出上等于**显示 pass**)。与 `BACKEND_REQUIRE_BENCH` 分工:后者管基准自身(debug 构建 / 样本 / 基线),它只管"夹具缺了";`convert_work_bench` 另认历史的 `BACKEND_REQUIRE_BENCH` 作别名(避免既有的环境配置在不知情的情况下被放宽)。**唯一出口**:`translate/mod.rs` 的 `require_fixtures` / `missing_fixture`(`reverse_tests` 原先那份重复定义已删,不再新增第四份)。覆盖 `reverse_tests` 的 10 处、`translate/mod.rs` 的 `diff_tests` 2 处、`pipeline.rs` 1 处(`f01a6a8`)、`convert_facade_bench`(原先**静默** `continue`)与 `convert_bench` 的**样本缺失**(全缺或**部分缺**都算;`strict_mode()` 与 `require_fixtures()` 任一为真即生效 — 此前部分缺失只打印一行警告、那些基线键静默不设防)。例外:`nemo_tests` 的真作品门是 `#[ignore]` 与硬 `assert!`(没有静默出口,不需要它);`translate/mod.rs` 的"无法执行 node"是环境错误,不折进本开关 |
| `BACKEND_BENCH_REFRESH=1` | **有据刷新**基线:写出新基线并**逐键打印**变化(产物字节/块数/告警数/源文件 SHA256);不用它时基线损坏/缺失**一律失败**,绝不静默重建 |

**验证纪律(由三次实践教训得出)**:
1. 性能/行为改动**必须同轮 A/B**(`git worktree` 双树交替、每侧多轮取最小);跨轮绝对值会漂 20–40%,不可比;
2. 探针/仪器**不得丢弃 stderr 输出**(曾因此漏看一次失败);
3. "块数/差异"类结论先**声明口径**(原始遍历 / 树可达 / id 是否存在,三者不等),定案用**真 API 的最小复现**交叉验证;
4. 为抵消改名而建的**等价类口径,自身必须对改名稳定**(代表要固定,否则把改名又变回差异)。

## 4. 错误类型

- 统一 `MewError` / `MewResult<T>`(`src/utils/requests.rs`);HTTP 状态码枚举已改名为 `StatusCode`(`HTTPStatus` 已不存在)。
- **底层失败一律折进 `MewError`**(`io::Error` / `serde_json::Error` 由 `From` 折进),域错误类型只留域内变体 + 一个 `Mew` 载体。2026-10-03 前 `ProcessorError`/`DataQueryError` 并列的 `Io`/`Json` 变体已删,两者的 `External` 也统一改名为 `Mew`;`FileError` 整型删除(`CodeMaoFile::write_bytes` 直接返回 `MewResult<()>`)。口径锚点:`translate/options.rs::TranslateError` 与 `convert/shared.rs::ConvertError`。
- `ConvertError`(2026-10-03 前叫 `DecompilerError`,见 `errata.md`)现只有 `Mew(#[from] MewError)` 加域内变体(`Crypto`/`Decompile`/`InvalidResponse`/`MissingField`/`TypeMismatch`/`Other`)。原先记的两条**都不再成立**:"仍自带 `Io/Json/Http`、与 `MewError` 重复**待改**"已于 2026-10-01 核实消除(见 `../rounds/39` §1.3/§W12d);零调用的死变体 `UnsupportedType` **已于 `fef30e7`(2026-10-02)删除**(破...
- 破坏性 API 变更**不留兼容别名**(已授权的前提下直接删)。
- **第三方 crate 的类型不进公共契约**(2026-10-03):公共请求原语返回自有 `MewResponse`(内部持有 ureq 的响应),传输失败装自有 `TransportError`(只给文本与 `is_timeout()`),不再出现 `ureq::Error` / `Response<Body>` / `Agent` / `Form`;域内专用的 `agent()` / `send_multipart()` 收 `pub(crate)`。判据:下游**不声明 ureq** 也能走完整链路(验收手法见 `../rounds/44`)。

## 5. 命名与文件组织

- 删除/合并**必须有零调用点证据**(全仓 grep 命中 0 才删);测试模块一律放**文件末尾**。
- `src/prelude.rs` 只 re-export `utils::requests`;`utils.rs` 现导出 `requests`/`filedata`/`socketio`(`acquire.rs`/`data.rs` 已成历史名)。
- **`core/convert/` 是"作品文件转换域"的唯一边界**:读(反编译)与写(互转)共用同一地基;域外不再有平铺的 `compiler.rs`/`unpacker.rs`/`decoders.rs`。
- **域内文件组织(以 `../rounds/31-convert-layout-consolidation-plan.md` §2「目标结构」的模块图 + §2.3「组织规则」为准;该文没有 §2.1)**:一个文件一个职责;
  生成物单独一处(`translate/tables_gen.rs`,**不可与手写表混放**);测试默认内联在被测文件末尾,
  「本体 + 测试 > 3 000 行」时才独立成 `*_tests.rs`;**单文件上限 ≈ 2 500 行**。
  当前布局:`mod.rs` + `shared.rs` + `upload.rs` + `decompile/{mod,editors,config,shadow,work}.rs` + `translate/{mod,model,mapping,assembly,pipeline,options,report,xml,nemo,nemo_mapping,source,tables_gen,kitten4_vocab,reverse_tests,nemo_tests}.rs`(**23 文件**;W2 后新增 4 个:`upload.rs` 是**域级工具层**(与门面同级,`shared.rs` 零反向依赖),`decompile/{config,shadow,work}.rs` 是反编译**私有件**;`translate/source.rs` 是源侧骨架解析(rounds/47 Step 5、rounds/50 参数化)。权威清单见 `../rounds/39` §W2 落地段;其 §1.1 的表是 W2 之前的快照)。
- 分层纪律:`translate` 子域**不碰网络**;需要网络(上传/建作品)的编排放 `core/convert/mod.rs` 门面
  (反编译侧的可选「上传到账号」同理,见 `../rounds/30`)。
- 文档:记录放 `docs/`;**历史轮次不改写**(保真),勘误集中到本库 `errata.md`。

## 6. 测试与验证门

| 门 | 内容 |
| --- | --- |
| 单测 | `cargo test`(库单测:注入契约、请求头大小写、分块终止性等) |
| `tests/repo_hygiene.rs` | 挡住明文账密/敏感串入库(曾经真的漏过) |
| 真机门 | `tests/live_features.rs`(登录 + AI + 云变量)、`tests/compile_live.rs`(7 种反编译)、`tests/convert_live.rs`(转换 + 写平台) |
| **严格模式** | `BACKEND_REQUIRE_LIVE=1` —— 缺配置/登录失败**必须失败**,不再"静默 pass" |
| 转换专项 | 官方校验器(`validateBcm`)硬门 + 语义 diff + `deterministic_ids` 字节一致 + 往返多重集守恒 |
| 词表新鲜度 | `kitten4_vocab::tests::editor_type_list_freshness_is_reported_not_enforced` —— **只打印读数**(条目数 / 导出日期 / 距今天数;超过 180 天、或条目数与单一事实源 `KITTEN4_VOCAB_EXPORTED` 不一致时醒目提醒)。**刻意不做按挂钟时间失败**:那会让门在某个日期之后**自动变红**、沦为噪音;允许随日期改变**打印内容**,不允许让测试失败 |
| **CI 跑什么** | `.github/workflows/CI.yml` 三个 job:`build`(**五目标** `cargo build --release` 矩阵,含 aarch64 交叉链接)、`hygiene`(`cargo test --test repo_hygiene`,与本地 pre-commit 同一份)、**`offline-gate`**(`cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + **逐目标点名**的离线测试:`--lib` / `--test repo_hygiene` / `--test convert_bench`) |
| **CI 刻意不管什么**(是遗漏的反面,别当缺口补) | 1)  **真机门**(`compile_live`/`convert_live`/`live_features`):无 `data/test-config.json` 时它们 `load_config()` -> None 后**直接 return(静默 pass)**,当门等于没验;CI 也**不设** `BACKEND_REQUIRE_LIVE`(设了就依赖凭据/网络)=> 真机验证只在本机做。2)  **语料扫描器与 `convert_bench` 的性能样本**:吃 gitignored 的 `download/`,干净检出上走 `missing_fixture` **打印一行并跳过**(**预期跳过**,不是没跑)。3)  **artifact 上传**:本仓 `[lib] crate-type=["rlib"]`(只产 rlib)、`src/main.rs` 是需账号的交互式控制台、仓内无消费方 => **没有可分发产物**,旧上传步已删(`f68c2e6`,详见 `errata.md` 末节与 `../goals/infra-backlog.md` §1) |
| **死代码 / 未用项** | `[lints.rust] unused = "warn"` **活着**(`8da596d`,rounds/40 R2 三阶段放开):`clippy --all-targets -- -D warnings` 下**任何**未用项(未用 import / 变量 / `mut` / 赋值 / `must_use`、`dead_code`)都会把门**打红**。新增死代码只有三条出路:**1)  接线**(真用起来)/ **2)  标 `#[cfg(test)]`**(仅测试用)/ **3)  `#[allow(dead_code)]` + 一句理由**(如"由生成器消费"),**不留无理由的 `allow`**。口径与 `Cargo.toml` 里 `[lints.rust]` 的注释一致;**放开过程的读数、两条实测坑与逐条处置见 `../goals/infra-backlog.md` §1.1** |
| 必备前置 | 大改先出方案文档,经子代理评审之后再动 Rust 代码 |

配置与代码分离:真机配置从 `tests/fixtures/test-config.example.json` 复制到 `data/test-config.json`(`data/` 已 gitignore);`temp/` 放临时产物并及时清理。

## 7. 提交习惯(本项目实际用法)

- 提交信息用中文、`type: 摘要` 形式(如 `perf(convert): …`、`docs: …`);一个提交一件事,搬迁与行为改动**不混在一个提交**。
- 交付前自检:`cargo fmt`、`cargo clippy`(零新增告警)、`cargo test`;`.githooks/pre-commit` 会在可用时执行这些。CI 侧由 `offline-gate` 跑同一套**离线**门(`fmt --check` + `clippy --all-targets -D warnings` + 逐目标点名的离线测试,见 §6)——真机门与吃语料的基准**不在 CI 里**。

## 8. 文档规范

- **权威**:仓库级文档规范以根目录 `AGENTS.md` 为准(加载协议、体例 9 条红线、收尾 SOP、输出禁忌、正文语体、三库定位)。`../README.md` 的"文档体例"节只登记两库分工与例外,不重复条文。
- **载体**:`README.md` 首行注释声明进入任务前须读取该契约,因此后续会话无需人工重复提示。
- **收尾**:完成任务后按契约第四节更新轮次记录、目标库与知识库,并检查引用该事实的目标库条目是否需同步。
- **指针**:契约见 `../../AGENTS.md`;该轮的落地记录见 `../rounds/41-agent-contract-and-doc-reformat.md`。

## 依据

- `CONTRIBUTING.md`;`../rounds/05/06/07-style|call|typing*.md`;`../rounds/08/09-protocol-compliance*.md`;
  `../rounds/14/15/16-*injection*.md`;`../rounds/17-error-convergence*.md`;`../rounds/18/19-*`;`../rounds/21-*`(域化)。
- 代码锚点:`src/utils/requests.rs`、`src/prelude.rs`、`src/utils.rs`、`src/core/convert/mod.rs`、`tests/repo_hygiene.rs`。
