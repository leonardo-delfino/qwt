//! A data structure for rank support provides an encoding scheme for counters of symbols
//! of a quaternary sequence up to the beginning of blocks of a fixed size `Self::BLOCK_SIZE`.
//!
//!
use crate::utils::get_64byte_aligned_vector;
use crate::QVector;
use crate::SpaceUsage; // Traits

use core::arch::x86_64::_mm_prefetch;

use serde::{Deserialize, Serialize};

use super::*;

/// The generic const `B_SIZE` specifies the number of symbols in each block.
/// The possible values are 256 (default) and 512.
/// The space overhead for 256 is 12.5% while 512 halves this
/// space overhead (6.25%) at the cost of (slightly) increasing the query time.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct RSSupportPlain<const B_SIZE: usize = 256> {
    superblocks: Vec<SuperblockPlain>,
    occs: [usize; 4], // number of total occurrences of each symbol in qv
    select_samples: [Vec<u32>; 4],
    n: usize,
}

impl<const B_SIZE: usize> SpaceUsage for RSSupportPlain<B_SIZE> {
    /// Gives the space usage in bytes of the struct.
    fn space_usage_bytes(&self) -> usize {
        let mut select_space = 0;
        for c in 0..4 {
            select_space += self.select_samples[c].space_usage_bytes();
        }
        self.superblocks.space_usage_bytes()
            + 4 * self.occs[0].space_usage_bytes()
            + select_space
            + self.n.space_usage_bytes()
    }
}

impl<const B_SIZE: usize> RSSupport for RSSupportPlain<B_SIZE> {
    const BLOCK_SIZE: usize = B_SIZE;

    fn new(qv: &QVector) -> Self {
        assert!(qv.len() < (1 << 43));

        assert!(
            (Self::BLOCK_SIZE == 256) | (Self::BLOCK_SIZE == 512),
            "Block size is either 256 or 512 symbols."
        );

        // A counter for each symbol
        //    - from the beginning of the sequence
        //    - from the beginning of the superblock
        let mut superblock_counters = [0; 4];
        let mut block_counters = [0; 4];
        let mut occs = [0; 4];

        // Sample superblock ids for each symbol at every SELECT_SAMPLES occurrences
        let mut select_samples: [Vec<u32>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];

        // Number of symbols in each superblock
        let superblock_size: usize = Self::BLOCKS_IN_SUPERBLOCK * Self::BLOCK_SIZE;
        let n_superblocks = (qv.len() + superblock_size) / superblock_size;
        let mut superblocks =
            unsafe { get_64byte_aligned_vector::<SuperblockPlain>(n_superblocks) };

        for i in 0..qv.len() + 1 {
            // Need position qv.len() to make last superblock if needed

            if i % superblock_size == 0 {
                superblocks.push(SuperblockPlain::new(&superblock_counters));
                block_counters = [0; 4]; // reset block counters
            }

            if i % Self::BLOCK_SIZE == 0 {
                // Start a new block and add occs in the block to its counter
                let block_id = (i / Self::BLOCK_SIZE) % Self::BLOCKS_IN_SUPERBLOCK;

                superblocks
                    .last_mut()
                    .unwrap()
                    .set_block_counters(block_id, &block_counters);

                for symbol in 0..4u8 {
                    // just check if everything is ok
                    debug_assert_eq!(
                        block_counters[symbol as usize],
                        superblocks
                            .last()
                            .unwrap()
                            .get_block_counter(symbol, block_id)
                    );
                }
            }

            if i < qv.len() {
                // Safety: We are sure to be not out of bound
                let symbol = unsafe { qv.get_unchecked(i) as usize};

                if occs[symbol] % Self::SELECT_NUM_SAMPLES == 0 {
                    // we store a superblock id in a u32. Make sure it fits.
                    debug_assert!(Self::superblock_index(i) <= u32::MAX as usize);
                    debug_assert!(Self::superblock_index(i) < superblocks.len());
                    select_samples[symbol].push(Self::superblock_index(i) as u32);
                }

                superblock_counters[symbol] += 1;
                block_counters[symbol] += 1;
                occs[symbol] += 1;
            }
        }

        // Fill next blocks with max occurrences. This is a sentinel for select algorithm
        let next_block_id = (qv.len() / Self::BLOCK_SIZE) % Self::BLOCKS_IN_SUPERBLOCK + 1;

        if next_block_id < Self::BLOCKS_IN_SUPERBLOCK {
            superblocks
                .last_mut()
                .unwrap()
                .set_block_counters(next_block_id, &block_counters);
        }

        // Add a sentinel for select_samples
        for symbol in 0..4 {
            if select_samples[symbol].is_empty() {
                // always sample at least once
                select_samples[symbol].push(0);
            }
            select_samples[symbol].push(superblocks.len() as u32 - 1); // sentinel
        }

        superblocks.shrink_to_fit();

        Self {
            superblocks,
            occs,
            select_samples,
            n: qv.len(),
        }
    }

