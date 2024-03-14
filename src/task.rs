use std::{
    fmt::Display,
    io::Result,
    process::{Child, Command, ExitStatus},
    time::{Duration, Instant},
};

use crate::utils::process::kill_process_tree;
use rustix::process::{Pid, Signal};

#[derive(Debug, Copy, Clone)]
pub enum TaskStatus {
    Waiting,
    Running,
    Exited,
    Timeout,
    Killed,
}

#[derive(Debug)]
pub struct Task {
    prog: String,
    args: Vec<String>,
    cmd: Command,

    status: TaskStatus,
    handler: Option<Child>,
    start_time: Option<Instant>,
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
            prog,
            args,
            cmd,
            status: TaskStatus::Waiting,
            handler: None,
            start_time: None,
        }
    }

    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
    }

    pub fn running_time(&self) -> Duration {
        match &self.start_time {
            Some(start_time) => start_time.elapsed(),
            None => Duration::from_secs(0),
        }
    }

    pub fn spawn(&mut self) {
        if self.handler.is_some() {
            self.stop()
                .expect("Failed to respawn, due to unknown reason.");
        }

        let p = match self.cmd.spawn() {
            Ok(p) => Some(p),
            Err(e) => {
                println!("Failed to spawn process: {}", e);
                None
            }
        };
        self.start_time = Some(Instant::now());
        self.handler = p;
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        if let Some(chlid) = &mut self.handler {
            chlid.try_wait()
        } else {
            Ok(None)
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
                        match kill_process_tree(Pid::from_child(&child), Signal::Kill) {
                            Ok(_) => Ok(Some(child.wait()?)),
                            Err(_) => unreachable!(),
                        }
                    }
                }
            }
            None => Ok(None),
        }
    }

    pub fn signal(&self, signal:Signal) -> Result<bool> {
        if let Some(child) = &self.handler {
            kill_process_tree(Pid::from_child(child), signal)
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
