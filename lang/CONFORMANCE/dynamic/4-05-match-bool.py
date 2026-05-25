def yes_no(b):
    def _v1():
        __t0 = b
        if __t0 == True:
            return "yes"
        if __t0 == False:
            return "no"
    return _v1()

def main():
    println(yes_no(True))
    println(yes_no(False))

