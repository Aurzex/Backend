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

// 举报处理控制台
// 操作流程完全由本层实现,通过调用内部引擎(ReportProcessor)的原语驱动;
// 内部引擎不感知任何交互。

use std::collections::HashMap;
use std::ops::AddAssign;

use crate::core::pipeline::BatchGroup;
use crate::core::registry::{ProcessorError, action_name, resolution_display_name};
use crate::core::retrieve::DataQuery;
use crate::core::services::{ReportItemView, ReportProcessor};

/// 单次处理运行的统计(动作分布);跳过也算一次决策
#[derive(Default, Clone, Copy)]
struct RunStats {
    passed: i64,
    deleted: i64,
    muted7: i64,
    muted3: i64,
    unloaded: i64,
    skipped: i64,
}

impl RunStats {
    fn record(&mut self, action: &str, n: i64) {
        match action {
            "P" => self.passed += n,
            "D" => self.deleted += n,
            "S" => self.muted7 += n,
            "T" => self.muted3 += n,
            "U" => self.unloaded += n,
            _ => {}
        }
    }

    fn total(&self) -> i64 {
        self.passed + self.deleted + self.muted7 + self.muted3 + self.unloaded + self.skipped
    }

    fn summary(&self) -> String {
        format!(
            "共 {} 条: 通过 {} | 删除 {} | 禁言7天 {} | 禁言3月 {} | 取消发布 {} | 跳过 {}",
            self.total(),
            self.passed,
            self.deleted,
            self.muted7,
            self.muted3,
            self.unloaded,
            self.skipped
        )
    }
}

impl AddAssign for RunStats {
    fn add_assign(&mut self, rhs: Self) {
        self.passed += rhs.passed;
        self.deleted += rhs.deleted;
        self.muted7 += rhs.muted7;
        self.muted3 += rhs.muted3;
        self.unloaded += rhs.unloaded;
        self.skipped += rhs.skipped;
    }
}

