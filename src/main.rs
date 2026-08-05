mod api;
mod core;
mod utils;

use crate::core::compiler::decompile_work;
use log::{LevelFilter, Log, Metadata, Record, info};

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
    for work_id in [317683843] {
        match decompile_work(work_id, None) {
            Ok(path) => println!("[OK] work_id={} -> {}", work_id, path),
            Err(e) => eprintln!("[ERR] work_id={} -> {:?}", work_id, e),
        }
    }
}
