# 仓库约定(协议/风格/纪律)

> 知识库条目:本库**已经定下来的工程约定**。违反它们会让评审被打回,或让改动与全仓不一致。
> 约定来源:`CONTRIBUTING.md` + 历史轮次(05/06/07/08/09/14–19/21)。

## 1. 「六协议」(API 形态的唯一标准)

| 协议 | 要求 |
| ---- | ---- |
| **构造** | Builder 链式;`build()` 返回实例;**必选参数走 `new`**、可选走链式;**禁止在 Builder 上直接发网络** |
| **执行** | 网络由**实例方法**触发、返回 `MewResult<T>`、构造无副作用 |
| **分页** | 数据源实现 `Iterator<Item = MewResult<T>>`;调用方 `for` 遍历;**`Err` 不中断**;**不暴露 `next_chunk`** |
| **回调** | 统一 `on_` 前缀 |
| **等待** | 统一 `_and_wait` 后缀;**超时参数进 Builder**,`_and_wait` 本身不接收超时 |
| **配置** | 复杂可选配置进 Builder 链式,**不单独定义 `Options` 结构体传自由函数** |

> **已知例外(勘误)**:auth 域当前是 `LoginBuilder::execute(&mut self)` —— `LoginSession` 那套"build 后再 execute"的形态已被后续依赖注入重构推翻。见 `errata.md`。

判定过"合规"的历史项(别再重新"整改"):`KittyRequestBuilder::send*` 属请求原语;`wait_for_*` 是等待原语;`CommentQueryBuilder::stream_*` 是惰性执行;`CheckConfig`/`KittyConfig`/`PaginationConfig` 是内部配置结构体;`done_chunks` 是历史查询收尾 API。

## 2. 编码风格

- 错误传播:`?` + `ok_or_else(|| ...)`;**禁止裸 `unwrap`/`expect`**(锁除外 —— `lock().unwrap()` 全仓接受)。
- **不引入**宏/泛型抽象;不引入新第三方依赖(沿用 std 与既有依赖)。
- 函数内聚优先于"看起来对称":已经判定**不做**的统一项(分页构造、错误构造、`with_page(Option<i32>)` vs `with_limit(usize)`、builder bool 开关枚举化)都有记录理由,**不要重开**。
- 注释写"为什么"(约束/证据),不写"做了什么"。

## 3. 依赖注入与全局状态(第 14–17 轮的结论)

- 业务面**统一注入**:`src/api/**` 的 Manager、反编译器、`core` 举报引擎的 `DataQuery`/`CommentQueryBuilder`/`ReportFetcher`/`ViolationChecker`/`ReportProcessor`/`FileProcessor` 都通过 `new_with_client(client)` 注入;`client: &'static CodeMaoClient` 字段已**全仓归零**。
- **有意保留的全局**:`CodemaoDecompiler::global()` 门面;多账号身份槽(`switch_identity`)、`ActionRegistry`(`LazyLock`)、报告展示注册表 —— 因为"多账号 = 切全局身份槽",**不能部分注入**。
- `KittyFactory` 门面已删除(全仓 grep 零命中),不要再引入同类门面。
- ✅ **已完成(2026-09-26)**:`cloudvar.rs` 的 `detect_editor` 原用全局 `WorkDataFetcher::new()` —— 现改为
  `CloudBuilder::new_with_client(work_id, client)` + `CloudConnection` 暴露 `client()`(`impl ClientAccess`),
  缺省构造仍走 `CodeMaoClient::global()`(对既有使用方行为不变)。回归测试见 `cloudvar::tests::cloud_builder_uses_injected_client`。
- **仍未注入的一处(已知,记在目标库)**:`src/core/convert/mod.rs` 上传前取作品 `preview` 用 `WorkDataFetcher::new()`
  (全局);要注入得让 `DecompileOptions`/`TranslateOptions` 持有客户端 ⇒ 独立决策。

