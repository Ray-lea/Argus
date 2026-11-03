import {
  prepareDevelopmentInput,
  watchDevelopmentInput,
} from "./scripts/dev.mjs";

// The shared configuration reads the edition sidebars as it loads.
const input = await prepareDevelopmentInput();
// Each configuration reload rebuilds the Starlight sidebar from the current routes.
const { default: base } = await import(
  new URL("./astro.config.mjs?dev=" + Date.now(), import.meta.url).href
);
export default {
  ...base,
  integrations: [...base.integrations, watchDevelopmentInput(input)],
};
