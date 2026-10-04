# 第四十五轮记录 — 机械批与核验收口

## 0. 一句话
把目标库里"不用拍板"的一批逐条做完或判掉:七项小改落地、两项核实不可行/不立项、三项陈旧挂账核销,并给反向查表补上与正向同口径的索引(产物字节零变化)。

## 1. 任务背景
2026-10-03 的指令是完成"不需要拍板"的条目集合。来源是第 43/44 轮登记的三处清单:`platform-backlog.md` §4(P2 性能小项)、`infra-backlog.md` §2 第 3 条与 §3、`pending-decisions.md` C 组(C3–C6)、`convert-backlog.md` §2 第 5/6 条与 §7(性能盘点)。**大件三项**(分片上传、NEMO 完整搬家的资源重传、夹具目录与输出目录分离)不属"机械小改",仍留在目标库。

## 2. 逐条结果

### 2.1 落地(代码)

| 条目 | 改动 | 证据 |
| --- | --- | --- |
| Bearer 头逐请求 `format!` + token 克隆 | 身份槽改为 `IdentitySlot { token, header }`,`"Bearer {token}"` 在 `set_token` 时**预计算**一次;`AuthProvider::auth_header` 改为返回 `Option<(&'static str, Arc<str>)>`(内置实现全部覆写,trait 默认实现保留给外部实现者) | `utils/requests.rs::{IdentitySlot,IdentityManager::auth_header,set_token}`、`GlobalKittyAuth`/`LocalKittyAuth` 的委托覆写 |
| flush 固定 100 ms 轮询 | 新增 `CloudInner.flush_pending`;入队走 `Notify::notify_with` 置位,`flush_loop` 用 `wait_flag(.., flush_interval, ..)` 等待;标记在 `commands` 锁内清除以消除丢失唤醒窗口;**保留超时兜底**(断线回退的批次仍定期重试) | `core/cloudvar.rs::{CloudInner::queue,flush_loop,CloudConnection::close}` |
| `simple.rs` 的整份 JSON 深拷 | `CocoDecompiler::decompile` 改为 `Arc::try_unwrap`,拿不到唯一所有权才回退克隆 | `core/convert/decompile/editors.rs` |
| `AccountStatus` 与 `Identity` 平行枚举 | 删除 `AccountStatus` 与 `to_identity()`,全链改用 `Identity`(`Average→Fluffy`/`Edu→Scholar`/`Judgement→Judge`,默认身份 `Fluffy` 不变) | `api/auth.rs`、`core/pipeline.rs`、`README.md` 示例 |
| 两个私有 trait 的样板 | `MessageHandler`(8 个)与 `ChatEventHandler`(5 个)的结构体 + trait 改为自由函数,分派结构保留 | `core/cloudvar.rs`、`core/converse.rs` |
| `core::registry` 的错位工具 | `value_to_string`/`timestamp_to_string`/`html_to_text`/`bytes_to_human` 四个与举报无关的函数移到 `utils::filedata`(与既在那里的 `value_to_i64` 同处) | `utils/filedata.rs`;调用点 `core/{pipeline,services}.rs` |
| 回调 panic 的契约 | 云变量与 AI 对话的全部 `on_*` 注册 API 顶部写明:`release` 是 `panic = "abort"`,回调内 panic 直接终止进程、`catch_unwind` 无效 | `core/cloudvar.rs`、`core/converse.rs` |
| 词表重导流程 | `kitten4_vocab.rs` 头部补成可复跑三步(打开 Kitten4 等 `Blockly.Blocks` 就绪 → 控制台按排序逐行打印可直接粘进 Rust 数组的形式 → 整体替换并更新 `KITTEN4_VOCAB_EXPORTED`) | 同上;常量值与快照未动 |

### 2.2 核实后不立项 / 不可行(有证据)

| 条目 | 结论 |
| --- | --- |
| `PaginatedIter::build_params` 每页克隆 `base_params` | **不立项**:要真省掉克隆必须让 `MewRequestBuilder::with_params` 改收 `&[(String, String)]`(公共面破坏),或把整链改 `&mut self`——而 `with_params` 按值消费 `Vec` 的现状下后者也省不掉克隆;`base_params` 通常只有个位数条目 ⇒ 收益 < 改动面 |
| `get_sha` 返回 `&str` | **不可行**:缓存是 `RefCell<HashMap<String, String>>`,借用守卫在函数返回时释放,无法把内部 `&str` 带出边界;改 `Rc/Arc` 或 `&mut self` 的改动面远超一次 64 字节克隆 |
| C7 把 `decompile_work` 拆成 `decompile_to_json`/`decompile_to_file` | **判不做**:前提不存在 —— 现签名里 `output_dir: None` 只表示"写默认目录",没有"返回 JSON 串"的分支(`../rounds/17` §3 已确认该重载模式从未实现且当时已删除);内存产物由 `CodemaoDecompiler::decompile_artifact_with` 提供 |

### 2.3 核销(陈旧挂账)

