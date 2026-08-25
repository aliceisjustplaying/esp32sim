// Host implementation of the Atech Speaker (MAX98357A / I2S) — same public API as the SDK driver.
// Audio goes to the VirtualBoard (→ browser WebAudio) and is analysed into SIM:AUDIO lines.
#pragma once
#include <Arduino.h>
#include <thread>
#include <mutex>
#include <atomic>
#include <deque>
#include <condition_variable>

#define NOTE_C3 131
#define NOTE_D3 147
#define NOTE_E3 165
#define NOTE_F3 175
#define NOTE_G3 196
#define NOTE_A3 220
#define NOTE_B3 247
#define NOTE_C4 262
#define NOTE_CS4 277
#define NOTE_D4 294
#define NOTE_DS4 311
#define NOTE_E4 330
#define NOTE_F4 349
#define NOTE_FS4 370
#define NOTE_G4 392
#define NOTE_GS4 415
#define NOTE_A4 440
#define NOTE_AS4 466
#define NOTE_B4 494
#define NOTE_C5 523
#define NOTE_CS5 554
#define NOTE_D5 587
#define NOTE_DS5 622
#define NOTE_E5 659
#define NOTE_F5 698
#define NOTE_FS5 740
#define NOTE_G5 784
#define NOTE_GS5 831
#define NOTE_A5 880
#define NOTE_AS5 932
#define NOTE_B5 988
#define NOTE_C6 1047
#define NOTE_D6 1175
#define NOTE_E6 1319
#define NOTE_F6 1397
#define NOTE_G6 1568
#define NOTE_A6 1760
#define NOTE_B6 1976
#define NOTE_REST 0
#define MAX_CHORD_NOTES 6

class Speaker {
public:
    static constexpr int DEFAULT_SAMPLE_RATE = 16000;
    Speaker(int bclkPin, int lrcPin, int doutPin);
    void begin(int sampleRate = DEFAULT_SAMPLE_RATE);
    void playTone(float freq, int durationMs);
    void playNote(float freq, int durationMs);
    void playChord(const float* freqs, int numFreqs, int durationMs);
    void stop();
    bool isPlaying();
    void setVolume(float vol);
    float getVolume();
    void writeSamples(const int16_t* samples, int count);   // raw mono; volume applied here (like the real driver)
    void playPCMBase64(const char* b64);
    void playRTTTL(const char* rtttl);
private:
    struct Req { float freqs[MAX_CHORD_NOTES]; int n; int durationMs; bool gap; };
    void queue(const Req& r);
    void worker();
    void render(const Req& r);
    void emit(const int16_t* s, size_t n);   // paced output + analysis
    int _bclk, _lrc, _dout, _rate = DEFAULT_SAMPLE_RATE;
    std::atomic<float> _volume{0.5f};
    std::atomic<bool> _playing{false}, _stop{false}, _initialized{false};
    std::mutex _qm; std::deque<Req> _q;
    std::thread _thread;
    // analysis
    uint32_t _winSamples = 0, _winCrossings = 0; double _winEnergy = 0; int16_t _prev = 0;
    void analyse(int16_t v);
    std::mutex _outm;
};
