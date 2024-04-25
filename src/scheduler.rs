use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::monitor::{Monitor, SysStatus};
use crate::task::{Task, TaskStatus};
use crate::utils::cli::Args;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use indicatif_log_bridge::LogWrapper;
use log::{debug, warn};

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
        let style = ProgressStyle::with_template(
            "[{elapsed_precise}]|{bar:40.cyan/blue}|{pos:>5}/{len:5}|{msg}",
        )
        .unwrap()
        .progress_chars("=>-");
        let msg_style = ProgressStyle::with_template("{spinner} {msg}").unwrap();

        let multi_pbar = MultiProgress::new();
        let logger =
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .build();

        LogWrapper::new(multi_pbar.clone(), logger)
            .try_init()
            .unwrap();

        let pbar = multi_pbar.add(ProgressBar::new(self.waiting_queue.len() as u64));
        pbar.set_style(style);

        let pmsg_bar = multi_pbar.add(ProgressBar::new_spinner());
        pmsg_bar.set_style(msg_style);
        pmsg_bar.enable_steady_tick(Duration::from_millis(100));

        loop {
            let tick_start = Instant::now();
            pbar.tick();
            debug!("New loop start");
            let tasks =
                self.waiting_queue.len() + self.running_pool.len() + self.timeout_pool.len();

            pmsg_bar.set_message(format!(
                "[running: {}|timeout_wait: {}|exited: {}]",
                self.running_pool.len(),
                self.timeout_pool.len(),
                self.exited_pool.len()
            ));

            debug!("Checking if should stop");
            if tasks == 0 || self.stop_flag {
                // all task is done.
                debug!("Cirno Loop Exited");
                break;
            }

            // do schedule
            // Firstly, check running pool for finished and timeout task
            debug!("Checking running pool...");
            let mut remain_running_tasks = Vec::new();
            for mut task in self.running_pool.drain(..) {
                // check if the task is done
                match task.try_wait() {
                    Ok(Some(_)) => {
                        task.set_status(TaskStatus::Exited);
                        self.exited_pool.push(task);
                        pbar.inc(1);
                        debug!("Found Exited");
                    }
                    Ok(None) => {
                        // task is still running
                        // if task is timeout
                        if self.timeout > 0.0 && task.running_time().as_secs_f64() >= self.timeout {
                            task.set_status(TaskStatus::Timeout);
                            task.reset_waiting_time();
                            self.timeout_pool.push(task);
                            debug!("Found Timeout");
                        } else {
                            remain_running_tasks.push(task);
                        }
                    }
                    Err(e) => {
                        // something going wrong, drop this task
                        pbar.inc(1);
                        warn!("Found Error Task Wait: {}", e);
                        continue;
                    }
                }
            }
            self.running_pool = remain_running_tasks;
            // Secondly, Check System Status
            debug!("Checking System Status...");
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
                    let ret = task.spawn();
                    debug!("Start a new Task");
                    if ret {
                        self.running_pool.push(task);
                    } else {
                        warn!("Unable to spawn new child!");
                        self.waiting_queue.push_back(task);
                    }
                }
            } else {
                match self.monitor.is_ok(running_tasks) {
                    SysStatus::Health => {
                        pbar.set_message("[System: Health]");
                        // if system load is health, try to add a task to run,
                        if !self.waiting_queue.is_empty() && workers < self.max_workers {
                            let mut task = self.waiting_queue.pop_front().unwrap();
                            task.stdout_from_file(Path::new(&format!(
                                "{}/{}.log",
                                self.run_dir,
                                task.get_name()
                            )));
                            let ret = task.spawn();
                            debug!("Start a new Task");
                            if ret {
                                self.running_pool.push(task);
                            } else {
                                // failed to spawn a new process, back to wait
                                warn!("Unable to spawn new child!");
                                self.waiting_queue.push_back(task);
                            }
                        }
                    }
                    SysStatus::Normal => {
                        // do nothing,
                        pbar.set_message("[System: Normal]");
                    }
                    SysStatus::Bad => {
                        // try to stop a task
                        pbar.set_message("[System: Bad]");
                        if workers > self.force_workers && !self.running_pool.is_empty() {
                            let mut task = self.running_pool.pop().unwrap();
                            task.stop().expect("Failed to kill task");
                            self.waiting_queue.push_back(task);
                        }
                    }
                }
            }

            // cleanup force stop pool
            debug!("Checking Force Stop Pool...");
            for mut task in self.force_stop_pool.drain(..) {
                match task.try_wait() {
                    Ok(Some(_)) => {
                        // task finally stop itself
                        self.exited_pool.push(task);
                        debug!("Task Stop Itself");
                        pbar.inc(1);
                    }
                    Ok(None) => {
                        // we should stop the task forcely
                        let _ = task.stop();
                        self.exited_pool.push(task);
                        debug!("Task Stop Forcely");
                        pbar.inc(1);
                    }
                    Err(e) => {
                        // something going wrong, drop this task
                        pbar.inc(1);
                        warn!("Found Error: {}", e);
                        continue;
                    }
                }
            }
            // reinit this pool
            self.force_stop_pool = Vec::new();

            // Finally, check the timeout pool to waiting process exit itself or kill it.
            debug!("Checking Timeout Pool...");
            let mut remain_timeout_tasks = Vec::new();
            for mut task in self.timeout_pool.drain(..) {
                match task.try_wait() {
                    Ok(Some(_)) => {
                        // task stop itself
                        debug!("Task Stop Itself");
                        self.exited_pool.push(task);
                        pbar.inc(1);
                    }
                    Ok(None) => {
                        let elapsed = task.waiting_time().as_secs_f64();
                        if elapsed >= self.timeout_wait {
                            // send kill to task all childern to help exit
                            let _ = task.signal(rustix::process::Signal::Kill, false);
                            let _ = task.signal(rustix::process::Signal::Alarm, true);
                            // move to force stop pool
                            self.force_stop_pool.push(task);
                        } else {
                            // signal alarm to process
                            let _ = task.signal(rustix::process::Signal::Alarm, true);
                            remain_timeout_tasks.push(task);
                        }
                    }
                    Err(e) => {
                        // something going wrong, drop this task
                        pbar.inc(1);
                        warn!("Found Error: {}", e);
                        continue;
                    }
                }
            }

            self.timeout_pool = remain_timeout_tasks;

            debug!("Time to Sleep");
            let tick_runing_time = tick_start.elapsed().as_millis();
            let tick_sleep_time = self.tick_time.saturating_sub(tick_runing_time);
            pbar.tick();

            sleep(Duration::from_millis(tick_sleep_time as u64));
        }
        pbar.finish();
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
