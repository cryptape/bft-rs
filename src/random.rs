use rand::{thread_rng, Rng};
use rand_core::{RngCore, SeedableRng};
use rand_pcg::Pcg64Mcg as Pcg;

pub(crate) fn random_proposer(seed: u64, weight: (Vec<u32>, u32)) -> usize {
    let sum = u64::from(weight.1);
    let weight = weight.0;
    let x = u64::max_value() / sum;

    let mut rng = Pcg::seed_from_u64(seed);
    let res = rng.next_u64();

    for (index, w) in weight.iter().enumerate() {
        if res < u64::from(*w) * x {
            return index;
        }
    }
    let mut rng = thread_rng();
    rng.gen_range(0, weight.len()) as usize
}

#[cfg(test)]
mod test {
    use super::random_proposer;

    fn cal_range(a: usize, b: usize, c: usize, d: usize) -> usize {
        let set = vec![a, b, c, d];
        let mut max = 0;
        let mut min = u64::max_value() as usize;

        for num in set.iter() {
            if *num > max {
                max = *num;
            }
            if *num < min {
                min = *num;
            }
        }
        max - min
    }

    #[test]
    fn test_rand_proposer() {
        let weight = vec![1, 2, 3, 4];
        let sum: u32 = 4;
        let mut sum_0 = 0;
        let mut sum_1 = 0;
        let mut sum_2 = 0;
        let mut sum_3 = 0;

        for s in 1..10000000 {
            let res_1 = random_proposer(s, (weight.clone(), sum));
            let res_2 = random_proposer(s, (weight.clone(), sum));
            assert_eq!(res_1, res_2);
            if res_1 == 0 {
                sum_0 += 1;
            } else if res_1 == 1 {
                sum_1 += 1;
            } else if res_1 == 2 {
                sum_2 += 1;
            } else {
                sum_3 += 1;
            }
        }
        println!("{}", cal_range(sum_0, sum_1, sum_2, sum_3));
    }
}
