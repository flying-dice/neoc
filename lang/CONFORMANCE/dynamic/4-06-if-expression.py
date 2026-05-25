def main():
    x = 7
    def _v1():
        if (x > 5):
            return (x * 2)
        else:
            return 0
    y = _v1()
    y = (y + 1)
    println(y)

