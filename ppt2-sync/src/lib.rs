use std::ffi::CStr;
use std::io::Read;
use std::io::Write;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use named_pipe::PipeOptions;
use once_cell::sync::Lazy;

#[warn(non_snake_case)]
use winapi::shared::minwindef::*;
use winapi::shared::ntdef::NULL;

// use winapi::um::consoleapi::AllocConsole;
use winapi::um::errhandlingapi::AddVectoredExceptionHandler;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::memoryapi::ReadProcessMemory;
use winapi::um::memoryapi::WriteProcessMemory;
use winapi::um::minwinbase::EXCEPTION_BREAKPOINT;
use winapi::um::processthreadsapi::GetCurrentProcess;
use winapi::um::processthreadsapi::GetCurrentProcessId;
use winapi::um::tlhelp32::CreateToolhelp32Snapshot;
use winapi::um::tlhelp32::Module32First;
use winapi::um::tlhelp32::Module32Next;
use winapi::um::tlhelp32::MODULEENTRY32;
use winapi::um::tlhelp32::TH32CS_SNAPMODULE;
use winapi::um::tlhelp32::TH32CS_SNAPMODULE32;
use winapi::um::winnt::DLL_PROCESS_ATTACH;
use winapi::um::winnt::EXCEPTION_POINTERS;
// use winapi::um::winuser::{MessageBoxW, MB_ICONINFORMATION, MB_OK};
use winapi::vc::excpt::EXCEPTION_CONTINUE_EXECUTION;
use winapi::vc::excpt::EXCEPTION_CONTINUE_SEARCH;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

macro_rules! w {
    ($f:ident($($content:tt)*)) => {
        if $f($($content)*) == FALSE {
            let err = GetLastError();
            let error_str = format!(
                "{} (line {}) failed with error code {}",
                stringify!(f), line!(), err
            );
            return Err(String::from(error_str).into())
        }
    };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Breakpoint {
    address: u64,
    original_byte: u8,
}

static INSTRUCTION_BREAKPOINT: Lazy<Mutex<Breakpoint>> =
    Lazy::new(|| Mutex::new(Breakpoint::default()));

impl Breakpoint {
    unsafe fn set(&mut self) -> Result<()> {
        let process = GetCurrentProcess();
        w!(ReadProcessMemory(
            process,
            self.address as _,
            &mut self.original_byte as *mut _ as *mut _,
            1,
            NULL as _
        ));

        w!(WriteProcessMemory(
            process,
            self.address as _,
            &0xCC as *const _ as *const _,
            1,
            NULL as _
        ));
        w!(CloseHandle(process));
        Ok(())
    }

    unsafe fn remove(&mut self) -> Result<()> {
        let process = GetCurrentProcess();

        w!(WriteProcessMemory(
            process,
            self.address as _,
            &mut self.original_byte as *mut _ as *mut _,
            1,
            NULL as _
        ));
        w!(CloseHandle(process));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncStatus {
    BreakpointReached,
    SentNotification,
    Continued,
}

static SYNC_STATUS: Lazy<Mutex<SyncStatus>> = Lazy::new(|| Mutex::new(SyncStatus::Continued));

#[no_mangle]
pub unsafe extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: u32) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // AllocConsole();
            match ppt2_main() {
                Ok(_) => {
                    println!("safe")
                }
                Err(err) => {
                    println!("fatal: {}", err);
                }
            };
            // FreeConsole();
        }
        _ => {}
    }
    TRUE
}

fn ppt2_main() -> Result<()> {
    let mut listener = PipeOptions::new("\\\\.\\pipe\\ppt2-sync").single()?;
    println!();

    let (done, waiter) = channel();
    let (notifs, conns) = channel();

    std::thread::spawn(move || {
        let _: Result<()> = (|| loop {
            let mut connection = listener.wait()?;
            listener = PipeOptions::new("\\\\.\\pipe\\ppt2-sync")
                .first(false)
                .single()?;
            let (notifier, wait) = channel();
            notifs.send(notifier)?;
            let done = done.clone();
            std::thread::spawn(move || {
                let _: Result<_> = (|| loop {
                    wait.recv()?;
                    let good = (|| {
                        connection.write(&[0])?;
                        connection.flush()?;
                        connection.read_exact(&mut [0])
                    })()
                    .is_ok();
                    if !good {
                        drop(wait);
                        done.send(())?;
                        return Ok(());
                    }
                    done.send(())?;
                })();
            });
        })();
    });

    unsafe {
        sync(waiter, conns)?;
    }

    Ok(())
}

