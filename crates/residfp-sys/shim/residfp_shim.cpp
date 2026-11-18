// extern "C" shim over libresidfp's reSIDfp::residfp class, for the Rust side (crates/residfp-sys/src/lib.rs).
// Part of giana-native; GPL-2.0-or-later like the library it wraps.
//
// Every entry point catches C++ exceptions: none may cross into Rust.

#include "residfp/residfp.h"
#include "siddefs-fp.h"
#include "FilterModelConfig.h"

#include <new>

// One chip and its own position in the dither table of the filter model (see the patch in FilterModelConfig.h): the
// output of a chip then depends only on what that chip was given, not on other chips or threads.
struct GianaSid
{
    int noise = 0;
    reSIDfp::residfp sid;
};

namespace
{
// Points the filter model's dither at the chip's own counter while the chip runs.
struct Use
{
    int* before;
    explicit Use(GianaSid* g) : before(reSIDfp::noiseIndex) { reSIDfp::noiseIndex = &g->noise; }
    ~Use() { reSIDfp::noiseIndex = before; }
};

// The shared tables of both filter models are built with the first chip; build them once (thread-safe, a function
// static) with a chip that is thrown away, so that every real chip starts from the same dither position.
bool buildTables()
{
    GianaSid g;
    Use use(&g);
    return true;
}

void warmUp()
{
    static const bool built = buildTables();
    (void)built;
}
}

using residfp = GianaSid;

extern "C" {

residfp* giana_residfp_new(int model)
{
    try
    {
        model = model == 1 ? 1 : 0;
        warmUp();
        // the chip is built (its integrators take dithered values) with a counter of its own that starts at 0
        int start = 0;
        int* before = reSIDfp::noiseIndex;
        reSIDfp::noiseIndex = &start;
        GianaSid* g = nullptr;
        try
        {
            g = new GianaSid();
        }
        catch (...)
        {
            reSIDfp::noiseIndex = before;
            throw;
        }
        reSIDfp::noiseIndex = before;
        g->noise = start;
        Use use(g);
        if (!g->sid.setChipModel(model == 1 ? reSIDfp::CSG8580 : reSIDfp::MOS6581))
        {
            delete g;
            return nullptr;
        }
        return g;
    }
    catch (...)
    {
        return nullptr;
    }
}

void giana_residfp_delete(residfp* sid)
{
    delete sid;
}

void giana_residfp_reset(residfp* sid)
{
    Use use(sid);
    sid->sid.reset();
}

void giana_residfp_write(residfp* sid, int offset, unsigned char value)
{
    Use use(sid);
    sid->sid.write(offset, value);
}

unsigned char giana_residfp_peek(const residfp* sid, int offset)
{
    return sid->sid.peek(offset);
}

// method: 0 = DECIMATE, 1 = RESAMPLE (sinc), 2 = NONE
int giana_residfp_set_sampling(residfp* sid, double clock_hz, int method, double rate_hz)
{
    reSIDfp::SamplingMethod m = method == 0 ? reSIDfp::DECIMATE : method == 2 ? reSIDfp::NONE : reSIDfp::RESAMPLE;
    try
    {
        Use use(sid);
        return sid->sid.setSamplingParameters(clock_hz, m, rate_hz) ? 1 : 0;
    }
    catch (...)
    {
        return 0;
    }
}

// The caller provides room for ceil(cycles * rate / clock) + 1 samples.
int giana_residfp_clock(residfp* sid, unsigned int cycles, short* buf)
{
    Use use(sid);
    return sid->sid.clock(cycles, buf);
}

void giana_residfp_set_filter6581_curve(residfp* sid, double curve)
{
    Use use(sid);
    sid->sid.setFilter6581Curve(curve);
}

void giana_residfp_set_filter6581_range(residfp* sid, double range)
{
    Use use(sid);
    sid->sid.setFilter6581Range(range);
}

void giana_residfp_set_filter8580_curve(residfp* sid, double curve)
{
    Use use(sid);
    sid->sid.setFilter8580Curve(curve);
}

// 0 = AVERAGE, 1 = WEAK, 2 = STRONG
int giana_residfp_set_combined_waveforms(residfp* sid, int strength)
{
    reSIDfp::CombinedWaveforms c = strength == 1 ? reSIDfp::WEAK : strength == 2 ? reSIDfp::STRONG : reSIDfp::AVERAGE;
    Use use(sid);
    return sid->sid.setCombinedWaveforms(c) ? 1 : 0;
}

void giana_residfp_enable_filter(residfp* sid, int enable)
{
    Use use(sid);
    sid->sid.enableFilter(enable != 0);
}

void giana_residfp_enable_old_6581_caps(residfp* sid, int enable)
{
    Use use(sid);
    sid->sid.enableOld6581caps(enable != 0);
}

void giana_residfp_set_dac_leakage(residfp* sid, double level)
{
    Use use(sid);
    sid->sid.setDacLeakage(level);
}

void giana_residfp_set_offset6581(residfp* sid, double offset)
{
    Use use(sid);
    sid->sid.setOffset6581(offset);
}

void giana_residfp_set_dcb_res(residfp* sid, double res)
{
    Use use(sid);
    sid->sid.setDCBRes(res);
}

const char* giana_residfp_version(void)
{
    return residfp_version_string;
}

}
