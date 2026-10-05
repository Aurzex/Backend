<!-- AI 助手注意:开始任何任务前,必须先读取根目录下的 AGENTS.md 并严格遵守其规范 -->

# backend

**面向编程猫(codemao)社区服务的 Rust 同步客户端库。** 覆盖账号与认证、业务 API、WebSocket 实时通道(云变量与 AI 对话)、作品反编译、编辑器间作品文件互转与举报治理,以依赖的形式嵌入其他 Rust 工程使用。

- 当依赖用:读[快速开始](#快速开始)与[使用示例](#使用示例)。
- 改这个仓库:先读 [`AGENTS.md`](AGENTS.md)(文档与语体红线)、[`CONTRIBUTING.md`](CONTRIBUTING.md)(编码与提交约定)。
- 查事实:协议、文件格式与性能基线在 [`docs/knowledge/`](docs/knowledge/);还剩什么没做在 [`docs/goals/`](docs/goals/);某个决定怎么来的在 [`docs/rounds/`](docs/rounds/)(读前先看 [`docs/knowledge/errata.md`](docs/knowledge/errata.md))。

## 为什么是同步阻塞

库内没有 `async`/`await`,也不依赖任何异步运行时:HTTP 走阻塞 IO,WebSocket 用线程加通道封装成同步接口,外部只接触回调(`on_change` / `on_connection` / `on_stream`)与等待原语(`connect_and_wait` / `send_and_wait`)。换来的是三点:调用方不必把整个程序推进异步运行时(命令行工具、批处理脚本、非 async 服务都能直接引);错误传播就是普通的 `Result`;依赖树里没有 tokio 一类的运行时。

## 能力一览

| 能力 | 说明 |
| --- | --- |
| 统一认证 | 普通用户 / 教育 / 评审三类身份登录(密码 v0/v1/v2、令牌、管理员令牌与密码、验证码票据);令牌写入全局身份槽后,后续请求自动携带 |
| 业务 API | 账号、认证、人机验证、云数据库、代码岛、社区、教育、论坛、小说、工作室、用户、举报、作品共 13 个业务域 |
| 云变量实时同步 | WebSocket 客户端:断线自动重连、命令批量合并、变量与列表与排行榜与在线人数事件回调 |
| AI 对话 | 流式回复客户端(开始、增量文本、结束、错误四类事件),并支持同步等待完整回复 |
| 作品反编译 | Kitten2/3/4、Coco、Neko、Nemo、Wood 七种编辑器,含 `.bcm` / `.bcm4` / `.bcmkn` 解密与 Blockly XML 输出;可选把产物上传到当前账号并新建草稿 |
| 作品文件互转 | Kitten4 `.bcm4` 与 KittenN `.bcmkn` 双向互转、NEMO 转 KittenN;正向与编辑器官方算法逐块对齐,反向由本库自建,降级与丢弃逐类进报告,产物过官方 `validateBcm` 校验 |
| 举报治理 | 分块拉取、批量分组、逐条与一键决策、多账号轮询举报、违规检查与处理统计 |

## 快速开始

```toml
[dependencies]
backend = { path = "…/Backend" }        # 本仓库未发布到 crates.io,按路径引入
serde_json = "1"
```

```rust
use backend::api::captcha::CaptchaManager;   // 业务域按域引入
use backend::prelude::*;                     // CodeMaoClient / ClientAccess / Identity / MewError / PaginatedIter

// 未登录也能读公开接口;登录后令牌写进全局身份槽,之后所有请求自动携带
let rule = CaptchaManager::new().fetch_captcha_rule()?;
println!("{rule}");
```

## 使用示例

**1. 登录 + 分页拉取**(`LoginBuilder` 写身份,`PaginatedIter` 直接进 `for` 循环):

```rust
use backend::api::auth::LoginBuilder;
use backend::api::education::EduDataFetcher;
use backend::utils::requests::Identity;

// 登录:令牌写进全局身份槽,之后的请求自动携带
LoginBuilder::new()
    .identity("13800138000")
    .password("student-pass")
    .status(Identity::Scholar)      // 教育身份
    .execute()?;                    // 构造阶段无副作用,execute() 才发网络请求

// 分页拉取:惰性逐页请求,单页瞬时错误可以重试
for work in EduDataFetcher::new().fetch_all_works_iter(None) {
    let work = work?;               // MewResult<serde_json::Value>
    println!(
        "{}",
        work.get("name").and_then(serde_json::Value::as_str).unwrap_or("")
    );
}
```

**2. 业务接口与分页边界**(所有 Manager 实现 `ClientAccess`,错误自动带上服务端响应体):

```rust
use backend::api::work::{KittenVersion, WorkDataFetcher};
use backend::prelude::*;
use backend::utils::requests::FETCH_ALL;

let rule = CaptchaManager::new().fetch_captcha_rule()?;

// 分页迭代器:with_limit 设上限,FETCH_ALL 显式全量;不设时用服务端页大小
let all_comments = WorkDataFetcher::new()
    .fetch_work_comments_iter(123456, None)
    .with_limit(FETCH_ALL);
let first_100 = WorkDataFetcher::new()
    .fetch_kitten_trash_iter(KittenVersion::V4, None, None)
    .with_limit(100);
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
    return Err("云存储连接超时".into());
}

// 订阅变量变化(旧值, 新值, 来源);来源为 "local" 或 "cloud",回调在库内线程触发
if let Some(var) = conn.get_private_variable("score") {
    var.on_change(|old, new, source| println!("score: {old:?} -> {new:?} ({source})"));
}

conn.list_push("history", "level-3")?;      // 列表操作经批量队列合并上传

// 排行榜:发起请求,结果经 on_ranking_received 回调返回
conn.on_ranking_received(|data| println!("排行榜就绪:{} 条", data.items.len()));
conn.get_ranking("score", 10, RankingOrder::Descending)?;

// 连接事件(断开、重连)同样走回调
conn.on_connection(|event| println!("{event:?}"));
```

**4. AI 对话**(流式回调 + 同步等待完整回复):

```rust
use backend::core::converse::{ChatBuilder, ChatEventType, HistoryMode};
use std::time::Duration;

let chat = ChatBuilder::new("authorization-token")
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

**5. 作品反编译**(Kitten2/3/4、Coco、Neko、Nemo、Wood 七种编辑器):

```rust
use backend::core::convert::decompile::{DecompileOptions, decompile_work, decompile_works};

let path = decompile_work(123456.into(), None)?;   // None 表示写入默认输出目录,返回产物路径

let results = decompile_works(
    &[111.into(), 222.into()],
    DecompileOptions::new().output_dir("/tmp/works").batch_concurrency(4),
);
for result in results {
    println!("{}", result?.display());             // 与输入顺序一致,单个失败不拖累其余
}
```

**6. 作品文件互转**(Kitten4 `.bcm4` 与 KittenN `.bcmkn` 双向互转):

```rust
use backend::core::convert::translate::{TargetEditor, TranslateOptions, translate_file};
use backend::core::convert::translate_work;   // 跨子域编排:按作品 id 取编辑版再转化

// 文件转文件(落盘,默认不上传)
let out = translate_file(
    "download/compile/某作品_123.bcm4".as_ref(),
    TargetEditor::KittenN,
    TranslateOptions::new().deterministic_ids(true),
)?;
println!("{} 积木 {} 共转化 {}", out.output.display(), out.report.blocks_total, out.report.blocks_converted);
println!("{}", out.report.to_markdown());      // 降级与丢弃逐类计数,不静默吞

// 作品 id 转作品,可选上传并新建草稿(upload 默认关;开启等于替用户在平台落一份草稿)
let out = translate_work(
    123456.into(),
    TargetEditor::KittenN,
    TranslateOptions::new().upload(true).strict(true),
)?;
println!("草稿作品 id = {:?}", out.work_id);
```

- 支持的方向:Kitten4 编辑版转 KN(与编辑器官方算法逐块对齐),以及 KN 转 Kitten4(官方无此方向,由本库自建;不可逆项进报告)。
- 不支持的方向:`.bcm`(Kitten2/3 的 `blocksXML`)转 KN,编辑器本身也拒绝该方向。
- 有损项见 `TranslateWarning` 与 `TranslateReport`;`TranslateOptions::strict(true)` 可让真损失(未映射、降级、丢字段)直接失败。新铸 id 与「官方导入时会重传资源」不计为损失。

**7. 举报处理引擎**(分块拉取 + 逐条决策):

```rust
use backend::core::registry::ReportAction;
use backend::core::services::ReportProcessor;

let processor = ReportProcessor::new();
let admin_id = 1;
let mut session = processor.pending_session();   // 后台 worker 预取分块
for (_groups, items) in session.by_ref() {
    for item in items {
        if let Some(view) = processor.item_view(&item) {
            for line in &view.details {
                println!("{line}");
            }
            processor.apply_action(&item, ReportAction::Pass, admin_id)?;
        }
    }
}
// 没凑够批量阈值的遗留组(可选处理)
for group in session.leftover_groups() {
    processor.apply_group(&group, ReportAction::Pass, admin_id);
}
```

**8. 注入自定义客户端**(默认 `new()` 走全局单例,需要隔离时显式注入):

```rust
use backend::api::auth::LoginBuilder;
use backend::core::convert::decompile::CodemaoDecompiler;
use backend::utils::filedata::PathConfig;
use backend::utils::requests::{ClientConfig, CodeMaoClient};

let client = CodeMaoClient::new_independent(ClientConfig::default());  // 独立身份槽
LoginBuilder::new_with_client(client.clone())
    .identity("13800138000")
    .password("student-pass")
    .execute()?;
let decompiler = CodemaoDecompiler::new(client);                       // 反编译走同一客户端

let paths = PathConfig::with_root("…/workspace");                      // 自定义路径根
println!("{:?}", paths.compile_file_path());
```

## 设计要点

- **同步阻塞、零异步运行时**:没有 tokio 一类依赖;WebSocket 由线程与通道封装成同步接口,调用栈与错误传播都是普通 Rust 函数。
- **全局客户端 + 身份槽**:`CodeMaoClient::global()` 单例持有全局身份槽(`Identity` 四态:普通用户 `Fluffy`、教育 `Scholar`、评审 `Judge`、空白 `Blanky`)。登录一次写入身份,之后所有请求自动带上对应令牌,调用方不必手工拼 `Authorization` 头或传递 token。
- **请求样板收敛到 `ClientAccess`**:业务 Manager 只需实现 `fn client()`,`send_and_parse` / `check_status` / `send_maybe_parse` 由默认实现提供,4xx 与 5xx 自动带上服务端错误体。
- **分页统一为 `PaginatedIter`**:惰性初始化、翻页、总数与上限终止、页大小兜底内聚在迭代器里。`with_limit(n)` 设上限,`with_limit(FETCH_ALL)` 显式全量拉取(直到服务端空页或总数耗尽)。
- **可替换边界**:`CodeMaoClient` 支持全局单例与独立实例(`new_with_global_auth` / `new_independent` / `new_with_auth`)及自定义 `AuthProvider`;业务 Manager、反编译器、举报引擎与登录均提供 `new_with_client(client)`,路径由 `PathConfig` 统一管理 —— 核心逻辑不绑定具体 HTTP 实现与目录。
- **错误模型分层**:传输层与通用错误归 `MewError`(`Http` / `Io` / `Json` / 带状态码的 `HttpStatus`,另有 `Auth` 与 `InvalidArgument`);WebSocket 侧共用 `SocketError`;业务域错误(转换域的 `ConvertError`、`ProcessorError`、`DataQueryError` 等)包装 `MewError`,不重复定义传输层变体。
- **WebSocket 封装成回调与等待原语**:帧解析、握手、重连、批量合并都收在库内,外部只接触回调与 `connect_and_wait` / `send_and_wait`。**回调不应 panic**:`release` 档为 `panic = "abort"`,回调内的 panic 会直接终止进程,库内无法兜住。
- **分层单向依赖**:`api` 依赖 `utils`,`core` 依赖 `api` 与 `utils`,上层不反向依赖;业务域之间互不引用,可按需单独使用。

## 技术选型

| 类别 | 选型 |
| --- | --- |
| 语言 | Rust(edition 2024,stable),同步阻塞模型,无 async 运行时 |
| HTTP | `ureq`(JSON、multipart、gzip) |
| WebSocket | `tungstenite`(rustls + webpki 根证书) |
| 序列化 | `serde` / `serde_json` |
| 加密与摘要 | `aes-gcm`(作品解密)、`sha2`(设备签名与密钥派生) |
| 编码 | `base64` |
| 日志与错误 | `log` / `thiserror` |
| 其他 | `fastrand`(随机 id)、`url`(URL 解析) |

版本以 [`Cargo.toml`](Cargo.toml) 与 `Cargo.lock` 为准。

## 目录结构

```
├── Cargo.toml
├── .github/workflows/CI.yml      # 五平台构建矩阵 + 卫生检查 + 离线门
├── src/
│   ├── lib.rs / api.rs / core.rs / utils.rs / prelude.rs
│   ├── main.rs                   # 演示二进制:举报处理控制台(需账号)
│   ├── bin/gen_translate_tables.rs   # 生成 translate/tables_gen.rs,整文件覆盖,勿手改
│   ├── api/                      # 业务域,一个域一个文件
│   ├── core/
│   │   ├── cloudvar.rs           # 云变量 WS 客户端
│   │   ├── converse.rs           # AI 对话 WS 客户端
│   │   ├── pipeline.rs / registry.rs / retrieve.rs / services.rs   # 举报引擎
│   │   ├── terminal.rs           # 控制台 UI(演示用,非核心能力)
│   │   └── convert/              # 作品文件转换域
│   │       ├── mod.rs            #   域门面:子域声明 + 单作品与批量编排 + 上传建草稿
│   │       ├── shared.rs         #   共用地基:错误、领域模型、配置、加密、HTTP、文件、上传
│   │       ├── upload.rs         #   上传编排
│   │       ├── decompile/        #   反编译:门面与选项、七种编辑器、工作项、影子模板、配置
│   │       │   └── {mod,editors,work,shadow,config}.rs
│   │       └── translate/        #   互相转化
│   │           ├── mod.rs / options.rs / report.rs    # 门面、公开配置、报告
│   │           ├── pipeline.rs / source.rs            # 文档级编排、源引用
│   │           ├── model.rs / mapping.rs / assembly.rs # 节点模型、语义映射、文档装配
│   │           ├── xml.rs / nemo.rs / nemo_mapping.rs  # XML 层、NEMO 管线与映射
│   │           ├── kitten4_vocab.rs                    # Kitten4 编辑器词表
│   │           └── tables_gen.rs                       # 生成物,勿手改
│   └── utils/
│       ├── requests.rs           # HTTP 客户端、身份管理、分页迭代器、上传、ClientAccess
│       ├── filedata.rs           # PathConfig、文件写入、Value 转换助手
│       └── socketio.rs           # Socket.IO over WebSocket 共享基础设施
├── tests/                        # 真机集成测试、转化基准与语料采集、仓库卫生检查
└── docs/                         # 知识库、目标库、轮次记录(入口 docs/README.md)
```

## 配置与路径

`PathConfig` 默认以当前工作目录为根,可用 `PathConfig::with_root` 另建一份:

| 路径 | 用途 |
| --- | --- |
| `data/password.txt` | 学生账号(自动举报用)。每行 `用户名:密码`,`#` 开头为注释,空行忽略;文件缺失时自动举报报错 |
| `cache/captcha.jpg` | 登录验证码图片(登录流程自动写入) |
| `download/compile/` | 作品反编译输出目录 |
| `download/convert/` | 作品互转的输出目录;中间产物落在 `staging/<作品 id>-<随机后缀>`,每次调用一个独立目录,收尾删除 |

`data/` 已在 `.gitignore` 中忽略,凭据不会入库;测试用配置同样放在 `data/`(见下节)。

```text
# data/password.txt 格式示例
13800138000:hygiene-allow-example
# 井号开头是注释,空行忽略
```

## 测试与基准

```bash
cargo test                             # 库单测 + 集成测试;缺配置的真机测试会打印提示后跳过
cargo test --test live_features        # 真机:登录 + AI 对话 + 云变量 + 反编译
cargo test --test compile_live -- --ignored      # 含 NEMO 反编译(分钟级)
cargo test --test convert_live         # 转化真机(离线用例)
cargo test --test convert_live -- --ignored      # 含上传建草稿(会写平台,用例自清理)
cargo test --test repo_hygiene         # 凭据不入库检查(CI 与本地 pre-commit 同一条)

# 转化域基准与语料(需 download/ 下的真作品语料)
cargo test --profile bench_perf --test convert_bench -- --ignored --nocapture
cargo test --test convert_corpus_harvest -- --ignored   # 采集真作品到 download/compile/
cargo test --test convert_edit_harvest   -- --ignored   # 采集平台原始编辑格式到 download/compile/k4edit/
WORK_ID=273988379 cargo test --test convert_work -- --ignored --nocapture   # 单件按类型自动选方向
```

| 环境变量 | 作用 |
| --- | --- |
| `BACKEND_TEST_CONFIG` | 覆盖真机测试配置文件路径(默认 `data/test-config.json`) |
| `BACKEND_REQUIRE_LIVE=1` | 真机测试严格模式:缺配置或登录失败一律失败,不再静默跳过 |
| `BACKEND_REQUIRE_BENCH=1` | 基准严格模式:缺样本或缺基线一律失败 |
| `BACKEND_REQUIRE_FIXTURES=1` | 夹具严格模式:缺语料与样例时失败并点名缺失项 |
| `BACKEND_BENCH_REFRESH=1` | 有据刷新产物基线(逐键打印变化,产物有意变化时才用) |

**产物不随优化而变**是转换域的第一职责:`convert_bench` 把产物的 SHA256 与元信息(源文件摘要、块数、告警数)与 `tests/fixtures/translate/convert_bench_baseline.json` 逐项比对;`deterministic_ids(true)` 下同输入两次必须逐字节相同,并发 1 与并发 N 也必须逐字节相同。产物**有意**变化时用 `BACKEND_BENCH_REFRESH=1` 重刷,并在提交信息里写明原因。

转化与反编译的产物级门还包括:官方 `validateBcm` 校验器(判断产物能否被编辑器加载)、Kitten4 转 KN 再转回来时积木类型多重集的守恒、以及与官方产物或串行参考的语义 diff。

真机测试的配置与代码分离:把 `tests/fixtures/test-config.example.json` 复制为 `data/test-config.json` 再填写;该文件已被 `.gitignore` 忽略。

## CI

GitHub Actions([`.github/workflows/CI.yml`](.github/workflows/CI.yml))在推送 main/master、tag、PR 与手动触发时跑三个 job:

| job | 内容 |
| --- | --- |
| `build` | 五目标 `cargo build --release` 矩阵:linux(x86_64、aarch64 交叉链接)、windows(x86_64)、macos(x86_64、aarch64) |
| `hygiene` | `cargo test --test repo_hygiene`,与本地 pre-commit 钩子同一份 |
| `offline-gate` | `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`,以及逐目标点名的离线测试 |

CI **刻意不跑**需要账号与网络的真机测试、需要 `download/` 语料的扫描器与基准(干净检出上它们打印一行后跳过),也不上传任何构建产物:本仓库只产 `rlib`,没有可分发的二进制。

## 性能

`[profile.release]` 为 `opt-level = 3`、`lto = true`、`strip = true`、`panic = "abort"`,取向是速度(该档取代了早期的 `opt-level = "z"`,读数与适用范围写在 `Cargo.toml` 注释里)。注意 `[profile.*]` 只作用于**本仓库作为顶层**的构建;本库被当作依赖引入时,用的是使用者工程自己的档。

**不改本库即能拿到的收益**:转换与反编译的 CPU 时间里有相当一部分花在全局分配与释放上(单次转换摊到百余次分配每产物节点,读数见 [`docs/knowledge/convert-performance.md`](docs/knowledge/convert-performance.md))。在**自己的**工程里换一个更快的全局分配器,可在不改动本库、不改变产物的前提下省下这部分开销:

```toml
# Cargo.toml(你的工程)
[dependencies]
mimalloc = "0.1"          # 或 tikv-jemallocator / tcmalloc 绑定
```

```rust
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

本库刻意不指定全局分配器(那会强加给所有使用者);临时验证也可以用 `LD_PRELOAD=/usr/lib/libjemalloc.so.2 <你的程序>`。

## 相关文档

- [`AGENTS.md`](AGENTS.md) — 项目知识库维护与迭代规范(文档体例、语体与输出禁忌)
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — 编码约定与提交规范
- [`docs/README.md`](docs/README.md) — 文档总入口与文档体例
- [`docs/knowledge/`](docs/knowledge/) — 知识库:平台接口与实时协议、作品文件格式、转换语义、性能基线、仓库约定、历史勘误
- [`docs/goals/`](docs/goals/) — 目标库:待决、待做、待核验、已决不做
- [`docs/rounds/`](docs/rounds/) — 轮次记录:每轮一份的方案、评审与真机实测证据(索引见 [`docs/rounds/README.md`](docs/rounds/README.md))