- `infra-backlog.md` §3-1:该契约测试**从未落地**,但这是 `../rounds/15` 方案自留的退化,不是遗漏(全仓同名 0 命中、`services.rs` 无测试模块)。
- `infra-backlog.md` §3-2:现行 `CONTRIBUTING.md` 已含 `../rounds/19` Phase 4 要求的三处改动(命名段、锁条款含 `parking_lot` 条件、错误段"保留底层变体")。
- `infra-backlog.md` §3-3:三条均已在代码里落实,且 `platform-backlog.md` §2 已同名标完成 ⇒ 本条是**重复挂账**,已删。
- `convert-backlog.md` §2 第 6 条:`../rounds/21` 的遗留实为 **§8.6 的 L1–L5**(§6 是"风险与不做清单")。L2 已由上传超时覆盖、L3 已按方案文档化、L5 判不做、§4/§5 各阶段与四条"偏差"均已定案;**仅 L1**(`translate_works` 并发端到端真机用例)与 **L4**(`DecompilerError` 改名 `ConvertError`)仍开放,已登记为待决。
- `convert-backlog.md` §2 第 0b 条:现状与条目描述不符(注释本就有流程,缺的只是可直接复制的命令)⇒ 本轮补齐后结案。

### 2.4 性能盘点(§7):反向查表的索引一致性

`mapping.rs` 的反向半边一直用 `iter().find` 线性扫静态表,而正向半边早在 `../rounds/37` P5 就换成了 `LazyLock` 索引。本轮补齐,并新增三种索引构造器(`model.rs`):

- `group_index`(**不去重**):外层键 → 该组原始条目切片。反向靠"命中数唯一才认"判歧义,去重会丢多命中 ⇒ 用它取代 `SPECIAL_FIELD_VALUES`/`INPUT_NAME_MAP` 的外层线性扫。
- `flat_reverse_index`(值 → 全部命中的键,保留表内顺序)⇒ 取代 `reverse_field_name` 对 `FIELD_NAME_MAP` 的**两趟**线性扫。
- `pair_reverse_index`(`(名, 值)` → 键,首个命中胜)⇒ 取代 `reverse_appearance_attribute` 的线性扫。
- 另补 `ZH_NAME_BY_TYPE_INDEX`(204 条,原先是逐块线性扫)与 `is_renamed_lc_key` 的索引化。

等价性依据:三张正向表实测**无重复键**(`KITTEN_TO_KN` 245 条、`ZH_NAME_BY_TYPE` 204 条、`FIELD_NAME_MAP` 键亦无重复),故"首个命中优先"的索引与原 `iter().find/.any` 逐键等价;歧义敏感的路径用不去重/全命中的索引形态。硬证据见 §3 的产物 SHA 门。

## 3. 验证

| 项 | 读数 |
| --- | --- |
| `cargo fmt --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过,零告警 |
| `cargo test` | 退出码 0:14 个目标 **144 通过 / 0 失败 / 11 忽略**(含两腿语料扫描器与真机 `convert_live`/`compile_live`/`live_features`) |
| `BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored` | **产物 SHA256 与 `#meta` 与基线逐项一致**,实体并发 1 vs 8 同 SHA |

索引改动的 A/B(单侧 2 轮取最小值,`core` / `e2e` 单位 ms):

| 样本 | 方向 | core 改前 → 改后 | e2e 改前 → 改后 |
| --- | --- | --- | --- |
| kn-9.4MB | 反向**(受影响)** | 213 → 213 | 371 → 370 |
| kn-3.7MB | 反向**(受影响)** | 68 → 66 | 114 → 118 |
| kitten4-10.8MB | 正向(不受影响) | 327 → 315 | 788 → 779 |
| kitten4-0.3MB | 正向(不受影响) | 7 → 6 | 25 → 22 |
| nemo-3.4MB | NEMO(不受影响) | 306 → 304 | 395 → 395 |
| nemo-old-1.5MB | NEMO(不受影响) | 95 → 89 | 125 → 123 |

判读:**受影响的反向样本与不受影响的样本落在同一量级**,且同侧两轮之间的固有波动(`nemo-3.4MB` 327↔306)就大于改动带来的差值 ⇒ 差异被运行间波动淹没,**不宣称提速**。该改动按"与正向口径一致 + 产物零变化"收,与 `../rounds/37` P5 的结论同型(那次换成哈希后同轮 A/B 也在 ±2% 噪声内)。

## 4. 未做(需单独决策或属大件)
- **大件三项**仍在目标库:分片上传(>20 MB,暂无需求)、NEMO"完整搬家"的资源重传(逐资源上传 + 文档内 URL 改写)、夹具目录与输出目录分离(`convert-backlog.md` §2 第 13 条,待方案)。
- 本轮新登记的待决:`../rounds/21` §8.6 的 **L1**(并发真机上传用例)与 **L4**(`DecompilerError` 改名)。

## 依据
- 代码提交:`9ddacbb`(`refactor:` 机械批收口 —— 认证头预计算 / flush 按需唤醒 / 枚举合并 / 工具归位 / 反向查表索引)。
- 核验结论:2026-10-03 只读核验(逐条的代码符号/提交号/文档 § 证据),结论已写入 `../goals/infra-backlog.md` §2/§3、`../goals/convert-backlog.md` §2、`../goals/platform-backlog.md` §4、`../goals/pending-decisions.md` C 组。
- 性能读数:本机 `bench_perf` 档四轮(改前 2 / 改后 2),产物 SHA 与 `#meta` 基线由 `tests/convert_bench.rs` 断言。
- 约定:`../knowledge/repo-conventions.md` §3(注入与全局)、§4(错误类型)、§6(门禁)。
