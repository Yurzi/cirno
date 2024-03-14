use clap::Parser;
use clap::{arg, command};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    input_list: String,
    #[arg(short, long)]
    workers: usize,
    #[arg(short, long, default_value_t = 2)]
    pub force_workers: usize,
    #[arg(short, long, default_value_t = 1.0)]
    pub tick_rate: f64,
    #[arg(short, long, default_value_t = 0.9, help = "use ratio of total mem")]
    pub high_mem_thres: f64,
    #[arg(short, long, default_value_t = 0.1, help = "use ratio of total mem")]
    pub low_mem_thres: f64,
    #[arg(short, long, default_value_t = 4294967296, help = "Byte as unit")]
    pub per_task_mem: usize,
    #[arg(short, long, default_value_t = 0, help = "Byte as unit")]
    pub reversed_mem: usize,
    #[arg(short, long, default_value_t = 0.8)]
    pub load_avg_thres: f64,
}
