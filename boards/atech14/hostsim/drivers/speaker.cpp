#include "modules/audio/speaker.h"
#include "modules/shared/atech_helpers.h"
#include "../sim/board.h"
#include <condition_variable>
#include <chrono>
#include <cmath>

static std::condition_variable_any g_qcv;

Speaker::Speaker(int b, int l, int d) : _bclk(b), _lrc(l), _dout(d) {}

void Speaker::begin(int sampleRate) {
    _rate = sampleRate;
    _initialized = true;
    _thread = std::thread([this] { worker(); });
    _thread.detach();
    ATECH_SIM_LOG("AUDIO:init port=0 rate=%d bits=16", _rate);
    ATECH_SIM_LOG("AUDIO:pins port=0 bclk=%d lrclk=%d dout=%d", _bclk, _lrc, _dout);
    Serial.printf("[Speaker] I2S initialized on port 0 (BCLK=%d LRC=%d DOUT=%d) @ %d Hz\n", _bclk, _lrc, _dout, _rate);
}

void Speaker::setVolume(float v) { _volume = constrain(v, 0.0f, 1.0f); }
float Speaker::getVolume() { return _volume; }
bool Speaker::isPlaying() { return _playing; }

void Speaker::analyse(int16_t v) {
    const uint32_t WINDOW = 2048;
    if (_prev < 0 && v >= 0) _winCrossings++;
    _prev = v;
    _winEnergy += (double)v * v;
    if (++_winSamples >= WINDOW) {
        float rms = sqrtf((float)(_winEnergy / _winSamples)) / 32768.0f;
        float f = (float)_winCrossings * _rate / _winSamples;
        if (rms > 0.005f && f > 20.0f) {
            static const char* NAMES[12] = {"C","C#","D","D#","E","F","F#","G","G#","A","A#","B"};
            int semis = (int)lroundf(12.0f * log2f(f / 440.0f));
            int idx = ((semis + 9) % 12 + 12) % 12;
            int oct = 4 + (int)floorf((semis + 9) / 12.0f);
            ATECH_SIM_LOG("AUDIO:note=%s%d f=%d rms=%.2f", NAMES[idx], oct, (int)f, rms);
        }
        _winSamples = 0; _winCrossings = 0; _winEnergy = 0;
    }
}

// Paced like I2S DMA: a write returns when the audio it carries would have played.
void Speaker::emit(const int16_t* s, size_t n) {
    std::lock_guard<std::mutex> l(_outm);
    for (size_t i = 0; i < n; i++) analyse(s[i]);
    VirtualBoard::get().pushAudio(s, n, _rate);
    std::this_thread::sleep_for(std::chrono::microseconds((int64_t)n * 1000000 / _rate));
}

void Speaker::writeSamples(const int16_t* samples, int count) {
    if (!_initialized || count <= 0) return;
    int16_t buf[256];
    int done = 0;
    float vol = _volume;
    while (done < count) {
        int chunk = std::min(256, count - done);
        for (int i = 0; i < chunk; i++) buf[i] = (int16_t)(samples[done + i] * vol);   // real driver applies volume here
        emit(buf, chunk);
        done += chunk;
    }
}

void Speaker::queue(const Req& r) {
    if (!_initialized) return;
    { std::lock_guard<std::mutex> l(_qm); _q.push_back(r); }
    g_qcv.notify_all();
}
void Speaker::playTone(float f, int ms) { Req r{}; r.freqs[0] = f; r.n = 1; r.durationMs = ms; r.gap = false; queue(r); }
void Speaker::playNote(float f, int ms) { Req r{}; r.freqs[0] = f; r.n = 1; r.durationMs = ms; r.gap = true; queue(r); }
void Speaker::playChord(const float* fs, int n, int ms) {
    Req r{}; r.n = std::min(n, MAX_CHORD_NOTES); for (int i = 0; i < r.n; i++) r.freqs[i] = fs[i]; r.durationMs = ms; r.gap = false; queue(r);
}
void Speaker::stop() {
    { std::lock_guard<std::mutex> l(_qm); _q.clear(); }
    _stop = true;
    ATECH_SIM_LOG("AUDIO:stop");
}

