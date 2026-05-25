export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export function lighten(color: string, amount: number): string;

export function toRgb(color: string): Rgb;

export function merge<T>(a: T, b: T): T;

export const WHITE: string;

function internal(): void;
