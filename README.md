# QWT: Rust Quad Wavelet Tree

Given a sequence, *rank* and *select* queries return the number of occurrences of a symbol up to a given position (rank) or the position of a symbol with a given rank (select). These queries have applications in, e.g., compression, computational geometry, and pattern matching in the form of the backward search---the backbone of many compressed full-text indices.

A wavelet tree [[1](#bib)] is a compact data structure that for a text of length $n$ over an alphabet of size $\sigma$ requires only $n\lceil\log \sigma \rceil (1+o(1))$ bits of space and can answer rank and select queries in $\Theta(\log \sigma)$ time.

This repository provides a very fast implementation of Wavelet Trees in Rust. A companion C++ implementation is available [here](https://github.com/MatteoCeregini/quad-wavelet-tree).

Our experimental evaluation shows that our quad wavelet tree can improve the latency of access, rank and select queries by a factor of $\approx$ 2 compared to other implementations of wavelet trees contained in the widely used C++ Succinct Data Structure Library ([SDSL](https://github.com/simongog/sdsl-lite)). See experimental results in the next Section [Benchmarks](#bench) and in paper [[2](#bib)].

Our implementation **QWT** improves query performance by using a 4-ary tree instead of a binary tree as basis of the wavelet tree. The 4-ary tree layout of a wavelet tree helps to halve the number of cache misses during queries and thus reduces the query latency. For more details, see [Benchmarks](#bench) and the paper [[2](#bib)].

## <a name="bench">Benchmarks</a>
We report here a few experiments to compare our implementation with other state-of-the-art implementations.
The experiments are performed using a single thread on a server machine with 8 Intel~i9-9900KF cores with base frequencies of 3.60 GHz running Linux 5.19.0. The code is compiled with Rust 1.69.0. Each core has a dedicated L1 cache of size 32 KiB, a dedicated L2 cache of size 256 KiB, a shared L3 cache of size 16 MiB, and 64 GiB of RAM.

A more detailed experimental evaluation can be found in [[2](#bib)].

The dataset is `english.2GiB`: the 2 GiB prefix of the [English](http://pizzachili.dcc.uchile.cl/texts/nlang/english.gz) collection from [Pizza&Chili corpus](http://pizzachili.dcc.uchile.cl/) (See details below). The text has an alphabet with 239 distinct symbols.

| Wavelet Tree | *access* (ns) | *rank* (ns) | *select* (ns) | space (MiB) |
| :----------- | ------------: | ----------: | ------------: | ----------: |
| SDSL         |           693 |         786 |          2619 |        3039 |
| Pasta        |           832 |         846 |          2403 |        2124 |
| QWT 256      |           436 |         441 |          1135 |        2308 |
| QWT 512      |           451 |         460 |          1100 |        2180 |

To run the experiments, we need to compile the binary executables with
```bash
cargo build --release
```

This produces two executables `perf_rs_quat_vector` and `perf_wavelet_tree` in `\target\release\`.

The former is used to measure the performance of QuadVectors, which are the building block of our implementation of Wavelet Trees. We can safely ignore it.

The latter is used to measure the performance of a Quad Wavelet Tree built on a given input text.

We can now download and uncompress in the current directory the [English](http://pizzachili.dcc.uchile.cl/texts/nlang/english.gz) collection from [Pizza&Chili corpus](http://pizzachili.dcc.uchile.cl/). Then, we take its prefix of length 2 GiB.

```bash
wget http://pizzachili.dcc.uchile.cl/texts/nlang/english.gz
gunzip english.gz
head -c 2147483648 english > english.2GiB
```

The following command builds the wavelet trees on this input text and runs 1 million random *access*, *rank*, and *select* queries.

```bash
./target/release/perf_wavelet_tree --input-file english.2GiB --access --rank --select
```

We can use the flag `--test-correctness` to perform some extra tests for the correctness of the index.

The code measures the *latency* of the queries by forcing the input of each query to depend on the output of the previous one. This is consistent with the use of the queries in a real setting. For example, the more advanced queries supported by compressed text indexes (e.g., CSA or FM-index) decompose into several dependent queries on the underlying wavelet tree.

## Examples

Run the following Cargo command in your project directory

```
cargo add qwt
```

to add the library.

Once the crate has been added, we can easily build a Quad Wavelet Tree with the following code. 

```rust
use qwt::QWaveletTreeP256;

let mut data: [u8; 8] = [1, 0, 1, 0, 3, 4, 5, 3];

let qwt = QWaveletTreeP256::new(&mut data);

assert_eq!(qwt.len(), 8);
```

Note that ```data``` must be mutable because the construction of the wavelet tree is going to permute it. Make a copy of ```data``` if you need it later on.

We can print the space usage of the wavelet tree with 

```rust
use qwt::SpaceUsage;

println!( qwt.space_usage_bytes() );
```

The data structure supports three operations:
- `get(i)` accesses the `i`-th symbols of the indexed sequence;
- `rank(c, i)` counts the number of occurrences of symbol `c` up to position `i` excluded;
- `select(c, i)` returns the position of the `i`-th occurrence of symbol `c`.

Here is an example of the three operations.

```rust
use qwt::{QWaveletTreeP256, AccessUnsigned, RankUnsigned, SelectUnsigned};

let mut data: [u8; 8] = [1, 0, 1, 0, 2, 4, 5, 3];

let qwt = QWaveletTreeP256::new(&mut data);

assert_eq!(qwt.get(2), Some(1));
assert_eq!(qwt.get(3), Some(0));
assert_eq!(qwt.get(8), None);

assert_eq!(qwt.rank(1, 2), Some(1));
assert_eq!(qwt.rank(1, 0), Some(0));
assert_eq!(qwt.rank(3, 8), Some(1));
assert_eq!(qwt.rank(1, 9), None);

assert_eq!(qwt.select(1, 1), Some(0));
assert_eq!(qwt.select(0, 2), Some(3));
assert_eq!(qwt.select(4, 0), None);
assert_eq!(qwt.select(1, 3), None);
```

In the following example, we use QWT to index a sequence over larger alphabet.

```rust
use qwt::{QWaveletTreeP256, AccessUnsigned, RankUnsigned, SelectUnsigned};

let mut data: [u32; 8] = [1, 0, 1, 0, 2, 1000000, 5, 3];
let qwt = QWaveletTreeP256::new(&mut data);

assert_eq!(qwt.get(2), Some(1));
assert_eq!(qwt.get(5), Some(1000000));
assert_eq!(qwt.get(8), None);
```

## <a name="bib">Bibliography</a>
1. Roberto Grossi, Ankur Gupta, and Jeffrey Scott Vitter. *High-order entropy-compressed text indexes.* In SODA, pages 841–850. ACM/SIAM, 2003.
2. Matteo Ceregini, Florian Kurpicz, Rossano Venturini. *Faster Wavelet Trees with Quad Vectors*. Arxiv, 2023.
----

Please cite the following paper if you use this code.

```
@misc{QWT,
  author = {Matteo Ceregini, Florian Kurpicz, Rossano Venturini},
  title = {Faster Wavelet Trees with Quad Vectors},
  publisher = {arXiv},
  year = {2023},
  doi = {-},
  url = {https://arxiv.org/abs/-}
}
```

## TODO
- Use a binary vector at the first level when log sigma is odd.
- Fix too large capacity after deserialization.
- Implement an efficient iterator over a Wavelet Tree.
- Fix select if ith occurrence does not exist