from __future__ import annotations

def main() -> None:
    xs = [1, 2, 3]
    _idx1 = 0
    _val2 = 10
    while len(xs) <= _idx1:
        xs.append(_val2)
    xs[_idx1] = _val2
    _idx3 = 1
    _val4 = 20
    while len(xs) <= _idx3:
        xs.append(_val4)
    xs[_idx3] = _val4
    i = 2
    _idx5 = i
    _val6 = 30
    while len(xs) <= _idx5:
        xs.append(_val6)
    xs[_idx5] = _val6
    println(xs[0])
    println(xs[1])
    println(xs[2])

