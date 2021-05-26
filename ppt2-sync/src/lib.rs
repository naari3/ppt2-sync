#[warn(non_snake_case)]
use winapi::shared::minwindef::*;

use winapi::um::consoleapi::AllocConsole;
use winapi::um::winnt::DLL_PROCESS_ATTACH;
use winapi::um::winuser::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

#[no_mangle]
pub unsafe extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: u32) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            AllocConsole();
            let s = "DLL_PROCESS_ATTACH\0".to_string();
            msg(&s);
        }
        _ => {
            // let s = "DEFAULT\0".to_string();
            // msg(&s);
        }
    }
    TRUE
}

fn msg(caption: &str) {
    let lp_text: Vec<u16> = "Hello World! \u{1F60E}\0".encode_utf16().collect();
    let lp_caption: Vec<u16> = caption.encode_utf16().collect();

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            lp_text.as_ptr(),
            lp_caption.as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
