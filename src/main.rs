mod api;
mod core;
mod utils;

use log::{LevelFilter, Log, Metadata, Record};

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
}
