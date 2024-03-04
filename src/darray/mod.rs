//! The module implements `DArray`, a data structure that answers [`select1`]: select1
//! and `select0` queries on a binary vector supporting the [`Select`] trait.
//! Rank queries are not supported.
//!
//! The query `select_1(i)` returns the position of the (i+1)-th occurrence of a bit
//! set to 1 in the binary vector.
//! For example, let the binary vector be 010000101, `select1(0)` = 1,
//! `select1(1)` = 6, and `select1(2)` = 8.
//! Similarly, the query select0(i) returns the position of the (i+1)-th zero
//! in the binary vector.
//!
//! ## Example
//! A `DArray` is built on a [`BitVector`] with [`DArray::new`].
//! A boolean const generic is used to specify the need for
//! `select0` query support, (otherwise, calling `select0` will panic).
//!
//! ```
//! use qwt::BitVector;
//! use qwt::DArray;
//! use qwt::{SpaceUsage,SelectBin};
//!
//! let vv: Vec<usize> = vec![0, 12, 33, 42, 55, 61, 1000];
//! let bv: BitVector = vv.iter().copied().collect();
//! let da: DArray<false> = DArray::new(bv);
//!
//! assert_eq!(da.select1(1), Some(12));
//! ```
//!
//! ## Technical details
//! `DArray` has been introduced in *D. Okanohara and K. Sadakane.
//! Practical entropy-compressed Rank/Select dictionary.
//! In Proceedings of the Workshop on Algorithm Engineering and Experiments (ALENEX), 2007* ([link](https://arxiv.org/abs/cs/0610001)).
//! This Rust implementation is inspired by C++ implementation by
//! [Giuseppe Ottaviano](https://github.com/ot/succinct/blob/master/darray.hpp).
//!
//! Efficient queries are obtained by storing additional information of
//! top of the binary vector. The binary vector is split into blocks
//! of variable size. We mark the end of a block every `BLOCK_SIZE` = 1024-th
//! occurrence of 1. For each block we have two cases:
//!
//! 1) the block is *dense* if it is at most `MAX_IN_BLOCK_DISTACE` = 1 << 16 bits long;
//! 2) the block is *sparse*, otherwise.
//!
//! For case 1), we further split occurrences of 1 into subblocks of size
//! `SUBBLOCK_SIZE` = 32 bits each. We store the position of the first 1
//! of each block in a vector (called *subblock_inventory*) using 16 bits each.
//! In case 2) we explicitly write the position of all the ones in a vector,  
//! called *overflow_positions*.
//! The vector *block_inventory* stores a pointer to *subblock_inventory*
//! for blocks of the first kind and a pointer to *overflow_positions*
//! for the other kind of blocks. We use positive or negative
//! integers to distinguish the two cases.
//!
//! A `select1`(i) query is solved as follows. First, compute b=i/BLOCK_SIZE,
//! i.e., the block of the ith occurrence of 1 and access *block_inventory\[b\]*.
//! If the block is dense, we access the position of the first one in its
//! block and we start a linear scan from that position
//! looking for the ith occurrence of 1.
//! Instead, if the block is sparse, the answer is stored in vector *overflow_positions*.
//!
//! Space overhead is
//!
//! These three vectors are stored in a private struct Inventories.
//! The const generic BITS in this struct allows us to build and to store
//! these vectors to support `select0` as well.   
//!
use crate::utils::select_in_word;
use crate::BitVector;
use crate::{AccessBin, SelectBin, SpaceUsage};
use serde::{Deserialize, Serialize};
use std::arch::x86_64::_popcnt64;

const BLOCK_SIZE: usize = 1024;
const SUBBLOCK_SIZE: usize = 32;
const MAX_IN_BLOCK_DISTACE: usize = 1 << 16;

/// Const generic SELECT0_SUPPORT may optionally add
/// extra data structures to support fast `select0` queries,
/// which otherwise are not supported.

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct DArray<const SELECT0_SUPPORT: bool = false> {
    bv: BitVector,
    ones_inventories: Inventories<true>,
    zeroes_inventories: Option<Inventories<false>>,
}

