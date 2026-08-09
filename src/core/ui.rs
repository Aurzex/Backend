//! 举报处理的外部交互界面
//!
//! 内部处理逻辑(services/pipeline/types)不传递 ui 参数,而是通过调用本模块的
//! 自由函数(`ui::info` / `ui::input` / `ui::menu` 等)完成交互;
//! 公开入口(如 `ReportProcessor::run_interactive`)先调用 [`with_ui`] 安装界面,
//! 作用域内的所有交互调用自动路由到该界面。
//! 处理过程中的运行日志统一走 `log` crate。

use std::cell::Cell;
use std::collections::HashSet;
use std::io::{self, Write};
use std::ptr;

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

// 举报处理控制台
// 操作流程完全由本层实现,通过调用内部引擎(ReportProcessor)的原语驱动;
// 内部引擎不感知任何交互。

use crate::core::pipeline::BatchGroup;
use crate::core::services::{PendingSession, ReportItemView, ReportProcessor};
use crate::core::types::{ProcessorError, action_name};

/// 动作菜单的最终选择
enum ActionChoice {
    /// 应用指定动作
    Apply(String),
    /// 跳过
    Skip,
    /// 用户中止
    Abort,
}

/// 举报处理控制台:实现"处理待办/查看已处理/分布"等操作流程
pub struct ReportConsole;

impl ReportConsole {
    /// 控制台主循环
    pub fn run(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
    ) -> Result<(), ProcessorError> {
        loop {
            let todo = processor.pending_total();
            let done = processor.done_total();
            ui.info("\n=== 举报处理控制台 ===");
            ui.info(&format!("待处理: {} 条 | 已处理: {} 条", todo, done));
            ui.info("1. 处理待处理举报");
            ui.info("2. 查看已处理记录");
            ui.info("3. 待处理分布(按类型)");
            ui.info("0. 退出");
            let input = ui.input("> ");
            match input.trim() {
                "1" => match self.process_pending(ui, processor, admin_id) {
                    Ok(n) => ui.info(&format!("本次共处理 {} 条举报", n)),
                    Err(ProcessorError::Aborted) => ui.info("已中止处理"),
                    Err(e) => ui.error(&format!("处理失败: {}", e)),
                },
                "2" => {
                    if let Err(e) = self.view_done(ui, processor) {
                        ui.error(&format!("查看已处理记录失败: {}", e));
                    }
                }
                "3" => {
                    if let Err(e) = self.backlog(ui, processor) {
                        ui.error(&format!("获取分布失败: {}", e));
                    }
                }
                "0" | "q" | "Q" | "" => {
                    ui.info("退出举报处理控制台");
                    return Ok(());
                }
                _ => ui.info("无效输入,请重试"),
            }
        }
    }

    /// 处理待办:分块拉取,批量组与单条依次决策
    fn process_pending(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
    ) -> Result<i64, ProcessorError> {
        let total = processor.pending_total();
        if total == 0 {
            ui.info("没有待处理的举报");
            return Ok(0);
        }
        if ui.choose("是否一键全部通过? (Y/N)", &["Y", "N"]) == "Y" {
            return processor.pass_all(admin_id);
        }

        let mut processed = 0i64;
        let outcome = (|| -> Result<(), ProcessorError> {
            let mut session = processor.pending_session();
            while let Some((groups, non_group)) = session.next_chunk() {
                for group in groups {
                    processed += self.process_group(ui, processor, admin_id, &group)?;
                }
                for item in &non_group {
                    processed +=
                        self.process_item(ui, processor, admin_id, item, processed + 1, total)?;
                }
            }
            for group in session.leftover_groups() {
                processed += self.process_group(ui, processor, admin_id, &group)?;
            }
            Ok(())
        })();

        match outcome {
            Ok(()) => {
                ui.info(&format!("所有举报处理完成, 共处理 {} 条举报", processed));
                Ok(processed)
            }
            Err(ProcessorError::Aborted) => {
                ui.info(&format!("已中止处理, 累计处理 {} 条", processed));
                Ok(processed)
            }
            Err(e) => Err(e),
        }
    }

