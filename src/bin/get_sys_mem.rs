use sysinfo::System;

fn main() {
    let monitor = System::new_all();
    let os_total_mem_used = monitor.used_memory() as usize;
    let os_total_mem_used = os_total_mem_used / (1024 * 1024 * 1024);
    println!("{os_total_mem_used} GB");
}
