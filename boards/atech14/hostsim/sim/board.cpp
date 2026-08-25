#include "board.h"
#include <cstdio>
#include <chrono>

VirtualBoard& VirtualBoard::get() { static VirtualBoard b; return b; }

void VirtualBoard::pinMode(int pin, int mode) {
    std::lock_guard<std::mutex> l(_m);
    _pinMode[pin] = mode;
    if (!_pinLevel.count(pin)) _pinLevel[pin] = (mode == 2 /*INPUT_PULLUP*/) ? 1 : 0;
}
int VirtualBoard::pinLevel(int pin) {
    std::lock_guard<std::mutex> l(_m);
    auto it = _pinLevel.find(pin);
    return it == _pinLevel.end() ? 1 : it->second;
}
void VirtualBoard::setPinLevel(int pin, int level) {
    Isr isr{nullptr, nullptr, 0}; int old;
    {
        std::lock_guard<std::mutex> l(_m);
        old = _pinLevel.count(pin) ? _pinLevel[pin] : 1;
        _pinLevel[pin] = level;
        auto it = _isr.find(pin);
        if (it != _isr.end()) isr = it->second;
    }
    if (isr.fn && old != level) {
        bool fire = isr.mode == 1 /*CHANGE*/ || (isr.mode == 2 /*FALLING*/ && level == 0) || (isr.mode == 3 /*RISING*/ && level == 1);
        if (fire) isr.fn(isr.arg);
    }
}
void VirtualBoard::attachInterrupt(int pin, void (*fn)(void*), void* arg, int mode) {
    std::lock_guard<std::mutex> l(_m);
    _isr[pin] = Isr{fn, arg, mode};
}

void VirtualBoard::registerButton(int pin, const std::string& label) {
    { std::lock_guard<std::mutex> l(_m); _buttons.push_back({pin, label}); }
    emit(BoardEvent{"board", boardJson(), {}});
}
std::vector<std::pair<int, std::string>> VirtualBoard::buttons() { std::lock_guard<std::mutex> l(_m); return _buttons; }
int VirtualBoard::buttonPinFor(const std::string& id) {
    std::lock_guard<std::mutex> l(_m);
    if (id.rfind("gpio", 0) == 0) return atoi(id.c_str() + 4);
    if (id.rfind("btn", 0) == 0) { int i = atoi(id.c_str() + 3) - 1; if (i >= 0 && i < (int)_buttons.size()) return _buttons[i].first; }
    return -1;
}
std::string VirtualBoard::boardJson() {
    std::lock_guard<std::mutex> l(_m);
    std::string s = "{\"t\":\"board\",\"buttons\":[";
    for (size_t i = 0; i < _buttons.size(); i++) {
        if (i) s += ",";
        s += "{\"pin\":" + std::to_string(_buttons[i].first) + ",\"label\":\"" + _buttons[i].second + "\"}";
    }
    s += "],\"encoder\":" + std::string(_encoder ? "true" : "false") + ",\"audioRate\":" + std::to_string(_audioRate) + "}";
    return s;
}

void VirtualBoard::emit(const BoardEvent& e) {
    std::vector<Listener> ls;
    { std::lock_guard<std::mutex> l(_m); ls = _listeners; }
    for (auto& l : ls) l(e);
}

void VirtualBoard::serialOut(const char* data, size_t n) {
    {
        std::lock_guard<std::mutex> l(_m);
        _capture.append(data, n);
        if (_capture.size() > (1u << 20)) { _capture.erase(0, _capture.size() - (1u << 19)); _captureCursor = 0; }
        _lineBuf.append(data, n);
    }
    _cv.notify_all();
    if (!quiet) { fwrite(data, 1, n, stdout); fflush(stdout); }
    // emit complete lines only, so the UI console gets whole lines
    std::vector<std::string> lines;
    {
        std::lock_guard<std::mutex> l(_m);
        size_t p;
        while ((p = _lineBuf.find('\n')) != std::string::npos) {
            std::string ln = _lineBuf.substr(0, p);
            if (!ln.empty() && ln.back() == '\r') ln.pop_back();
            lines.push_back(ln);
            _lineBuf.erase(0, p + 1);
        }
    }
    for (auto& ln : lines) emit(BoardEvent{"serial", ln, {}});
}
void VirtualBoard::simLog(const std::string& line) { std::string s = line + "\n"; serialOut(s.data(), s.size()); }
void VirtualBoard::serialIn(const std::string& text) {
    std::lock_guard<std::mutex> l(_m);
    for (char c : text) _rx.push_back(c);
}
int VirtualBoard::serialAvailable() { std::lock_guard<std::mutex> l(_m); return (int)_rx.size(); }
int VirtualBoard::serialRead() { std::lock_guard<std::mutex> l(_m); if (_rx.empty()) return -1; int c = (unsigned char)_rx.front(); _rx.pop_front(); return c; }
int VirtualBoard::serialPeek() { std::lock_guard<std::mutex> l(_m); return _rx.empty() ? -1 : (unsigned char)_rx.front(); }
bool VirtualBoard::waitSerial(const std::string& needle, int timeoutMs) {
    std::unique_lock<std::mutex> l(_m);
    auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeoutMs);
    while (true) {
        size_t p = _capture.find(needle, _captureCursor);
        if (p != std::string::npos) { _captureCursor = p + needle.size(); return true; }
        if (_cv.wait_until(l, deadline) == std::cv_status::timeout) {
            if (_capture.find(needle, _captureCursor) != std::string::npos) continue;
            return false;
        }
    }
}

void VirtualBoard::pushFrame(const uint16_t* px, int w, int h) {
    BoardEvent e{"frame", "", {}};
    {
        std::lock_guard<std::mutex> l(_m);
        _frame.assign(px, px + (size_t)w * h); _fw = w; _fh = h;
    }
    e.bin.resize(4 + (size_t)w * h * 2);
    e.bin[0] = (uint8_t)w; e.bin[1] = (uint8_t)(w >> 8); e.bin[2] = (uint8_t)h; e.bin[3] = (uint8_t)(h >> 8);
    for (size_t i = 0; i < (size_t)w * h; i++) { e.bin[4 + 2 * i] = (uint8_t)px[i]; e.bin[5 + 2 * i] = (uint8_t)(px[i] >> 8); }
    emit(e);
}
bool VirtualBoard::latestFrame(std::vector<uint16_t>& out, int& w, int& h) {
    std::lock_guard<std::mutex> l(_m);
    if (_frame.empty()) return false;
    out = _frame; w = _fw; h = _fh; return true;
}
void VirtualBoard::pushAudio(const int16_t* s, size_t n, int rate) {
    _audioRate = rate;
    BoardEvent e{"audio", "", {}};
    e.bin.resize(n * 2);
    for (size_t i = 0; i < n; i++) { e.bin[2 * i] = (uint8_t)s[i]; e.bin[2 * i + 1] = (uint8_t)(s[i] >> 8); }
    emit(e);
}
void VirtualBoard::setRing(const RingState& s) {
    { std::lock_guard<std::mutex> l(_m); _ring = s; }
    char buf[160];
    snprintf(buf, sizeof buf, "{\"t\":\"ring\",\"r\":%d,\"g\":%d,\"b\":%d,\"bright\":%d,\"pos\":%.2f,\"enabled\":%s,\"leds\":%d}",
             s.r, s.g, s.b, s.bright, s.pos, s.enabled ? "true" : "false", s.leds);
    emit(BoardEvent{"ring", buf, {}});
}
