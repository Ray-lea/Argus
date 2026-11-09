// Interactive benchmark of owned windows only. The supervisor must obtain
// permission to use the input desktop before passing --interactive-desktop.
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <dwmapi.h>
#include <roapi.h>
#include <UIAutomation.h>
#include <psapi.h>
#include <wincodec.h>
#include <windows.graphics.capture.interop.h>
#include <windows.graphics.directx.direct3d11.interop.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Foundation.Collections.h>
#include <winrt/Windows.Foundation.Metadata.h>
#include <winrt/Windows.Graphics.Capture.h>
#include <winrt/Windows.Graphics.DirectX.Direct3D11.h>
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <memory>
#include <functional>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>
#include <utility>
using namespace winrt;
using namespace winrt::Windows::Graphics::Capture;
using namespace winrt::Windows::Graphics::DirectX;

static std::string utf8(const std::wstring& s) {
    if(s.empty()) return {};
    int n=WideCharToMultiByte(CP_UTF8,0,s.data(),int(s.size()),nullptr,0,nullptr,nullptr);
    std::string out(n,0);WideCharToMultiByte(CP_UTF8,0,s.data(),int(s.size()),out.data(),n,nullptr,nullptr);return out;
}
static std::wstring wide(const std::string& s) {
    if(s.empty()) return {};
    int n=MultiByteToWideChar(CP_UTF8,0,s.data(),int(s.size()),nullptr,0);
    std::wstring out(n,0);MultiByteToWideChar(CP_UTF8,0,s.data(),int(s.size()),out.data(),n);return out;
}
static std::string json(const std::string& s) {
    std::string out="\""; for(unsigned char c:s) {if(c=='"'||c=='\\'){out+='\\';out+=char(c);}else if(c<32){char b[8];sprintf_s(b,"\\u%04x",c);out+=b;}else out+=char(c);}return out+'"';
}
static void require(bool condition,const char* message) {if(!condition)throw std::runtime_error(message);}
static long long now() {LARGE_INTEGER v{},f{};QueryPerformanceCounter(&v);QueryPerformanceFrequency(&f);return (v.QuadPart/f.QuadPart)*10000000+(v.QuadPart%f.QuadPart)*10000000/f.QuadPart;}
static void pause(unsigned ms) {Sleep(ms);require(!(GetAsyncKeyState(VK_ESCAPE)&0x8000),"cancelled with Escape");}
static bool benchmark_vm() {
    wchar_t manufacturer[256]{};DWORD size=sizeof manufacturer;
    if(RegGetValueW(HKEY_LOCAL_MACHINE,L"HARDWARE\\DESCRIPTION\\System\\BIOS",L"SystemManufacturer",RRF_RT_REG_SZ,nullptr,manufacturer,&size)!=ERROR_SUCCESS)return false;
    return std::wstring(manufacturer).find(L"QEMU")!=std::wstring::npos;
}
static std::string registry_text(const wchar_t* key,const wchar_t* name) {
    wchar_t value[512]{};DWORD size=sizeof value;
    return RegGetValueW(HKEY_LOCAL_MACHINE,key,name,RRF_RT_REG_SZ,nullptr,value,&size)==ERROR_SUCCESS?utf8(value):"";
}
struct Area {int x=0,y=0,w=0,h=0;};
struct Image {int w=0,h=0;long long time=0;std::vector<unsigned char> pixels;};
static std::string outputRoot="target/interaction-tools";
static std::string artifact(const std::string& name){return outputRoot+'/'+name;}

class Capture {
    Direct3D11CaptureFramePool pool{nullptr}; GraphicsCaptureSession session{nullptr};
    com_ptr<ID3D11Device> device; com_ptr<ID3D11DeviceContext> context; com_ptr<ID3D11Texture2D> staging;
    int width=0,height=0;
    bool supportsInterval=false,secondaryWindows=false;
    Image last;
    Image read(Direct3D11CaptureFrame frame) {
        auto size=frame.ContentSize();auto access=frame.Surface().as<::Windows::Graphics::DirectX::Direct3D11::IDirect3DDxgiInterfaceAccess>();
        com_ptr<ID3D11Texture2D> texture;check_hresult(access->GetInterface(__uuidof(ID3D11Texture2D),texture.put_void()));
        D3D11_TEXTURE2D_DESC desc{};texture->GetDesc(&desc);
        if(size.Width<=0||size.Height<=0||size.Width>int(desc.Width)||size.Height>int(desc.Height)){frame.Close();return {};}
        if(width!=int(desc.Width)||height!=int(desc.Height)) {
            desc.Usage=D3D11_USAGE_STAGING;desc.BindFlags=0;desc.CPUAccessFlags=D3D11_CPU_ACCESS_READ;desc.MiscFlags=0;staging=nullptr;
            check_hresult(device->CreateTexture2D(&desc,nullptr,staging.put()));width=int(desc.Width);height=int(desc.Height);
        }
        context->CopyResource(staging.get(),texture.get());D3D11_MAPPED_SUBRESOURCE mapped{};check_hresult(context->Map(staging.get(),0,D3D11_MAP_READ,0,&mapped));
        Image out{size.Width,size.Height,frame.SystemRelativeTime().count(),{}};out.pixels.resize(size_t(out.w)*out.h*4);
        for(int y=0;y<out.h;y++)memcpy(out.pixels.data()+size_t(y)*out.w*4,(BYTE*)mapped.pData+size_t(y)*mapped.RowPitch,size_t(out.w)*4);
        context->Unmap(staging.get(),0);frame.Close();last=out;return out;
    }
public:
    ~Capture(){stop();}
    void stop(){if(session)session.Close();if(pool)pool.Close();session=nullptr;pool=nullptr;staging=nullptr;width=height=0;last={};}
    void prepare() {
        if(device)return;
        auto result=D3D11CreateDevice(nullptr,D3D_DRIVER_TYPE_HARDWARE,nullptr,D3D11_CREATE_DEVICE_BGRA_SUPPORT,nullptr,0,D3D11_SDK_VERSION,device.put(),nullptr,context.put());
        if(FAILED(result))check_hresult(D3D11CreateDevice(nullptr,D3D_DRIVER_TYPE_WARP,nullptr,D3D11_CREATE_DEVICE_BGRA_SUPPORT,nullptr,0,D3D11_SDK_VERSION,device.put(),nullptr,context.put()));
        (void)get_activation_factory<GraphicsCaptureItem,IGraphicsCaptureItemInterop>();
        supportsInterval=winrt::Windows::Foundation::Metadata::ApiInformation::IsPropertyPresent(L"Windows.Graphics.Capture.GraphicsCaptureSession",L"MinUpdateInterval");
        secondaryWindows=winrt::Windows::Foundation::Metadata::ApiInformation::IsPropertyPresent(L"Windows.Graphics.Capture.GraphicsCaptureSession",L"IncludeSecondaryWindows");
    }
    void start(HWND hwnd) {
        stop();prepare();
        com_ptr<::IInspectable> raw;check_hresult(CreateDirect3D11DeviceFromDXGIDevice(device.as<IDXGIDevice>().get(),raw.put()));
        auto interop=get_activation_factory<GraphicsCaptureItem,IGraphicsCaptureItemInterop>();GraphicsCaptureItem item{nullptr};
        check_hresult(interop->CreateForWindow(hwnd,guid_of<GraphicsCaptureItem>(),put_abi(item)));
        pool=Direct3D11CaptureFramePool::CreateFreeThreaded(raw.as<Direct3D11::IDirect3DDevice>(),DirectXPixelFormat::B8G8R8A8UIntNormalized,3,item.Size());
        session=pool.CreateCaptureSession(item);session.IsCursorCaptureEnabled(false);
        if(secondaryWindows)session.IncludeSecondaryWindows(true);
        if(supportsInterval)session.MinUpdateInterval(winrt::Windows::Foundation::TimeSpan{0});
        session.StartCapture();
    }
    Image next(unsigned timeout=2000) {
        const auto deadline=now()+timeout*10000LL;
        while(now()<deadline) {
            auto frame=pool.TryGetNextFrame();if(!frame){pause(2);continue;}
            auto image=read(frame);if(image.w)return image;
        }
        throw std::runtime_error("window capture timed out");
    }
    Image latest(){
        Direct3D11CaptureFrame newest{nullptr};
        while(auto frame=pool.TryGetNextFrame()){if(newest)newest.Close();newest=frame;}
        if(newest){auto image=read(newest);if(image.w)return image;}
        return last.w?last:next(6000);
    }
    void drain(){while(auto frame=pool.TryGetNextFrame())frame.Close();}
};

