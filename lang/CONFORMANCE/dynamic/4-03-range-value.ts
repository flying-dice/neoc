function main(): void {
  let half = ((s, e) => { const r = []; for (let i = s; i < e; i++) r.push(i); return r; })(1, 4);
  let incl = ((s, e) => { const r = []; for (let i = s; i <= e; i++) r.push(i); return r; })(1, 4);
  let sum = 0;
  for (const x of half) {
    sum = (sum + x);
  }
  for (const y of incl) {
    sum = (sum + y);
  }
  println(sum);
}
