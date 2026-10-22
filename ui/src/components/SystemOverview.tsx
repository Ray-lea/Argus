// What the details panel shows with no row selected: this computer's parts,
// each one a way to its view on the performance page, and how the panel works
// (select a row to see it here; the toolbar's button at the top right hides
// the panel).

import { createEffect, createUniqueId, For, type JSX, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { copyText } from "../lib/actions";
import { fmtCapacity, fmtGB, fmtGHz, fmtLinkSpeed } from "../lib/format";
import { rich, t } from "../lib/i18n";
import { persisted } from "../lib/prefs";
import { setPage, setPerfDevice, snapshot } from "../lib/store";
import type { Hardware } from "../lib/types";
import {
  BatteryIcon,
  ChipIcon,
  ComputerIcon,
  CopyIcon,
  DiskIcon,
  EyeIcon,
  EyeOffIcon,
  GpuIcon,
  MemoryIcon,
  NetworkIcon,
  PanelIcon,
  PointerIcon,
} from "./icons";
import { diskKind } from "./Performance";

const [partsVisible, setPartsVisible] = persisted("overviewHardwareVisible", true);

interface Part {
  /** The device on the performance page. */
  device: string;
  icon: (p: { size?: number }) => JSX.Element;
  colorVar: string;
  kind: string;
  name: string;
  detail: string;
}

/** The parts that are known, in a line: "a · b". */
const joined = (...items: unknown[]) =>
  items.filter((item): item is string => typeof item === "string" && item !== "").join(" · ");

/** One entry per part, in the performance page's order: processor, memory, storage, network, graphics, battery. */
function parts(hw: Hardware, memTotal: number): Part[] {
  const cpu = hw.cpu;
  const mem = hw.memory;
  const list: Part[] = [
    {
      device: "cpu",
      icon: ChipIcon,
      colorVar: "--cpu",
      kind: t("details.overview.cpu"),
      name: cpu.name,
      detail: joined(
        t("details.overview.cores", { count: cpu.cores }) +
          (cpu.performanceCores
            ? t("details.overview.hybrid", {
                performance: cpu.performanceCores,
                efficiency: cpu.efficiencyCores,
              })
            : ""),
        t("details.overview.threads", { count: cpu.logical }),
        cpu.baseMhz > 0 && t("details.overview.base", { speed: fmtGHz(cpu.baseMhz) }),
      ),
    },
    {
      device: "memory",
      icon: MemoryIcon,
      colorVar: "--mem",
      kind: t("common.resource.memory"),
      name: `${fmtGB(mem.installed || memTotal, 0)} GB${mem.kind ? ` ${mem.kind}` : ""}`,
      detail: joined(
        mem.speed && `${mem.speed} MT/s`,
        mem.slotsTotal > 0 && t("details.overview.slots", { used: mem.slotsUsed, total: mem.slotsTotal }),
        mem.formFactor === "onboard" ? t("performance.memory.onboard") : mem.formFactor,
      ),
    },
  ];
  for (const d of hw.disks) {
    list.push({
      device: `disk:${d.info.index}`,
      icon: DiskIcon,
      colorVar: "--io",
      kind: t("details.overview.disk"),
      name: d.info.model || t("performance.disk.title", { index: d.info.index }),
      detail: joined(d.info.size > 0 && fmtCapacity(d.info.size), diskKind(d), d.info.volumes.join(" ")),
    });
  }
  for (const n of hw.nics) {
    if (n.info.kind === "virtual") continue;
    list.push({
      device: `nic:${n.info.id}`,
      icon: NetworkIcon,
      colorVar: "--net",
      kind: t(`performance.network.kind.${n.info.kind}`),
      name: n.info.description || n.info.name,
      detail: joined(
        n.info.name !== n.info.description && n.info.name,
        n.info.linkSpeed > 0 && fmtLinkSpeed(n.info.linkSpeed),
      ),
    });
  }
  for (const g of hw.gpus) {
    list.push({
      device: `gpu:${g.info.id}`,
      icon: GpuIcon,
      colorVar: "--gpu",
      kind: t("details.overview.gpu"),
      name: g.info.name,
      detail:
        g.info.dedicatedTotal > 0
          ? t("details.overview.vram", { size: `${fmtGB(g.info.dedicatedTotal)} GB` })
          : t("details.overview.sharedMemory"),
    });
  }
  for (const b of hw.batteries) {
    const designed = b.info.designedCapacity;
    const full = b.info.fullCapacity;
    list.push({
      device: `bat:${b.info.id}`,
      icon: BatteryIcon,
      colorVar: "--bat",
      kind: t("details.overview.battery"),
      name: joined(b.info.manufacturer, b.info.name) || t("details.overview.battery"),
      detail: joined(
        designed && t("details.overview.designed", { capacity: `${Math.round(designed / 1000)} Wh` }),
        designed && full && t("details.overview.health", { percent: Math.round((full / designed) * 100) }),
      ),
    });
  }
  return list;
}

export default function SystemOverview() {
  const [list, setList] = createStore<Part[]>([]);
  const partsId = createUniqueId();
  createEffect(() => {
    const s = snapshot();
    // Preserve each device's button, hover state and keyboard focus between samples.
    setList(reconcile(s ? parts(s.hardware, s.system.memTotal) : [], { key: "device" }));
  });
  const open = (device: string) => {
    setPerfDevice(device);
    setPage("performance");
  };
  const copy = () => {
    const text = [
      t("details.overview.title"),
      ...list.map((part) =>
        [t("details.overview.copyPart", { kind: part.kind, name: part.name }), part.detail]
          .filter(Boolean)
          .join("\n"),
      ),
    ].join("\n\n");
    void copyText(text, t("details.overview.copyWhat"));
  };
  const visibilityLabel = () => t(partsVisible() ? "details.overview.hide" : "details.overview.show");
  return (
    <div class="details-scroll overview">
      <div class="details-head">
        <ComputerIcon size={32} />
        <div class="details-title">{t("details.overview.title")}</div>
        <div class="overview-actions">
          <button
            type="button"
            class="btn ghost small icon"
            title={t("details.overview.copy")}
            aria-label={t("details.overview.copy")}
            disabled={list.length === 0}
            onClick={copy}
          >
            <CopyIcon />
          </button>
          <button
            type="button"
            class="btn ghost small icon"
            title={visibilityLabel()}
            aria-label={visibilityLabel()}
            aria-expanded={partsVisible()}
            aria-controls={partsId}
            onClick={() => setPartsVisible(!partsVisible())}
          >
            <Show when={partsVisible()} fallback={<EyeOffIcon />}>
              <EyeIcon />
            </Show>
          </button>
        </div>
      </div>

      <ul class="overview-tips">
        <li>
          <PointerIcon size={14} />
          <span>{t("details.overview.selectTip")}</span>
        </li>
        <li>
          <PanelIcon size={14} />
          <span>
            {rich("details.overview.closeTip", {
              icon: (
                <span class="overview-key" role="img" aria-label={t("shell.details.label")}>
                  <PanelIcon size={12} />
                </span>
              ),
            })}
          </span>
        </li>
      </ul>

      <div class="overview-parts" id={partsId} hidden={!partsVisible()}>
        <For each={list}>
          {(part) => (
            <button
              type="button"
              class="overview-part"
              style={{ "--c": `var(${part.colorVar})` }}
              onClick={() => open(part.device)}
            >
              <span class="overview-icon">{part.icon({ size: 16 })}</span>
              <span class="overview-text">
                <span class="overview-kind">{part.kind}</span>
                <span class="overview-name">{part.name}</span>
                <Show when={part.detail}>
                  <span class="overview-detail">{part.detail}</span>
                </Show>
              </span>
            </button>
          )}
        </For>
      </div>
    </div>
  );
}
