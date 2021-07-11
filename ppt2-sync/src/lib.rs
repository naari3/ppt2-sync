use std::ffi::CStr;
use std::io::Read;
use std::io::Write;
use std::sync::mpsc::channel;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use named_pipe::PipeOptions;
use once_cell::sync::OnceCell;

#[warn(non_snake_case)]
use winapi::shared::minwindef::*;

use winapi::um::errhandlingapi::AddVectoredExceptionHandler;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::handleapi::INVALID_HANDLE_VALUE;
use winapi::um::memoryapi::VirtualProtect;
use winapi::um::minwinbase::EXCEPTION_BREAKPOINT;
use winapi::um::processthreadsapi::GetCurrentProcessId;
use winapi::um::tlhelp32::CreateToolhelp32Snapshot;
use winapi::um::tlhelp32::Module32First;
use winapi::um::tlhelp32::Module32Next;
use winapi::um::tlhelp32::MODULEENTRY32;
use winapi::um::tlhelp32::TH32CS_SNAPMODULE;
use winapi::um::tlhelp32::TH32CS_SNAPMODULE32;
use winapi::um::winnt::DLL_PROCESS_ATTACH;
use winapi::um::winnt::EXCEPTION_POINTERS;
use winapi::um::winnt::PAGE_EXECUTE_READWRITE;
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

static SYNC_CONTEXT: OnceCell<Mutex<Synchronizer>> = OnceCell::new();

const INSTRUCTION_OFFSET: u64 = 0x004B32BE;
const NEXT_INSTRUCTION: u64 = 0x4B32C1;

#[no_mangle]
pub unsafe extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: u32) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            let _ = ppt2_main();
        }
        _ => {}
    }
    TRUE
}

struct Synchronizer {
    new_clients: Receiver<Sender<()>>,
    waiter: Receiver<()>,
    clients: Vec<Sender<()>>,
    instruction: *mut u8,
    next_instruction: *mut u8,
    original_value: u8,
}

unsafe impl Send for Synchronizer {}
unsafe impl Sync for Synchronizer {}

fn ppt2_main() -> Result<()> {
    let (done, waiter) = channel();
    let (notifs, new_clients) = channel();

    std::thread::spawn(move || {
        let mut listener = PipeOptions::new("\\\\.\\pipe\\ppt2-sync").single().unwrap();
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
        let pid = GetCurrentProcessId();
        let base_address = get_module_base_address(pid, "PuyoPuyoTetris2.exe")?;
        let instruction = (base_address + INSTRUCTION_OFFSET) as *mut u8;
        let next_instruction = (base_address + NEXT_INSTRUCTION) as *mut u8;

        let _ = SYNC_CONTEXT.set(Mutex::new(Synchronizer {
            new_clients,
            waiter,
            clients: vec![],
            original_value: *instruction,
            instruction,
            next_instruction,
        }));

        AddVectoredExceptionHandler(1, Some(veh));

        let mut old = 0;
        VirtualProtect(instruction as _, 4, PAGE_EXECUTE_READWRITE, &mut old);

        // set breakpoint
        *instruction = 0xCC;
    }

    Ok(())
}

unsafe extern "system" fn veh(exception: *mut EXCEPTION_POINTERS) -> i32 {
    let exception = &*(*exception).ExceptionRecord;
    if exception.ExceptionCode == EXCEPTION_BREAKPOINT {
        let mut sync = SYNC_CONTEXT.get().unwrap().lock().unwrap();
        let sync = &mut *sync;

        if exception.ExceptionAddress == sync.instruction as _ {
            // collect new clients
            for c in sync.new_clients.try_iter() {
                sync.clients.push(c);
            }

            // notify clients
            sync.clients.retain(|c| c.send(()).is_ok());

            // wait for clients to respond
            for _ in 0..sync.clients.len() {
                sync.waiter.recv().ok();
            }

            // go past breakpointed instruction
            *sync.instruction = sync.original_value;
            sync.original_value = *sync.next_instruction;
            *sync.next_instruction = 0xCC;

            EXCEPTION_CONTINUE_EXECUTION
        } else if exception.ExceptionAddress == sync.next_instruction as _ {
            *sync.next_instruction = sync.original_value;
            sync.original_value = *sync.instruction;
            *sync.instruction = 0xCC;

            EXCEPTION_CONTINUE_EXECUTION
        } else {
            EXCEPTION_CONTINUE_SEARCH
        }
    } else {
        EXCEPTION_CONTINUE_SEARCH
    }
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
