# Vendored libresidfp 1.2.3

- Project: libresidfp (reSIDfp), https://github.com/libsidplayfp/libresidfp
- Release: v1.2.3 (2026-10-02), official source tarball
  https://github.com/libsidplayfp/libresidfp/releases/download/v1.2.3/libresidfp-1.2.3.tar.gz
- SHA-256 of the tarball: `e9b53a984ca708c3e8d9139eb6aad6141f73ffc22d70a020f7966803f4e2f76d`
- License: GPL-2.0-or-later (`libresidfp-1.2.3/COPYING`); authors in `libresidfp-1.2.3/AUTHORS`
  (Dag Lem, Antti S. Lankila, Ken Händel, Leandro Nini).

## What was copied

`src/` of the tarball without `resample/test.cpp` and the two `*.in` templates, plus `COPYING`, `AUTHORS`,
`README.md`, `NEWS.md`. The build system (autotools), the tests and the docs were left out; `build.rs` compiles the
`.cpp` files with the `cc` crate.

## Changes against the release

1. `src/siddefs-fp.h` and `src/residfp/sidversion.h` are written by hand from the `.in` templates (what `configure`
   would make): branch hints on, `__builtin_expect` only for GCC/Clang, inlining on, version 1.2.3.
2. `src/FilterModelConfig.h`, class `Randomnoise` (the dither of the filter model):
   - `std::default_random_engine` + `std::uniform_real_distribution` replaced by `std::minstd_rand0` with a fixed
     conversion to [0, 1): the standard libraries implement both differently (MSVC uses another engine), so the
     sound differed per platform.
   - The table position was a member of a singleton shared by all chips and threads (a data race); it is now one
     position per thread (`static thread_local`), so a chip rendered on a fresh thread is reproducible.

Both patches change only the ±0.5 LSB dither noise, not the model.
