# backend

Rust 编写的同步 HTTP / WebSocket 客户端库,对接编程猫(codemao)社区服务。

项目以库为主体(`lib.rs`,crate 类型 `rlib`),覆盖账号、认证、人机验证、云变量、作品反编译、AI 对话、举报引擎等完整业务面,可独立嵌入其他 Rust 项目。

## 项目定位

面向编程猫(codemao)社区服务的 **Rust 同步客户端库**:为社区自动化、数据治理与作品生态工具提供开箱即用的 API 封装——登录与身份管理、业务接口调用、WebSocket 实时通道(云变量 / AI 对话)、作品反编译、举报内容治理。库以「简单直接、零异步负担」为设计取向,强调嵌入式使用(作为依赖引入)而非独立服务。

典型使用方:

- 社区内容治理 / 举报审核自动化
- 作品数据抓取、批量反编译与备份
- 基于云变量 / AI 对话的扩展工具

## 核心能力

| 能力            | 说明                                                                                                 |
| --------------- | ---------------------------------------------------------------------------------------------------- |
| 统一认证        | 管理员 / 用户 / 学生多身份登录(密码 v0/v1/v2、Token、验证码票据),令牌写入全局身份槽,后续请求自动携带 |
| 业务 API 全覆盖 | 13 个业务域:账号、认证、人机验证、云数据库、代码岛、社区、教育、论坛、小说、商店、用户、举报、作品   |
| 云变量实时同步  | WebSocket 客户端:断线重连、命令批量合并、变量 / 列表 / 排行榜 / 在线人数事件回调                     |
| 作品反编译      | Kitten2/3/4、Coco、Neko、Nemo、Wood 七种编辑器,含 `.bcm` 解密与 Blockly XML 输出                     |
| AI 对话         | 流式回复客户端(Start / Text / End 事件),同步等待完整回复                                             |
| 举报治理引擎    | 分块拉取、批量分组、逐条 / 一键决策、多账号自动举报、违规检查、处理统计                              |

## 技术栈

| 类别        | 选型                                                   |
| ----------- | ------------------------------------------------------ |
| 语言        | Rust(edition 2024,stable),同步阻塞模型,无 async 运行时 |
| HTTP 客户端 | `ureq` 3(JSON / multipart / gzip)                      |
| WebSocket   | `tungstenite` 0.30(rustls-tls-webpki-roots)            |
| 序列化      | `serde` / `serde_json`                                 |
| 加密        | `aes-gcm` 0.11(作品解密)、`sha2` 0.11(设备签名)        |
| 编码        | `base64` 0.23                                          |
| 日志 / 错误 | `log` 0.4 / `thiserror` 2.0                            |
| 其他        | `fastrand`(随机 ID)、`url`(URL 解析)                   |

## 构建

要求 Rust stable(edition 2024)。构建期依赖见 `Cargo.toml`,无第三方运行依赖。

```bash
cargo build --release
cargo test          # 运行库单测
```

`[profile.release]` 已配置 `lto`、`opt-level = "z"`、`strip` 与 `panic = "abort"`,产物为最小体积。

## 模块一览

