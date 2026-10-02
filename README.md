<!-- AI 助手注意：开始任何任务前，必须先读取根目录下的 AGENTS.md 并严格遵守其规范 -->

# backend

Rust 写的同步 HTTP / WebSocket 客户端库,面向编程猫(codemao)社区服务。

本项目以库为主体(`lib.rs`,crate 类型 `rlib`),涵盖账号、认证、人机验证、云变量、作品反编译、AI 对话、举报引擎等业务,可直接嵌入其他 Rust 项目使用。

## 项目定位

面向编程猫社区服务的 **Rust 同步客户端库**。给社区自动化、数据治理和作品生态工具提供开箱即用的 API 封装:登录与身份管理、业务接口调用、WebSocket 实时通道(云变量 / AI 对话)、作品反编译、举报内容治理。设计取向是「简单直接、零异步负担」,主打嵌入式使用(作为依赖引入),不是独立服务。

典型使用方:

- 社区内容治理 / 举报审核自动化
- 作品数据抓取、批量反编译与备份
- 基于云变量 / AI 对话的扩展工具

## 核心能力

| 能力            | 说明                                                                                                                                |
| --- | --- |
| 统一认证        | 普通用户 / 教育 / 评审三种身份登录(密码 v0/v1/v2、Token、管理员令牌/密码、验证码票据),小鱼干(令牌)写进全局身份槽,之后的请求自动携带 |
| 业务 API 全覆盖 | 业务域共 13 个:账号、认证、人机验证、云数据库、代码岛、社区、教育、论坛、小说、工作室、用户、举报、作品                                |
| 云变量实时同步  | WebSocket 客户端:断线自动重连、命令批量合并、变量 / 列表 / 排行榜 / 在线人数事件回调                                                |
| 作品反编译      | Kitten2/3/4、Coco、Neko、Nemo、Wood 七种编辑器,含 `.bcm` / `.bcm4` / `.bcmkn` 解密与 Blockly XML 输出;产物可**另行上传到当前账号**(建一份标「可删」的草稿) |
| 作品文件互相转化 | Kitten4 `.bcm4` 与 KittenN `.bcmkn` 双向互转、NEMO 转 KittenN;官方算法逐块对齐 + 反向自建,降级/丢弃逐类报告,产物过官方 `validateBcm` 硬门 |
| AI 对话         | 流式回复客户端(Start / Text / End / Error 事件),同步等完整回复                                                                      |
| 举报治理引擎    | 分块拉取、批量分组、逐条 / 一键决策、多账号自动举报、违规检查、处理统计                                                             |

## 技术栈

| 类别        | 选型                                                           |
| --- | --- |
| 语言        | Rust(edition 2024,stable),同步阻塞模型,无 async 运行时         |
| HTTP 客户端 | `ureq` 3(JSON / multipart / gzip)                              |
| WebSocket   | `tungstenite` 0.30(rustls-tls-webpki-roots)                    |
| 序列化      | `serde` / `serde_json`                                         |
| 加密        | `aes-gcm` 0.11(作品解密)、`sha2` 0.11(设备签名 / AES 密钥派生) |
| 编码        | `base64` 0.23                                                  |
| 日志 / 错误 | `log` 0.4 / `thiserror` 2.0                                    |
| 其他        | `fastrand`(随机 ID)、`url`(URL 解析)                           |

## 构建

需要 Rust stable(edition 2024)。构建期依赖见 `Cargo.toml`,没有第三方运行依赖。

```bash
cargo build --release
cargo test          # 跑库单测
```

`[profile.release]` 已配置 `lto`、`opt-level = "z"`、`strip` 与 `panic = "abort"`,产物体积最小。

## 模块一览