    /// 处理一个批量组:有保存动作直接应用,否则询问首条后应用到全部
    fn process_group(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
        group: &BatchGroup,
    ) -> Result<i64, ProcessorError> {
        ui.info(&format!(
            "处理批量组 [{}] {} (共 {} 条举报)",
            group.group_type,
            group.group_key,
            group.items.len()
        ));

        if let Some(saved) = processor.group_saved_action(group) {
            ui.info(&format!("应用保存的批量动作: {}", action_name(&saved)));
            return processor.apply_group(group, &saved, admin_id);
        }

        let Some(first) = group.items.first() else {
            return Ok(0);
        };
        let Some(view) = processor.item_view(first) else {
            return Ok(0);
        };
        if view.is_official {
            ui.info(&format!(
                "官方内容, 批量自动通过 (举报ID: {})",
                view.record_id
            ));
            processor.save_group_action(group, "P");
            return processor.apply_group(group, "P", admin_id);
        }

        ui.info(&format!(
            "--- 批量组首条记录 (举报ID: {}) ---",
            view.record_id
        ));
        for line in &view.details {
            ui.info(line);
        }
        let action = match self.ask_action(ui, processor, admin_id, first, &view) {
            ActionChoice::Apply(key) => key,
            ActionChoice::Skip => {
                ui.info("已跳过该批量组");
                return Ok(0);
            }
            ActionChoice::Abort => return Err(ProcessorError::Aborted),
        };
        processor.save_group_action(group, &action);
        ui.info(&format!(
            "批量组动作: {}, 应用到 {} 条记录",
            action_name(&action),
            group.items.len()
        ));
        processor.apply_group(group, &action, admin_id)
    }

    /// 处理单条非组内举报,返回是否已处理
    fn process_item(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
        item: &serde_json::Value,
        index: i64,
        total: i64,
    ) -> Result<i64, ProcessorError> {
        let Some(view) = processor.item_view(item) else {
            return Ok(0);
        };
        if view.is_official {
            processor.apply_action(item, "P", admin_id)?;
            ui.info(&format!(
                "--- [{}/{}] {} (举报ID: {}) --- 官方内容,自动通过",
                index, total, view.type_name, view.record_id
            ));
            return Ok(1);
        }

        ui.info(&format!(
            "--- [{}/{}] {} (举报ID: {}) ---",
            index, total, view.type_name, view.record_id
        ));
        for line in &view.details {
            ui.info(line);
        }
        match self.ask_action(ui, processor, admin_id, item, &view) {
            ActionChoice::Apply(key) => {
                processor.apply_action(item, &key, admin_id)?;
                ui.info(&format!("  => 已处理: {}", action_name(&key)));
                Ok(1)
            }
            ActionChoice::Skip => {
                ui.info("已跳过该举报");
                Ok(1)
            }
            ActionChoice::Abort => Err(ProcessorError::Aborted),
        }
    }

    /// 弹出动作菜单并处理检查违规(F)/跳过(J);返回最终动作选择
    fn ask_action(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
        item: &serde_json::Value,
        view: &ReportItemView,
    ) -> ActionChoice {
        let options: Vec<(&str, &str)> = view
            .actions
            .iter()
            .map(|(k, n)| (k.as_str(), n.as_str()))
            .collect();
        let default_idx = view.actions.iter().position(|(k, _)| k == "P");

        loop {
            let Some(idx) = ui.menu("请选择操作:", &options, default_idx) else {
                return ActionChoice::Abort;
            };
            let key = &view.actions[idx].0;
            match key.as_str() {
                "F" => {
                    if !processor.supports_violation_check(item) {
                        ui.info("该类型不支持检查违规操作");
                        continue;
                    }
                    let limit = ui
                        .input("输入要获取的评论数: ")
                        .parse()
                        .unwrap_or_else(|_| processor.default_comment_limit());
                    match processor.check_violations(item, limit) {
                        Ok(violations) if violations.is_empty() => {
                            ui.info("未检测到违规内容");
                        }
                        Ok(violations) => {
                            ui.info(&format!("检测到 {} 条违规内容", violations.len()));
                            if ui.choose("是否自动举报违规内容? (Y/N)", &["Y", "N"]) == "Y"
                            {
                                match processor.auto_report(&violations) {
                                    Ok(n) => ui.info(&format!(
                                        "自动举报完成, 成功 {}/{}",
                                        n,
                                        violations.len()
                                    )),
                                    Err(e) => ui.error(&format!("自动举报失败: {}", e)),
                                }
                            }
                        }
                        Err(e) => ui.error(&format!("违规检查失败: {}", e)),
                    }
                }
                "J" => return ActionChoice::Skip,
                key => return ActionChoice::Apply(key.to_string()),
            }
        }
    }

