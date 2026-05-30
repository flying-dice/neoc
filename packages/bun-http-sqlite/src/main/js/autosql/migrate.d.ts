import type { Result, Option } from "../neoc-lib";
import type { SqlExecutor } from "./ports";

export declare function migrate<E extends SqlExecutor>(db: E): Result<number, string>;
