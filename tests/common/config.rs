use super::utils::RandomMode;
use std::time::Duration;
use std::u64::MAX as MAX_U64;
use std::u8::MAX as MAX_U8;

pub const LIVENESS_TICK: Duration = Duration::from_secs(60);
pub const ADDRESS_SIZE: usize = 20; // 160
pub const WAL_ROOT: &'static str = "wal";
pub const RANDOM_U8: RandomMode = RandomMode::Uniform(0u64, MAX_U8 as u64);
pub const RANDOM_U64: RandomMode = RandomMode::Uniform(0u64, MAX_U64);

#[derive(Clone, Copy)]
pub struct Config {
    pub block_size: RandomMode,
    pub max_block_size: usize,
    pub min_block_size: usize,
    pub check_txs_failed_rate: f64,
    pub message_lost_rate: f64,
    pub max_delay: u64, // ms
    pub min_delay: u64, // ms
    pub check_txs_delay: RandomMode,
    pub commit_delay: RandomMode,
    pub sync_trigger_duration: u64, //ms
    pub sync_delay: RandomMode,
    pub message_delay: RandomMode,
}

pub const NORMAL_CONFIG: Config = Config {
    block_size: RandomMode::Normal(1_024_000.0, 512_000.0),
    max_block_size: 10_240_000,
    min_block_size: 100,
    check_txs_failed_rate: 0.02,
    message_lost_rate: 0.01,
    max_delay: 60_000,
    min_delay: 5,
    check_txs_delay: RandomMode::Normal(100.0, 30.0),
    commit_delay: RandomMode::Normal(50.0, 20.0),
    sync_trigger_duration: 6_000,
    sync_delay: RandomMode::Normal(10.0, 2.0),
    message_delay: RandomMode::Normal(30.0, 20.0),
};

pub const PERFECT_CONFIG: Config = Config {
    block_size: RandomMode::Normal(1_000.0, 100.0),
    max_block_size: 2_000,
    min_block_size: 100,
    check_txs_failed_rate: 0.0,
    message_lost_rate: 0.0,
    max_delay: 60_000,
    min_delay: 1,
    check_txs_delay: RandomMode::Normal(3.0, 1.0),
    commit_delay: RandomMode::Normal(3.0, 1.0),
    sync_trigger_duration: 6_000,
    sync_delay: RandomMode::Normal(3.0, 1.0),
    message_delay: RandomMode::Normal(10.0, 5.0),
};

