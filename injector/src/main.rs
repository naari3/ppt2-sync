use std::ffi::CString;
use std::fs::canonicalize;
use std::ptr::null_mut;

use winapi::shared::minwindef::*;
use winapi::shared::ntdef::NULL;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::libloaderapi::GetModuleHandleA;
use winapi::um::libloaderapi::GetProcAddress;
use winapi::um::memoryapi::VirtualAllocEx;
use winapi::um::memoryapi::WriteProcessMemory;
use winapi::um::processthreadsapi::CreateRemoteThread;
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::psapi::EnumProcessModules;
use winapi::um::psapi::EnumProcesses;
use winapi::um::psapi::GetModuleBaseNameW;
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::MEM_COMMIT;
use winapi::um::winnt::MEM_RESERVE;
use winapi::um::winnt::PAGE_EXECUTE_READWRITE;
use winapi::um::winnt::PROCESS_CREATE_THREAD;
use winapi::um::winnt::PROCESS_QUERY_INFORMATION;
use winapi::um::winnt::PROCESS_VM_OPERATION;
use winapi::um::winnt::PROCESS_VM_READ;
use winapi::um::winnt::PROCESS_VM_WRITE;

fn main() {
    println!("====== injector ======");
    let pid = find_ppt_process().expect("Could not find ppt2 process.");
    let path = canonicalize("..\\target\\debug\\ppt2_sync.dll").unwrap();
    unsafe {
        inject_dll(pid, path.to_str().unwrap()).unwrap();
    }
}

macro_rules! w {
    ($f:ident($($content:tt)*)) => {
        match $f($($content)*) {
            0 => {
                eprintln!(
                    "{} (line {}) failed with error code {}",
                    stringify!(f), line!(), GetLastError()
                );
                None
            }
            v => Some(v)
        }
    };
}

fn find_ppt_process() -> Option<u32> {
    #[cfg(target_os = "windows")]
    use std::os::windows::ffi::OsStringExt;
    unsafe {
        let mut pids = [0; 4096];
        let mut used = 0;

        w!(EnumProcesses(
            pids.as_mut_ptr(),
            std::mem::size_of_val(&pids) as u32,
            &mut used
        ))
        .unwrap();

        for &process in &pids[..used as usize / std::mem::size_of::<u32>()] {
            let handle = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, process);
            if !handle.is_null() {
                let mut module = 0 as *mut _;
                if EnumProcessModules(
                    handle,
                    &mut module,
                    std::mem::size_of::<*mut ()>() as u32,
                    &mut used,
                ) != 0
                {
                    let mut buffer = vec![0; 4096];
                    GetModuleBaseNameW(
                        handle,
                        module,
                        buffer.as_mut_ptr(),
                        2 * buffer.len() as u32,
                    );
                    for i in 0..buffer.len() {
                        if buffer[i] == 0 {
                            let s = std::ffi::OsString::from_wide(&buffer[..i]);
                            if let Some(s) = s.to_str() {
                                if s == "PuyoPuyoTetris2.exe" {
                                    CloseHandle(handle);
                                    return Some(process);
                                }
                            }
                            break;
                        }
                    }
                }

                CloseHandle(handle);
            }
        }
        None
    }
}

unsafe fn inject_dll<'a>(pid: u32, dll_path: &str) -> Result<(), &'a str> {
    let process = OpenProcess(
        PROCESS_CREATE_THREAD | PROCESS_VM_OPERATION | PROCESS_VM_WRITE,
        FALSE,
        pid,
    );
    if process == NULL {
        panic!("GetLastError: {}", GetLastError());
    }

    let dll_path_str = CString::new(dll_path).unwrap();
    let dll_path_size = dll_path_str.as_bytes_with_nul().len();

    let remote_buff = VirtualAllocEx(
        process,
        null_mut(),
        dll_path_size,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    );
    if remote_buff == NULL {
        panic!("GetLastError: {}", GetLastError());
    }

    let dw_size = dll_path.len() + 1;
    let mut dw_write = 0;
    w!(WriteProcessMemory(
        process,
        remote_buff,
        dll_path_str.as_ptr() as LPVOID,
        dll_path_size,
        &mut dw_write,
    ));

    if dw_write != dw_size {
        panic!("GetLastError: {}", GetLastError());
    }

    let lla = get_fn_addr("Kernel32.dll", "LoadLibraryA")?;
    type ThreadStartRoutine = unsafe extern "system" fn(LPVOID) -> DWORD;
    let start_routine: ThreadStartRoutine = std::mem::transmute(lla);

    let remote_thread = CreateRemoteThread(
        process,
        null_mut(),
        0,
        Some(start_routine),
        remote_buff,
        0,
        null_mut(),
    );

    WaitForSingleObject(remote_thread, INFINITE);

    Ok(())
}

fn get_fn_addr<'a>(mod_name: &str, fn_name: &str) -> Result<u64, &'a str> {
    let mod_str = CString::new(mod_name).unwrap();
    let fn_str = CString::new(fn_name).unwrap();

    let mod_handle = unsafe { GetModuleHandleA(mod_str.as_ptr()) };

    if mod_handle == null_mut() {
        return Err("Could not get module handler");
    }

    let fn_addr = unsafe { GetProcAddress(mod_handle, fn_str.as_ptr()) };

    if fn_addr == null_mut() {
        return Err("Could not get function address");
    }

    Ok(fn_addr as u64)
}
