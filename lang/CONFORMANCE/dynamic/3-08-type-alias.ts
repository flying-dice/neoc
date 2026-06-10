type Count = number;

function double(c: Count): Count {
  return (c + c);
}
function main(): void {
  println(double(7));
  println(double(21));
}
