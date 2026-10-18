// What a failed operation says (ActionError in argus-core), and whether its
// notice offers to update the privileged service. No Solid, so ui/tests can
// run it.

import type { MessageKey } from "./i18n.ts";
import type { ActionError } from "./types.ts";

/** A failure's message, with the error text when no message of its own fits. */
export interface Failure {
  key: MessageKey;
  error?: string;
}

/** Ending a process failed: a message under `actions.error`, which names the process `{name}`. */
export function endFailure(e: unknown): Failure {
  const err = e as ActionError | null | undefined;
  switch (err?.kind) {
    case "accessDenied":
    case "notFound":
    case "critical":
    case "refused":
    case "serviceOutdated":
      return { key: `actions.error.${err.kind}` };
    case "os":
      return { key: "actions.error.generic", error: err.message };
    default:
      return { key: "actions.error.generic", error: String(e) };
  }
}

/** Another operation failed: a message under `actions.failed`, which says what in `{what}`. */
export function actionFailure(e: unknown): Failure {
  const err = e as ActionError | null | undefined;
  switch (err?.kind) {
    case "accessDenied":
    case "notFound":
    case "serviceOutdated":
      return { key: `actions.failed.${err.kind}` };
    case "os":
      return { key: "actions.failed.generic", error: err.message };
    default:
      return { key: "actions.failed.generic", error: String(e) };
  }
}

const outdated = (e: unknown) => (e as ActionError | null | undefined)?.kind === "serviceOutdated";

/**
 * Denied to Argus's own rights while the privileged service, which would have
 * done it, needs an update (commands.rs, `outdated`): the notice then offers
 * to update the service.
 */
export const needsServiceUpdate = (errors: readonly unknown[]) => errors.some(outdated);

/** Which of several failures to report: one that updating the service would help, else the first. */
export const telling = (errors: readonly unknown[]): unknown => errors.find(outdated) ?? errors[0];
