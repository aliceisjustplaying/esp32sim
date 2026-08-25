#include "scenario.h"
#include "board.h"
#include "png.h"
#include <fstream>
#include <map>
#include <thread>
#include <cstdio>

static std::string trim(const std::string& s) {
    size_t a = s.find_first_not_of(" \t\r\n"), b = s.find_last_not_of(" \t\r\n");
    return a == std::string::npos ? "" : s.substr(a, b - a + 1);
}
static std::string unquote(const std::string& in) {
    std::string s = trim(in);
    if (s.size() >= 2 && s.front() == '"' && s.back() == '"') {
        std::string o;
        for (size_t i = 1; i + 1 < s.size(); i++) {
            if (s[i] == '\\' && i + 2 < s.size()) { char n = s[++i]; o += n == 'n' ? '\n' : n == 't' ? '\t' : n; }
            else o += s[i];
        }
        return o;
    }
    if (s.size() >= 2 && s.front() == '\'' && s.back() == '\'') return s.substr(1, s.size() - 2);
    return s;
}
static std::map<std::string, std::string> flowMap(const std::string& in) {   // { a: 1, b: "x" }
    std::map<std::string, std::string> m;
    std::string s = trim(in); if (s.size() < 2) return m; s = s.substr(1, s.size() - 2);
    size_t p = 0;
    while (p < s.size()) {
        size_t c = s.find(':', p); if (c == std::string::npos) break;
        std::string k = trim(s.substr(p, c - p)); size_t q = c + 1;
        while (q < s.size() && s[q] == ' ') q++;
        size_t e;
        if (q < s.size() && (s[q] == '"' || s[q] == '\'')) { e = s.find(s[q], q + 1); e = e == std::string::npos ? s.size() : e + 1; }
        else { e = s.find(',', q); if (e == std::string::npos) e = s.size(); }
        m[k] = unquote(s.substr(q, e - q));
        p = s.find(',', e); if (p == std::string::npos) break; p++;
    }
    return m;
}

ScenarioResult runScenario(const std::string& path, int stepTimeoutMs, const std::string& shotDir) {
    std::ifstream f(path);
    if (!f) return {false, "cannot open " + path, 0};
    std::vector<std::pair<std::string, std::string>> steps;
    std::string line, name;
    while (std::getline(f, line)) {
        std::string t = trim(line);
        if (t.rfind("name:", 0) == 0) name = unquote(t.substr(5));
        if (t.rfind("- ", 0) != 0) continue;
        t = t.substr(2);
        size_t c = t.find(':'); if (c == std::string::npos) continue;
        steps.push_back({trim(t.substr(0, c)), trim(t.substr(c + 1))});
    }
    auto& b = VirtualBoard::get();
    fprintf(stderr, "[scenario] %s (%zu steps)\n", name.c_str(), steps.size());
    int n = 0;
    for (auto& st : steps) {
        n++;
        const std::string& k = st.first; const std::string& v = st.second;
        if (k == "wait-serial") {
            std::string needle = unquote(v);
            if (!b.waitSerial(needle, stepTimeoutMs)) return {false, "step " + std::to_string(n) + ": wait-serial timed out waiting for \"" + needle + "\"", n};
            fprintf(stderr, "[scenario]   ok  wait-serial \"%s\"\n", needle.c_str());
        } else if (k == "delay") {
            std::string d = unquote(v); int ms = atoi(d.c_str()); if (d.find("ms") == std::string::npos && d.find('s') != std::string::npos) ms *= 1000;
            std::this_thread::sleep_for(std::chrono::milliseconds(ms));
        } else if (k == "write-serial") {
            b.serialIn(unquote(v));
        } else if (k == "set-control") {
            auto m = flowMap(v);
            std::string part = m["part-id"], ctl = m.count("control") ? m["control"] : m["name"]; int val = atoi(m["value"].c_str());
            if (part == "knob" || part == "encoder") {
                auto* e = b.encoder(); if (!e) return {false, "step " + std::to_string(n) + ": no encoder registered", n};
                if (ctl == "rotate") e->hostRotate(val); else if (ctl == "pressed") e->hostSetPressed(val != 0);
                else return {false, "step " + std::to_string(n) + ": unknown knob control " + ctl, n};
            } else {
                int pin = b.buttonPinFor(part);
                if (pin < 0) return {false, "step " + std::to_string(n) + ": unknown part " + part, n};
                b.setPinLevel(pin, val ? 0 : 1);   // active-low buttons
            }
            fprintf(stderr, "[scenario]   set %s.%s = %d\n", part.c_str(), ctl.c_str(), val);
        } else if (k == "take-screenshot") {
            auto m = flowMap(v);
            std::string to = m["save-to"];
            std::string fn = to.substr(to.find_last_of('/') == std::string::npos ? 0 : to.find_last_of('/') + 1);
            std::vector<uint16_t> px; int w, h;
            if (!b.latestFrame(px, w, h)) return {false, "step " + std::to_string(n) + ": no frame to screenshot", n};
            std::string out = shotDir + "/" + fn;
            if (!writePngRgb565(out, px.data(), w, h, 3)) return {false, "step " + std::to_string(n) + ": cannot write " + out, n};
            fprintf(stderr, "[scenario]   screenshot -> %s\n", out.c_str());
        } else if (k == "expect-pin") {
            auto m = flowMap(v); int pin = atoi(m["pin"].c_str()); int exp = atoi(m["value"].c_str());
            if (b.pinLevel(pin) != exp) return {false, "step " + std::to_string(n) + ": pin " + std::to_string(pin) + " != " + std::to_string(exp), n};
        } else {
            fprintf(stderr, "[scenario]   (skipping unsupported step %s)\n", k.c_str());
        }
    }
    return {true, "passed", n};
}
