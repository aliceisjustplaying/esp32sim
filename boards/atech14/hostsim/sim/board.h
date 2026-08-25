// VirtualBoard — the single meeting point between the firmware-facing HAL/drivers
// and the host side (web UI, scenario runner). Thread-safe.
#pragma once
#include <cstdint>
#include <string>
#include <vector>
#include <deque>
#include <map>
#include <mutex>
#include <condition_variable>
#include <functional>

struct RingState { uint8_t r = 0, g = 0, b = 0, bright = 50; float pos = 0; bool enabled = true; int leds = 12; };

struct EncoderHost {                       // implemented by the host RotaryEncoder driver
    virtual ~EncoderHost() {}
    virtual void hostRotate(int detents) = 0;
    virtual void hostSetPressed(bool down) = 0;
};

struct BoardEvent {                        // pushed to listeners (the web server)
    std::string type;                      // "serial" | "frame" | "audio" | "ring" | "board"
    std::string text;                      // serial text or JSON
    std::vector<uint8_t> bin;              // frame (RGB565 LE) or audio (int16 LE mono)
};

class VirtualBoard {
public:
    static VirtualBoard& get();

    // --- GPIO
    void pinMode(int pin, int mode);
    int  pinLevel(int pin);
    void setPinLevel(int pin, int level);  // host side: buttons
    void attachInterrupt(int pin, void (*fn)(void*), void* arg, int mode);

    // --- registrations (drivers announce what exists)
    void registerButton(int pin, const std::string& label);
    std::vector<std::pair<int, std::string>> buttons();
    void registerEncoder(EncoderHost* e) { std::lock_guard<std::mutex> l(_m); _encoder = e; }
    EncoderHost* encoder() { std::lock_guard<std::mutex> l(_m); return _encoder; }
    int buttonPinFor(const std::string& partId);   // "btn1" -> first registered, "gpio17" -> 17, else -1

    // --- serial
    void serialOut(const char* data, size_t n);    // firmware -> host
    void serialIn(const std::string& text);        // host -> firmware
    int  serialAvailable();
    int  serialRead();
    int  serialPeek();
    bool waitSerial(const std::string& needle, int timeoutMs);   // scenario: consume up to and incl. match
    void simLog(const std::string& line);          // "SIM:..." lines, same channel as serial

    // --- display / audio / ring
    void pushFrame(const uint16_t* rgb565, int w, int h);
    bool latestFrame(std::vector<uint16_t>& out, int& w, int& h);
    void pushAudio(const int16_t* mono, size_t n, int rate);
    void setRing(const RingState& s);
    RingState ring() { std::lock_guard<std::mutex> l(_m); return _ring; }
    int audioRate() { return _audioRate; }

    // --- listeners
    using Listener = std::function<void(const BoardEvent&)>;
    void addListener(Listener l) { std::lock_guard<std::mutex> l2(_m); _listeners.push_back(std::move(l)); }
    std::string boardJson();

    bool quiet = false;                            // don't mirror serial to stdout

private:
    VirtualBoard() {}
    void emit(const BoardEvent& e);
    std::mutex _m;
    std::condition_variable _cv;
    std::map<int, int> _pinLevel, _pinMode;
    struct Isr { void (*fn)(void*); void* arg; int mode; };
    std::map<int, Isr> _isr;
    std::vector<std::pair<int, std::string>> _buttons;
    EncoderHost* _encoder = nullptr;
    std::deque<char> _rx;
    std::string _capture; size_t _captureCursor = 0;
    std::string _lineBuf;
    std::vector<uint16_t> _frame; int _fw = 0, _fh = 0;
    RingState _ring;
    int _audioRate = 44100;
    std::vector<Listener> _listeners;
};
