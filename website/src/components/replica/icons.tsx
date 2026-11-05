import type { Glyph } from "./model";

// The app's own 16 px line icons (resources/ui-icons), stroked in the text colour.
// A leading "@w@" overrides the 1.25 px stroke.
const ui = {
  apps: "<rect x=\"2.5\" y=\"2.5\" width=\"4.5\" height=\"4.5\" rx=\"1\"/><rect x=\"9\" y=\"2.5\" width=\"4.5\" height=\"4.5\" rx=\"1\"/><rect x=\"2.5\" y=\"9\" width=\"4.5\" height=\"4.5\" rx=\"1\"/><rect x=\"9\" y=\"9\" width=\"4.5\" height=\"4.5\" rx=\"1\"/>",
  arrowRight: "<path d=\"M3 8h10M9 4l4 4-4 4\"/>",
  battery: "<rect x=\"1.5\" y=\"4.5\" width=\"11.5\" height=\"7\" rx=\"1.5\"/><path d=\"M14.5 7v2M4 7v2M6.5 7v2\"/>",
  bolt: "<path d=\"M9.2 1.8 3.8 9.2h4l-1 5 5.4-7.4h-4Z\"/>",
  branchEnd: "<path d=\"M3.5 2.5v8.5h3M3.5 6.5h3\"/><circle cx=\"11\" cy=\"9.5\" r=\"3.5\"/><path d=\"m9.6 8.1 2.8 2.8M12.4 8.1l-2.8 2.8\"/>",
  chat: "<path d=\"M4 3h8a1.5 1.5 0 0 1 1.5 1.5v5A1.5 1.5 0 0 1 12 11H7.5l-3 2.5V11H4a1.5 1.5 0 0 1-1.5-1.5v-5A1.5 1.5 0 0 1 4 3Z\"/>",
  check: "<path d=\"m3.5 8.5 3 3 6-7\"/>",
  chevronDown: "<path d=\"M3.5 6 8 10.5 12.5 6\"/>",
  chevronRight: "<path d=\"m6 3.5 4.5 4.5L6 12.5\"/>",
  chevronUp: "<path d=\"M3.5 10 8 5.5l4.5 4.5\"/>",
  chip: "<rect x=\"4.5\" y=\"4.5\" width=\"7\" height=\"7\" rx=\"1\"/><rect x=\"6.5\" y=\"6.5\" width=\"3\" height=\"3\" rx=\".4\"/><path d=\"M6.5 2v2.5M9.5 2v2.5M6.5 11.5V14M9.5 11.5V14M2 6.5h2.5M2 9.5h2.5M11.5 6.5H14M11.5 9.5H14\"/>",
  computer: "<rect x=\"2\" y=\"2.5\" width=\"12\" height=\"8.5\" rx=\"1.2\"/><path d=\"M6 14h4M8 11v3\"/>",
  copy: "<rect x=\"5\" y=\"5\" width=\"8.5\" height=\"8.5\" rx=\"1.5\"/><path d=\"M11 5V3.5c0-.8-.7-1.5-1.5-1.5h-6C2.7 2 2 2.7 2 3.5v6c0 .8.7 1.5 1.5 1.5H5\"/>",
  disk: "<rect x=\"2\" y=\"4\" width=\"12\" height=\"8\" rx=\"1.5\"/><path d=\"M2 9.2h12\"/><circle cx=\"11.6\" cy=\"10.6\" r=\".45\" fill=\"currentColor\"/>",
  dismiss: "<path d=\"m4 4 8 8M12 4l-8 8\"/>",
  document: "<path d=\"M4 2h5l3.5 3.5V13c0 .6-.4 1-1 1H4c-.6 0-1-.4-1-1V3c0-.6.4-1 1-1Z\"/><path d=\"M9 2v3.5h3.5M5.5 8.5h5M5.5 11h3.5\"/>",
  end: "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"m6 6 4 4M10 6l-4 4\"/>",
  external: "<path d=\"M9 2.5h4.5V7M13.5 2.5 7.5 8.5\"/><path d=\"M12 9.5v3c0 .8-.7 1.5-1.5 1.5h-7c-.8 0-1.5-.7-1.5-1.5v-7C2 4.7 2.7 4 3.5 4h3\"/>",
  eyeOff: "<path d=\"m2 2 12 12M6.5 3.7A6 6 0 0 1 8 3.5c4 0 6.5 4.5 6.5 4.5a13 13 0 0 1-2 2.5M10 12.2a6 6 0 0 1-2 .3C4 12.5 1.5 8 1.5 8a13 13 0 0 1 2.6-3M6.6 6.6a2 2 0 0 0 2.8 2.8\"/>",
  eye: "<path d=\"M1.5 8s2.5-4.5 6.5-4.5 6.5 4.5 6.5 4.5-2.5 4.5-6.5 4.5S1.5 8 1.5 8Z\"/><circle cx=\"8\" cy=\"8\" r=\"2\"/>",
  filter: "<path d=\"M2.5 3.5h11l-4.25 5v4.25l-2.5 1.25V8.5Z\"/>",
  folder: "<path d=\"M2 4.5c0-.8.7-1.5 1.5-1.5h2.8l1.5 1.5h4.7c.8 0 1.5.7 1.5 1.5v5.5c0 .8-.7 1.5-1.5 1.5h-9c-.8 0-1.5-.7-1.5-1.5Z\"/>",
  fontSize: "<path d=\"m1.5 12.5 3.25-9 3.25 9M2.7 9h4.1m2.7 3.5 2.25-6 2.25 6M10.4 10h2.7\"/>",
  gauge: "<path d=\"M2.8 11.5a5.5 5.5 0 1 1 10.4 0\"/><path d=\"m8 9.5 2.6-3.4\"/>",
  genericApp: "@1@<rect x=\"1.5\" y=\"2.5\" width=\"13\" height=\"11\" rx=\"2\"/><path d=\"M1.5 5.5h13\"/><circle cx=\"3.6\" cy=\"4\" r=\".55\" fill=\"currentColor\" stroke=\"none\"/><circle cx=\"5.3\" cy=\"4\" r=\".55\" fill=\"currentColor\" stroke=\"none\"/>",
  github: "<path transform=\"scale(0.6667)\" stroke-width=\"1.875\" d=\"M9 19c-4 1.3-4-2-6-2.5m12 4.5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1-.3-3.4 1.3a11.8 11.8 0 0 0-6.2 0C6.6 2.8 5.6 3.1 5.6 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4.2 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21\"/>",
  globe: "<circle cx=\"8\" cy=\"8\" r=\"5.5\"/><path d=\"M2.5 8h11\"/><path d=\"M8 2.5c1.5 1.6 2.25 3.4 2.25 5.5S9.5 11.9 8 13.5C6.5 11.9 5.75 10.1 5.75 8S6.5 4.1 8 2.5Z\"/>",
  goTo: "<path d=\"M3 8h9M8.5 4.5 12 8l-3.5 3.5\"/>",
  gpu: "<rect x=\"1.5\" y=\"4\" width=\"11\" height=\"7.5\" rx=\"1\"/><circle cx=\"7\" cy=\"7.75\" r=\"2\"/><path d=\"M12.5 6h2M12.5 9.5h2M3.5 11.5v2\"/>",
  history: "<path d=\"M2.7 8.2A5.3 5.3 0 1 0 4.3 4.3\"/><path d=\"M4.4 1.9v2.5H1.9\"/><path d=\"M8 5.3V8l1.9 1.4\"/>",
  info: "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"M8 7.2v3.8\"/><circle cx=\"8\" cy=\"5\" r=\".4\" fill=\"currentColor\"/>",
  interfaceScale: "<circle cx=\"6.75\" cy=\"6.75\" r=\"4.75\"/><path d=\"M10.25 10.25 14 14M4.5 6.75H9M6.75 4.5V9\"/>",
  kernel: "@1@<rect x=\"3\" y=\"3\" width=\"10\" height=\"10\" rx=\"2\"/><rect x=\"5.5\" y=\"5.5\" width=\"5\" height=\"5\" rx=\"1\" fill=\"currentColor\" stroke=\"none\"/><path d=\"M6 1.5V3M10 1.5V3M6 13v1.5M10 13v1.5M1.5 6H3M1.5 10H3M13 6h1.5M13 10h1.5\" stroke-linecap=\"round\"/>",
  keyboard: "<rect x=\"1.5\" y=\"3.5\" width=\"13\" height=\"9\" rx=\"1.5\"/><path d=\"M4.2 6.5h.6M6.7 6.5h.6M9.2 6.5h.6M11.7 6.5h.6M5 9.5h6\"/>",
  leaf: "<path d=\"M13.5 2.5c-6.1 0-9.5 2.8-9.5 6.9 0 2.4 1.7 4.1 4.1 4.1 4.1 0 5.4-4.5 5.4-11Z\"/><path d=\"M2.5 13.5c1.8-3 4.2-5.2 7.3-6.6\"/>",
  list: "<path d=\"M2.5 4h11M2.5 8h11M2.5 12h11\"/>",
  lock: "<rect x=\"3.5\" y=\"7\" width=\"9\" height=\"6.5\" rx=\"1.5\"/><path d=\"M5.5 7V5a2.5 2.5 0 0 1 5 0v2\"/>",
  memory: "<path d=\"M1.5 5h13v5.5h-13Z\"/><path d=\"M4 7.2v1.3M6.7 7.2v1.3M9.3 7.2v1.3M12 7.2v1.3M3.5 10.5V12M6 10.5V12M10 10.5V12M12.5 10.5V12\"/>",
  menu: "<path d=\"M2.5 4.5h11M2.5 8h11M2.5 11.5h11\"/>",
  minimize: "<rect x=\"2.5\" y=\"2.5\" width=\"11\" height=\"9\" rx=\"1.5\"/><path d=\"M5.5 14h5\"/>",
  navSide: "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"1.5\"/><path d=\"M6 3v10\"/>",
  navTop: "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"1.5\"/><path d=\"M2 6.5h12\"/>",
  network: "<rect x=\"6\" y=\"2\" width=\"4\" height=\"3.5\" rx=\".6\"/><rect x=\"1.8\" y=\"10.5\" width=\"4\" height=\"3.5\" rx=\".6\"/><rect x=\"10.2\" y=\"10.5\" width=\"4\" height=\"3.5\" rx=\".6\"/><path d=\"M8 5.5V8M3.8 10.5V8h8.4v2.5\"/>",
  note: "<path d=\"M3.5 2.5h9c.6 0 1 .4 1 1V10L10 13.5H3.5c-.6 0-1-.4-1-1v-9c0-.6.4-1 1-1Z\"/><path d=\"M13.5 10H11c-.6 0-1 .4-1 1v2.5\"/>",
  notes: "<path d=\"M9.5 2H4c-.6 0-1 .4-1 1v10c0 .6.4 1 1 1h7.5c.6 0 1-.4 1-1V7\"/><path d=\"M5.5 8.5h4.5M5.5 11h3M12.5 1.5v3M11 3h3\"/>",
  panel: "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"1.5\"/><path d=\"M9.5 3v10\"/>",
  pause: "<rect x=\"4\" y=\"3\" width=\"2.5\" height=\"10\" rx=\"0.8\"/><rect x=\"9.5\" y=\"3\" width=\"2.5\" height=\"10\" rx=\"0.8\"/>",
  pin: "<path d=\"M5.5 2.5h5\"/><path d=\"M6.5 2.5v3.8L4.5 9.5h7l-2-3.2V2.5\"/><path d=\"M8 9.5v4\"/>",
  play: "<path d=\"M5 3.2v9.6a.6.6 0 0 0 .9.5l7.6-4.8a.6.6 0 0 0 0-1L5.9 2.7a.6.6 0 0 0-.9.5Z\"/>",
  pointer: "<path d=\"M4 2.5 12 8l-3.7.8 2.1 3.9-1.5.8-2.1-3.9L4 12Z\"/>",
  power: "<path d=\"M5.1 4.3a5 5 0 1 0 5.8 0\"/><path d=\"M8 2.2v5.3\"/>",
  processes: "<rect x=\"2\" y=\"2.5\" width=\"3\" height=\"3\" rx=\".75\"/><rect x=\"2\" y=\"6.5\" width=\"3\" height=\"3\" rx=\".75\"/><rect x=\"2\" y=\"10.5\" width=\"3\" height=\"3\" rx=\".75\"/><path d=\"M7.5 4h6M7.5 8h6M7.5 12h6\"/>",
  pulse: "<path d=\"M1.5 8.5h2.6l1.6-4 2.6 7 1.9-5 1 2h3.3\"/>",
  restart: "<path d=\"M12.6 6.2A5 5 0 1 0 13 9.5\"/><path d=\"M13.2 2.8v3.6H9.6\"/>",
  runTask: "<path d=\"M8.5 13H3a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h9a1 1 0 0 1 1 1v3M2 6h11M12 9v5M9.5 11.5h5\"/>",
  search: "<circle cx=\"7\" cy=\"7\" r=\"4.5\"/><path d=\"M10.4 10.4 14 14\"/>",
  services: "<path d=\"M2.5 4.5h3a1.5 1.5 0 0 1 3 0h3v3a1.5 1.5 0 0 1 0 3v3h-9Z\"/>",
  settings: "<path d=\"M6.82 3.24 6.97 1.79h2.06l.15 1.45 1.35.56 1.13-.93 1.47 1.47-.93 1.13.56 1.35 1.45.15v2.06l-1.45.15-.56 1.35.93 1.13-1.47 1.47-1.13-.93-1.35.56-.15 1.45H6.97l-.15-1.45-1.35-.56-1.13.93-1.47-1.47.93-1.13-.56-1.35-1.45-.15V6.97l1.45-.15.56-1.35-.93-1.13 1.47-1.47 1.13.93Z\"/><circle cx=\"8\" cy=\"8\" r=\"2\"/>",
  shield: "<path d=\"M8 1.8 13 3.6v3.9c0 3.1-2.1 5.5-5 6.7-2.9-1.2-5-3.6-5-6.7V3.6Z\"/>",
  sparkle: "<path d=\"M7 2.5c.5 2.6 1.4 3.5 4 4-2.6.5-3.5 1.4-4 4-.5-2.6-1.4-3.5-4-4 2.6-.5 3.5-1.4 4-4Z\"/><path d=\"M12.5 10.5v3M11 12h3\"/>",
  star: "<path d=\"m8 1.9 1.85 3.75 4.15.6-3 2.92.71 4.13L8 11.35l-3.71 1.95.71-4.13-3-2.92 4.15-.6Z\"/>",
  stop: "<rect x=\"4\" y=\"4\" width=\"8\" height=\"8\" rx=\"1.2\"/>",
  storage: "<ellipse cx=\"8\" cy=\"4\" rx=\"5\" ry=\"2\"/><path d=\"M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4\"/><path d=\"M3 8c0 1.1 2.2 2 5 2s5-.9 5-2\"/>",
  theme: "<circle cx=\"8\" cy=\"8\" r=\"5.5\"/><path d=\"M8 2.5a5.5 5.5 0 0 1 0 11Z\" fill=\"currentColor\"/>",
  timer: "<circle cx=\"8\" cy=\"9\" r=\"5\"/><path d=\"M8 9V6.5M6.3 2h3.4M12.2 4.8l.9-.9\"/>",
  transfer: "<path d=\"M5 13.5v-11M2.5 5 5 2.5 7.5 5\"/><path d=\"M11 2.5v11M8.5 11l2.5 2.5 2.5-2.5\"/>",
  trash: "<path d=\"M2.5 4.5h11\"/><path d=\"M6 4.5V3c0-.6.4-1 1-1h2c.6 0 1 .4 1 1v1.5\"/><path d=\"m3.8 4.5.6 8.1c.1.8.7 1.4 1.5 1.4h4.2c.8 0 1.4-.6 1.5-1.4l.6-8.1\"/><path d=\"M6.6 7.2v4M9.4 7.2v4\"/>",
  tray: "<path d=\"M8 2v6.5M5.6 6.1 8 8.5l2.4-2.4\"/><path d=\"M2 10h3.2l.8 1.5h4l.8-1.5H14v2.5c0 .8-.7 1.5-1.5 1.5h-9c-.8 0-1.5-.7-1.5-1.5Z\"/>",
  tree: "<path d=\"M3 3h5M5 3v9.5h4M5 7.5h4\"/><circle cx=\"11\" cy=\"7.5\" r=\"1.5\"/><circle cx=\"11\" cy=\"12.5\" r=\"1.5\"/>",
  unpin: "<path d=\"M5.5 2.5h5\"/><path d=\"M6.5 2.5v3.8L4.5 9.5h7l-2-3.2V2.5\"/><path d=\"M8 9.5v4M2.5 2.5l11 11\"/>",
  update: "<path d=\"M8 2.5v7.5M4.75 6.75 8 10l3.25-3.25\"/><path d=\"M2.5 10.5v1.5c0 .8.7 1.5 1.5 1.5h8c.8 0 1.5-.7 1.5-1.5v-1.5\"/>",
  warning: "<path d=\"M7.1 2.6a1 1 0 0 1 1.8 0l5.2 9.4a1 1 0 0 1-.9 1.5H2.8a1 1 0 0 1-.9-1.5Z\"/><path d=\"M8 6.2v3\"/><circle cx=\"8\" cy=\"11.2\" r=\".4\" fill=\"currentColor\"/>",
};
export type UiIcon = keyof typeof ui;