## 3bis. `.gitignore` 的一个坑(已修,读到这里先知道)

仓库的 `.gitignore` 从 Python 模板继承了通配规则 `bin/`,它会**连带忽略 Rust 的 `src/bin/`**
⇒ `src/bin/gen_translate_tables.rs`(translate 表的生成器)**长期没进版本库**,而文件头与 `docs/rounds/20` 都以为它已提交。
已在 `.gitignore` 里显式放行(`!/src/bin/`、`!/src/bin/**`),该文件现已入库。
**教训**:往仓库里加"生成器/工具"时,确认 `git check-ignore -v <path>` 是空的 —— 否则"改生成器"这件事本身会悄悄丢失。

## 3ter. 基准与门的环境开关(2026-09-26 起)

| 开关 | 作用 |
| ---- | ---- |
| `BACKEND_REQUIRE_LIVE=1` | 真机测试严格模式:缺配置/登录失败**一律失败**,不再静默 pass |
| `BACKEND_REQUIRE_BENCH=1` | **基准**严格模式:debug 构建、样本缺失一律失败(默认这两条是"打印后 return **显示 pass**",干净检出上等于没有这条门)。**基线例外**:`NotFound`/解析失败在**默认模式下也直接失败**(`f62aec7`,W3a;此前"NotFound ⇒ 返回空 map + 首跑自动写盘"会让门在「基线被删 / 全新检出」时静默消失)——建基线的**唯一**通道是显式 `BACKEND_BENCH_REFRESH=1` |
| `BACKEND_REQUIRE_FIXTURES=1` | **夹具**严格模式:跑测试要用的夹具(语料 / 真作品样例)缺失时**失败并点名缺了什么**(默认只打印一行 `跳过:` 后 return —— 干净检出上等于**显示 pass**)。与 `BACKEND_REQUIRE_BENCH` 分工:后者管基准自身(debug 构建 / 样本 / 基线),它只管"夹具缺了";`convert_work_bench` 另认历史的 `BACKEND_REQUIRE_BENCH` 作别名(别让已有环境悄悄变松)。覆盖:`reverse_tests` 的 10 处缺夹具出口 + `convert_facade_bench`(原先**静默** `continue`)|
| `BACKEND_BENCH_REFRESH=1` | **有据刷新**基线:写出新基线并**逐键打印**变化(产物字节/块数/告警数/源文件 SHA256);不用它时基线损坏/缺失**一律炸**,绝不静默重建 |

**验证纪律(踩了三次总结出来的)**:
1. 性能/行为改动**必须同轮 A/B**(`git worktree` 双树交替、每侧多轮取最小);跨轮绝对值会漂 20–40%,不可比;
2. 探针/仪器**不得吞 stderr**(曾因此漏看一次失败);
3. "块数/差异"类结论先**声明口径**(原始遍历 / 树可达 / id 在不在,三者不等),定案用**真 API 的最小复现**交叉验证;
4. 为抵消改名而建的**等价类口径,自身必须对改名稳定**(代表要固定,否则把改名又变回差异)。

## 4. 错误类型

- 统一 `MewError` / `MewResult<T>`(`src/utils/requests.rs`);HTTP 状态码枚举已改名为 `StatusCode`(`HTTPStatus` 已不存在)。
- `ProcessorError`/`DataQueryError` 已包装 `MewError`。`DecompilerError` 现只有 `Mew(#[from] MewError)` 加域内变体(`Crypto`/`Decompile`/`UnsupportedType`/`InvalidResponse`/`MissingField`/`TypeMismatch`/`Other`),`io::Error`/`serde_json::Error` 经 `From` 折进 `Mew` —— 原先记的"仍自带 `Io/Json/Http`、与 `MewError` 重复**待改**"**已不存在**(2026-10-01 核实,见 `docs/rounds/39` §1.3/§W12d)。唯一残留是零调用死变体 `UnsupportedType`;删它 = 动公共枚举,需授权(`rounds/39` §W5②)。
- 破坏性 API 变更**不留兼容别名**(已授权的前提下直接删)。

