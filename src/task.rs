use std::{
    fmt::Display,
    fs::{self},
    io::Result,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use crate::utils::process::kill_process_tree;
use log::warn;
use rustix::process::{Pid, Signal};
use uuid::Uuid;

const NODE_ID: [u8; 6] = [1, 1, 4, 5, 1, 4];

#[derive(Debug, Copy, Clone)]
pub enum TaskStatus {
    Waiting,
    Running,
    Exited,
    Timeout,
    Killed,
}

impl Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display_str = match self {
            Self::Waiting => "Waiting",
            Self::Running => "Running",
            Self::Exited => "Exited",
            Self::Timeout => "Timeout",
            Self::Killed => "Killed",
        };
        write!(f, "{}", display_str)
    }
}

#[derive(Debug)]
pub struct Task {
    name: String,
    prog: String,
    args: Vec<String>,
    cmd: Command,

    status: TaskStatus,
    handler: Option<Child>,
    start_time: Option<Instant>,
    start_waiting_time: Option<Instant>,
}

impl Task {
    pub fn new(cmd: &str) -> Self {
        let mut tokens = cmd.split_whitespace();
        // if paninc here, it means the input is invalid
        let prog = tokens.next().unwrap().to_string();
        let mut args = Vec::new();
        for token in tokens {
            args.push(token.to_string());
        }
        // get command obj
        let mut cmd = Command::new(&prog);
        cmd.args(args.clone());

        Task {
            name: String::from(Uuid::now_v1(&NODE_ID)),
            prog,
            args,
            cmd,
            status: TaskStatus::Waiting,
            handler: None,
            start_time: None,
            start_waiting_time: None,
        }
    }

    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
    }

    pub fn get_status(&self) -> TaskStatus {
        self.status
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_cmd(&self) -> String {
        let cmd = &self.prog;
        let args = self.args.join(" ");

        format!("{} {}", cmd, args)
    }

    pub fn running_time(&self) -> Duration {
        match &self.start_time {
            Some(start_time) => start_time.elapsed(),
            None => Duration::from_secs(0),
        }
    }

    pub fn waiting_time(&self) -> Duration {
        match &self.start_waiting_time {
            Some(start_time) => start_time.elapsed(),
            None => Duration::from_secs(0),
        }
    }

    pub fn reset_waiting_time(&mut self) {
        self.start_waiting_time = Some(Instant::now());
    }

    fn stdout(&mut self, pipe: Stdio) -> &mut Self {
        self.cmd.stdout(pipe);
        self
    }

    pub fn stdout_from_file(&mut self, path: &Path) -> &mut Self {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).expect("Failed to create runtime dir");
        }
        let file = fs::File::create(path).expect("Failed to create file");
        self.stdout(Stdio::from(file));
        self
    }

    pub fn spawn(&mut self) -> bool {
        if self.handler.is_some() {
            self.stop()
                .expect("Failed to respawn, due to unknown reason.");
        }

        let p = match self.cmd.spawn() {
            Ok(p) => Some(p),
            Err(e) => {
                warn!("Failed to spawn process: {}", e);
                None
            }
        };
        if p.is_none() {
            return false;
        }
        self.start_time = Some(Instant::now());
        self.handler = p;
        true
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if let Some(chlid) = &mut self.handler {
            chlid.try_wait()
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "child process not found",
            ))
        }
    }

    pub fn stop(&mut self) -> Result<Option<ExitStatus>> {
        let p = self.handler.take();
        match p {
            Some(mut child) => {
                let status = child.try_wait()?;
                match status {
                    Some(status) => Ok(Some(status)),
                    None => {
                        // use kill signl to stop process forcely.
                        match kill_process_tree(Pid::from_child(&child), Signal::Kill, true) {
                            Ok(_) => Ok(Some(child.wait()?)),
                            Err(_) => unreachable!(),
                        }
                    }
                }
            }
            None => Ok(None),
        }
    }

    pub fn signal(&self, signal: Signal, with_self: bool) -> Result<bool> {
        if let Some(child) = &self.handler {
            kill_process_tree(Pid::from_child(child), signal, with_self)
        } else {
            Ok(false)
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let args_str = self.args.join(" ");
        write!(f, "Task: {} {:?}", self.prog, args_str)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if self.handler.is_some() {
            // we should kill process tree at this time
            let _ = self.stop();
        }
    }
}

pub fn gen_tasks_from_file(filename: &Path) -> Vec<Task> {
    let contents = fs::read_to_string(filename).expect("Failed to read task list");
    let contents = contents.trim();
    if contents.is_empty() {
        return Vec::new();
    }
    let mut task_list = Vec::new();
    for line in contents.split('\n') {
        let task = Task::new(line);
        task_list.push(task);
    }

    task_list
}
