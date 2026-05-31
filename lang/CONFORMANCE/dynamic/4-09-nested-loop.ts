function main(): void {
  let acc = 0;
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      if ((j === 1)) {
        continue;
      }
      if ((j === 2)) {
        break;
      }
      acc = ((acc + (i * 10)) + j);
    }
  }
  println(acc);
}
