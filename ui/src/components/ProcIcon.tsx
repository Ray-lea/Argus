import { Match, Switch } from "solid-js";
import { iconUrl } from "../lib/ipc";
import type { ProcessRow } from "../lib/types";
import { GenericAppIcon, KernelIcon } from "./icons";

export default function ProcIcon(props: { proc: Pick<ProcessRow, "icon" | "path">; size?: number }) {
  const size = () => props.size ?? 16;
  return (
    <Switch fallback={<GenericAppIcon class="picon" size={size()} />}>
      <Match when={props.proc.icon !== null}>
        <img
          class="picon"
          src={iconUrl(props.proc.icon as number)}
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
