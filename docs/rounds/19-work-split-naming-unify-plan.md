# 第十一轮评审 — 剩余整改:unpacker 切分 / 命名统一(保留 MewError·MewResult)/ WorkId 试点 / 文档优化

审阅日期:2026-08-30 · 基线:HEAD `f8d394a`(第十轮已落地) · 范围:`src/core/{unpacker,compiler}.rs` + `src/utils/requests.rs` + `src/prelude.rs` + `README.md` + `CONTRIBUTING.md` + tests

> 方案先行(本文档),随后落地代码。第十轮已完成:登录流精简、compiler 拆门面/引擎(unpacker.rs)、converse 活性修复、错误模型(Other→InvalidArgument)、pub(crate) 收紧、`_gen`→`_iter`、unwrap 硬化、prelude。本轮承接剩余项,**按团队决策调整**:① `work.rs` **不切分**(2533 行维持单文件);② `MewError`/`MewResult` **保留不改**,其余萌化名照改;③ README/CONTRIBUTING 同步优化。**延续允许破坏性 pub API 变更**(0.1.0 窗口)。

## Context

第十轮后重新摸底(25,376 行):

| 文件                               | 行数        | 状态                                         |
| ---------------------------------- | ----------- | -------------------------------------------- |
| `core/unpacker.rs`                 | 3759        | **本轮再切**(框架 / 编辑器实现)              |
| `api/work.rs`                      | 2533        | **不切分**(团队决策,维持单文件,见「不落地」) |
| `core/cloudvar.rs`                 | 2495        | 浅切收益小,深拆成本高,**不切**(见「不落地」) |
| `core/retrieve.rs` / `pipeline.rs` | 1263 / 1133 | 语义内聚,维持单文件                          |
| `utils/requests.rs`                | 1942        | 命名统一落点                                 |

**命名统一影响面量化**(本轮改名的下游可见标识符):`HTTPStatus` 131、`Catsona` 62、`KittyAuth` 19、`KittyRequestBuilder` 15、`KittyConfig` 12;README 出现点:Catsona(L78/210)、KittyAuth(L213)、MewResult(L80/84/97,保留)、MewError(L215,保留)。**`MewError`/`MewResult` 按团队决策保留**(crate 品牌名,下游已习惯),其余萌化名改直白。

| 文件                               | 行数        | 状态                                         |
| ---------------------------------- | ----------- | -------------------------------------------- |
| `core/unpacker.rs`                 | 3759        | **本轮再切**(框架 / 编辑器实现)              |
| `api/work.rs`                      | 2533        | **不切分**(团队决策,维持单文件,见「不落地」) |
| `core/cloudvar.rs`                 | 2495        | 浅切收益小,深拆成本高,**不切**(见「不落地」) |
| `core/retrieve.rs` / `pipeline.rs` | 1263 / 1133 | 语义内聚,维持单文件                          |
| `utils/requests.rs`                | 1942        | 命名统一落点                                 |

**命名统一影响面量化**(本轮改名的下游可见标识符):`HTTPStatus` 131、`Catsona` 62、`KittyAuth` 19、`KittyRequestBuilder` 15、`KittyConfig` 12;README 出现点:Catsona(L78/210)、KittyAuth(L213)、MewResult(L80/84/97,保留)、MewError(L215,保留)。**`MewError`/`MewResult` 按团队决策保留**(crate 品牌名,下游已习惯),其余萌化名改直白。

**新检查结论**:上轮改动零回归(`cargo check`/`test`/`clippy` 全绿,真机 `live_features` 3 passed);`auth.rs` 1370 行、`requests.rs` 1942 行,无新问题;`LoginCredentials` 仍被 `AuthManager.login` 使用,无死代码。

### 优先级总表

