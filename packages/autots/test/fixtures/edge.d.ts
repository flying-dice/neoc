declare module "edge-lib" {
  export interface Widget {
    id: number;
    label?: string;
    resize(w: number, h: number): void; // void method with args
    reset(): void; // void method, no args
    clone(): Widget; // returns a wrapper
    onClick(handler: () => void): void; // function-typed param -> JsRef
    nudge(by: undefined): void; // unit param must become JsRef, never `()`
    $(): void; // non-identifier member -> skipped
  }

  export interface JsRef {
    x: number; // collides with the shim type -> must be skipped
  }

  export function make(): Widget;
  export function $(): void; // non-identifier export -> skipped
  export const VERSION: string;
}
