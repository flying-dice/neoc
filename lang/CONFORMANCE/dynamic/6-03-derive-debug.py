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


class Point(_Struct):
    _struct = "Point"
    def __init__(self, x, y):
        self.x = x
        self.y = y
    def debug(self):
        return "Point { " + "x = " + _fmt(self.x) + ", " + "y = " + _fmt(self.y) + " }"

def main():
    p = Point(x=1, y=2)
    println(p.debug())

