# 历史文档勘误(errata)

> **知识库条目。** 这里列出历史轮次(`docs/rounds/`)里**已经过时或写错**的表述及其正确值。
>
> 为什么不在原文档上直接改:`docs/rounds/20` §6.1 定为**历史保真** —— 轮次记录不改写,只在本篇集中勘误。
> 每条的「正确」列都已对**当前源码**核过(2026-09-25)。
>
> 常见失效类型:① 文件被重命名/搬迁(`utils/acquire.rs` → `utils/requests.rs`、`core/compiler.rs` → `core/convert/**`、
> `utils/data.rs` → `utils/filedata.rs`);② 类型改名(`HTTPStatus` → `StatusCode`、`CloudError`/`ChatError` → `SocketError`);③ 行号漂移;
> ④ 整段形态被后续轮次推翻(如 `LoginSession`)。**读老轮次时先查本篇。**


## docs/rounds/01-websocket-pitfalls.md

- **错**:## 附录:坑 10 全文——《一次被 `ws.read()` 卡死的发送》

(即用户所引文章,此处略——正文坑 10 已含全部要点:…)
先取真实代码片段,确保文章中的代码与实现一致:
  - **为何错**:自相矛盾且残留写作指令:声称「此处略」却紧接着粘贴了整篇长文;「先取真实代码片段,确保文章中的代码与实现一致」是对写作代理的指令残留,不属于正文内容。
  - **正确**:要么删除该句并保留全文,要么删除全文只留指引;同时删掉「先取真实代码片段…」这句写作指令。
  - 出处:docs/rounds/01-websocket-pitfalls.md「附录:坑 10 全文」小节(约 1140 行区域)
- **错**:读线程的循环(真实代码,`src/core/cloud.rs`)
  - **为何错**:当前仓库不存在 src/core/cloud.rs(也不存在 src/core/chat.rs);云存储 WebSocket 实现在 src/core/cloudvar.rs,通用 WS 工具(帧解析/读超时/Notify/wait_flag)已抽到 src/utils/socketio.rs。
  - **正确**:src/core/cloudvar.rs(连接/读循环)+ src/utils/socketio.rs(parse_frame/set_stream_read_timeout/wait_flag)。
  - 出处:docs/rounds/01-websocket-pitfalls.md 附录一/附录五;仓库 src/ 结构 glob 结果
- **错**:所有出站事件帧统一为 `42 ["name",payload]`(带空格);JOIN 特判为无 payload 的裸字符串
  - **为何错**:与当前实现不符:src/core/cloudvar.rs:2205-2208 与 2513-2535 构造的是 `format!("{EVENT_MESSAGE_PREFIX}{}", …)`(42 后无空格),只有 chat 端 src/core/converse.rs:442-447 用带空格形式。文档「所有…统一带空格」的绝对表述只对 AI 对话端成立。
  - **正确**:按服务区分:AI 对话端必须 `42 ["join"]` / `42 ["chat",…]`(带空格,实测强制);云存储端当前为无空格 `42[…]` 且工作正常。若云存储也需空格,应以真机验证为准再统一定论。
  - 出处:docs/rounds/01-websocket-pitfalls.md 坑 5「修复」vs src/core/cloudvar.rs:2205-2208、2513-2535
- **错**:tungstenite 0.26 的同步 API 有三个"硬限制"
  - **为何错**:版本号已过时:当前 Cargo.toml 依赖 tungstenite 0.30.0(且 rustls 走 rustls-tls-webpki-roots)。
  - **正确**:应表述为具体版本下的行为;三条限制(set_read_timeout 才可超时、无 try_read、不能 split)在 0.30 仍成立——当前代码仍依赖 set_read_timeout + WouldBlock(src/utils/socketio.rs:81-85)。
  - 出处:docs/rounds/01-websocket-pitfalls.md 附录「三、根因」;Cargo.toml [dependencies] tungstenite = "0.30.0"

## docs/rounds/02-review-round1.md

