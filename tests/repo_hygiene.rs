//! 仓库卫生:**阻止凭据入库**。
//!
//! 背景:`docs/rounds/10-ai-chat-cloudvar-test.md` 的真机冒烟记录曾把三组账号密码明文写进文档
//! (2026-09-25 已脱敏);同一份密码更早还出现在 `src/main.rs` / `src/lib.rs` 里。
//! 本测试用**高信号规则**在 `cargo test` 阶段就把这类内容挡住(CI 也会跑本文件)。
//!
//! 判据设计(为什么这样定):
//! - 要求**两个信号同时出现** —— 一个"账号样"(手机号/邮箱)与一个"口令样"token,
//!   或 markdown 表格里出现**带符号**的口令样 token(真机账号表最典型的形态);
//! - 口令样 = 长度 ≥10、同时有大小写字母与数字、且含 `.`/`!`/`#`/`$`/`%`/`&`/`*` 之一
//!   (真实口令常见 `CODExhr1106.mao` 这种形态;而 `Kitten4Frontend`、`base64-STANDARD`
//!   这类标识符/常量没有这些符号,不会误伤);
//! - 账号样旁边只要求"弱口令样"(≥8 字符、字母+数字、无 `.`)——覆盖 `miao520mitao` 这类;
//! - 命中时输出 `文件:行: 规则: 片段`,口令只打印首尾各 2 字符(**不把疑似口令再写进日志**);
//! - 豁免:同一行含 `hygiene-allow` 标记则跳过(用于 README 里"格式示例"这类必须写 `password` 的行);
//! - `data/` 是凭据的**正确**存放处(`.gitignore` 已忽略),扫描时跳过。
//!
//! 已知取舍:宁可漏报也不误伤正常文档 —— 误报会让人加豁免标记成习惯,反而削弱防线。

use std::path::{Path, PathBuf};

/// 扫描根(相对仓库根)
const SCAN_DIRS: &[&str] = &["docs", "src", "tests"];
/// 仓库根的散文件
const SCAN_ROOT_FILES: &[&str] = &["README.md", "CONTRIBUTING.md", "Cargo.toml"];
/// 不扫的目录/文件(`data/` 存真实凭据且已 gitignore;其余是产物/工作区)
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "data",
    "cache",
    "download",
    "temp",
    ".reasonix",
];
/// 只看这些扩展名
const SCAN_EXTS: &[&str] = &["rs", "md", "json", "toml", "yml", "yaml"];
/// 行内豁免标记
const ALLOW_MARKER: &str = "hygiene-allow";
/// 口令允许出现的符号(真实口令常用;标识符/常量一般不含)
const SECRET_SYMBOLS: &[char] = &['.', '!', '#', '$', '%', '&', '*'];

/// 口令样:≥10 字符、大小写字母 + 数字齐全、且含 [`SECRET_SYMBOLS`] 之一
fn looks_like_password(token: &str) -> bool {
    let len = token.chars().count();
    // 十六进制/范围字面量(如 `0x00..0x1E`)是协议常量,不是口令
    if token.starts_with("0x") || token.contains("..") {
        return false;
    }
    (10..=64).contains(&len)
        && token.chars().all(|c| {
            c.is_ascii_alphanumeric() || SECRET_SYMBOLS.contains(&c) || c == '_' || c == '-'
        })
        && token.chars().any(|c| c.is_ascii_lowercase())
        && token.chars().any(|c| c.is_ascii_uppercase())
        && token.chars().any(|c| c.is_ascii_digit())
        && token.contains(SECRET_SYMBOLS)
}

/// 弱口令样:≥8 字符、字母 + 数字、不含 `.` —— 用于"账号旁边那串东西"的兜底信号
fn looks_like_weak_secret(token: &str) -> bool {
    let len = token.chars().count();
    // 含 `_`/`-` 的多半是标识符或协议常量(`password_v0`/`block_data_json`),不当口令看
    if token.contains(['_', '-']) {
        return false;
    }
    (8..=64).contains(&len)
        && token.chars().all(|c| c.is_ascii_alphanumeric())
        && token.chars().any(|c| c.is_ascii_alphabetic())
        && token.chars().any(|c| c.is_ascii_digit())
}

/// 中国大陆手机号(11 位)
fn is_cn_mobile(token: &str) -> bool {
    token.len() == 11
        && token.starts_with('1')
        && token.chars().all(|c| c.is_ascii_digit())
        && matches!(token.as_bytes()[1], b'3'..=b'9')
}

fn is_email(token: &str) -> bool {
    token.contains('@') && token.contains('.') && token.len() >= 6
}