| 优先级 | 编号 | 建议                                                        | 破坏 API?    | 性质         |
| ------ | ---- | ----------------------------------------------------------- | ------------ | ------------ |
| P1     | #1   | unpacker.rs 再切(框架 / 编辑器实现)                         | 否(路径不变) | 可读性       |
| P1     | #2   | 公开 API 萌化名→直白名(6 个;`MewError`/`MewResult` 保留)    | 是           | DX/命名      |
| P1     | #3   | `WorkId` newtype 试点(反编译链)                             | 是           | 类型安全     |
| P2     | #4   | README/CONTRIBUTING 同步优化(目录结构/命名/锁条款)          | 否           | 文档         |
| 不做   | #5   | work.rs / cloudvar 切分、retrieve / pipeline、#11 HTTP 注入 | —            | 见「不落地」 |

---

## Approach

四个阶段相互独立四个阶段相互独立,按优先级顺序执行(每阶段结束 `cargo check --all-targets` 绿)。

### Phase 1 — unpacker.rs 再切:框架 / 编辑器实现(P1)

**问题定位**:`unpacker.rs` 3759 行(第十轮从 compiler.rs 切出的引擎),内部是共享框架(错误/配置/模型/文件/加密/积木块核心/契约)+ 各编辑器实现(5 组 fetcher/decompiler + BlockDecompiler impls)的混合,按「框架 vs 实现」再切两文件。

**切分方案(纯搬迁,零签名改写)**,边界以当前行号为准:

**`src/core/unpacker.rs`(保留,共享框架)** — L1-1597 + L3708-3765:

- use 块(L1-21)+ 错误(L22-156)/ 配置(L158-565)/ 模型(L566-679)/ 文件与 id(L680-756)/ 加密(L756-853)/ 积木块核心(`ShadowBuilder`/`BlockBehavior`/`BlockContext`/`BlockDecompilerCore`,L854-1450)/ 上下文(`DecompilerContext`/`Builder`,L1452-1532)/ 契约(`DecompileResult`/`RawWorkData`/`WorkFetcher`/`WorkDecompiler`/`save_json_result`/`save_path_result`,L1533-1596)+ `HttpClient`/`CodeMaoHttpClient`(L3709-3765)。
- `save_json_result`(L1561)/`save_path_result`(L1587)两个私有 fn 被 decoders 调用 → 升 `pub(crate)`(2 处可见性提升,与第十轮 compiler 切分同款)。

**`src/core/decoders.rs`(新,各编辑器实现,8 字母)** — L1598-3707(连续):

- 5 组 fetcher + decompiler(Neko/Kitten/Nemo/Wood/Coco)+ resource managers + `XmlBlockWriter`(L1598-3191)+ `BlockDecompiler` trait(L3195)+ 9 个 impl(L3199-3633)+ `create_block_decompiler`(L3663)+ `BlockDecompilerFactory`(L3690-3707)。
- 顶部补 `use crate::core::unpacker::{…}`。**实测引用的框架项**(按次数):`Result`(46)/`RawWorkData`(20)/`DecompilerConfig`(17)/`BlockDecompilerCore`(17)/`FileService`(16)/`DecompileResult`(15)/`BlockContext`(15)/`HttpClient`(13)/`DecompilerContext`(13)/`WorkType`(9)/`BlockBehavior`(9)/`WorkInfo`(6)/`WorkFetcher`(5)/`WorkDecompiler`(5)/`IdGenerator`(4)/`save_json_result`(3)/`CryptoService`(3)/`ShadowBuilder`(2)/`save_path_result`(2)/`BCMKNDecryptor`(1);**`ResultExt` 必须导入**(`with_context` 4 处 trait 方法调用);`DecompilerContextBuilder`/`ValueExt` 实测 0 引用,不必导入。+ 原有外部 use(aes_gcm/base64/sha2/serde_json/log/std)按需复制。

**`src/core.rs`**:加 `pub(crate) mod decoders;`(私有)。

