#include "ws_server.h"
#include <sys/socket.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>
#include <unistd.h>
#include <thread>
#include <fstream>
#include <sstream>
#include <cstring>
#include <cstdio>
#include <algorithm>

// ---- SHA-1 (for the WebSocket handshake) — public-domain style compact implementation
static void sha1(const uint8_t* msg, size_t len, uint8_t out[20]) {
    uint32_t h[5] = {0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0};
    std::vector<uint8_t> m(msg, msg + len);
    m.push_back(0x80);
    while (m.size() % 64 != 56) m.push_back(0);
    uint64_t bits = (uint64_t)len * 8;
    for (int i = 7; i >= 0; i--) m.push_back((uint8_t)(bits >> (i * 8)));
    auto rol = [](uint32_t v, int b) { return (v << b) | (v >> (32 - b)); };
    for (size_t off = 0; off < m.size(); off += 64) {
        uint32_t w[80];
        for (int i = 0; i < 16; i++) w[i] = (m[off + 4 * i] << 24) | (m[off + 4 * i + 1] << 16) | (m[off + 4 * i + 2] << 8) | m[off + 4 * i + 3];
        for (int i = 16; i < 80; i++) w[i] = rol(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        uint32_t a = h[0], b = h[1], c = h[2], d = h[3], e = h[4];
        for (int i = 0; i < 80; i++) {
            uint32_t f, k;
            if (i < 20) { f = (b & c) | (~b & d); k = 0x5A827999; }
            else if (i < 40) { f = b ^ c ^ d; k = 0x6ED9EBA1; }
            else if (i < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8F1BBCDC; }
            else { f = b ^ c ^ d; k = 0xCA62C1D6; }
            uint32_t t = rol(a, 5) + f + e + k + w[i];
            e = d; d = c; c = rol(b, 30); b = a; a = t;
        }
        h[0] += a; h[1] += b; h[2] += c; h[3] += d; h[4] += e;
    }
    for (int i = 0; i < 5; i++) { out[4 * i] = h[i] >> 24; out[4 * i + 1] = h[i] >> 16; out[4 * i + 2] = h[i] >> 8; out[4 * i + 3] = h[i]; }
}
static std::string b64(const uint8_t* d, size_t n) {
    static const char* T = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string o;
    for (size_t i = 0; i < n; i += 3) {
        uint32_t v = d[i] << 16 | (i + 1 < n ? d[i + 1] << 8 : 0) | (i + 2 < n ? d[i + 2] : 0);
        o += T[v >> 18 & 63]; o += T[v >> 12 & 63];
        o += i + 1 < n ? T[v >> 6 & 63] : '='; o += i + 2 < n ? T[v & 63] : '=';
    }
    return o;
}

WsServer::WsServer(int port, std::string webDir) : _port(port), _webDir(std::move(webDir)) {}

bool WsServer::start() {
    _listenFd = socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; setsockopt(_listenFd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    sockaddr_in a{}; a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); a.sin_port = htons(_port);
    if (bind(_listenFd, (sockaddr*)&a, sizeof a) < 0) { perror("bind"); return false; }
    if (listen(_listenFd, 8) < 0) { perror("listen"); return false; }
    std::thread([this] { acceptLoop(); }).detach();
    return true;
}
void WsServer::acceptLoop() {
    while (true) {
        int fd = accept(_listenFd, nullptr, nullptr);
        if (fd < 0) continue;
        int one = 1; setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
        std::thread([this, fd] { clientLoop(fd); }).detach();
    }
}
static bool readAll(int fd, uint8_t* buf, size_t n) {
    size_t got = 0;
    while (got < n) { ssize_t r = recv(fd, buf + got, n - got, 0); if (r <= 0) return false; got += r; }
    return true;
}
static void writeAll(int fd, const void* buf, size_t n) {
    const uint8_t* p = (const uint8_t*)buf; size_t s = 0;
    while (s < n) { ssize_t w = send(fd, p + s, n - s, 0); if (w <= 0) return; s += w; }
}

bool WsServer::serveHttp(int fd, const std::string& req) {
    std::istringstream is(req); std::string method, path; is >> method >> path;
    if (path == "/" || path.empty()) path = "/index.html";
    if (path.find("..") != std::string::npos) path = "/index.html";
    std::ifstream f(_webDir + path, std::ios::binary);
    std::string body, status = "200 OK", type = "text/html; charset=utf-8";
    if (f) { std::stringstream ss; ss << f.rdbuf(); body = ss.str(); } else { status = "404 Not Found"; body = "not found"; type = "text/plain"; }
    if (path.size() > 3 && path.compare(path.size() - 3, 3, ".js") == 0) type = "application/javascript";
    if (path.size() > 4 && path.compare(path.size() - 4, 4, ".css") == 0) type = "text/css";
    std::string hdr = "HTTP/1.1 " + status + "\r\nContent-Type: " + type + "\r\nContent-Length: " + std::to_string(body.size()) + "\r\nConnection: close\r\n\r\n";
    writeAll(fd, hdr.data(), hdr.size()); writeAll(fd, body.data(), body.size());
    return false;
}

void WsServer::clientLoop(int fd) {
    std::string req; char buf[4096];
    while (req.find("\r\n\r\n") == std::string::npos) {
        ssize_t r = recv(fd, buf, sizeof buf, 0); if (r <= 0) { close(fd); return; }
        req.append(buf, r); if (req.size() > 65536) { close(fd); return; }
    }
    auto kpos = req.find("Sec-WebSocket-Key:");
    if (kpos == std::string::npos) { serveHttp(fd, req); close(fd); return; }
    std::string key = req.substr(kpos + 18); key = key.substr(0, key.find("\r\n"));
    key.erase(0, key.find_first_not_of(' ')); key.erase(key.find_last_not_of(" \t") + 1);
    key += "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    uint8_t dig[20]; sha1((const uint8_t*)key.data(), key.size(), dig);
    std::string resp = "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + b64(dig, 20) + "\r\n\r\n";
    writeAll(fd, resp.data(), resp.size());
    { std::lock_guard<std::mutex> l(_m); _clients.push_back(fd); }
    if (_onConnect) _onConnect(fd);
    while (true) {
        uint8_t h[2]; if (!readAll(fd, h, 2)) break;
        uint8_t op = h[0] & 0x0F; bool masked = h[1] & 0x80; uint64_t len = h[1] & 0x7F;
        if (len == 126) { uint8_t e[2]; if (!readAll(fd, e, 2)) break; len = e[0] << 8 | e[1]; }
        else if (len == 127) { uint8_t e[8]; if (!readAll(fd, e, 8)) break; len = 0; for (int i = 0; i < 8; i++) len = len << 8 | e[i]; }
        uint8_t mask[4] = {0, 0, 0, 0}; if (masked && !readAll(fd, mask, 4)) break;
        if (len > (1u << 20)) break;
        std::vector<uint8_t> data(len); if (len && !readAll(fd, data.data(), len)) break;
        if (masked) for (size_t i = 0; i < len; i++) data[i] ^= mask[i & 3];
        if (op == 8) break;
        if (op == 9) { sendFrame(fd, 10, data.data(), data.size()); continue; }
        if (op == 1 && _onText) _onText(std::string(data.begin(), data.end()));
    }
    { std::lock_guard<std::mutex> l(_m); _clients.erase(std::remove(_clients.begin(), _clients.end(), fd), _clients.end()); }
    close(fd);
}

void WsServer::sendFrame(int fd, uint8_t opcode, const uint8_t* data, size_t n) {
    uint8_t hdr[10]; size_t hl = 0;
    hdr[hl++] = 0x80 | opcode;
    if (n < 126) hdr[hl++] = (uint8_t)n;
    else if (n < 65536) { hdr[hl++] = 126; hdr[hl++] = n >> 8; hdr[hl++] = n & 255; }
    else { hdr[hl++] = 127; for (int i = 7; i >= 0; i--) hdr[hl++] = (uint8_t)((uint64_t)n >> (i * 8)); }
    std::lock_guard<std::mutex> l(_m);
    writeAll(fd, hdr, hl); writeAll(fd, data, n);
}
void WsServer::sendText(int fd, const std::string& s) { sendFrame(fd, 1, (const uint8_t*)s.data(), s.size()); }
void WsServer::sendBinary(int fd, const std::vector<uint8_t>& b) { sendFrame(fd, 2, b.data(), b.size()); }
void WsServer::broadcastText(const std::string& s) {
    std::vector<int> cs; { std::lock_guard<std::mutex> l(_m); cs = _clients; }
    for (int fd : cs) sendText(fd, s);
}
void WsServer::broadcastBinary(const std::vector<uint8_t>& b) {
    std::vector<int> cs; { std::lock_guard<std::mutex> l(_m); cs = _clients; }
    for (int fd : cs) sendBinary(fd, b);
}
