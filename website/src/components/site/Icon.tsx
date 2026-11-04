const paths = {
  download: "M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5",
  arrow: "M4 12h15m-6-6 6 6-6 6",
  external: "M7 17 17 7M7 7h10v10",
  language:
    "M2 6h12M8 3v3m4 0c-1 6-5 10-9 12m2-9c1 3 3 5 6 7m2 5 4-11 5 11m-7-4h5",
  search: "M21 21l-5-5m2-6a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
  theme: "M12 3a9 9 0 1 0 0 18 9 9 0 1 0 0-18Zm0 0v18",
  chevron: "m6 9 6 6 6-6",
  menu: "M4 7h16M4 12h16M4 17h16",
  close: "M6 6l12 12M18 6 6 18",
  book: "M3 5c3-1 6-1 9 1 3-2 6-2 9-1v15c-3-1-6-1-9 1-3-2-6-2-9-1zm9 1v15",
  github:
    "M9 19c-4 1.3-4-2-6-2.5m12 4.5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1-.3-3.4 1.3a11.8 11.8 0 0 0-6.2 0C6.6 2.8 5.6 3.1 5.6 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4.2 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21",
  plus: "M12 5v14M5 12h14",
  check: "m5 12 5 5 9-10",
  edit: "M4 20h4L19 9l-4-4L4 16v4Zm9-13 4 4",
  home: "M4 11 12 4l8 7v9h-5v-6H9v6H4z",
  copy: "M9 9h11v11H9zM15 9V4H4v11h5",
  link: "M10 14a4 4 0 0 0 6 0l3-3a4 4 0 0 0-6-6l-1 1m2 4a4 4 0 0 0-6 0l-3 3a4 4 0 0 0 6 6l1-1",
  sun: "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8ZM12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
  moon: "M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z",
  monitor: "M3 4h18v12H3zM8 20h8M12 16v4",
  outline: "M4 6h16M4 12h10M4 18h13",
  updown: "m7 9 5-5 5 5M7 15l5 5 5-5",
  clock: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18Zm0-13v4l3 2",
  rotate: "M6 3h7a1 1 0 0 1 1 1v16a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Zm11 4a5 5 0 0 1 3 4.5m0 0 1.5-1.5M20 11.5 18.5 10",
  star: "m12 3.5 2.6 5.3 5.9.9-4.3 4.1 1 5.8-5.2-2.7-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z",
  fork: "M6 3a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Zm12 0a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5Zm-6 13a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5ZM6 8v1a3 3 0 0 0 3 3h6a3 3 0 0 0 3-3V8m-6 4v4",
  scale: "M12 3v18M7 21h10M5 7h14M5 7l-3 7a3.5 3.5 0 0 0 6 0zm14 0-3 7a3.5 3.5 0 0 0 6 0z",
  tag: "M3 12V4a1 1 0 0 1 1-1h8l9 9-9 9zm5-5h.01",
  commit: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM2 12h6m8 0h6",
  box: "M21 8 12 3 3 8v8l9 5 9-5zM3 8l9 5 9-5M12 13v8",
  file: "M14 3H6v18h12V7zm0 0v4h4M9 13h6M9 17h4",
  shield: "M12 3 4 6v6c0 4.5 3.4 8 8 9 4.6-1 8-4.5 8-9V6zm-3.5 9 2.5 2.5 4.5-5",
  terminal: "M3 4h18v16H3zM7 9l3 3-3 3m6 0h4",
  replace: "M4 7h13m-4-4 4 4-4 4M20 17H7m4-4-4 4 4 4",
  rss: "M5 19h.01M4 11a9 9 0 0 1 9 9M4 4a16 16 0 0 1 16 16",
  alert: "M12 3.5 2.5 20h19zM12 10v4.5m0 3v.01",
};
export type IconName = keyof typeof paths;

// The site's line icons; plain markup, usable from server and client components.
export function Icon({ name, size = 18 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.7"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name]}></path>
    </svg>
  );
}
