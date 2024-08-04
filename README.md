## Cirno, a "smartest" processes scheduler

Cirno will help you to run tasks and keep your system away from freezing.

## Usage

See `ciron --help` for details.

This `cirno` will send signal to control child process.

`SIGALRM` is used to notify child when the child timeout.
`SIGTERM` is used to terminate child when resources are insufficient.
`SIGKILL` is used to kill child when child refused to self stop.

When Cirno finds that the system load exceeds the set value,
it will use the signal `SIGTERM` to temporarily terminate the task process in preparation for the next scheduling.

When Cirno detects that a task has timed out,
it will use the signal `SIGALRM` to notify the process and wait for process to exit on its own.

When Cirno must terminate a process,
it will help the process escape from child process hell (sending the signal `SIGKILL` to its child processes),
and check the child process status in the next scheduling loop.
If these do not work, then `SIGKILL` will be sent to all.

## Examples

Run with task list without task name.

```
cirno -w 2 examples.list
```

Run with task list which contains task name.

```
cirno -w 2 --with-task-name examples_with_taskname.list
```

See `cirno --help` for more info.
