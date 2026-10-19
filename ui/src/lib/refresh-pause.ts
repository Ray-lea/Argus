// Manual pause and a temporary Ctrl hold share one sampler command stream.

export interface PauseState {
  manual: boolean;
  control: boolean;
  paused: boolean;
}

export function refreshPause(
  publish: (state: PauseState) => void,
  send: (paused: boolean) => Promise<void>,
  failed: (error: unknown) => void,
) {
  let state: PauseState = { manual: false, control: false, paused: false };
  let confirmed = false;
  let revision = 0;
  let pending = Promise.resolve();
  const set = (source: "manual" | "control", value: boolean) => {
    if (state[source] === value) return;
    const before = state.paused;
    state = { ...state, [source]: value };
    state.paused = state.manual || state.control;
    publish(state);
    if (state.paused === before) return;
    const next = state.paused;
    const request = ++revision;
    // A quick press/release must not let the resume reach the sampler first.
    pending = pending
      .then(() => send(next))
      .then(() => {
        confirmed = next;
      })
      .catch((error: unknown) => {
        if (revision === request) {
          state = { manual: confirmed, control: false, paused: confirmed };
          publish(state);
        }
        failed(error);
      });
  };
  return {
    setControl: (held: boolean) => set("control", held),
    toggleManual: () => set("manual", !state.manual),
  };
}
