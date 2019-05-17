use super::utils::RandomMode;
use std::time::Duration;
use std::u64::MAX as MAX_U64;
use std::u8::MAX as MAX_U8;

pub const LIVENESS_TICK: Duration = Duration::from_secs(60);

pub const BLOCK_SIZE: RandomMode = RandomMode::Normal(10_240_000.0, 1_000.0);
pub const MAX_BLOCK_SIZE: usize = 2_048_000;
pub const MIN_BLOCK_SIZE: usize = 100;

pub const ADDRESS_SIZE: usize = 20; // 160

pub const WAL_ROOT: &'static str = "wal";

pub const RANDOM_U8: RandomMode = RandomMode::Uniform(0u64, MAX_U8 as u64);
pub const RANDOM_U64: RandomMode = RandomMode::Uniform(0u64, MAX_U64);

pub const CHECK_TXS_FAILED_RATE: f64 = 0.05;
pub const MESSAGE_LOST_RATE: f64 = 0.01;

pub const MAX_DELAY: u64 = 60_000; // ms
pub const MIN_DELAY: u64 = 10; // ms
pub const CHECK_TXS_DELAY: RandomMode = RandomMode::Normal(300.0, 20.0); // ms
pub const COMMIT_DELAY: RandomMode = RandomMode::Normal(200.0, 20.0); // ms
pub const SYNC_DELAY: RandomMode = RandomMode::Normal(200.0, 5.0); // ms
pub const MESSAGE_DELAY: RandomMode = RandomMode::Normal(10.0, 0.0); // ms
