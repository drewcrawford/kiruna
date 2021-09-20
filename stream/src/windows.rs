pub mod read;
mod threadpool;

use winbind::Windows::Win32::System::Diagnostics::Debug::WIN32_ERROR;

#[derive(Debug)]
pub struct OSError(WIN32_ERROR);
