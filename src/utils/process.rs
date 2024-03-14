use std::char;
use std::fmt::Display;
use std::fs::{read_dir, read_to_string};
use std::io::{ErrorKind, Result};
use std::num::NonZeroI32;
use std::path::Path;

use rustix::param::page_size;
use rustix::process::{kill_process, Pid, Signal};

const PROC_DIR: &str = "/proc";

#[derive(Clone, Debug)]
pub struct Process {
    pid: Pid,
    ppid: Option<Pid>,
    comm: String,
    create_time: usize,
}

impl Process {
    pub fn new(pid: Pid) -> Result<Self> {
        let proc_path = format!("{}/{}/stat", PROC_DIR, pid.as_raw_nonzero());
        let proc_path = Path::new(&proc_path);

        // read process info at one time to decrease unsync status
        let proc_stat = read_to_string(proc_path)?;
        let comm = proc_stat
            .chars()
            .skip_while(|&x| x != '(')
            .skip(1)
            .take_while(|&x| x != ')')
            .collect::<String>();

        // parse proc_stat
        let proc_stat = proc_stat
            .chars()
            .skip_while(|&x| x != ')')
            .skip_while(|&x| !char::is_alphanumeric(x))
            .collect::<String>();
        let proc_stat: Vec<&str> = proc_stat.split_ascii_whitespace().collect();
        let ppid = proc_stat
            .get(1)
            .ok_or(ErrorKind::NotFound)?
            .parse::<i32>()
            .expect("Bad format in proc/pid/stat");
        // Safety: the ppid is came from proc/stat file,
        // so it must be positive
        let ppid = if ppid == 0 {
            None
        } else {
            Some(unsafe { Pid::from_raw_unchecked(ppid) })
        };
        let proc_create_time = proc_stat
            .get(19)
            .ok_or(ErrorKind::NotFound)?
            .parse::<usize>()
            .expect("Bad format in proc/pid/stat");

        Ok(Process {
            pid,
            ppid,
            comm: comm.to_string(),
            create_time: proc_create_time,
        })
    }

    pub fn mem(&self) -> usize {
        if !self.is_exist() {
            return 0;
        }
        let pid: i32 = self.pid.as_raw_nonzero().get();
        let proc_mem_path = format!("{}/{}/statm", PROC_DIR, pid);
        let proc_mem_path = Path::new(&proc_mem_path);

        let proc_statm = read_to_string(proc_mem_path).unwrap();
        let mut proc_statm = proc_statm.split_whitespace();
        let _size = proc_statm.next().unwrap().parse::<usize>().unwrap();
        // use `page` as unit
        let res_size = proc_statm.next().unwrap().parse::<usize>().unwrap();

        // use `Byte` as unit
        res_size * page_size()
    }

    pub fn is_exist(&self) -> bool {
        let pid: i32 = self.pid.as_raw_nonzero().get();
        let proc_path = format!("{}/{}/stat", PROC_DIR, pid);
        let proc_path = Path::new(&proc_path);
        let proc_stat = read_to_string(proc_path).unwrap();
        let proc_stat = proc_stat
            .chars()
            .skip_while(|&x| x != ')')
            .skip_while(|&x| !char::is_alphanumeric(x))
            .collect::<String>();
        let proc_stat: Vec<&str> = proc_stat.split_ascii_whitespace().collect();
        let proc_create_time = proc_stat
            .get(19)
            .unwrap()
            .parse::<usize>()
            .expect("Bad format in proc/pid/stat");

        // os fatal, panic is better
        self.create_time == proc_create_time
    }
}

impl Display for Process {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ppid = match self.ppid {
            Some(p) => p.as_raw_nonzero(),
            // Safety: it should be
            None => unsafe { NonZeroI32::new_unchecked(1) },
        };

        write!(
            f,
            "Process: {} {} {} {}",
            self.pid.as_raw_nonzero(),
            ppid,
            self.comm,
            self.create_time
        )
    }
}

impl PartialEq for Process {
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid
            && self.ppid == other.ppid
            && self.comm == other.comm
            && self.create_time == other.create_time
    }
}

pub fn get_sys_process_list() -> Vec<Process> {
    let mut process_list = Vec::new();

    let proc_dir = Path::new(PROC_DIR);
    // on *nix os, the /proc/ is must exist;
    let proc_dir = read_dir(proc_dir).unwrap();

    // iter all pid dir
    for entry in proc_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        // get filename, equal to pid
        let filename = match path.file_name() {
            Some(filename) => filename,
            None => continue,
        };

        let filename = filename.to_os_string().into_string().unwrap();
        // convert filename to pid and get process object
        match filename.parse::<i32>() {
            Ok(pid) => {
                // Safety: the pid is come from the filename in /proc,
                // so it must be positive
                let process = Process::new(unsafe { Pid::from_raw_unchecked(pid) });
                match process {
                    Ok(process) => process_list.push(process),
                    Err(_) => continue,
                }
            }
            Err(_) => continue,
        }
    }

    process_list
}

pub fn get_process_tree(pid: Pid) -> Result<Vec<Process>> {
    let mut childern_process_list: Vec<Process> = Vec::new();
    let mut children: Vec<Process> = Vec::new();
    let process_list = get_sys_process_list();

    // push first child process to stack, the first one will be duplicated,
    // but is safe
    let first_one = Process::new(pid)?;
    children.push(first_one);
    while let Some(child) = children.pop() {
        // iter process_list to find children
        for process in process_list.iter() {
            if let Some(ppid) = process.ppid {
                if ppid == child.pid {
                    // this one is a child
                    children.push(process.clone());
                }
            }
        }
        childern_process_list.push(child);
    }

    Ok(childern_process_list)
}

pub fn kill_process_tree(pid: Pid, signal: Signal) -> Result<bool> {
    // try to kill every children and self
    let mut process_list_to_kill = get_process_tree(pid)?;
    process_list_to_kill.reverse();
    for process in process_list_to_kill {
        if process.is_exist() {
            let _ = kill_process(process.pid, signal);
        }
    }

    Ok(true)
}
