/** Poll a page only while it can be seen, refreshing immediately on return. */
export function pollWhileVisible(
  refresh: () => void,
  interval: number,
  visibility: Pick<Document, "visibilityState" | "addEventListener" | "removeEventListener"> = document,
): () => void {
  let timer: ReturnType<typeof setInterval> | undefined;
  const follow = () => {
    clearInterval(timer);
    timer = undefined;
    if (visibility.visibilityState !== "visible") return;
    refresh();
    timer = setInterval(refresh, interval);
  };
  visibility.addEventListener("visibilitychange", follow);
  follow();
  return () => {
    clearInterval(timer);
    visibility.removeEventListener("visibilitychange", follow);
  };
}
