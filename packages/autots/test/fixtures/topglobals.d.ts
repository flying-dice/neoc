// A global *script* .d.ts: no top-level import/export, so its top-level
// declarations live in the global scope (like bun-types/globals.d.ts, where
// `console` / `fetch` are declared). Contrast fixtures/globals.d.ts, which uses
// a `declare global { … }` block inside a module.

declare function tfetch(url: string): TResponse;
declare var tconsole: TConsole;

interface TConsole {
  log(s: string): void;
}

interface TResponse {
  status: number;
}
