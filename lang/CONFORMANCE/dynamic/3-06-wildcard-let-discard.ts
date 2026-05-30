interface Point { x: number; y: number; }

function main(): void {
  ({ __struct: "Point", x: 1, y: 2 });
  println(0);
}
