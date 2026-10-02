# 作品文件格式(权威事实)

> 知识库条目:三种作品文件**实际长什么样**。全部来自真实样例与官方前端 bundle,出处见文末「依据」。
> 方案与实施过程见 `../rounds/20-kitten-kn-work-conversion-plan.md`。

## 1. 三种扩展名与顶层结构

| 扩展名   | 编辑器     | 积木存放位置                                                                                                        | 顶层关键字段                                                                                                                                                    |
| ---- | ---- | ---- | ---- |
| `.bcm`   | Kitten 2/3 | `theatre.{actors,scenes}.<uuid>.blocksXML`(**Blockly XML 字符串**)                                                  | `width`/`height`、`theatre.{scenes,actors,styles,scenes_order,current_scene,current_entity}`、`variables`、`variable_order`、`audio`、`toolbox`、`work_type:"KITTEN"`、`version:16` |
| `.bcm4`  | Kitten 4   | `theatre.{actors,scenes}.<uuid>.block_data_json`(**JSON 积木图**)                                                    | `size:{width,height}`、`theatre.{scenes,actors,styles,groups,timer,videos,scenes_order}`、`variables`、`variable_order`、`cloud_variables`、`broadcasts`、`audio`、`midimusic`、`matrix`、`models`、`toolbox`、`version:25` |
| `.bcmkn` | KN(Neko)   | `actors.actorsDict.<uuid>.nekoBlockJsonList`、`scenes.scenesDict.<uuid>.nekoBlockJsonList`、`procedures.proceduresDict.<uuid>.nekoBlockJsonList` | `stageSize:{width,height}`、`styles.stylesDict`、`variables.variablesDict`、`broadcasts.broadcastsDict`、`audios.audiosDict`、`scenes{scenesDict,currentSceneId,sortList}`、`actors.actorsDict`、`procedures.proceduresDict`、`projectName`、`version`(**字符串**,真机 0.13.0 / 0.27.1)、`toolType:"KN"`、`textToBlock`、`hidden_toolbox`、`previewUrl`、`resourceZip`、`guideUrl`、`aiImageUrls` |

**Kitten4 与 KN 的顶层布局不同**,不是"同一份 JSON 换名字":Kitten4 用 `theatre.*` 包实体,KN 用 `actorsDict`/`scenesDict`/`stylesDict` 平铺。转换必须做一次**重排**(官方也分两阶段做,见 `convert-semantics.md`)。

## 2. Kitten4 的 `block_data_json`

```json
{
  "blocks": { "<id>": { "type": "...", "id": "...", "shadows": {"times": "<shadow xmlns=…/>"}, "fields": {}, "mutation": "", "location": [x,y], "parent_id": "…" } },
  "connections": { "<parentId>": { "<childId>": { "type": "next"|"input", "input_name": "…" } } },
  "comments": {}
}
```

- `blocks` 是 **id → 积木对象** 字典;`connections` 是父子邻接表,**根积木 = 从未作为子键出现的 id**。
- `shadows` 的值仍然是 **XML 字符串**(Kitten4 沿用 XML shadow,KN 亦然 ⇒ 双向搬运可直接复用)。
  ⚠️ **例外(老形态)**:平台上还有作品把槽值写成**内联对象**(`{type, id, visible, editable, fields}`;全语料仅一件)。
  正向入口会把它就地改写成平台同款影子 XML 再解析(`pipeline::normalize_object_shadows`,只改解析副本);
  形态依据、只作必要条件的理由与残留,见 `convert-semantics.md` §3(**此处不重复**)。
- 实测:188 个积木 ↔ 188 个 `connections` 条目(86 个非空),叶子节点是空对象 `{}`。
- 反向写 `.bcm4` 时 `blocks`/`connections`/`parent_id`/`location` **都要重建**。
- **编译版**(`compiled_block_map`)与编辑版不同:编译版引用**恒为内联对象**,字符串 id 只出现在编辑版的 `connections`(实测 2236 处采样,字符串 0 处)。见 `src/core/convert/decompile/mod.rs`(编译版积木层,原 `blocks.rs`)。

## 3. KN 的积木节点模型(`nekoBlockJsonList` 元素)

三个生产者(Kitten 路径 `jC.parseBlock`、Nemo 路径 `hI.parseBlocksXML`、文本路径 `DC`)与一个消费者(`Ey` block→text)共用同一节点模型:

