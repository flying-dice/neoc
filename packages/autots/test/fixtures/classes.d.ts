declare module "demo-db" {
  export interface Changes {
    changes: number;
  }

  export class Database {
    constructor(filename?: string, options?: number);
    run(sql: string): Changes;
    count(): number;
  }

  // No declared constructor -> a 0-arg `new`.
  export class Ping {
    ping(): boolean;
  }

  // `export default` aliases the class; must not emit a `struct default`.
  export default Database;
}
