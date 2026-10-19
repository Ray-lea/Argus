import { Match, Switch } from "solid-js";
import { iconUrl } from "../lib/ipc";
import type { ProcessRow } from "../lib/types";
import { GenericAppIcon, KernelIcon } from "./icons";

export default function ProcIcon(props: {
  proc: Pick<ProcessRow, "icon" | "path" | "webView">;
  size?: number;
}) {
  const size = () => props.size ?? 16;
  const icon = () => props.proc.webView?.hostIcon ?? props.proc.icon;
  return (
    <Switch fallback={<GenericAppIcon class="picon" size={size()} />}>
      <Match when={icon() !== null}>
        <img
          class="picon"
          src={iconUrl(icon() as number)}
          width={size()}
          height={size()}
          style={{ width: `${size()}px`, height: `${size()}px` }}
          alt=""
          draggable={false}
        />
      </Match>
      <Match when={props.proc.path === null}>
        <KernelIcon class="picon" size={size()} />
      </Match>
    </Switch>
  );
}
