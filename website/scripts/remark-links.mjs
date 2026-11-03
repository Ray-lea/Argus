import { readFileSync } from "node:fs";
import { join, posix, relative, sep } from "node:path";
import { siteInput } from "./paths.mjs";
import { repo } from "./release.mjs";

// Markdown stays in the release repository. Rewrite only parsed link destinations, never code samples.
export default function releaseLinks() {
  const input = siteInput();
  const { editions } = JSON.parse(
    readFileSync(join(input, "site.json"), "utf8"),
  );
  return (tree, file) => {
    if (!file.path) return;
    const local = relative(join(input, "content"), file.path)
      .split(sep)
      .join("/");
    const route = "/" + local.replace(/index\.md$/, "");
    const edition = editions.findLast((item) => route.startsWith(item.base));
    if (!edition) return;
    const original = Object.keys(edition.routes).find(
      (key) => edition.routes[key] === route,
    );
    if (!original) return;
    function rewrite(node) {
      if (
        ["link", "definition", "image"].includes(node.type) &&
        node.url &&
        !/^(?:[a-z][\w+.-]*:|\/|#)/i.test(node.url)
      ) {
        const [target, hash] = node.url.split(/#(.*)/s);
        const resolved = posix.normalize(
          posix.join(posix.dirname(original), decodeURIComponent(target)),
        );
        node.url =
          (edition.routes[resolved] ??
            repo +
              (node.type === "image" ? "/raw/" : "/blob/") +
              edition.ref +
              "/" +
              resolved) + (hash ? "#" + hash : "");
      }
      for (const child of node.children ?? []) rewrite(child);
    }
    rewrite(tree);
  };
}
