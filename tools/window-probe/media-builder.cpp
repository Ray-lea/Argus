// Build a data ISO for the isolated guest, using the Windows IMAPI service.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <imapi2fs.h>
#include <shlwapi.h>
#include <wrl/client.h>
#include <cstdio>
using Microsoft::WRL::ComPtr;
static void check(HRESULT result){if(FAILED(result))throw result;}
int wmain(int argc,wchar_t** argv){
    if(argc!=3)return 2;
    try{
        check(CoInitializeEx(nullptr,COINIT_MULTITHREADED));
        ComPtr<IFileSystemImage> image;
        check(CoCreateInstance(CLSID_MsftFileSystemImage,nullptr,CLSCTX_INPROC_SERVER,IID_PPV_ARGS(&image)));
        check(image->put_FileSystemsToCreate(FsiFileSystems(FsiFileSystemISO9660|FsiFileSystemJoliet)));
        BSTR name=SysAllocString(L"ARGUS_BENCH");check(image->put_VolumeName(name));SysFreeString(name);
        ComPtr<IFsiDirectoryItem> root;check(image->get_Root(&root));
        BSTR directory=SysAllocString(argv[1]);check(root->AddTree(directory,VARIANT_FALSE));SysFreeString(directory);
        ComPtr<IFileSystemImageResult> result;check(image->CreateResultImage(&result));
        ComPtr<IStream> input,output;check(result->get_ImageStream(&input));
        check(SHCreateStreamOnFileEx(argv[2],STGM_CREATE|STGM_WRITE|STGM_SHARE_EXCLUSIVE,FILE_ATTRIBUTE_NORMAL,TRUE,nullptr,&output));
        STATSTG info{};check(input->Stat(&info,STATFLAG_NONAME));
        check(input->CopyTo(output.Get(),info.cbSize,nullptr,nullptr));check(output->Commit(STGC_DEFAULT));
        std::printf("ISO bytes: %llu\n",info.cbSize.QuadPart);return 0;
    }catch(HRESULT error){std::fprintf(stderr,"ISO HRESULT: %08lx\n",static_cast<unsigned long>(error));return 1;}
}
