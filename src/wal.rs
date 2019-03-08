use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;

/// A function to initialize BFT log config.
pub fn initialize_log_config(log_path: &str, log_level: LevelFilter) -> Config {
    // fileappender config
    let requests = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new("{d} - {m}{n}")))
        .build(log_path)
        .unwrap();
    // log4rs config
    let config = Config::builder()
        .appender(Appender::builder().build("requests", Box::new(requests)))
        .logger(
            Logger::builder()
                .appender("request")
                .additive(false)
                .build("BFT_Log", log_level),
        )
        .build(Root::builder().appender("requests").build(log_level))
        .unwrap();

    config
}