export function Icon({ name, size = 16, className }: { name: UiIcon; size?: number; className?: string }) {
  let body: string = ui[name];
  let width = "1.25";
  const custom = /^@([\d.]+)@/.exec(body);
  if (custom) {
    width = custom[1];
    body = body.slice(custom[0].length);
  }
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={width}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={"ico" + (className ? " " + className : "")}
      dangerouslySetInnerHTML={{ __html: body }}
    />
  );
}

// The window's caption glyphs (the component library's 24-unit window icons).
const caption = {
  minimize: "M19.2 12H4.8a.8.8 0 0 1 0-1.6h14.4a.8.8 0 0 1 0 1.6Z",
  maximize:
    "M6.36 4h11.28A2.36 2.36 0 0 1 20 6.36v11.28A2.36 2.36 0 0 1 17.64 20H6.36A2.36 2.36 0 0 1 4 17.64V6.36A2.36 2.36 0 0 1 6.36 4Zm.04 1.6a.8.8 0 0 0-.8.8v11.2c0 .44.36.8.8.8h11.2a.8.8 0 0 0 .8-.8V6.4a.8.8 0 0 0-.8-.8Z",
  restore:
    "M18.4 16.66V8.74a3.24 3.24 0 0 0-3.2-3.14H7.34A2.4 2.4 0 0 1 9.6 4h5.6A4.8 4.8 0 0 1 20 8.8v5.6a2.4 2.4 0 0 1-1.6 2.26ZM6.36 7.2h8.08a2.36 2.36 0 0 1 2.36 2.36v8.08A2.36 2.36 0 0 1 14.44 20H6.36A2.36 2.36 0 0 1 4 17.64V9.56A2.36 2.36 0 0 1 6.36 7.2Zm.04 1.6a.8.8 0 0 0-.8.8v8a.8.8 0 0 0 .8.8h8a.8.8 0 0 0 .8-.8v-8a.8.8 0 0 0-.8-.8Z",
  close:
    "M4.23 4.23a.8.8 0 0 1 1.14 0L12 10.87l6.63-6.64a.8.8 0 1 1 1.14 1.14L13.13 12l6.64 6.63a.8.8 0 1 1-1.14 1.14L12 13.13l-6.63 6.64a.8.8 0 0 1-1.14-1.14L10.87 12 4.23 5.37a.8.8 0 0 1 0-1.14Z",
};
export function CaptionIcon({ name }: { name: keyof typeof caption }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className="ico">
      <path d={caption[name]} />
    </svg>
  );
}

