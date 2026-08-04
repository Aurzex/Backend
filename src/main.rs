mod api;
mod core;
mod utils;

use crate::core::compiler::decompile_work;
use log::{LevelFilter, Log, Metadata, Record, info};

struct ConsoleLogger; // 改这里

impl Log for ConsoleLogger {
    // 改这里
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
    log::set_logger(&ConsoleLogger).unwrap(); // 改这里
    log::set_max_level(LevelFilter::Info);
    for work_id in [215246857i64, 301113412] {
        match decompile_work(work_id, None) {
            Ok(path) => println!("[OK] work_id={} -> {}", work_id, path),
            Err(e) => eprintln!("[ERR] work_id={} -> {:?}", work_id, e),
        }
    }
    println!("1")
}
