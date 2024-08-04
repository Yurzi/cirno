use clap::Parser;
use clap::{arg, command};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    pub input_list: String,

    #[arg(short, long)]
    pub workers: usize,

    #[arg(short, long, default_value_t = 2)]
    pub force_workers: usize,

    #[arg(short, long, default_value_t = -1.0, help = "set smaller than 0 to disable timeout, seconds")]
    pub timeout: f64,

    #[arg(
        long,
        default_value_t = 15.0,
        help = "wait for timeout process to quit, seconds"
    )]
    pub timeout_wait: f64,

    #[arg(long, default_value_t = 1.0)]
    pub tick_rate: f64,

    #[arg(long, default_value_t = 0.9, help = "use ratio of total mem")]
    pub high_mem_thres: f64,

    #[arg(long, default_value_t = 0.7, help = "use ratio of total mem")]
    pub low_mem_thres: f64,

    #[arg(short, long, default_value_t = 4294967296, help = "Byte as unit")]
    pub per_task_mem: usize,

    #[arg(short, long, default_value_t = 0, help = "Byte as unit")]
    pub reversed_mem: usize,

    #[arg(short, long, default_value_t = 0.8)]
    pub load_avg_thres: f64,

    #[arg(short = 'd', long, default_value = "run")]
    pub run_dir: String,

    #[arg(long, action, help = "if cirno will consider gpu mem")]
    pub with_gpu: bool,

    #[arg(
        long,
        action,
        help = "if input task list has task name as first element"
    )]
    pub with_task_name: bool,

    #[arg(
        long,
        default_value_t = 0.72,
        help = "thershold for free mem in a card to be consdier as free card"
    )]
    pub gpu_mem_thres: f64,
}
