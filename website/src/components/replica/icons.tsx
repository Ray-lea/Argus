import type { Glyph } from "./model";

// The app's line icons: the Lucide icon each of its product icon names draws
// (`product_icons!` in src-gpui/src/frontend/assets.rs), copied from the
// catalog its component library bundles (gpui-kit-assets, Lucide 1.43.0) so
// the shapes are the app's own; lucide-react is a later release and differs.
// 24-unit grid, stroked in the text colour. GitHub is that catalog's solid mark.
const ui = {
  apps: '<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="14" y="3" rx="1"/><rect width="7" height="7" x="14" y="14" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/>',
  arrowRight: '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
  battery: '<path d="M10 14v-4"/><path d="M22 14v-4"/><path d="M6 14v-4"/><rect x="2" y="6" width="16" height="12" rx="2"/>',
  bolt: '<path d="M15.914 4a1.5 1.5 0 00-2.474-1.561l-9 9A1.5 1.5 0 005.5 14h4.002a.5.5 0 01.471.666L8.086 20a1.5 1.5 0 002.475 1.56l9-9A1.5 1.5 0 0018.5 10h-3.997a.5.5 0 01-.472-.667z"/>',
  branchEnd: '<path d="M16 5H3"/><path d="M11 12H3"/><path d="M16 19H3"/><path d="m15.5 9.5 5 5"/><path d="m20.5 9.5-5 5"/>',
  chat: '<path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  chevronDown: '<path d="m6 9 6 6 6-6"/>',
  chevronRight: '<path d="m9 18 6-6-6-6"/>',
  chevronUp: '<path d="m18 15-6-6-6 6"/>',
  chip: '<path d="M12 20v2"/><path d="M12 2v2"/><path d="M17 20v2"/><path d="M17 2v2"/><path d="M2 12h2"/><path d="M2 17h2"/><path d="M2 7h2"/><path d="M20 12h2"/><path d="M20 17h2"/><path d="M20 7h2"/><path d="M7 20v2"/><path d="M7 2v2"/><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="8" y="8" width="8" height="8" rx="1"/>',
  collapseAll: '<path d="m7 20 5-5 5 5"/><path d="m7 4 5 5 5-5"/>',
  columns: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/><path d="M15 3v18"/>',
  computer: '<rect width="20" height="14" x="2" y="3" rx="2"/><line x1="8" x2="16" y1="21" y2="21"/><line x1="12" x2="12" y1="17" y2="21"/>',
  copy: '<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>',
  disk: '<path d="M10 16h.01"/><path d="M2.212 11.577a2 2 0 0 0-.212.896V18a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-5.527a2 2 0 0 0-.212-.896L18.55 5.11A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><path d="M21.946 12.013H2.054"/><path d="M6 16h.01"/>',
  dismiss: '<path d="M18 6 6 18"/><path d="m6 6 12 12"/>',
  document: '<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>',
  end: '<circle cx="12" cy="12" r="10"/><path d="m15 9-6 6"/><path d="m9 9 6 6"/>',
  expandAll: '<path d="m7 15 5 5 5-5"/><path d="m7 9 5-5 5 5"/>',
  external: '<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>',
  eyeOff: '<path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .696 10.747 10.747 0 0 1-1.444 2.49"/><path d="M14.084 14.158a3 3 0 0 1-4.242-4.242"/><path d="M17.479 17.499a10.75 10.75 0 0 1-15.417-5.151 1 1 0 0 1 0-.696 10.75 10.75 0 0 1 4.446-5.143"/><path d="m2 2 20 20"/>',
  eye: '<path d="M2.062 12.348a1 1 0 0 1 0-.696 10.75 10.75 0 0 1 19.876 0 1 1 0 0 1 0 .696 10.75 10.75 0 0 1-19.876 0"/><circle cx="12" cy="12" r="3"/>',
  filter: '<path d="M10 20a1 1 0 0 0 .553.895l2 1A1 1 0 0 0 14 21v-7a2 2 0 0 1 .517-1.341L21.74 4.67A1 1 0 0 0 21 3H3a1 1 0 0 0-.742 1.67l7.225 7.989A2 2 0 0 1 10 14z"/>',
  folder: '<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>',
  fontSize: '<path d="m15 16 2.536-7.328a1.02 1.02 1 0 1 1.928 0L22 16"/><path d="M15.697 14h5.606"/><path d="m2 16 4.039-9.69a.5.5 0 0 1 .923 0L11 16"/><path d="M3.304 13h6.392"/>',
  gauge: '<path d="m12 14 4-4"/><path d="M3.34 19a10 10 0 1 1 17.32 0"/>',
  genericApp: '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M10 4v4"/><path d="M2 8h20"/><path d="M6 4v4"/>',
  github: '<path fill="currentColor" stroke="none" d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"/>',
  globe: '<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>',
  goTo: '<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>',
  gpu: '<path d="M2 17h18a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2H2"/><path d="M2 21V3"/><path d="M7 17v3a1 1 0 0 0 1 1h5a1 1 0 0 0 1-1v-3"/><circle cx="16" cy="11" r="2"/><circle cx="8" cy="11" r="2"/>',
  grip: '<circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/>',
  history: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>',
  interfaceScale: '<circle cx="11" cy="11" r="8"/><line x1="21" x2="16.65" y1="21" y2="16.65"/><line x1="11" x2="11" y1="8" y2="14"/><line x1="8" x2="14" y1="11" y2="11"/>',
  kernel: '<path d="M10 12h4"/><path d="M10 17h4"/><path d="M10 7h4"/><path d="M18 12h2"/><path d="M18 18h2"/><path d="M18 6h2"/><path d="M4 12h2"/><path d="M4 18h2"/><path d="M4 6h2"/><rect x="6" y="2" width="12" height="20" rx="2"/>',
  keyboard: '<path d="M10 8h.01"/><path d="M12 12h.01"/><path d="M14 8h.01"/><path d="M16 12h.01"/><path d="M18 8h.01"/><path d="M6 8h.01"/><path d="M7 16h10"/><path d="M8 12h.01"/><rect width="20" height="16" x="2" y="4" rx="2"/>',
  leaf: '<path d="M11 20a10 10 0 0010-10 25.9 25.9 0 00-1.04-7.281 1 1 0 00-1.755-.325C15.833 5.5 13 5.5 9.8 6.1A7 7 0 0011 20"/><path d="M2 21a5 5 0 012.911-4.544C7.613 15.212 8.351 15.24 11 13"/>',
  list: '<path d="M3 5h.01"/><path d="M3 12h.01"/><path d="M3 19h.01"/><path d="M8 5h13"/><path d="M8 12h13"/><path d="M8 19h13"/>',
  lock: '<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>',
  memory: '<path d="M12 12v-2"/><path d="M12 18v-2"/><path d="M16 12v-2"/><path d="M16 18v-2"/><path d="M2 11h1.5"/><path d="M20 18v-2"/><path d="M20.5 11H22"/><path d="M4 18v-2"/><path d="M8 12v-2"/><path d="M8 18v-2"/><rect x="2" y="6" width="20" height="10" rx="2"/>',
  menu: '<path d="M4 5h16"/><path d="M4 12h16"/><path d="M4 19h16"/>',
  minimize: '<path d="m14 10 7-7"/><path d="M20 10h-6V4"/><path d="m3 21 7-7"/><path d="M4 14h6v6"/>',
  navSide: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>',
  navTop: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/>',
  network: '<rect x="16" y="16" width="6" height="6" rx="1"/><rect x="2" y="16" width="6" height="6" rx="1"/><rect x="9" y="2" width="6" height="6" rx="1"/><path d="M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3"/><path d="M12 12V8"/>',
  note: '<path d="M21 9a2.4 2.4 0 0 0-.706-1.706l-3.588-3.588A2.4 2.4 0 0 0 15 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2z"/><path d="M15 3v5a1 1 0 0 0 1 1h5"/>',
  notes: '<path d="M15 12h-5"/><path d="M15 8h-5"/><path d="M19 17V5a2 2 0 0 0-2-2H4"/><path d="M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3"/>',
  panel: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>',
  panelAuto: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 14v1"/><path d="M15 19v2"/><path d="M15 3v2"/><path d="M15 9v1"/>',
  pause: '<rect x="14" y="3" width="5" height="18" rx="1"/><rect x="5" y="3" width="5" height="18" rx="1"/>',
  pin: '<path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>',
  play: '<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>',
  pointer: '<path d="M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"/>',
  power: '<path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>',
  processes: '<rect width="7" height="7" x="3" y="3" rx="1"/><rect width="7" height="7" x="3" y="14" rx="1"/><path d="M14 4h7"/><path d="M14 9h7"/><path d="M14 15h7"/><path d="M14 20h7"/>',
  pulse: '<path d="M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2"/>',
  reset: '<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>',
  restart: '<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>',
  runTask: '<path d="m7 11 2-2-2-2"/><path d="M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2" ry="2"/>',
  search: '<path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>',
  services: '<path d="M15.39 4.39a1 1 0 0 0 1.68-.474 2.5 2.5 0 1 1 3.014 3.015 1 1 0 0 0-.474 1.68l1.683 1.682a2.414 2.414 0 0 1 0 3.414L19.61 15.39a1 1 0 0 1-1.68-.474 2.5 2.5 0 1 0-3.014 3.015 1 1 0 0 1 .474 1.68l-1.683 1.682a2.414 2.414 0 0 1-3.414 0L8.61 19.61a1 1 0 0 0-1.68.474 2.5 2.5 0 1 1-3.014-3.015 1 1 0 0 0 .474-1.68l-1.683-1.682a2.414 2.414 0 0 1 0-3.414L4.39 8.61a1 1 0 0 1 1.68.474 2.5 2.5 0 1 0 3.014-3.015 1 1 0 0 1-.474-1.68l1.683-1.682a2.414 2.414 0 0 1 3.414 0z"/>',
  settings: '<path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>',
  shield: '<path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/>',
  sortAscending: '<path d="m3 8 4-4 4 4"/><path d="M7 4v16"/><path d="M11 12h4"/><path d="M11 16h7"/><path d="M11 20h10"/>',
  sortDescending: '<path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="M11 4h10"/><path d="M11 8h7"/><path d="M11 12h4"/>',
  sparkle: '<path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z"/><path d="M20 2v4"/><path d="M22 4h-4"/><circle cx="4" cy="20" r="2"/>',
  star: '<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>',
  stop: '<rect width="18" height="18" x="3" y="3" rx="2"/>',
  storage: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M3 5V19A9 3 0 0 0 21 19V5"/><path d="M3 12A9 3 0 0 0 21 12"/>',
  theme: '<circle cx="12" cy="12" r="10"/><path d="M12 18a6 6 0 0 0 0-12v12z"/>',
  tip: '<path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/><path d="M9 18h6"/><path d="M10 22h4"/>',
  timer: '<line x1="10" x2="14" y1="2" y2="2"/><line x1="12" x2="15" y1="14" y2="11"/><circle cx="12" cy="14" r="8"/>',
  transfer: '<path d="m21 16-4 4-4-4"/><path d="M17 20V4"/><path d="m3 8 4-4 4 4"/><path d="M7 4v16"/>',
  trash: '<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  tray: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 15h18"/>',
  tree: '<path d="M8 5h13"/><path d="M13 12h8"/><path d="M13 19h8"/><path d="M3 10a2 2 0 0 0 2 2h3"/><path d="M3 5v12a2 2 0 0 0 2 2h3"/>',
  unpin: '<path d="M12 17v5"/><path d="M15 9.34V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H7.89"/><path d="m2 2 20 20"/><path d="M9 9v1.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h11"/>',
  update: '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>',
  warning: '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
};
export type UiIcon = keyof typeof ui;

// Lucide's own stroke is 2 units; the app draws 1.875, 1.25 px at 16 px.
export function Icon({ name, size = 16, className }: { name: UiIcon; size?: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.875"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={"ico" + (className ? " " + className : "")}
      data-icon={name}
      dangerouslySetInnerHTML={{ __html: ui[name] }}
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
