import type { Result, Option } from "../neoc-lib";
import type { SqlExecutor } from "./ports";
import type { SqlValue } from "./ports";
import type { decode_number } from "./ports";
import type { decode_string } from "./ports";

export interface ListNotesRow { id: number; body: string; }
export declare function list_notes<E>(db: E): Result<ListNotesRow[], string>;
