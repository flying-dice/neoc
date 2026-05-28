declare global {
  function gfetch(url: string): GResponse;
  var gconsole: GConsole;
  interface GConsole {
    log(s: string): void;
  }
  interface GResponse {
    status: number;
  }
}

export {};
