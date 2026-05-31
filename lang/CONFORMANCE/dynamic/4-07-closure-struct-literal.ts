interface Point { x: number; y: number; }
type Shape = { __enum: "Shape"; __tag: "Square"; side: number };

function main(): void {
  let mk = (n) => ({ __struct: "Point", x: n, y: n });
  let p = mk(7);
  println(p.x);
  println(p.y);
  let sq = (n) => ({ __enum: "Shape", __tag: "Square", side: n });
  let s = sq(5);
  return (() => {
    const __s1 = s;
    const side = __s1.side;
    return println(side);
  })();
}
