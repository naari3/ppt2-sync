use named_pipe::PipeClient;
use std::io::prelude::*;
use std::process::{Command, Stdio};

pub struct Ppt2Syncronizer {
    connection: PipeClient,
    first_frame: bool,
}

impl Ppt2Syncronizer {
    pub fn new() -> std::io::Result<Self> {
        let connection = PipeClient::connect("\\\\.\\pipe\\ppt2-sync").or_else(|_| {
            Command::new("ppt2-sync")
                .stdout(Stdio::piped())
                .spawn()
                .and_then(|child| child.stdout.unwrap().read_exact(&mut [0]))
                .and_then(|_| PipeClient::connect("\\\\.\\pipe\\ppt2-sync"))
        })?;
        Ok(Ppt2Syncronizer {
            connection,
            first_frame: true,
        })
    }

    pub fn next_frame(&mut self) -> bool {
        if !self.first_frame {
            self.connection.write_all(&[0]).ok();
        }
        self.first_frame = false;
        self.connection.read_exact(&mut [0]).is_ok()
    }
}

#[no_mangle]
pub extern "C" fn ppt2sync_new() -> *mut Ppt2Syncronizer {
    match Ppt2Syncronizer::new() {
        Ok(v) => Box::into_raw(Box::new(v)),
        Err(e) => {
            eprintln!("Failed to set up ppt2-sync: {}", e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn ppt2sync_wait_for_frame(sync: &mut Ppt2Syncronizer) -> bool {
    sync.next_frame()
}

#[no_mangle]
pub extern "C" fn ppt2sync_destroy(sync: *mut Ppt2Syncronizer) {
    unsafe {
        Box::from_raw(sync);
    }
}
