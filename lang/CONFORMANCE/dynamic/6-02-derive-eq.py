def _fmt(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, float):
        return str(int(v)) if v.is_integer() else repr(v)
    return str(v)


class _Struct:
    pass


class _Enum:
    def __init__(self, tag, *args, **fields):
        self.tag = tag
        for i, a in enumerate(args):
            setattr(self, "_" + str(i), a)
        for k, v in fields.items():
            setattr(self, k, v)


def Ok(v):
    return _Enum("Ok", v)


def Err(e):
    return _Enum("Err", e)


def Some(v):
    return _Enum("Some", v)


None_ = _Enum("None")


class Pair(_Struct):
    _struct = "Pair"
    def __init__(self, a, b):
        self.a = a
        self.b = b
    def eq(self, other):
        return self._struct == other._struct and self.a == other.a and self.b == other.b

def main():
    p = Pair(a=1, b=2)
    q = Pair(a=1, b=2)
    r = Pair(a=9, b=2)
    if p.eq(q):
        println("eq")
    def _v1():
        if (not p.eq(r)):
            println("ne")
    return _v1()

