import type { Result, Option } from "../neoc-lib";
import type { SqlExecutor } from "./ports";
import type { SqlValue } from "./ports";

export declare function add_note<E extends SqlExecutor>(db: E, body: string): Result<number, string>;
