use cirno::task::gen_tasks_from_file;
use cirno::{scheduler::Scheduler, utils::cli::Args};
use clap::Parser;
use std::path::Path;

fn main() {
    let cli_args = Args::parse();
    let input_list = &cli_args.input_list;

    let mut scheduler = Scheduler::new(&cli_args);
    for task in gen_tasks_from_file(Path::new(input_list)) {
        scheduler.submit(task);
    }

    scheduler.start();
    scheduler.stop();
    scheduler.write_report();
}
