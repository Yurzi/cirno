import random
from enum import Enum
from multiprocessing import Process, Queue, cpu_count
from threading import Lock, Thread
from time import sleep

import psutil as ps


class CirnoProcess(Process):
    """
    用于表示进程上执行的任务的对象
    """

    def __init__(self, func: callable, *args, **kwargs) -> None:
        super().__init__()
        self._func: callable = func
        self._args = args
        self._kwargs = kwargs
        self._result: None | object = None
        self._expection: None | BaseException = None
        # pipe between parent with child
        self._result_pipe = None
        self._expection_pipe = None
        # monitor
        self._ps_process: None | ps.Process = None

        self._is_closed: bool = False

    def start(self) -> None:
        self._result_pipe = Queue()
        self._expection_pipe = Queue()
        super().start()

    def run(self) -> None:
        try:
            self._result_pipe.put(self._func(*self._args, **self._kwargs))
        except BaseException as e:
            self._expection_pipe.put(e)

    def close(self) -> None:
        self._is_closed = True

        if self._result_pipe.empty() is False:
            self._result = self._result_pipe.get()
        if self._expection_pipe.empty() is False:
            self._expection = self._expection_pipe.get()

        del self._result_pipe
        del self._expection_pipe
        del self._ps_process

        self._result_pipe = None
        self._expection_pipe = None
        self._ps_process = None
        super().close()

    def terminate(self) -> None:
        if self._is_closed:
            return
        # 使用psutil 来关闭所有子进程
        parent = ps.Process(self.pid)
        for child in parent.children(recursive=True):
            child.terminate()
        parent = None
        super().terminate()

    @property
    def result(self) -> None | object:
        """
        返回进程的调用结果
        如果进程抛出了一个异常，那么其也会抛出其异常
        """
        if self._is_closed:
            if self._expection is None:
                return self._result
            else:
                raise self._expection

        # 进程还在执行时，返回None
        if self.is_alive():
            return None

        # Cache
        # 检查是否有异常
        if self._expection is not None:
            raise self._expection

        # 检查是否已经调用过结果
        if self._result is not None:
            return self._result

        if self._expection_pipe.empty():
            if self._result_pipe.empty():
                return None
            else:
                self._result = self._result_pipe.get()
                return self._result
        else:
            self._expection = self._expection_pipe.get()
            raise self._expection

    @property
    def expection(self) -> None | BaseException:
        """
        返回进程的所抛出的异常
        """
        # 进程已经被关闭
        if self._is_closed:
            return self._expection

        # 进程还在执行时，返回None
        if self.is_alive():
            return None

        if self._expection is not None:
            return self._expection

        if self._expection_pipe.empty():
            return None
        else:
            self._expection = self._expection_pipe.get()
            return self._expection

    @property
    def runtime_info(self) -> (float, float):
        if self._is_closed or self.pid is None or self.is_alive() is False:
            return (0, 0)

        memory_usage = 0
        cpu_usage = 0

        if self._ps_process is None:
            self._ps_process = ps.Process(self.pid)

        memory_usage = self._ps_process.memory_percent()
        cpu_usage = self._ps_process.cpu_percent(interval=0.2)

        return (cpu_usage, memory_usage)


