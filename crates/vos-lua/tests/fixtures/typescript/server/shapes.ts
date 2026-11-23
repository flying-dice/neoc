// A second module of the backend: the compiler bundles what `main.ts` imports.

export class Tally {
  private count = 0

  constructor(private readonly step: number) {}

  next(): number {
    this.count += this.step
    return this.count
  }

  get double(): number {
    return this.count * 2
  }
}

export function* upTo(last: number): Generator<number> {
  for (let n = 1; n <= last; n++) yield n
}
