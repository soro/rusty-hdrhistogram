extern crate test;

use self::test::Bencher;
use crate::st::Histogram;
use rand::Rng;

#[bench]
fn a_record_precalc_random_values_with_1_count_u64(b: &mut Bencher) {
    let mut h = Histogram::<u64>::with_low_high_sigvdig(1, u64::MAX, 3).unwrap();
    let mut indices = Vec::<u64>::new();
    let mut rng = rand::thread_rng();

    for _ in 0..1000_000 {
        indices.push(rng.gen());
    }

    b.iter(|| {
        for i in indices.iter() {
            // u64 counts, won't overflow
            h.record_value(*i).unwrap()
        }
    })
}
