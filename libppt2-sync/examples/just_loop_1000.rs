use libppt2_sync::Ppt2Syncronizer;

fn main() -> std::io::Result<()> {
    let mut ppt2sync = Ppt2Syncronizer::new()?;
    let mut frame_count = 0;
    loop {
        if !ppt2sync.next_frame() {
            println!("fatal!");
            break;
        }
        println!("frame");
        frame_count += 1;
        if frame_count > 1000 {
            break;
        }
    }
    println!("completed");

    Ok(())
}