**`src/core/compiler.rs`(门面 use 分流)**:`use crate::core::unpacker::{…}` 中的 10 个编辑器项(`CocoDecompiler`/`CocoFetcher`/`KittenDecompiler`/`KittenFetcher`/`NekoDecompiler`/`NekoFetcher`/`NemoDecompiler`/`NemoFetcher`/`WoodDecompiler`/`WoodFetcher`)改从 `crate::core::decoders` 导入;其余(`CodeMaoHttpClient`/`DecompilerConfig`/`DecompilerContextBuilder`/`FileService`/`HttpClient`/`IdGenerator`/`RawWorkData`/`Result`/`ResultExt`/`WorkDecompiler`/`WorkFetcher`/`WorkInfo`/`WorkType`)留在 unpacker。

**影响面与风险**

- 公开路径不变(unpacker/decoders 均私有,公开面仅 compiler.rs 的 `pub use DecompilerError` 等);依赖单向 decoders → unpacker(**实测框架段 L1-1597 对 decoders 内容零反向引用**);纯移动无逻辑变更。
- 风险:compiler 门面 use 分流遗漏 → `cargo check` 定位;`save_json_result`/`save_path_result` 可见性提升 2 处。

**验证**:`cargo check --all-targets`;`grep -n "CocoFetcher\|KittenFetcher\|NekoFetcher\|NemoFetcher\|WoodFetcher" src/core/compiler.rs` 均从 `decoders` 导入;`use backend::core::compiler::{DecompileOptions, decompile_work, decompile_works}` 在 tests 仍编译。

---

### Phase 22 — 公开 API 萌化名→直白名(6 个;`MewError`/`MewResult` 保留)(P1,0.1.0 窗口)

**问题定位**:萌化名泄漏进公开 API,下游必写面(身份、认证 trait、builder、状态码)「猜不中」,违 DX First「清晰优于简洁」。**`MewError`/`MewResult` 按团队决策保留**(crate 品牌名,错误模型段落与示例已稳定,改名破坏面大而收益低)。

**改动 — 映射表**(全仓机械改名,方法体/逻辑不动):

| 现名                                    | 新名              | 全仓计数 | 说明                                                                                                                                                                    |
| --------------------------------------- | ----------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Catsona`                               | `Identity`        | 62       | 成员 `Fluffy/Scholar/Judge/Blanky` 保留;辅助项同步:`Catsona::ALL`(requests.rs:355)、`index()`(247/375/388)、`FromStr`(209)、`AccountStatus::to_identity()`(auth.rs:133) |
| `HTTPStatus`                            | `HttpStatus`      | 131      | RFC 命名惯例(HTTP→Http)                                                                                                                                                 |
| `KittyAuth`                             | `AuthProvider`    | 19       | 认证提供者 trait                                                                                                                                                        |
| `KittyConfig`                           | `ClientConfig`    | 12       | 客户端配置                                                                                                                                                              |
| `KittyRequestBuilder`                   | `RequestBuilder`  | 15       | 请求构建器                                                                                                                                                              |
| `KittyIdentityManager`(已 `pub(crate)`) | `IdentityManager` | 9        | 内部实现                                                                                                                                                                |

**保留不改**:`MewError`/`MewResult`(团队决策);`KittyCore`(私有)/`KITTY_HEADERS`(私有)/`generate_meow_id`(私有);`GlobalKittyAuth`/`LocalKittyAuth`(已 `pub(crate)` 且实测零跨文件引用,内部名保留萌化,不列入改名);`BaseKey`/`ResponseMode`/`ToggleAction`/`UploadChannel`(非萌化,不动)。

**改名顺序**(每步 `cargo check` 零残留):

1. 身份:`Catsona`→`Identity`、`KittyIdentityManager`→`IdentityManager`(requests.rs + auth.rs `status.to_identity()` + pipeline.rs `switch_identity(Catsona::Judge)` 等全仓 use/构造点)。
2. 认证与配置:`KittyAuth`→`AuthProvider`、`KittyConfig`→`ClientConfig`。
3. 请求与状态码:`KittyRequestBuilder`→`RequestBuilder`、`HTTPStatus`→`HttpStatus`。
4. 同步 `src/prelude.rs` 重导出(保留 `MewError`/`MewResult`,`KittyRequestBuilder`→`RequestBuilder` 等)。

**影响面与风险**

- 公开 API 改名(SemVer breaking),0.1.0 窗口内做;`cargo check` 逐残留定位,机械可逆。`MewError`/`MewResult` 不动,错误模型段落/示例零连带。
- 风险:`Catsona` 成员名与 `ALL` 数组同步;`HTTPStatus` 131 处含 `as u16`/`From<HTTPStatus> for u16` 等 impl,纯标识符替换无逻辑改动。

**验证**:`grep -rn "Catsona\|KittyAuth\|KittyRequestBuilder\|KittyConfig\|HTTPStatus" src/ tests/ README.md` → 0;`grep -rn "MewError\|MewResult" src/` 保持原样计数(64/509,不归零);`cargo check --all-targets` + `cargo test` + `cargo clippy` 零新增警告。

---

### Phase 33 — `WorkId` newtype 试点(反编译链)(P1)

**问题定位**:反编译链 `work_id: i64` 裸类型(`unpacker.rs` 的 `WorkFetcher`/`WorkInfo`/`fetch_work_info`,`compiler.rs` 的 `decompile_work(work_id: i64, …)`/`decompile_works(&[i64], …)`/`CodemaoDecompiler::decompile`),与 `user_id`/`admin_id` 同为裸 i64,无法编译期区分。

**改动(试点范围:仅 core 反编译链,不做 api 层 13 manager)**:

1. 新类型(置于 `src/core/unpacker.rs` 框架段,`pub` 经 `compiler.rs` `pub use` 暴露):

```rust
/// 作品 ID 新类型:与 user_id/admin_id 等裸 i64 区分,编译期防混用
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkId(i64);