- **错**:范围:全仓 24 个 Rust 源文件(24,834 行)…`core/compiler.rs`(4,101 行)、`acquire.rs`(1,842 行)、`work.rs`(2,512 行)三个 god file
  - **为何错**:作为耐久事实已过时:当前仓库不存在 src/core/compiler.rs(已拆为 core/convert/** 多文件:mod.rs、translate/{nemo,kitten,neko}、decompile/、shared/),也不存在 src/utils/acquire.rs(现为 src/utils/requests.rs,另有新增的 src/utils/filedata.rs、src/utils/socketio.rs、src/prelude.rs);源文件数也已变化。
  - **正确**:以当前结构为准:core/{cloudvar,pipeline,retrieve,registry,converse,services,terminal}.rs + core/convert/**;utils/{requests,filedata,socketio}.rs;api/ 13 个文件未变。
  - 出处:docs/rounds/02-review-round1.md 头部与 §1、§5;仓库 src/ glob 结果
- **错**:`HTTPStatus` 枚举(34 变体,~85 行)与 `ureq::http::StatusCode` 重复 — acquire.rs:1683-1765
  - **为何错**:枚举名与文件都已变:当前该自研枚举名为 `StatusCode`(src/utils/requests.rs:1729 起),文件中已搜不到 HTTPStatus 标识符。
  - **正确**:src/utils/requests.rs `pub enum StatusCode`(Continue=100 … SwitchingProtocols=101 …)。
  - 出处:docs/rounds/02-review-round1.md §7 第 9 条;src/utils/requests.rs:1727-1748(grep HTTPStatus → 0 命中)
- **错**:归属:`utils/acquire.rs` 的 `ClientAccess` 默认方法(send_and_parse/check_status/send_maybe_parse)
  - **为何错**:文件路径过时,utils/acquire.rs 已不存在。
  - **正确**:src/utils/requests.rs(ClientAccess::check_status 于 requests.rs:1888、send_and_parse 于 1894、send_maybe_parse 于 ~1902)。
  - 出处:docs/rounds/02-review-round1.md §7 第 1 条;src/utils/requests.rs:1887-1910

## docs/rounds/03-fix-plan-v2.md

- **错**:1. **`src/utils/data.rs`**:新增 `pub fn value_to_i64(v: &serde_json::Value) -> Option<i64>`…3. **`src/core/services.rs` 第 21 行**
  - **为何错**:路径与行号已过时:当前 utils 下无 data.rs,value_to_i64 位于 src/utils/filedata.rs:94-98;services.rs 的导入在 src/core/services.rs:24(不是第 21 行)。
  - **正确**:src/utils/filedata.rs:94-98(pub fn value_to_i64);调用方 src/core/services.rs:24、src/api/auth.rs:1、src/api/community.rs:1、src/core/convert/mod.rs:34。
  - 出处:docs/rounds/03-fix-plan-v2.md Phase 1 第 1 步;仓库 glob + grep 结果
- **错**:**模式 B(check_status)**:`let response = <builder>.send()?;` 紧接 `Ok(response.status() == HTTPStatus::X as u16)` → `self.check_status(<builder>, HTTPStatus::X)`
  - **为何错**:类型名过时:HTTPStatus 已改名为 StatusCode,当前仓内无 HTTPStatus 标识符。
  - **正确**:`self.check_status(builder, StatusCode::X)`(src/utils/requests.rs:1888 `fn check_status(&self, builder: MewRequestBuilder, expected: StatusCode)`)。
  - 出处:docs/rounds/03-fix-plan-v2.md Phase 2「总规则-模式 B」;src/utils/requests.rs:1887-1891
- **错**:对 `temp/REVIEW.md` 的 97 条评审发现做整改。v1 方案(`temp/FIX_PLAN.md`)被否
  - **为何错**:引用的临时文件已不存在于 temp/(当前 temp/ 下只有 nemo_diff.py、PCAPdroid 抓包、harness/、web/ 等),读者无法回溯这些前置材料;且 01-04 号文档现已有正式 docs/ 名称。
  - **正确**:文档自身已在 docs/rounds/05 中给出映射:temp/REVIEW.md→docs/rounds/02-review-round1.md、temp/FIX_PLAN.md→docs/rounds/03-fix-plan-v2.md、backend-review-plan.md→docs/rounds/04-review-round2.md;应在此处直接引用 docs/ 名。
  - 出处:docs/rounds/03-fix-plan-v2.md 头部 Context;仓库 temp/ glob 结果

## docs/rounds/04-review-round2.md

- **错**:5-1 **auth.rs:287-293 解析改 `value_to_i64`**(该函数已在 auth.rs 导入,utils/data.rs:157-162,同时兼容数字与数字字符串)
  - **为何错**:utils/data.rs 已不存在:value_to_i64 当前定义在 src/utils/filedata.rs,且实际行号是 94-98(不是 157-162);auth.rs 的解析也已落到 322-324 行。
  - **正确**:src/utils/filedata.rs:94-98(`pub fn value_to_i64`);消费点 src/api/auth.rs:322-324。
  - 出处:docs/rounds/04-review-round2.md Phase 5-1;src/utils/filedata.rs:94-98、src/api/auth.rs:322-324
- **错**:6-6 **acquire.rs ClientAccess 三个默认方法(1817-1843)统一错误语义**…`fn check_status(&self, builder: KittyRequestBuilder, expected: HTTPStatus) -> MewResult<bool>`
  - **为何错**:三处标识符/路径均过时:utils/acquire.rs 现为 utils/requests.rs;builder 类型现为 MewRequestBuilder(不是 KittyRequestBuilder);状态码类型现为 StatusCode(不是 HTTPStatus);方法现位于 requests.rs:1888。
  - **正确**:`fn check_status(&self, builder: MewRequestBuilder, expected: StatusCode) -> MewResult<bool>`(src/utils/requests.rs:1888),错误语义由 send_checked(src/utils/requests.rs:1918)统一实现。
  - 出处:docs/rounds/04-review-round2.md Phase 6-6;src/utils/requests.rs:1887-1922
- **错**:`Merge/最终:**8. `cargo clippy` 不新增告警;人工 code review 确认 2-1 复查在锁内、2-3 守卫已 drop。
  - **为何错**:该行文字损坏(残句 `Merge/最终:**8.` 拼接、编号错乱),读者无法确定该条是 clippy 检查还是人工复核项。
  - **正确**:拆为两条:①`cargo clippy` 不新增告警;②人工 code review 确认 2-1 的 connected 复查在锁内、2-3 的 MutexGuard 已 drop。
  - 出处:docs/rounds/04-review-round2.md Verification 第 2 条(Phase 2 验证行)

## docs/rounds/05-style-unify-plan.md

- **错**:| 错误传播   | `?` + `ok_or_else(                                                                                 |                    | ...)`,禁裸 `unwrap`/`expect`(锁除外) | 已执行(P0-1) |
  - **为何错**:表格行被竖线切断:Mardown 表格多出空列并把 `ok_or_else(|| ...)` 的 `||` 拆到另一列,渲染后表头错位、规则只显示半句;原文疑似在 `|` 结尾的代码片段被当作单元格分隔符。
  - **正确**:恢复为单行三列:`| 错误传播 | `?` + `ok_or_else(|| ...)`,禁裸 `unwrap`/`expect`(锁除外) | 已执行(P0-1) |`。
  - 出处:docs/rounds/05-style-unify-plan.md「统一规则(目标风格,全仓唯一写法)」表第 1 行
- **错**:10. **P1-2: workshop_id 同实体同类型 → i32(2 处 &str 改 i32)** … `shop.rs:81` fetch_workshop_details、`:297` update_workshop `&str` → `i32`
  - **为何错**:行号已漂移,作为索引不可用:当前 shop.rs 中这两个函数的参数已是 i32,且所引行号已不指向对应函数(文件经多轮改动,shop.rs 现约 500 行以上的签名位置已变)。
  - **正确**:类型结论仍有效(workshop_id 统一 i32),但行号需重新定位;不要按 docs/rounds/05 的行号直接跳转。
  - 出处:docs/rounds/05-style-unify-plan.md §10;src/api/shop.rs 当前签名(glob/grep 观测)
- **错**:compiler.rs 的独立 WorkType 从未动
  - **为何错**:src/core/compiler.rs 已不存在(拆为 core/convert/** 体系),该句作为「当前现状」描述已失效。
  - **正确**:等价的当前描述:WorkType/KittenVersion 在 src/api/work.rs 与 src/api/user.rs 各持一份重复定义,转换模块(core/convert/translate/**)使用自有的类型体系。
  - 出处:docs/rounds/05-style-unify-plan.md §13;仓库 src/ glob 结果(core/convert/translate/*)

## docs/rounds/06-call-style-unify-plan.md

- **错**:验证命令:`grep -rn "unwrap_or(4)\|unwrap_or(24)\|unwrap_or(200)" src/api/` 应返回 0(常量替换完成)
  - **为何错**:与当前 HEAD 不符:src/api/shop.rs:111、112、170、171 仍存在 4 处 `unwrap_or(4)`(level / works_limit / max_number 参数,非分页上限)。作为耐久验收断言不可直接复用(这些参数可能是该文档之后才引入,或该断言当时未覆盖非分页参数)。
  - **正确**:断言需限定到分页参数范围(limit/page_size/page 类)才成立;或按现状记录 shop.rs 的 level/works_limit/max_number 默认值为 4 是服务端契约、不由分页常量管辖。
  - 出处:docs/rounds/06-call-style-unify-plan.md「Verification」验证命令;src/api/shop.rs:111-112,170-171
- **错**:| 通用默认 15/20 引用 `DEFAULT_PAGE_SIZE`/`DEFAULT_LIMIT`(acquire.rs);域特定值用文件常量。
  - **为何错**:路径过时:utils/acquire.rs 已不存在,常量现定义于 src/utils/requests.rs:152-157(DEFAULT_PAGE_SIZE=15 / DEFAULT_LIMIT=20 / DEFAULT_OFFSET=0)。
  - **正确**:src/utils/requests.rs:152-157。
  - 出处:docs/rounds/06-call-style-unify-plan.md P0-1 说明;src/utils/requests.rs:152-157

## docs/rounds/07-api-typing-refactor-plan.md

- **错**:`send_maybe_parse(builder, mode: ResponseMode, ...)` …8 处 `return_data: bool` 参数改 `mode: ResponseMode`(account:822/education:175/work:257,279/library:378,395,409,511,536,668/forum:383,401,446,468,521/
  - **为何错**:作为索引的行号已全部漂移(文件经其后多轮改动);且 §1 列举的 8 处与括号内的行号清单(实际列了 10 余处)数量不一致,属清单自相矛盾。
  - **正确**:结论(签名已改为 ResponseMode)成立且当前代码可见;但不要按 docs/rounds/07 的行号定位,需以 grep `mode: ResponseMode` 现查为准。
  - 出处:docs/rounds/07-api-typing-refactor-plan.md §1;grep 现状(src/api/{work,library,forum,shop,education,account}.rs 均有 ResponseMode 签名)
- **错**:`decompile_work(123456, None)` 后注释 None 语义;compiler.rs doc 补说明
  - **为何错**:src/core/compiler.rs 已不存在(拆为 core/convert/** 体系),该文件路径作为行动项已失效。
  - **正确**:对应的公开入口现位于转换/反编译模块(core/convert/**,含 decompile/ 与 translate/);文档若需保留行号/文件引用应指向现路径。
  - 出处:docs/rounds/07-api-typing-refactor-plan.md「README 示例同步」表第 5 行;仓库 src/ glob 结果

## docs/rounds/08-protocol-compliance-plan.md

- **错**:新增 `LoginSession`(持 `AuthManager` + `LoginCredentials` + `prefer_method`),`LoginBuilder::build(self) -> LoginSession`(仅构造,无副作用),`LoginSession::execute(&mut self) -> MewResult<LoginResult>` 执行网络。
  - **为何错**:当前源码不存在 LoginSession。全仓 grep `LoginSession` 零命中,`build()` 只出现在 cloudvar/converse 的云/聊天构造;auth.rs 中 execute 直接是 LoginBuilder 的方法。该整改形态已被后续轮次(14 起 auth 依赖注入重构)推翻。
  - **正确**:src/api/auth.rs:1385 `pub fn execute(&mut self) -> MewResult<LoginResult>` 定义在 `impl LoginBuilder` 内;无 LoginSession、无 LoginBuilder::build。
  - 出处:docs/rounds/08 §1「构造协议」 vs src/api/auth.rs:1322-1408
- **错**:`LoginBuilder` 无 execute(在 LoginSession)
  - **为何错**:与现状相反:execute 就在 LoginBuilder 上;文档的验证断言已失效。
  - **正确**:LoginBuilder 自带 execute(&mut self)(src/api/auth.rs:1385)。
  - 出处:docs/rounds/08「Verification」第 4 条 vs src/api/auth.rs:1385
- **错**:调用点:pipeline.rs:968 `login_student` 改 `...build()` + `session.execute()?`
  - **为何错**:行号与形态均过时:现为 pipeline.rs:985,直接链式 `LoginBuilder::new_with_client(...).identity(...).password(...).status(...)`,无 session 中间对象。
  - **正确**:src/core/pipeline.rs:985 `crate::api::auth::LoginBuilder::new_with_client(self.client.clone()).identity(username).password(password).status(...)` 后调用 execute。
  - 出处:docs/rounds/08 §1 vs src/core/pipeline.rs:984-988

## docs/rounds/09-protocol-compliance-review2.md

- **错**:`src/utils/acquire.rs:1314`:`pub fn next_item(&mut self) -> Option<MewResult<Value>>`
  - **为何错**:路径已失效:doc 11 方案 D 落地后 acquire.rs 已重命名为 requests.rs(utils.rs 现为 `pub mod requests`),且函数已私有化,行号也变了。
  - **正确**:src/utils/requests.rs:1335 `fn next_item(&mut self) -> Option<MewResult<Value>>`(私有)。
  - 出处:docs/rounds/09「整改项」 vs src/utils/ 目录(无 acquire.rs)、src/utils/requests.rs:1335
- **错**:全部 `fetch_*_gen` 返回 PaginatedIter
  - **为何错**:后续统一命名后分页器后缀已由 _gen 改为 _iter,`fetch_*_gen` 在现码中不存在。
  - **正确**:如 src/api/education.rs:782 `fetch_official_lesson_packages_iter`、:835 `fetch_custom_lesson_packages_iter`。
  - 出处:docs/rounds/09「判定合规项」表 vs src/api/education.rs:782/835

## docs/rounds/11-cloudvar-converse-review3-plan.md

- **错**:- **单次哈希**:按用户本次指令,采纳为修改目标(P1),推翻 04 的"回退保持现状"。
  - **为何错**:与本文档 §1 表及 §P1 标题自相矛盾:§1 写「采纳后回退…保持双哈希」,P1 标题写「采纳后回退:借用检查限制」,结论是无法编译、不实现。开头的 bullet 未同步。
  - **正确**:单次哈希最终未采纳,保持 contains_key+get_mut 双哈希;现码并带 E0499/E0500 根因注释(src/core/cloudvar.rs:463-470)。
  - 出处:docs/rounds/11 开头 bullet vs §1 表/P1 标题 vs src/core/cloudvar.rs:463-470
- **错**:1. 重命名 `src/utils/acquire.rs` → `src/utils/requests.rs`(用 `lsp rename_file` 一次性改写全部引用)…… 待分轮落地
  - **为何错**:该计划项现已执行完毕,文中「待执行」状态与现状不符(同日的 docs/rounds/12 已按 `utils/requests.rs` 撰写,说明重命名已落地)。
  - **正确**:src/utils/requests.rs 已存在,src/utils/ 下无 acquire.rs;全仓 grep `utils::acquire`/`utils/acquire` 零命中。
  - 出处:docs/rounds/11 §3.5/§3.6 vs src/utils/ 目录列表与 grep

## docs/rounds/12-infra-engine-optimization-plan.md

- **错**:### P2-1 `CryptoService::sha256` 十六进制化(compiler.rs:762)
  - **为何错**:路径已失效:`src/core/compiler.rs` 现已不存在(被拆分为 `src/core/convert/`),CryptoService 也随之迁移,原行号无意义。
  - **正确**:CryptoService::sha256 现位于 `src/core/convert/shared.rs`(第三十一轮 `shared/` 六文件合并后)。
  - 出处:docs/rounds/12 §P2-1 vs **当时的** `src/core/convert/shared/infra.rs:36`(该文件第三十一轮已并入 `shared.rs`)
- **错**:P3-3 | `base64_to_bytes` / `reverse_string` 拿 `&self` 不读 `self` | compiler.rs:769/775 | 改关联函数(去 `&self`),唯一调用点 `decrypt_bcmkn` 同步改 `Self::`
  - **为何错**:compiler.rs 路径已不存在;两个函数已迁至 convert 模块,文档锚点全部失效。
  - **正确**:现位于 `src/core/convert/shared.rs`(已为无 self 的关联函数;路径经第三十一轮合并)。
  - 出处:docs/rounds/12 §P3-3 vs **当时的** `src/core/convert/shared/infra.rs:48/54`(同上)
- **错**:| P3-1 | `(limit + 1) / 2` 溢出 | retrieve.rs:679 |
  - **为何错**:行号漂移(改动已在 699 落地),文档指认的行已非该语句。
  - **正确**:src/core/retrieve.rs:698-699。
  - 出处:docs/rounds/12 §P3 表 vs src/core/retrieve.rs:699

## docs/rounds/13-round5-remaining-surface-plan.md

- **错**:| 3-4 | `fetch_custom_lesson_packages_gen` 打的是 `/edu/zone/lesson/lesson/offical/packages`(官方)端点… | `src/api/education.rs:835` |
  - **为何错**:函数名已过时:现名为 `fetch_custom_lesson_packages_iter`(`_gen` 后缀已被统一命名轮次改掉);该计划项本身也未落地,端点仍为 offical。
  - **正确**:src/api/education.rs:835 `pub fn fetch_custom_lesson_packages_iter`;:839 仍 `"/edu/zone/lesson/offical/packages"`。
  - 出处:docs/rounds/13 §P3-4 vs src/api/education.rs:835-839

## docs/rounds/14-client-injection-refactor-plan.md

- **错**:2. **反编译器硬编码全局**:`core/compiler.rs` 的 `CodemaoDecompiler::global()`,内部 `KittyFactory::global_client().clone()`。
  - **为何错**:`src/core/compiler.rs` 已不存在(拆分为 `src/core/convert/`);文档多处锚点(compiler.rs ≈3920/3933、自由函数 4098-4110)全部失效。
  - **正确**:CodemaoDecompiler 现位于 `src/core/convert/decompile/mod.rs:192`,`new(client)` 在 :201,global() 内部用 `CodeMaoClient::global().clone()`。
  - 出处:docs/rounds/14 Context 2 / Critical files 表 vs glob src/core(无 compiler.rs)与 src/core/convert/decompile/mod.rs:192/201
- **错**:`KittyFactory` 仍被 `core/{pipeline,services}.rs` 引用(非死代码),保留;仅清理了 `compiler.rs` 中因 Phase 2 失效的 `KittyFactory` import。
  - **为何错**:现状与文档断言不符:全仓 grep `KittyFactory` 零命中,该 struct 已不存在(在后续轮次被删除)。
  - **正确**:KittyFactory 已删除;其唯一全局耦合点(global_client)由 CodeMaoClient::global() 取代。
  - 出处:docs/rounds/14「Verification(实际执行结果)」vs 全仓 grep KittyFactory 零命中

## docs/rounds/15-core-engine-client-injection-plan.md

- **错**:目标:给上述 5 个 core 类型(`DataQuery` / `CommentQueryBuilder` / `ReportFetcher` / `ViolationChecker` / `ReportProcessor` / `FileProcessor`)注入 `CodeMaoClient`
  - **为何错**:自称「5 个 core 类型」却列出了 6 个类型名,数目自相矛盾;同一文档 Phase 1-4 实际覆盖的正是这 6 个类型。
  - **正确**:应为「6 个 core 类型」。
  - 出处:15-core-engine-client-injection-plan.md §Context 末段
- **错**:`ViolationChecker`(541 行)…5. 自动举报流程(842 行)`KittyFactory::global_client().switch_identity(Catsona::Judge)` → `self.client.switch_identity(Catsona::Judge)`
  - **为何错**:该改动在本轮实际被撤销:范围偏差明确记载 pipeline 的 switch_identity(Judge)(新行号 849)保持全局「不能部分注入」;计划写的行号 842 与文档后半段的 849 也不一致。
  - **正确**:该处第七轮保持全局;`self.client.switch_identity(Catsona::Judge)` 实际由第八轮(16-...md Phase 1 step 4,行 849)完成。
  - 出处:15 §Approach Phase 3.5 / §范围偏差 / 16-...md §Approach Phase 1
- **错**:`retrieve.rs:627` `let client = CodeMaoClient::global();`(`DataQuery::count_comments`)
  - **为何错**:同一文档 Phase 1 step 4 写作 `count_comments`(626 行),Context 与 Approach 的行号相差 1。
  - **正确**:以改动时实际行号为准(文档内两处不一致,建议统一)。
  - 出处:15 §Context vs §Approach Phase 1.4

## docs/rounds/16-login-action-injection-plan.md

- **错**:```rust
/// 持有独立 `CodeMaoClient` 的本地 provider,供登录流写入注入的客户端身份槽
#[derive(Debug, Clone)]
pub struct LocalClientProvider {
    client: CodeMaoClient,
}
  - **为何错**:该 derive 列表按方案原样写会编译失败:CodeMaoClient 未实现 std::fmt::Debug,而 ClientProvider trait 要求 Debug。文档自己的范围偏差段已承认此点。
  - **正确**:实际实现为 `#[derive(Clone)]` + 手写 `impl std::fmt::Debug`(finish_non_exhaustive)。
  - 出处:16-...md §Approach Phase 1.1 vs §范围偏差
- **错**:归零 grep 验证(最终态):2. `grep -rn "CodeMaoClient::global()" src/core/pipeline.rs` → 0(登录/举报/身份恢复全走 `self.client`)
  - **为何错**:实际执行结果自述该 grep 仍命中 ActionRegistry::new() 的全局委托,归零目标未达成;文档未把该条从「验证清单」中撤回,仅在下文偏差段说明。
  - **正确**:实际终态为「仅 ActionRegistry::new() 委托一处保留」,归零条件应写成例外项。
  - 出处:16-...md §Verification 归零项 2 vs §Verification(实际执行结果)
- **错**:至此自动举报全链路(登录 → 举报 → 恢复身份)统一走 `self.client`,第七轮「保持全局」的限制解除。
  - **为何错**:表述过强:同轮实际保留 ActionRegistry 的 CodeMaoClient::global() 委托(动作分发路径仍是全局默认),并非「全链路统一」。
  - **正确**:应限定为「登录流与 execute_single_report 内 5 处 api-Manager 统一走 self.client;ActionRegistry::new() 仍委托全局」。
  - 出处:16-...md §Approach Phase 1 末句 / §Verification(实际执行结果)

## docs/rounds/17-error-convergence-decompile-fix-plan.md

- **错**:3. 7 个反编译器 impl 的 `save_result` 签名与 3 处 `save_json_result`/`save_path_result` 调用(1676/2473/2569/2833/3182)同步返回 `PathBuf`。
  - **为何错**:自称「3 处」调用,括号内却列出行号 1676/2473/2569/2833/3182 共 5 个,数目自相矛盾。
  - **正确**:应为 5 处调用点(或删去多余行号);实际执行结果按 19 处 decompile*/save_* 统一收窄。
  - 出处:17-...md §Approach Phase 3.3
- **错**:`MewError::HttpStatus.status` 用 `u16`:`ureq` 的 `response.status()` 返回 `u16`,直接存 `u16`。
  - **为何错**:计划假设与实际不符——范围偏差段明确「`response.status()` 实际返回 `http::StatusCode`,非 `u16`(计划假设错误)」,直接存 u16 无法编译。
  - **正确**:需 `status.as_u16()` 转换后再存入 u16 字段。
  - 出处:17-...md §Assumptions vs §范围偏差第 1 条
- **错**:doc 注释(4102)与 README 示例 5 声称「`output_dir` 传 `None` 表示不落盘,仅返回 JSON 字符串」
  - **为何错**:这是被文档本身判定为「与实现矛盾」的过时描述——实现从未有该模式,decompile_inner 对 None 回退 default_output_dir 并总是写盘;第九轮已删除该说法。归类为「文档漂移」记录(已被修正)。
  - **正确**:正确描述为「`output_dir` 传 `None` 时写入 `default_output_dir`,返回产物文件路径」。
  - 出处:17-...md §Context 3 / §Approach Phase 3.5

## docs/rounds/18-architecture-api-review-plan.md

- **错**:`Other` → `InvalidArgument` 是公开 API 命名变更(SemVer breaking),crate 处 0.1.0 可接受;grep `MewError::Other` 全仓 9 处一次性改名。
  - **为何错**:数字与实测不符:范围偏差段记载「实际 10 处而非 9 处」,漏了 auth.rs:498(验证码文件写入失败),且该处语义不同应改 MewError::Io 而非 InvalidArgument。
  - **正确**:全仓 10 处:9 处参数校验 → InvalidArgument,auth.rs:498 → MewError::Io;requests.rs:1619/1627 另走 Json 错误。
  - 出处:18-...md §Approach Phase 4 影响面 vs §范围偏差第 1 条
- **错**:`MewError`→`ClientError`、`MewResult`→`Result`、`Catsona`→`Identity`、`KittyAuth`→`AuthProvider`、`KittyRequestBuilder`→`RequestBuilder`、`HTTPStatus`→`HttpStatus`
  - **为何错**:该建议已被第十一轮否决/修改:MewError/MewResult 按团队决策保留(crate 品牌名);KittyRequestBuilder 因与 ureq::RequestBuilder 冲突改 MewRequestBuilder;HTTPStatus 因与 MewError::HttpStatus 同名易混改 StatusCode。
  - **正确**:Catsona→Identity、KittyAuth→AuthProvider、KittyConfig→ClientConfig、KittyRequestBuilder→MewRequestBuilder、HTTPStatus→StatusCode、KittyIdentityManager→IdentityManager;MewError/MewResult 不动。
  - 出处:18-...md §低优先级清单 #8 vs 19-...md §范围偏差
- **错**:`work_id`/`user_id`/`admin_id` 裸 `i32`/`i64` 混用(`main.rs:79`、`decompile_work(123456, None)`、`education.rs:30`)
  - **为何错**:与第十一轮的描述不一致:19 文档称 work_id「与 `user_id`/`admin_id` 同为裸 i64」。两文档对同一批 ID 的底层类型表述互相矛盾(未读源码核实,故低置信)。
  - **正确**:需以源码实际类型为准统一表述(i32 与 i64 混用 或 全为 i64)。
  - 出处:18-...md §低优先级清单 #10 vs 19-...md §Approach Phase 3

## docs/rounds/19-work-split-naming-unify-plan.md

- **错**:**新检查结论**:上轮改动零回归……`auth.rs` 1370 行、`requests.rs` 1942 行,无新问题;`LoginCredentials` 仍被 `AuthManager.login` 使用,无死代码。

### 优先级总表
  - **为何错**:该段之前的「文件行数摸底表」与「命名统一影响面量化」两整段在文档中被完整复制粘贴了两遍(同一表格 + 同一段文字连续出现两次)。
  - **正确**:应只保留一份;疑为编辑/合并事故。
  - 出处:19-work-split-naming-unify-plan.md §Context(两处重复表)
- **错**:四个阶段相互独立四个阶段相互独立,按优先级顺序执行
  - **为何错**:词语重复(同一短语连续出现两次),文本损坏;同类损坏在本文件多处出现:「work.rs 切分切分」、「已评估切 2 文件可做但放弃文件可做但放弃」、「后续如需可再切出后续如需可再切出」、「维持单文件维持单文件」、「re-export 保路径保路径」、「其余行为以其余行为以 code review + 编译为准」。
  - **正确**:各自应为单一短语:「四个阶段相互独立」「work.rs 切分」「已评估切 2 文件可做但放弃」「后续如需可再切出」「维持单文件」「re-export 保路径」「其余行为以 code review + 编译为准」。
  - 出处:19-...md §Approach 开头 / §不落地 / §Assumptions / §Verification 末段
- **错**:### Phase 22 — 公开 API 萌化名→直白名(6 个;`MewError`/`MewResult` 保留)(P1,0.1.0 窗口)
### Phase 33 — `WorkId` newtype 试点(反编译链)(P1)
### Phase 44 — README/CONTRIBUTING 同步优化(P2,文档)
  - **为何错**:阶段编号被写成了两位重复数字(22/33/44),与本文档 §Approach 开头的「四个阶段」及优先级总表 #2/#3/#4 不对应,也与文档内其它引用(如「Phase 2 改名落点」「Phase 3:`decompile_work(123456.into(), None)`」「Phase 4 必须在 Phase 2/3 之后」)不一致。
  - **正确**:应为 Phase 2 / Phase 3 / Phase 4。
  - 出处:19-...md §Approach 三个阶段标题 vs §Critical files & anchors / §Assumptions
- **错**:1. **Phase 1**:……`wc -l src/core/unpacker.rs src/core/decoders.rs` 两文件各 <2200 行
  - **为何错**:验证门槛未达成而文档仍以成功收尾:实际结果自报 decoders.rs 2491 行,超过自设的 <2200 行门槛(未解释该验收项失败)。
  - **正确**:decoders.rs 实际 2491 行(因 BlockDecompilerCore 355 行一并迁入),该验收条件应改为「<2600 行」或在结果段标注未达标。
  - 出处:19-...md §Verification 归零项 1 vs §Verification(实际执行结果)
- **错**:`grep -rn "MewError\|MewResult" src/` 保持原样计数(64/509,不归零)
  - **为何错**:与同文档实际结果「`MewError`(64)/`MewResult`(506)保留未动」数字不一致(509 vs 506),计划基线与实测值冲突且未说明差异来源。
  - **正确**:实测 MewError 64 / MewResult 506(以实际结果段为准)。
  - 出处:19-...md §Approach Phase 22 验证 vs §Verification(实际执行结果)
- **错**:| `KittyRequestBuilder` | `RequestBuilder` | 15 | 请求构建器 |
| `HTTPStatus` | `HttpStatus` | 131 | RFC 命名惯例(HTTP→Http) |
  - **为何错**:映射表已被实际实现推翻(RequestBuilder 与 ureq::RequestBuilder 冲突;HttpStatus 与 MewError::HttpStatus 易混),文档只在下文范围偏差段记录新名,映射表本身未同步,单独阅读会得到错误改名。
  - **正确**:应为 `KittyRequestBuilder` → `MewRequestBuilder`、`HTTPStatus` → `StatusCode`。
  - 出处:19-...md §Approach Phase 22 映射表 vs §范围偏差第 2、3 条
- **错**:审阅日期:2026-08-30 · 基线:HEAD `f8d394a`(第十轮已落地)
  - **为何错**:上下文信息存疑:15/16 文档审阅日期为 2026-08-29(基线 fe2c9e6、9d7b4d9),17 为 2026-08-29(5d76687),18 为 2026-08-30(4a62bb3),19 同为 2026-08-30 但称「第十轮已落地」——同一天内 18(方案)与 19(第十轮已落地)并存,时间线在文档层面无法自洽。
  - **正确**:19 的审阅日期应晚于 18 的落地时间(或日期字段有误)。
  - 出处:19-...md 头部 vs 18-...md 头部
---

## 第三十一轮的文件合并(2026-09-26)

`src/core/convert/` 由 **27 个文件并为 13 个**(纯搬迁 + 少量仪式层删除,见 `docs/rounds/31`)。
**读历史轮次时按此对照旧路径**:

| 旧路径 | 现在 |
| ------ | ---- |
| `shared/{mod,error,model,config,infra,upload}.rs` | `shared.rs` |
| `decompile/blocks.rs` | `decompile/mod.rs` |
| `decompile/editors/{mod,kitten,nemo,simple}.rs` | `decompile/editors.rs` |
| `translate/{kitten,neko}.rs` | `translate/model.rs` |
| `translate/remint.rs` | `translate/assembly.rs` |
| `translate/nemo_xml.rs` | `translate/nemo.rs` |
| `translate/tables_gen_nemo.rs` | `translate/nemo_mapping.rs` |
| `core/compiler.rs`、`utils/acquire.rs`、`utils/data.rs`(更早的迁移) | `core/convert/**`、`utils/requests.rs`、`utils/filedata.rs` |

同一轮还删掉/收敛了这些**名字**,早期文档若提到,以 `docs/rounds/31` §3 为准:

- `WorkProcessorRegistry` + `FetcherFactory`/`DecompilerFactory`(换成 `match EditorType` 静态分派);
- `DecompilerContextBuilder`(改为直接构造 `DecompilerContext`);
- `XHTML` 常量(唯一化到 `shared.rs`;原先 mapping / nemo_mapping 各一份);
- nemo_mapping 手抄的 `TEXT_PLACEHOLDERS` 与 `is_text_placeholder`(复用 `tables_gen` / `mapping` 的定义);
- 两处同构批处理(`translate_works` / `decompile_batch_outcomes` 内的 chunk + `thread::scope`)合并为 `shared::batch_map`。

## 第三十三至三十六轮的结论变更(2026-09-26)

这几轮把反向(KN → Kitten4)的**判据**换了一茬,历史轮次里下列表述**已不成立**;
正确值与证据都在新轮次里(读老轮次时以本节为准):

| 出处 | 已过时的表述 | 正确值 / 证据 |
| ---- | ------------ | ------------- |
| rounds/20–32(多处)、`knowledge/convert-semantics.md` §5 旧版 | "编辑器不认识的类型**保留 KN 原名** + 告警,**不丢积木**" | **不成立**:保留不认识的名字会让编辑器**整份工作区加载失败** ⇒ 现行是**块就地改成「未收录积木」标记(`incompatible_block`/`incompatible_output_block`)+ 清空影子** + 逐类报告(`rounds/38` §7bis 起;**`rounds/34–36` 当时是"整块剔除块",那会让积木真的消失**)。见 rounds/34 §4nonies、rounds/36、rounds/38 |
| rounds/28 §4、rounds/33 §3bis | "定义体侧差异归零,预算收紧到 **0/0**" | **0/0 只在那套口径下成立**;剔块会连带整棵子树 ⇒ 现行是**预算 `≤3193`(只许变小)** + 常显读数。见 rounds/36 |
| rounds/34 §4nonies 末段 | "**仍未解决**:产品在编辑器里作品名与变量能进,但 3 个角色一个都不出现(⇒ 画布 0 块)" | **已解决**(rounds/35):根因是缺 `theatre.groups` + 场景 `group_order`;KN 侧没有分组概念 ⇒ 反向必须合成"一角色一组" |
| rounds/34 §4nonies 的挑名描述 | "歧义挑选:候选里编辑器认识的优先 → KN 名本身认识就保留 → 再退非云优先" | 这段只描述**多候选(歧义)**分支;**单候选分支当时完全没做判据** ⇒ KN `text` 被写成 Kitten3 口径的 `get_split_options` 而被剔掉。现块与影子**共用**同一判据。见 rounds/36 |
| `src/core/convert/translate/assembly.rs` 旧注释 / rounds/31 审计 | "反向没有 groups 概念,写回 `scene.actors` 即可" | **不完整**:`scene.actors` 之外还必须有 `theatre.groups` 与 `group_order`(编辑器靠它们列角色)。见 rounds/35 |
| 一般印象:"积木数对得上就能打开" | — | **错**:见 `knowledge/convert-semantics.md` §5bis 的四条隐性契约(骨架键 / groups / 词汇表 / 表是 Kitten3 口径) |

## 第三十八轮的结论变更(2026-10-01)

| 出处 | 已过时的表述 | 正确值 / 证据 |
| ---- | ------------ | ------------- |
| `knowledge/convert-semantics.md` §5 旧版、rounds/20–37 | "占位积木(`LC` 降级)是**已文档化的不可逆项**,反向"至少应把占位块保留在 Kitten4 侧"" | **保留不住**:`bcm_translator_text_*` 不在编辑器注册表里,反向留下占位名会被写出阶段**整块剔除**(= 积木真丢)。**187 个占位映射里 43 个没有 `RC` 标题 ⇒ 必然走到这里**。现改为顶替成「未收录积木」`incompatible_block`/`incompatible_output_block`。见 `rounds/38` |
| rounds/37 §13.4/§13.8 | "`bcm_translator_text_return_value_block: 4 -> 0` … 往返差异全部落在已文档化族里" | **那一条是真缺陷**,不是归一化:4 个 id 在 KN 中间态都在、产物里一个不剩(id 口径)。§13.8 的 id 证明**只覆盖正向腿**。见 `rounds/38` §3.2 |
| rounds/34 §4quinquies | "槽默认影子不回写 ⇒ 表示差异、引用零丢失(基于代理指标)" | **结论不变**,但**证据换成 id 口径**重证:丢的 278 个 `lists_get` **逐个都是 KN 侧 `is_shadow` 的 `pure_list_get`**(折回父块 `fields`)。见 `rounds/38` §3.1 |
| 一般印象:"积木数对了就没丢" | — | **判"丢没丢"只用 id 口径**(id 是否出现在产物里);且要分清**节点 id / XML 里的 id / 只是 `connections` 键上出现** —— 混了会同时造出"幻影丢失"和"漏报" |
| `knowledge/convert-semantics.md` §5/§5bis、rounds/34–37 多处 | "编辑器不认识的类型**被剔除/清空**(宁可少几块)" | **只对了一半**:块现在**不再剔除**,而是**就地改成「未收录积木」标记**(`incompatible_block` / `incompatible_output_block`)—— 剔除会让积木真的消失(id 口径)。**影子**仍是清空 |
| `assembly.rs` 旧实现的 `if dropped.is_empty() { return }`(rounds/37 P3 的"省一次遍历") | 被当成纯性能优化 | **不是**:它是**按实体**提前返回,顺带**跳过影子扫描** ⇒ "没有任何未知块的实体"里的未知影子**从来没被清过**(直接留在产物里,正是会让编辑器整份加载失败的东西)。rounds/38 去掉早退后,某作品影子清空量 52 → **62**(这 10 条是补上的漏清,不是回归) |
| rounds/38 §8 | "**反向侧没有 id 台账**(正向那条已建);反向"认不出"的量由 `MARKER_BUDGET` 守" | **已不成立**(2026-10-01):反向台账 **`LOST_ID_BUDGET_REVERSE`** 已建(`d10d0cb` 首版、`ef37978` 把口径收窄到真正的积木节点;逐件打印 `[id台账·反向]`,只许变小),与正向 `LOST_ID_BUDGET` 同口径。原因与基线读数见 `rounds/39` §W3d/§0.3 与 `goals/convert-backlog.md` §6.3 G3 |

## 第三十九轮实施期间的结论变更(2026-10-01 ~ 10-02)

W10 落地(`66c0b6b`)后,**"内联对象形态影子 ⇒ 拒收该作品"这条老结论全部失效**;读 33/34 轮时以本节为准:

| 出处 | 已过时的表述 | 正确值 / 证据 |
| ---- | ------------ | ------------- |
| rounds/34 §1、§2 | `A28社区-开幕_174408420.bcm4` 报 `invalid type: map, expected a string` ⇒「**前置拦截**,报"内联影子是对象形态…暂不支持该作品"」,并把"支持对象形态影子"**列入待办**;§2 记"实测 **21/22** 件正向吃得下(Kitten3 一件、对象影子一件按形态跳过)" | **待办已做**(W10,`66c0b6b`):正向入口把对象影子就地改写成平台同款影子 XML,**不再拒收**。该件转换成功(源积木 6029 / 告警 19 / `validateBcm` = VALID),该语料 **`[跳过]` 归零**;Kitten2/3(`.bcm` + `blocksXML`)仍按形态守卫跳过。见 `rounds/39` §W10 落地段、`knowledge/convert-semantics.md` §3 |
| rounds/33 §1 表 ① | 上传格式(`download/compile/*.bcm4`)⇒ 正向「**报错** `invalid type: map, expected a string`」 | **不再成立**:那条报错的根因正是对象形态影子,已由 `66c0b6b` 容错 |
| rounds/17(错误收敛一节第 2 条) | 「保留 `Crypto`/`Decompile`/**`UnsupportedType`**/`InvalidResponse`/…(反编译专属变体)」 | **`UnsupportedType` 已不存在**:它是全仓零调用点的死变体,已于 `fef30e7`(2026-10-02,W5②)删除(破坏性公共面变更、已授权);其它变体未动。见 `rounds/39` §W5②、`knowledge/repo-conventions.md` §4 |

## CI 产物口径与 `Cargo.toml` 不符(2026-10-02 实测)

> 这条不是某篇轮次正文写错,而是**仓库里两处配置互相矛盾**,根因出处恰在 `docs/rounds/01`,故记在此。

- **错**:`.github/workflows/CI.yml:28/33/39/44/49` 的 `libname` 列(`libbackend.so` ×2 / `backend.dll` / `libbackend.dylib` ×2)+ 上传步 `:63-67`(`path: target/<target>/release/<libname>`)—— 蕴含"release 会产出动态库"。
  - **为何错**:`Cargo.toml:8` 是 `crate-type = ["rlib"]`,**只产 rlib,不产 `.so`/`.dll`/`.dylib`** ⇒ 上传步按该路径**找不到文件**;`actions/upload-artifact` 的 `if-no-files-found` **默认 `warn`** ⇒ **job 静默绿,产物其实从未上传**。
  - **根因线索**:`docs/rounds/01-websocket-pitfalls.md:791`(为让库可被测试引用,把 `"rlib"` **加进** `crate-type`)与 `:809`(「`crate-type = ["cdylib"]` 的库不参与测试…加了 `"rlib"` 后测试才运行」)。此后 `cdylib` 从 `crate-type` 里消失,而 CI 的 `libname` 列表没跟上。
  - **正确(已决并落地,2026-10-02,`f68c2e6`)**:取方案 ② 的删法 —— **删掉 artifact 上传步**及矩阵里的 `artifact:`/`libname:` 键。理由:本仓 `[lib] crate-type=["rlib"]` **只产 rlib**、`src/main.rs` 又是需账号的**交互式管理控制台**、仓内无消费方 ⇒ **没有可分发产物**,不恢复 `cdylib`。五目标 `cargo build --release` 矩阵保留;另新增 `offline-gate` job(fmt --check + clippy -D warnings + 逐目标点名的离线测试)。CI 口径(含**刻意不跑**真机门与吃 `download/` 的语料扫描器)见 `knowledge/repo-conventions.md` §6。
  - 出处:`.github/workflows/CI.yml`、`Cargo.toml:8`、`docs/rounds/01` 附录「空的 lib」两节;登记在 `docs/goals/infra-backlog.md` §1。
