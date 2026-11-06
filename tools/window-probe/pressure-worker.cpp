// Bounded external CPU, memory and CUDA load. It owns every allocation and
// leaves physical-memory headroom; the supervisor measures actual load.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <atomic>
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstdint>
#include <thread>
#include <vector>

static std::atomic<bool> stopping{false};
static std::atomic<unsigned long long> checksum{0};
struct Nvml {
    struct Rates { unsigned gpu; unsigned memory; };
    HMODULE dll{};
    void* device{};
    bool initialized=false;
    int (WINAPI* shutdown)(){};
    int (WINAPI* utilization)(void*,Rates*){};
    ~Nvml() { if(initialized)shutdown();if(dll)FreeLibrary(dll); }
    void bind(const char* pci) {
        dll=LoadLibraryExW(L"nvml.dll",nullptr,LOAD_LIBRARY_SEARCH_SYSTEM32);
        if(!dll)return;
        auto init=reinterpret_cast<int(WINAPI*)()>(GetProcAddress(dll,"nvmlInit_v2"));
        auto by_pci=reinterpret_cast<int(WINAPI*)(const char*,void**)>(GetProcAddress(dll,"nvmlDeviceGetHandleByPciBusId_v2"));
        shutdown=reinterpret_cast<decltype(shutdown)>(GetProcAddress(dll,"nvmlShutdown"));
        utilization=reinterpret_cast<decltype(utilization)>(GetProcAddress(dll,"nvmlDeviceGetUtilizationRates"));
        if(!init||!by_pci||!shutdown||!utilization||init())return;
        initialized=true;
        if(by_pci(pci,&device))device=nullptr;
    }
    void sample() {
        Rates rates{};
        if(device&&!utilization(device,&rates)) {
            std::printf("{\"gpuUtilization\":%u,\"gpuMemoryBusyPercent\":%u}\n",rates.gpu,rates.memory);
            std::fflush(stdout);
        }
    }
};
struct Gpu {
    HMODULE dll{};
    void* context{};
    void* program{};
    void* began{};
    void* ended{};
    unsigned long long memory{};
    int (WINAPI* free_memory)(unsigned long long){};
    int (WINAPI* unload)(void*){};
    int (WINAPI* destroy)(void*){};
    int (WINAPI* destroy_event)(void*){};
    ~Gpu() {
        if (began && destroy_event) destroy_event(began);
        if (ended && destroy_event) destroy_event(ended);
        if (memory && free_memory) free_memory(memory);
        if (program && unload) unload(program);
        if (context && destroy) destroy(context);
        if (dll) FreeLibrary(dll);
    }
    template<class T> bool function(T& output, const char* name) {
        output = reinterpret_cast<T>(GetProcAddress(dll, name));
        return output != nullptr;
    }
};

