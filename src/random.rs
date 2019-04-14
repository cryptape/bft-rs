use rand_core::{RngCore, SeedableRng};
use rand_pcg::Pcg64Mcg as Pcg;

pub(crate) fn get_proposer(seed: u64, weight: &[u64]) -> usize {

    let sum: u64 = weight.iter().sum();
    let x = u64::max_value() / sum;

    let mut rng = Pcg::seed_from_u64(seed);
    let mut res= rng.next_u64();
    while res >= sum * x {
        res = rng.next_u64();
    }
    let mut acc = 0u64;
    for (index, w) in weight.iter().enumerate() {
        acc += *w;
        if res < acc * x {
            info!("randomly choose index: {}", index);
            return index;
        }
    }
    0
}

#[cfg(test)]
mod test {
    use super::get_proposer;

    #[test]
    fn test_rand_proposer() {
        let weight = vec![1u64, 2u64, 1u64, 10u64];
        let mut count_0 = 0;
        let mut count_1 = 0;
        let mut count_2 = 0;
        let mut count_3 = 0;
        for x in 0..1000 {
            let seed = x as u64;
            let index_1 = get_proposer(seed, &weight);
            let index_2 = get_proposer(seed, &weight);
            match index_1{
                0 => count_0 += 1,
                1 => count_1 += 1,
                2 => count_2 += 1,
                3 => count_3 += 1,
                _ => {}
            }
            assert_eq!(index_1, index_2);
        }
        println!("count_0:{}, count_1:{}, count_2:{}, count_3:{}", count_0, count_1, count_2, count_3);
    }
}
