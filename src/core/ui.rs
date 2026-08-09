//! 举报处理的外部交互界面
//!
//! 内部处理逻辑(services/pipeline/types)通过 [`ProcessorUi`] trait 与外部交互,
//! 不直接读写控制台;控制台实现 [`ConsoleUi`] 供二进制与外部驱动使用。
//! 处理过程中的运行日志统一走 `log` crate。

use std::collections::HashSet;
use std::io::{self, Write};

/// 交互界面:内部逻辑依赖该 trait 完成输入/选择/展示
pub trait ProcessorUi {
    /// 输出一行普通信息(标题,列表行等)
    fn info(&mut self, msg: &str);
    /// 输出一行错误信息
    fn error(&mut self, msg: &str);
    /// 读取一行输入
    fn input(&mut self, prompt: &str) -> String;
    /// 在合法选项中循环选择(大小写不敏感),返回大写的合法键
    fn choose(&mut self, prompt: &str, valid: &[&str]) -> String;
    /// 编号菜单:列出 `options`(编号,键,名称),`default_idx` 为回车默认项
    /// 返回选中项索引,None 表示用户取消/退出
    fn menu(
        &mut self,
        title: &str,
        options: &[(&str, &str)],
        default_idx: Option<usize>,
    ) -> Option<usize>;
}

/// 控制台实现:直接读写 stdin/stdout
#[derive(Default)]
pub struct ConsoleUi;

impl ProcessorUi for ConsoleUi {
    fn info(&mut self, msg: &str) {
        println!("{}", msg);
    }

    fn error(&mut self, msg: &str) {
        eprintln!("{}", msg);
    }

    fn input(&mut self, prompt: &str) -> String {
        read_line(prompt)
    }

    fn choose(&mut self, prompt: &str, valid: &[&str]) -> String {
        let valid_set: HashSet<&str> = valid.iter().copied().collect();
        loop {
            let input = read_line(prompt);
            let upper = input.trim().to_uppercase();
            if valid_set.contains(upper.as_str()) {
                return upper;
            }
            println!("无效输入,请重试");
        }
    }

    fn menu(
        &mut self,
        title: &str,
        options: &[(&str, &str)],
        default_idx: Option<usize>,
    ) -> Option<usize> {
        println!("{}", title);
        for (i, (key, name)) in options.iter().enumerate() {
            let default_mark = if default_idx == Some(i) {
                " [回车默认]"
            } else {
                ""
            };
            println!("  {}. {}{} ({})", i + 1, name, default_mark, key);
        }
        println!("  0. 取消 (Q)");
        loop {
            let input = read_line("> ");
            let trimmed = input.trim();
            if trimmed.is_empty() {
                if let Some(idx) = default_idx {
                    return Some(idx);
                }
                println!("无效输入,请重试");
                continue;
            }
            if trimmed == "0" || trimmed.eq_ignore_ascii_case("q") {
                return None;
            }
            if let Ok(n) = trimmed.parse::<usize>() {
                if n >= 1 && n <= options.len() {
                    return Some(n - 1);
                }
                println!("无效输入,请重试");
                continue;
            }
            if let Some(idx) = options
                .iter()
                .position(|(key, _)| key.eq_ignore_ascii_case(trimmed))
            {
                return Some(idx);
            }
            println!("无效输入,请重试");
        }
    }
}

/// 从 stdin 读取一行并去除首尾空白
fn read_line(prompt: &str) -> String {
    print!("{}", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}
