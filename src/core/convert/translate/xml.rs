//! 积木 XML 层:最小 DOM(NEMO/Scratch 风格积木 XML 的解析与序列化)+ 字符串级
//! 属性手术 + 影子/mutation 模板。
//!
//! 拆出来的动因是打断两条环:`model ⇄ mapping` 与 `nemo ⇄ nemo_mapping`
//! (拆分前 `model.rs` 从 `mapping` 借影子构造、`nemo_mapping.rs` 从 `nemo` 借 DOM)。
//! 本模块只依赖 `shared` 的**类型**,不反向依赖 `model`/`mapping`/`nemo`。

use crate::core::convert::shared::{ConvertError, XHTML};
use serde_json::Value;
use std::ops::Range;

/// 从整段 XML 取开始标签里 `attr="…"` 的值(先按引号感知找标签尾,再取属性)
/// 唯一的 XML 属性读取实现:本模块与 `model.rs` 的 mutation 改写共用(那份改写原在 `neko.rs`,
/// 已随 `41470d9` 并入 `model.rs`)
pub(super) fn xml_attr_value<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    attr_value(&xml[..start_tag_end(xml)?], attr)
}

/// 开始标签里 `attr="…"` 的值区间(只认双引号属性;属性名前须是空白,避免 `xname=` 误命中)
pub(super) fn attr_span(tag: &str, attr: &str) -> Option<Range<usize>> {
    let bytes = tag.as_bytes();
    let mut from = 0;
    loop {
        let at = from + tag[from..].find(attr)?;
        let name_end = at + attr.len();
        if (at == 0 || bytes[at - 1].is_ascii_whitespace())
            && bytes.get(name_end) == Some(&b'=')
            && bytes.get(name_end + 1) == Some(&b'"')
        {
            let start = name_end + 2;
            return Some(start..start + tag[start..].find('"')?);
        }
        from = name_end;
    }
}

pub(super) fn attr_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    attr_span(tag, attr).map(|span| &tag[span])
}

/// 替换开始标签里某个属性的值(其余字节原样保留)
pub(super) fn set_attr_value(tag: &str, attr: &str, value: &str) -> String {
    match attr_span(tag, attr) {
        Some(span) => format!("{}{}{}", &tag[..span.start], value, &tag[span.end..]),
        None => tag.to_string(),
    }
}