/// 一个 chunk 内非组项的一次性预计算上下文,避免每条记录重复推断类型/构建详情
struct ChunkContext {
    /// 每条的分组键 (group_type, group_key)
    keys: Vec<Option<(String, String)>>,
    /// 每条是否官方内容(批量应用时排除)
    official: Vec<bool>,
    /// 同源计数:分组键 -> 出现次数
    source_counts: HashMap<String, usize>,
}

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
    /// 控制台主循环:未处理/已处理两个工作区 + 管理员统计,
    /// 支持快捷键直达,总数带缓存,退出时汇报会话统计
    pub fn run(ui: &mut dyn ProcessorUi, processor: &ReportProcessor, admin_id: i32) {
        let mut session = RunStats::default();
        loop {
            let (todo, done) = processor.totals();
            ui.info("\n=== 举报处理控制台 ===");
            ui.info(&format!("未处理: {} 条 | 已处理: {} 条", todo, done));
            ui.info("1(p). 处理未处理举报");
            ui.info("2(d). 查看已处理记录");
            ui.info("3(s). 管理员统计");
            ui.info("0(q). 退出");
            let input = ui.input("> ");
            match input.trim().to_lowercase().as_str() {
                "1" | "p" => Self::process_flow(ui, processor, admin_id, &mut session),
                "2" | "d" => {
                    if Self::view_done(ui, processor, admin_id) {
                        // 已处理记录中直接切到处理未处理
                        Self::process_flow(ui, processor, admin_id, &mut session);
                    }
                }
                "3" | "s" => Self::show_stats(ui),
                "0" | "q" | "" => {
                    if session.total() > 0 {
                        ui.info(&format!("本次会话处理统计: {}", session.summary()));
                    }
                    ui.info("退出举报处理控制台");
                    return;
                }
                _ => ui.info("无效输入,请重试"),
            }
        }
    }

    /// 执行一次"处理未处理"流程并汇报动作分布,累计进会话统计
    fn process_flow(
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
        session: &mut RunStats,
    ) {
        match Self::process_pending(ui, processor, admin_id) {
            Ok(stats) => {
                ui.info(&format!("本次处理 {}", stats.summary()));
                *session += stats;
            }
            Err(e) => ui.error(&format!("处理失败: {}", e)),
        }
    }

    /// 只读视图:各管理员的举报处理量统计
    fn show_stats(ui: &mut dyn ProcessorUi) {
        ui.info("=== 管理员处理统计 ===");
        match DataQuery::new().compute_admin_report_stats() {
            Ok(stats) => {
                for e in &stats.statistics {
                    let pct = if stats.total_all_reports > 0 {
                        f64::from(e.total_reports) * 100.0 / f64::from(stats.total_all_reports)
                    } else {
                        0.0
                    };
                    ui.info(&format!(
                        "  {} (ID {}): 评论 {} | 作品 {} | 合计 {} ({:.1}%)",
                        e.admin_name,
                        e.admin_id,
                        e.comment_reports,
                        e.work_reports,
                        e.total_reports,
                        pct
                    ));
                }
                ui.info(&format!(
                    "  总计: 评论 {} | 作品 {} | 全部 {}",
                    stats.total_comment_reports, stats.total_work_reports, stats.total_all_reports
                ));
            }
            Err(e) => ui.error(&format!("获取统计失败: {}", e)),
        }
    }

    /// 处理待办:分块拉取,批量组与单条依次决策;返回动作分布统计
    fn process_pending(
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
    ) -> Result<RunStats, ProcessorError> {
        let total = processor.pending_total();
        if total == 0 {
            ui.info("没有待处理的举报");
            return Ok(RunStats::default());
        }
        if ui.choose("是否一键全部通过? (Y/N)", &["Y", "N"]) == "Y" {
            let passed = processor.pass_all(admin_id);
            return Ok(RunStats {
                passed,
                ..RunStats::default()
            });
        }

        let mut stats = RunStats::default();
        let outcome = (|| -> Result<(), ProcessorError> {
            let mut session = processor.pending_session();
            while let Some((groups, non_group)) = session.next_chunk() {
                for group in groups {
                    stats += Self::process_group(ui, processor, admin_id, &group)?;
                }
                // 一次性预计算分组键/官方标记/同源计数(避免逐条重复推断与构建详情)
                let ctx = {
                    let keys: Vec<Option<(String, String)>> = non_group
                        .iter()
                        .map(|it| processor.item_group_key(it))
                        .collect();
                    let official: Vec<bool> = non_group
                        .iter()
                        .map(|it| processor.is_official(it))
                        .collect();
                    let mut source_counts: HashMap<String, usize> = HashMap::new();
                    for key in keys.iter().filter_map(|k| k.as_ref().map(|(_, key)| key)) {
                        *source_counts.entry(key.clone()).or_default() += 1;
                    }
                    ChunkContext {
                        keys,
                        official,
                        source_counts,
                    }
                };
                // 已被"同类型批量"应用过的下标不再逐条询问
                let mut decided: HashSet<usize> = HashSet::new();
                for i in 0..non_group.len() {
                    if decided.contains(&i) {
                        continue;
                    }
                    stats += Self::process_item(
                        ui,
                        processor,
                        admin_id,
                        &non_group[i],
                        &non_group,
                        i,
                        &mut decided,
                        stats.total() + 1,
                        total,
                        &ctx,
                    )?;
                }
            }
            for group in session.leftover_groups() {
                stats += Self::process_group(ui, processor, admin_id, &group)?;
            }
            Ok(())
        })();

        match outcome {
            Ok(()) => {
                ui.info("所有举报处理完成");
                Ok(stats)
            }
            Err(ProcessorError::Aborted) => {
                ui.info(&format!("已中止处理, 累计处理 {} 条", stats.total()));
                Ok(stats)
            }
            Err(e) => Err(e),
        }
    }

    /// 处理一个批量组:有保存动作直接应用,否则询问首条后应用到全部
    fn process_group(
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
        group: &BatchGroup,
    ) -> Result<RunStats, ProcessorError> {
        ui.info(&format!(
            "处理批量组 [{}] {} (共 {} 条举报)",
            group.group_type,
            group.group_key,
            group.items.len()
        ));

        if let Some(saved) = processor.group_saved_action(group) {
            ui.info(&format!("应用保存的批量动作: {}", action_name(&saved)));
            let n = processor.apply_group(group, &saved, admin_id);
            let mut st = RunStats::default();
            st.record(&saved, n);
            return Ok(st);
        }

        let Some(first) = group.items.first() else {
            return Ok(RunStats::default());
        };
        let Some(view) = processor.item_view(first) else {
            return Ok(RunStats::default());
        };
        if view.is_official {
            ui.info(&format!(
                "官方内容, 批量自动通过 (举报ID: {})",
                view.record_id
            ));
            processor.save_group_action(group, "P");
            let n = processor.apply_group(group, "P", admin_id);
            let mut st = RunStats::default();
            st.record("P", n);
            return Ok(st);
        }

        ui.info(&format!(
            "--- 批量组首条记录 (举报ID: {}) ---",
            view.record_id
        ));
        for line in &view.details {
            ui.info(line);
        }
        match Self::ask_action(ui, processor, first, &view) {
            ActionChoice::Apply(key) => {
                processor.save_group_action(group, &key);
                ui.info(&format!(
                    "批量组动作: {}, 应用到 {} 条记录",
                    action_name(&key),
                    group.items.len()
                ));
                let n = processor.apply_group(group, &key, admin_id);
                let mut st = RunStats::default();
                st.record(&key, n);
                Ok(st)
            }
            ActionChoice::Skip => {
                for item in &group.items {
                    processor.mark_decided(item);
                }
                ui.info("已跳过该批量组");
                Ok(RunStats {
                    skipped: group.items.len() as i64,
                    ..RunStats::default()
                })
            }
            ActionChoice::Abort => Err(ProcessorError::Aborted),
        }
    }

    /// 处理单条非组内举报;返回该条(含同类型批量)的统计
    #[allow(clippy::too_many_arguments)]
    fn process_item(
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
        admin_id: i32,
        item: &serde_json::Value,
        chunk: &[serde_json::Value],
        idx: usize,
        decided: &mut HashSet<usize>,
        index: i64,
        total: i64,
        ctx: &ChunkContext,
    ) -> Result<RunStats, ProcessorError> {
        // 官方内容直接自动通过:用预计算的标记,不构建详情
        if ctx.official[idx] {
            processor.apply_action(item, "P", admin_id)?;
            let (type_name, record_id) = processor.item_brief(item).unwrap_or_default();
            ui.info(&format!(
                "--- [{}/{}] {} (举报ID: {}) --- 官方内容,自动通过",
                index, total, type_name, record_id
            ));
            return Ok(RunStats {
                passed: 1,
                ..RunStats::default()
            });
        }

        let Some(view) = processor.item_view(item) else {
            return Ok(RunStats::default());
        };
        ui.info(&format!(
            "--- [{}/{}] {} (举报ID: {}) ---",
            index, total, view.type_name, view.record_id
        ));
        // 同源聚合提醒 + 本会话内的历史处理
        if let Some((group_type, key)) = &ctx.keys[idx] {
            if let Some(&n) = ctx.source_counts.get(key).filter(|&&n| n > 1) {
                ui.info(&format!("  [提示] 同源举报 x{}", n));
            }
            if let Some(prev) = processor.saved_action_for_key(group_type, key) {
                ui.info(&format!("  [提示] 本内容此前处理: {}", action_name(&prev)));
            }
        }
        for line in &view.details {
            ui.info(line);
        }
        match Self::ask_action(ui, processor, item, &view) {
            ActionChoice::Apply(key) => {
                match processor.apply_action(item, &key, admin_id) {
                    Ok(()) => {
                        ui.info(&format!("  => 已处理: {}", action_name(&key)));
                        let mut st = RunStats::default();
                        st.record(&key, 1);
                        // 同类型批量应用(本 chunk 内未决策项;官方内容走自动通过,不纳入批量)
                        let rest: Vec<usize> = (idx + 1..chunk.len())
                            .filter(|&j| !decided.contains(&j))
                            .filter(|&j| {
                                ReportProcessor::item_report_type(&chunk[j])
                                    == ReportProcessor::item_report_type(item)
                                    && !ctx.official[j]
                            })
                            .collect();
                        if !rest.is_empty()
                            && ui.choose(
                                &format!("是否对同类型剩余 {} 条应用相同动作? (Y/N)", rest.len()),
                                &["Y", "N"],
                            ) == "Y"
                        {
                            let mut ok = 0i64;
                            for &j in &rest {
                                match processor.apply_action(&chunk[j], &key, admin_id) {
                                    Ok(()) => {
                                        ok += 1;
                                        decided.insert(j);
                                    }
                                    Err(e) => ui.error(&format!("批量应用失败: {}", e)),
                                }
                            }
                            st.record(&key, ok);
                            if ok > 0 {
                                ui.info(&format!("  => 批量应用 {} 条", ok));
                            }
                        }
                        Ok(st)
                    }
                    Err(e) => {
                        // 单条失败仅记录,不中断本会话其余举报(与批量路径语义一致)
                        ui.error(&format!("处理失败: {}", e));
                        Ok(RunStats::default())
                    }
                }
            }
            ActionChoice::Skip => {
                processor.mark_decided(item);
                ui.info("已跳过该举报");
                Ok(RunStats {
                    skipped: 1,
                    ..RunStats::default()
                })
            }
            ActionChoice::Abort => Err(ProcessorError::Aborted),
        }
    }

    /// 弹出动作菜单并处理检查违规(F)/跳过(J);返回最终动作选择
    fn ask_action(
        ui: &mut dyn ProcessorUi,
        processor: &ReportProcessor,
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

    /// 分页浏览已处理记录,支持类型/状态/仅我处理/关键字过滤;
    /// 返回 true 表示用户要求切换到"处理未处理"
    fn view_done(ui: &mut dyn ProcessorUi, processor: &ReportProcessor, admin_id: i32) -> bool {
        const PAGE_SIZE: usize = 15;

        ui.info("=== 已处理记录 ===");
        let type_options = processor.report_type_options();
        let mut filter = DoneFilter::default();
        let mut chunks = processor.done_chunks();
        let mut raw_items: Vec<serde_json::Value> = Vec::new();
        // 匹配当前过滤的条目在 raw_items 中的下标;翻页/详情只走索引,避免每页全量重扫
        let mut visible: Vec<usize> = Vec::new();
        let mut exhausted = false;
        let mut page = 0usize;

        loop {
            // 按需拉取,直到当前页有足够条目或流耗尽
            while visible.len() < (page + 1) * PAGE_SIZE && !exhausted {
                match chunks.next() {
                    Some(chunk) => {
                        let base = raw_items.len();
                        raw_items.extend(chunk);
                        for (i, item) in raw_items[base..].iter().enumerate() {
                            if filter.matches(processor, admin_id, item) {
                                visible.push(base + i);
                            }
                        }
                    }
                    None => exhausted = true,
                }
            }

            if raw_items.is_empty() {
                ui.info("暂无已处理记录");
                return false;
            }

            // 表头:当前过滤条件 + 页码
            let mut parts = vec![format!(
                "类型: {}",
                filter
                    .report_type
                    .as_deref()
                    .and_then(|t| type_options
                        .iter()
                        .find(|(rt, _)| rt == t)
                        .map(|(_, n)| n.as_str()))
                    .unwrap_or("全部")
            )];
            if let Some(s) = &filter.status {
                parts.push(format!("状态: {}", resolution_display_name(s)));
            }
            if filter.mine {
                parts.push("仅我处理".into());
            }
            if let Some(k) = &filter.keyword {
                parts.push(format!("搜索: \"{}\"", k));
            }
            let page_count = visible.len().div_ceil(PAGE_SIZE).max(1);
            ui.info(&format!(
                "\n=== 已处理记录 ({}, 第 {}/{} 页, 共 {} 条{}) ===",
                parts.join(", "),
                page + 1,
                page_count,
                visible.len(),
                if exhausted {
                    ""
                } else {
                    ", 按需加载更多"
                }
            ));

            if !visible.is_empty() {
                let start = page * PAGE_SIZE;
                for (row, &idx) in visible.iter().skip(start).take(PAGE_SIZE).enumerate() {
                    ui.info(&processor.done_row(start + row + 1, &raw_items[idx]));
                }
            } else {
                ui.info("(当前过滤下暂无记录)");
            }

            ui.info("[序号] 查看详情 | n 下一页 | b 上一页 | t 类型 | s 状态 | m 仅我处理 | k 搜索 | x 清除过滤 | u 处理未处理 | q 返回");
            let input = ui.input("> ");
            let trimmed = input.trim();
            if let Ok(idx) = trimmed.parse::<usize>() {
                if visible.is_empty() {
                    ui.info("当前无记录");
                } else if idx >= 1 && idx <= visible.len() {
                    for line in processor.done_item_details(&raw_items[visible[idx - 1]]) {
                        ui.info(&line);
                    }
                } else {
                    ui.info("序号超出范围");
                }
            } else {
                let mut filter_changed = false;
                match trimmed.to_lowercase().as_str() {
                    "n" => {
                        if exhausted && page + 1 >= page_count {
                            ui.info("已经是最后一页");
                        } else {
                            page += 1;
                        }
                    }
                    "b" => {
                        if page > 0 {
                            page -= 1;
                        } else {
                            ui.info("已经是第一页");
                        }
                    }
                    "t" => {
                        filter_changed = true;
                        match Self::pick_type(ui, &type_options, filter.report_type.as_deref()) {
                            TypeFilterChoice::Cancel => {}
                            TypeFilterChoice::All => filter.report_type = None,
                            TypeFilterChoice::Select(rt) => filter.report_type = Some(rt),
                        }
                    }
                    "s" => {
                        filter_changed = true;
                        match Self::pick_status(ui, filter.status.as_deref()) {
                            StatusFilterChoice::Cancel => {}
                            StatusFilterChoice::All => filter.status = None,
                            StatusFilterChoice::Select(s) => filter.status = Some(s),
                        }
                    }
                    "m" => {
                        filter_changed = true;
                        filter.mine = !filter.mine;
                    }
                    "k" => {
                        filter_changed = true;
                        let k = ui.input("输入搜索关键字(回车清除): ");
                        let k = k.trim();
                        filter.keyword = if k.is_empty() {
                            None
                        } else {
                            Some(k.to_lowercase())
                        };
                    }
                    "x" => {
                        filter_changed = true;
                        filter = DoneFilter::default();
                    }
                    "u" => return true,
                    "q" | "" => return false,
                    _ => ui.info("无效输入,请重试"),
                }
                if filter_changed {
                    // 过滤变化后重建可见索引并回到第一页
                    visible.clear();
                    visible.extend(
                        raw_items
                            .iter()
                            .enumerate()
                            .filter(|(_, v)| filter.matches(processor, admin_id, v))
                            .map(|(i, _)| i),
                    );
                    page = 0;
                }
            }
        }
    }

    /// 交互选择要查看的举报类型
    fn pick_type(
        ui: &mut dyn ProcessorUi,
        type_options: &[(String, String)],
        current: Option<&str>,
    ) -> TypeFilterChoice {
        ui.info("\n选择要查看的举报类型:");
        ui.info("  0. 全部");
        for (i, (rt, name)) in type_options.iter().enumerate() {
            let mark = if current == Some(rt.as_str()) {
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
            return TypeFilterChoice::Cancel;
        }
        if trimmed == "0" {
            return TypeFilterChoice::All;
        }
        if let Ok(n) = trimmed.parse::<usize>()
            && n >= 1
            && n <= type_options.len()
        {
            return TypeFilterChoice::Select(type_options[n - 1].0.clone());
        }
        ui.info("无效输入");
        TypeFilterChoice::Cancel
    }

    /// 交互选择状态过滤
    fn pick_status(ui: &mut dyn ProcessorUi, current: Option<&str>) -> StatusFilterChoice {
        ui.info("\n选择状态过滤:");
        ui.info("  0. 全部");
        for (i, raw) in STATUS_FILTER_OPTIONS.iter().enumerate() {
            let mark = if current == Some(raw) {
                " (当前)"
            } else {
                ""
            };
            ui.info(&format!(
                "  {}. {}{}",
                i + 1,
                resolution_display_name(raw),
                mark
            ));
        }
        ui.info("  回车返回");
        let input = ui.input("> ");
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return StatusFilterChoice::Cancel;
        }
        if trimmed == "0" {
            return StatusFilterChoice::All;
        }
        if let Ok(n) = trimmed.parse::<usize>()
            && n >= 1
            && n <= STATUS_FILTER_OPTIONS.len()
        {
            return StatusFilterChoice::Select(STATUS_FILTER_OPTIONS[n - 1].to_string());
        }
        ui.info("无效输入");
        StatusFilterChoice::Cancel
    }
}

