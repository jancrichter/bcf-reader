# bcf_reader

Currently, the `rust_htslib` crate only works on linux and macos, not windows (?).
The `noodles` crate is a pure rust library for many bioinformatic file format
and works for windows, linux and macos.
However, currently, the `noodles::bcf` api of reading genotype data from bcf is
quite slow becauses it involes a lot of memory allocations. 
It seems there is not efficient BCF reader that works cross platforms.

One way to get around memory allocation is to parse BCF records manually according
its specification. 
https://samtools.github.io/hts-specs/VCFv4.2.pdf

Steps:
1. use `noodles` to decompress `bgzf` format and 
`noodles::bcf::Reader::read_header` to parse header info.
2. manually parse BCF recorder use the underlying reader.
- read size of the `shared` block and `indv` block.
- load raw bytes of the two blocks.
- parsing/skiping shared fields or genotype fields.