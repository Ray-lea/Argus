// React's JSX runtime with the demo's translation applied to HTML elements
// (see ./index.ts). Files opt in with a jsxImportSource pragma.
import * as runtime from "react/jsx-runtime";
import { translateProps } from "./index";

export type { JSX } from "react/jsx-runtime";
export const Fragment = runtime.Fragment;
export const jsx: typeof runtime.jsx = (type, props, key) => runtime.jsx(type, translateProps(type, props), key);
export const jsxs: typeof runtime.jsxs = (type, props, key) => runtime.jsxs(type, translateProps(type, props), key);
