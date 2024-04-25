use crate::utils::cli::Args;
use crate::utils::gpu::{GpuInfo, GpuVendor};
use crate::utils::process::get_process_tree;
use rustix::process::getpid;

use sysinfo::System;

#[derive(PartialEq, Eq, Debug)]
pub enum SysStatus {
    Health,
    Normal,
    Bad,
}

pub struct Monitor {
    system: System,

    high_mem_thres: usize,
    low_mem_thres: usize,

    per_task_mem: usize,
    reversed_mem: usize,

    load_avg_thres: f64,

    with_gpu: bool,
    gpu_mem_thres: f64,
}

impl Monitor {
    pub fn new(args: &Args) -> Self {
        let system = System::new_all();
        let total_res_mem = system.total_memory() as usize;
        let per_task_mem = args.per_task_mem;
        let reversed_mem = args.reversed_mem;
        let high_mem_thres = (args.high_mem_thres * total_res_mem as f64) as usize;
        let low_mem_thres = (args.low_mem_thres * total_res_mem as f64) as usize;

        let high_mem_thres = if high_mem_thres > total_res_mem - reversed_mem {
            total_res_mem - reversed_mem
        } else {
            high_mem_thres
        };

        let low_mem_thres = if low_mem_thres <= high_mem_thres {
            low_mem_thres
        } else {
            high_mem_thres
        };

        let with_gpu = args.with_gpu;
        let gpu_mem_thres = args.gpu_mem_thres.clamp(0.0, 1.0);
        Monitor {
            system,
            high_mem_thres,
            low_mem_thres,
            per_task_mem,
            reversed_mem,
            load_avg_thres: args.load_avg_thres,
            with_gpu,
            gpu_mem_thres,
        }
    }

    pub fn is_ok(&mut self, running_task_amount: usize) -> SysStatus {
        // update monitor
        self.system.refresh_memory();

        // check system load average
        let load_avg = System::load_average().five / self.system.cpus().len() as f64;
        if load_avg > self.load_avg_thres * 2.0 {
            return SysStatus::Bad;
        }

        // try to statistc per task mem usage
        let process_list = get_process_tree(getpid(), false).unwrap();
        let mut total_mem = 0;
        for process in process_list {
            total_mem += process.mem();
        }

        // `Byte` unit
        let os_per_task_mem = if running_task_amount == 0 {
            0
        } else {
            total_mem / running_task_amount
        };
        let per_task_mem = if self.per_task_mem >= os_per_task_mem {
            self.per_task_mem
        } else {
            os_per_task_mem
        };

        let os_total_mem_used = self.system.used_memory() as usize;
        // if mem has free
        let predicate_mem_used = os_total_mem_used + per_task_mem;
        let mut sys_status_res = if predicate_mem_used <= self.low_mem_thres {
            SysStatus::Health
        } else if predicate_mem_used > self.high_mem_thres {
            SysStatus::Bad
        } else {
            SysStatus::Normal
        };

        // check gpu usage
        if self.with_gpu && sys_status_res == SysStatus::Health {
            let gpu_cards = GpuInfo::get_gpus_info(GpuVendor::Nvidia);
            let mut has_free_card = false;
            for card in gpu_cards {
                if card.memory_free / card.memory_total >= self.gpu_mem_thres {
                    has_free_card = true;
                    break;
                }
            }

            if !has_free_card {
                sys_status_res = SysStatus::Normal;
            }
        }

        sys_status_res
    }
}
