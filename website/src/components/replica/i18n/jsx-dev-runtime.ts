// The development build's JSX runtime, translated like ./jsx-runtime.ts.
import * as runtime from "react/jsx-dev-runtime";
import { translateProps } from "./index";

export type { JSX } from "react/jsx-dev-runtime";
export const Fragment = runtime.Fragment;
export const jsxDEV: typeof runtime.jsxDEV = (type, props, key, isStatic, source, self) =>
  runtime.jsxDEV(type, translateProps(type, props), key, isStatic, source, self);
