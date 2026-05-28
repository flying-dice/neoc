/** Greet someone by name. */
export function greet(name: string): string {
  return `hi ${name}`;
}

export interface User {
  id: number;
  name: string;
}

/** Look a user up; absent when not found. */
export function findUser(id: number): User | undefined {
  return undefined;
}

export function tags(): string[] {
  return [];
}

export function log(message: string): void {
  console.log(message);
}

export const VERSION: string = "1.0.0";
