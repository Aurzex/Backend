# 第六十三轮记录 — 2026-11-08 横竖屏判定单一来源

## 0. 一句话

`width > height` 这条规则原先在正向装配与两条管线(K4→KN、KN→K4)各写一遍;本轮收成
`assembly::landscape_from_size(width, height)` 一处,行为与产物字节不变。

## 1. 改动

| 位置 | 之前 | 之后 |
| --- | --- | --- |
| `translate/assembly.rs::build_document_product` | `let landscape = src_w > src_h;` | `landscape_from_size(src_w, src_h)` |
| `translate/pipeline.rs`(正向) | `let landscape = src_w > src_h;` | 同上 |
| `translate/pipeline.rs`(反向) | `let landscape = kn_w > kn_h;` | 同上 |

判据来源:官方 `UC = size.width > size.height`(`../rounds/20` §？的模块级常量;
`translate/mapping.rs` 的注释同源)。

## 2. 验证

- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings` 通过;
- `cargo test --lib` 165 passed / 2 ignored;
- `convert_bench` 六样本产物 SHA256 与 `#meta` 与基线一致;实体并发 1 vs 8 同 SHA。

## 依据

- 提交:`83993ce`;
- 审计来源:`../rounds/61` §3(T11 候选:同一规则两处实现);
- 口径:`../knowledge/convert-semantics.md` §6(产物字节门)、`../knowledge/convert-performance.md` §2bis。
