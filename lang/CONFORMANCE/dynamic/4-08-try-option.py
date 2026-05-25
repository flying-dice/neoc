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


def incr(o):
    __try_0 = o
    if __try_0.tag == "Err" or __try_0.tag == "None":
        return __try_0
    n = __try_0._0
    return Some((n + 1))

def unwrap_or(o):
    def _v1():
        __t0 = o
        if __t0.tag == "Some":
            n = __t0._0
            return n
        if __t0.tag == "None":
            return 0
    return _v1()

def main():
    println(unwrap_or(incr(Some(41))))
    println(unwrap_or(incr(None_)))