void gpu_load(unsigned seconds, unsigned duty, size_t requested_mib) {
    if (!duty) return;
    Gpu gpu;
    gpu.dll = LoadLibraryExW(L"nvcuda.dll", nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!gpu.dll) { std::printf("{\"gpuAvailable\":false}\n"); return; }
    int (WINAPI* init)(unsigned){};
    int (WINAPI* device)(int*, int){};
    int (WINAPI* pci_bus)(char*,int,int){};
    int (WINAPI* create)(void**, unsigned, int){};
    int (WINAPI* allocate)(unsigned long long*, size_t){};
    int (WINAPI* memory_info)(size_t*, size_t*){};
    int (WINAPI* fill)(unsigned long long,unsigned char,size_t){};
    int (WINAPI* load)(void**, const void*, unsigned, unsigned*, void**){};
    int (WINAPI* kernel_function)(void**, void*, const char*){};
    int (WINAPI* launch)(void*,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,unsigned,void*,void**,void**){};
    int (WINAPI* sync)(){};
    int (WINAPI* event_create)(void**,unsigned){};
    int (WINAPI* event_record)(void*,void*){};
    int (WINAPI* event_sync)(void*){};
    int (WINAPI* elapsed)(float*,void*,void*){};
    if (!gpu.function(init,"cuInit") || !gpu.function(device,"cuDeviceGet") || !gpu.function(create,"cuCtxCreate_v2") ||
        !gpu.function(allocate,"cuMemAlloc_v2") || !gpu.function(memory_info,"cuMemGetInfo_v2") || !gpu.function(fill,"cuMemsetD8_v2") || !gpu.function(load,"cuModuleLoadDataEx") ||
        !gpu.function(kernel_function,"cuModuleGetFunction") || !gpu.function(launch,"cuLaunchKernel") || !gpu.function(sync,"cuCtxSynchronize") ||
        !gpu.function(gpu.free_memory,"cuMemFree_v2") || !gpu.function(gpu.unload,"cuModuleUnload") || !gpu.function(gpu.destroy,"cuCtxDestroy_v2") ||
        !gpu.function(event_create,"cuEventCreate") || !gpu.function(event_record,"cuEventRecord") || !gpu.function(event_sync,"cuEventSynchronize") ||
        !gpu.function(elapsed,"cuEventElapsedTime") || !gpu.function(gpu.destroy_event,"cuEventDestroy_v2")) return;
    int dev=0;
    if (init(0) || device(&dev,0) || create(&gpu.context,4,dev)) { std::printf("{\"gpuAvailable\":false}\n"); return; }
    Nvml telemetry;
    char pci[32]{};
    if(gpu.function(pci_bus,"cuDeviceGetPCIBusId")&&!pci_bus(pci,sizeof(pci),dev))telemetry.bind(pci);
    const char* ptx = R"(.version 7.0
.target sm_75
.address_size 64
.visible .entry busy(.param .u64 output) {
 .reg .u64 %out,%offset;
 .reg .u32 %thread,%block,%width,%i;
 .reg .pred %more;
 .reg .f32 %v;
 ld.param.u64 %out,[output];
 mov.u32 %thread,%tid.x;mov.u32 %block,%ctaid.x;mov.u32 %width,%ntid.x;
 mad.lo.u32 %thread,%block,%width,%thread;cvt.rn.f32.u32 %v,%thread;mov.u32 %i,0;
 loop: fma.rn.f32 %v,%v,0f3f800001,0f3f000000;add.u32 %i,%i,1;setp.lt.u32 %more,%i,4000;@%more bra loop;
 mul.wide.u32 %offset,%thread,4;add.u64 %out,%out,%offset;st.global.f32 [%out],%v;ret;
})";
    if (load(&gpu.program,ptx,0,nullptr,nullptr)) { std::printf("{\"gpuAvailable\":false,\"stage\":\"module\"}\n"); return; }
    void* kernel{};
    if (kernel_function(&kernel,gpu.program,"busy")) return;
    size_t available=0,total=0;
    if (memory_info(&available,&total)) return;
    const size_t reserve=std::max<size_t>(1024ull*1024*1024,total/8);
    if(available<reserve+16*1024*1024) { std::printf("{\"gpuAvailable\":false,\"stage\":\"headroom\"}\n"); return; }
    const size_t bytes=std::max<size_t>(16*1024*1024,std::min<size_t>(requested_mib*1024*1024,available>reserve?available-reserve:0));
    if (allocate(&gpu.memory,bytes)) { std::printf("{\"gpuAvailable\":false,\"stage\":\"allocation\"}\n"); return; }
    if (fill(gpu.memory,0x5a,bytes) || sync()) return;
    if (event_create(&gpu.began,1) || event_create(&gpu.ended,1)) return;
    LARGE_INTEGER frequency; QueryPerformanceFrequency(&frequency);
    unsigned long long launches=0; double longest=0,wall_longest=0; int error=0;
    const auto until=GetTickCount64()+seconds*1000ull;
    auto last_telemetry=GetTickCount64();
    std::printf("{\"gpuAvailable\":true,\"gpuBytes\":%llu,\"duty\":%u}\n",static_cast<unsigned long long>(bytes),duty); std::fflush(stdout);
    while (!stopping.load() && GetTickCount64()<until) {
        const auto burst_until=GetTickCount64()+duty;
        while (!stopping.load() && GetTickCount64()<burst_until) {
            LARGE_INTEGER before,after;QueryPerformanceCounter(&before);
            void* arguments[]={&gpu.memory};
            error=event_record(gpu.began,nullptr);
            for(unsigned batch=0;batch<64&&!error;++batch) error=launch(kernel,16384,1,1,128,1,1,0,nullptr,arguments,nullptr);
            if (!error) error=event_record(gpu.ended,nullptr);
            if (!error) error=event_sync(gpu.ended);
            QueryPerformanceCounter(&after);
            float ms=0;
            if (!error) error=elapsed(&ms,gpu.began,gpu.ended);
            wall_longest=std::max(wall_longest,(after.QuadPart-before.QuadPart)*1000.0/frequency.QuadPart);
            longest=std::max(longest,static_cast<double>(ms));launches+=64;
            if (error || ms>500) { stopping.store(true); break; }
        }
        if(GetTickCount64()-last_telemetry>=1000) { telemetry.sample();last_telemetry=GetTickCount64(); }
        Sleep(100-duty);
    }
    std::printf("{\"gpuLaunches\":%llu,\"batchSize\":64,\"maxGpuBatchMs\":%.3f,\"maxSubmissionMs\":%.3f,\"gpuError\":%d}\n",launches,longest,wall_longest,error); std::fflush(stdout);
}