- **api/** — 13 个业务域(均实现 `ClientAccess`,请求经统一客户端发送):
  `account` 账号 · `auth` 认证 · `captcha` 人机验证 · `clouddb` 云字典 · `codegame` 代码岛 · `community` 社区 · `education` 教育 · `forum` 论坛 · `library` 小说 · `shop` 商店/工作室 · `user` 用户 · `whale` 举报 · `work` 作品
- **core/** — 业务引擎:
  `cloudvar` 云变量 WS 客户端 · `compiler` 作品反编译(七编辑器) · `converse` AI 对话 · `pipeline` / `services` / `registry` / `retrieve` 举报处理引擎 · `terminal` 交互式 UI(演示)
- **utils/** — 基础设施:
  `acquire` HTTP/WS 客户端、身份管理、分页迭代器、上传、`ClientAccess` · `data` 路径配置、文件读写、`value_to_i64`

## 示例代码

```toml
[dependencies]
backend = { path = "../Backend" }
```

**1. 登录 + 分页拉取**(`LoginBuilder` 写身份,`PaginatedIter` 直接 for 循环):

```rust
use backend::api::auth::{AccountStatus, LoginBuilder};
use backend::api::education::EduDataFetcher;

// 登录:令牌写入全局身份槽,后续所有请求自动携带
let mut session = LoginBuilder::new()
    .identity("13800138000")
    .password("student-pass")
    .status(AccountStatus::Edu)
    .build(); // 构造阶段无副作用
session.execute()?; // -> LoginResult(网络请求)

// 分页拉取:惰性逐页请求,单页瞬时错误可重试
for work in EduDataFetcher::new().fetch_all_works_gen(None) {
    let work = work?; // MewResult<Value>
    println!(
        "{}",
        work.get("name").and_then(serde_json::Value::as_str).unwrap_or("")
    );
}
```

**2. 业务接口样板**(所有 Manager 实现 `ClientAccess`,错误自动携带服务端 body):

```rust
use backend::api::captcha::CaptchaManager;

let rule = CaptchaManager::new().fetch_captcha_rule()?; // MewResult<Value>
```

**3. 云变量 WebSocket**(回调订阅 + 同步等待):

```rust
use backend::core::cloudvar::{CloudBuilder, EditorType, RankingOrder};
use std::time::Duration;

let conn = CloudBuilder::new(12345)
    .editor(EditorType::Kitten)
    .connect_timeout(Duration::from_secs(5))
    .sync_timeout(Duration::from_secs(10))
    .build();
if !conn.connect_and_wait()? {
    return Err("云存储连接超时".into());
}

// 订阅变量变化(旧值, 新值, 来源),回调在库内线程触发
if let Some(var) = conn.get_private_variable("score") {
    var.on_change(|old, new, source| println!("score: {old:?} -> {new:?} ({source})"));
}

conn.list_push("history", "level-3")?;   // 列表操作经批量队列合并上传
conn.get_ranking("score", 10, RankingOrder::Descending)?; // 排行榜,结果经 on_ranking 回调返回
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
use backend::core::compiler::{DecompileOptions, decompile_work, decompile_works};

let json = decompile_work(123456, None)?; // None = 不写文件,仅返回 JSON 字符串

let results = decompile_works(
    &[111, 222],
    DecompileOptions::new().output_dir("/tmp/works").batch_concurrency(4),
);
for result in results {
    println!("{}", result?); // 与输入顺序一致,单个失败不中断其余
}
```

**6. 举报处理引擎**(分块拉取 + 逐条决策):

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
// 未达批量阈值的遗留组(可选处理)
for group in session.leftover_groups() {
    processor.apply_group(&group, ReportAction::Pass, admin_id);
}
```

## 配置

路径由 `PathConfig`(`src/utils/data.rs`)管理,默认以当前工作目录为根,可用 `with_root` 自定义:

| 路径                | 用途                                                                                |
| ------------------- | ----------------------------------------------------------------------------------- |
| `data/password.txt` | 学生账号(自动举报用)。每行 `用户名:密码`,`#` 开头为注释,空行忽略;缺失时自动举报报错 |
| `data/token.txt`    | 令牌持久化                                                                          |
| `cache/captcha.jpg` | 登录验证码图片(登录流程自动写入)                                                    |
| `download/`         | 编译/小说文件等下载产物                                                             |
| `cache/`            | 运行时缓存                                                                          |

`data/password.txt` 示例:

```
# 学生账号(用于自动举报)
13800138000:password123
13800138001:password456
```

## 设计要点

库的分层与写法带来的直接收益:

- **同步阻塞、零异步运行时**:无 tokio / async 依赖;WebSocket 用线程 + 通道封装成同步接口。嵌入任何项目(含非 async 环境)零成本,调用栈与错误传播都是普通 Rust 函数。
- **全局客户端 + 身份槽**:`CodeMaoClient::global()` 单例持有 `Catsona` 身份与令牌槽。登录一次(`LoginBuilder::build().execute()`)写入身份,之后所有请求自动携带对应身份令牌——调用方不需要手动拼 `Authorization` 头,也不需要把 token 传来传去。
- **样板收敛到 `ClientAccess`**:每个业务 Manager 只需实现 `fn client()`,`send_and_parse` / `check_status` / `send_maybe_parse` 由默认实现提供,且 4xx/5xx 自动携带服务端错误体——几十个 Manager 的请求代码只剩"端点 + 参数"。
- **分页统一为 `PaginatedIter`**:惰性初始化、翻页、总数/上限终止、页大小兜底全部内聚,调用方一个 `for` 循环即可,不关心 offset/page 计算。`.with_limit(n)` 设上限,`.with_all()` 显式全量拉取(直到服务端空页或总数耗尽)。
- **可替换边界**:`ClientProvider`(auth,可注入自定义客户端)、`PathConfig::with_root`(路径)——核心逻辑不绑定具体 HTTP 实现与目录,便于测试与定制。
- **WS 状态机封装成回调 + 等待原语**:cloudvar / converse 把帧解析、握手、重连、批量合并全部收进库内,外部通过 `on_change` / `on_connection` 回调与 `connect_and_wait` / `send_and_wait` 同步原语交互。
- **分层单向依赖**:`api → utils`,`core → api / utils`,上层不反向依赖;业务域模块之间互不引用,可按需单独使用。

## 目录结构

```
├── Cargo.toml
├── .github/workflows/CI.yml   # 5 平台 release 构建
├── src/
│   ├── lib.rs                 # 库入口(公开 api/core/utils)
│   ├── api.rs                 # api 模块声明
│   ├── core.rs                # core 模块声明
│   ├── utils.rs               # utils 模块声明
│   ├── api/                   # 业务域(13 个模块,见上表)
│   ├── core/
│   │   ├── cloudvar.rs        # 云变量 WS 客户端:连接状态机/断线重连/命令批量合并/变量列表排行榜回调
│   │   ├── compiler.rs        # 作品反编译:7 编辑器抓取/BCMKN 解密/积木树递归反编译/Blockly XML 序列化
│   │   ├── converse.rs        # AI 对话 WS 客户端:流式回复/历史记录/超时断连检测
│   │   ├── pipeline.rs        # 举报引擎:动作注册表/多账号轮流/违规检查/分块拉取
│   │   ├── registry.rs        # 举报类型注册表/来源配置/分块迭代与总数统计
│   │   ├── retrieve.rs        # 数据查询:评论/回复流(分块并行)、管理员与粉丝统计、聚合
│   │   ├── services.rs        # ReportProcessor 原语/批量分组/文件上传处理
│   │   └── terminal.rs        # 控制台 UI(演示举报处理流程,非核心库能力)
│   └── utils/
│       ├── acquire.rs         # HTTP/WS 客户端、身份管理、分页迭代器、上传、ClientAccess
│       └── data.rs            # PathConfig、文件写入、value_to_i64
```

## 测试

```bash
cargo test
```

现有单测覆盖:管理员信息固定字段提取(`AdminInfo::from_details` 正常/缺失字段两条)、分页迭代器终止性(单类型数据量超过 chunk 大小不重复、不丢失)。

## CI

GitHub Actions(`.github/workflows/CI.yml`):main/master 推送、PR 与手动触发;矩阵构建 linux(x86_64 / aarch64)、windows(x86_64)、macos(x86_64 / aarch64) 五平台 release,产物上传为 `backend-<平台>`。

## 相关文档

- `CONTRIBUTING.md` — 编码约定与提交规范
