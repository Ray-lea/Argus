// 16px line icons drawn to match Fluent's regular weight.

import type { JSX } from "solid-js";

type IconProps = { class?: string; size?: number };

function Svg(props: IconProps & { children: JSX.Element }) {
  return (
    <svg
      class={props.class}
      width={props.size ?? 16}
      height={props.size ?? 16}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.25"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {props.children}
    </svg>
  );
}

export const SearchIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="7" cy="7" r="4.5" />
    <path d="M10.4 10.4 14 14" />
  </Svg>
);

export const ChevronRight = (p: IconProps) => (
  <Svg {...p}>
    <path d="m6 3.5 4.5 4.5L6 12.5" />
  </Svg>
);

export const ChevronDown = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 6 8 10.5 12.5 6" />
  </Svg>
);

export const ChevronUp = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 10 8 5.5l4.5 4.5" />
  </Svg>
);

export const PauseIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="3" width="2.5" height="10" rx="0.8" />
    <rect x="9.5" y="3" width="2.5" height="10" rx="0.8" />
  </Svg>
);

export const PlayIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5 3.2v9.6a.6.6 0 0 0 .9.5l7.6-4.8a.6.6 0 0 0 0-1L5.9 2.7a.6.6 0 0 0-.9.5Z" />
  </Svg>
);

export const ShieldIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M8 1.8 13 3.6v3.9c0 3.1-2.1 5.5-5 6.7-2.9-1.2-5-3.6-5-6.7V3.6Z" />
  </Svg>
);

export const AppsIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="2.5" y="2.5" width="4.5" height="4.5" rx="1" />
    <rect x="9" y="2.5" width="4.5" height="4.5" rx="1" />
    <rect x="2.5" y="9" width="4.5" height="4.5" rx="1" />
    <rect x="9" y="9" width="4.5" height="4.5" rx="1" />
  </Svg>
);

export const TreeIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 3h5M5 3v9.5h4M5 7.5h4" />
    <circle cx="11" cy="7.5" r="1.5" />
    <circle cx="11" cy="12.5" r="1.5" />
  </Svg>
);

export const ListIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2.5 4h11M2.5 8h11M2.5 12h11" />
  </Svg>
);

export const FolderIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2 4.5c0-.8.7-1.5 1.5-1.5h2.8l1.5 1.5h4.7c.8 0 1.5.7 1.5 1.5v5.5c0 .8-.7 1.5-1.5 1.5h-9c-.8 0-1.5-.7-1.5-1.5Z" />
  </Svg>
);

export const CopyIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="5" y="5" width="8.5" height="8.5" rx="1.5" />
    <path d="M11 5V3.5c0-.8-.7-1.5-1.5-1.5h-6C2.7 2 2 2.7 2 3.5v6c0 .8.7 1.5 1.5 1.5H5" />
  </Svg>
);

export const DismissIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m4 4 8 8M12 4l-8 8" />
  </Svg>
);

export const EndIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="8" cy="8" r="6" />
    <path d="m6 6 4 4M10 6l-4 4" />
  </Svg>
);

export const BranchEndIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3.5 2.5v8.5h3M3.5 6.5h3" />
    <circle cx="11" cy="9.5" r="3.5" />
    <path d="m9.6 8.1 2.8 2.8M12.4 8.1l-2.8 2.8" />
  </Svg>
);

export const InfoIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="8" cy="8" r="6" />
    <path d="M8 7.2v3.8" />
    <circle cx="8" cy="5" r=".4" fill="currentColor" />
  </Svg>
);

export const PanelIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="2" y="3" width="12" height="10" rx="1.5" />
    <path d="M9.5 3v10" />
  </Svg>
);

export const WarningIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M7.1 2.6a1 1 0 0 1 1.8 0l5.2 9.4a1 1 0 0 1-.9 1.5H2.8a1 1 0 0 1-.9-1.5Z" />
    <path d="M8 6.2v3" />
    <circle cx="8" cy="11.2" r=".4" fill="currentColor" />
  </Svg>
);

export const CheckIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="m3.5 8.5 3 3 6-7" />
  </Svg>
);

/** Fallback for processes without an executable icon. */
export const GenericAppIcon = (p: IconProps) => (
  <svg width={p.size ?? 16} height={p.size ?? 16} viewBox="0 0 16 16" class={p.class} aria-hidden="true">
    <rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="var(--glyph-bg)" stroke="var(--glyph-stroke)" />
    <path d="M1.5 5.5h13" stroke="var(--glyph-stroke)" />
    <circle cx="3.6" cy="4" r=".55" fill="var(--glyph-stroke)" />
    <circle cx="5.3" cy="4" r=".55" fill="var(--glyph-stroke)" />
  </svg>
);

/** Fallback for kernel pseudo-processes (System, Registry, Memory Compression). */
export const KernelIcon = (p: IconProps) => (
  <svg width={p.size ?? 16} height={p.size ?? 16} viewBox="0 0 16 16" class={p.class} aria-hidden="true">
    <rect x="3" y="3" width="10" height="10" rx="2" fill="var(--glyph-bg)" stroke="var(--glyph-stroke)" />
    <rect x="5.5" y="5.5" width="5" height="5" rx="1" fill="var(--glyph-stroke)" />
    <path
      d="M6 1.5V3M10 1.5V3M6 13v1.5M10 13v1.5M1.5 6H3M1.5 10H3M13 6h1.5M13 10h1.5"
      stroke="var(--glyph-stroke)"
      stroke-linecap="round"
    />
  </svg>
);