class CirnoPool(Thread):
    """
    进程池，使用Thread实现
    """

    class Status(Enum):
        """
        用于表示检查的结果
        """

        Healthy = 0
        MaybeOK = 1
        Bad = 2

    def __init__(
        self,
        max_process: int = cpu_count(),
        is_smart: bool = True,
        min_threshold: (float, float) = ((cpu_count() * 80), 80),
        max_threshold: (float, float) = ((cpu_count() * 95), 95),
        sleep_timeout: int = 9,
    ) -> None:
        """
        max_process: int, 设置进程池支持的最大进程数
        is_smart: bool, 设置进程池是否在运行时和琪露诺一样智能的调整运行的进程
        threshold: (float, float), 代表 CPU(%) 和 MEM(%) 的限制值，但最终可能超过这个值
        """
        super().__init__()
        self._max_process: int = max_process
        self._is_smart: bool = is_smart

        self._shutdown: bool = False
        self._is_closed: bool = False

        self._now_process: int = 0
        self._todo_process_list: list[Process] = list()
        self._now_process_list: list[Process] = list()
        self._done_process_list: list[Process] = list()
        self._now_process_lock: Lock = Lock()
        self._todo_process_lock: Lock = Lock()
        self._done_process_lock: Lock = Lock()

        self._min_threshold: (float, float) = min_threshold
        self._max_threshold: (float, float) = max_threshold

        self._sleep_timeout = sleep_timeout

        # 进程池，启动！
        self.start()

    def submit(self, func: callable, *args, **kwargs) -> CirnoProcess:
        if self._shutdown:
            raise Exception("CirnoPool has closed")

        p = CirnoProcess(func, *args, **kwargs)
        # 将这个进程加入到todolist
        self._todo_process_lock.acquire()
        self._todo_process_list.append(p)
        self._todo_process_lock.release()

        return p

    def shutdown(self) -> None:
        self._shutdown = True

    def close(self) -> None:
        if not self._shutdown:
            raise Exception("You should call shutdown() before close()")

        while True:
            total_undo = 0
            self._todo_process_lock.acquire()
            total_undo += len(self._todo_process_list)
            self._todo_process_lock.release()

            self._now_process_lock.acquire()
            total_undo += self._now_process
            self._now_process_lock.release()

            if total_undo == 0:
                break
            sleep(self._sleep_timeout)
        self._is_closed = True

    def run(self) -> None:
        while not self._is_closed:
            ready_to_move: list[Process] = list()
            self._now_process_lock.acquire()
            # 检查现在运行的进程是否有终止的
            for p in self._now_process_list:
                if p.is_alive() is False:
                    ready_to_move.append(p)
            self._now_process_lock.release()

            # 移出这个进程
            self._move_to_done(ready_to_move)

            # 检查
            result = self._cirno_check()
            if result is self.Status.MaybeOK:
                # 暂时没事 先睡一会
                sleep(self._sleep_timeout)
                continue
            if result is self.Status.Healthy:
                # 前景大好，干点事
                # 挑选一个进程进入线程池
                self._move_to_run()
                # 好累，睡会
                sleep(self._sleep_timeout)
                continue
            if result is self.Status.Bad:
                # 好像要寄了
                # 挑选一个任务暂时的结束
                self._move_to_todo()
                # 睡会再看看
                sleep(self._sleep_timeout)
                continue

    @property
    def now_process(self) -> int:
        self._now_process_lock.acquire()
        res = self._now_process
        self._now_process_lock.release()
        return res

    def _move_to_todo(self) -> None:
        # 挑选一个进程回收
        # 这里假设self._now_process_list是顺序的，即末尾的是后加入的
        # 所以回收后加入的
        self._now_process_lock.acquire()
        if len(self._now_process_list) <= 1:
            # 只有一个进程了，还是得继续跑吧？
            self._now_process_lock.release()
            return

        last_one = self._now_process_list[-1]
        # 结束进程
        last_one.terminate()
        # 移出真正运行列表
        self._now_process_list.remove(last_one)
        # 修改计数器
        self._now_process -= 1
        self._now_process_lock.release()

        # 重新加入todolist
        self._todo_process_lock.acquire()
        self._todo_process_list.append(last_one)
        self._todo_process_lock.release()

    def _move_to_run(self) -> None:
        # 检查是否为空
        self._todo_process_lock.acquire()
        if len(self._todo_process_list) == 0:
            self._todo_process_lock.release()
            return

        # 挑选幸运儿
        lucky_one = random.choice(self._todo_process_list)
        # 移出原列表
        self._todo_process_list.remove(lucky_one)
        self._todo_process_lock.release()

        # 加入运行列表
        self._now_process_lock.acquire()
        self._now_process_list.append(lucky_one)
        # 修改计数器
        self._now_process += 1
        self._now_process_lock.release()
        # 启动
        lucky_one.start()

    def _move_to_done(self, process_list: None | list[Process]) -> None:
        if process_list is None:
            return

        # 将进程移出正在运行的队列
        self._now_process_lock.acquire()
        for p in process_list:
            self._now_process_list.remove(p)

        # 调整计数器
        self._now_process -= len(process_list)
        self._now_process_lock.release()

        # 将这些进程关闭，并移入完成列表
        self._done_process_lock.acquire()
        for p in process_list:
            p.close()
            self._done_process_list.append(p)
        self._done_process_lock.release()

    def _cirno_check(self) -> Status:
        """
        因为琪露诺很笨，所以这个检查非常的耗时，所以不建议经常进行
        """
        if self._now_process >= self._max_process:
            return self.Status.Bad

        if self._is_smart is False:
            return self.Status.Healthy

        # 进行运行时的环境检查以查看是否能继续增加一个进程
        os_mem = ps.virtual_memory()
        total_cpu = ps.cpu_percent(interval=0.2)
        total_mem = os_mem.used / os_mem.total

        if total_cpu < self._min_threshold[0] and total_mem < self._min_threshold[1]:
            return self.Status.Healthy

        if total_cpu >= self._max_threshold[0] or total_mem >= self._max_threshold[1]:
            return self.Status.Bad

        return self.Status.MaybeOK
