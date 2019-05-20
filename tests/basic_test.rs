pub mod common;

use crate::common::config::NORMAL_CONFIG;
use crate::common::env::{Content, Env};
use crate::common::utils::{clean_log_file, clean_wal, set_log_file};
use log::LevelFilter;
use std::time::Duration;

#[test]
fn test_basic() {
    let path = "log/test_basic.log";
    clean_wal();
    clean_log_file(path);
    set_log_file(path, LevelFilter::Info);
    let mut env = Env::new(NORMAL_CONFIG, 4, 0);
    env.run(10);
}

#[test]
fn test_restart_nodes() {
    let path = "log/test_restart_nodes.log";
    clean_wal();
    clean_log_file(path);
    set_log_file(path, LevelFilter::Debug);
    let mut env = Env::new(NORMAL_CONFIG, 4, 0);
    // stop node 0, 1 start node 1
    env.set_node(0, Content::Stop, Duration::from_millis(2_000));
    env.set_node(1, Content::Stop, Duration::from_millis(8_000));
    env.set_node(1, Content::Start(1), Duration::from_millis(15_000));
    // stop all nodes, start all nodes
    env.set_node(1, Content::Stop, Duration::from_millis(30_000));
    env.set_node(2, Content::Stop, Duration::from_millis(30_000));
    env.set_node(3, Content::Stop, Duration::from_millis(30_000));
    env.set_node(0, Content::Start(0), Duration::from_millis(32_000));
    env.set_node(1, Content::Start(1), Duration::from_millis(36_000));
    env.set_node(2, Content::Start(2), Duration::from_millis(36_000));
    env.set_node(3, Content::Start(3), Duration::from_millis(40_000));

    env.run(100);
}
