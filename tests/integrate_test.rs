pub mod common;

use crate::common::config::{BAD_CONFIG, NORMAL_CONFIG, PERFECT_CONFIG};
use crate::common::env::{Content, Env};
use crate::common::utils::{
    clean_log_file, clean_wal, get_random_integer, set_log_file, RandomMode,
};
use log::{info, LevelFilter};
use std::collections::HashMap;
use std::time::Duration;

#[test]
fn test_basic() {
    let path = "log/test_basic.log";
    let wal_dir = "wal/test_basic/wal";
    clean_wal(wal_dir);
    clean_log_file(path);
    set_log_file(path, LevelFilter::Debug);
    let mut env = Env::new(PERFECT_CONFIG, 4, wal_dir);
    env.run(3);
}

#[test]
fn test_restart() {
    let path = "log/test_restart.log";
    let wal_dir = "wal/test_restart/wal";
    clean_wal(wal_dir);
    clean_log_file(path);
    set_log_file(path, LevelFilter::Debug);
    let mut env = Env::new(NORMAL_CONFIG, 4,  wal_dir);

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

#[test]
fn test_wild() {
    let path = "log/test_wild.log";
    let wal_dir = "wal/test_wild/wal";
    clean_wal(wal_dir);
    clean_log_file(path);
    set_log_file(path, LevelFilter::Debug);
    let mut env = Env::new(BAD_CONFIG, 4,  wal_dir);

    let mut stat = HashMap::new();
    for i in 0..4 {
        stat.insert(i, Content::Start(i));
    }

    let mut rands = vec![];
    let max_duration = 500_000;
    for _ in 0..30 {
        let rand = get_random_integer(RandomMode::Uniform(1_000, max_duration));
        rands.push(rand);
    }
    rands.sort();
    rands.into_iter().for_each(|n| {
        let ele = (n % 4) as usize;
        let duration = Duration::from_millis(n);
        let old_stat = stat.get(&ele).unwrap();
        match old_stat {
            &Content::Start(_) => {
                env.set_node(ele, Content::Stop, duration);
                stat.insert(ele, Content::Stop);
                info!("stop node {} after {:?}", ele, duration);
            }
            &Content::Stop => {
                env.set_node(ele, Content::Start(ele), duration);
                stat.insert(ele, Content::Start(ele));
                info!("start node {} after {:?}", ele, duration);
            }
            _ => {}
        }
    });

    stat.iter().for_each(|(i, stat)| {
        if let &Content::Stop = stat {
            let duration = Duration::from_millis(max_duration);
            env.set_node(*i, Content::Start(*i), duration);
            info!("finally start node {} after {:?}", i, duration);
        }
    });

    env.run(50);
}

#[test]
fn test_byzantine() {
    let path = "log/test_byzantine.log";
    let wal_dir = "wal/test_byzantine/wal";
    clean_wal(wal_dir);
    clean_log_file(path);
    set_log_file(path, LevelFilter::Debug);
    let mut env = Env::new(NORMAL_CONFIG, 4, wal_dir);
    env.set_node(0, Content::Corrupt, Duration::from_millis(1000));
    env.run(100);
}