impl WorkId {
    pub fn new(id: i64) -> Self { Self(id) }
    pub fn get(self) -> i64 { self.0 }
}
impl From<i64> for WorkId { fn from(id: i64) -> Self { Self(id) } }
impl From<WorkId> for i64 { fn from(id: WorkId) -> i64 { id.0 } }
impl std::fmt::Display for WorkId { … }
```

2. 签名收窄:unpacker.rs 框架段的 `WorkInfo.id`/`fetch_work_info` 与 decoders.rs 各 fetcher 的 `work_id: i64` → `WorkId`(以 `cargo check` 定位全链);`compiler.rs` 的 `decompile_work(work_id: WorkId, …)`、`decompile_works(work_ids: &[WorkId], …)`、`CodemaoDecompiler::decompile(work_id: WorkId, …)`、`fetch_work_info(&self, http_client, work_id: WorkId)`。

3. 调用点:README 示例 5 `decompile_work(123456, None)` → `decompile_work(123456.into(), None)`(或 `WorkId::new(123456)`);`tests/compile_live.rs`/`live_features.rs` 的 `decompile_work_with`/`decompile_works` 调用点同步;URL 拼接处 `format!("…/works/{}", work_id)` 经 `Display` 自动适配。

**影响面与风险**

- 公开签名破坏(0.1.0 可接受);试点范围小(仅反编译链,~10 处签名 + ~8 调用点),验证收益后再推广 `UserId`(api 层全量 newtype 不做,见「不落地」)。
- 风险:unpacker 内 `work_id` 字段(如 `WorkInfo.id`、fetcher 返回)若被当作普通 i64 计算需 `work_id.get()`;以 `cargo check` 逐点定位。

**验证**:`cargo check --all-targets`;`decompile_work(123456.into(), None)` 编译通过;`cargo test`(compile_live 真机反编译路径)。

---

### Phase 44 — README/CONTRIBUTING 同步优化(P2,文档)

**问题定位**:第十轮 + 本轮改后,README/CONTRIBUTING 与实际代码漂移(目录结构未反映 compiler 拆分与 prelude;命名约定段仍是旧萌化名;锁条款缺第十轮澄清)。

**改动 — README.md**:

1. **目录结构段(L229-248)**:
    - `lib.rs` 注释 `# 库入口(公开 api/core/utils)` → `# 库入口(公开 api/core/prelude/utils)`。
    - `core/` 下列出实际文件:`cloudvar.rs`、`compiler.rs # 作品反编译门面(DecompileOptions/CodemaoDecompiler/便捷函数)`、`unpacker.rs # 反编译引擎:抓取/解密/积木块反编译/序列化`、`decoders.rs # 各编辑器实现(Neko/Kitten/Nemo/Wood/Coco)`(Phase 1 后)、`converse.rs`、`pipeline.rs`、`registry.rs`、`retrieve.rs`、`services.rs`、`terminal.rs`。
    - `src/` 级补一行 `prelude.rs # 常用类型与 trait 预导入`。