    /// Returns the number of occurrences of `SYMBOL` up to the beginning
    /// of the block that contains position `i`.
    ///
    /// We use a const generic to have a specialized method for each symbol.
    #[inline(always)]
    fn rank_block<const SYMBOL: u8>(&self, i: usize) -> usize {
        debug_assert!(SYMBOL <= 3, "Symbols are in [0, 3].");

        let superblock_index = Self::superblock_index(i);
        let block_index = Self::block_index(i);

        let mut result = self.superblocks[superblock_index].get_superblock_counter(SYMBOL);

        result += self.superblocks[superblock_index].get_block_counter(SYMBOL, block_index % 8);

        result
    }

    /// Returns a pair `(position, rank)` where the position is the beginning of the block
    /// that contains the `i`th occurrence of `symbol`, and `rank` is the number of
    /// occurrences of `symbol` up to the beginning of this block.
    ///
    /// The caller must guarantee that `i` is not zero or greater than the length of the indexed sequence.
    #[inline(always)]
    fn select_block(&self, symbol: u8, i: usize) -> (usize, usize) {
        let sampled_i = (i - 1) / Self::SELECT_NUM_SAMPLES;

        let mut first_sblock_id = self.select_samples[symbol as usize][sampled_i] as usize;
        let last_sblock_id = 1 + self.select_samples[symbol as usize][sampled_i + 1] as usize; // dont worry we have a sentinel

        let step = f64::sqrt((last_sblock_id - first_sblock_id) as f64) as usize + 1;

        while first_sblock_id < last_sblock_id {
            if self.superblocks[first_sblock_id].get_superblock_counter(symbol) >= i {
                break;
            }
            first_sblock_id += step;
        }

        first_sblock_id -= step;

        while first_sblock_id < last_sblock_id {
            if self.superblocks[first_sblock_id].get_superblock_counter(symbol) >= i {
                break;
            }
            first_sblock_id += 1;
        }

        first_sblock_id -= 1;

        let mut position = first_sblock_id * Self::BLOCK_SIZE * Self::BLOCKS_IN_SUPERBLOCK; // i.e., superblocksize
        let mut rank = self.superblocks[first_sblock_id].get_superblock_counter(symbol);

        // we have a sentinel block at the end. No way we can go too far.
        let (block_id, block_rank) =
            self.superblocks[first_sblock_id].block_predecessor(symbol, i - rank);

        position += block_id * Self::BLOCK_SIZE;
        rank += block_rank;

        (position, rank)
    }

    /// Returns the number of occurrences of `SYMBOL` in the whole sequence.
    #[inline(always)]
    fn n_occs(&self, symbol: u8) -> usize {
        debug_assert!(symbol <= 3, "Symbols are in [0, 3].");

        self.occs[symbol as usize]
    }

    /// Prefetches the counters for the specified position `i`.
    ///
    /// This is very useful for speeding up operations where
    /// the compiler is not able to predict that a rank at a certain position
    /// will be required soon. This happens, for example, for `get` operation on Wavelet
    /// trees.  
    #[inline(always)]
    fn prefetch(&self, i: usize) {
        unsafe {
            let p = self.superblocks.as_ptr().add(Self::superblock_index(i));
            _mm_prefetch(p as *const i8, core::arch::x86_64::_MM_HINT_NTA);
        }
    }

    /// Shrinks to fit
    fn shrink_to_fit(&mut self) {
        self.superblocks.shrink_to_fit();
        for i in 0..4 {
            self.select_samples[i].shrink_to_fit();
        }
    }

    /// Returns the length of the indexed sequence.
    fn len(&self) -> usize {
        self.n
    }
}

impl<const B_SIZE: usize> RSSupportPlain<B_SIZE> {
    const SELECT_NUM_SAMPLES: usize = 1 << 13;
    const BLOCKS_IN_SUPERBLOCK: usize = 8; // Number of blocks in each superblock