export const StopIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="4" y="4" width="8" height="8" rx="1.2" />
  </Svg>
);

export const RestartIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M12.6 6.2A5 5 0 1 0 13 9.5" />
    <path d="M13.2 2.8v3.6H9.6" />
  </Svg>
);

export const LockIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="3.5" y="7" width="9" height="6.5" rx="1.5" />
    <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" />
  </Svg>
);

export const GoToIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M3 8h9M8.5 4.5 12 8l-3.5 3.5" />
  </Svg>
);

export const ExternalIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9 2.5h4.5V7M13.5 2.5 7.5 8.5" />
    <path d="M12 9.5v3c0 .8-.7 1.5-1.5 1.5h-7c-.8 0-1.5-.7-1.5-1.5v-7C2 4.7 2.7 4 3.5 4h3" />
  </Svg>
);

/** Efficiency mode. */
export const LeafIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M13.5 2.5c-6.1 0-9.5 2.8-9.5 6.9 0 2.4 1.7 4.1 4.1 4.1 4.1 0 5.4-4.5 5.4-11Z" />
    <path d="M2.5 13.5c1.8-3 4.2-5.2 7.3-6.6" />
  </Svg>
);

/** Process priority. */
export const GaugeIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2.8 11.5a5.5 5.5 0 1 1 10.4 0" />
    <path d="m8 9.5 2.6-3.4" />
  </Svg>
);

// ---- Settings ----------------------------------------------------------------------

export const PowerIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5.1 4.3a5 5 0 1 0 5.8 0" />
    <path d="M8 2.2v5.3" />
  </Svg>
);

/** Closing to the tray. */
export const TrayIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M8 2v6.5M5.6 6.1 8 8.5l2.4-2.4" />
    <path d="M2 10h3.2l.8 1.5h4l.8-1.5H14v2.5c0 .8-.7 1.5-1.5 1.5h-9c-.8 0-1.5-.7-1.5-1.5Z" />
  </Svg>
);

export const KeyboardIcon = (p: IconProps) => (
  <Svg {...p}>
    <rect x="1.5" y="3.5" width="13" height="9" rx="1.5" />
    <path d="M4.2 6.5h.6M6.7 6.5h.6M9.2 6.5h.6M11.7 6.5h.6M5 9.5h6" />
  </Svg>
);

/** Interface animation. */
export const SparkleIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M7 2.5c.5 2.6 1.4 3.5 4 4-2.6.5-3.5 1.4-4 4-.5-2.6-1.4-3.5-4-4 2.6-.5 3.5-1.4 4-4Z" />
    <path d="M12.5 10.5v3M11 12h3" />
  </Svg>
);

/** Chart animation. */
export const PulseIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M1.5 8.5h2.6l1.6-4 2.6 7 1.9-5 1 2h3.3" />
  </Svg>
);

/** Refresh interval. */
export const TimerIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="8" cy="9" r="5" />
    <path d="M8 9V6.5M6.3 2h3.4M12.2 4.8l.9-.9" />
  </Svg>
);

export const HistoryIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2.7 8.2A5.3 5.3 0 1 0 4.3 4.3" />
    <path d="M4.4 1.9v2.5H1.9" />
    <path d="M8 5.3V8l1.9 1.4" />
  </Svg>
);

/** Stored data. */
export const StorageIcon = (p: IconProps) => (
  <Svg {...p}>
    <ellipse cx="8" cy="4" rx="5" ry="2" />
    <path d="M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4" />
    <path d="M3 8c0 1.1 2.2 2 5 2s5-.9 5-2" />
  </Svg>
);

export const TrashIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M2.5 4.5h11" />
    <path d="M6 4.5V3c0-.6.4-1 1-1h2c.6 0 1 .4 1 1v1.5" />
    <path d="m3.8 4.5.6 8.1c.1.8.7 1.4 1.5 1.4h4.2c.8 0 1.4-.6 1.5-1.4l.6-8.1" />
    <path d="M6.6 7.2v4M9.4 7.2v4" />
  </Svg>
);

/** The license. */
export const DocumentIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M4 2h5l3.5 3.5V13c0 .6-.4 1-1 1H4c-.6 0-1-.4-1-1V3c0-.6.4-1 1-1Z" />
    <path d="M9 2v3.5h3.5M5.5 8.5h5M5.5 11h3.5" />
  </Svg>
);

/** Rules: applied automatically. */
export const BoltIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M9.2 1.8 3.8 9.2h4l-1 5 5.4-7.4h-4Z" />
  </Svg>
);

/** Network rates: up and down. */
export const TransferIcon = (p: IconProps) => (
  <Svg {...p}>
    <path d="M5 13.5v-11M2.5 5 5 2.5 7.5 5" />
    <path d="M11 2.5v11M8.5 11l2.5 2.5 2.5-2.5" />
  </Svg>
);