2. **设计要点段**:L210 `Catsona`(普通用户 Fluffy / 教育 Scholar / 评审 Judge / 空白 Blanky)→ `Identity`;L213 `KittyAuth` → `AuthProvider`;L215 错误模型段落保留 `MewError`/`MewResult`(已含 `Auth`/`InvalidArgument`),不动。
3. **示例 1(L78-97)**:注释「映射到 `Catsona::Scholar`」→「映射到 `Identity::Scholar`」;`MewResult<Value>` 保留。
4. **示例 5**(反编译):`decompile_work(123456, None)` → `decompile_work(123456.into(), None)`(与 Phase 3 同步)。

**改动 — CONTRIBUTING.md**:

1. **命名约定段(L28)**:更新为「基础设施层沿用「萌化」命名约定(已在 `utils/requests.rs` 落地):**公开名直白化**——身份 `Identity`、认证 `AuthProvider`、客户端配置 `ClientConfig`、请求构建器 `RequestBuilder`、状态码 `HttpStatus`;**保留**错误类型 `MewError`/`MewResult` 与内部私有萌化名(`KittyCore` 等);业务域命名保持直白(如 `CaptchaManager`、`ReportProcessor`)。新代码要和所在模块既有命名风格一致。」
2. **锁条款(L29)**:追加第十轮澄清——「默认使用标准库锁;仅当测量证明出现竞争/中毒热点时,经评审可对粗粒度令牌(`connect_lock`/`network_lock`)引入 `parking_lot::Mutex`,对读多写少 Map 引入 `RwLock`。」
3. **错误条款(L35)**:与 L38 错误模型段一致,补「`Auth`/`InvalidArgument` 用于域错误,不用裸字符串」——已含,微调措辞即可(可选)。

**影响面与风险**:纯文档;与代码保持同步,无行为影响。README 示例需与 Phase 22/33 的改名/签名一致(执行顺序:Phase 22/33 改代码 → Phase 44 同步文档)。

**验证**:`cargo test` 的 doc-test 通过(README 示例非 doc-test,以人工核对 + `cargo check` 为准);`grep -n "Catsona\|KittyAuth" README.md CONTRIBUTING.md` → 0。

---

## 不落地(记录在案)

- **work.rs 切分切分**:团队决策**不切分**(2533 行维持单文件,已评估切 2 文件可做但放弃文件可做但放弃);后续如需可再切出后续如需可再切出 `WorkDataFetcher`(最大单结构 ~840 行,纯搬迁)。
- **cloudvar.rs 浅切**:batching 段(L203-297)与 model 段(L45-201)合计仅 ~250 行;主结构 `CloudInner` 状态机(L570-2495,~1900 行)深拆需 60+ 可见性提升 + `Arc<CloudInner>` 贯通,收益/成本差;model 段难取 8 字母名(候选 `cloudval`=8,不自然)。**不切**。
- **`MewError`/`MewResult` 改名**:团队决策保留(品牌名 + 破坏面大),不列入 Phase 22。
- **retrieve.rs(1263)/pipeline.rs(1133)**:语义内聚(数据查询 / 举报处理核心),维持单文件。
- **#11 反编译 HTTP 注入(`HttpClient` 公开)**:维持不做(反编译 HTTP 已可经 `CodeMaoClient` 注入)。
- **api 层全量 newtype ID**:13 manager 数百签名机械量大、收益递减,仅反编译链试点 `WorkId`。
- **`terminal.rs` 迁出**:维持第十轮结论(可外部调用的举报控制台 UI 组件,保留在库内)。

