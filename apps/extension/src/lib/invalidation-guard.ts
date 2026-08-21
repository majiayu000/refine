export interface InvalidationGuard {
  snapshot: () => number
  isCurrent: (snapshot: number) => boolean
  invalidate: () => void
}

export function createInvalidationGuard(): InvalidationGuard {
  let generation = 0
  return {
    snapshot: () => generation,
    isCurrent: (snapshot) => snapshot === generation,
    invalidate: () => {
      generation += 1
    },
  }
}
