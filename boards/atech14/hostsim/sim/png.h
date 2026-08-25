// Write an RGB565 frame as PNG (zlib for deflate + crc32).
#pragma once
#include <cstdint>
#include <string>
#include <vector>
bool writePngRgb565(const std::string& path, const uint16_t* px, int w, int h, int scale = 1);