int main(int argc,char** argv) {
    if (argc!=6) return 2;
    const unsigned seconds=static_cast<unsigned>(std::strtoul(argv[1],nullptr,10));
    const unsigned threads=static_cast<unsigned>(std::strtoul(argv[2],nullptr,10));
    const size_t requested_mib=std::strtoull(argv[3],nullptr,10);
    const unsigned gpu_duty=static_cast<unsigned>(std::strtoul(argv[4],nullptr,10));
    const size_t gpu_mib=std::strtoull(argv[5],nullptr,10);
    SYSTEM_INFO info;GetSystemInfo(&info);
    if (!seconds || seconds>60 || threads>=info.dwNumberOfProcessors || requested_mib>4096 || gpu_duty>100 || gpu_mib>2048) return 2;
    // GPU submission and driver threads must remain runnable while CPU workers saturate.
    SetPriorityClass(GetCurrentProcess(),NORMAL_PRIORITY_CLASS);
    PROCESS_POWER_THROTTLING_STATE power{};power.Version=PROCESS_POWER_THROTTLING_CURRENT_VERSION;power.ControlMask=PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    SetProcessInformation(GetCurrentProcess(),ProcessPowerThrottling,&power,sizeof(power));
    DWORD_PTR allowed=0,system=0;GetProcessAffinityMask(GetCurrentProcess(),&allowed,&system);
    if (info.dwNumberOfProcessors>1 && info.dwNumberOfProcessors<=sizeof(DWORD_PTR)*8) {
        const DWORD_PTR reserved=static_cast<DWORD_PTR>(1)<<(info.dwNumberOfProcessors-1);
        if (allowed&~reserved) SetProcessAffinityMask(GetCurrentProcess(),allowed&~reserved);
    }
    GetProcessAffinityMask(GetCurrentProcess(),&allowed,&system);
    std::vector<DWORD_PTR> cpus;
    for(unsigned bit=0;bit<sizeof(DWORD_PTR)*8;++bit) if(allowed&(static_cast<DWORD_PTR>(1)<<bit)) cpus.push_back(static_cast<DWORD_PTR>(1)<<bit);
    if(threads>cpus.size())return 2;
    std::thread([seconds] { Sleep((seconds+12)*1000); ExitProcess(124); }).detach();
    std::thread([] { char command=0; std::fread(&command,1,1,stdin); stopping.store(true); }).detach();
    std::vector<void*> pages;
    const size_t chunk=16*1024*1024;
    for (size_t committed=0;committed<requested_mib*1024*1024;committed+=chunk) {
        MEMORYSTATUSEX status{};status.dwLength=sizeof(status);GlobalMemoryStatusEx(&status);
        const auto reserve=std::max<unsigned long long>(2ull*1024*1024*1024,status.ullTotalPhys/10);
        if (status.ullAvailPhys<reserve+chunk) break;
        auto memory=static_cast<unsigned char*>(VirtualAlloc(nullptr,chunk,MEM_COMMIT|MEM_RESERVE,PAGE_READWRITE));
        if (!memory) break;
        for (size_t at=0;at<chunk;at+=4096) memory[at]=0x5a;
        pages.push_back(memory);
    }
    std::printf("{\"pid\":%lu,\"cpuThreads\":%u,\"memoryBytes\":%llu,\"seconds\":%u}\n",GetCurrentProcessId(),threads,static_cast<unsigned long long>(pages.size()*chunk),seconds);std::fflush(stdout);
    const auto end=GetTickCount64()+seconds*1000ull;
    std::vector<std::thread> workers;
    for(unsigned i=0;i<threads;++i) workers.emplace_back([i,end,affinity=cpus[i]] {
        SetThreadPriority(GetCurrentThread(),THREAD_PRIORITY_BELOW_NORMAL);
        SetThreadAffinityMask(GetCurrentThread(),affinity);
        unsigned long long value=i+1;
        while(!stopping.load()&&GetTickCount64()<end) {
            for(unsigned n=0;n<65536;++n) value=(value^(value>>7))*0x9e3779b97f4a7c15ull+1;
            checksum.fetch_xor(value,std::memory_order_relaxed);
        }
    });
    std::thread graphics([=] { SetThreadPriority(GetCurrentThread(),THREAD_PRIORITY_HIGHEST);if(!cpus.empty())SetThreadAffinityMask(GetCurrentThread(),cpus.back());gpu_load(seconds,gpu_duty,gpu_mib); });
    while(!stopping.load()&&GetTickCount64()<end) Sleep(50);
    stopping.store(true);
    for(auto& worker:workers)worker.join();
    graphics.join();
    for(auto memory:pages) VirtualFree(memory,0,MEM_RELEASE);
    return 0;
}
