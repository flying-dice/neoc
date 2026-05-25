def classify(n):
    def _v1():
        __t0 = n
        if __t0 == 0:
            return "zero"
        if ((__t0 == 1) or (__t0 == 2) or (__t0 == 3)):
            return "small"
        if True:
            x = __t0
            if (x > 100):
                return "huge"
        if True:
            return "other"
    return _v1()

def main():
    println(classify(0))
    println(classify(2))
    println(classify(200))
    println(classify(50))