## Critical files & anchors

| 文件                                                                   | 锚点                                                                   | 原因                                          |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | --------------------------------------------- |
| `src/core/unpacker.rs:1-1597,1598-3191,3195-3707,3708-3765`            | 框架段 / 编辑器段 / BlockDecompiler 段 / HTTP 段                       | Phase 1 切分边界 + WorkId 落点                |
| `src/core/compiler.rs:1-15`(use 列表)+ `src/core.rs`                   | 门面 use 分流 + `mod decoders;`                                        | Phase 1 同步点                                |
| `src/utils/requests.rs:183-227,258-298,333-342,483,1713-1796`          | `Catsona`/`KittyConfig`/`KittyAuth`/`KittyRequestBuilder`/`HTTPStatus` | Phase 2 改名落点(`MewError`/`MewResult` 不动) |
| `README.md:78-97,149-161,210-215,229-248` / `CONTRIBUTING.md:28-29,35` | 示例/设计要点/目录结构 + 命名/锁/错误条款                              | Phase 3/4 落点                                |
| `src/prelude.rs`                                                       | 重导出同步(保留 `MewError`/`MewResult`)                                | Phase 2 同步点                                |

## Verification

前置:每阶段结束 `cargo check --all-targets` 0 error;最终 `cargo clippy --all-targets` 不新增警告;`cargo test` 全绿(库单测 + `compile_live` + `live_features` 无配置时自动跳过)。

归零 grep 验证(最终态):

1. **Phase 1**:`grep -n "CocoFetcher\|KittenFetcher\|NekoFetcher\|NemoFetcher\|WoodFetcher" src/core/compiler.rs` 均从 `decoders` 导入;`wc -l src/core/unpacker.rs src/core/decoders.rs` 两文件各 <2200 行;`sed -n '1,1597p' src/core/unpacker.rs | grep -c "NekoFetcher\|BlockDecompiler\b\|XmlBlockWriter"` → 0(框架段零反向引用)。
2. **Phase 2**:`grep -rn "Catsona\|KittyAuth\|KittyRequestBuilder\|KittyConfig\|HTTPStatus" src/ tests/ README.md` → 0;`grep -rn "MewError\|MewResult" src/` 计数保持(64/509,未误改)。
3. **Phase 3**:`grep -n "decompile_work" README.md tests/` 为 `WorkId` 参数形态;`grep -n "work_id: i64\|work_id: &i64" src/core/unpacker.rs src/core/decoders.rs` → 0。
4. **Phase 4**:`grep -n "Catsona\|KittyAuth" README.md CONTRIBUTING.md` → 0;`grep -n "unpacker.rs\|decoders.rs\|prelude.rs" README.md` 命中目录结构段;`grep -n "parking_lot" CONTRIBUTING.md` 命中锁条款。

新行为检查:

- Phase 3:`decompile_work(123456.into(), None)` 编译通过,返回 `Result<PathBuf>`(真机 `compile_live` 验证产物落盘)。

其余行为以其余行为以 code review + 编译为准:Phase 1 为纯搬迁,Phase 22/33 为改名 + 签名收窄(语义等价),Phase 44 为文档同步,不改任何端点/请求体/落盘行为。

## Assumptions & contingencies

