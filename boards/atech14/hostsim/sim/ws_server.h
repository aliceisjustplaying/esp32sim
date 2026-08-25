// Minimal dependency-free HTTP + WebSocket server (RFC 6455) for the virtual board UI.
#pragma once
#include <string>
#include <vector>
#include <mutex>
#include <functional>
#include <cstdint>

class WsServer {
public:
    using TextHandler = std::function<void(const std::string& msg)>;
    WsServer(int port, std::string webDir);
    bool start();                                   // spawns accept thread; false if bind fails
    void broadcastText(const std::string& s);
    void broadcastBinary(const std::vector<uint8_t>& b);
    void onText(TextHandler h) { _onText = std::move(h); }
    void onConnect(std::function<void(int fd)> h) { _onConnect = std::move(h); }
    void sendText(int fd, const std::string& s);
    void sendBinary(int fd, const std::vector<uint8_t>& b);
    int port() const { return _port; }
private:
    void acceptLoop();
    void clientLoop(int fd);
    bool serveHttp(int fd, const std::string& req);
    void sendFrame(int fd, uint8_t opcode, const uint8_t* data, size_t n);
    int _port; std::string _webDir; int _listenFd = -1;
    std::mutex _m; std::vector<int> _clients;
    TextHandler _onText; std::function<void(int)> _onConnect;
};
