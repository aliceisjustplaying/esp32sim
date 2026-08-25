// Minimal MOS 6581/8580 "SID" style synth core.
//
// This is a small, portable, dependency-free sound engine inspired by the
// Commodore 64 SID chip. It is NOT a cycle-accurate emulation (see reSID for
// that); it aims to be tiny, allocation-free and good enough to design punchy
// alert/UI sounds and simple music on microcontrollers.
//
// Three voices, each with:
//   - a 24-bit phase-accumulator oscillator (triangle / sawtooth / pulse / noise)
//   - hard sync and ring modulation from the previous voice
//   - an ADSR envelope (SID rate table + exponential decay/release)
// One multimode (LP/BP/HP) state-variable filter with per-voice routing, and a
// master volume.
//
// Drive it either the "authentic" way by poking the 25 SID registers
// (sid_write / sid_poke), or via the convenience helpers (sid_set_*).
//
// The core uses `float` internally (for the filter) but performs no allocation
// and no I/O; render into an int16 buffer and hand that to your audio sink.

#ifndef SID_H
#define SID_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define SID_NUM_VOICES 3

// Default chip clock (PAL). Determines the register-frequency -> Hz mapping.
#define SID_CLOCK_PAL_HZ  985248u
#define SID_CLOCK_NTSC_HZ 1022730u

// Control-register (0x04 + voice*7) waveform / gate bits.
enum {
    SID_CTRL_GATE     = 0x01,
    SID_CTRL_SYNC     = 0x02,
    SID_CTRL_RING     = 0x04,
    SID_CTRL_TEST     = 0x08,
    SID_CTRL_TRIANGLE = 0x10,
    SID_CTRL_SAWTOOTH = 0x20,
    SID_CTRL_PULSE    = 0x40,
    SID_CTRL_NOISE    = 0x80,
};

// Filter mode bits within the mode/volume register (0x18).
enum {
    SID_FILT_LP  = 0x10, // low-pass
    SID_FILT_BP  = 0x20, // band-pass
    SID_FILT_HP  = 0x40, // high-pass
    SID_FILT_3OFF = 0x80, // mute voice 3 when it is not routed to the filter
};

typedef struct {
    // Register-mirrored oscillator state.
    uint32_t accumulator;   // 24-bit phase accumulator
    uint32_t freq;          // 16-bit frequency register
    uint32_t inc;           // precomputed per-sample accumulator increment
    uint32_t pw;            // 12-bit pulse width
    uint8_t  control;       // waveform/gate/sync/ring/test bits
    uint32_t noise_lfsr;    // 23-bit LFSR for the noise source
    bool     prev_msb;      // for hard-sync edge detection

    // ADSR envelope.
    uint8_t  attack, decay, sustain, release; // 4-bit each
    int      env_state;     // internal SID_ENV_* state
    uint16_t rate_counter;
    uint16_t rate_period;
    uint8_t  exp_counter;
    uint8_t  exp_period;
    uint8_t  envelope;      // 0..255 current envelope level
    bool     hold_zero;
} sid_voice_t;

typedef struct {
    uint32_t sample_rate;
    uint32_t clock_hz;

    sid_voice_t voice[SID_NUM_VOICES];

    // Filter / global registers.
    uint16_t fc;          // 11-bit cutoff
    uint8_t  res_filt;    // hi nibble resonance, lo nibble per-voice routing
    uint8_t  mode_vol;    // filter mode bits + 4-bit master volume

    // Filter state (TPT state-variable filter).
    float f_g, f_k, f_a1, f_a2, f_a3; // cached coefficients
    float f_ic1, f_ic2;               // integrator state
    bool  f_dirty;                    // recompute coefficients on next sample

    // Fractional carry (16.16) of SID clocks per output sample, so envelope
    // timing stays exact even when clock/sample_rate is non-integer.
    uint32_t cycle_carry;
} sid_t;

// Initialise a SID with the given output sample rate and chip clock (use
// SID_CLOCK_PAL_HZ if unsure). Clears all registers.
void sid_init(sid_t *sid, uint32_t sample_rate, uint32_t clock_hz);

// Reset all registers and voice/filter state (keeps sample rate & clock).
void sid_reset(sid_t *sid);

// Poke a SID register (reg 0x00..0x18), exactly like writing $D400+reg.
void sid_write(sid_t *sid, uint8_t reg, uint8_t value);

// Render one mono output sample in the range [-32768, 32767].
int16_t sid_sample(sid_t *sid);

// Render `frames` mono samples into buf.
void sid_render(sid_t *sid, int16_t *buf, size_t frames);

// ---- Convenience helpers (wrap sid_write) -----------------------------------

// Set a voice's oscillator frequency in Hz.
void sid_set_freq_hz(sid_t *sid, int voice, float hz);
// Set the raw 16-bit frequency register.
void sid_set_freq_reg(sid_t *sid, int voice, uint16_t freq);
// Set pulse width as a fraction 0.0..1.0 (only audible with the PULSE waveform).
void sid_set_pulse_width(sid_t *sid, int voice, float duty);
// Set the control register (waveform + gate/sync/ring/test bits).
void sid_set_control(sid_t *sid, int voice, uint8_t control);
// Set ADSR (each parameter 0..15).
void sid_set_adsr(sid_t *sid, int voice, uint8_t a, uint8_t d, uint8_t s, uint8_t r);
// Gate a voice on/off (start attack / start release) without touching waveform.
void sid_gate(sid_t *sid, int voice, bool on);
// Configure the filter: 11-bit cutoff, 4-bit resonance, routing bitmask
// (bit i routes voice i through the filter), and mode/volume register.
void sid_set_filter(sid_t *sid, uint16_t cutoff, uint8_t resonance, uint8_t routing, uint8_t mode_vol);
// Set master volume (0..15).
void sid_set_volume(sid_t *sid, uint8_t vol);

#ifdef __cplusplus
}
#endif

#endif // SID_H
