/**
 * Keep the first of every key. Components key an `{#each}` by an id they were handed (an option id, a command name),
 * and Svelte throws `each_key_duplicate` when two items share one: a list that came from outside is made unique first.
 */
export const uniqueBy =
  <T>(key: (item: T) => string) =>
  (items: readonly T[]): T[] => {
    const seen = new Set<string>()
    return items.filter((item) => {
      const k = key(item)
      if (seen.has(k)) return false
      seen.add(k)
      return true
    })
  }
