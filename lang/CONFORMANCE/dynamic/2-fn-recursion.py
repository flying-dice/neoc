def fib(n):
    def _v1():
        if (n < 2):
            return n
        else:
            return (fib((n - 1)) + fib((n - 2)))
    return _v1()

def main():
    println(fib(10))

