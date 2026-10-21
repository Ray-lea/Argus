// Which engines a GPU's performance view charts, and in which order: how many
// and which one each chart shows are the user's choice, kept per GPU model.
// No Solid; run by ui/tests directly.

/** What the user chose for one GPU; absent until they choose. */
export interface EngineChoice {
  /** How many charts; null for one per engine. */
  count: number | null;
  /** The engines in the order the user arranged them; unknown names are skipped. */
  order: string[];
}

/** Every engine, in display order: the arranged ones first, then the rest as the GPU lists them. */
export function arrange(engines: readonly string[], choice?: EngineChoice): string[] {
  const known = new Set(engines);
  const first = [...new Set(choice?.order ?? [])].filter((e) => known.has(e));
  const placed = new Set(first);
  return [...first, ...engines.filter((e) => !placed.has(e))];
}

/** How many charts to show, between one and the number of engines. */
export function chartCount(engines: readonly string[], choice?: EngineChoice): number {
  const all = engines.length;
  const count = choice?.count;
  return count == null ? all : Math.min(all, Math.max(1, Math.round(count)));
}

/** The engines charted, in order. */
export const shownEngines = (engines: readonly string[], choice?: EngineChoice): string[] =>
  arrange(engines, choice).slice(0, chartCount(engines, choice));

/** Shows `count` charts, keeping the order arranged so far. */
export function withCount(
  engines: readonly string[],
  choice: EngineChoice | undefined,
  count: number,
): EngineChoice {
  const all = Math.max(1, engines.length);
  const n = Math.min(all, Math.max(1, Math.round(count)));
  return { count: n === all ? null : n, order: arrange(engines, choice) };
}

/**
 * Charts `engine` in place `slot`. An engine already charted elsewhere trades
 * places with the one there; one not charted takes its place, and the one it
 * replaces moves just past the charted ones.
 */
export function withEngine(
  engines: readonly string[],
  choice: EngineChoice | undefined,
  slot: number,
  engine: string,
): EngineChoice {
  const order = arrange(engines, choice);
  const from = order.indexOf(engine);
  const count = choice?.count ?? null;
  if (from < 0 || slot < 0 || slot >= order.length || from === slot) {
    return { count, order };
  }
  const shown = chartCount(engines, choice);
  if (from < shown) {
    [order[slot], order[from]] = [order[from], order[slot]];
  } else {
    const [replaced] = order.splice(slot, 1, engine);
    order.splice(order.indexOf(engine, slot + 1), 1);
    order.splice(shown, 0, replaced);
  }
  return { count, order };
}
