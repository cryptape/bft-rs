#[cfg(feature = "random_proposer")]
use rand_core::{RngCore, SeedableRng};
#[cfg(feature = "random_proposer")]
use rand_pcg::Pcg64Mcg as Pcg;

#[cfg(feature = "random_proposer")]
pub(crate) fn get_index(seed: u64, weight: &[u64]) -> usize {
    let sum: u64 = weight.iter().sum();
    let x = u64::max_value() / sum;

    let mut rng = Pcg::seed_from_u64(seed);
    let mut res = rng.next_u64();
    while res >= sum * x {
        res = rng.next_u64();
    }
    let mut acc = 0u64;
    for (index, w) in weight.iter().enumerate() {
        acc += *w;
        if res < acc * x {
            return index;
        }
    }
    0
}

#[cfg(not(feature = "random_proposer"))]
pub(crate) fn get_index(seed: u64, weight: &[u64]) -> usize {
    let sum: u64 = weight.iter().sum();
    let x = seed % sum;

    let mut acc = 0u64;
    for (index, w) in weight.iter().enumerate() {
        acc += *w;
        if x < acc {
            return index;
        }
    }
    0
}

#[cfg(test)]
mod test {
    use super::get_index;

    #[test]
    fn test_get_index() {
        let weight = vec![1u64, 2u64, 1u64, 10u64];
        let mut count_0 = 0;
        let mut count_1 = 0;
        let mut count_2 = 0;
        let mut count_3 = 0;
        for x in 0..1000 {
            let seed = x as u64;
            let index_1 = get_index(seed, &weight);
            let index_2 = get_index(seed, &weight);
            match index_1 {
                0 => count_0 += 1,
                1 => count_1 += 1,
                2 => count_2 += 1,
                3 => count_3 += 1,
                _ => {}
            }
            assert_eq!(index_1, index_2);
        }
        println!(
            "count_0:{}, count_1:{}, count_2:{}, count_3:{}",
            count_0, count_1, count_2, count_3
        );
    }
}