- **0.1.0 破坏窗口**:Phase 22(6 个标识符)、Phase 33(`WorkId`)属 SemVer breaking,假设 1.0 前可接受;**若不可接受**,Phase 22/33 降级为「doc 标注 deprecated 别名,1.0 移除」(Phase 1/44 不受影响,照做)。
- **work.rs 维持单文件维持单文件**:团队已决策不切分,后续如需再切 `WorkDataFetcher`(纯搬迁,re-export 保路径保路径)。
- **`Identity` 命名异议**:若团队不认可 `Catsona`→`Identity`,可保留 `Catsona`(身份是领域概念)——但 0.1.0 窗口错过即永久,默认按映射表改。
- **unpacker 再切命名**:默认 `decoders`(8 字母,「解密+反编译」语义);备选 `blockdec`(8,侧重积木块)。以 `cargo check` 确认 10 个编辑器项从 decoders 导入后零残留。
- **`WorkId` 定义位置**:默认 `unpacker.rs` 框架段定义 + `compiler.rs` `pub use`;若实现时发现门面更合适(无 unpacker 内部引用),就地定义,以 `cargo check` 为准。
- **`WorkInfo.id` 类型**:unpacker 内 `WorkInfo.id: i64`(服务端返回)改 `WorkId` 需在 `from_api_response` 处转换(用 `WorkId::new(v.as_i64()…)`),保持 `WorkId` 只作类型边界、内部计算用 `.get()`。
- **文档执行顺序**:Phase 4 必须在 Phase 2/3 之后(README 示例需与改名/签名一致);若跳过 Phase 2/3,Phase 4 仅做目录结构 + 锁条款部分,命名段不动。
- **真机验证依赖账号**:Phase 3 端到端需有效账号;无账号时退化为 `cargo check --all-targets` + `cargo test`(集成测试自动跳过)。

## Verification(实际执行结果)

- `cargo check --all-targets` 0 error;`cargo clippy --all-targets` 0 warning。
- `cargo test` 全绿:库单测 5+5 passed、`compile_live` 1 passed(NEMO 1 ignored)、`live_features` 3 passed(真机命中 codemao 服务)、doc-tests 0。
- 归零验证:6 个旧名(`Catsona`/`KittyAuth`/`KittyConfig`/`KittyRequestBuilder`/`HTTPStatus`/`KittyIdentityManager`)src/tests/README/CONTRIBUTING → 0(实际 240 处替换);`MewError`(64)/`MewResult`(506)保留未动。
- 文件规模:`unpacker.rs` 3759 → 1296 行(框架),`decoders.rs` 2491 行(编辑器实现 + BlockDecompilerCore)。
- `WorkId` 全链收窄:compiler.rs 7 处签名 + unpacker/decoders 字段,`decompile_work(123456.into(), None)` 编译通过。

## 范围偏差(实际执行中确定,记录在案)

- **`BlockDecompilerCore` 一并移去 decoders**:方案假设「框架段零反向引用」,实测 `BlockDecompilerCore`(框架段)内部调用 `create_block_decompiler`(decoders),框架不能自洽。修正:将 `BlockDecompilerCore` 段(L1095-1448,355 行)也移去 decoders(与 BlockDecompiler trait/impls/factory 同族),框架段不再引用 decoders,单向依赖成立。use 清单相应调整(删 `BlockDecompilerCore` 导入、加 `BlockDecompilerBehavior` trait——`get_child_input_name` 是它的方法)。
- **`KittyRequestBuilder` → `MewRequestBuilder`(非 `RequestBuilder`)**:`RequestBuilder` 已被 `ureq::RequestBuilder`(requests.rs:14 import + 7 处类型引用)占用,改名 `RequestBuilder` 会冲突;改 `MewRequestBuilder` 与 `MewError`/`MewResult` 同品牌族,直白度仍提升。
- **`HTTPStatus` → `StatusCode`(非 `HttpStatus`)**:`HttpStatus` 与 `MewError::HttpStatus`(结构化错误变体,requests.rs:26)同名易混;改 `StatusCode`(http crate 标准名),`check_status(builder, StatusCode::Ok)` 更直觉。
- **decoders 曾出现 `BlockDecompilerCore` 双份定义**(第一次移动时插入逻辑造成),已删除第二份副本,归零确认 1 份。
