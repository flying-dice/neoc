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


class Wrapper(_Struct):
    _struct = "Wrapper"
    def __init__(self, value):
        self._0 = value

    @staticmethod
    def new(value):
        return Wrapper(value)

def _Wrapper_get(self):
    __t0 = self
    n = __t0._0
    return n
Wrapper.get = _Wrapper_get

def main():
    w = Wrapper.new(42)
    println(w.get())
    __t1 = w
    m = __t1._0
    println(m)

