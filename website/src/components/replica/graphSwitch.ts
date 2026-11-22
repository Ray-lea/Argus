// cpu_motion.rs keeps a frozen outgoing graph for 83 ms. Clone its pixels
// before React changes the graph, including canvases painted by the worker.
export function graphSnapshot(element: HTMLElement) {
  const ghost = element.cloneNode(true) as HTMLElement;
  ghost.querySelectorAll("[id]").forEach((el) => el.removeAttribute("id"));
  const sources = element.querySelectorAll("canvas");
  ghost.querySelectorAll("canvas").forEach((canvas, i) => {
    const src = sources[i];
    if (src?.width && src.height) canvas.getContext("2d")?.drawImage(src, 0, 0);
  });
  ghost.inert = true;
  ghost.setAttribute("aria-hidden", "true");
  return ghost;
}
export function animateGraphSwitch(parent: HTMLElement, ghost: HTMLElement) {
  ghost.classList.add("a-graph-ghost");
  parent.append(ghost);
  const incoming = parent.firstElementChild as HTMLElement | null;
  incoming?.animate(
    [
      { opacity: 0, transform: "scale(.985)" },
      { opacity: 1, transform: "scale(1)" },
    ],
    { duration: 220, easing: "cubic-bezier(.1,.9,.2,1)" },
  );
  const out = ghost.animate([{ opacity: 1 }, { opacity: 0 }], { duration: 83, easing: "linear", fill: "forwards" });
  out.onfinish = () => ghost.remove();
}
