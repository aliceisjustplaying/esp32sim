// Minimal SID-style synth core. See sid.h for an overview.

#include "sid.h"

#include <math.h>
#include <string.h>

#ifndef M_PI
#define M_PI 3.14159265358979323846
#endif

enum { SID_ENV_ATTACK = 0, SID_ENV_DECAY_SUSTAIN = 1, SID_ENV_RELEASE = 2 };

// reSID envelope rate-counter periods, indexed by the 4-bit attack/decay/release
// value. These set the SID-clock count between envelope steps and give the SID
// its characteristic ADSR timing.
static const uint16_t kRatePeriod[16] = {
    9, 32, 63, 95, 149, 220, 267, 313, 392, 977, 1954, 3126, 3907, 11720, 19532, 31251,
};

static void recompute_inc(sid_t *sid, sid_voice_t *v)
{
    // Register frequency -> per-output-sample 24-bit accumulator increment.
    // The chip adds `freq` to the accumulator every chip clock, so per output
    // sample we advance by freq * clock / sample_rate.
    v->inc = (uint32_t)(((uint64_t)v->freq * sid->clock_hz) / sid->sample_rate);
}

void sid_init(sid_t *sid, uint32_t sample_rate, uint32_t clock_hz)
{
    memset(sid, 0, sizeof(*sid));
    sid->sample_rate = sample_rate ? sample_rate : 24000u;
    sid->clock_hz = clock_hz ? clock_hz : SID_CLOCK_PAL_HZ;
    sid_reset(sid);
}

void sid_reset(sid_t *sid)
{
    for (int i = 0; i < SID_NUM_VOICES; i++) {
        sid_voice_t *v = &sid->voice[i];
        memset(v, 0, sizeof(*v));
        v->noise_lfsr = 0x7ffff8u; // canonical non-zero seed
        v->env_state = SID_ENV_RELEASE;
        v->exp_period = 1;
        v->hold_zero = true;
        v->rate_period = kRatePeriod[0];
    }
    sid->fc = 0;
    sid->res_filt = 0;
    sid->mode_vol = 0x0f; // full volume, no filter modes
    sid->f_ic1 = sid->f_ic2 = 0.0f;
    sid->f_dirty = true;
}

// ---- envelope ---------------------------------------------------------------

static void env_set_exp_period(sid_voice_t *v)
{
    switch (v->envelope) {
    case 0xff: v->exp_period = 1; break;
    case 0x5d: v->exp_period = 2; break;
    case 0x36: v->exp_period = 4; break;
    case 0x1a: v->exp_period = 8; break;
    case 0x0e: v->exp_period = 16; break;
    case 0x06: v->exp_period = 30; break;
    case 0x00: v->exp_period = 1; v->hold_zero = true; break;
    default: break; // unchanged
    }
}

static inline void env_clock(sid_voice_t *v)
{
    if (++v->rate_counter != v->rate_period) {
        return;
    }
    v->rate_counter = 0;

    // In attack the envelope steps every rate period; in decay/release it is
    // gated by the exponential counter to approximate an exponential curve.
    if (v->env_state != SID_ENV_ATTACK && ++v->exp_counter != v->exp_period) {
        return;
    }
    v->exp_counter = 0;

    if (v->hold_zero) {
        return;
    }

    switch (v->env_state) {
    case SID_ENV_ATTACK:
        v->envelope++;
        if (v->envelope == 0xff) {
            v->env_state = SID_ENV_DECAY_SUSTAIN;
            v->rate_period = kRatePeriod[v->decay]; // switch attack rate -> decay rate
        }
        break;
    case SID_ENV_DECAY_SUSTAIN: {
        uint8_t sustain_level = (uint8_t)(v->sustain * 0x11);
        if (v->envelope > sustain_level) {
            v->envelope--;
        }
        break;
    }
    case SID_ENV_RELEASE:
        if (v->envelope > 0) {
            v->envelope--;
        }
        break;
    }
    env_set_exp_period(v);
}

static void env_set_rate(sid_voice_t *v)
{
    // Choose the active rate period for the current phase.
    switch (v->env_state) {
    case SID_ENV_ATTACK: v->rate_period = kRatePeriod[v->attack]; break;
    case SID_ENV_DECAY_SUSTAIN: v->rate_period = kRatePeriod[v->decay]; break;
    default: v->rate_period = kRatePeriod[v->release]; break;
    }
}