// fn msg(caption: &str, message: &str) {
//     let lp_text: Vec<u16> = message.encode_utf16().collect();
//     let lp_caption: Vec<u16> = caption.encode_utf16().collect();

//     unsafe {
//         MessageBoxW(
//             std::ptr::null_mut(),
//             lp_text.as_ptr(),
//             lp_caption.as_ptr(),
//             MB_OK | MB_ICONINFORMATION,
//         );
//     }
// }

unsafe extern "system" fn veh(exception: *mut EXCEPTION_POINTERS) -> i32 {
    if (*(*exception).ExceptionRecord).ExceptionCode == EXCEPTION_BREAKPOINT {
        // println!("Breakpoint reached!");
        *(SYNC_STATUS.lock().unwrap()) = SyncStatus::BreakpointReached;

        // println!("Remove breakpoint");
        INSTRUCTION_BREAKPOINT.lock().unwrap().remove().unwrap();

        // println!("Wait to sent notification");
        loop {
            if matches!(*(SYNC_STATUS.lock().unwrap()), SyncStatus::SentNotification) {
                break;
            }
        }

        *(SYNC_STATUS.lock().unwrap()) = SyncStatus::Continued;

        return EXCEPTION_CONTINUE_EXECUTION;
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

unsafe fn sync(waiter: Receiver<()>, new: Receiver<Sender<()>>) -> Result<()> {
    let pid = GetCurrentProcessId();
    let base_address = get_module_base_address(pid, "PuyoPuyoTetris2.exe")?;
    // println!("base address: 0x{:x}", base_address);
    let instruction_address = base_address + 0x004B3306;
    INSTRUCTION_BREAKPOINT.lock()?.address = instruction_address;

    AddVectoredExceptionHandler(1, Some(veh));

    let mut clients = vec![];
    if let Ok(c) = new.recv() {
        clients.push(c);
    }

    loop {
        INSTRUCTION_BREAKPOINT.lock()?.set()?;

        // println!("Wait to reaching breakpoint");
        loop {
            if matches!(*(SYNC_STATUS.lock()?), SyncStatus::BreakpointReached) {
                break;
            }
        }

        // collect new clients
        for c in new.try_iter() {
            clients.push(c);
        }
        // notify clients
        clients.retain(|c| c.send(()).is_ok());

        // wait for clients to respond
        for _ in 0..clients.len() {
            waiter.recv().ok();
        }

        if clients.is_empty() {
            break;
        }

        // let caption = "Sync\0".to_string();
        // let message = "Send\0".to_string();
        // msg(&caption, &message);
        // println!("Sent!");

        *(SYNC_STATUS.lock()?) = SyncStatus::SentNotification;

        // println!("Wait to continue");
        loop {
            if matches!(*(SYNC_STATUS.lock()?), SyncStatus::Continued) {
                break;
            }
        }
    }

    Ok(())
}

unsafe fn get_module_base_address(pid: u32, mod_name: &str) -> Result<u64> {
    let ss = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
    if ss == INVALID_HANDLE_VALUE {
        return Err(String::from("Could not get snapshot").into());
    }
    let mut entry = MODULEENTRY32::default();
    entry.dwSize = std::mem::size_of::<MODULEENTRY32>() as u32;
    let addr;
    w!(Module32First(ss, &mut entry));
    loop {
        let mut sz_module = [0; 256];
        sz_module.copy_from_slice(&entry.szModule);
        let entry_mod_name = CStr::from_ptr(entry.szModule.as_mut_ptr())
            .to_string_lossy()
            .into_owned();
        if entry_mod_name == mod_name {
            addr = entry.modBaseAddr as u64;
            break;
        }
        w!(Module32Next(ss, &mut entry));
    }

    w!(CloseHandle(ss));
    if addr == 0 {
        return Err(String::from("Could not get module name").into());
    }

    Ok(addr)
}
