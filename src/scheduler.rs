use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::monitor::{Monitor, SysStatus};
use crate::task::{Task, TaskStatus};
use crate::utils::cli::Args;

pub struct Scheduler {
    // spaces for tasks
    waiting_queue: VecDeque<Task>,
    running_pool: Vec<Task>,
    timeout_pool: Vec<Task>,
    force_stop_pool: Vec<Task>,
    exited_pool: Vec<Task>,

    // propreties of scheduler
    // how many ticks per second
    tick_rate: f64,
    tick_time: u128,   // miliseconds
    timeout: f64,      // seconds
    timeout_wait: f64, // seconds

    max_workers: usize,
    force_workers: usize,

    monitor: Monitor,
    stop_flag: bool,

    run_dir: String,
}

impl Scheduler {
    pub fn new(args: &Args) -> Self {
        let tick_time = (1000.0 / args.tick_rate) as u128;
        let monitor = Monitor::new(args);
        let res = Scheduler {
            waiting_queue: VecDeque::new(),
            running_pool: Vec::new(),
            timeout_pool: Vec::new(),
            force_stop_pool: Vec::new(),
            exited_pool: Vec::new(),

            tick_rate: args.tick_rate,
            tick_time,
            timeout: args.timeout,
            timeout_wait: args.timeout_wait,

            max_workers: args.workers,
            force_workers: args.force_workers,

            monitor,
            stop_flag: false,

            run_dir: args.run_dir.clone(),
        };
        res.init_runtime();
        res
    }

    fn init_runtime(&self) {
        std::fs::create_dir_all(&self.run_dir).expect("Failed to create runtime directory");
    }

    pub fn submit(&mut self, task: Task) {
        self.waiting_queue.push_back(task);
    }

    pub fn start(&mut self) {
        self.run();
    }

    pub fn stop(&mut self) {
        self.stop_flag = true;
    }

    fn run(&mut self) {
        loop {
            let tick_start = Instant::now();
            let tasks =
                self.waiting_queue.len() + self.running_pool.len() + self.timeout_pool.len();

            println!(
                "Working..., {} task(s) remained, runing: {}, timeout: {}, waiting: {}",
                tasks,
                self.running_pool.len(),
                self.timeout_pool.len(),
                self.waiting_queue.len()
            );

            if tasks == 0 || self.stop_flag {
                // all task is done.
                break;
            }

            // do schedule
            // Firstly, check running pool for finished and timeout task
            let mut remain_running_tasks = Vec::new();
            for mut task in self.running_pool.drain(..) {
                // check if the task is done
                match task.try_wait() {
                    Ok(Some(_)) => {
                        task.set_status(TaskStatus::Exited);
                        self.exited_pool.push(task);
                    }
                    Ok(None) => {
                        // task is still running
                        // if task is timeout
                        if self.timeout > 0.0 && task.running_time().as_secs_f64() >= self.timeout {
                            task.set_status(TaskStatus::Timeout);
                            task.reset_waiting_time();
                            self.timeout_pool.push(task);
                        } else {
                            remain_running_tasks.push(task);
                        }
                    }
                    Err(_) => {
                        // something going wrong, drop this task
                        continue;
                    }
                }
            }
            self.running_pool = remain_running_tasks;
            // Secondly, Check System Status
            let running_tasks = self.running_pool.len() + self.timeout_pool.len();
            let workers = self.running_pool.len() + self.timeout_pool.len();
            if workers < self.force_workers {
                // if the force worker is larger than workers
                // run tasks directly
                if !self.waiting_queue.is_empty() {
                    let mut task = self.waiting_queue.pop_front().unwrap();
                    task.stdout_from_file(Path::new(&format!(
                        "{}/{}.log",
                        self.run_dir,
                        task.get_name()
                    )));
                    task.spawn();
                    self.running_pool.push(task);
                }
            } else {
                match self.monitor.is_ok(running_tasks) {
                    SysStatus::Health => {
                        // if system load is health, try to add a task to run,
                        if !self.waiting_queue.is_empty() && workers < self.max_workers {
                            let mut task = self.waiting_queue.pop_front().unwrap();
                            task.stdout_from_file(Path::new(&format!(
                                "{}/{}.log",
                                self.run_dir,
                                task.get_name()
                            )));
                            let ret = task.spawn();
                            if ret {
                                self.running_pool.push(task);
                            } else {
                                // failed to spawn a new process, back to wait
                                self.waiting_queue.push_back(task);
                            }
                        }
                    }
                    SysStatus::Normal => {
                        // do nothing,
                    }
                    SysStatus::Bad => {
                        // try to stop a task
                        if workers > self.force_workers && !self.running_pool.is_empty() {
                            let mut task = self.running_pool.pop().unwrap();
                            task.stop().expect("Failed to kill task");
                            self.waiting_queue.push_back(task);
                        }
                    }
                }
            }

            // Finally, check the timeout pool to waiting process exit itself or kill it.
            let mut remain_timeout_tasks = Vec::new();
            for mut task in self.timeout_pool.drain(..) {
                match task.try_wait() {
                    Ok(Some(_)) => {
                        // task stop itself
                        self.exited_pool.push(task);
                    }
                    Ok(None) => {
                        let elapsed = task.waiting_time().as_secs_f64();
                        if elapsed >= self.timeout_wait {
                            // send kill to task all childern to help exit
                            let _ = task.signal(rustix::process::Signal::Kill, false);
                            // move to force stop pool
                            self.force_stop_pool.push(task);
                        } else {
                            // signal alarm to process
                            let _ = task.signal(rustix::process::Signal::Alarm, true);
                            remain_timeout_tasks.push(task);
                        }
                    }
                    Err(_) => {
                        // something going wrong, drop this task
                        continue;
                    }
                }
            }

            self.timeout_pool = remain_timeout_tasks;

            // cleanup force stop pool
            for mut task in self.force_stop_pool.drain(..) {
                match task.try_wait() {
                    Ok(Some(_)) => {
                        // task finally stop itself
                        self.exited_pool.push(task);
                    }
                    Ok(None) => {
                        // we should stop the task forcely
                        let _ = task.stop();
                        self.exited_pool.push(task);
                    }
                    Err(_) => {
                        // something going wrong, drop this task
                        continue;
                    }
                }
            }
            // reinit this pool
            self.force_stop_pool = Vec::new();

            let tick_runing_time = tick_start.elapsed().as_millis();
            let tick_sleep_time = (self.tick_time - tick_runing_time) as u64;

            sleep(Duration::from_millis(tick_sleep_time));
        }
    }

    pub fn write_report(&self) {
        let log_path = format!("{}/cirno_task_pair.log", self.run_dir);
        let mut file = std::fs::File::create(log_path).unwrap();

        for task in &self.exited_pool {
            let line = format!(
                "{},{},{}\n",
                task.get_name(),
                task.get_cmd(),
                task.get_status()
            );

            let _ = file.write(line.as_bytes());
        }
    }
}
