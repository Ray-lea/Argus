// Short-lived, owned external load: file I/O, loopback traffic and process churn.
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#include <windows.h>
#include <cstdlib>
#include <vector>

int wmain(int argc, wchar_t** argv) {
    if (argc != 3) return 2;
    const int port = _wtoi(argv[2]);
    if (port < 1 || port > 65535) return 2;
    WSADATA data;
    if (WSAStartup(MAKEWORD(2, 2), &data)) return 3;
    const auto socket_handle = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    sockaddr_in address{};
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = htons(static_cast<u_short>(port));
    const auto file = CreateFileW(argv[1], GENERIC_READ | GENERIC_WRITE, 0, nullptr, CREATE_NEW,
        FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_DELETE_ON_CLOSE, nullptr);
    if (file == INVALID_HANDLE_VALUE) { closesocket(socket_handle); WSACleanup(); return 4; }
    std::vector<char> buffer(64 * 1024, 0x5a);
    int result = 0;
    for (int i = 0; i < 16; ++i) {
        DWORD written;
        if (!WriteFile(file, buffer.data(), static_cast<DWORD>(buffer.size()), &written, nullptr) || written != buffer.size()) result = 5;
        if (sendto(socket_handle, buffer.data(), 1024, 0, reinterpret_cast<sockaddr*>(&address), sizeof(address)) != 1024) result = 6;
        Sleep(50);
    }
    CloseHandle(file);
    closesocket(socket_handle);
    WSACleanup();
    return result;
}
