use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    tick_time: u128,   // miliseconds of a loop
    timeout: f64,      // seconds
    timeout_wait: f64, // seconds

    max_workers: usize,
    force_workers: usize,

    monitor: Monitor,
    stop_flag: Arc<AtomicBool>,

    run_dir: String,
    socket_file: String,
}

impl Scheduler {
    pub fn new(args: &Args) -> Self {
        let tick_time = (1000.0 / args.tick_rate) as u128;
        let monitor = Monitor::new(args);
        let pid = std::process::id();

        let socket_file = format!("{}/cirno_{}.sock", args.run_dir, pid);

        let res = Scheduler {
            waiting_queue: VecDeque::new(),
            running_pool: Vec::new(),
            timeout_pool: Vec::new(),
            force_stop_pool: Vec::new(),
            exited_pool: Vec::new(),

            tick_time,
            timeout: args.timeout,
            timeout_wait: args.timeout_wait,

            max_workers: args.workers,
            force_workers: args.force_workers,

            monitor,
            stop_flag: Arc::new(AtomicBool::new(false)),

            run_dir: args.run_dir.clone(),
            socket_file,
        };
        res.init_runtime();
        res
    }

    fn init_runtime(&self) {
        std::fs::create_dir_all(&self.run_dir).expect("Failed to create runtime directory");
    }

    fn init_socket(&self) {
        if Path::new(&self.socket_file).exists() {
            std::fs::remove_file(&self.socket_file).expect("Failed to remove existing socket file");
        }

        // create normal file
        std::fs::File::create(&self.socket_file).expect("Failed to create socket file");
    }

    fn cleanup_socket(&self) {
        if Path::new(&self.socket_file).exists() {
            std::fs::remove_file(&self.socket_file).expect("Failed to remove existing socket file");
        }
    }

    fn read_socke_update_param(&mut self) {
        // open socket file
        let fd = std::fs::File::open(&self.socket_file);
        if let Ok(input) = fd {
            let bufferd = BufReader::new(input);
            for line in bufferd.lines() {
                // split by =
                let line = if let Ok(l) = line {
                    if l.trim().is_empty() || l.starts_with('#') || !l.contains('=') {
                        continue;
                    }
                    l
                } else {
                    continue;
                };

                let (key, value) = line.split_once('=').unwrap();
                match key {
                    "workers" => {
                        if let Ok(v) = value.parse::<usize>() {
                            self.max_workers = v;
                        }
                    }
                    "force_workers" => {
                        if let Ok(v) = value.parse::<usize>() {
                            self.force_workers = v;
                        }
                    }
                    "per-task-mem" => {
                        if let Ok(v) = value.parse::<usize>() {
                            self.monitor.set_per_task_mem(v);
                        }
                    }
                    _ => {}
                }
            }
        }

        // remove all content in socket file
        let _ = std::fs::File::create(&self.socket_file);
    }

    pub fn get_stop_flag_ref(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop_flag)
    }

    pub fn submit(&mut self, task: Task) {
        self.waiting_queue.push_back(task);
    }

    pub fn start(&mut self) {
        self.init_socket();
        self.run();
        self.cleanup_socket();
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
        pbar.enable_steady_tick(Duration::from_millis(100));

        let pmsg_bar = multi_pbar.add(ProgressBar::new_spinner());
        pmsg_bar.set_style(msg_style);
        pmsg_bar.enable_steady_tick(Duration::from_millis(100));

        loop {
            let tick_start = Instant::now();
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
            if tasks == 0 || self.stop_flag.load(Ordering::Relaxed) {
                // all task is done.
                debug!("Cirno Loop Exited");
                break;
            }

            // write report to file if necessary
            self.write_report();

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
                    task.stderr_from_file(Path::new(&format!(
                        "{}/{}.err",
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
                            task.stderr_from_file(Path::new(&format!(
                                "{}/{}.err",
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
                            let _ = task.signal(rustix::process::Signal::INT, false);
                            let _ = task.signal(rustix::process::Signal::ALARM, true);
                            // move to force stop pool
                            self.force_stop_pool.push(task);
                        } else {
                            // signal alarm to process
                            let _ = task.signal(rustix::process::Signal::ALARM, true);
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

            // update param
            self.read_socke_update_param();

            debug!("Time to Sleep");
            let tick_runing_time = tick_start.elapsed().as_millis();
            let tick_sleep_time = self.tick_time.saturating_sub(tick_runing_time);

            pmsg_bar.set_message(format!(
                "[running: {}|timeout_wait: {}|exited: {}]",
                self.running_pool.len(),
                self.timeout_pool.len(),
                self.exited_pool.len()
            ));
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
