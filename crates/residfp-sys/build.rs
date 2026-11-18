//! Builds the vendored libresidfp 1.2.3 (`vendor/libresidfp-1.2.3`, see `vendor/README.md`) and the `extern "C"` shim
//! with the `cc` crate: no autotools, the two headers `configure` would generate are written by hand.

fn main() {
    let lib = "vendor/libresidfp-1.2.3/src";
    let sources = [
        "Dac.cpp",
        "EnvelopeGenerator.cpp",
        "ExternalFilter.cpp",
        "Filter.cpp",
        "Filter6581.cpp",
        "Filter8580.cpp",
        "FilterModelConfig.cpp",
        "FilterModelConfig6581.cpp",
        "FilterModelConfig8580.cpp",
        "Integrator6581.cpp",
        "Integrator8580.cpp",
        "OpAmp.cpp",
        "SID.cpp",
        "Spline.cpp",
        "State.cpp",
        "WaveformCalculator.cpp",
        "WaveformGenerator.cpp",
        "resample/SincResampler.cpp",
        "version.cc",
        "residfp/residfp.cpp",
    ];
    let mut build = cc::Build::new();
    // `std` gives -std=c++17 on GCC/Clang and /std:c++17 on MSVC; cc adds /EHsc for C++ on MSVC by itself
    build.cpp(true).std("c++17").include(lib).file("shim/residfp_shim.cpp");
    for s in sources {
        build.file(format!("{lib}/{s}"));
    }
    // always optimised: the chip model runs at about 1 MHz per chip, far too slow unoptimised (debug builds, tests)
    build.opt_level(2).warnings(false);
    if build.get_compiler().is_like_msvc() {
        // math constants from <cmath>, no min/max macros; /Zc:__cplusplus makes MSVC report the real C++ version
        // (otherwise `__cplusplus` stays 199711L and siddefs-fp.h stops with "This is not a C++11 compiler")
        build.define("_USE_MATH_DEFINES", None).define("NOMINMAX", None).flag("/Zc:__cplusplus");
    }
    build.compile("residfp");
    // the tables of the filter model are computed by a few std::threads when the first chip is made
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=pthread");
    }
    println!("cargo:rerun-if-changed=shim/residfp_shim.cpp");
    println!("cargo:rerun-if-changed=vendor/libresidfp-1.2.3/src");
}
