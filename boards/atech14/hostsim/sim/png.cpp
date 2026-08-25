#include "png.h"
#include <zlib.h>
#include <cstdio>
#include <cstring>

static void put32(std::vector<uint8_t>& v, uint32_t x) { v.push_back(x >> 24); v.push_back(x >> 16); v.push_back(x >> 8); v.push_back(x); }
static void chunk(FILE* f, const char* type, const std::vector<uint8_t>& data) {
    std::vector<uint8_t> buf; put32(buf, (uint32_t)data.size());
    std::vector<uint8_t> td(type, type + 4); td.insert(td.end(), data.begin(), data.end());
    uint32_t crc = crc32(0, td.data(), (uInt)td.size());
    fwrite(buf.data(), 1, 4, f); fwrite(td.data(), 1, td.size(), f);
    std::vector<uint8_t> c; put32(c, crc); fwrite(c.data(), 1, 4, f);
}
bool writePngRgb565(const std::string& path, const uint16_t* px, int w, int h, int scale) {
    int W = w * scale, H = h * scale;
    std::vector<uint8_t> raw; raw.reserve((size_t)H * (W * 3 + 1));
    for (int y = 0; y < H; y++) {
        raw.push_back(0);
        for (int x = 0; x < W; x++) {
            uint16_t p = px[(y / scale) * w + x / scale];
            raw.push_back((p >> 11) * 255 / 31); raw.push_back(((p >> 5) & 63) * 255 / 63); raw.push_back((p & 31) * 255 / 31);
        }
    }
    uLongf clen = compressBound((uLong)raw.size());
    std::vector<uint8_t> comp(clen);
    if (compress2(comp.data(), &clen, raw.data(), (uLong)raw.size(), 6) != Z_OK) return false;
    comp.resize(clen);
    FILE* f = fopen(path.c_str(), "wb"); if (!f) return false;
    static const uint8_t sig[8] = {137, 80, 78, 71, 13, 10, 26, 10}; fwrite(sig, 1, 8, f);
    std::vector<uint8_t> ihdr; put32(ihdr, W); put32(ihdr, H); ihdr.push_back(8); ihdr.push_back(2); ihdr.push_back(0); ihdr.push_back(0); ihdr.push_back(0);
    chunk(f, "IHDR", ihdr); chunk(f, "IDAT", comp); chunk(f, "IEND", {});
    fclose(f); return true;
}
