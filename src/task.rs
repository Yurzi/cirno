use std::{
    fmt::Display,
    io::Result,
    process::{Child, Command, ExitStatus},
};

use crate::utils::process::kill_process_tree;
use rustix::process::{Pid, Signal};

#[derive(Default, Debug)]
pub struct TaskTimer {
    waiting: i64,
    running: i64,
    stopping: i64,
}

#[derive(Debug)]
pub struct Task {
    prog: String,
    args: Vec<String>,
    cmd: Command,

    timer: TaskTimer,
    handler: Option<Child>,
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
            timer: TaskTimer::default(),
            handler: None,
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
        self.handler = p;
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
                            Ok(_) => return Ok(Some(child.wait()?)),
                            Err(_) => unreachable!(),
                        }
                    }
                }
            }
            None => Ok(None),
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let args_str = self.args.join(" ");
        write!(f, "Task: {} {:?}", self.prog, args_str)
    }
}
