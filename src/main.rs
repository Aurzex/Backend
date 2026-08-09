mod api;
mod core;
mod utils;

use log::{LevelFilter, Log, Metadata, Record};
use serde_json::Value;

use crate::api::auth::{AuthProcessor, LoginHandler, LoginResult};
use crate::core::services::ReportProcessor;
use crate::core::types::value_to_i64;
use crate::core::ui::{ConsoleUi, ProcessorUi, ReportConsole};
use crate::utils::data::PathConfig;

struct ConsoleLogger;

impl Log for ConsoleLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }
    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("[{}] - {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn main() {
    log::set_logger(&ConsoleLogger).unwrap();
    log::set_max_level(LevelFilter::Info);

    let mut ui = ConsoleUi;

    // 1-2. 管理员登录(验证码错误时自动重新获取验证码重试)
    println!("=== 管理员登录 ===");
    let result = match admin_login_with_retry(&mut ui) {
        Ok(r) => r,
        Err(msg) => {
            eprintln!("管理员登录失败: {}", msg);
            return;
        }
    };

    // 3. 打印登录返回的信息
    println!("\n=== 登录返回信息 ===");
    println!("成功: {}", result.success);
    println!("登录方式: {:?}", result.method);
    println!("消息: {}", result.message);
    println!("Token: {}", result.token);
    if !result.data.is_null() {
        println!("数据: {}", pretty(&result.data));
    }
    if let Some(details) = &result.auth_details {
        println!("认证信息: {}", pretty(details));
    }

    // 4. 打印管理员详情
    let admin_details = match AuthProcessor::new().fetch_admin_details() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("获取管理员信息失败: {}", e);
            return;
        }
    };
    println!("\n=== 管理员信息 (fetch_admin_details) ===");
    println!("{}", pretty(&admin_details));

    // 5. 自动提取管理员 ID,无需手动输入
    let admin_id = if let Some(id) = extract_admin_id(&admin_details) {
        id
    } else {
        eprintln!("无法从管理员信息中提取管理员ID, 请检查接口返回");
        return;
    };
    let admin_name = admin_details
        .get("admin")
        .and_then(|a| a.get("full_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("未知");
    println!("管理员: {} (ID {})", admin_name, admin_id);

    let Ok(admin_id) = i32::try_from(admin_id) else {
        eprintln!("管理员ID超出范围");
        return;
    };
    println!("\n=== 启动举报处理控制台 ===");
    let processor = ReportProcessor::new();
    ReportConsole::run(&mut ui, &processor, admin_id);
}

/// 管理员账密登录,验证码错误时自动重新获取验证码并重试
fn admin_login_with_retry(ui: &mut dyn ProcessorUi) -> Result<LoginResult, String> {
    const MAX_ATTEMPTS: usize = 3;
    let mut attempts = 0usize;
    let mut username = String::new();
    let mut password = String::new();

    loop {
        attempts += 1;
        if attempts > 1 {
            println!("验证码错误, 第 {} 次重试 (已重新获取验证码)", attempts);
        }

        let timestamp = match AuthProcessor::new().fetch_admin_captcha() {
            Ok(ts) => ts,
            Err(e) => return Err(format!("获取验证码失败: {}", e)),
        };
        println!(
            "验证码图片已保存: {}",
            PathConfig::global().captcha_file_path().display()
        );

        // 用户名/密码只在首次尝试时询问,重试仅重新输入验证码
        if attempts == 1 {
            username = ui.input("管理员用户名: ");
            password = ui.input("管理员密码: ");
        }
        let captcha = ui.input("请输入验证码: ");

        match LoginHandler::new().handle_admin_password(&username, &password, timestamp, &captcha) {
            Ok(result) => return Ok(result),
            Err(e) => {
                let msg = e.to_string();
                eprintln!("管理员登录失败: {}", msg);
                // 仅验证码类错误重试;密码错误/网络错误等直接终止
                if !msg.contains("验证码") {
                    return Err(msg);
                }
                if attempts >= MAX_ATTEMPTS {
                    let again = ui.input("多次验证码错误, 是否继续重试? (Y/N): ");
                    if !again.trim().eq_ignore_ascii_case("y") {
                        return Err(msg);
                    }
                }
            }
        }
    }
}

/// 从管理员信息中提取管理员 ID,依次尝试 admin.id / id / admin
fn extract_admin_id(details: &Value) -> Option<i64> {
    let paths: [&[&str]; 3] = [&["admin", "id"], &["id"], &["admin"]];
    for path in paths {
        let mut cur = details;
        let mut found = true;
        for &key in path {
            if let Some(v) = cur.get(key) {
                cur = v
            } else {
                found = false;
                break;
            }
        }
        if found && let Some(id) = value_to_i64(cur) {
            return Some(id);
        }
    }
    None
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}
