// Which engines a GPU's performance view charts, and in which order: by
// default the ones that have been busy within the charts' span (smart
// display), or as many as the user picks, each chart showing the engine they
// choose. Kept per GPU model. No Solid; run by ui/tests directly.

/** Utilization (%) an engine has to reach within the charts' span to be charted by smart display. */
export const BUSY_PERCENT = 1;

/** Whether an engine's samples show work worth a chart. */
export const wasBusy = (samples: readonly number[]) => samples.some((v) => v >= BUSY_PERCENT);

/** What the user chose for one GPU; absent until they choose. */
export interface EngineChoice {
  /**
   * Charts the engines that have been busy instead of a count. Choices saved
   * before smart display existed lack it and keep the count picked then.
   */
  smart?: boolean;
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

/** Smart display: what a GPU shows until the user picks a count. */
export const isSmart = (choice?: EngineChoice) => !choice || choice.smart === true;

/**
 * The engines charted, in order. Under smart display, given `busy`: the busy
 * ones, or the first engine (usually 3D) when none is; without `busy`, all.
 */
export function shownEngines(
  engines: readonly string[],
  choice?: EngineChoice,
  busy?: (engine: string) => boolean,
): string[] {
  const order = arrange(engines, choice);
  if (busy && isSmart(choice)) {
    const active = order.filter(busy);
    return active.length > 0 ? active : order.slice(0, 1);
  }
  return order.slice(0, chartCount(engines, choice));
}

/** Back to smart display, keeping the count and order for when the user picks a count again. */
export const withSmart = (engines: readonly string[], choice?: EngineChoice): EngineChoice => ({
  smart: true,
  count: choice?.count ?? null,
  order: arrange(engines, choice),
});

/**
 * What smart display shows (`shown`), as a choice of that many charts in that
 * order: a manual change starts from what is on screen.
 */
function fromScreen(engines: readonly string[], choice: EngineChoice | undefined, shown?: readonly string[]) {
  if (!shown || !isSmart(choice)) return choice;
  const first = shown.filter((e) => engines.includes(e));
  const rest = arrange(engines, choice).filter((e) => !first.includes(e));
  return { count: first.length === engines.length ? null : first.length, order: [...first, ...rest] };
}

/** Shows `count` charts, keeping the order arranged so far (under smart display, the charts on screen first). */
export function withCount(
  engines: readonly string[],
  choice: EngineChoice | undefined,
  count: number,
  shown?: readonly string[],
): EngineChoice {
  const all = Math.max(1, engines.length);
  const n = Math.min(all, Math.max(1, Math.round(count)));
  return { count: n === all ? null : n, order: arrange(engines, fromScreen(engines, choice, shown)) };
}

/**
 * Charts `engine` in place `slot`. An engine already charted elsewhere trades
 * places with the one there; one not charted takes its place, and the one it
 * replaces moves just past the charted ones. Under smart display (`onScreen`
 * is what it charts), this picks those charts by hand from then on.
 */
export function withEngine(
  engines: readonly string[],
  current: EngineChoice | undefined,
  slot: number,
  engine: string,
  onScreen?: readonly string[],
): EngineChoice {
  const choice = fromScreen(engines, current, onScreen);
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