static void png(const std::wstring& path,const Image& image) {
    com_ptr<IWICImagingFactory> factory;check_hresult(CoCreateInstance(CLSID_WICImagingFactory,nullptr,CLSCTX_INPROC_SERVER,__uuidof(IWICImagingFactory),factory.put_void()));
    com_ptr<IWICStream> stream;check_hresult(factory->CreateStream(stream.put()));check_hresult(stream->InitializeFromFilename(path.c_str(),GENERIC_WRITE));
    com_ptr<IWICBitmapEncoder> encoder;check_hresult(factory->CreateEncoder(GUID_ContainerFormatPng,nullptr,encoder.put()));check_hresult(encoder->Initialize(stream.get(),WICBitmapEncoderNoCache));
    com_ptr<IWICBitmapFrameEncode> frame;check_hresult(encoder->CreateNewFrame(frame.put(),nullptr));check_hresult(frame->Initialize(nullptr));check_hresult(frame->SetSize(image.w,image.h));
    WICPixelFormatGUID format=GUID_WICPixelFormat32bppBGRA;check_hresult(frame->SetPixelFormat(&format));
    check_hresult(frame->WritePixels(image.h,image.w*4,UINT(image.pixels.size()),const_cast<BYTE*>(image.pixels.data())));check_hresult(frame->Commit());check_hresult(encoder->Commit());
}
struct Handle {HANDLE value=nullptr;~Handle(){if(value&&value!=INVALID_HANDLE_VALUE)CloseHandle(value);}};
class Application {
    PROCESS_INFORMATION process{}; Handle job;
    Handle inputWrite;
    static BOOL CALLBACK find(HWND h,LPARAM data) {
        auto self=reinterpret_cast<Application*>(data);DWORD pid=0;GetWindowThreadProcessId(h,&pid);RECT r{};GetClientRect(h,&r);
        if(pid==self->process.dwProcessId&&r.right>500&&r.bottom>300)self->hwnd=h;return TRUE;
    }
public:
    std::wstring executable,arguments;HWND hwnd=nullptr;Capture capture;std::function<bool(const Image&)> finalReady;
    int sort_direction(const Image& image){
        const bool native=executable.find(L"Taskmgr.exe")!=std::wstring::npos;
        if(native)return pid_order();
        const auto crop=frame_area({485,263,12,9});
        const auto* background=&image.pixels[(size_t(crop.y)*image.w+crop.x)*4];
        double count=0,sx=0,sy=0,sxy=0;
        for(int y=0;y<crop.h;y++)for(int x=0;x<crop.w;x++){
            const auto* p=&image.pixels[(size_t(crop.y+y)*image.w+crop.x+x)*4];
            if(std::max({abs(p[0]-background[0]),abs(p[1]-background[1]),abs(p[2]-background[2])})<30)continue;
            const double radial=abs(x-(crop.w-1)/2.);count++;sx+=radial;sy+=y;sxy+=radial*y;
        }
        if(count<3)return 0;const double covariance=sxy-sx*sy/count;return covariance>.1?1:covariance<-.1?-1:0;
    }
    ~Application(){stop();}
    void activate(){
        if(SetForegroundWindow(hwnd)||GetForegroundWindow()==hwnd)return;
        const DWORD current=GetCurrentThreadId(),previous=GetWindowThreadProcessId(GetForegroundWindow(),nullptr),target=GetWindowThreadProcessId(hwnd,nullptr);
        const BOOL a=previous!=current&&AttachThreadInput(current,previous,TRUE);
        const BOOL b=target!=current&&target!=previous&&AttachThreadInput(current,target,TRUE);
        SetForegroundWindow(hwnd);SetFocus(hwnd);
        if(b)AttachThreadInput(current,target,FALSE);if(a)AttachThreadInput(current,previous,FALSE);
    }
    void stop() {
        capture.stop();if(process.hProcess){
            if(inputWrite.value){const char quit[]="{\"exit\":true}\n";DWORD written=0;WriteFile(inputWrite.value,quit,sizeof quit-1,&written,nullptr);}
            else if(hwnd)PostMessageW(hwnd,WM_CLOSE,0,0);
            if(WaitForSingleObject(process.hProcess,5000)!=WAIT_OBJECT_0)TerminateProcess(process.hProcess,0);WaitForSingleObject(process.hProcess,5000);CloseHandle(process.hProcess);CloseHandle(process.hThread);
        }
        if(inputWrite.value){CloseHandle(inputWrite.value);inputWrite.value=nullptr;}
        process={};hwnd=nullptr;if(job.value){CloseHandle(job.value);job.value=nullptr;}
    }
    long long launch(bool hidden=false) {
        stop();capture.prepare();job.value=CreateJobObjectW(nullptr,nullptr);require(job.value!=nullptr,"create owned job");
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};limits.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        require(SetInformationJobObject(job.value,JobObjectExtendedLimitInformation,&limits,sizeof limits),"set owned job limit");
        STARTUPINFOEXW extended{};auto& startup=extended.StartupInfo;startup.cb=sizeof startup;startup.dwFlags=STARTF_USESHOWWINDOW;startup.wShowWindow=hidden?SW_HIDE:SW_SHOWNORMAL;
        const bool controlled=executable.find(L"Taskmgr.exe")==std::wstring::npos;
        Handle readInput,nullOutput;std::vector<unsigned char> attributes;HANDLE inherited[2]{};
        if(controlled){
            SECURITY_ATTRIBUTES security{sizeof security,nullptr,TRUE};require(CreatePipe(&readInput.value,&inputWrite.value,&security,0),"create owned exit pipe");
            require(SetHandleInformation(inputWrite.value,HANDLE_FLAG_INHERIT,0),"keep exit writer private");
            nullOutput.value=CreateFileW(L"NUL",GENERIC_WRITE,FILE_SHARE_READ|FILE_SHARE_WRITE,&security,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);require(nullOutput.value!=INVALID_HANDLE_VALUE,"open null output");
            SIZE_T bytes=0;InitializeProcThreadAttributeList(nullptr,1,0,&bytes);attributes.resize(bytes);extended.lpAttributeList=reinterpret_cast<LPPROC_THREAD_ATTRIBUTE_LIST>(attributes.data());
            require(InitializeProcThreadAttributeList(extended.lpAttributeList,1,0,&bytes),"initialize child handle list");
            inherited[0]=readInput.value;inherited[1]=nullOutput.value;
            const auto assigned=UpdateProcThreadAttribute(extended.lpAttributeList,0,PROC_THREAD_ATTRIBUTE_HANDLE_LIST,inherited,sizeof inherited,nullptr,nullptr);
            if(!assigned){DeleteProcThreadAttributeList(extended.lpAttributeList);throw std::runtime_error("restrict inherited child handles");}
            startup.cb=sizeof extended;startup.dwFlags|=STARTF_USESTDHANDLES;startup.hStdInput=readInput.value;startup.hStdOutput=startup.hStdError=nullOutput.value;
        }
        std::wstring command=L"\""+executable+L"\" "+arguments;const auto began=now();
        const auto created=CreateProcessW(executable.c_str(),command.data(),nullptr,nullptr,controlled,CREATE_SUSPENDED|(controlled?EXTENDED_STARTUPINFO_PRESENT:0),nullptr,nullptr,&startup,&process);
        if(controlled)DeleteProcThreadAttributeList(extended.lpAttributeList);require(created,"launch target");
        if(!AssignProcessToJobObject(job.value,process.hProcess)){TerminateProcess(process.hProcess,1);throw std::runtime_error("assign owned process");}
        require(SetPriorityClass(process.hProcess,NORMAL_PRIORITY_CLASS)!=0,"set target launch priority");
        PROCESS_POWER_THROTTLING_STATE power{PROCESS_POWER_THROTTLING_CURRENT_VERSION,PROCESS_POWER_THROTTLING_EXECUTION_SPEED,0};
        require(SetProcessInformation(process.hProcess,ProcessPowerThrottling,&power,sizeof power),"disable target EcoQoS");
        ResumeThread(process.hThread);
        while(!hwnd&&now()-began<300000000LL){
            EnumWindows(find,reinterpret_cast<LPARAM>(this));
            if(!hwnd&&WaitForSingleObject(process.hProcess,0)==WAIT_OBJECT_0){DWORD code=0;GetExitCodeProcess(process.hProcess,&code);throw std::runtime_error("target exited before creating its main window: "+std::to_string(code));}
            pause(2);
        }
        require(hwnd!=nullptr,"main window not created");
        if(hidden)ShowWindow(hwnd,SW_SHOWNORMAL);
        while(!IsWindowVisible(hwnd)&&now()-began<300000000LL)pause(2);
        require(IsWindowVisible(hwnd),"main window did not become visible");
        pause(20);
        RECT rect{},client{};GetWindowRect(hwnd,&rect);GetClientRect(hwnd,&client);
        if(client.right!=1240||client.bottom!=800)SetWindowPos(hwnd,nullptr,80,80,rect.right-rect.left+1240-client.right,rect.bottom-rect.top+800-client.bottom,SWP_NOZORDER|SWP_NOACTIVATE);
        if(executable.find(L"Taskmgr.exe")==std::wstring::npos)SetWindowPos(hwnd,HWND_TOPMOST,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
        activate();capture.start(hwnd);
        return began;
    }
    void foreground() {DWORD pid=0;GetWindowThreadProcessId(GetForegroundWindow(),&pid);if(pid!=process.dwProcessId)throw std::runtime_error("focus left the owned test window; expected PID "+std::to_string(process.dwProcessId)+" observed "+std::to_string(pid));}
    bool responsive() {DWORD_PTR value=0;return SendMessageTimeoutW(hwnd,WM_NULL,0,0,SMTO_ABORTIFHUNG|SMTO_BLOCK,200,&value)!=0;}
    void finish_display(){
        require(hwnd&&IsWindow(hwnd),"target window closed before final verification");
        if(IsIconic(hwnd)||!IsWindowVisible(hwnd)){ShowWindow(hwnd,SW_RESTORE);activate();}
        neutral();
        const auto deadline=now()+150000000LL;
        while(now()<deadline){try{
            auto image=capture.latest();int populated=0;
            for(int band=1;band<6;band++){int ink=0;
                for(int y=band*image.h/6;y+2<(band+1)*image.h/6;y+=3)for(int x=10;x+2<image.w-10;x+=3){size_t p=(size_t(y)*image.w+x)*4;
                    const size_t q=p+size_t(image.w)*8;
                    const int horizontal=std::max({abs(image.pixels[p]-image.pixels[p+8]),abs(image.pixels[p+1]-image.pixels[p+9]),abs(image.pixels[p+2]-image.pixels[p+10])});
                    const int vertical=std::max({abs(image.pixels[p]-image.pixels[q]),abs(image.pixels[p+1]-image.pixels[q+1]),abs(image.pixels[p+2]-image.pixels[q+2])});
                    ink+=horizontal>=25&&vertical>=25;
                }
                populated+=ink>=30;
            }
            if((finalReady?finalReady(image):populated>=3)&&responsive()){pause(1000);auto confirmed=capture.latest();if((!finalReady||finalReady(confirmed))&&responsive()){
                png(wide(artifact(std::to_string(GetCurrentProcessId())+"-final.png")),confirmed);return;
            }}
        }catch(const hresult_error&){}pause(100);}
        throw std::runtime_error("final window did not recover before the bounded cleanup deadline");
    }
    void wait_content(){
        SetWindowPos(hwnd,HWND_TOPMOST,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);activate();
        const auto deadline=now()+600000000LL;int consecutive=0,maxConsecutive=0,maxPopulated=0,replies=0,attempts=0;
        while(now()<deadline){auto image=capture.latest();int populated=0;
            for(int band=1;band<6;band++){int ink=0;
                for(int y=band*image.h/6;y+2<(band+1)*image.h/6;y+=3)for(int x=55;x+2<image.w-10;x+=3){size_t p=(size_t(y)*image.w+x)*4,q=p+size_t(image.w)*8;
                    int h=std::max({abs(image.pixels[p]-image.pixels[p+8]),abs(image.pixels[p+1]-image.pixels[p+9]),abs(image.pixels[p+2]-image.pixels[p+10])});
                    int v=std::max({abs(image.pixels[p]-image.pixels[q]),abs(image.pixels[p+1]-image.pixels[q+1]),abs(image.pixels[p+2]-image.pixels[q+2])});
                    ink+=h>=25&&v>=25;
                }
                populated+=ink>=20;
            }
            const bool replied=responsive();attempts++;replies+=replied;maxPopulated=std::max(maxPopulated,populated);
            consecutive=populated>=3&&replied?consecutive+1:0;maxConsecutive=std::max(maxConsecutive,consecutive);
            if(consecutive>=2){SetWindowPos(hwnd,HWND_TOPMOST,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);activate();return;}
            pause(50);
        }
        std::cerr<<"warmup: populated="<<maxPopulated<<" consecutive="<<maxConsecutive<<" replies="<<replies<<'/'<<attempts<<"\n";
        throw std::runtime_error("window content did not become complete and responsive");
    }
    long long click(int x,int y) {
        const bool native=executable.find(L"Taskmgr.exe")!=std::wstring::npos;
        const bool search=x>=400&&x<=770&&y>=10&&y<=45;
        const bool allowed=search||(native?((x<48&&y>=758&&y<=796)||(x<280&&y>=80&&y<=500)||(x<1220&&y>=104&&y<=760)):
            ((x<=500&&y>=60&&y<=85)||(x<=700&&y>=240&&y<=740)));
        require(allowed,"click lies outside benchmark navigation/table regions");
        foreground();POINT p{x,y};ClientToScreen(hwnd,&p);DWORD hit=0;GetWindowThreadProcessId(WindowFromPoint(p),&hit);
        if(hit!=process.dwProcessId){png(wide(artifact("blocked-window.png")),capture.latest());throw std::runtime_error("target point is obscured; expected PID "+std::to_string(process.dwProcessId)+" observed "+std::to_string(hit));}
        require(SetCursorPos(p.x,p.y),"position owned input");pause(20);POINT actual{},physical{};GetCursorPos(&actual);GetPhysicalCursorPos(&physical);
        if(actual.x!=p.x||actual.y!=p.y){RECT clip{};GetClipCursor(&clip);throw std::runtime_error("pointer mismatch; requested="+std::to_string(p.x)+","+std::to_string(p.y)+" actual="+std::to_string(actual.x)+","+std::to_string(actual.y)+" physical="+std::to_string(physical.x)+","+std::to_string(physical.y)+" clip="+std::to_string(clip.left)+","+std::to_string(clip.top)+","+std::to_string(clip.right)+","+std::to_string(clip.bottom)+" dpi="+std::to_string(GetDpiForWindow(hwnd)));}
        INPUT inputs[2]{};inputs[0].type=inputs[1].type=INPUT_MOUSE;inputs[0].mi.dwFlags=MOUSEEVENTF_LEFTDOWN;inputs[1].mi.dwFlags=MOUSEEVENTF_LEFTUP;
        const auto began=now();require(SendInput(2,inputs,sizeof(INPUT))==2,"send owned click");
        return began;
    }
    long long input_space(bool erase=false){
        foreground();INPUT keys[2]{};keys[0].type=keys[1].type=INPUT_KEYBOARD;
        if(erase){keys[0].ki.wVk=keys[1].ki.wVk=VK_BACK;keys[1].ki.dwFlags=KEYEVENTF_KEYUP;}
        else{keys[0].ki.wScan=keys[1].ki.wScan=L' ';keys[0].ki.dwFlags=KEYEVENTF_UNICODE;keys[1].ki.dwFlags=KEYEVENTF_UNICODE|KEYEVENTF_KEYUP;}
        const auto began=now();require(SendInput(2,keys,sizeof(INPUT))==2,"send search input probe");return began;
    }
    void focus_search(int x,int y){
        const auto clicked=click(x,y);POINT point{x,y};ClientToScreen(hwnd,&point);
        if(executable.find(L"Taskmgr.exe")==std::wstring::npos){
            const auto underline=frame_area({410,42,358,6});const auto deadline=now()+50000000LL;
            while(now()<deadline){auto frame=capture.latest();size_t accent=0;
                for(int yy=underline.y;yy<underline.y+underline.h;yy++)for(int xx=underline.x;xx<underline.x+underline.w;xx++){
                    const auto* p=&frame.pixels[(size_t(yy)*frame.w+xx)*4];accent+=p[0]>=140&&p[1]>=120&&int(p[0])-p[2]>=35;
                }
                if(frame.time>=clicked&&accent>=200)return;pause(10);
            }
            throw std::runtime_error("search focus underline was not presented");
        }
        com_ptr<IUIAutomation> automation;check_hresult(CoCreateInstance(CLSID_CUIAutomation,nullptr,CLSCTX_INPROC_SERVER,__uuidof(IUIAutomation),automation.put_void()));
        const auto deadline=now()+50000000LL;
        while(now()<deadline){
            com_ptr<IUIAutomationElement> element;check_hresult(automation->GetFocusedElement(element.put()));
            if(element){int pid=0;CONTROLTYPEID type=0;BOOL focused=FALSE;RECT bounds{};element->get_CurrentProcessId(&pid);element->get_CurrentControlType(&type);element->get_CurrentHasKeyboardFocus(&focused);element->get_CurrentBoundingRectangle(&bounds);
                if(DWORD(pid)==process.dwProcessId&&type==UIA_EditControlTypeId&&focused&&PtInRect(&bounds,point))return;
            }
            capture.latest();pause(10);
        }
        throw std::runtime_error("search edit did not receive keyboard focus");
    }
    void neutral() {POINT p{36,16};ClientToScreen(hwnd,&p);SetCursorPos(p.x,p.y);}
    Area frame_area(Area area) const {RECT r{};POINT p{};if(FAILED(DwmGetWindowAttribute(hwnd,DWMWA_EXTENDED_FRAME_BOUNDS,&r,sizeof r)))GetWindowRect(hwnd,&r);ClientToScreen(hwnd,&p);area.x+=p.x-r.left;area.y+=p.y-r.top;return area;}
    long long reopen() {
        STARTUPINFOW si{};si.cb=sizeof si;PROCESS_INFORMATION child{};std::wstring cmd=L"\""+executable+L"\" "+arguments;
        const auto began=now();require(CreateProcessW(executable.c_str(),cmd.data(),nullptr,nullptr,FALSE,CREATE_SUSPENDED,nullptr,nullptr,&si,&child),"launch resident reopen");
        if(!AssignProcessToJobObject(job.value,child.hProcess)){TerminateProcess(child.hProcess,1);CloseHandle(child.hThread);CloseHandle(child.hProcess);throw std::runtime_error("assign resident reopen");}
        ResumeThread(child.hThread);CloseHandle(child.hThread);CloseHandle(child.hProcess);return began;
    }
    int pid_order() {
        com_ptr<IUIAutomation> automation;check_hresult(CoCreateInstance(CLSID_CUIAutomation,nullptr,CLSCTX_INPROC_SERVER,__uuidof(IUIAutomation),automation.put_void()));
        com_ptr<IUIAutomationElement> root;check_hresult(automation->ElementFromHandle(hwnd,root.put()));
        VARIANT property{};property.vt=VT_BSTR;property.bstrVal=SysAllocString(L"PID");com_ptr<IUIAutomationCondition> condition;const auto made=automation->CreatePropertyCondition(UIA_NamePropertyId,property,condition.put());VariantClear(&property);check_hresult(made);
        com_ptr<IUIAutomationElement> heading;check_hresult(root->FindFirst(TreeScope_Descendants,condition.get(),heading.put()));if(!heading)return 0;
        RECT header{},bounds{};heading->get_CurrentBoundingRectangle(&header);root->get_CurrentBoundingRectangle(&bounds);
        if(header.right<=header.left)return 0;
        std::vector<unsigned long long> rows;LONG y=header.bottom+8;
        for(int i=0;i<10&&y<bounds.bottom;i++){com_ptr<IUIAutomationElement> e;check_hresult(automation->ElementFromPoint({(header.left+header.right)/2,y},e.put()));int pid=0;e->get_CurrentProcessId(&pid);if(DWORD(pid)!=process.dwProcessId)return 0;
            RECT r{};e->get_CurrentBoundingRectangle(&r);if(r.bottom<=y)return 0;y=r.bottom+8;
            if(r.right<=r.left||r.top<header.bottom||r.left<header.left-1||r.right>header.right+1)continue;
            BSTR name=nullptr;e->get_CurrentName(&name);std::wstring value=name?name:L"";SysFreeString(name);
            if(!value.empty()&&value.size()<=10&&std::all_of(value.begin(),value.end(),[](wchar_t c){return c>=L'0'&&c<=L'9';}))rows.push_back(std::stoull(value));
        }
        if(rows.size()<3)return 0;bool ascending=true,descending=true;
        for(size_t i=1;i<rows.size();i++){ascending&=rows[i-1]<=rows[i];descending&=rows[i-1]>=rows[i];}
        return ascending&&!descending?1:descending&&!ascending?-1:0;
    }
    void resources() {
        PROCESS_MEMORY_COUNTERS_EX memory{};memory.cb=sizeof memory;GetProcessMemoryInfo(process.hProcess,reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&memory),sizeof memory);
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION limits{};QueryInformationJobObject(job.value,JobObjectExtendedLimitInformation,&limits,sizeof limits,nullptr);
        std::cout<<"{\"event\":\"resources\",\"privateBytes\":"<<memory.PrivateUsage<<",\"workingSetBytes\":"<<memory.WorkingSetSize<<",\"peakJobCommitBytes\":"<<limits.PeakJobMemoryUsed<<",\"gdiObjects\":"<<GetGuiResources(process.hProcess,GR_GDIOBJECTS)<<",\"userObjects\":"<<GetGuiResources(process.hProcess,GR_USEROBJECTS)<<"}\n"<<std::flush;
    }
    void elements() {
        resources();
        com_ptr<IUIAutomation> automation;check_hresult(CoCreateInstance(CLSID_CUIAutomation,nullptr,CLSCTX_INPROC_SERVER,__uuidof(IUIAutomation),automation.put_void()));
        com_ptr<IUIAutomationElement> root;check_hresult(automation->ElementFromHandle(hwnd,root.put()));com_ptr<IUIAutomationCondition> condition;automation->CreateTrueCondition(condition.put());
        com_ptr<IUIAutomationElementArray> children;check_hresult(root->FindAll(TreeScope_Descendants,condition.get(),children.put()));int length=0;children->get_Length(&length);POINT origin{};ClientToScreen(hwnd,&origin);
        for(int i=0;i<length;i++){com_ptr<IUIAutomationElement> e;children->GetElement(i,e.put());BSTR name=nullptr,id=nullptr;CONTROLTYPEID type=0;RECT r{};
            e->get_CurrentName(&name);e->get_CurrentAutomationId(&id);e->get_CurrentControlType(&type);e->get_CurrentBoundingRectangle(&r);
            if(r.right>r.left&&r.bottom>r.top)std::cout<<"{\"event\":\"element\",\"name\":"<<json(utf8(name?name:L""))<<",\"id\":"<<json(utf8(id?id:L""))<<",\"type\":"<<type<<",\"rect\":["<<r.left-origin.x<<','<<r.top-origin.y<<','<<r.right-origin.x<<','<<r.bottom-origin.y<<"]}\n";
            SysFreeString(name);SysFreeString(id);
        }
    }
};

