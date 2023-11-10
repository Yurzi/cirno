## Cirno, a stupid process pool

just a stupid work

## Usage

```python
from cirno import CirnoPool


def action():
    print("qwq")


if __name__ == "__main__":
    pool = CirnoPool()
    for i in range(sleep_timeout=1):
        pool.submit(action)

    pool.shutdown()
    pool.close()
```
