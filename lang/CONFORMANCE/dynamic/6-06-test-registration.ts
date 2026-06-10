const __tests: Record<string, () => void> = {};

function checks_alpha(): void {
  println("alpha-ran");
}
__tests.checks_alpha = checks_alpha;
function checks_beta(): void {
  println("beta-ran");
}
__tests.checks_beta = checks_beta;
function main(): void {
}
