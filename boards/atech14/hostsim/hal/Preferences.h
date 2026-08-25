// File-backed stand-in for the ESP32 Preferences (NVS) API.
#pragma once
#include <cstdint>
#include <string>
#include <map>

class Preferences {
public:
    bool begin(const char* ns, bool readOnly = false, const char* partition = nullptr);
    void end();
    bool clear();
    uint32_t getUInt(const char* key, uint32_t def = 0);
    size_t putUInt(const char* key, uint32_t v);
    int32_t getInt(const char* key, int32_t def = 0);
    size_t putInt(const char* key, int32_t v);
    float getFloat(const char* key, float def = 0);
    size_t putFloat(const char* key, float v);
    std::string getString(const char* key, const std::string& def = "");
    size_t putString(const char* key, const std::string& v);
    bool isKey(const char* key);
private:
    std::string _path;
    std::map<std::string, std::string> _kv;
    bool _dirty = false;
    void save();
};