/// `<` 到开始标签结束(跳过引号里的 `>`)
pub(super) fn start_tag_end(xml: &str) -> Option<usize> {
    let mut quoted = false;
    for (i, b) in xml.as_bytes().iter().enumerate().skip(1) {
        match b {
            b'"' => quoted = !quoted,
            b'>' if !quoted => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// 官方 `text_select_changeable` 修补:`items` 属性减一(解析失败则原样保留)
pub(super) fn decrement_items(xml: &str) -> String {
    match attr_span(xml, "items").and_then(|span| {
        xml[span.clone()]
            .trim()
            .parse::<i64>()
            .ok()
            .map(|n| (span, n))
    }) {
        Some((span, n)) => format!("{}{}{}", &xml[..span.start], n - 1, &xml[span.end..]),
        None => xml.to_string(),
    }
}

/// 官方 `parseInt(s, 10)` 的前缀解析(允许前导空白、正负号);解析不出数字 → `None`(JS 的 `NaN`)
///
/// NEMO 侧的属性值解析共用这一份(`nemo_mapping.rs` 的 `parse_int` 与 `nemo.rs` 的几个前置改写;
/// 原先两边各有一份逐字相同的副本)。
pub(super) fn parse_int_prefix(text: &str) -> Option<i64> {
    let trimmed = text.trim_start();
    let (sign, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    digits[..end].parse::<i64>().ok().map(|value| value * sign)
}

/// JS 属性访问 / `String(v)` / 模板拼接的等价物(缺失 → `undefined`)
pub(super) fn js_text(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
    }
}

/// 官方两个算术包装里共用的 `math_number` 影子串(默认值恒为 `1`)
pub(super) fn math_number_shadow(id: &str, num: &str) -> String {
    format!(
        "<shadow xmlns=\"{XHTML}\" type=\"math_number\" id=\"{id}\" visible=\"visible\"><field constraints=\"-Infinity,Infinity,0,\" name=\"NUM\">{num}</field></shadow>"
    )
}

pub(super) fn pure_list_shadow(id: &str, list: &Value) -> String {
    format!(
        "<shadow xmlns=\"{XHTML}\" type=\"pure_list_get\" id=\"{id}\" visible=\"visible\" inline=\"true\"><field name=\"list\">{}</field></shadow>",
        js_text(Some(list))
    )
}

/// `<mutation …>正文</mutation>` 的正文(占位积木的降级文本就存在这里)
pub(super) fn mutation_body(xml: &str) -> &str {
    let Some(open) = xml.find('>') else { return "" };
    let rest = &xml[open + 1..];
    match rest.rfind("</mutation>") {
        Some(close) => &rest[..close],
        None => rest,
    }
}

/// 源 XML 元素计数:`block` / `shadow` / `empty`(官方研究的统计口径)
pub(super) fn count_source_elements(nodes: &[XmlNode]) -> usize {
    let mut total = 0;
    let mut stack: Vec<&XmlNode> = nodes.iter().collect();
    while let Some(node) = stack.pop() {
        if matches!(node.tag.as_str(), "block" | "shadow" | "empty") {
            total += 1;
        }
        for child in &node.children {
            stack.push(child);
        }
    }
    total
}

// 来自 src/core/convert/translate/nemo_xml.rs
// NEMO 编辑版积木 XML 的**最小 DOM**。
// NEMO(Scratch 风格编辑器)把积木存成一段 `text/xml` 字符串,官方前端用浏览器的
// `DOMParser.parseFromString(xml, "text/xml")` 解析、`XMLSerializer.serializeToString`
// 回写,再用 `getAttribute`/`setAttribute`/`removeAttribute`/`textContent` 做改写。
// 官方 `blockly` 侧的惯例是给一段积木 XML 套一层 `<root>` 再解析(包装根
// `<variables></variables>` 那种也是同一用法),所以这里同时提供
// [`parse`](整份文档)与 [`parse_fragment`](整段输入 = 虚拟包装根的内容,取直接子元素)。
// Rust 侧没有 DOM,本文件把同一套语义**最小可用**地移植过来:
// | 浏览器 | 本文件 |
// | --- | --- |
// | `DOMParser.parseFromString(xml, "text/xml")` | [`parse`] |
// | `parseFromString("<root>" + blocksXML + "</root>", …)` 取 `root.childNodes` | [`parse_fragment`](语义同上,但**不构造**包装串) |
// | `XMLSerializer.serializeToString(node)` | [`XmlNode::serialize`] |
// | `getAttribute` / `setAttribute` / `removeAttribute` | [`XmlNode::attr`] / [`XmlNode::set_attr`] / [`XmlNode::remove_attr`] |
// | `textContent` | [`XmlNode::text_content`] |
// | `querySelector` / `getElementsByTagName` 的"首个/全部子元素"用法 | [`XmlNode::child`] / [`XmlNode::child_mut`] / [`XmlNode::children_of`] |
// 对齐浏览器的几个语义点:
// - 属性**按出现顺序**存放:浏览器命名属性表不保证顺序,但序列化顺序即插入顺序,
// 所以 `set_attr` 同名**替换值并保持原位置**、异名**追加到末尾**,与 `setAttribute` 一致;
// - 序列化转义与 `XMLSerializer` 一致:属性值转义 `&` `"` `<` `>`,文本转义 `&` `<` `>`;
// - 空元素(无子元素且无文本)序列化成自闭合 `<tag/>`,哪怕原文写的是 `<tag></tag>`
// (浏览器同样如此);文本哪怕只是空白,也不算空元素;
// - 未声明的属性返回 `None`,与 `getAttribute` 返回 `null` 对应:空白串 ≠ 缺失;
// - 未知实体(`&nbsp;`)直接报错,与 `text/xml` 下的 `DOMParser` 一致
// (`text/html` 会容错,我们不学它);
// - 畸形输入(未闭合 / 开闭不匹配 / 属性缺引号 / 意外字符)一律返回
// [`ConvertError::Decompile`],带行、列与字节偏移,不 panic、不产出半截结果。
// 已知取舍(与浏览器**不**完全等价的地方,调用方需要知道):
// - **不做命名空间处理**:`xmlns` / `xmlns:xxx` 只是普通属性,按字符串读写
// (积木 XML 里的命名空间只是装饰,没有前缀解析/`localName` 语义);
// - **文本只保留"两段式"**:DOM 里文本节点与元素节点交错,这里把元素之间的直接文本
// 按文档顺序拼进父节点的 [`XmlNode::text`],序列化时统一排在子元素**之后**。
// 因此对"子元素之间夹着文本"的输入,`serialize` 的输出与输入不逐字节相同,但
// `parse(serialize)` 与 `parse` 的结果相同(不动点),反复读写稳定。
// Scratch 积木 XML 的文本只出现在叶子元素里(field/mutation),真实数据不受影响;
// - **宽松处**:重复属性名按出现顺序各留一项(浏览器会当畸形文档报错);注释里含 `--`、
// 文本里含 `]]>` 等次要良构约束不校验;
// - **严格处**:根元素之外出现文本、无匹配的结束标签或非法 `<!` 开头标记一律报错(与
// `text/xml` 下的 `DOMParser` 一致);[`parse`] 允许顶层有多个元素(取第一个,编辑器
// 会把 `<variables></variables>` 与各根积木并排存),[`parse_fragment`] 则把整段输入当成
// **虚拟包装根的内容**(顶层元素全部返回,"唯一根"的要求随包装串一起去掉);
// - **嵌套上限**:元素嵌套超过 `MAX_DEPTH` 层直接报错(真实作品只有几十层)。
// 解析/序列化/取文本都是**迭代**实现(显式栈),不依赖调用方线程的栈大小;
// 唯一与嵌套深度相关的递归是 `XmlNode` 自身的析构,上限就是为了兜住它。

/// 元素嵌套上限:超过即报错。
///
/// 真实作品的积木嵌套只有几十层,这里给足余量;设上限的唯一目的是兜住
/// `XmlNode` 递归析构(`Drop`)与畸形输入的内存占用。
const MAX_DEPTH: usize = 4096;

/// 一个 XML 元素(属性按出现顺序存,子节点按文档顺序存)
#[derive(Debug, Clone, PartialEq)]
pub(super) struct XmlNode {
    /// 元素名(Scratch 积木 XML 一律小写:`block`/`shadow`/`value`/`field`/…)
    pub tag: String,
    /// 属性:按**出现顺序**存放(同名重复的按宽松策略各留一项)
    pub attrs: Vec<(String, String)>,
    /// 子元素:按文档顺序存放
    pub children: Vec<XmlNode>,
    /// 本元素的**直接**文本(不聚合子节点;解析时保留原样,转义已解码)
    pub text: String,
}

impl XmlNode {
    /// 新建元素(无属性、无子元素、无文本)
    pub(super) fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
            attrs: Vec::new(),
            children: Vec::new(),
            text: String::new(),
        }
    }

    /// 首个同名属性;未声明同名属性时返回 `None`(注意:不存在 ≠ 空串,对应 `getAttribute` 的 `null`)
    pub(super) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 同名则替换值(**保持原位置**),否则追加到末尾(`DOMParser` + `setAttribute` 语义)
    pub(super) fn set_attr(&mut self, name: &str, value: &str) {
        match self.attrs.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = value.to_string(),
            None => self.attrs.push((name.to_string(), value.to_string())),
        }
    }

    /// 删除所有同名属性(对应 `removeAttribute`;属性不存在时无操作)
    /// 注:当前只被本文件的 `#[cfg(test)]` 用例使用 ⇒ 生产构建里不需要它。
    #[cfg(test)]
    pub(super) fn remove_attr(&mut self, name: &str) {
        self.attrs.retain(|(k, _)| k != name);
    }

    /// 首个直接子元素(tag 精确匹配,不做大小写折叠)
    pub(super) fn child(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// 首个直接子元素(可变)
    pub(super) fn child_mut(&mut self, tag: &str) -> Option<&mut XmlNode> {
        self.children.iter_mut().find(|c| c.tag == tag)
    }

    /// 所有直接子元素中 tag 匹配的那些(文档顺序)
    pub(super) fn children_of<'a>(
        &'a self,
        tag: &'a str,
    ) -> impl Iterator<Item = &'a XmlNode> + 'a {
        self.children.iter().filter(move |c| c.tag.as_str() == tag)
    }

    /// **深度优先**拼接自身文本与所有后代的文本(与 DOM `textContent` 同义)。
    ///
    /// 元素按文档顺序先序展开("两段式"模型下父节点的直接文本无法还原交错位置,
    /// 这里按"自身文本 + 各后代"的顺序拼;对文本只出现在叶子里的积木 XML 结果一致)。
    pub(super) fn text_content(&self) -> String {
        let mut out = String::new();
        // 显式栈先序遍历:弹一个节点 → 收它的直接文本 → 逆序压入子节点
        let mut stack: Vec<&XmlNode> = Vec::with_capacity(8);
        stack.push(self);
        while let Some(node) = stack.pop() {
            out.push_str(&node.text);
            for child in node.children.iter().rev() {
                stack.push(child);
            }
        }
        out
    }

    /// 与浏览器 `XMLSerializer` 等价的最小实现。
    ///
    /// 布局:`<tag a="1" b="2">子元素序列 + 文本</tag>`;无子元素且无文本 → 自闭合 `<tag/>`。
    /// 迭代实现(显式工作栈),深层嵌套不会栈溢出。
    pub(super) fn serialize(&self) -> String {
        /// 待完成的工作项(先序:开始标签 → 子元素 → 文本 → 结束标签)
        enum Job<'a> {
            Open(&'a XmlNode),
            Text(&'a str),
            Close(&'a XmlNode),
        }

        let mut out = String::new();
        let mut jobs: Vec<Job<'_>> = Vec::with_capacity(8);
        jobs.push(Job::Open(self));
        while let Some(job) = jobs.pop() {
            match job {
                Job::Open(node) => {
                    out.push('<');
                    out.push_str(&node.tag);
                    for (name, value) in &node.attrs {
                        out.push(' ');
                        out.push_str(name);
                        out.push_str("=\"");
                        push_attr_escaped(&mut out, value);
                        out.push('"');
                    }
                    if node.children.is_empty() && node.text.is_empty() {
                        // 空元素:浏览器也序列化成自闭合
                        out.push_str("/>");
                    } else {
                        out.push('>');
                        // 栈是后进先出:先压结束标签与文本,再逆序压子节点,
                        // 弹出顺序即为"子元素(文档序) → 文本 → 结束标签"
                        jobs.push(Job::Close(node));
                        jobs.push(Job::Text(&node.text));
                        for child in node.children.iter().rev() {
                            jobs.push(Job::Open(child));
                        }
                    }
                }
                Job::Text(text) => push_text_escaped(&mut out, text),
                Job::Close(node) => {
                    out.push_str("</");
                    out.push_str(&node.tag);
                    out.push('>');
                }
            }
        }
        out
    }
}

/// 属性值转义:`&` `"` `<` `>`(与 `XMLSerializer` 一致;单引号不需要转,因为统一用双引号包)
fn push_attr_escaped(out: &mut String, s: &str) {
    // 绝大多数属性值(编辑器 id、类名、数字)无需转义:先做一次扫描快速返回
    if !s.contains(['&', '"', '<', '>', '\r']) {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // 与官方 harness 的 `dom.js#encodeAttr` 一致:回车也走数字实体
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

/// 文本转义:`&` `<` `>`(与 `XMLSerializer` 一致)
fn push_text_escaped(out: &mut String, s: &str) {
    if !s.contains(['&', '<', '>', '\r']) {
        out.push_str(s);
        return;
    }
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // 与官方 harness 的 `dom.js#encodeText` 一致:回车也走数字实体
            '\r' => out.push_str("&#13;"),
            _ => out.push(c),
        }
    }
}

/// 解析一整份文档,返回**第一个元素**
/// (跳过 XML 声明/注释/DOCTYPE/前后空白/末尾其它节点)
/// 注:只被本文件的 `#[cfg(test)]` 用例使用(生产入口是 [`parse_fragment`])⇒ 标 `cfg(test)`;
/// 它连带 [`Parser::run`](仅此处调用)一起只进测试构建。
#[cfg(test)]
pub(super) fn parse(xml: &str) -> Result<XmlNode, ConvertError> {
    let mut parser = Parser::new(xml);
    let roots = parser.run()?;
    // 顶层可以有多个元素(编辑器把 `<variables></variables>` 与各根积木并排存),
    // `parse` 只要第一个,与"取第一个元素"的调用方一致
    match roots.into_iter().next() {
        Some(node) => Ok(node),
        None => Err(ConvertError::Decompile(format!(
            "积木 XML 解析失败:文档里没有任何元素(输入 {} 字节)",
            xml.len()
        ))),
    }
}

/// 解析一段**片段**(NEMO/KittenN 的 `blocksXML`),返回它的顶层元素
///
/// 等价于官方 `parseFromString("<root>" + blocksXML + "</root>", "text/xml")` 之后取
/// `root.childNodes`,但**不构造**那个包装串:整段输入被当成一个**虚拟包装根**的内容
/// (见 [`Parser::run_wrapped`])⇒ 顶层文本/空白的落点、嵌套深度计数、成功结果都与套了
/// `<root>` 的写法**逐属性相同**,而"每个实体一次整串 `format!`"没了(3.4 MB 的 NEMO 作品有
/// 847 个实体 ⇒ 省 847 次整串拷贝,见 `docs/rounds/39` §W8)。
///
/// 与"真套一层 `<root>`"的差异**只在失败路径**,共三处(成功路径由
/// `nemo_xml_tests::parse_fragment_equals_wrapped_parse` 逐字段对照):
/// 1. **错误位置整体左移 6 字节**(少掉 `"<root>"` 前缀):`err_at` 里报的 byte/line/col 与
///    包装写法不同 —— 都是 `Err`,只是坐标不同,不进产物、只进 `DroppedField` 的文案;
/// 2. 输入在元素**未闭合**时结束 → 这里报"元素 `<x>` 未闭合",包装版被末尾那个 `</root>` 撞成
///    "开闭不匹配";
/// 3. 输入里出现能**闭合虚拟根**的 `</root>`(裸的顶层结束标签)→ 这里显式报错
///    ("与虚拟包装根提前闭合"),包装版则报"根外非法标记/开闭不匹配" —— 两者都是 `Err`,
///    关键是**都不允许**把它当成"片段结束"而静默丢掉后面的内容。
///
/// 契约上另外两处放宽(都与**旧的生产路径**一致,只是不再需要包装串):顶层可以有多个元素
/// (本来就合法),空片段 = 0 个顶层元素。
pub(super) fn parse_fragment(xml: &str) -> Result<Vec<XmlNode>, ConvertError> {
    let mut parser = Parser::new(xml);
    let root = parser.run_wrapped("root")?;
    Ok(root.children)
}

/// 名称首字符:字母 / `_` / `:`(允许 `:` 只是为了让 `<a:b>` 这类名字别被误判,不做命名空间)
fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':'
}

/// 名称后续字符
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | ':' | '-' | '.')
}

