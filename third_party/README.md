# Third-party sources

## fearless_simd 0.7.0

A copy of [fearless_simd](https://github.com/linebender/fearless_simd) 0.7.0
as published on crates.io (git `57ec21a`), without the files `cargo publish`
adds. The root `Cargo.toml` uses it through `[patch.crates-io]`. It keeps its
own licenses, `Apache-2.0 OR MIT`.

Only `src/generated/neon.rs` differs: `mul_add` and `mul_sub` of `f32x4` and
`f64x2` multiply and then add or subtract, instead of calling `vfmaq` and
`vfmsq`. A fused multiply-add rounds once where SSE4.2 and the scalar path
round twice, so ARM drew different bytes from x86
([decisions.md](../docs/rust/decisions.md), section 11). The x86 code is the
same as on crates.io. The test
`raster::tests::antialiased_edges_come_out_the_same_on_every_cpu` in
`ugu-render` fails on ARM if the fused operations come back.

When Vello moves to another version of fearless_simd, make the same change in
that version's source, and look for new fused operations: `vfmaq`, `vfmsq`,
`vfmaq_n`, `vfnmsq` and the like.
