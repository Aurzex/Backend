# 优化整改目标(2026-11-06 代码审计)

> 目标库。审计证据、探针读数与逐项方案见 `../rounds/54-code-audit-and-optimization-batch.md`;本页只记录状态、验收与指针,不复制读数。
> 完成一项后在本页标“已完成(日期,提交号)”,并把该项移入上述轮次记录或对应知识库。

## 1. 优先级清单

| # | 项 | 状态 | 验收 | 出处 |
| --- | --- | --- | --- | --- |
| P0-1 | 举报引擎每类型分页被截断 | 待做 | 举报生成器不再因默认 `limit` 停在单页;待处理、一键通过、已处理浏览总量一致;回归测试覆盖 | `../rounds/54-code-audit-and-optimization-batch.md` §2.1 |
| P0-2 | 粉丝统计只取单页粉丝 | 待做 | `compute_fans_by_like_threshold` 覆盖完整粉丝流,或在签名与文档中显式声明截断上限 | `../rounds/54-code-audit-and-optimization-batch.md` §2.2 |
| P0-3 | 论坛回复流只取单页回复 | 待做 | 论坛来源的回复流不再静默漏掉第 11 条以后的回复;接口文档与行为一致 | `../rounds/54-code-audit-and-optimization-batch.md` §2.3 |
| P1-1 | 评论聚合与粉丝统计的无界线程 | 待做 | 两处并行线程数有固定上界;结果顺序与错误语义不变 | `../rounds/54-code-audit-and-optimization-batch.md` §2.4 |
| P1-2 | 回调句柄与移除语义 | 待做 | 不同事件类型的句柄互不误删;排行榜回调可移除;回调执行期间移除不再被复活 | `../rounds/54-code-audit-and-optimization-batch.md` §2.5 |
| P1-3 | 分页首屏错误不可重试 | 待做 | 首屏请求的瞬时错误经 `next()` 可重试,与后续页错误语义一致 | `../rounds/54-code-audit-and-optimization-batch.md` §2.6 |
| P1-4 | `response_offset_key` 无实现 | 待决 | 实现按响应偏移修正翻页,或删除该公共入口并记录破坏性变更 | `../rounds/54-code-audit-and-optimization-batch.md` §2.7 |
| P2-1 | 门面 `translate_work` 未接流式产物 | 待做 | 同一内存源文档的三腿 A/B 与产物 SHA 门;门面产物字节不变 | `../rounds/54-code-audit-and-optimization-batch.md` §3.1 |
| P2-2 | NEMO 内存形态整份深拷贝 | 待做 | `editable_document` 在 Arc 唯一时直接移出文档;分配读数下降且产物不变 | `../rounds/54-code-audit-and-optimization-batch.md` §3.2 |
| P2-3 | `cloudvar` 断线 flush 抖动与关闭阻塞 | 待做 | 断线积压时不按固定短周期全量搬移队列;`close()` 不被重连退避长时间阻塞 | `../rounds/54-code-audit-and-optimization-batch.md` §3.3 |
| P2-4 | 库内嵌路径与客户端注入 | 待方案 | `PathConfig` 根与门面客户端可显式配置;staging 跟随输出目录;API 形态先定方案 | `../rounds/54-code-audit-and-optimization-batch.md` §3.4 |
| P2-5 | `translate_file` 网络选项与源引用失败报告 | 待做 | 网络型选项的生效范围在文档或参数校验中明确;源引用上传失败进入报告或结果字段 | `../rounds/54-code-audit-and-optimization-batch.md` §3.5 |
| P3-1 | 文档读数与承诺漂移 | 待做 | 举报上限口径、pre-commit 范围、分页选项与知识库中的过时结论同步修正 | `../rounds/54-code-audit-and-optimization-batch.md` §4 |
| P3-2 | `ide/load` 编辑格式快速通道 | 待核验 | 证明版本选择、权限回退与同版语义等价后,再决定是否作为可选入口 | `../rounds/54-code-audit-and-optimization-batch.md` §5.1 |
| P3-3 | 两个独立作品源串行请求 | 待做 | 两个互不依赖的请求并行发起,错误与结果顺序语义不变 | `../rounds/54-code-audit-and-optimization-batch.md` §5.2 |
| P3-4 | 目录上传逐文件串行 | 待做 | 目录上传使用有界并发,失败清单与返回值语义不变 | `../rounds/54-code-audit-and-optimization-batch.md` §5.3 |
| P3-5 | 举报分页死分支清理 | 待做 | 删除恒空的 `pending_items` 分支,分块行为由测试固定 | `../rounds/54-code-audit-and-optimization-batch.md` §5.4 |

## 2. 维护约定

- 状态术语只使用 `待决` / `待做` / `待核验` / `待方案` / `已完成(日期,提交号)` / `部分` / `判不做` / `暂缓(待重新立项)`。
- 每项在对应轮次记录中只展开一次读数与理由;本页只保留验收与指针。
- 代码修复必须先过 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 与相关离线测试;有产物的改动另过 SHA 基线。