    /// 分页浏览已处理记录,支持按类型过滤切换
    fn view_done(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
    ) -> Result<(), ProcessorError> {
        ui.info("=== 已处理记录 ===");
        let type_options = processor.report_type_options();
        let mut current_type: Option<String> = None; // None = 全部
        let mut chunks = processor.done_chunks();
        let mut raw_items: Vec<serde_json::Value> = Vec::new();
        let mut visible_count = 0usize;
        let mut exhausted = false;

        const PAGE_SIZE: usize = 15;
        let mut page = 0usize;

        loop {
            let needed = (page + 1) * PAGE_SIZE;
            while visible_count < needed && !exhausted {
                match chunks.next() {
                    Some(chunk) => {
                        visible_count += chunk
                            .iter()
                            .filter(|v| self.type_matches(processor, v, &current_type))
                            .count();
                        raw_items.extend(chunk);
                    }
                    None => exhausted = true,
                }
            }

            if raw_items.is_empty() {
                ui.info("暂无已处理记录");
                return Ok(());
            }

            let filter_name = current_type
                .as_deref()
                .and_then(|t| {
                    type_options
                        .iter()
                        .find(|(rt, _)| rt == t)
                        .map(|(_, n)| n.as_str())
                })
                .unwrap_or("全部");
            let page_count = visible_count.div_ceil(PAGE_SIZE).max(1);
            ui.info(&format!(
                "\n=== 已处理记录 (类型: {}, 第 {}/{} 页, 共 {} 条{}) ===",
                filter_name,
                page + 1,
                page_count,
                visible_count,
                if exhausted {
                    ""
                } else {
                    ", 按需加载更多"
                }
            ));

            if visible_count > 0 {
                let start = page * PAGE_SIZE;
                for (i, item) in raw_items
                    .iter()
                    .filter(|v| self.type_matches(processor, v, &current_type))
                    .skip(start)
                    .take(PAGE_SIZE)
                    .enumerate()
                {
                    ui.info(&processor.done_row(start + i + 1, item));
                }
            } else {
                ui.info("(该类型暂无已处理记录)");
            }

            ui.info("[序号] 查看详情 | n 下一页 | p 上一页 | t 切换类型 | q 返回");
            let input = ui.input("> ");
            let trimmed = input.trim();
            if let Ok(idx) = trimmed.parse::<usize>() {
                if visible_count == 0 {
                    ui.info("当前无记录");
                } else if idx >= 1 && idx <= visible_count {
                    if let Some(item) = raw_items
                        .iter()
                        .filter(|v| self.type_matches(processor, v, &current_type))
                        .nth(idx - 1)
                    {
                        for line in processor.done_item_details(item) {
                            ui.info(&line);
                        }
                    }
                } else {
                    ui.info("序号超出范围");
                }
            } else {
                match trimmed.to_lowercase().as_str() {
                    "n" => {
                        if exhausted && page + 1 >= page_count {
                            ui.info("已经是最后一页");
                        } else {
                            page += 1;
                        }
                    }
                    "p" => {
                        if page > 0 {
                            page -= 1;
                        } else {
                            ui.info("已经是第一页");
                        }
                    }
                    "t" => {
                        if let Some(new_type) = self.pick_type(ui, &type_options, &current_type) {
                            current_type = new_type;
                            visible_count = raw_items
                                .iter()
                                .filter(|v| self.type_matches(processor, v, &current_type))
                                .count();
                            page = 0;
                        }
                    }
                    "q" | "" => break,
                    _ => ui.info("无效输入,请重试"),
                }
            }
        }
        Ok(())
    }

    /// 记录是否匹配当前类型过滤
    fn type_matches(
        &self,
        processor: &ReportProcessor,
        item: &serde_json::Value,
        current: &Option<String>,
    ) -> bool {
        match current {
            Some(t) => processor.item_report_type(item) == Some(t.as_str()),
            None => true,
        }
    }

    /// 交互选择要查看的举报类型,None 表示取消,Some(None) 表示全部
    fn pick_type(
        &self,
        ui: &mut dyn ProcessorUi,
        type_options: &[(String, String)],
        current: &Option<String>,
    ) -> Option<Option<String>> {
        ui.info("\n选择要查看的举报类型:");
        ui.info("  0. 全部");
        for (i, (rt, name)) in type_options.iter().enumerate() {
            let mark = if current.as_ref() == Some(rt) {
                " (当前)"
            } else {
                ""
            };
            ui.info(&format!("  {}. {}{}", i + 1, name, mark));
        }
        ui.info("  回车返回");
        let input = ui.input("> ");
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed == "0" {
            return Some(None);
        }
        if let Ok(n) = trimmed.parse::<usize>()
            && n >= 1
            && n <= type_options.len()
        {
            return Some(Some(type_options[n - 1].0.clone()));
        }
        ui.info("无效输入");
        None
    }

    /// 展示各举报类型的待处理数量
    fn backlog(
        &self,
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
    ) -> Result<(), ProcessorError> {
        ui.info("=== 待处理举报分布 ===");
        let items = processor.backlog();
        let mut total = 0i64;
        for (name, count) in items {
            ui.info(&format!("  {}: {}", name, count));
            total += count;
        }
        ui.info(&format!("  合计: {}", total));
        Ok(())
    }
}