// Helper struct for DArray that stores
// statistics, counters and overflow positions for bits
// set either to 0 or 1
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
struct Inventories<const BIT: bool> {
    n_sets: usize, // number of bits set to
    block_inventory: Vec<i64>,
    subblock_inventory: Vec<u16>,
    overflow_positions: Vec<usize>,
}

/// Const generic BIT specifies if we are computing statistics
/// for zeroes (BIT=false) or for ones (BIT=true).
impl<const BIT: bool> Inventories<BIT> {
    fn new(bv: &BitVector) -> Self {
        let mut me: Inventories<BIT> = Inventories::default();

        let mut curr_block_positions = Vec::with_capacity(BLOCK_SIZE);

        // FIXME: Need to duplicate the code because
        // let mut iter_positions: BitVectorBitPositionsIter = if !BIT {bv.zeroes(0)} else {bv.ones(0)};
        // doesn't compile.

        if !BIT {
            for curr_pos in bv.zeros() {
                curr_block_positions.push(curr_pos);
                if curr_block_positions.len() == BLOCK_SIZE {
                    me.flush_block(&curr_block_positions);
                    curr_block_positions.clear()
                }
                me.n_sets += 1;
            }
        } else {
            for curr_pos in bv.ones() {
                curr_block_positions.push(curr_pos);
                if curr_block_positions.len() == BLOCK_SIZE {
                    me.flush_block(&curr_block_positions);
                    curr_block_positions.clear()
                }
                me.n_sets += 1;
            }
        }

        me.flush_block(&curr_block_positions);
        me.shrink_to_fit();

        me
    }

    fn flush_block(&mut self, curr_positions: &[usize]) {
        if curr_positions.is_empty() {
            return;
        }
        if curr_positions.last().unwrap() - curr_positions.first().unwrap() < MAX_IN_BLOCK_DISTACE {
            let v = *curr_positions.first().unwrap();
            self.block_inventory.push(v as i64);
            for i in (0..curr_positions.len()).step_by(SUBBLOCK_SIZE) {
                let dist = (curr_positions[i] - v) as u16;
                self.subblock_inventory.push(dist);
            }
        } else {
            let v: i64 = (-(self.overflow_positions.len() as i64)) - 1;
            self.block_inventory.push(v);
            self.overflow_positions.extend(curr_positions.iter());
            self.subblock_inventory
                .extend(std::iter::repeat(u16::MAX).take(curr_positions.len()));
        }
    }

    // Shinks vectors to let their capacity to fit.
    fn shrink_to_fit(&mut self) {
        self.block_inventory.shrink_to_fit();
        self.subblock_inventory.shrink_to_fit();
        self.overflow_positions.shrink_to_fit();
    }
}

/// Const genetic SELECT0_SUPPORT
impl<const SELECT0_SUPPORT: bool> DArray<SELECT0_SUPPORT> {
    pub fn new(bv: BitVector) -> Self {
        let ones_inventories = Inventories::new(&bv);
        let zeroes_inventories = if SELECT0_SUPPORT {
            Some(Inventories::new(&bv))
        } else {
            None
        };
        DArray {
            bv,
            ones_inventories,
            zeroes_inventories,
        }
    }

    pub fn access(&self, pos: usize) -> Option<bool> {
        self.bv.get(pos)
    }

    pub fn len(&self) -> usize {
        self.bv.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bv.len() == 0
    }

    // pub fn unary_iter(&self, pos: usize) -> UnaryIter {
    //     self.bv.unary_iter(pos)
    // }

    // pub fn ones(&self, pos: usize) -> UnaryIterOnes {
    //     self.bv.ones(pos)
    // }

    // pub fn zeroes(&self, pos: usize) -> UnaryIterZeroes {
    //     self.bv.zeroes(pos)
    // }