// Process icons. Programs without an icon of their own take the app's outline
// glyphs (generic app, kernel); the rest are small full-colour stand-ins for
// the icons Windows would extract, drawn generically.
const art: Partial<Record<Glyph, string>> = {
  browser:
    '<circle cx="8" cy="8" r="6.5" fill="#1b8ad6"/><path d="M1.6 8h12.8M8 1.5c1.8 1.8 2.7 4 2.7 6.5S9.8 12.7 8 14.5M8 1.5C6.2 3.3 5.3 5.5 5.3 8s.9 4.7 2.7 6.5" fill="none" stroke="#dff1fd" stroke-width="1"/>',
  code: '<rect x="1.5" y="1.5" width="13" height="13" rx="3" fill="#2f7fd0"/><path d="M6.4 5.1 3.7 8l2.7 2.9M9.6 5.1 12.3 8l-2.7 2.9" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>',
  folder:
    '<path d="M1.5 4c0-.8.6-1.5 1.4-1.5h3.3l1.5 1.5h5.4c.8 0 1.4.7 1.4 1.5v.6h-13Z" fill="#dc9a22"/><rect x="1.5" y="5.4" width="13" height="8.1" rx="1.4" fill="#f5c141"/>',
  terminal:
    '<rect x="1.5" y="2.5" width="13" height="11" rx="2.2" fill="#2c2c33" stroke="#74747f" stroke-width=".8"/><path d="m4.4 6.6 2.1 1.6-2.1 1.6M8 10.4h3.6" fill="none" stroke="#ececf0" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>',
  console:
    '<rect x="1.5" y="2.5" width="13" height="11" rx="1.2" fill="#161616" stroke="#9b9ba3" stroke-width=".8"/><path d="M1.9 5h12.2" stroke="#9b9ba3" stroke-width=".8"/><path d="m4 7.2 1.7 1.4L4 10" fill="none" stroke="#e0e0e0" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/>',
  pwsh: '<rect x="1.5" y="2.5" width="13" height="11" rx="2" fill="#24527d"/><path d="m4.5 5.9 2.6 2.1-2.6 2.1M8.6 10.4h3" fill="none" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>',
  paint:
    '<path d="M8 1.8c3.6 0 6.2 2.4 6.2 5.3 0 1.8-1.3 2.7-2.9 2.7h-1.2c-.8 0-1.2.7-.9 1.3.6 1.1-.1 3-1.6 3C4.2 14.1 1.8 11.4 1.8 8 1.8 4.6 4.5 1.8 8 1.8Z" fill="#f3eee4" stroke="#c9bfae" stroke-width=".6"/><circle cx="5" cy="7.3" r="1.15" fill="#e2463c"/><circle cx="7.4" cy="4.8" r="1.15" fill="#f2b33d"/><circle cx="10.4" cy="5.4" r="1.15" fill="#3d9be9"/><circle cx="5.5" cy="10.5" r="1.15" fill="#4caf50"/>',
  note: '<path d="M3.5 1.5h6.2l2.8 2.8v9.2c0 .6-.4 1-1 1h-8c-.6 0-1-.4-1-1v-11c0-.6.4-1 1-1Z" fill="#f4f6f9" stroke="#b6c2d0" stroke-width=".6"/><path d="M3.5 1.5h6.2l1.6 1.6v1.6H2.5V2.5c0-.6.4-1 1-1Z" fill="#3b86d6"/><path d="M4.8 7.6h6.4M4.8 9.7h6.4M4.8 11.8h4" stroke="#8aa3bf" stroke-width=".9"/>',
  gear: '<path d="M6.82 3.24 6.97 1.79h2.06l.15 1.45 1.35.56 1.13-.93 1.47 1.47-.93 1.13.56 1.35 1.45.15v2.06l-1.45.15-.56 1.35.93 1.13-1.47 1.47-1.13-.93-1.35.56-.15 1.45H6.97l-.15-1.45-1.35-.56-1.13.93-1.47-1.47.93-1.13-.56-1.35-1.45-.15V6.97l1.45-.15.56-1.35-.93-1.13 1.47-1.47 1.13.93Z" fill="#7d8796"/><circle cx="8" cy="8" r="2.1" fill="#e3e7ec"/>',
  cloud: '<path d="M4.4 13.2h8.1a3 3 0 0 0 .5-6A4.4 4.4 0 0 0 4.6 6.1a3.6 3.6 0 0 0-.2 7.1Z" fill="#1f7ad8"/><path d="M6.2 13.2h6.3a3 3 0 0 0 .5-6 4.4 4.4 0 0 0-1.5-2.3A4 4 0 0 0 6.2 8.8Z" fill="#3b95ee"/>',
  calc: '<rect x="3" y="1.5" width="10" height="13" rx="1.8" fill="#4b515e"/><rect x="4.5" y="3.1" width="7" height="2.6" rx=".6" fill="#d4dbe4"/><path d="M5.3 7.9h.1M8 7.9h.1M10.7 7.9h.1M5.3 10.1h.1M8 10.1h.1M5.3 12.3h.1M8 12.3h.1" stroke="#d4dbe4" stroke-width="1.5" stroke-linecap="round"/><path d="M10.7 10.1v2.2" stroke="#f08a24" stroke-width="1.5" stroke-linecap="round"/>',
  phone:
    '<rect x="4.4" y="1.5" width="7.2" height="13" rx="1.6" fill="#3f6fd8"/><rect x="5.5" y="3" width="5" height="8.6" rx=".5" fill="#d3e2ff"/><circle cx="8" cy="13" r=".6" fill="#d3e2ff"/>',
  shield:
    '<path d="M8 1.5 13.5 3.5v4.2c0 3.3-2.3 5.8-5.5 6.9-3.2-1.1-5.5-3.6-5.5-6.9V3.5Z" fill="#2f7fd0"/><path d="M8 1.5v13.1c3.2-1.1 5.5-3.6 5.5-6.9V3.5Z" fill="#1c62ab"/>',
  search:
    '<circle cx="6.8" cy="6.8" r="4.4" fill="#e8f2fc" stroke="#2f80d1" stroke-width="1.7"/><path d="m10.2 10.2 3.6 3.6" stroke="#2f80d1" stroke-width="2.1" stroke-linecap="round"/>',
  node: '<path d="M8 1.2 14 4.6v6.8L8 14.8 2 11.4V4.6Z" fill="#5c9e4d"/><path d="M8 1.2V8l6-3.4Z" fill="#76b865"/>',
  service:
    '<rect x="1.5" y="2.5" width="13" height="11" rx="1.6" fill="#dde2e8" stroke="#98a2ae" stroke-width=".6"/><path d="M1.5 4.1c0-.9.7-1.6 1.6-1.6h9.8c.9 0 1.6.7 1.6 1.6v1.2h-13Z" fill="#6f7a88"/><circle cx="8" cy="9.4" r="2.3" fill="none" stroke="#56606d" stroke-width="1.3"/><path d="M8 6.4v.8M8 11.6v.8M5 9.4h.8M10.2 9.4h.8" stroke="#56606d" stroke-width="1.1" stroke-linecap="round"/>',
};

export function ProcessIcon({ glyph, size = 16 }: { glyph: Glyph; size?: number }) {
  if (glyph === "argus")
    return <img src="/assets/argus.svg" width={size} height={size} alt="" className="pico" draggable={false} />;
  const drawn = art[glyph];
  if (drawn)
    return (
      <svg
        width={size}
        height={size}
        viewBox="0 0 16 16"
        aria-hidden="true"
        className="pico"
        dangerouslySetInnerHTML={{ __html: drawn }}
      />
    );
  return <Icon name={glyph === "kernel" ? "kernel" : "genericApp"} size={size} className="pico is-outline" />;
}