    #[inline(always)]
    fn superblock_index(i: usize) -> usize {
        i / (Self::BLOCK_SIZE * Self::BLOCKS_IN_SUPERBLOCK)
    }

    #[inline(always)]
    fn block_index(i: usize) -> usize {
        i / Self::BLOCK_SIZE
    }
}

/// Stores counters for a superblock and its blocks.
/// We use a u128 for each of the 4 symbols.
/// A u128 is subdivided as follows:
/// - First 44 bits to store superblock counters
/// - Next 84 to store counters for 7 (out of 8) blocks (the first one is excluded)
#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, PartialEq)]
struct SuperblockPlain {
    counters: [u128; 4],
}

impl SpaceUsage for SuperblockPlain {
    /// Gives the space usage in bytes of the struct.
    fn space_usage_bytes(&self) -> usize {
        4 * 128 / 8
    }
}

impl SuperblockPlain {
    const BLOCKS_IN_SUPERBLOCK: usize = 8; // Number of blocks in each superblock

    /// Creates a new superblock initialized with the number of occurrences
    /// of the four symbols from the beginning of the text.
    fn new(sbc: &[usize; 4]) -> Self {
        let mut counters = [0u128; 4];
        for symbol in 0..4 {
            counters[symbol] = (sbc[symbol] as u128) << 84;
        }

        Self { counters }
    }

    #[inline(always)]
    fn get_superblock_counter(&self, symbol: u8) -> usize {
        (self.counters[symbol as usize] >> 84) as usize
    }

    fn set_block_counters(&mut self, block_id: usize, counters: &[usize; 4]) {
        assert!(block_id < 8);
        for i in 0..4 {
            //assert!(counters[i] < SUPERBLOCK_SIZE);
            assert!(counters[i] < (1 << 12));
        }
        if block_id == 0 {
            return;
        }

        for symbol in 0..4 {
            self.counters[symbol] |= (counters[symbol] as u128) << ((block_id - 1) * 12);
        }
    }

    #[inline(always)]
    fn get_block_counter(&self, symbol: u8, block_id: usize) -> usize {
        debug_assert!(block_id < Self::BLOCKS_IN_SUPERBLOCK);
        if block_id == 0 {
            0
        } else {
            (self.counters[symbol as usize] >> ((block_id - 1) * 12) & 0b111111111111) as usize
        }
    }

    // Returns the largest block id for `symbol` in this superblock
    // such that its counter is smaller than `target` value.
    //
    // # TODO
    // The loop is not (auto)vectorized but we know we are just searching for the predecessor
    // of a 12bit value in the last 84 bits of a u128.
    #[inline(always)]
    pub fn block_predecessor(&self, symbol: u8, target: usize) -> (usize, usize) {
        let mut cnt = self.counters[symbol as usize];

        /*
        if v = (cnt & 0b111111111111) as usize >= target {
            return (0, 0);
        }
        if ((cnt >> 12) & 0b111111111111) as usize >= target {
            return (1, (cnt & 0b111111111111) as usize);
        }
        if ((cnt >> 24) & 0b111111111111) as usize >= target {
            return (2, ((cnt >> 12) & 0b111111111111) as usize);
        }
        if ((cnt >> 36) & 0b111111111111) as usize >= target {
            return (3, ((cnt >> 24) & 0b111111111111) as usize);
        }
        if ((cnt >> 48) & 0b111111111111) as usize >= target {
            return (4, ((cnt >> 36) & 0b111111111111) as usize);
        }
        if ((cnt >> 60) & 0b111111111111) as usize >= target {
            return (5, ((cnt >> 48) & 0b111111111111) as usize);
        }
        if ((cnt >> 72) & 0b111111111111) as usize >= target {
            return (6, ((cnt >> 60) & 0b111111111111) as usize);
        }
        (7, ((cnt >> 72) & 0b111111111111) as usize)*/

        let mut prev_cnt = 0;

        for block_id in 1..Self::BLOCKS_IN_SUPERBLOCK {
            let curr_cnt = (cnt & 0b111111111111) as usize;
            if curr_cnt >= target {
                return (block_id - 1, prev_cnt);
            }
            cnt >>= 12;
            prev_cnt = curr_cnt;
        }

        (Self::BLOCKS_IN_SUPERBLOCK - 1, prev_cnt)
    }
}