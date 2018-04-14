// External Dependencies ------------------------------------------------------
use log;
use log::{Record, Level, LevelFilter, Metadata, SetLoggerError};
use chrono;


// Logger Implementation ------------------------------------------------------
pub struct Logger;

impl Logger {
    pub fn init() -> Result<(), SetLoggerError> {
        let result = log::set_boxed_logger(Box::new(Logger));
        log::set_max_level(LevelFilter::Info);
        result
    }
}

impl log::Log for Logger {

    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!("[{}] [{}] {}", chrono::Local::now(), record.level(), record.args());
        }
    }

    fn flush(&self) {}

}