    // Private generic select query, which solves either select0 and select1.
    #[inline(always)]
    fn select<const BIT: bool>(&self, i: usize, inventories: &Inventories<BIT>) -> Option<usize> {
        if i >= inventories.n_sets {
            return None;
        }
        let block = i / BLOCK_SIZE;
        let block_pos = inventories.block_inventory[block];

        if block_pos < 0 {
            // block is sparse
            let overflow_pos: usize = (-block_pos - 1) as usize;
            let idx = overflow_pos + (i & (BLOCK_SIZE - 1));
            return Some(inventories.overflow_positions[idx]);
        }
        let subblock = i / SUBBLOCK_SIZE;
        let start_pos = (block_pos as usize) + (inventories.subblock_inventory[subblock] as usize);
        let mut reminder = i & (SUBBLOCK_SIZE - 1);

        if reminder == 0 {
            return Some(start_pos);
        }

        let mut word_idx = start_pos >> 6;
        let word_shift = start_pos & 63;
        let mut word = if !BIT {
            !self.bv.get_word(word_idx) & (std::u64::MAX << word_shift) // if select0, negate the current word!
        } else {
            self.bv.get_word(word_idx) & (std::u64::MAX << word_shift)
        };

        loop {
            let popcnt;
            //popcnt = word.count_ones() as usize;
            unsafe {
                popcnt = _popcnt64(word as i64) as usize;
            }
            if reminder < popcnt {
                break;
            }
            reminder -= popcnt;
            word_idx += 1;
            word = self.bv.get_word(word_idx);
            if !BIT {
                word = !word; // if select0, negate the current word!
            }
        }
        let select_intra = select_in_word(word, reminder as u64) as usize;

        Some((word_idx << 6) + select_intra)
    }

    pub fn shrink_to_fit(&mut self) {
        self.bv.shrink_to_fit();
        self.ones_inventories.shrink_to_fit();
        if self.zeroes_inventories.is_some() {
            self.zeroes_inventories.as_mut().unwrap().shrink_to_fit();
        }
    }
}

impl<const SELECT0_SUPPORT: bool> SelectBin for DArray<SELECT0_SUPPORT> {
    #[inline(always)]
    fn select1(&self, i: usize) -> Option<usize> {
        self.select(i, &self.ones_inventories)
    }

    #[inline(always)]
    unsafe fn select1_unchecked(&self, i: usize) -> usize {
        self.select(i, &self.ones_inventories).unwrap()
    }

    /// Answers a `select0` query.
    ///
    /// The query `select0(i)` returns the position of the (i+1)-th
    /// occurrence of 0 in the binary vector.
    ///
    /// # Examples
    /// ```
    /// use qwt::DArray;
    /// use qwt::BitVector;
    /// use qwt::SelectBin;
    ///
    /// let vv: Vec<usize> = vec![0, 12, 33, 42, 55, 61, 1000];
    /// let bv: BitVector = vv.iter().copied().collect();
    /// let da: DArray<false> = DArray::new(bv);
    ///
    /// assert_eq!(da.select1(1), Some(12));
    /// ```
    ///
    /// # Panics
    /// It panics if [`DArray`] is built without support for `select0`query.
    #[inline(always)]
    fn select0(&self, i: usize) -> Option<usize> {
        assert!(SELECT0_SUPPORT);

        self.select(i, self.zeroes_inventories.as_ref().unwrap())
    }

    #[inline(always)]
    unsafe fn select0_unchecked(&self, i: usize) -> usize {
        assert!(SELECT0_SUPPORT);

        self.select(i, self.zeroes_inventories.as_ref().unwrap())
            .unwrap()
    }
}

impl<const SELECT0_SUPPORT: bool> SpaceUsage for DArray<SELECT0_SUPPORT> {
    fn space_usage_byte(&self) -> usize {
        let mut space = self.bv.space_usage_byte() + self.ones_inventories.space_usage_byte();

        if let Some(p) = self.zeroes_inventories.as_ref() {
            space += p.space_usage_byte();
        }
        space
    }
}

impl<const BIT: bool> SpaceUsage for Inventories<BIT> {
    fn space_usage_byte(&self) -> usize {
        self.n_sets.space_usage_byte()
            + self.block_inventory.space_usage_byte()
            + self.subblock_inventory.space_usage_byte()
            + self.overflow_positions.space_usage_byte()
    }
}

#[cfg(test)]
mod tests;