/// 解码数字实体(`&#39;` 十进制 / `&#x27;` 十六进制);非法或越界返回 `None`
fn decode_numeric_entity(body: &str) -> Option<char> {
    let digits = body.strip_prefix('#')?;
    let code = match digits
        .strip_prefix('x')
        .or_else(|| digits.strip_prefix('X'))
    {
        // 十六进制:至少一位,且全是十六进制数字
        Some(hex) => {
            if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(hex, 16).ok()?
        }
        // 十进制
        None => {
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            digits.parse::<u32>().ok()?
        }
    };
    // 代理区(U+D800..U+DFFF)与超出 U+10FFFF 的码点取不到字符,按畸形报错
    char::from_u32(code)
}

/// 极简 XML 解析器。
///
/// 迭代实现:用"未闭合元素栈"代替递归下降,栈深即嵌套深度,
/// 因此深层嵌套只吃堆内存,不吃调用栈。
struct Parser<'a> {
    /// 待解析的完整输入(按字节索引推进,所有切片都落在字符边界上)
    src: &'a str,
    /// 当前字节偏移
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    /// 当前字符(不推进)
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// 取当前字符并推进(按 UTF-8 长度推进,保证不会切在字符中间)
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn starts_with(&self, needle: &str) -> bool {
        self.src[self.pos..].starts_with(needle)
    }

    /// 跳过 XML 空白(空格/Tab/CR/LF)。
    ///
    /// 不能直接用 `char::is_whitespace`:那会把 NBSP 之类的也当空白吞掉,
    /// 而 XML 里 NBSP 是正经的文本内容。
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if matches!(c, ' ' | '\t' | '\r' | '\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    /// 期望当前字符是 `want`(否则按当前位置报错)
    fn expect_char(&mut self, want: char, msg: &str) -> Result<(), ConvertError> {
        match self.peek() {
            Some(c) if c == want => {
                self.bump();
                Ok(())
            }
            _ => Err(self.err_here(msg)),
        }
    }

    /// 构造带定位信息的解析错误
    fn err_here(&self, msg: impl std::fmt::Display) -> ConvertError {
        self.err_at(self.pos, msg)
    }

    /// 带行、列与字节偏移的解析错误,便于定位畸形输入
    fn err_at(&self, pos: usize, msg: impl std::fmt::Display) -> ConvertError {
        let mut line = 1usize;
        let mut col = 1usize;
        for (i, c) in self.src.char_indices() {
            if i >= pos {
                break;
            }
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        ConvertError::Decompile(format!(
            "积木 XML 解析失败(第 {line} 行第 {col} 列,字节 {pos}): {msg}"
        ))
    }

    /// 解析一个 XML 名称(元素名/属性名)。
    ///
    /// Scratch 积木 XML 的名称是纯 ASCII(`procedures_2_parameter_shadow` 这种),
    /// 这里放宽到 Unicode 字母,避免对合法文档误报。
    fn parse_name(&mut self) -> Result<String, ConvertError> {
        let start = self.pos;
        match self.peek() {
            Some(c) if is_name_start(c) => self.pos += c.len_utf8(),
            _ => return Err(self.err_here("期望元素名或属性名")),
        }
        while let Some(c) = self.peek() {
            if is_name_char(c) {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }
        Ok(self.src[start..self.pos].to_string())
    }

    /// 解析整份文档,返回所有**顶层**元素(调用方决定取第一个还是要求唯一)
    /// 注:只被 [`parse`](cfg(test) 的函数)调用 ⇒ 同样只进测试构建(`run_wrapped` 走 `run_inner`)。
    #[cfg(test)]
    fn run(&mut self) -> Result<Vec<XmlNode>, ConvertError> {
        self.run_inner(None)
    }

    /// 把整份输入当成**某个包装根的内容**来解析,返回那个包装根
    ///
    /// 与 `parse(&format!("<{tag}>{xml}</{tag}>"))` 等价,但**不构造**包装串(见 [`parse_fragment`])。
    fn run_wrapped(&mut self, tag: &str) -> Result<XmlNode, ConvertError> {
        let roots = self.run_inner(Some(tag))?;
        match roots.into_iter().next() {
            Some(root) => Ok(root),
            // `virtual_root = Some` 时必然产出一个根;真走到这里就是解析器坏了,报错不 panic
            None => Err(ConvertError::Decompile(
                "积木 XML 片段解析失败:虚拟包装根缺失".to_string(),
            )),
        }
    }

    /// [`run`](Self::run) / [`run_wrapped`](Self::run_wrapped) 的共同实现:
    /// `virtual_root = Some(tag)` 时先压入那个**合成**的包装根。
    fn run_inner(&mut self, virtual_root: Option<&str>) -> Result<Vec<XmlNode>, ConvertError> {
        let mut roots: Vec<XmlNode> = Vec::new();
        // 未闭合元素栈:栈顶就是当前正在收文本/子元素的元素
        let mut open: Vec<XmlNode> = Vec::new();
        // 虚拟包装根:让整段输入从头到尾都处在"元素内容级"—— 顶层文本/空白的落点、嵌套深度
        // 计数、报错位置都与真套一层 `<root>` 的写法一致(唯一差别是元素未闭合时的错误文案)。
        if let Some(tag) = virtual_root {
            open.push(XmlNode::new(tag));
        }
        loop {
            if open.is_empty() {
                // 文档级:先跳过空白/注释/处理指令/DOCTYPE;
                // 此处再遇到文本、结束标签、非法 `<!` 都是畸形(DOMParser 同样报错)
                self.skip_document_misc()?;
                if self.eof() {
                    break;
                }
                if self.starts_with("<!DOCTYPE") {
                    self.skip_doctype()?;
                    continue;
                }
                if self.starts_with("<![CDATA[") {
                    return Err(self.err_here("根元素之外不允许出现 CDATA 段"));
                }
                if self.starts_with("<!") {
                    return Err(self.err_here("根元素之外出现非法的 <! 标记"));
                }
                if self.starts_with("</") {
                    return Err(self.err_here("结束标签没有对应的开始标签"));
                }
                if !self.starts_with("<") {
                    return Err(self.err_here("根元素之外不允许出现文本"));
                }
            } else {
                // 元素内容级:这里的空白属于**文本**,必须原样收进父节点,不能跳过
                if self.eof() {
                    // 虚拟包装根:输入结束就是它的 `</root>`(真包装版在这里同样收尾)
                    if virtual_root.is_some() && open.len() == 1 {
                        break;
                    }
                    let tag = open.last().map(|n| n.tag.as_str()).unwrap_or("");
                    let end = self.src.len();
                    return Err(self.err_at(end, format!("元素 <{tag}> 未闭合(缺少 </{tag}>)")));
                }
                // 结束标签:弹出栈顶并校验配对
                if self.starts_with("</") {
                    let close_pos = self.pos;
                    self.pos += 2;
                    let name = self.parse_name()?;
                    self.skip_ws();
                    self.expect_char('>', "结束标签未以 > 结束")?;
                    let node = match open.pop() {
                        Some(node) => node,
                        None => {
                            return Err(self.err_at(close_pos, "结束标签没有对应的开始标签"));
                        }
                    };
                    if node.tag != name {
                        return Err(self.err_at(
                            close_pos,
                            format!("结束标签 </{name}> 与开始标签 <{}> 不匹配", node.tag),
                        ));
                    }
                    // 虚拟包装根(R1②):输入里出现一个能**闭合它**的 `</root>` = 畸形 —— 真包装版
                    // (`<root>` + 片段 + `</root>`)在这里是"多出一个 </root>"从而撞成开闭不匹配/根外
                    // 非法标记,都必须 Err;若不拦,虚拟根会提前出栈、后面的内容被静默截断。
                    if virtual_root.is_some() && open.is_empty() {
                        return Err(self.err_at(
                            close_pos,
                            format!("片段里出现了 </{name}>,与虚拟包装根提前闭合"),
                        ));
                    }
                    match open.last_mut() {
                        Some(parent) => parent.children.push(node),
                        None => roots.push(node),
                    }
                    continue;
                }
                // 注释/PI/CDATA:丢弃或当文本,都不算子元素
                if self.starts_with("<!--") {
                    self.skip_comment()?;
                    continue;
                }
                if self.starts_with("<![CDATA[") {
                    let text = self.parse_cdata()?;
                    if let Some(parent) = open.last_mut() {
                        parent.text.push_str(&text);
                    }
                    continue;
                }
                if self.starts_with("<?") {
                    self.skip_pi()?;
                    continue;
                }
                if self.starts_with("<!") {
                    return Err(self.err_here("元素内容里出现非法的 <! 标记"));
                }
                // 普通文本(含空白)
                if !self.starts_with("<") {
                    let text = self.parse_text()?;
                    if let Some(parent) = open.last_mut() {
                        parent.text.push_str(&text);
                    }
                    continue;
                }
            }
            // 走到这里,pos 一定指向一个开始标签的 '<'
            if open.len() >= MAX_DEPTH {
                return Err(self.err_here(format!("元素嵌套超过 {MAX_DEPTH} 层,拒绝继续解析")));
            }
            let (node, self_closing) = self.parse_open_tag()?;
            if self_closing {
                match open.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => roots.push(node),
                }
            } else {
                open.push(node);
            }
        }
        // 虚拟包装根出栈:它与"真套一层 `<root>` 后解析出的那个唯一顶层元素"同形
        if virtual_root.is_some()
            && let Some(root) = open.pop()
        {
            roots.push(root);
        }
        Ok(roots)
    }

    /// 文档级杂项:空白、注释、`<?…?>` 处理指令(含开头的 `<?xml …?>` 声明)。
    /// 这些都对应 DOM 里的非元素节点,积木 XML 用不到,直接丢弃。
    fn skip_document_misc(&mut self) -> Result<(), ConvertError> {
        loop {
            self.skip_ws();
            if self.starts_with("<!--") {
                self.skip_comment()?;
                continue;
            }
            if self.starts_with("<?") {
                self.skip_pi()?;
                continue;
            }
            return Ok(());
        }
    }

    /// 解析开始标签(调用时 `pos` 指向 `<`),返回元素与"是否自闭合"
    fn parse_open_tag(&mut self) -> Result<(XmlNode, bool), ConvertError> {
        let tag_pos = self.pos;
        self.pos += 1; // '<'
        let tag = self.parse_name()?;
        let mut attrs: Vec<(String, String)> = Vec::new();
        loop {
            // 属性之间允许任意空白(含换行/Tab),这一点与 DOMParser 一致
            self.skip_ws();
            match self.peek() {
                None => {
                    return Err(self.err_at(
                        tag_pos,
                        format!("元素 <{tag}> 的属性区未结束(输入提前结束)"),
                    ));
                }
                Some('/') => {
                    // 自闭合:`/` 与 `>` 之间不允许空白(XML 文法如此)
                    self.pos += 1;
                    self.expect_char('>', "<x/ 后面必须紧跟 >")?;
                    return Ok((
                        XmlNode {
                            tag,
                            attrs,
                            children: Vec::new(),
                            text: String::new(),
                        },
                        true,
                    ));
                }
                Some('>') => {
                    self.pos += 1;
                    break;
                }
                Some(c) if is_name_start(c) => {
                    let name = self.parse_name()?;
                    self.skip_ws();
                    self.expect_char('=', &format!("属性 {name} 后面缺少 ="))?;
                    self.skip_ws();
                    let value = self.parse_attr_value(&name)?;
                    // 重复属性名按出现顺序各留一项(浏览器 DOM 会当畸形报错,这里按宽松策略放行)
                    attrs.push((name, value));
                }
                Some(c) => {
                    return Err(self.err_here(format!("元素 <{tag}> 的属性区出现意外字符 {c:?}")));
                }
            }
        }
        Ok((
            XmlNode {
                tag,
                attrs,
                children: Vec::new(),
                text: String::new(),
            },
            false,
        ))
    }

    /// 解析一个属性值(`"…"` 或 `'…'`,两种引号都支持),实体在此解码
    fn parse_attr_value(&mut self, name: &str) -> Result<String, ConvertError> {
        let quote_pos = self.pos;
        let quote = match self.peek() {
            Some(c @ ('"' | '\'')) => {
                self.pos += 1;
                c
            }
            _ => {
                return Err(self.err_at(
                    quote_pos,
                    format!("属性 {name} 的值缺少引号(要用 \" 或 ' 包起来)"),
                ));
            }
        };
        let mut out = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(
                        self.err_at(quote_pos, format!("属性 {name} 的值缺少结束引号 {quote}"))
                    );
                }
                // 结束引号
                Some(c) if c == quote => {
                    self.pos += 1;
                    return Ok(out);
                }
                // 实体解码
                Some('&') => {
                    let amp = self.pos;
                    self.pos += 1;
                    out.push(self.parse_entity(amp)?);
                }
                // 属性值里的裸 '<' 在 XML 里非法(必须写 &lt;),DOMParser 同样报错
                Some('<') => {
                    return Err(self.err_at(self.pos, format!("属性 {name} 的值里不允许出现裸 <")));
                }
                Some(c) => {
                    self.pos += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }

    /// 收一段字符数据直到下一个 `<` 或输入结束;`&…;` 实体在此解码
    fn parse_text(&mut self) -> Result<String, ConvertError> {
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some('<') => return Ok(out),
                Some('&') => {
                    let amp = self.pos;
                    self.pos += 1;
                    out.push(self.parse_entity(amp)?);
                }
                Some(c) => {
                    self.pos += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }

    /// 解码一个实体引用(调用时 `pos` 已在 `&` 之后,`amp_pos` 是 `&` 的位置)。
    ///
    /// 只认 `amp`/`lt`/`gt`/`quot`/`apos` 与数字实体;未知实体直接报错,
    /// 与浏览器 `text/xml` 下的 `DOMParser` 一致(它不认 `&nbsp;`,也不容忍裸 `&`)。
    fn parse_entity(&mut self, amp_pos: usize) -> Result<char, ConvertError> {
        let rest = &self.src[self.pos..];
        let semi = match rest.find(';') {
            Some(i) => i,
            None => return Err(self.err_at(amp_pos, "实体引用缺少结束的 ;")),
        };
        // 真实实体名最长 4 个 ASCII 字符;给数字实体留点余量,
        // 超过就说明是随手写的裸 '&',不必再往后找 ';'
        let body = &rest[..semi];
        if semi > 32 || !body.is_ascii() {
            return Err(self.err_at(amp_pos, format!("非法的实体引用 &{body}…")));
        }
        let ch = match body {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if body.starts_with('#') => match decode_numeric_entity(body) {
                Some(c) => c,
                None => {
                    return Err(self.err_at(amp_pos, format!("非法的数字实体 &{body};")));
                }
            },
            _ => {
                return Err(self.err_at(
                    amp_pos,
                    format!("未知实体引用 &{body};(只认 &amp; &lt; &gt; &quot; &apos; 与数字实体)"),
                ));
            }
        };
        self.pos += semi + 1; // 跳过实体名与 ';'
        Ok(ch)
    }

    /// 跳过注释 `<!-- … -->`(Pascal/Scratch XML 里可能出现;DOM 里是注释节点,我们丢弃)
    fn skip_comment(&mut self) -> Result<(), ConvertError> {
        let start = self.pos;
        self.pos += 4; // "<!--"
        match self.src[self.pos..].find("-->") {
            Some(i) => {
                self.pos += i + 3;
                Ok(())
            }
            None => Err(self.err_at(start, "注释未闭合(缺少 -->)")),
        }
    }

    /// 跳过处理指令 `<? … ?>`(含文档头的 `<?xml version="1.0" encoding="UTF-8"?>`)
    fn skip_pi(&mut self) -> Result<(), ConvertError> {
        let start = self.pos;
        self.pos += 2; // "<?"
        match self.src[self.pos..].find("?>") {
            Some(i) => {
                self.pos += i + 2;
                Ok(())
            }
            None => Err(self.err_at(start, "处理指令未闭合(缺少 ?>)")),
        }
    }

    /// 取 CDATA 段内容(`<![CDATA[ … ]]>`):按原文当文本,**不解码**实体
    fn parse_cdata(&mut self) -> Result<String, ConvertError> {
        let start = self.pos;
        self.pos += 9; // "<![CDATA["
        match self.src[self.pos..].find("]]>") {
            Some(i) => {
                let text = self.src[self.pos..self.pos + i].to_string();
                self.pos += i + 3;
                Ok(text)
            }
            None => Err(self.err_at(start, "CDATA 段未闭合(缺少 ]]>)")),
        }
    }

    /// 跳过 `<!DOCTYPE …>`(含 `[ … ]` 内部子集;内容不做任何解析)
    fn skip_doctype(&mut self) -> Result<(), ConvertError> {
        let start = self.pos;
        self.pos += 9; // "<!DOCTYPE"
        let mut depth = 0usize;
        loop {
            match self.bump() {
                None => return Err(self.err_at(start, "DOCTYPE 声明未闭合(缺少 >)")),
                // 跳过引号里的字符串(内部子集可能写成 SYSTEM "…" 并含 '>')
                Some(q @ ('"' | '\'')) => loop {
                    match self.bump() {
                        None => return Err(self.err_at(start, "DOCTYPE 声明里的引号未闭合")),
                        Some(c) if c == q => break,
                        Some(_) => {}
                    }
                },
                Some('[') => depth += 1,
                Some(']') => depth = depth.saturating_sub(1),
                // 只有内部子集闭合后的 `>` 才是声明结束
                Some('>') if depth == 0 => return Ok(()),
                Some(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod nemo_xml_tests {
    use super::*;

    /// 断言解析失败且是 `Decompile` 错误,返回错误说明(测试里用,非测试代码不 panic)
    fn parse_err(xml: &str) -> String {
        match parse(xml) {
            Ok(node) => panic!("期望解析失败,却解析出了 <{}>", node.tag),
            Err(ConvertError::Decompile(msg)) => msg,
            Err(other) => panic!("期望 Decompile 错误,实际 {other:?}"),
        }
    }

    fn attr_vec(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// 真实形状的 NEMO/Scratch 积木 XML:自闭合、单引号属性、注释、CDATA、
    /// 十进制/十六进制数字实体、`&quot;` 都要能正确解析,且 `parse∘serialize` 是不动点。
    #[test]
    fn parses_and_serializes_realistic_block_xml() {
        let src = r##"<?xml version="1.0" encoding="UTF-8"?>
<!-- NEMO 编辑版积木(Scratch 风格 XML) -->
<block type="procedures_call" id='b1' x="295" y="-133" inline="true" visible="visible">
  <mutation proccode="说 &quot;你好&quot;" argumentids="[]"></mutation>
  <value name="NUM">
    <shadow type="math_number" id="s1">
      <field name="NUM">-1.5</field>
    </shadow>
  </value>
  <field name="TEXT"><![CDATA[a < b & c]]></field>
  <field name="AMP">&#39; &#x2F; &apos; &amp;#39;</field>
  <next>
    <block type="data_setvariableto" id="b2"/>
  </next>
</block>"##;
        let doc = parse(src).expect("真实形状的积木 XML 必须能解析");

        assert_eq!(doc.tag, "block");
        // 属性按出现顺序保存(单引号的 id='b1' 与双引号等价)
        assert_eq!(
            doc.attrs,
            attr_vec(&[
                ("type", "procedures_call"),
                ("id", "b1"),
                ("x", "295"),
                ("y", "-133"),
                ("inline", "true"),
                ("visible", "visible"),
            ]),
            "属性应按出现顺序保存,且两种引号都要能解析"
        );
        // `&quot;` 解码
        assert_eq!(
            doc.child("mutation").and_then(|m| m.attr("proccode")),
            Some("说 \"你好\""),
            "属性值里的 &quot; 应解码成双引号"
        );
        // 自闭合 shadow 有子元素
        assert_eq!(
            doc.child("value")
                .and_then(|v| v.child("shadow"))
                .and_then(|s| s.child("field"))
                .map(|f| f.text_content()),
            Some("-1.5".to_string()),
            "嵌套的 field 文本应能取到"
        );
        // 直接子元素查找:同名多个用 children_of,不存在的 tag 返回 None
        assert_eq!(
            doc.children_of("field").count(),
            2,
            "block 下应有 TEXT/AMP 两个 field"
        );
        assert!(
            doc.child("procedures_definition").is_none(),
            "不存在的子元素应返回 None"
        );
        let text_field = doc.child("field").expect("TEXT 是第一个 field 子元素");
        assert_eq!(text_field.attr("name"), Some("TEXT"));
        assert_eq!(text_field.text, "a < b & c", "CDATA 内容按原文保留");
        // 数字实体:十进制 &#39; 与十六进制 &#x2F;;&amp;#39; 是单遍解码后剩下的字面量
        let amp_field = doc
            .children_of("field")
            .find(|f| f.attr("name") == Some("AMP"))
            .expect("应有 name=AMP 的 field");
        assert_eq!(amp_field.text, "' / ' &#39;", "数字实体与 &apos; 都要解码");
        // 自闭合的空元素:无子元素、无文本
        let next_block = doc
            .child("next")
            .and_then(|n| n.child("block"))
            .expect("next 下有 block");
        assert!(next_block.children.is_empty() && next_block.text.is_empty());
        assert_eq!(next_block.attr("type"), Some("data_setvariableto"));

        // 序列化结果对齐 XMLSerializer 的口径
        let xml_text = doc.serialize();
        for needle in [
            r#"<block type="procedures_call" id="b1" x="295" y="-133" inline="true" visible="visible">"#,
            r#"<mutation proccode="说 &quot;你好&quot;" argumentids="[]"/>"#,
            r#"<field name="TEXT">a &lt; b &amp; c</field>"#,
            r#"<field name="AMP">' / ' &amp;#39;</field>"#,
            r#"<block type="data_setvariableto" id="b2"/>"#,
        ] {
            assert!(
                xml_text.contains(needle),
                "序列化结果应包含 {needle},实际:{xml_text}"
            );
        }

        // 不动点:序列化 → 再解析 → 再序列化,结果必须逐字节一致
        // (元素之间的空白作为父节点文本,统一排在子元素之后,位置有变但稳定)
        let again = parse(&xml_text)
            .expect("自家序列化的结果必须能再解析")
            .serialize();
        assert_eq!(xml_text, again, "parse∘serialize 应稳定(不动点)");

        // 规范形状(子元素在前、子元素之间无文本)可以逐字节往返
        let canonical =
            r#"<block type="math_number" id="s1"><field name="NUM">-1.5</field></block>"#;
        assert_eq!(
            parse(canonical).expect("规范形状可解析").serialize(),
            canonical,
            "规范形状应逐字节往返"
        );
    }

    /// 属性读写:`attr` 缺失返回 `None`、`set_attr` 同名替换保持位置、异名追加到末尾、
    /// `remove_attr` 删除(不存在时无操作)。
    #[test]
    fn attr_read_write() {
        let mut node = XmlNode::new("block");
        assert_eq!(
            node.attr("type"),
            None,
            "未声明的属性应返回 None,而不是空串"
        );

        node.set_attr("type", "math_number");
        node.set_attr("id", "b1");
        node.set_attr("visible", "");
        assert_eq!(
            node.attr("visible"),
            Some(""),
            "声明过的空串属性要与缺失区分开"
        );
        assert_eq!(node.attrs.len(), 3, "三个属性各一项");

        // 同名替换:值更新,位置不动
        node.set_attr("type", "text");
        assert_eq!(
            node.attrs,
            attr_vec(&[("type", "text"), ("id", "b1"), ("visible", "")]),
            "同名替换应保持原位置"
        );

        // 异名追加:排到末尾
        node.set_attr("inline", "true");
        assert_eq!(
            node.attrs
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["type", "id", "visible", "inline"],
            "新属性应追加到末尾"
        );

        node.remove_attr("id");
        assert_eq!(node.attr("id"), None, "删除后应返回 None");
        assert_eq!(
            node.attrs
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["type", "visible", "inline"],
            "删除只影响同名属性"
        );
        node.remove_attr("nonexistent");
        assert_eq!(node.attrs.len(), 3, "删除不存在的属性应无操作");

        // 属性值转义对齐 XMLSerializer:& " < > 都转;单引号不转(统一用双引号包)
        node.set_attr("mode", "a&b\"c<d>e'f");
        let xml = node.serialize();
        assert!(
            xml.contains(r#"mode="a&amp;b&quot;c&lt;d&gt;e'f""#),
            "属性值转义应与 XMLSerializer 一致,实际:{xml}"
        );
        assert_eq!(
            parse(&xml).expect("自家序列化结果应可解析").attr("mode"),
            Some("a&b\"c<d>e'f"),
            "转义必须可逆"
        );

        // 只有空白文本的元素不算空元素,不能自闭合
        assert_eq!(
            parse("<block> </block>").expect("空白文本元素").serialize(),
            "<block> </block>",
            "有空白文本时应保留 <tag> </tag> 而不是自闭合"
        );
    }

    /// 片段解析:整段输入 = 虚拟包装根的内容,返回它的顶层元素;
    /// 包装根自己收到的空白文本**不**聚合进子元素(与官方
    /// `parseFromString("<root>"+blocksXML+"</root>")` 后取 `root.children` 同义)。
    #[test]
    fn parse_fragment_returns_direct_children() {
        let children = parse_fragment(
            r#"<block type="math_number" id="b1"/><value name="A"><shadow type="math_number" id="s1"/></value>"#,
        )
        .expect("片段应能解析");
        assert_eq!(children.len(), 2, "应返回 2 个顶层元素");
        assert_eq!(children[0].tag, "block");
        assert_eq!(children[0].attr("type"), Some("math_number"));
        assert_eq!(children[1].tag, "value");
        assert_eq!(children[1].attr("name"), Some("A"));
        assert_eq!(
            children[1].child("shadow").and_then(|s| s.attr("id")),
            Some("s1"),
            "顶层元素的子树应完整保留"
        );

        // 顶层元素之间的空白归虚拟包装根,不聚合进子元素
        let padded = parse_fragment("\n  <block/>\n  <block/>\n").expect("带空白的片段");
        assert_eq!(padded.len(), 2, "顶层元素仍是 2 个");
        assert_eq!(padded[0].text, "", "空白归虚拟包装根,不归子元素");

        // 顶层多元素是**正常**输入(包装串已经不存在,"唯一根"的要求随之取消)
        assert_eq!(
            parse_fragment("<a/><b/>").expect("多顶层元素").len(),
            2,
            "顶层可以有多个元素"
        );
        // 空片段 = 没有顶层元素(与 `"<root></root>"` 的旧写法一致)
        assert!(parse_fragment("").expect("空片段").is_empty());
    }

    /// **等价性**:新的"虚拟包装根"实现与老的"真套一层 `<root>` 再取子元素"逐字段同结果
    ///
    /// (老写法是 `docs/rounds/39` §W8 去掉的那次整串 `format!`;这条测试就是它的替身,
    /// 防止将来改解析器时把两者悄悄改岔。)
    ///
    /// 只比"成功结果逐字段相同"与"失败侧两边都失败" —— **失败路径按设计有三处不同**
    /// (见 [`parse_fragment`] 的文档:错误位置整体左移 6 字节的 `"<root>"` 前缀;元素未闭合时报
    /// "元素未闭合"而不是被末尾 `</root>` 撞出的开闭不匹配;裸的顶层 `</root>` 报"与虚拟包装根
    /// 提前闭合"而不是"根外非法标记"),文案与坐标都不是协议。
    #[test]
    fn parse_fragment_equals_wrapped_parse() {
        let cases = [
            r#"<block type="a" id="b1"/>"#,
            r#"<block a="1"><value name="V"><shadow type="math_number" id="s"/></value></block>"#,
            "<block/>\n  <block/>",
            "  ",
            "",
            r#"<variables></variables><block type="a"/>"#,
            r#"<block type="a">文本 &amp; 实体</block>"#,
            // 畸形:两边都必须 Err(文案可以不同)
            r#"<block>"#,
            r#"<block></value>"#,
            r#"<block a=1/>"#,
            r#"根之外有文本"#,
            r#"<block>&nbsp;</block>"#,
            // R1②:裸的顶层 `</root>` 两边都必须 Err —— 不许把"虚拟根被提前闭合"当成片段结束,
            // 否则后面的内容会被静默截断(第二条就是那种会截断的输入)
            "</root>",
            "</root><block type=\"a\"/>",
        ];
        for case in cases {
            let mine = parse_fragment(case);
            let reference = parse(&format!("<root>{case}</root>")).map(|root| root.children);
            match (&mine, &reference) {
                (Ok(left), Ok(right)) => {
                    assert_eq!(left, right, "输入 {case:?} 的子元素应逐字段相同")
                }
                (Err(_), Err(_)) => {}
                _ => {
                    panic!("输入 {case:?}:新实现 {mine:?} 与包装版 {reference:?} 的成功/失败不一致")
                }
            }
        }
        // R1② 的两条:新实现必须**显式**报"虚拟包装根提前闭合",不能返回 Ok(那会静默截断)
        for case in ["</root>", "</root><block type=\"a\"/>"] {
            let error = parse_fragment(case).expect_err("裸的顶层 </root> 必须报错");
            assert!(
                format!("{error}").contains("虚拟包装根"),
                "{case:?} 的错误应点明虚拟包装根提前闭合:{error}"
            );
        }
    }

    /// `text_content` 深度优先聚合:`<field name="NUM">1</field>` → `"1"`;
    /// 含子元素的父节点聚合后代文本;空白与语义相关的前后空格不能被 trim。
    #[test]
    fn text_content_aggregates_descendants() {
        let field = parse(r#"<field name="NUM">1</field>"#).expect("field 解析");
        assert_eq!(field.text_content(), "1", "field 的文本就是 NUM 的值");

        let value = parse(
            r#"<value name="A"><shadow type="math_number" id="s1"><field name="NUM">42</field></shadow></value>"#,
        )
        .expect("value 解析");
        assert_eq!(value.text, "", "value 自己没有直接文本");
        assert_eq!(value.text_content(), "42", "没有直接文本时聚合后代文本");

        // 前后空格必须原样保留(field 值可能带空格)
        let spaced = parse(concat!(
            r#"<block>"#,
            r#"<field name="TEXT">  hi  </field>"#,
            r#"<field name="OTHER">x</field>"#,
            r#"</block>"#,
        ))
        .expect("带空格的 field 解析");
        assert_eq!(
            spaced.child("field").map(|f| f.text_content()),
            Some("  hi  ".to_string()),
            "文本不能 trim"
        );
        assert_eq!(spaced.text_content(), "  hi  x", "按文档顺序先序聚合");
    }

    /// 畸形输入一律 `Err(Decompile)`(不 panic、不产出半截结果)。
    #[test]
    fn malformed_input_errors() {
        // 元素未闭合
        let msg = parse_err(r#"<block><field name="NUM">1</field>"#);
        assert!(msg.contains("未闭合"), "应说明未闭合,实际:{msg}");
        // 开闭标签不匹配
        let msg = parse_err("<block></value>");
        assert!(msg.contains("不匹配"), "应说明不匹配,实际:{msg}");
        // 未知实体(与浏览器 text/xml 一致:&nbsp; 不认)
        let msg = parse_err("<block>&nbsp;</block>");
        assert!(msg.contains("未知实体"), "应说明未知实体,实际:{msg}");
        // 属性值里的未知实体
        assert!(
            parse(r#"<block type="a&nbsp;b"/>"#).is_err(),
            "属性值里的未知实体也要报错"
        );
        // 非法数字实体(代理区取不到字符)
        assert!(
            parse("<block>&#xD800;</block>").is_err(),
            "代理区码点应报错"
        );
        assert!(parse("<block>&#x;</block>").is_err(), "空十六进制应报错");
        assert!(parse("<block>&#;</block>").is_err(), "空十进制应报错");
        // 裸 '&'
        assert!(parse("<block>a & b</block>").is_err(), "裸 & 应报错");
        // 属性值缺引号 / 缺结束引号 / 裸 '<'
        assert!(
            parse("<block type=math_number/>").is_err(),
            "属性值缺引号应报错"
        );
        assert!(
            parse(r#"<block type="math_number/>"#).is_err(),
            "属性值缺结束引号应报错"
        );
        assert!(
            parse(r#"<block type="a<b"/>"#).is_err(),
            "属性值里的裸 < 应报错"
        );
        // 属性区中途结束
        assert!(parse("<block type").is_err(), "属性区未结束应报错");
        assert!(parse("<block type=").is_err(), "缺属性值应报错");
        // 自闭合标签没写完
        assert!(parse("<block/").is_err(), "自闭合缺 > 应报错");
        // 没有元素 / 根之外的裸文本 / 多余的结束标签
        assert!(parse("").is_err(), "空文档应报错");
        assert!(parse("   \n  ").is_err(), "只有空白的文档应报错");
        assert!(parse("hello").is_err(), "根元素之外的文本应报错");
        assert!(parse("</block>").is_err(), "无匹配的结束标签应报错");
        assert!(parse("<block/>tail").is_err(), "根元素之后的裸文本应报错");
        // 注释 / PI / CDATA 未闭合
        assert!(
            parse("<block><!-- oops </block>").is_err(),
            "未闭合注释应报错"
        );
        assert!(
            parse("<block><![CDATA[oops</block>").is_err(),
            "未闭合 CDATA 应报错"
        );
        // 顶层多根对 parse 合法(编辑器把 <variables> 与积木并排存),parse 取第一个
        let multi =
            parse("<variables></variables><block type=\"a\"/>").expect("多顶层元素应可解析");
        assert_eq!(multi.tag, "variables");
    }

    /// 深层嵌套必须能处理(几百层),解析/序列化都是迭代实现,不吃调用栈。
    #[test]
    fn deep_nesting_does_not_overflow_stack() {
        const DEPTH: usize = 300;
        let mut src = String::new();
        for _ in 0..DEPTH {
            src.push_str("<block>");
        }
        src.push_str("deep");
        for _ in 0..DEPTH {
            src.push_str("</block>");
        }
        let node = parse(&src).expect("几百层嵌套必须能解析");
        assert_eq!(node.text_content(), "deep", "深层文本应聚合出来");
        let mut depth = 0usize;
        let mut cursor = &node;
        while let Some(next) = cursor.child("block") {
            depth += 1;
            cursor = next;
        }
        assert_eq!(depth, DEPTH - 1, "嵌套层数应完整保留");
        // 深层树的序列化也是迭代实现
        assert_eq!(
            parse(&node.serialize())
                .expect("深层树的序列化结果可再解析")
                .text_content(),
            "deep",
            "序列化-解析应稳定"
        );
        // 超过上限时是 Err 而不是爆栈
        let mut too_deep = String::new();
        for _ in 0..(MAX_DEPTH + 2) {
            too_deep.push_str("<block>");
        }
        assert!(
            parse(&too_deep).is_err(),
            "超过 MAX_DEPTH 层应报错而不是栈溢出"
        );
    }

    /// P0-3 回归:标签里前一个属性值含 `>` 时,标签尾必须按引号感知找。
    /// 旧实现(neko.rs 的 `xml.find('>')`)会把标签截断在引号内的 `>`,导致后面的属性读不到。
    #[test]
    fn xml_attr_value_is_quote_aware_about_tag_end() {
        let xml = r#"<mutation items="2" def_id="a>b" name="x">"#;
        assert_eq!(xml_attr_value(xml, "name"), Some("x"));
        assert_eq!(xml_attr_value(xml, "def_id"), Some("a>b"));
        assert_eq!(xml_attr_value(xml, "items"), Some("2"));
        assert_eq!(xml_attr_value(xml, "missing"), None);
    }
}
