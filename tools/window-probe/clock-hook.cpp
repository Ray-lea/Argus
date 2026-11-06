// Test-only IAT clock replacement. The controller owns the isolated child;
// this DLL never changes the Windows clock or patches another module's code.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cwchar>
#include <cstring>

static LONGLONG base_qpc, frequency;
static ULONGLONG base_wall, base_tick;
static DWORD rate;
static volatile LONG64 qpc_calls, wall_calls, wait_calls;
static HANDLE log_file = INVALID_HANDLE_VALUE;

static LONGLONG elapsed() {
    LARGE_INTEGER now;
    QueryPerformanceCounter(&now);
    return now.QuadPart - base_qpc;
}
static ULONGLONG scaled_elapsed(ULONGLONG units) {
    return static_cast<ULONGLONG>(static_cast<long double>(elapsed()) * rate * units / frequency);
}
static DWORD delay(DWORD ms) {
    InterlockedIncrement64(&wait_calls);
    if (!ms || ms == INFINITE) return ms;
    return static_cast<DWORD>((static_cast<ULONGLONG>(ms) + rate - 1) / rate);
}
static BOOL WINAPI clock_qpc(LARGE_INTEGER* value) {
    InterlockedIncrement64(&qpc_calls);
    value->QuadPart = base_qpc + elapsed() * rate;
    return TRUE;
}
static void WINAPI clock_wall(FILETIME* value) {
    InterlockedIncrement64(&wall_calls);
    const auto ticks = base_wall + scaled_elapsed(10000000);
    value->dwLowDateTime = static_cast<DWORD>(ticks);
    value->dwHighDateTime = static_cast<DWORD>(ticks >> 32);
}
static ULONGLONG WINAPI clock_tick64() {
    return base_tick + scaled_elapsed(1000);
}
static DWORD WINAPI clock_tick() { return static_cast<DWORD>(clock_tick64()); }
static void WINAPI clock_sleep(DWORD ms) { Sleep(delay(ms)); }
static DWORD WINAPI clock_sleep_ex(DWORD ms, BOOL alertable) { return SleepEx(delay(ms), alertable); }
static DWORD WINAPI clock_wait(HANDLE h, DWORD ms) { return WaitForSingleObject(h, delay(ms)); }
static DWORD WINAPI clock_wait_ex(HANDLE h, DWORD ms, BOOL alertable) {
    return WaitForSingleObjectEx(h, delay(ms), alertable);
}
static DWORD WINAPI clock_wait_many(DWORD n, const HANDLE* h, BOOL all, DWORD ms) {
    return WaitForMultipleObjects(n, h, all, delay(ms));
}
static DWORD WINAPI clock_wait_many_ex(DWORD n, const HANDLE* h, BOOL all, DWORD ms, BOOL alertable) {
    return WaitForMultipleObjectsEx(n, h, all, delay(ms), alertable);
}
static BOOL WINAPI clock_address(volatile VOID* address, PVOID compare, SIZE_T size, DWORD ms) {
    return WaitOnAddress(address, compare, size, delay(ms));
}
static FILETIME scaled_due(const FILETIME& input) {
    const LONGLONG ticks = static_cast<LONGLONG>((static_cast<ULONGLONG>(input.dwHighDateTime) << 32) | input.dwLowDateTime);
    LONGLONG due;
    if (ticks < 0) due = ticks / rate - (ticks % rate != 0);
    else {
        const auto virtual_now = base_wall + scaled_elapsed(10000000);
        due = ticks > static_cast<LONGLONG>(virtual_now)
            ? -((ticks - static_cast<LONGLONG>(virtual_now) + rate - 1) / rate) : -1;
    }
    return {static_cast<DWORD>(due), static_cast<DWORD>(static_cast<ULONGLONG>(due) >> 32)};
}
static void WINAPI clock_pool_timer(PTP_TIMER timer, const FILETIME* due, DWORD period, DWORD window) {
    InterlockedIncrement64(&wait_calls);
    if (!due) { SetThreadpoolTimer(timer, nullptr, period, window); return; }
    auto scaled = scaled_due(*due);
    SetThreadpoolTimer(timer, &scaled, period ? delay(period) : 0, 0);
}
struct Hook { const char* name; void* replacement; };
#define HOOK(name, function) {name, reinterpret_cast<void*>(function)}
static Hook hooks[] = {
    HOOK("QueryPerformanceCounter", clock_qpc),
    HOOK("GetSystemTimeAsFileTime", clock_wall),
    HOOK("GetSystemTimePreciseAsFileTime", clock_wall),
    HOOK("GetTickCount64", clock_tick64), HOOK("GetTickCount", clock_tick),
    HOOK("Sleep", clock_sleep), HOOK("SleepEx", clock_sleep_ex),
    HOOK("WaitForSingleObject", clock_wait), HOOK("WaitForSingleObjectEx", clock_wait_ex),
    HOOK("WaitForMultipleObjects", clock_wait_many), HOOK("WaitForMultipleObjectsEx", clock_wait_many_ex),
    HOOK("WaitOnAddress", clock_address), HOOK("SetThreadpoolTimer", clock_pool_timer),
};

