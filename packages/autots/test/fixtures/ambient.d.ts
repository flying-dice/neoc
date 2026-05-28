declare module "fake-lib" {
  /** Do the thing. */
  export function doThing(name: string): number;

  export interface Opts {
    verbose: boolean;
  }

  export function run(opts: Opts): void;

  export const NAME: string;
}

declare module "fake-lib:sub" {
  export function helper(): boolean;
}
