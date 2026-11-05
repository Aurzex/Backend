# 优化整改目标(2026-11-06 代码审计)

> 目标库。审计证据、探针读数与逐项方案见 `../rounds/54-code-audit-and-optimization-batch.md`;本页只记录状态、验收与指针,不复制读数。
> 某一项完成后从本页删除,并把结论与提交号写入上述轮次记录或对应知识库。

## 1. 未完成事项

> 2026-11-06 第一批已完成项与逐项提交号只在 `../rounds/54-code-audit-and-optimization-batch.md` §6 展开;本页只保留未完成项。

| # | 项 | 状态 | 验收 | 出处 |
| --- | --- | --- | --- | --- |
| P2-1 | 门面 `translate_work` 流式产物的性能读数 | 部分(2026-11-06,`48890df`) | 已与内存入口逐字节等价(三条方向);仍需同一内存源文档的三腿 A/B 读数 | `../rounds/54-code-audit-and-optimization-batch.md` §3.1 |
| P2-4 | 库内嵌路径与客户端注入 | 待方案 | `PathConfig` 根与门面客户端可显式配置;staging 跟随输出目录;API 形态先定方案 | `../rounds/54-code-audit-and-optimization-batch.md` §3.4 |
| P3-2 | `ide/load` 编辑格式快速通道 | 待核验 | 证明版本选择、权限回退与同版语义等价后,再决定是否作为可选入口 | `../rounds/54-code-audit-and-optimization-batch.md` §5.1 |

## 2. 维护约定

- 状态术语只使用 `待决` / `待做` / `待核验` / `待方案` / `已完成(日期,提交号)` / `部分` / `判不做` / `暂缓(待重新立项)`。
- 每项在对应轮次记录中只展开一次读数与理由;本页只保留验收与指针。
- 代码修复必须先过 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 与相关离线测试;有产物的改动另过 SHA 基线。
