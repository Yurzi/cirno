use std::{fmt::Display, process::{Child, Command}};

#[derive(Debug)]
enum TaskState {
    Running,
    Waiting,
    Stopping,
    Exited,
}

#[derive(Debug)]
struct TaskTimer {
    running: i64,
    waiting: i64,
    stopping: i64,
}

impl Default for TaskTimer {
    fn default() -> Self {
        TaskTimer {
            running: 0,
            waiting: 0,
            stopping: 0,
        }
    }
}

#[derive(Debug)]
pub struct Task {
    prog: String,
    args: Vec<String>,
    cmd: Command,
    
    state: TaskState,
    
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
        let cmd = Command::new(&prog);
        Task {
            prog: prog,
            args: args,
            cmd: cmd,
            state: TaskState::Waiting,
            timer: TaskTimer::default(),
            handler: None,
        }
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let args_str = self.args.join(" ");
        write!(f, "Task: {} {:?}", self.prog, args_str)
    }
}