/// 把一行拆成候选 token(去掉 markdown 竖线、引号、括号、标点等)
fn tokens(line: &str) -> Vec<String> {
    line.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '|' | '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '`'
            )
    })
    .map(|s| s.trim_matches(|c: char| matches!(c, ':' | '=' | '*' | '#')))
    .filter(|s| !s.is_empty())
    .map(str::to_string)
    .collect()
}

/// `convert_bench` 的样本键(`kitten4-10.8MB` / `kn-9.4MB` …)会被
/// `looks_like_password` 误判成口令 —— 它们是仓库自己的基准标签,文档里反复出现,
/// 明确放行(比在每张表上打 `hygiene-allow` 标记更不容易漏)。
fn is_bench_sample_label(token: &str) -> bool {
    let rest = match token.split_once('-') {
        Some(("kitten4", rest)) | Some(("kn", rest)) => rest,
        _ => return false,
    };
    let Some(size) = rest.strip_suffix("MB") else {
        return false;
    };
    !size.is_empty() && size.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// 只打印首尾各 2 字符,避免把疑似口令原样写进测试输出/CI 日志
fn masked(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 4 {
        return "****".to_string();
    }
    format!(
        "{}…{}",
        chars[..2].iter().collect::<String>(),
        chars[chars.len() - 2..].iter().collect::<String>()
    )
}

/// 检查一段文本,返回发现(空 = 干净)
///
/// 规则:
/// - `account+secret`:同一行既有手机号/邮箱,又有口令样或弱口令样 token;
/// - `table-secret`:markdown 表格行(以 `|` 开头)里出现口令样 token;
/// - `assigned-secret`:`password|passwd|pwd|secret` 后跟看起来是真口令的字面量。
pub fn findings_in(path: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.contains(ALLOW_MARKER) {
            continue;
        }
        let line_no = index + 1;
        let toks = tokens(line);
        let secret = toks
            .iter()
            .find(|t| looks_like_password(t))
            .or_else(|| toks.iter().find(|t| looks_like_weak_secret(t)));
        let account = toks.iter().find(|t| is_cn_mobile(t) || is_email(t));

        if let (Some(account), Some(secret)) = (account, secret) {
            out.push(format!(
                "{path}:{line_no}: account+secret: 账号 {account} 旁疑似口令 {}",
                masked(secret)
            ));
            continue;
        }
        if line.trim_start().starts_with('|')
            && let Some(secret) = toks
                .iter()
                .find(|t| looks_like_password(t) && !is_bench_sample_label(t))
        {
            out.push(format!(
                "{path}:{line_no}: table-secret: 表格里疑似口令 {}",
                masked(secret)
            ));
        }
    }

    // 赋值形态(不要求同行有账号)
    for (index, line) in text.lines().enumerate() {
        if line.contains(ALLOW_MARKER) {
            continue;
        }
        let lowered = line.to_ascii_lowercase();
        let Some(keyword) = ["password", "passwd", "pwd", "secret"]
            .into_iter()
            .find(|k| lowered.contains(k))
        else {
            continue;
        };
        let Some(start) = line.find('"') else {
            continue;
        };
        let Some(end) = line[start + 1..].find('"').map(|i| start + 1 + i) else {
            continue;
        };
        let value = &line[start + 1..end];
        let placeholder = [
            "replace",
            "your",
            "example",
            "xxx",
            "student-pass",
            "password1",
            "<",
            ">",
        ]
        .iter()
        .any(|p| value.to_ascii_lowercase().contains(p));
        if placeholder || !(looks_like_password(value) || looks_like_weak_secret(value)) {
            continue;
        }
        out.push(format!(
            "{path}:{}: assigned-secret: 疑似把口令写成字面量({keyword})",
            index + 1
        ));
    }
    out
}

fn collect_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect_files(&path, out);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if SCAN_EXTS.contains(&ext) {
            out.push(path);
        }
    }
}

#[test]
fn no_credentials_in_tracked_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in SCAN_DIRS {
        collect_files(&root.join(dir), &mut files);
    }
    for name in SCAN_ROOT_FILES {
        let path = root.join(name);
        if path.exists() {
            files.push(path);
        }
    }
    assert!(!files.is_empty(), "扫描没找到任何文件,规则形同虚设");

    let mut findings = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        // 本文件自身含**编造**的"像口令"字面量(供下面的规则自测),跳过
        if rel == "tests/repo_hygiene.rs" {
            continue;
        }
        findings.extend(findings_in(&rel, &text));
    }
    assert!(
        findings.is_empty(),
        "疑似凭据入库(真实凭据只放 data/test-config.json;若确实是示例,在该行加 `{ALLOW_MARKER}` 标记):\n{}",
        findings.join("\n")
    );
}

#[test]
fn scanner_catches_realistic_leaks_and_ignores_examples() {
    // 真机冒烟记录里的三种形态(口令值为**编造**的,只保留形状)
    let leaked_rows = "| Aurzex | NotReal1106.mao | 普通用户 | ✅ |\n\
                       | 13900000001 | NotRealBlack114514 | 普通用户 | ✅ |\n\
                       | 13900000002 | miao520mitao | 普通用户 | ✅ |";
    assert_eq!(
        findings_in("docs/x.md", leaked_rows).len(),
        3,
        "三行账号表都应被抓住"
    );

    let leaked_code = r#"let password = "NotReal1106.mao";"#;
    assert_eq!(
        findings_in("src/x.rs", leaked_code).len(),
        1,
        "硬编码口令应被抓住"
    );

    // 正常内容不能误报(标识符、常量、文件名、占位符、格式示例)
    for (path, text) in [
        ("README.md", r#"    .password("student-pass")"#),
        ("src/x.rs", "let kind = Kitten4Frontend::new();"),
        ("src/x.rs", r#"let algo = "base64-STANDARD";"#),
        (
            "docs/x.md",
            "| `mapping.rs` | `Kitten4Backend` 重建 `connections` |",
        ),
        ("src/x.rs", r#"let token = "REPLACE_ME";"#),
        (
            "src/api/auth.rs",
            r#"LoginMethod::PasswordV0 => "password_v0","#,
        ),
        (
            "docs/x.md",
            "| `AES-256-GCM`;key = `SHA256(salt)`,`salt = 0x00..0x1E` |",
        ),
        ("docs/x.md", "密码: 见 `data/test-config.json`"),
        ("README.md", "| `data/password.txt` | 每行 `用户名:密码` |"),
    ] {
        let found = findings_in(path, text);
        assert!(found.is_empty(), "{path} 误报: {found:?} :: {text}");
    }

    // 豁免标记能压掉命中
    let allowed = format!("| Aurzex | NotReal1106.mao | {ALLOW_MARKER} |");
    assert!(findings_in("docs/x.md", &allowed).is_empty());
}
