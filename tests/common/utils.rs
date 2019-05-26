use rand::distributions::{Distribution, Normal, Uniform};
use std::fs::{self, read_dir};
use std::time::Duration;
use std::u64::MAX as MAX_U64;

use super::config::*;
use bft_rs::*;
use digest_hash::EndianInput;
use digest_hash::{BigEndian, Hash as DigestHash};
use log::info;
use log::LevelFilter;
use log4rs::append::file::FileAppender;
use log4rs::config::{Appender, Config as LogConfig, Root};
use log4rs::encode::pattern::PatternEncoder;
use sha2::{Digest, Sha256};

pub fn generate_block(byzantine: bool, config: &Config) -> Block {
    let random_size = get_random_integer(config.block_size) as usize;
    let size = if random_size < config.max_block_size {
        if random_size < config.min_block_size {
            config.min_block_size
        } else {
            random_size
        }
    } else {
        config.max_block_size
    };
    let mut vec = Vec::with_capacity(size);
    unsafe {
        vec.set_len(size);
    }
    let mark = if byzantine { 1u8 } else { 0u8 };
    vec.insert(0, mark);
    for i in 1..config.min_block_size {
        vec.insert(i, get_random_integer(RANDOM_U8) as u8);
    }
    vec.into()
}

pub fn check_block_result(block: &Block) -> bool {
    !block.is_empty() && block[0] == 0u8
}

pub fn check_txs_result(config: &Config) -> bool {
    get_dice_result(config.check_txs_failed_rate)
}

pub fn check_txs_delay(config: &Config) -> Duration {
    let rand_num = get_random_integer(config.check_txs_delay);
    let delay = if rand_num < config.min_delay {
        config.min_delay
    } else {
        rand_num
    };
    Duration::from_millis(delay)
}

pub fn sync_delay(height_diff: Height, config: &Config) -> Duration {
    let rand_num = get_random_integer(config.sync_delay);
    let delay = if rand_num < config.min_delay {
        config.min_delay
    } else {
        rand_num
    };
    Duration::from_millis(delay * height_diff)
}

pub fn commit_delay(config: &Config) -> Duration {
    let rand_num = get_random_integer(config.commit_delay);
    let delay = if rand_num < config.min_delay {
        config.min_delay
    } else {
        rand_num
    };
    Duration::from_millis(delay)
}

pub fn is_message_lost(config: &Config) -> bool {
    get_dice_result(config.message_lost_rate)
}

pub fn message_delay(config: &Config) -> Duration {
    let rand_num = get_random_integer(config.message_delay);
    let cost_time = if rand_num < config.max_delay {
        if rand_num < config.min_delay {
            config.min_delay
        } else {
            rand_num
        }
    } else {
        config.max_delay
    };
    Duration::from_millis(cost_time)
}

pub fn generate_address() -> Address {
    let mut vec = Vec::with_capacity(ADDRESS_SIZE);
    for _i in 0..ADDRESS_SIZE {
        vec.push(get_random_integer(RANDOM_U8) as u8);
    }
    vec.into()
}

pub fn hash(msg: &[u8]) -> Hash {
    let mut hasher = BigEndian::<Sha256>::new();
    hash_slice(msg, &mut hasher);
    let output = hasher.result().as_ref().to_vec();
    output.into()
}

// simplified for test
pub fn sign(_hash_value: &Hash, address: &Address) -> Signature {
    address.to_vec().into()
}

pub fn clean_wal(wal_dir: &str) {
    let mut i = 0;
    let mut dir = format!("{}{}", wal_dir, i);
    while read_dir(&dir).is_ok() {
        fs::remove_dir_all(&dir).unwrap();
        i += 1;
        dir = format!("{}{}", wal_dir, i);
    }
    info!("Successfully clean wal logs!");
}

pub fn clean_log_file(path: &str) {
    if fs::read(path).is_ok() {
        fs::remove_file(path).unwrap();
    }
}

pub fn set_log_file(path: &str, level: LevelFilter) {
    let logfile = FileAppender::builder()
        .encoder(Box::new(PatternEncoder::new(
            "{d(%Y-%m-%d %H:%M:%S .%f)} {l} - {m} - {M}-{L} {n}",
        )))
        .build(path)
        .unwrap();
    let config = LogConfig::builder()
        .appender(Appender::builder().build("logfile", Box::new(logfile)))
        .build(Root::builder().appender("logfile").build(level))
        .unwrap();
    let _ = log4rs::init_config(config);
}

pub fn get_dice_result(likelihood: f64) -> bool {
    let rand_num = get_random_integer(RANDOM_U64) as f64;
    let rate = rand_num / ((MAX_U64 - 1) as f64);
    rate > likelihood
}

#[derive(Clone, Copy)]
pub enum RandomMode {
    Normal(f64, f64),
    Uniform(u64, u64),
}

pub fn get_random_integer(mode: RandomMode) -> u64 {
    let v;
    match mode {
        RandomMode::Normal(_, _) => {
            v = get_random_float(mode) as u64;
        }
        RandomMode::Uniform(lower_bound, upper_bound) => {
            let between = Uniform::from(lower_bound..upper_bound);
            v = between.sample(&mut rand::thread_rng());
        }
    }
    v
}

pub fn get_random_float(mode: RandomMode) -> f64 {
    let v;
    match mode {
        RandomMode::Normal(mean, standard_deviation) => {
            let normal = Normal::new(mean, standard_deviation);
            v = normal.sample(&mut rand::thread_rng());
        }
        RandomMode::Uniform(_, _) => {
            v = get_random_integer(mode) as f64;
        }
    }
    v
}

fn hash_slice<T, H>(slice: &[T], digest: &mut H)
where
    T: DigestHash,
    H: EndianInput,
{
    for elem in slice {
        elem.hash(digest);
    }
}