- **api/** — 业务域(都实现了 `ClientAccess`,请求走统一客户端):
  `account` 账号 · `auth` 认证 · `captcha` 人机验证 · `clouddb` 云数据库 · `codegame` 代码岛 · `community` 社区 · `education` 教育 · `forum` 论坛 · `library` 小说 · `shop` 工作室 · `user` 用户 · `whale` 举报 · `work` 作品
- **core/** — 业务引擎:
  `cloudvar` 云变量 WS 客户端 · `convert` 作品文件转换域(`decompile` 反编译七种编辑器 / `translate` 编辑器间互相转化) · `converse` AI 对话 · `pipeline` / `services` / `registry` / `retrieve` 举报处理引擎 · `terminal` 交互式 UI(演示用)
- **utils/** — 基础设施:
  `requests` HTTP 客户端、身份管理、分页迭代器、上传、`ClientAccess` · `filedata` 路径配置、文件写入、`value_to_i64` · `socketio` Socket.IO over WebSocket 共享基础设施(cloudvar / converse 共用),含共享错误类型 `SocketError`

## 示例代码

```toml
[dependencies]
backend = { path = "../Backend" }
```

**1. 登录 + 分页拉取**(`LoginBuilder` 写身份,`PaginatedIter` 直接 for 循环):

```rust
use backend::api::auth::{AccountStatus, LoginBuilder};
use backend::api::education::EduDataFetcher;

// 登录:小鱼干(令牌)写进全局身份槽,之后所有请求自动携带
LoginBuilder::new()
    .identity("13800138000")
    .password("student-pass")
    .status(AccountStatus::Edu)   // 教育身份,映射到 IdentityIdentity::Scholar
    .execute()?;                  // 构造阶段无副作用,execute() 才发网络请求
//                                  -> MewResult<LoginResult>

// 分页拉取:惰性逐页请求,单页瞬时错误可以重试
for work in EduDataFetcher::new().fetch_all_works_iter(None) {
    let work = work?; // MewResult<Value>
    println!(
        "{}",
        work.get("name").and_then(serde_json::Value::as_str).unwrap_or("")
    );
}
```

**2. 业务接口样板**(所有 Manager 实现 `ClientAccess`,错误自动带上服务端 body):

```rust
use backend::api::captcha::CaptchaManager;

let rule = CaptchaManager::new().fetch_captcha_rule()?; // MewResult<Value>
```

**3. 云变量 WebSocket**(回调订阅 + 同步等待):

```rust
use backend::core::cloudvar::{CloudBuilder, CloudEditorType, RankingOrder};
use std::time::Duration;

// 不传 .editor() 时,连接阶段按作品详情自动识别编辑器类型
// (KITTEN2/3/4→Kitten,NEMO→Nemo,NEKO→KittenN,COCO→Coco);也可以显式指定
let conn = CloudBuilder::new(12345)
    .editor(CloudEditorType::Kitten)
    .connect_timeout(Duration::from_secs(5))
    .sync_timeout(Duration::from_secs(10))
    .build();
if !conn.connect_and_wait()? {
    return Err("喵呜,云存储连接超时".into());
}

// 订阅变量变化(旧值, 新值, 来源),来源是 "local" / "cloud",回调在库内线程触发
if let Some(var) = conn.get_private_variable("score") {
    var.on_change(|old, new, source| println!("score: {old:?} -> {new:?} ({source})"));
}

conn.list_push("history", "level-3")?;   // 列表操作经批量队列合并上传
// 排行榜:发起请求,结果经 on_ranking_received 回调返回
conn.on_ranking_received(|_data| println!("排行榜已就绪(喵)"));
conn.get_ranking("score", 10, RankingOrder::Descending)?;
```

**4. AI 对话**(流式回调 + 同步等待完整回复):

```rust
use backend::core::converse::{ChatBuilder, ChatEventType, HistoryMode};
use std::time::Duration;

let chat = ChatBuilder::new("user-token")
    .sync_timeout(Duration::from_secs(30))
    .build();
chat.connect()?;
chat.on_stream(|text, ev| match ev {
    ChatEventType::Text => print!("{text}"),
    ChatEventType::End => println!(),
    _ => {}
});
let reply = chat.send_and_wait("你好", HistoryMode::Exclude)?;
```

**5. 作品反编译**(支持 Kitten2/3/4、Coco、Neko、Nemo、Wood):

```rust
use backend::core::convert::decompile::{DecompileOptions, decompile_work, decompile_works};

let path = decompile_work(123456.into(), None)?; // None = 写入默认输出目录,返回文件路径

let results = decompile_works(
    &[111.into(), 222.into()],
    DecompileOptions::new().output_dir("/tmp/works").batch_concurrency(4),
);
for result in results {
    println!("{}", result?); // 和输入顺序一致,单个失败不耽误其余
}
```

**6. 作品文件互相转化**(Kitten4 `.bcm4` 与 KittenN `.bcmkn` 双向互转;加载器/编辑器间搬运积木与资源引用):

```rust
use backend::core::convert::translate::{TargetEditor, TranslateOptions, translate_file};
use backend::core::convert::translate_work; // 跨子域编排:按作品 id 取编辑版再转化

// 文件 → 文件(落盘,默认不上传)
let out = translate_file(
    "download/compile/某作品_123.bcm4".as_ref(),
    TargetEditor::KittenN,
    TranslateOptions::new().deterministic_ids(true),
)?;
println!("{} 积木 {} → {}", out.output.display(), out.report.blocks_total, out.report.blocks_converted);
println!("{}", out.report.to_markdown()); // 降级/丢弃逐类计数,不静默吞

// 作品 id → 转化,可选上传并新建草稿(upload 默认关;开=替用户在平台落一份草稿)
let out = translate_work(123456.into(), TargetEditor::KittenN, TranslateOptions::new().upload(true))?;
println!("草稿作品 id = {:?}", out.work_id);
```

- 方向:当前支持 **Kitten4 编辑版转为 KN**(与编辑器官方算法逐块对齐,产物通过编辑器自带的 `validateBcm`)与**反向由 KN 转为 Kitten4**(官方无此方向,由本库自建;不可逆项进报告)。
- 不支持:`.bcm`(Kitten2/3,`blocksXML`)——编辑器本身也拒绝该方向(会引导去 Kitten V4.0)。
- 有损项(`TranslateWarning`)与覆盖率写在 `TranslateReport` 里,`TranslateOptions::strict(true)` 可让**真损失**(未映射/降级/丢字段)直接失败;新铸 id 与「官方导入时会重传资源」不算损失。

**7. 举报处理引擎**(分块拉取 + 逐条决策):

```rust
use backend::core::registry::ReportAction;
use backend::core::services::ReportProcessor;

let processor = ReportProcessor::new();
let mut session = processor.pending_session(); // 后台 worker 预取分块
for (_groups, items) in session.by_ref() {
    for item in items {
        if let Some(view) = processor.item_view(&item) {
            for line in &view.details { println!("{line}"); }
            processor.apply_action(&item, ReportAction::Pass, admin_id)?; // 通过
        }
    }
}
// 没凑够批量阈值的遗留组(可选处理)
for group in session.leftover_groups() {
    processor.apply_group(&group, ReportAction::Pass, admin_id);
}
```

## 配置

路径由 `PathConfig`(`src/utils/filedata.rs`)管理,默认以当前工作目录为根,可以用 `with_root` 自定义:

| 路径                | 用途                                                                                      |
| --- | --- |
| `data/password.txt` | 学生账号(自动举报用)。每行 `用户名:密码`,`#` 开头是注释,空行忽略;缺失时自动举报会报错 |
| `data/token.txt`    | 小鱼干(令牌)持久化                                                                        |
| `cache/captcha.jpg` | 登录验证码图片(登录流程自动写入)                                                          |
| `download/compile/` | 作品反编译输出目录                                                                        |
| `download/convert/` | 作品互相转化的输出目录(`staging/<作品 id>-<随机>`,每个作品一次调用一个独立中间目录) |
| `download/fiction/` | 小说文件下载目录                                                                          |
| `cache/`            | 运行时缓存                                                                                |

`data/password.txt` 示例:

```
# 学生账号(用于自动举报)
13800138000:password123
13800138001:password456
```

## 设计要点

库的分层与写法带来的好处:

- **同步阻塞、零异步运行时**:没有 tokio / async 依赖;WebSocket 用线程 + 通道封装成同步接口。嵌进任何项目(包括非 async 环境)零成本,调用栈和错误传播都是普通 Rust 函数。
- **全局客户端 + 身份槽**:`CodeMaoClient::global()` 单例拿着 `IdentityIdentity`(普通用户 `Fluffy` / 教育 `Scholar` / 评审 `Judge` / 空白 `Blanky`)身份和令牌槽。登录一次写入身份,之后所有请求自动带上对应身份的小鱼干——调用方不用手动拼 `Authorization` 头,也不用把 token 传来传去。
- **样板收敛到 `ClientAccess`**:每个业务 Manager 只需要实现 `fn client()`,`send_and_parse` / `check_status` / `send_maybe_parse` 由默认实现提供,4xx/5xx 还会自动带上服务端错误体——几十个 Manager 的请求代码只剩下「端点 + 参数」。
- **分页统一为 `PaginatedIter`**:惰性初始化、翻页、总数/上限终止、页大小兜底全部内聚,调用方只需一个 `for` 循环,不关心 offset/page 怎么算。`.with_limit(n)` 设上限,`.with_limit(FETCH_ALL)` 显式全量拉取(直到服务端空页或总数耗尽)。
- **可替换边界**:`CodeMaoClient` 支持全局单例 / 独立实例(`new_with_global_auth` / `new_independent` / `new_with_auth`)和自定义 `AuthProviderAuthProvider` 认证提供者;业务 Manager、反编译器、举报引擎与登录(`LoginBuilder::new_with_client`)均提供 `new_with_client(client)`(默认 `new()` 走全局),`ClientProvider`(auth 域)与 `PathConfig::with_root`(路径)——核心逻辑不绑定具体 HTTP 实现和目录,方便测试和定制。

- **错误模型分层**:传输层 / 通用错误归 `MewError`(`Http` / `Io` / `Json` / `HttpStatus` 结构化 4xx/5xx,外加域类别 `Auth` 凭据错误与 `InvalidArgument` 调用方参数错误);WS 客户端共享 `SocketError`(cloudvar / converse);业务域错误(`DecompilerError` / `ProcessorError` / `DataQueryError`)包装 `MewError`,不重复传输层变体。
- **WS 状态机封装成回调 + 等待原语**:cloudvar / converse 把帧解析、握手、重连、批量合并全部收进库内,外部通过 `on_change` / `on_connection` / `on_stream` 回调和 `connect_and_wait` / `send_and_wait` 同步原语交互;Socket.IO 帧解析与回调存储由 `utils/socketio` 统一提供。
- **分层单向依赖**:`api` 依赖 `utils`,`core` 依赖 `api` 与 `utils`,上层不反向依赖;业务域模块之间互不引用,可以按需单独使用。

## 目录结构

```
├── Cargo.toml
├── Cargo.lock
├── .github/workflows/CI.yml   # 5 平台 release 构建
├── src/
│   ├── lib.rs                 # 库入口(公开 api/core/prelude/utils)
│   ├── api.rs                 # api 模块声明(13 个业务域)
│   ├── core.rs                # core 模块声明
│   ├── utils.rs               # utils 模块声明
│   ├── prelude.rs             # 常用类型与 trait 预导入
│   ├── main.rs                # 演示二进制:举报处理控制台(登录 → 举报审核)
│   ├── bin/
│   │   └── gen_translate_tables.rs  # 生成 translate/tables_gen.rs(整文件覆盖,勿与手写表混放)
│   ├── api/                   # 业务域(见「模块一览」)
│   ├── core/
│   │   ├── convert/           # 作品文件转换域(读写作品文件的唯一边界,14 个文件)
│   │   │   ├── mod.rs         #   域门面:子域声明 + 单作品/批量编排 + 上传建草稿编排
│   │   │   ├── shared.rs      #   共用地基:错误/领域模型/配置/加密·HTTP·文件·JSON/上传/批量执行
│   │   │   ├── decompile/
│   │   │   │   ├── mod.rs     #   反编译门面:选项 + 上下文 + 契约 + 编译版积木层
│   │   │   │   └── editors.rs #   七种编辑器的抓取与重建(Kitten / Neko / Nemo / Coco / Wood)
│   │   │   └── translate/
│   │   │       ├── mod.rs     #   转化门面:入口/路径/源引用(公开类型从 options/report 再导出)
│   │   │       ├── options.rs #   公开配置面(TargetEditor / StageOrientation / TranslateOptions / 错误)
│   │   │       ├── report.rs  #   报告层(TranslateWarning / TranslateReport)
│   │   │       ├── pipeline.rs#   正/反文档级编排 + 工作项并行调度 + 临时 id 改写
│   │   │       ├── model.rs   #   节点/树模型 + Kitten/KN 适配器 + id 策略 + 程序集
│   │   │       ├── mapping.rs #   语义映射(双向)
│   │   │       ├── assembly.rs#   文档装配(双向)
│   │   │       ├── xml.rs     #   XML 层:最小 DOM + 字符串手术 + 影子渲染/转义
│   │   │       ├── nemo.rs    #   NEMO → KN(NEMO 管线 + 版本迁移 + 前置改写)
│   │   │       ├── nemo_mapping.rs # NEMO 映射表(含人工转录的官方表)
│   │   │       ├── tables_gen.rs   # 生成物,勿手改(生成器见 src/bin/gen_translate_tables.rs)
│   │   │       └── {reverse,nemo}_tests.rs
│   │   ├── cloudvar.rs        # 云变量 WS 客户端:连接状态机/断线重连/命令批量合并/变量列表排行榜回调
│   │   ├── converse.rs        # AI 对话 WS 客户端:流式回复/历史记录/超时断连检测
│   │   ├── pipeline.rs        # 举报引擎:动作注册表/多账号轮流/违规检查/分块拉取
│   │   ├── registry.rs        # 举报类型注册表/来源配置/分块迭代与总数统计
│   │   ├── retrieve.rs        # 数据查询:评论/回复流(分块并行)、管理员与粉丝统计、聚合
│   │   ├── services.rs        # ReportProcessor 原语/批量分组/文件上传处理
│   │   └── terminal.rs        # 控制台 UI(演示举报处理流程,非核心库能力)
│   └── utils/
│       ├── requests.rs        # HTTP 客户端、身份管理、分页迭代器、上传、ClientAccess
│       ├── filedata.rs        # PathConfig、文件写入、value_to_i64
│       └── socketio.rs        # Socket.IO over WebSocket 共享基础设施
├── tests/
│   ├── live_features.rs       # 真机集成测试(登录 + AI 对话 + 云变量 + 反编译)
│   ├── compile_live.rs        # 反编译真机集成测试(NEMO 用例 #[ignore])
│   ├── convert_bench.rs       # 转化基准:4 份真作品的分阶段耗时 + 产物 SHA256 基线(§测试的门)
│   ├── convert_facade_bench.rs# 门面路径对照(盘→盘 vs 内存直通),断言两条路径同 SHA256
│   ├── convert_work_bench.rs  # `translate_work` 端到端基准 + `save_raw` 本地探针(#[ignore])
│   ├── convert_corpus_harvest.rs # 采集真作品(公开发现流)到 download/compile(#[ignore])
│   ├── convert_edit_harvest.rs   # 采集**平台原始编辑格式**(`/kitten/work/ide/load`)到 k4edit/(#[ignore])
│   ├── convert_work.rs        # 单件作品按类型自动选方向转换(#[ignore])
│   └── fixtures/test-config.example.json
└── docs/                      # 文档:知识库 / 目标库 / 轮次记录(入口 docs/README.md)
```

## 测试

```bash
cargo test                        # 库单测 + 集成测试(没配置会自动跳过)
cargo test --test live_features   # 真机:登录 + AI 对话 + 云变量 + 反编译
cargo test --test compile_live -- --ignored   # 含 NEMO 反编译(约 5 分钟)
cargo test --test convert_live               # 转化真机(离线用例)
cargo test --test convert_live -- --ignored  # 含上传建草稿(会写平台,用例自清理)
BACKEND_REQUIRE_LIVE=1 cargo test            # 严格模式:缺配置 / 登录失败一律失败,不再静默跳过

# 转化域基准与语料(都需 download/ 下的真作品;见 docs/rounds/37)
cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture   # 分阶段耗时 + SHA256 基线
BACKEND_REQUIRE_BENCH=1 cargo test --profile bench_perf --test convert_bench -- --ignored  # 严格:缺样本/缺基线一律失败
BACKEND_BENCH_REFRESH=1 cargo test --profile bench_perf --test convert_bench -- --ignored  # **有据刷新**基线(逐键打印变化)
cargo test --test convert_corpus_harvest -- --ignored   # 抓真作品(公开发现流)⇒ download/compile/
cargo test --test convert_edit_harvest   -- --ignored   # 抓**平台原始编辑格式** ⇒ download/compile/k4edit/
WORK_ID=273988379 cargo test --test convert_work -- --ignored --nocapture      # 单件按类型自动选方向
```

**产物不变**是转化域的第一职责:上面 `convert_bench` 会把 4 份产物的 **SHA256 与 `#meta`**(源文件 SHA/字节/块数/告警数)
与 `tests/fixtures/translate/convert_bench_baseline.json` 逐项比对;`deterministic_ids(true)` 下同输入两次必须逐字节相同。
产物**有意**变化时用 `BACKEND_BENCH_REFRESH=1` 重刷,并在提交信息里写清原因。

- 库单测覆盖:`AdminInfo::from_details` 固定字段提取(正常 / 缺失字段)、`manager_new_with_client_uses_injected_client`(客户端注入契约)、`header_override_is_case_insensitive`(请求头大小写覆盖)、分块迭代器终止性(数据量超过 chunk 大小不重复、不丢失)。
- 转化/反编译的**产物级门**:官方 `validateBcm` 校验器(产物能否被编辑器加载)、KN 转 Kitten4 再转回 KN 的往返积木类型多重集守恒、`deterministic_ids` 下并发 1 与并发 N 产物**逐字节一致**、与官方产物/串行参考的语义 diff。
- 集成测试配置和代码分离:把 `tests/fixtures/test-config.example.json` 复制成 `data/test-config.json` 再填好;`data/` 已被 `.gitignore` 忽略,账号密码不会入库。没配置时测试打印提示并跳过,不会导致失败;也可以用 `BACKEND_TEST_CONFIG` 环境变量覆盖配置文件路径。

## CI

GitHub Actions(`.github/workflows/CI.yml`):main/master 推送、tag、PR 和手动触发;矩阵构建 linux(x86_64 / aarch64)、windows(x86_64)、macos(x86_64 / aarch64)五平台 release,库产物以 `backend-<平台>` 命名上传。

## 相关文档

- `CONTRIBUTING.md` — 编码约定与提交规范
- `docs/README.md` — 文档总入口(知识库 / 目标库 / 轮次记录)
- `docs/knowledge/` — **知识库**:平台接口与实时协议、作品文件格式、转换语义、性能基线、仓库约定、历史勘误
- `docs/goals/` — **目标库**:待决策、待实现、待核验、已决不做
- `docs/rounds/` — 历史轮次记录(`docs/rounds/01`–`docs/rounds/40`:方案/评审/真机实测证据;索引见 `docs/rounds/README.md`,读前先看 `docs/knowledge/errata.md`;现至第 40 轮)
