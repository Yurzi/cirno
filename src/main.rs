use cirno::task::gen_tasks_from_file;
use cirno::{scheduler::Scheduler, utils::cli::Args};
use clap::Parser;
use std::path::Path;

fn main() {
    let cli_args = Args::parse();
    let input_list = &cli_args.input_list;
    let with_task_name = cli_args.with_task_name;

    let mut scheduler = Scheduler::new(&cli_args);
    for task in gen_tasks_from_file(Path::new(input_list), with_task_name) {
        scheduler.submit(task);
    }
    let _ = signal_hook::flag::register(signal_hook::consts::SIGINT, scheduler.get_stop_flag_ref());
    let _ =
        signal_hook::flag::register(signal_hook::consts::SIGTERM, scheduler.get_stop_flag_ref());

    scheduler.start();
    scheduler.write_report();
}
