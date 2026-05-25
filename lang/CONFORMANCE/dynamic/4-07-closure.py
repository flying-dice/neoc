def apply(f, x):
    return f(x)

def main():
    double = (lambda n: (n * 2))
    println(apply(double, 21))

