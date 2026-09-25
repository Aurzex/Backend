// 各编辑器(作品类型)的抓取器与反编译器
// 本目录按作品类型切分,每个编辑器一个文件;编译版积木 → 编辑版的通用骨架在 `../blocks/`,
// 与类型无关的公共设施(配置/加解密/HTTP/文件/模型)在 `../shared/`。
//
// - `simple.rs`: COCO / NEKO / WOOD(三个轻量编辑器合一处)
//   - `NekoFetcher`: 取作品详情并下载加密内容(`RawWorkData::NekoEncrypted`)
//   - `NekoDecompiler`: BCMKN 解密 → JSON
// - `nemo.rs`: NEMO
//   - `NemoFetcher` / `NemoDecompiler`: 解密与场景重建
//   - `NemoResourceManager`: 素材/封面/用户库落盘
//   - `WoodFetcher` / `WoodDecompiler`: 解密与作品重建
//   - `CocoFetcher` / `CocoDecompiler`: 取源文件并重建场景/角色
// - `kitten.rs`: Kitten(Kitten2/3/4)
//   - `KittenFetcher` 与 `KittenDecompiler` 的 `WorkDecompiler` 实现
//   - 编译版积木树 → 编辑版重建(角色/场景/全局字段)
//   - `XmlBlockWriter`: 编译版积木树 → blocksXML 序列化(Kitten2/3 编辑版)
//
// 门面(`decompile/mod.rs`)只经本文件取用下列十个名字,子模块内部结构可自由调整:
// `CocoDecompiler` / `CocoFetcher`、`KittenDecompiler` / `KittenFetcher`、
// `NekoDecompiler` / `NekoFetcher`、`NemoDecompiler` / `NemoFetcher`、
// `WoodDecompiler` / `WoodFetcher`。

pub(crate) mod kitten;
pub(crate) mod nemo;
pub(crate) mod simple;

pub(crate) use kitten::{KittenDecompiler, KittenFetcher};
pub(crate) use nemo::{NemoDecompiler, NemoFetcher};
pub(crate) use simple::{
    CocoDecompiler, CocoFetcher, NekoDecompiler, NekoFetcher, WoodDecompiler, WoodFetcher,
};
