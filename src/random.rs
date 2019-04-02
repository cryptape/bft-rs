use crate::error::BftError;

use rand_core::{RngCore, SeedableRng};
use rand_pcg::Pcg64Mcg as Pcg;

pub(crate) fn random_proposer(seed: u64, weight: (Vec<u32>, u32)) -> Result<usize, BftError> {
    let mut rng = Pcg::seed_from_u64(seed);
    let res = rng.next_u64();
    let sum = weight.1 as f32;
    let weight = weight.0;

    for ni in 0..weight.len() {
        let threshold: f32 = u64::max_value() as f32 * (weight[ni] as f32 / sum);
        if res < threshold.ceil() as u64 {
            return Ok(ni);
        }
    }
    Err(BftError::DetermineProposerErr)
}