// ---- oscillator -------------------------------------------------------------

static inline uint16_t noise_output(uint32_t s)
{
    return (uint16_t)(((s >> 22) & 1) << 11 | ((s >> 20) & 1) << 10 | ((s >> 16) & 1) << 9 |
                      ((s >> 13) & 1) << 8 | ((s >> 11) & 1) << 7 | ((s >> 7) & 1) << 6 |
                      ((s >> 4) & 1) << 5 | ((s >> 2) & 1) << 4);
}

static inline uint32_t noise_advance(uint32_t s)
{
    uint32_t fb = ((s >> 22) ^ (s >> 17)) & 1u;
    return ((s << 1) | fb) & 0x7fffffu;
}

// 12-bit unsigned oscillator output (0..4095, centred at 2048).
static uint16_t osc_output(sid_t *sid, int idx)
{
    sid_voice_t *v = &sid->voice[idx];
    uint8_t ctrl = v->control;
    uint8_t wave = ctrl & 0xf0;
    if (wave == 0) {
        return 0x800; // no waveform -> silence (centre)
    }

    uint32_t acc = v->accumulator;
    uint16_t out = 0xfff; // AND-combine selected waveforms (as the real chip does)

    if (ctrl & SID_CTRL_TRIANGLE) {
        uint32_t src = acc;
        if (ctrl & SID_CTRL_RING) {
            src ^= sid->voice[(idx + 2) % SID_NUM_VOICES].accumulator; // previous voice
        }
        uint32_t msb = src & 0x800000u;
        uint16_t tri = (uint16_t)(((msb ? ~acc : acc) >> 11) & 0xfff);
        out &= tri;
    }
    if (ctrl & SID_CTRL_SAWTOOTH) {
        out &= (uint16_t)((acc >> 12) & 0xfff);
    }
    if (ctrl & SID_CTRL_PULSE) {
        out &= ((acc >> 12) >= v->pw) ? 0xfff : 0x000;
    }
    if (ctrl & SID_CTRL_NOISE) {
        out &= noise_output(v->noise_lfsr);
    }
    return out;
}

static void osc_advance(sid_t *sid, int idx)
{
    sid_voice_t *v = &sid->voice[idx];
    if (v->control & SID_CTRL_TEST) {
        v->accumulator = 0; // TEST holds the oscillator reset
        return;
    }

    uint32_t old_acc = v->accumulator;
    // `inc` already spans one whole output sample (freq * clock / sample_rate).
    v->accumulator = (old_acc + v->inc) & 0xffffffu;

    // Hard sync: reset when the sync-source voice's MSB rose during this step.
    if (v->control & SID_CTRL_SYNC) {
        sid_voice_t *src = &sid->voice[(idx + 2) % SID_NUM_VOICES];
        bool msb = (src->accumulator & 0x800000u) != 0;
        if (msb && !src->prev_msb) {
            v->accumulator = 0;
        }
    }

    // Clock the noise LFSR once per rising edge of accumulator bit 19.
    if (v->control & SID_CTRL_NOISE) {
        uint32_t steps = ((v->accumulator >> 19) - (old_acc >> 19)) & 0x1f;
        for (uint32_t i = 0; i < steps; i++) {
            v->noise_lfsr = noise_advance(v->noise_lfsr);
        }
    }
}

// ---- filter -----------------------------------------------------------------

static void filter_update_coeffs(sid_t *sid)
{
    // 11-bit cutoff -> ~30 Hz..~15 kHz (musical, exponential).
    float fc_hz = 30.0f * powf(2.0f, ((float)sid->fc / 2047.0f) * 9.0f);
    float nyq = (float)sid->sample_rate * 0.49f;
    if (fc_hz > nyq) {
        fc_hz = nyq;
    }
    float res = (float)(sid->res_filt >> 4); // 0..15
    float Q = 0.707f + (res / 15.0f) * 6.0f; // 0.707..~6.7
    float g = tanf((float)M_PI * fc_hz / (float)sid->sample_rate);
    float k = 1.0f / Q;
    sid->f_g = g;
    sid->f_k = k;
    sid->f_a1 = 1.0f / (1.0f + g * (g + k));
    sid->f_a2 = g * sid->f_a1;
    sid->f_a3 = g * sid->f_a2;
    sid->f_dirty = false;
}

