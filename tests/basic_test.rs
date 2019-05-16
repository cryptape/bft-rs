pub mod common;

use common::env::Env;
use common::utils::clean_wal;

use crate::common::utils::{clean_log_file, set_log_file};
use log::LevelFilter;

#[test]
fn test_basic() {
    clean_wal();
    clean_log_file("log/test_basic.log");
    set_log_file("log/test_basic.log", LevelFilter::Info);
    let mut env = Env::new(4, 0);
    env.run(1000);
}