## 5. 命名与文件组织

- 删除/合并**必须有零调用点证据**(全仓 grep 命中 0 才删);测试模块一律放**文件末尾**。
- `src/prelude.rs` 只 re-export `utils::requests`;`utils.rs` 现导出 `requests`/`filedata`/`socketio`(`acquire.rs`/`data.rs` 已成历史名)。
- **`core/convert/` 是"作品文件转换域"的唯一边界**:读(反编译)与写(互转)共用同一地基;域外不再有平铺的 `compiler.rs`/`unpacker.rs`/`decoders.rs`。
- **域内文件组织(以 `docs/rounds/31-convert-layout-consolidation-plan.md` §2.1 为准)**:一个文件一个职责;
  生成物单独一处(`translate/tables_gen.rs`,**不可与手写表混放**);测试默认内联在被测文件末尾,
  「本体 + 测试 > 3 000 行」时才独立成 `*_tests.rs`;**单文件上限 ≈ 2 500 行**。
  当前布局:`mod.rs` + `shared.rs` + `decompile/{mod,editors}.rs` + `translate/{mod,model,mapping,assembly,pipeline,options,report,xml,nemo,nemo_mapping,tables_gen,kitten4_vocab,reverse_tests,nemo_tests}.rs`(**18 文件**;权威清单见 `docs/rounds/39` §1.1;`options`/`report`/`pipeline`/`xml` 与 `kitten4_vocab` 是 rounds/34–37 加进来的)。
- 分层纪律:`translate` 子域**不碰网络**;需要网络(上传/建作品)的编排放 `core/convert/mod.rs` 门面
  (反编译侧的可选「上传到账号」同理,见 `docs/rounds/30`)。
- 文档:记录放 `docs/`;**历史轮次不改写**(保真),勘误集中到本库 `errata.md`。

## 6. 测试与验证门

| 门 | 内容 |
| -- | ---- |
| 单测 | `cargo test`(库单测:注入契约、请求头大小写、分块终止性等) |
| `tests/repo_hygiene.rs` | 挡住明文账密/敏感串入库(曾经真的漏过) |
| 真机门 | `tests/live_features.rs`(登录 + AI + 云变量)、`tests/compile_live.rs`(7 种反编译)、`tests/convert_live.rs`(转换 + 写平台) |
| **严格模式** | `BACKEND_REQUIRE_LIVE=1` —— 缺配置/登录失败**必须失败**,不再"静默 pass" |
| 转换专项 | 官方校验器(`validateBcm`)硬门 + 语义 diff + `deterministic_ids` 字节一致 + 往返多重集守恒 |
| 必备前置 | 大改先出方案文档 → 子代理评审 → 再动 Rust 代码 |

配置与代码分离:真机配置从 `tests/fixtures/test-config.example.json` 复制到 `data/test-config.json`(`data/` 已 gitignore);`temp/` 放临时产物并及时清理。

## 7. 提交习惯(本项目实际用法)

- 提交信息用中文、`type: 摘要` 形式(如 `perf(convert): …`、`docs: …`);一个提交一件事,搬迁与行为改动**不混在一个提交**。
- 交付前自检:`cargo fmt`、`cargo clippy`(零新增告警)、`cargo test`;`.githooks/pre-commit` 会在可用时执行这些。

## 依据

- `CONTRIBUTING.md`;`docs/rounds/05/06/07-style|call|typing*.md`;`docs/rounds/08/09-protocol-compliance*.md`;
  `docs/rounds/14/15/16-*injection*.md`;`docs/rounds/17-error-convergence*.md`;`docs/rounds/18/19-*`;`docs/rounds/21-*`(域化)。
- 代码锚点:`src/utils/requests.rs`、`src/prelude.rs`、`src/utils.rs`、`src/core/convert/mod.rs`、`tests/repo_hygiene.rs`。