// ---- register interface -----------------------------------------------------

void sid_write(sid_t *sid, uint8_t reg, uint8_t value)
{
    if (reg < 0x15) {
        int idx = reg / 7;
        int r = reg % 7;
        if (idx >= SID_NUM_VOICES) {
            return;
        }
        sid_voice_t *v = &sid->voice[idx];
        switch (r) {
        case 0: v->freq = (v->freq & 0xff00) | value; recompute_inc(sid, v); break;
        case 1: v->freq = (v->freq & 0x00ff) | ((uint32_t)value << 8); recompute_inc(sid, v); break;
        case 2: v->pw = (v->pw & 0x0f00) | value; break;
        case 3: v->pw = (v->pw & 0x00ff) | ((uint32_t)(value & 0x0f) << 8); break;
        case 4: {
            bool was_gated = (v->control & SID_CTRL_GATE) != 0;
            bool now_gated = (value & SID_CTRL_GATE) != 0;
            v->control = value;
            if (now_gated && !was_gated) {
                v->env_state = SID_ENV_ATTACK;
                v->hold_zero = false;
            } else if (!now_gated && was_gated) {
                v->env_state = SID_ENV_RELEASE;
            }
            env_set_rate(v);
            break;
        }
        case 5: v->attack = value >> 4; v->decay = value & 0x0f; env_set_rate(v); break;
        case 6: v->sustain = value >> 4; v->release = value & 0x0f; env_set_rate(v); break;
        }
        return;
    }

    switch (reg) {
    case 0x15: sid->fc = (uint16_t)((sid->fc & 0x7f8) | (value & 0x07)); sid->f_dirty = true; break;
    case 0x16: sid->fc = (uint16_t)((sid->fc & 0x007) | ((uint16_t)value << 3)); sid->f_dirty = true; break;
    case 0x17: sid->res_filt = value; sid->f_dirty = true; break;
    case 0x18: sid->mode_vol = value; break;
    default: break;
    }
}

// ---- rendering --------------------------------------------------------------

static inline int voice_signed(sid_t *sid, int idx)
{
    // 12-bit osc centred at 2048 -> signed, scaled by envelope (0..255).
    int osc = (int)osc_output(sid, idx) - 0x800;
    return (osc * (int)sid->voice[idx].envelope) / 255;
}

int16_t sid_sample(sid_t *sid)
{
    // Number of SID clocks represented by one output sample (fixed 16.16),
    // with the fractional part carried in sid->cycle_carry so envelope timing
    // stays exact over time.
    const uint32_t kFpShift = 16;
    uint32_t cyc_fp = (uint32_t)(((uint64_t)sid->clock_hz << kFpShift) / sid->sample_rate);
    uint32_t total = sid->cycle_carry + cyc_fp;
    uint32_t cycles = total >> kFpShift;
    sid->cycle_carry = total & ((1u << kFpShift) - 1u);
    if (cycles == 0) {
        cycles = 1;
    }

    // Advance oscillators (one output sample) then clock envelopes per SID clock.
    for (int i = 0; i < SID_NUM_VOICES; i++) {
        osc_advance(sid, i);
    }
    for (uint32_t c = 0; c < cycles; c++) {
        for (int i = 0; i < SID_NUM_VOICES; i++) {
            env_clock(&sid->voice[i]);
        }
    }
    for (int i = 0; i < SID_NUM_VOICES; i++) {
        sid->voice[i].prev_msb = (sid->voice[i].accumulator & 0x800000u) != 0;
    }

    // Route voices to filter / direct paths.
    if (sid->f_dirty) {
        filter_update_coeffs(sid);
    }
    int direct = 0;
    int filt_in_i = 0;
    for (int i = 0; i < SID_NUM_VOICES; i++) {
        int s = voice_signed(sid, i);
        bool routed = (sid->res_filt & (1u << i)) != 0;
        if (routed) {
            filt_in_i += s;
        } else if (i == 2 && (sid->mode_vol & SID_FILT_3OFF)) {
            // Voice 3 disconnected when 3OFF is set and it is not filtered.
        } else {
            direct += s;
        }
    }

    // State-variable filter (TPT).
    float out_filt = 0.0f;
    if (sid->mode_vol & (SID_FILT_LP | SID_FILT_BP | SID_FILT_HP)) {
        float in = (float)filt_in_i;
        float v3 = in - sid->f_ic2;
        float v1 = sid->f_a1 * sid->f_ic1 + sid->f_a2 * v3;
        float v2 = sid->f_ic2 + sid->f_a2 * sid->f_ic1 + sid->f_a3 * v3;
        sid->f_ic1 = 2.0f * v1 - sid->f_ic1;
        sid->f_ic2 = 2.0f * v2 - sid->f_ic2;
        float lp = v2, bp = v1, hp = in - sid->f_k * v1 - v2;
        if (sid->mode_vol & SID_FILT_LP) out_filt += lp;
        if (sid->mode_vol & SID_FILT_BP) out_filt += bp;
        if (sid->mode_vol & SID_FILT_HP) out_filt += hp;
    } else {
        // Filter modes all off: routed voices are silenced (as on the real chip
        // the filter output is the only path for routed voices).
        out_filt = 0.0f;
    }

    int master = sid->mode_vol & 0x0f;
    // Scale to int16 with headroom for up to three summed voices.
    float mixed = ((float)direct + out_filt) * (float)master * (5.0f / 15.0f);
    if (mixed > 32767.0f) mixed = 32767.0f;
    if (mixed < -32768.0f) mixed = -32768.0f;
    return (int16_t)mixed;
}

