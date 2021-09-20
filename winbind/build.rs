fn main() {
    windows::build! {
        Windows::Win32::Storage::FileSystem::ReadFileEx,
        Windows::Win32::System::SystemServices::{OVERLAPPED},
        Windows::Win32::System::Diagnostics::Debug::GetLastError,
       Windows::Win32::System::Threading::{CreateSemaphoreA,WaitForSingleObjectEx,ReleaseSemaphore},
        Windows::Win32::Foundation::{PSTR,CloseHandle},
        Windows::Win32::System::Diagnostics::Debug::WIN32_ERROR,
    }
}