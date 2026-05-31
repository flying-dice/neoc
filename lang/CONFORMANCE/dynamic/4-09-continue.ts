function main(): void {
  let acc = 0;
  for (let i = 0; i < 10; i++) {
    if ((i === 3)) {
      continue;
    }
    if ((i === 7)) {
      break;
    }
    acc = (acc + i);
  }
  println(acc);
}