void sid_render(sid_t *sid, int16_t *buf, size_t frames)
{
    for (size_t i = 0; i < frames; i++) {
        buf[i] = sid_sample(sid);
    }
}

// ---- convenience helpers ----------------------------------------------------

void sid_set_freq_reg(sid_t *sid, int voice, uint16_t freq)
{
    sid_write(sid, (uint8_t)(voice * 7 + 0), (uint8_t)(freq & 0xff));
    sid_write(sid, (uint8_t)(voice * 7 + 1), (uint8_t)(freq >> 8));
}

void sid_set_freq_hz(sid_t *sid, int voice, float hz)
{
    // Inverse of the register->Hz mapping: freq = hz * 2^24 / clock.
    float f = hz * 16777216.0f / (float)sid->clock_hz;
    if (f < 0.0f) f = 0.0f;
    if (f > 65535.0f) f = 65535.0f;
    sid_set_freq_reg(sid, voice, (uint16_t)(f + 0.5f));
}

void sid_set_pulse_width(sid_t *sid, int voice, float duty)
{
    if (duty < 0.0f) duty = 0.0f;
    if (duty > 1.0f) duty = 1.0f;
    uint16_t pw = (uint16_t)(duty * 4095.0f + 0.5f);
    sid_write(sid, (uint8_t)(voice * 7 + 2), (uint8_t)(pw & 0xff));
    sid_write(sid, (uint8_t)(voice * 7 + 3), (uint8_t)((pw >> 8) & 0x0f));
}

void sid_set_control(sid_t *sid, int voice, uint8_t control)
{
    sid_write(sid, (uint8_t)(voice * 7 + 4), control);
}

void sid_set_adsr(sid_t *sid, int voice, uint8_t a, uint8_t d, uint8_t s, uint8_t r)
{
    sid_write(sid, (uint8_t)(voice * 7 + 5), (uint8_t)((a << 4) | (d & 0x0f)));
    sid_write(sid, (uint8_t)(voice * 7 + 6), (uint8_t)((s << 4) | (r & 0x0f)));
}

void sid_gate(sid_t *sid, int voice, bool on)
{
    uint8_t ctrl = sid->voice[voice].control;
    if (on) {
        ctrl |= SID_CTRL_GATE;
    } else {
        ctrl &= (uint8_t)~SID_CTRL_GATE;
    }
    sid_set_control(sid, voice, ctrl);
}

void sid_set_filter(sid_t *sid, uint16_t cutoff, uint8_t resonance, uint8_t routing, uint8_t mode_vol)
{
    sid_write(sid, 0x15, (uint8_t)(cutoff & 0x07));
    sid_write(sid, 0x16, (uint8_t)((cutoff >> 3) & 0xff));
    sid_write(sid, 0x17, (uint8_t)(((resonance & 0x0f) << 4) | (routing & 0x0f)));
    sid_write(sid, 0x18, mode_vol);
}

void sid_set_volume(sid_t *sid, uint8_t vol)
{
    sid->mode_vol = (uint8_t)((sid->mode_vol & 0xf0) | (vol & 0x0f));
}