/// 类型过滤选择结果
enum TypeFilterChoice {
    /// 取消(保持当前过滤)
    Cancel,
    /// 全部类型
    All,
    /// 选定具体类型
    Select(String),
}

/// 状态过滤选择结果
enum StatusFilterChoice {
    /// 取消(保持当前过滤)
    Cancel,
    /// 全部状态
    All,
    /// 选定具体状态(原始状态值,如 "PASS")
    Select(String),
}

/// 已处理记录状态过滤选项 (原始状态值, 显示名由 resolution_display_name 提供)
const STATUS_FILTER_OPTIONS: [&str; 5] = [
    "PASS",
    "DELETE",
    "MUTE_SEVEN_DAYS",
    "MUTE_THREE_MONTHS",
    "UNLOAD",
];

/// 已处理记录的过滤条件
#[derive(Default)]
struct DoneFilter {
    report_type: Option<String>,
    status: Option<String>,
    mine: bool,
    keyword: Option<String>,
}

impl DoneFilter {
    fn matches(
        &self,
        processor: &ReportProcessor,
        admin_id: i32,
        item: &serde_json::Value,
    ) -> bool {
        if let Some(t) = &self.report_type
            && ReportProcessor::item_report_type(item) != Some(t.as_str())
        {
            return false;
        }
        if let Some(s) = &self.status
            && processor.item_status_str(item).as_deref() != Some(s.as_str())
        {
            return false;
        }
        if self.mine && !processor.item_handled_by(item, admin_id) {
            return false;
        }
        if let Some(k) = &self.keyword
            && !serde_json::to_string(item)
                .unwrap_or_default()
                .to_lowercase()
                .contains(k)
        {
            return false;
        }
        true
    }
}