static DWORD WINAPI report(void*) {
    // This module retains the original imports, so supervision uses real time.
    for (unsigned i = 0; i < 3600; ++i) {
        char line[512];
        const auto real_ms = elapsed() * 1000 / frequency;
        const int length = std::snprintf(line, sizeof(line),
            "{\"realMs\":%lld,\"virtualMs\":%lld,\"rate\":%lu,\"qpcCalls\":%lld,\"wallCalls\":%lld,\"waitCalls\":%lld}\n",
            real_ms, real_ms * rate, rate,
            InterlockedCompareExchange64(&qpc_calls, 0, 0),
            InterlockedCompareExchange64(&wall_calls, 0, 0),
            InterlockedCompareExchange64(&wait_calls, 0, 0));
        DWORD written;
        if (!WriteFile(log_file, line, length, &written, nullptr)) break;
        Sleep(1000);
    }
    CloseHandle(log_file);
    return 0;
}

extern "C" __declspec(dllexport) DWORD WINAPI ArgusInstallClock(void*) {
    const auto command = GetCommandLineW();
    if (!wcsstr(command, L"--test") || !wcsstr(command, L"dev.argus.taskmanager.test.")) return 0;
    wchar_t value[32], path[32768];
    if (!GetEnvironmentVariableW(L"ARGUS_TEST_CLOCK_RATE", value, 32)) return 0;
    rate = wcstoul(value, nullptr, 10);
    if (rate < 1 || rate > 3600 || !GetEnvironmentVariableW(L"ARGUS_TEST_CLOCK_LOG", path, 32768)) return 0;
    log_file = CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ, nullptr, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (log_file == INVALID_HANDLE_VALUE) return 0;
    LARGE_INTEGER counter, hz;
    QueryPerformanceCounter(&counter); QueryPerformanceFrequency(&hz);
    base_qpc = counter.QuadPart; frequency = hz.QuadPart;
    FILETIME wall; GetSystemTimePreciseAsFileTime(&wall);
    base_wall = (static_cast<ULONGLONG>(wall.dwHighDateTime) << 32) | wall.dwLowDateTime;
    base_tick = GetTickCount64();
    auto base = reinterpret_cast<BYTE*>(GetModuleHandleW(nullptr));
    auto dos = reinterpret_cast<IMAGE_DOS_HEADER*>(base);
    auto nt = reinterpret_cast<IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    auto imports = reinterpret_cast<IMAGE_IMPORT_DESCRIPTOR*>(base + nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress);
    DWORD patched = 0;
    for (; imports->Name; ++imports) {
        if (!imports->OriginalFirstThunk) continue;
        auto names = reinterpret_cast<IMAGE_THUNK_DATA*>(base + imports->OriginalFirstThunk);
        auto slots = reinterpret_cast<IMAGE_THUNK_DATA*>(base + imports->FirstThunk);
        for (; names->u1.AddressOfData; ++names, ++slots) {
            if (IMAGE_SNAP_BY_ORDINAL(names->u1.Ordinal)) continue;
            auto name = reinterpret_cast<IMAGE_IMPORT_BY_NAME*>(base + names->u1.AddressOfData)->Name;
            for (auto& hook : hooks) {
                if (strcmp(name, hook.name)) continue;
                DWORD old, restored;
                if (!VirtualProtect(&slots->u1.Function, sizeof(void*), PAGE_READWRITE, &old)) continue;
                InterlockedExchangePointer(reinterpret_cast<void* volatile*>(&slots->u1.Function), hook.replacement);
                VirtualProtect(&slots->u1.Function, sizeof(void*), old, &restored);
                char line[160];
                const int size = std::snprintf(line, sizeof(line), "{\"hook\":\"%s\"}\n", hook.name);
                DWORD written; WriteFile(log_file, line, size, &written, nullptr);
                ++patched;
            }
        }
    }
    const auto thread = CreateThread(nullptr, 0, report, nullptr, 0, nullptr);
    if (thread) CloseHandle(thread);
    return patched;
}

BOOL WINAPI DllMain(HINSTANCE, DWORD, void*) {
    // Keep thread notifications for the statically linked C runtime.
    return TRUE;
}
