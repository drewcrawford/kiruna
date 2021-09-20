fn main() {
    /*although this crate is only useful on windows, if you use the `--all` flag on another platform you will build it
maybe this is not what we want */
    #[cfg(target_os = "windows")]
    windows::build! {
        Windows::Win32::Storage::FileSystem::ReadFileEx,
        Windows::Win32::System::SystemServices::{OVERLAPPED},
        Windows::Win32::System::Diagnostics::Debug::GetLastError,
       Windows::Win32::System::Threading::{CreateSemaphoreA,WaitForSingleObjectEx,ReleaseSemaphore},
        Windows::Win32::Foundation::{PSTR,CloseHandle},
        Windows::Win32::System::Diagnostics::Debug::{WIN32_ERROR,FormatMessageA},
        Windows::Win32::System::Memory::LocalFree,

    }
}