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


def area(s):
    def _v1():
        __t0 = s
        if __t0.tag == "Move":
            dx = __t0._0
            dy = __t0._1
            return (dx + dy)
        if __t0.tag == "Box":
            w = __t0.w
            h = __t0.h
            return (w * h)
        if True:
            return 0
    return _v1()

def main():
    println(area(_Enum("Move", 3, 4)))
    println(area(_Enum("Box", w=5, h=6)))
    println(area(_Enum("Dot")))

