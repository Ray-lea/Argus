import { defineRouteMiddleware } from "@astrojs/starlight/route-data";
import { site } from "./site";

// The header handles edition switching. Keep the reading sidebar within the selected book.
export const onRequest = defineRouteMiddleware((context, next) => {
  const route = context.locals.starlightRoute;
  const edition = site.editions.findLast((item) =>
    context.url.pathname.startsWith(item.base),
  );
  if (!edition) return next();
  const book = route.sidebar.find(
    (group) => group.type === "group" && group.label === edition.label,
  );
  if (book?.type === "group") route.sidebar = book.entries;
  for (const direction of ["prev", "next"] as const) {
    const link = route.pagination[direction];
    if (
      link &&
      site.editions.findLast((item) => link.href.startsWith(item.base)) !==
        edition
    )
      route.pagination[direction] = undefined;
  }
  return next();
});
