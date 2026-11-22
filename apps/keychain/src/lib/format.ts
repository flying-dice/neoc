/** How many items there are, in words. */
export const itemsSummary = (count: number): string => (count === 0 ? 'No items' : count === 1 ? '1 item' : `${count} items`)