static int distance(const unsigned char* a,const unsigned char* b) {return std::max({abs(a[0]-b[0]),abs(a[1]-b[1]),abs(a[2]-b[2])});}
struct Mask {
    std::vector<size_t> points;std::vector<int> tolerance;Image target;Area area;
    Mask(const Image& a,const Image& b,const Image* other,Area crop,const Image* otherStable=nullptr,int stride=3,int contrast=18,size_t minimum=80,int border=0):target(b),area(crop){
        require(a.w==b.w&&a.h==b.h,"reference geometry changed");
        require(crop.x>=0&&crop.y>=0&&crop.x+crop.w<=b.w&&crop.y+crop.h<=b.h,"reference crop out of bounds");
        for(int y=crop.y;y<crop.y+crop.h;y+=stride)for(int x=crop.x;x<crop.x+crop.w;x+=stride){size_t p=(size_t(y)*b.w+x)*4;
            if(border&&y>=crop.y+border&&y<crop.y+crop.h-border)continue;
            const bool edge=x+2<b.w&&distance(&b.pixels[p],&b.pixels[p+8])>=20;
            const int delta=other?distance(&b.pixels[p],&other->pixels[p]):255;
            if(distance(&a.pixels[p],&b.pixels[p])<=6&&(!otherStable||distance(&other->pixels[p],&otherStable->pixels[p])<=6)&&(other?delta>=contrast:edge)){points.push_back(p);tolerance.push_back(other?std::clamp(int(delta*.4),1,10):10);}
        }
        require(points.size()>=minimum,"insufficient distinct, stable content pixels");
    }
    double match(const Image& image) const {
        if(image.w!=target.w||image.h!=target.h)return 0.;size_t good[5]{},total[5]{};
        for(size_t i=0;i<points.size();i++){auto p=points[i];int y=int(p/4)/target.w;int band=std::min(4,(y-area.y)*5/area.h);total[band]++;good[band]+=distance(&image.pixels[p],&target.pixels[p])<=tolerance[i];}
        double ratio=1.;for(int i=0;i<5;i++)if(total[i]>=2)ratio=std::min(ratio,double(good[i])/total[i]);return ratio;
    }
    bool filled(const Image& image) const {
        if(image.w!=target.w||image.h!=target.h)return false;
        // Check content across the entire viewport, including both edges and
        // the bottom. Numeric glyphs may change; blank or partially drawn
        // regions must not pass on the strength of a small matching crop.
        for(int by=0;by<6;by++)for(int bx=0;bx<8;bx++){
            int expected=0,actual=0;const int x0=bx*target.w/8,x1=(bx+1)*target.w/8;
            const int y0=by*target.h/6,y1=(by+1)*target.h/6;
            for(int y=y0;y+2<y1;y+=3)for(int x=x0;x+2<x1;x+=3){size_t p=(size_t(y)*target.w+x)*4;
                expected+=std::min(distance(&target.pixels[p],&target.pixels[p+8]),distance(&target.pixels[p],&target.pixels[p+target.w*8]))>=25;
                actual+=std::min(distance(&image.pixels[p],&image.pixels[p+8]),distance(&image.pixels[p],&image.pixels[p+image.w*8]))>=25;
            }
            if(expected>=12&&actual<2)return false;
        }
        return true;
    }
};
struct ContentStructure {
    struct Point {size_t offset;unsigned signature;unsigned band;};
    int width,height;std::vector<Point> points;
    static unsigned edges(const Image& image,size_t p,int threshold=18) {
        return (distance(&image.pixels[p],&image.pixels[p+8])>=threshold?1u:0u)
            |(distance(&image.pixels[p],&image.pixels[p+size_t(image.w)*8])>=threshold?2u:0u);
    }
    ContentStructure(const Image& a,const Image& b,Area area,const Image* other=nullptr,Area movingGrid={},const std::vector<Area>& excluded={}):width(b.w),height(b.h){
        require(a.w==b.w&&a.h==b.h,"content geometry changed");
        require(area.x>=0&&area.y>=0&&area.w>2&&area.h>2&&area.x+area.w<=b.w&&area.y+area.h<=b.h,"content crop out of bounds");
        for(int y=area.y;y+2<area.y+area.h;y+=2)for(int x=area.x;x+2<area.x+area.w;x+=2){
            if(std::any_of(excluded.begin(),excluded.end(),[&](Area r){return x>=r.x&&x<r.x+r.w&&y>=r.y&&y<r.y+r.h;}))continue;
            // Native Task Manager scrolls vertical grid lines with history.
            // Horizontal lines still verify that the graph body was drawn.
            const bool scrolling=x>=movingGrid.x&&x<movingGrid.x+movingGrid.w&&y>=movingGrid.y&&y<movingGrid.y+movingGrid.h;
            const unsigned directions=scrolling?2u:3u;
            const size_t p=(size_t(y)*b.w+x)*4;const auto signature=edges(b,p)&directions;
            if(signature&&(edges(a,p)&directions)==signature&&(!other||(edges(*other,p)&signature)!=signature))
                points.push_back({p,signature,unsigned(std::min(5,(y-area.y)*6/area.h))});
        }
        require(points.size()>=50,"insufficient stable body structure");
    }
    double match(const Image& image) const {
        if(image.w!=width||image.h!=height)return 0.;
        size_t good[6]{},total[6]{};
        for(auto point:points){total[point.band]++;good[point.band]+=(edges(image,point.offset,10)&point.signature)==point.signature;}
        double ratio=1.;for(int i=0;i<6;i++)if(total[i]>=8)ratio=std::min(ratio,double(good[i])/total[i]);return ratio;
    }
};
struct GraphContent {
    Area plot;std::vector<int> rows;
    static double horizontal(const Image& image,Area plot,int y){
        int count=0,total=0;
        for(int x=plot.x+4;x<plot.x+plot.w-4;x+=4){const auto p=(size_t(y)*image.w+x)*4;total++;count+=distance(&image.pixels[p],&image.pixels[p+size_t(image.w)*8])>=10;}
        return total?double(count)/total:0.;
    }
    GraphContent(const Image& a,const Image& b,Area area):plot(area){
        for(int y=plot.y+3;y+3<plot.y+plot.h;y++)if(horizontal(a,plot,y)>=.7&&horizontal(b,plot,y)>=.7)rows.push_back(y);
        require(rows.size()>=6,"reference graph grid is incomplete");
    }
    bool match(const Image& image)const{
        return std::all_of(rows.begin(),rows.end(),[&](int y){return horizontal(image,plot,y)>=.65;})&&curve_pixels_present(image);
    }
    bool curve_pixels_present(const Image& image)const{
        size_t count=0;for(int y=plot.y+3;y<plot.y+plot.h-3;y++)for(int x=plot.x+3;x<plot.x+plot.w-3;x++){
            const auto* p=&image.pixels[(size_t(y)*image.w+x)*4];count+=p[0]>=140&&p[1]>=120&&int(p[0])-p[2]>=35&&int(p[1])-p[2]>=25;
            if(count>=16)return true;
        }return false;
    }
};
struct TableContent {
    int width,height,rowHeight;Area body;std::vector<Area> cells;
    static int text_lines(const Image& image,Area cell){
        int lines=0;
        for(int y=cell.y+2;y<cell.y+cell.h-2;y++){
            bool ink=false;
            for(int x=cell.x+3;x+2<cell.x+cell.w-3;x++){
                const auto p=(size_t(y)*image.w+x)*4;
                const auto bright=[&](size_t i){return std::min({image.pixels[i],image.pixels[i+1],image.pixels[i+2]})>=95;};
                if((bright(p)||bright(p+4)||bright(p+8))&&std::max(distance(&image.pixels[p],&image.pixels[p+4]),distance(&image.pixels[p],&image.pixels[p+8]))>=18){ink=true;break;}
            }
            lines+=ink;
        }
        return lines;
    }
    TableContent(const Image& image,Area crop,bool native,Area offset):width(image.w),height(image.h),rowHeight(native?25:30),body(crop){
        const std::vector<std::pair<int,int>> columns=native?std::vector<std::pair<int,int>>{{80,115},{195,50},{425,40},{465,80}}:std::vector<std::pair<int,int>>{{65,382},{447,60},{507,70},{577,94}};
        int possible=0;
        for(int y=body.y;y+rowHeight*.7<body.y+body.h;y+=rowHeight)for(auto column:columns){
            Area cell{column.first+offset.x,y,column.second,std::min(rowHeight,body.y+body.h-y)};possible++;
            require(cell.x>=0&&cell.x+cell.w<=width&&cell.y>=0&&cell.y+cell.h<=height,"table cell is outside the captured frame");
            if(text_lines(image,cell)>=4)cells.push_back(cell);
        }
        require(cells.size()>=size_t(possible*.75),"reference table counters are incomplete");
    }
    double match(const Image& image)const{
        if(image.w!=width||image.h!=height)return 0.;size_t present=0;for(auto cell:cells)present+=text_lines(image,cell)>=4;return double(present)/cells.size();
    }
};
static size_t curve_pixels(const Image& image,Area plot){
    size_t count=0;for(int y=plot.y+2;y<plot.y+plot.h-2;y++)for(int x=plot.x+2;x<plot.x+plot.w-2;x++){
        const auto* p=&image.pixels[(size_t(y)*image.w+x)*4];count+=p[0]>=140&&p[1]>=120&&int(p[0])-p[2]>=35&&int(p[1])-p[2]>=25;
    }return count;
}
static double changed(const Image& a,const Image& b,Area area,int contrast=20){
    if(a.w!=b.w||a.h!=b.h)return 1.;int count=0,different=0;
    for(int y=area.y;y<area.y+area.h;y+=3)for(int x=area.x;x<area.x+area.w;x+=3){size_t p=(size_t(y)*a.w+x)*4;count++;different+=distance(&a.pixels[p],&b.pixels[p])>=contrast;}
    return count?double(different)/count:0.;
}
static Image settled(Application& app,unsigned ms=700){
    const auto deadline=now()+ms*10000LL;auto image=app.capture.latest();
    while(now()<deadline){pause(4);image=app.capture.latest();}
    return image;
}
static void native_pid_ascending(Application& app){
    if(app.executable.find(L"Taskmgr.exe")==std::wstring::npos)return;
    for(int attempt=0;attempt<2&&app.pid_order()!=1;attempt++){app.click(220,118);settled(app,350);}
    require(app.pid_order()==1,"native PID order did not become ascending");
}
static Image await_match(Application& app,const Mask& mask,long long began,unsigned timeout=15000,double threshold=.985,const Image* before=nullptr,Area body={},std::function<bool(const Image&)> semantic={},int changeContrast=20,unsigned confirmationMs=120) {
    double best=0.;unsigned headers=0,content=0,replies=0;
    while(now()-began<timeout*10000LL){auto image=app.capture.next(6000);const auto matched=mask.match(image);best=std::max(best,matched);
        if(image.time<began||matched<threshold)continue;headers++;
        if((before&&changed(*before,image,body,changeContrast)<=.003)||!(semantic?semantic(image):mask.filled(image)))continue;content++;
        if(!app.responsive())continue;replies++;
        {
        if(!confirmationMs)return image;
        auto confirmed=settled(app,confirmationMs);
        if(mask.match(confirmed)>=threshold&&(semantic?semantic(confirmed):mask.filled(confirmed))&&app.responsive())return image;
    }}
    std::cerr<<"best target match "<<best<<"; headers="<<headers<<" content="<<content<<" replies="<<replies<<"\n";
    throw std::runtime_error("target content did not fully match before timeout");
}
static void measurement(const std::string& operation,long long began,const Image& image,double matched) {
    static unsigned serial=0;const std::string path=artifact("sample-"+std::to_string(GetCurrentProcessId())+"-"+std::to_string(++serial)+"-"+operation+".png");png(wide(path),image);
    require(image.time>=began,"capture timestamp predates input");
    PERFORMANCE_INFORMATION info{};info.cb=sizeof info;require(GetPerformanceInfo(&info,sizeof info),"read measurement process count");
    std::cout<<"{\"event\":\"measurement\",\"operation\":"<<json(operation)<<",\"ms\":"<<double(image.time-began)/10000.<<",\"matched\":"<<matched<<",\"processCount\":"<<info.ProcessCount<<",\"frame\":"<<json(path)<<"}\n"<<std::flush;
}
static void benchmark(Application& app,std::istringstream& command) {
    app.finalReady={};
    std::string label;int ax,ay,bx,by,n;Area crop;command>>label>>ax>>ay>>bx>>by>>crop.x>>crop.y>>crop.w>>crop.h>>n;require(bool(command)&&n>0&&n<=30,"invalid benchmark command");
    if(label=="sort"&&app.executable.find(L"Taskmgr.exe")!=std::wstring::npos)(void)app.pid_order();
    app.click(ax,ay);app.wait_content();if(label!="sort")native_pid_ascending(app);auto a=settled(app,2500);auto aa=settled(app,500);int orderA=label=="sort"?app.sort_direction(aa):0;
    app.click(bx,by);app.wait_content();auto b=settled(app,2500);auto bb=settled(app,500);int orderB=label=="sort"?app.sort_direction(bb):0;
    const std::string prefix=artifact(std::to_string(GetCurrentProcessId())+"-"+label);
    png(wide(prefix+"-a.png"),aa);png(wide(prefix+"-b.png"),bb);
    const bool sorting=label=="sort";if(sorting)require(orderA&&orderB&&orderA!=orderB,"sort indicator did not change");
    const bool native=app.executable.find(L"Taskmgr.exe")!=std::wstring::npos;
    const auto region=app.frame_area(sorting?Area{ax-32,ay-22,72,43}:(native?Area{65,61,200,35}:Area{20,260,480,22})),body=app.frame_area(crop);
    TableContent tableA(aa,body,native,app.frame_area({}));require(tableA.match(a)==1.,"reference table has missing cells");
    const auto destination=sorting?region:app.frame_area(native?Area{286,115,240,42}:Area{294,113,240,42});
    Mask ma(a,aa,sorting?&bb:nullptr,region,sorting?&b:nullptr,1,18,sorting?6:10),mb(b,bb,sorting?&aa:nullptr,destination,sorting?&a:nullptr,1,18,sorting?6:10);
    const auto top=app.frame_area(native?Area{194,130,50,75}:Area{447,286,58,90});
    std::unique_ptr<Mask> ascending;
    if(sorting)ascending=std::make_unique<Mask>(orderA==1?a:b,orderA==1?aa:bb,orderA==1?&bb:&aa,top,orderA==1?&b:&a,1,18,10);
    const auto plot=app.frame_area(native?Area{288,173,928,437}:Area{294,216,904,246});
    const auto curveReference=sorting?0:std::min(curve_pixels(b,plot),curve_pixels(bb,plot));
    app.resources();
    require(sorting||curveReference>=16,"reference CPU graph has no visible data curve");
    std::unique_ptr<TableContent> tableB;
    std::unique_ptr<ContentStructure> structureB;
    std::shared_ptr<GraphContent> nativeGraph;
    if(sorting)tableB=std::make_unique<TableContent>(bb,body,native,app.frame_area({}));
    else {
        std::vector<Area> excluded;
        if(native){
            nativeGraph=std::make_shared<GraphContent>(b,bb,plot);
            for(auto r:{Area{50,130,208,668},Area{288,654,205,31},Area{288,705,205,29},Area{288,756,205,42}})excluded.push_back(app.frame_area(r));
            excluded.push_back(plot);
        }
        structureB=std::make_unique<ContentStructure>(b,bb,body,&aa,Area{},excluded);
    }
    const auto matchesA=[&](const Image& image){return tableA.match(image)==1.&&(!sorting||(orderA==1?ascending->match(image)>.98:ascending->match(image)<.6));};
    const auto matchesB=[&](const Image& image){return (sorting?tableB->match(image)==1.:structureB->match(image)>=.90)&&(!sorting||(orderB==1?ascending->match(image)>.98:ascending->match(image)<.6))&&(!nativeGraph||nativeGraph->match(image))&&(curveReference<16||curve_pixels(image,plot)>=16);};
    const double threshold=.985;
    std::cout<<"{\"event\":\"reference\",\"a\":"<<json(prefix+"-a.png")<<",\"b\":"<<json(prefix+"-b.png")<<",\"pixelsA\":"<<ma.points.size()<<",\"pixelsB\":"<<mb.points.size()<<",\"threshold\":"<<threshold<<",\"directionA\":"<<orderA<<",\"directionB\":"<<orderB<<",\"doubleClickMs\":"<<GetDoubleClickTime()<<"}\n"<<std::flush;
    try{for(int i=0;i<n;i++){
        auto before=app.capture.latest();app.capture.drain();auto began=app.click(ax,ay);auto out=await_match(app,ma,began,15000,threshold,&before,body,matchesA);measurement(label+"-a",began,out,ma.match(out));settled(app,std::max(1000u,GetDoubleClickTime()+100));
        before=app.capture.latest();app.capture.drain();began=app.click(bx,by);auto outB=await_match(app,mb,began,15000,threshold,&before,body,matchesB);measurement(label+"-b",began,outB,mb.match(outB));settled(app,std::max(1000u,GetDoubleClickTime()+100));
    }}catch(...){auto last=app.capture.latest();std::cerr<<"body scores "<<tableA.match(last)<<','<<(sorting?tableB->match(last):structureB->match(last))<<"; ascending "<<(ascending?ascending->match(last):-1)<<"; curve "<<curve_pixels(last,plot)<<'/'<<curveReference<<"\n";png(wide(artifact("failed-body.png")),last);throw;}
    if(sorting)app.finalReady=[table=*tableB](const Image& image){return table.match(image)==1.;};
    else app.finalReady=[structure=*structureB,nativeGraph,curveReference,plot](const Image& image){return structure.match(image)>=.90&&(!nativeGraph||nativeGraph->match(image))&&(curveReference<16||curve_pixels(image,plot)>=16);};
}
static void lifecycle(Application& app,std::istringstream& command) {
    app.finalReady={};
    std::string mode;int ax,ay,bx,by,n;Area crop;command>>mode>>ax>>ay>>bx>>by>>crop.x>>crop.y>>crop.w>>crop.h>>n;require(bool(command)&&n>0&&n<=15,"invalid lifecycle command");
    native_pid_ascending(app);
    app.click(ax,ay);settled(app,500);app.focus_search(bx,by);auto a=settled(app);auto aa=settled(app,300);
    app.input_space();auto b=settled(app);auto bb=settled(app,300);
    const std::string prefix=artifact(std::to_string(GetCurrentProcessId())+"-"+mode);
    png(wide(prefix+"-a.png"),aa);png(wide(prefix+"-b.png"),bb);
    const bool native=app.executable.find(L"Taskmgr.exe")!=std::wstring::npos;
    const auto region=app.frame_area(crop);
    Mask complete(a,aa,nullptr,app.frame_area(native?Area{65,105,420,24}:Area{20,260,480,22}),nullptr,1,18,10);
    TableContent coverage(aa,region,native,app.frame_area({}));
    const auto bodyReady=[&](const Image& image){return coverage.match(image)==1.;};
    const auto focus=app.frame_area(native?Area{392,8,424,36}:Area{408,12,370,36});
    Mask selected(b,bb,&aa,focus,&a,1,18,80),cleared(a,aa,&bb,focus,&b,1,18,80);
    auto erased=app.input_space(true);await_match(app,cleared,erased,15000,.985,nullptr,{},bodyReady);
    try{for(int i=0;i<n;i++){
        app.click(ax,ay);settled(app);long long began;
        if(mode=="cold") {app.stop();pause(1000);began=app.launch();}
        else if(mode=="resident") {
            if(native)ShowWindow(app.hwnd,SW_MINIMIZE);
            else{PostMessageW(app.hwnd,WM_CLOSE,0,0);const auto hiddenBy=now()+80000000LL;while(IsWindow(app.hwnd)&&IsWindowVisible(app.hwnd)&&now()<hiddenBy){app.capture.latest();pause(10);}require(IsWindow(app.hwnd)&&!IsWindowVisible(app.hwnd),"close-to-tray did not hide the main window");}
            pause(2000);app.capture.drain();began=app.reopen();
        }
        else {require(mode=="restore","unknown lifecycle mode");ShowWindow(app.hwnd,SW_MINIMIZE);pause(2000);app.capture.drain();began=now();ShowWindowAsync(app.hwnd,SW_RESTORE);}
        // Probe input immediately after the first complete frame. Confirm the
        // final response afterwards, so the stability hold does not inflate it.
        auto image=await_match(app,complete,began,15000,.985,nullptr,{},bodyReady,20,0);measurement(mode+"-visible",began,image,complete.match(image));
        app.activate();app.foreground();app.focus_search(bx,by);
        auto input=app.input_space();image=await_match(app,selected,input,15000,.985,nullptr,{},bodyReady);measurement(mode+"-ready",began,image,selected.match(image));
        std::cout<<"{\"event\":\"input-ack\",\"operation\":"<<json(mode)<<",\"ms\":"<<double(image.time-input)/10000.<<"}\n"<<std::flush;
        erased=app.input_space(true);await_match(app,cleared,erased,15000,.985,nullptr,{},bodyReady);settled(app,500);
    }}catch(...){std::cerr<<"body coverage "<<coverage.match(app.capture.latest())<<"\n";throw;}
    app.finalReady=[coverage](const Image& image){return coverage.match(image)==1.;};
}
int wmain(int argc,wchar_t** argv) {
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    if(argc==2&&std::wstring(argv[1])==L"--selftest"){
        try{
            Image full{120,120,0,std::vector<unsigned char>(120*120*4,24)};
            for(int band=0;band<6;band++)for(int column=0;column<4;column++)for(int y=band*20+4;y<band*20+14;y++)for(int x=column*25+8;x<column*25+18;x++)
                for(int c=0;c<3;c++)full.pixels[(size_t(y)*120+x)*4+c]=220;
            ContentStructure expected(full,full,{0,0,120,120});
            auto blank=full;std::fill(blank.pixels.begin(),blank.pixels.end(),static_cast<unsigned char>(24));
            auto partial=full;std::fill(partial.pixels.begin()+120*20*4,partial.pixels.end(),static_cast<unsigned char>(24));
            auto shaded=full;for(auto& channel:shaded.pixels)if(channel==220)channel=180;
            require(expected.match(full)==1.,"complete body was rejected");
            require(expected.match(blank)<.90,"blank body was accepted");
            require(expected.match(partial)<.90,"header-only body was accepted");
            require(expected.match(shaded)==1.,"stable body structure depended on absolute color");
            auto grid=blank,shifted=blank,verticalOnly=blank;
            for(int y=0;y<120;y++)for(int x=0;x<120;x++)for(int c=0;c<3;c++){
                const auto p=(size_t(y)*120+x)*4+c;
                if(x%30==10||y%30==10)grid.pixels[p]=80;
                if(x%30==14||y%30==10)shifted.pixels[p]=80;
                if(x%30==14)verticalOnly.pixels[p]=80;
            }
            ContentStructure gridShape(grid,grid,{0,0,120,120},nullptr,{0,0,120,120});
            require(gridShape.match(shifted)>=.90,"scrolling grid lines were treated as fixed content");
            require(gridShape.match(verticalOnly)<.90,"missing horizontal graph structure was accepted");
            auto border=blank;for(int y=0;y<120;y++)for(int x=0;x<120;x++)if(!x||!y||x==119||y==119){auto* p=&border.pixels[(size_t(y)*120+x)*4];p[0]=240;p[1]=200;p[2]=50;}
            require(curve_pixels(border,{0,0,120,120})==0,"graph border was counted as data");
            for(int x=20;x<100;x++){auto* p=&border.pixels[(size_t(60)*120+x)*4];p[0]=240;p[1]=200;p[2]=50;}
            require(curve_pixels(border,{0,0,120,120})>=16,"visible curve was rejected");
            for(int x=20;x<100;x++)for(auto* image:{&grid,&shifted,&verticalOnly}){auto* p=&image->pixels[(size_t(60)*120+x)*4];p[0]=240;p[1]=200;p[2]=50;}
            GraphContent graph(grid,grid,{0,0,120,120});
            require(graph.match(shifted),"a scrolling graph was rejected");
            require(!graph.match(verticalOnly),"a graph with missing horizontal lines was accepted");
            Image table{600,150,0,std::vector<unsigned char>(600*150*4,24)};
            const auto rectangle=[](Image& image,Area area,unsigned char color){for(int y=area.y;y<area.y+area.h;y++)for(int x=area.x;x<area.x+area.w;x++)for(int c=0;c<3;c++)image.pixels[(size_t(y)*image.w+x)*4+c]=color;};
            for(int y=0;y<150;y+=25)for(int x:{90,205,435,475})rectangle(table,{x,y+5,9,10},220);
            TableContent counters(table,{60,0,500,150},true,{});
            auto updated=table;rectangle(updated,{465,0,80,150},24);
            for(int y=0;y<150;y+=25)rectangle(updated,{487,y+5,15,10},220);
            require(counters.match(updated)==1.,"changing numeric glyphs were rejected");
            auto missing=updated;rectangle(missing,{465,125,80,25},24);
            rectangle(missing,{487,136,10,2},220);
            require(counters.match(missing)<1.,"a missing counter displayed as a dash was accepted");
            rectangle(missing,{60,100,500,50},24);
            require(counters.match(missing)<1.,"a partially painted table was accepted");
            std::cout<<"content validation: tables, changing counters, blank/partial bodies and graph structure passed\n";return 0;
        }catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}
    }
    if(argc==4&&std::wstring(argv[1])==L"--display"){
        if(!benchmark_vm())return 2;
        DWORD sessionId=0;ProcessIdToSessionId(GetCurrentProcessId(),&sessionId);if(!sessionId)return 2;
        const int width=_wtoi(argv[2]),height=_wtoi(argv[3]);
        if(width<1440||width>2560||height<900||height>1440)return 2;
        DEVMODEW mode{};mode.dmSize=sizeof mode;
        for(DWORD i=0;EnumDisplaySettingsW(nullptr,i,&mode);i++)if(mode.dmPelsWidth==DWORD(width)&&mode.dmPelsHeight==DWORD(height)&&mode.dmBitsPerPel==32){
            const auto result=ChangeDisplaySettingsExW(nullptr,&mode,nullptr,CDS_UPDATEREGISTRY,nullptr);
            std::cout<<"{\"displayResult\":"<<result<<"}\n";return result==DISP_CHANGE_SUCCESSFUL?0:1;
        }
        std::cerr<<"Requested display mode is unavailable\n";return 1;
    }
    if(argc==3&&std::wstring(argv[1])==L"--idle"){
        if(!benchmark_vm())return 2;
        Handle parent{OpenProcess(SYNCHRONIZE,FALSE,wcstoul(argv[2],nullptr,10))};if(!parent.value)return 1;
        SetPriorityClass(GetCurrentProcess(),IDLE_PRIORITY_CLASS);WaitForSingleObject(parent.value,900000);return 0;
    }
    if(argc==2&&std::wstring(argv[1])==L"--system"){
        PERFORMANCE_INFORMATION info{};info.cb=sizeof info;require(GetPerformanceInfo(&info,sizeof info),"read system counts");
        Handle token;require(OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token.value),"read own token");TOKEN_ELEVATION elevation{};DWORD length=0;GetTokenInformation(token.value,TokenElevation,&elevation,sizeof elevation,&length);
        SYSTEM_INFO cpu{};GetNativeSystemInfo(&cpu);const wchar_t* os=L"SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";DWORD ubr=0,size=sizeof ubr,sessionId=0;ProcessIdToSessionId(GetCurrentProcessId(),&sessionId);DEVMODEW mode{};mode.dmSize=sizeof mode;EnumDisplaySettingsW(nullptr,ENUM_CURRENT_SETTINGS,&mode);
        RegGetValueW(HKEY_LOCAL_MACHINE,os,L"UBR",RRF_RT_REG_DWORD,nullptr,&ubr,&size);
        std::cout<<"{\"benchmarkVm\":"<<(benchmark_vm()?"true":"false")<<",\"processes\":"<<info.ProcessCount<<",\"threads\":"<<info.ThreadCount<<",\"availableBytes\":"<<info.PhysicalAvailable*info.PageSize<<",\"totalBytes\":"<<info.PhysicalTotal*info.PageSize<<",\"elevated\":"<<(elevation.TokenIsElevated?"true":"false")
            <<",\"windowsRelease\":"<<json(registry_text(os,L"DisplayVersion"))<<",\"windowsBuild\":"<<json(registry_text(os,L"CurrentBuildNumber")+"."+std::to_string(ubr))
            <<",\"cpu\":"<<json(registry_text(L"HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",L"ProcessorNameString"))<<",\"logicalProcessors\":"<<cpu.dwNumberOfProcessors
            <<",\"sessionId\":"<<sessionId<<",\"desktopPixels\":["<<GetSystemMetrics(SM_CXSCREEN)<<','<<GetSystemMetrics(SM_CYSCREEN)<<"],\"dpi\":"<<GetDpiForSystem()<<",\"refreshHz\":"<<mode.dmDisplayFrequency<<",\"displayModes\":[";
        std::vector<std::pair<DWORD,DWORD>> modes;
        for(DWORD i=0;EnumDisplaySettingsW(nullptr,i,&mode);i++)if(mode.dmBitsPerPel==32){
            auto dimensions=std::make_pair(mode.dmPelsWidth,mode.dmPelsHeight);
            if(std::find(modes.begin(),modes.end(),dimensions)==modes.end()){if(!modes.empty())std::cout<<',';std::cout<<'['<<dimensions.first<<','<<dimensions.second<<']';modes.push_back(dimensions);}
        }
        std::cout<<"]}\n";return 0;
    }
    if(argc<4||std::wstring(argv[1])!=L"--interactive-desktop"){std::fprintf(stderr,"Explicit --interactive-desktop authorization is required.\n");return 2;}
    try {
        require(benchmark_vm(),"This benchmark runs only inside the dedicated QEMU virtual machine");
        require(SetPriorityClass(GetCurrentProcess(),NORMAL_PRIORITY_CLASS)!=0,"set probe priority");
        require(SetThreadExecutionState(ES_CONTINUOUS|ES_SYSTEM_REQUIRED|ES_DISPLAY_REQUIRED)!=0,"keep the VM display awake during measurement");
        init_apartment(apartment_type::multi_threaded);
        if(argc>=5)outputRoot=utf8(argv[4]);
        Application app;app.executable=argv[2];app.arguments=argv[3];app.launch();
        const auto offset=app.frame_area({});
        std::cout<<"{\"event\":\"launched\",\"probePid\":"<<GetCurrentProcessId()<<",\"frameOffset\":["<<offset.x<<','<<offset.y<<"]}\n"<<std::flush;
        std::string line;while(std::getline(std::cin,line)){
            std::istringstream command(line);std::string kind;command>>kind;
            if(kind=="stop"){app.finish_display();break;}
            try{
            if(kind=="wait"){unsigned ms;command>>ms;pause(ms);app.wait_content();}
            else if(kind=="elements"){png(wide(artifact("last-window.png")),settled(app,1000));app.elements();}
            else if(kind=="capture"){std::string path;std::getline(command,path);png(wide(path.substr(1)),settled(app,50));}
            else if(kind=="click"){int x,y;command>>x>>y;app.finalReady={};app.click(x,y);}
            else if(kind=="bench")benchmark(app,command);
            else if(kind=="lifecycle")lifecycle(app,command);
            else throw std::runtime_error("unknown benchmark command");
            }catch(...){app.finish_display();throw;}
            std::cout<<"{\"event\":\"complete\",\"command\":"<<json(line)<<"}\n"<<std::flush;
        }
    } catch(const hresult_error& e){std::cerr<<"HRESULT "<<std::hex<<unsigned(e.code().value)<<"\n";return 1;}
    catch(const std::exception& e){std::cerr<<e.what()<<"\n";return 1;}
    return 0;
}
