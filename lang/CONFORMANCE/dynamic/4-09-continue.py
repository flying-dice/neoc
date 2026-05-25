def main():
    acc = 0
    for i in range(0, 10):
        if (i == 3):
            continue
        if (i == 7):
            break
        acc = (acc + i)
    println(acc)

