use std::{env, thread, time::Duration};

fn main() {
    let mb: usize = env::args().nth(1).unwrap_or("300".into()).parse().unwrap();
    let bytes = mb * 1024 * 1024;
    let mut v = Vec::<u8>::with_capacity(bytes);
    v.resize(bytes, 0u8);
    println!("Allocated {} MiB, sleeping...", mb);
    thread::sleep(Duration::from_secs(300));
}
