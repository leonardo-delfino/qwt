//! The module provides utilities to perform tests and performance evaluations
//! of different data structures.
//! In particular, it provides functions to generate random increasing sequences and
//! random queries, to measure rank and select queries, and so on.

use rand::Rng;
use std::time::Instant;

/// Returns the type name of its argument.
pub fn type_of<T>(_: &T) -> &'static str {
    std::any::type_name::<T>()
}

/// Generates a random sequence of length `n` over the alphabet [0, `sigma`].
pub fn gen_sequence(n: usize, sigma: usize) -> Vec<u8> {
    assert!(sigma <= 256);
    let mut rng = rand::thread_rng();
    (0..n).map(|_| rng.gen_range(0..sigma) as u8).collect()
}

/// Generates a random vector of `n_queries` values in [0, `range_size`].
/// This can be used to generate random queries.
pub fn gen_queries(n_queries: usize, range_size: usize) -> Vec<usize> {
    let mut rng = rand::thread_rng();
    (0..n_queries)
        .map(|_| rng.gen_range(0..range_size))
        .collect()
}

/// Generates a random vector of `n_queries`.
/// Each query is a pair: a value in [0, `range_size`] and a symbol in [0, `sigma`].
/// This can be used to generate random queries for rank/select over a general alphabet.
pub fn gen_queries_pairs(n_queries: usize, range_size: usize, sigma: usize) -> Vec<(usize, usize)> {
    let mut rng = rand::thread_rng();
    (0..n_queries)
        .map(|_| (rng.gen_range(0..range_size), rng.gen_range(0..sigma)))
        .collect()
}

/// Generates a random strictly increasing sequence of `n` values up to `u`.
pub fn gen_strictly_increasing_sequence(n: usize, u: usize) -> Vec<usize> {
    let mut rng = rand::thread_rng();
    let mut v: Vec<usize> = (0..n).map(|_x| rng.gen_range(0..(u - n))).collect();
    v.sort_unstable();
    for (i, value) in v.iter_mut().enumerate() {
        // remove duplicates to make a strictly increasing sequence
        *value += i;
    }
    v
}

/*
/// Tests rank1 op by querying every position of a bit set to 1 in the binary vector
/// and the next position.
pub fn test_rank1<T>(ds: &T, bv: &BitVector)
where
    T: Rank,
{
    for (rank, pos) in bv.ones().enumerate() {
        let result = ds.rank1(pos);
        assert_eq!(result, Some(rank));
        let result = ds.rank1(pos + 1);
        dbg!(pos + 1, rank);
        assert_eq!(result, Some(rank + 1));
    }
    let result = ds.rank1(bv.len() + 1);
    assert_eq!(result, None);
}

/// Tests select1 op by querying every position of vector.
pub fn test_select1<T>(ds: &T, data: &[usize])
where
    T: Select,
{
    for (i, &v) in data.iter().enumerate() {
        let result = ds.select1(i);
        dbg!(i, v);
        assert_eq!(result, Some(v));
    }
}
*/

pub struct TimingQueries {
    timings: Vec<u128>,
    time: Instant,
    n_queries: usize,
}

impl TimingQueries {
    pub fn new(n_runs: usize, n_queries: usize) -> Self {
        Self {
            timings: Vec::with_capacity(n_runs),
            time: Instant::now(),
            n_queries,
        }
    }

    #[inline(always)]
    pub fn start(&mut self) {
        self.time = Instant::now();
    }

    #[inline(always)]
    pub fn stop(&mut self) {
        self.timings.push(self.time.elapsed().as_nanos());
    }

    /// Returns minimum, maximum, average query time per query in nanosecs.
    pub fn get(&self) -> (u128, u128, u128) {
        let min = *self.timings.iter().min().unwrap() / (self.n_queries as u128);
        let max = *self.timings.iter().max().unwrap() / (self.n_queries as u128);
        let avg =
            self.timings.iter().sum::<u128>() / ((self.timings.len() * self.n_queries) as u128);
        (min, max, avg)
    }
}

/// Given a strictly increasing vector v, it returns a vector with all
/// the values not in v.
pub fn negate_vector(v: &[usize]) -> Vec<usize> {
    let max = *v.last().unwrap();
    let mut vv = Vec::with_capacity(max - v.len() + 1);
    let mut j = 0;
    for i in 0..max {
        if i == v[j] {
            j += 1;
        } else {
            vv.push(i);
        }
    }
    assert_eq!(max - v.len() + 1, vv.len());
    vv
}