| 字段 | 类型 | 说明 |
| ---- | ---- | ---- |
| `type` | string | KN 积木类型名(`on_running_group_activated` / `repeat_n_times` / `variables_set` …) |
| `id` | string | **UUID**(`crypto.randomUUID()`);Kitten 侧的 22 字符 nanoid **不是**同一体系,以 UUID 为准 |
| `location` | `[x,y]` | 工作区坐标 |
| `next` | node | 单后继 |
| `inputs` | `{slot: node}` | 值输入(`input_value`) |
| `statements` | `{slot: node}` | 语句槽(`input_statement`:`DO`/`DO0`/`STACK`/`ELSE` …) |
| `fields` | `{name: value}` | 字段(下拉、实体引用 id 等) |
| `shadows` | `{slot: xmlString}` | 每输入的 shadow(**XML 字符串**;空串 `""` = 占位) |
| `mutation` | string | mutation XML(`<mutation xmlns="http://www.w3.org/1999/xhtml" …>`) |
| `is_shadow`/`is_output`/`shield`/`disabled`/`deletable`/`editable` | bool | 渲染/行为标志 |
| `field_constraints` | `{field:{min,max,precision,mod}}` | 数字输入约束(相机/变量等) |
| `parent_id` | string | 反向引用,可选 |

## 4. 程序集是独立实体

带返回值的函数定义在 KN 里**不在** `nekoBlockJsonList` 内重复,而是抽进 `procedures.proceduresDict`:

```json
"procedures": { "proceduresDict": { "<procId>": {
  "id": "…", "name": "函数_数_字符_5", "type": "NORMAL" | "ROUND",
  "params": [ { "id": "…", "type": "Label" | "String", "name": "字符串" } ],
  "nekoBlockJsonList": [ { "type": "procedures_2_defnoreturn", "fields": { "NAME": "<procId>" }, … } ],
  "comments": {}, "workspaceScrollXy": { "x": 100, "y": 30 }
} } }
```

## 5. 加密与传输

| 形态 | 是否加密 | 依据 |
| ---- | ---- | ---- |
| `.bcmkn` 经 **NEKO 播放器详情接口**取回(`source_urls[0]`) | **是**:`reversed(base64-STANDARD)` → `IV(12) ‖ AES-256-GCM(明文 JSON)`;key = `SHA256(salt)`,`salt = 0x00..0x1E` | 本库 `unpacker.rs` 已实现并真机验证 |
| `.bcmkn` 落盘产物 / CDN 模板 / 编辑器本地导入 | **否**,明文 JSON | `kn-default-v-0.13.1.bcmkn`(3.7 KB)实测明文 |
| `.bcm/.bcm4`(播放器 `player/load`、编辑版文件) | **否**,明文 JSON | 本库无 Kitten 侧加密代码 |

**编辑器自身不做文件加解密**:4 个主 bundle + 107 个 chunk 全文检索 `AES/CryptoJS/decrypt/encrypt/subtle/forge` 只命中 vendored 库内部标识与 `crypto.getRandomValues`。加密只出现在 NEKO 播放器详情接口。

## 6. 实现必读的边界与陷阱

1. **舞台尺寸只有两档**:KN 仅 `562×900`(竖)/ `900×562`(横)。Kitten 的 `size` 任意 ⇒ 反向必须显式选一个目标尺寸(`ConvertOptions.stage`),否则坐标失真。官方代码里额外的 `/1.3` 是 Kitten4 横屏特例,**不是**通用缩放。
2. **可选字段不可假设齐全**:同一节点模型在真实作品里字段有无不一(HEX Editor 的 359 个节点:`location` 16、`mutation` 19、`comment` 0)。解析/比较要容忍缺失,写出时按模板给全。
3. **shadow XML 有多形态**:带/不带 `xmlns`、带/不带 `id`、约束字面量 `-Infinity,Infinity,0,` vs `1,Infinity,1,` —— 语义等价、字节不同。产出要**稳定**(带 `xmlns` + 显式 `id`),比较用语义 diff。
4. **脏键会传染**:`broadcasts.broadcastsDict` 里出现过 `"toJSON"` 这类键,本库反编译产物里也有 ⇒ 转换前应过滤/告警。
5. **大文件是常态**:真实作品 3.7 MB(HEX Editor)到 63 598 143 B(≈60.6 MiB,`原气骑士 且听风吟-编辑版.bcm4`)。构建"邻接表→树"要用 `HashMap` 一次归并,禁止线性查找父节点。
6. **资源(造型/音频)必须是可访问 URL**:官方导入时会 `fetch` + 重新 `upload` 并归一化 `centerPoint`,失败是 `try/catch` + 继续。
7. **`source` 字段是"保留原件"**:KN 编辑器导入 Kitten 作品时,把**原始 Kitten 文件字节**当 `bcm4` 重新上传,URL 写进 KN 作品的 `source`。

## 依据

- `../rounds/20-kitten-kn-work-conversion-plan.md` §2(结构表、`block_data_json`、节点模型、加密矩阵)、§8(陷阱)、§11(实测)。
- 样例:`download/compile/raw/春风得意-编辑版.bcm`(Kitten3)、`几何对战-联机.bcm4`(Kitten4)、`download/compile/HEX Editor_317683843.bcmkn`、`https://creation.codemao.cn/neko/bcm/kn-default-v-0.13.1.bcmkn`。
- 代码锚点:`src/core/convert/decompile/mod.rs`(编译版引用校验)、`src/core/convert/shared.rs`(bcmkn 解密)、`src/core/convert/translate/model.rs`(节点模型)。
