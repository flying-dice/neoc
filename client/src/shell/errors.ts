/** The text of anything that was thrown: an `Error`'s message, else the value as a string. The one place that says so. */
export const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))