void Speaker::worker() {
    while (true) {
        Req r;
        {
            std::unique_lock<std::mutex> l(_qm);
            g_qcv.wait(l, [this] { return !_q.empty(); });
            r = _q.front(); _q.pop_front();
        }
        _stop = false; _playing = true;
        render(r);
        _playing = false;
    }
}

void Speaker::render(const Req& r) {
    int soundMs = r.gap ? r.durationMs * 85 / 100 : r.durationMs;
    long total = (long)_rate * soundMs / 1000;
    double phase[MAX_CHORD_NOTES] = {0};
    int16_t buf[256];
    long done = 0;
    float vol = _volume;
    long fade = _rate / 200;   // 5 ms
    while (done < total && !_stop) {
        int chunk = (int)std::min(256L, total - done);
        for (int i = 0; i < chunk; i++) {
            double mix = 0;
            for (int k = 0; k < r.n; k++) {
                if (r.freqs[k] <= 0) continue;
                mix += sin(phase[k]);
                phase[k] += 2.0 * M_PI * r.freqs[k] / _rate;
                if (phase[k] > 2 * M_PI) phase[k] -= 2 * M_PI;
            }
            long pos = done + i; double env = 1.0;
            if (pos < fade) env = (double)pos / fade; else if (total - pos < fade) env = (double)(total - pos) / fade;
            buf[i] = (int16_t)(mix / (r.n ? r.n : 1) * env * vol * 30000.0);
        }
        emit(buf, chunk);
        done += chunk;
    }
    if (r.gap && !_stop) std::this_thread::sleep_for(std::chrono::milliseconds(r.durationMs - soundMs));
}

void Speaker::playPCMBase64(const char*) { Serial.println("[Speaker] playPCMBase64 not supported in hostsim"); }

// Minimal RTTTL: "name:d=4,o=5,b=140:c,e,g,2c6"
void Speaker::playRTTTL(const char* rtttl) {
    std::string s = rtttl ? rtttl : "";
    auto c1 = s.find(':'); if (c1 == std::string::npos) return;
    auto c2 = s.find(':', c1 + 1); if (c2 == std::string::npos) return;
    int d = 4, o = 5, b = 63;
    std::string defs = s.substr(c1 + 1, c2 - c1 - 1);
    for (size_t p = 0; p < defs.size();) {
        auto e = defs.find(',', p); if (e == std::string::npos) e = defs.size();
        std::string kv = defs.substr(p, e - p);
        if (kv.size() > 2) { int v = atoi(kv.c_str() + 2); if (kv[0] == 'd') d = v; else if (kv[0] == 'o') o = v; else if (kv[0] == 'b') b = v; }
        p = e + 1;
    }
    int whole = 60000 * 4 / b;
    std::string notes = s.substr(c2 + 1);
    static const int base[7] = {NOTE_C4, NOTE_D4, NOTE_E4, NOTE_F4, NOTE_G4, NOTE_A4, NOTE_B4};
    for (size_t p = 0; p < notes.size();) {
        auto e = notes.find(',', p); if (e == std::string::npos) e = notes.size();
        std::string n = notes.substr(p, e - p); p = e + 1;
        size_t i = 0; int dur = d;
        if (i < n.size() && isdigit(n[i])) { dur = 0; while (i < n.size() && isdigit(n[i])) dur = dur * 10 + (n[i++] - '0'); }
        if (i >= n.size()) continue;
        char note = tolower(n[i++]); bool sharp = (i < n.size() && n[i] == '#'); if (sharp) i++;
        bool dotted = (i < n.size() && n[i] == '.'); if (dotted) i++;
        int oct = o; if (i < n.size() && isdigit(n[i])) oct = n[i++] - '0';
        if (i < n.size() && n[i] == '.') dotted = true;
        int ms = whole / dur; if (dotted) ms += ms / 2;
        float f = 0;
        if (note >= 'a' && note <= 'g') {
            static const int idx[7] = {5, 6, 0, 1, 2, 3, 4};   // a b c d e f g
            f = base[idx[note - 'a']] * powf(2.0f, oct - 4) * (sharp ? 1.0595f : 1.0f);
        }
        Req r{}; r.freqs[0] = f; r.n = 1; r.durationMs = ms; r.gap = true; queue(r);
    }
}
