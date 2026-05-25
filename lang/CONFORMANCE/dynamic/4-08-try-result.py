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


def parse(ok):
    def _v1():
        if ok:
            return Ok(10)
        else:
            return Err("bad")
    return _v1()

def doubled(ok):
    __try_0 = parse(ok)
    if __try_0.tag == "Err" or __try_0.tag == "None":
        return __try_0
    n = __try_0._0
    return Ok((n + n))

def show(r):
    def _v2():
        __t0 = r
        if __t0.tag == "Ok":
            n = __t0._0
            return "ok"
        if __t0.tag == "Err":
            e = __t0._0
            return e
    return _v2()

def main():
    println(show(doubled(True)))
    println(show(doubled(False)))

