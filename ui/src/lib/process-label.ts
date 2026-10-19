import { t } from "./i18n";
import type { ProcessRow } from "./types";

export function webViewTitle(p: ProcessRow): string | null {
  const host = p.webView?.hostDescription ?? p.webView?.hostName;
  return host ? t("processes.webView.host", { name: host }) : null;
}

export function processTitle(p: ProcessRow): string {
  return webViewTitle(p) ?? p.description ?? p.name;
}

export function webViewRole(p: ProcessRow): string | null {
  const role = p.webView?.role;
  return role && role !== "unknown" ? t(`processes.webView.role.${role}`) : null;
}

export function processDetail(p: ProcessRow): string | null {
  const role = webViewRole(p);
  return role ? `${p.name} · ${role}` : webViewTitle(p) ? p.name : p.description;
}
