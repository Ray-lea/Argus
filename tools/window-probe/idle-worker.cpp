// Minimal waiting process for list-size tests; no GUI or graphics libraries.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
extern "C" void mainCRTStartup(){
    const wchar_t* text=GetCommandLineW();
    if(*text==L'"'){++text;while(*text&&*text!=L'"')++text;if(*text)++text;}
    else while(*text&&*text!=L' ')++text;
    while(*text==L' ')++text;
    DWORD parent=0;while(*text>=L'0'&&*text<=L'9'){parent=parent*10+DWORD(*text-L'0');++text;}
    if(!parent)ExitProcess(2);
    HANDLE handle=OpenProcess(SYNCHRONIZE,FALSE,parent);if(!handle)ExitProcess(3);
    SetPriorityClass(GetCurrentProcess(),IDLE_PRIORITY_CLASS);
    WaitForSingleObject(handle,1800000);CloseHandle(handle);ExitProcess(0);
}
