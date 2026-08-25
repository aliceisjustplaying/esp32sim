// hostsim — run Atech ESP32-S3 firmware natively with a local web UI.
//
//   hostsim [--port 8765] [--web DIR] [--headless] [--quiet]
//           [--scenario FILE [--scenario FILE ...]] [--step-timeout MS] [--screenshots DIR]
//
// Without --scenario it runs until Ctrl-C. With scenarios it runs them in order and exits
// 0 on success, 1 on failure.
#include "board.h"
#include "ws_server.h"
#include "scenario.h"
#include <thread>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>
#include <sys/stat.h>
#include <libgen.h>
#include <unistd.h>
#ifdef __APPLE__
#include <mach-o/dyld.h>
#endif

void setup();
void loop();

static std::string exeDir() {
    char buf[4096]; uint32_t n = sizeof buf;
#ifdef __APPLE__
    if (_NSGetExecutablePath(buf, &n) != 0) return ".";
#else
    ssize_t r = readlink("/proc/self/exe", buf, sizeof buf - 1); if (r < 0) return "."; buf[r] = 0;
#endif
    return dirname(buf);
}
static std::string jsonEscape(const std::string& s) {
    std::string o;
    for (unsigned char c : s) {
        if (c == '"') o += "\\\""; else if (c == '\\') o += "\\\\"; else if (c == '\n') o += "\\n"; else if (c == '\r') o += "\\r";
        else if (c < 0x20) { char b[8]; snprintf(b, sizeof b, "\\u%04x", c); o += b; } else o += c;
    }
    return o;
}
static std::string jsonStr(const std::string& msg, const char* key) {   // tiny extractor: "key":"value" or "key":number
    std::string k = "\"" + std::string(key) + "\""; auto p = msg.find(k); if (p == std::string::npos) return "";
    p = msg.find(':', p); if (p == std::string::npos) return ""; p++;
    while (p < msg.size() && msg[p] == ' ') p++;
    if (p < msg.size() && msg[p] == '"') {
        std::string o; p++;
        while (p < msg.size() && msg[p] != '"') { if (msg[p] == '\\' && p + 1 < msg.size()) { char n = msg[++p]; o += n == 'n' ? '\n' : n; } else o += msg[p]; p++; }
        return o;
    }
    size_t e = p; while (e < msg.size() && (isalnum(msg[e]) || msg[e] == '-' || msg[e] == '.')) e++;
    return msg.substr(p, e - p);
}

int main(int argc, char** argv) {
    int port = 8765, stepTimeout = 20000; bool headless = false, quiet = false;
    std::string web = exeDir() + "/web", shots = "screenshots";
    std::vector<std::string> scenarios;
    for (int i = 1; i < argc; i++) {
        std::string a = argv[i];
        auto next = [&]() -> std::string { return i + 1 < argc ? argv[++i] : ""; };
        if (a == "--port") port = atoi(next().c_str());
        else if (a == "--web") web = next();
        else if (a == "--headless") headless = true;
        else if (a == "--quiet") quiet = true;
        else if (a == "--scenario") scenarios.push_back(next());
        else if (a == "--step-timeout") stepTimeout = atoi(next().c_str());
        else if (a == "--screenshots") shots = next();
        else { fprintf(stderr, "unknown arg %s\n", a.c_str()); return 2; }
    }
    auto& board = VirtualBoard::get();
    board.quiet = quiet;
    WsServer* ws = nullptr;
    if (!headless) {
        ws = new WsServer(port, web);
        if (!ws->start()) return 2;
        board.addListener([ws](const BoardEvent& e) {
            if (e.type == "serial") ws->broadcastText("{\"t\":\"serial\",\"line\":\"" + jsonEscape(e.text) + "\"}");
            else if (e.type == "frame") { std::vector<uint8_t> b; b.push_back(1); b.insert(b.end(), e.bin.begin(), e.bin.end()); ws->broadcastBinary(b); }
            else if (e.type == "audio") { std::vector<uint8_t> b; b.push_back(2); b.insert(b.end(), e.bin.begin(), e.bin.end()); ws->broadcastBinary(b); }
            else ws->broadcastText(e.text);
        });
        ws->onConnect([ws, &board](int fd) {
            ws->sendText(fd, board.boardJson());
            RingState r = board.ring();
            char buf[160];
            snprintf(buf, sizeof buf, "{\"t\":\"ring\",\"r\":%d,\"g\":%d,\"b\":%d,\"bright\":%d,\"pos\":%.2f,\"enabled\":%s,\"leds\":%d}", r.r, r.g, r.b, r.bright, r.pos, r.enabled ? "true" : "false", r.leds);
            ws->sendText(fd, buf);
            std::vector<uint16_t> px; int w, h;
            if (board.latestFrame(px, w, h)) {
                std::vector<uint8_t> b{1, (uint8_t)w, (uint8_t)(w >> 8), (uint8_t)h, (uint8_t)(h >> 8)};
                for (auto p : px) { b.push_back((uint8_t)p); b.push_back((uint8_t)(p >> 8)); }
                ws->sendBinary(fd, b);
            }
        });
        ws->onText([&board](const std::string& m) {
            std::string t = jsonStr(m, "t");
            if (t == "btn") board.setPinLevel(atoi(jsonStr(m, "pin").c_str()), atoi(jsonStr(m, "v").c_str()) ? 0 : 1);
            else if (t == "knob") { if (auto* e = board.encoder()) e->hostRotate(atoi(jsonStr(m, "d").c_str())); }
            else if (t == "knobpress") { if (auto* e = board.encoder()) e->hostSetPressed(atoi(jsonStr(m, "v").c_str()) != 0); }
            else if (t == "serial") board.serialIn(jsonStr(m, "line") + "\n");
        });
        fprintf(stderr, "[hostsim] web UI: http://127.0.0.1:%d/\n", port);
    }
    mkdir(shots.c_str(), 0755);

    std::thread fw([] { setup(); while (true) loop(); });
    fw.detach();

    if (scenarios.empty()) { while (true) std::this_thread::sleep_for(std::chrono::hours(1)); }
    bool allOk = true;
    for (auto& s : scenarios) {
        ScenarioResult r = runScenario(s, stepTimeout, shots);
        fprintf(stderr, "[scenario] %s: %s\n", r.ok ? "PASS" : "FAIL", r.message.c_str());
        if (!r.ok) { allOk = false; break; }
    }
    fflush(stdout);
    _exit(allOk ? 0 : 1);
}
