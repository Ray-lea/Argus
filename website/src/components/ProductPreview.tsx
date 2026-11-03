import { useState } from "react";
import { Lens } from "./magicui/Lens";
import type { Language } from "../lib/site";

export default function ProductPreview({
  lang,
  tag,
}: {
  lang: Language;
  tag: string;
}) {
  const en = lang === "en";
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  const asset = `/assets/preview-${lang}-${theme}`;
  return (
    <figure className="preview">
      <div className="preview-tools flex items-center justify-between gap-3">
        <span className="preview-title">
          <i aria-hidden="true" />
          {en ? "Process view" : "进程视图"}
        </span>
        <div
          className="preview-switch"
          role="group"
          aria-label={en ? "Screenshot appearance" : "截图主题"}
        >
          {(["dark", "light"] as const).map((t) => (
            <button
              key={t}
              type="button"
              aria-pressed={theme === t}
              onClick={() => setTheme(t)}
            >
              {en
                ? t === "dark"
                  ? "Dark"
                  : "Light"
                : t === "dark"
                  ? "深色"
                  : "浅色"}
            </button>
          ))}
        </div>
      </div>
      <Lens
        label={
          en
            ? "Inspect the process table. Move the pointer or use the arrow keys to magnify."
            : "查看进程表。移动指针或使用方向键放大读数。"
        }
      >
        <picture>
          <source media="(max-width: 900px)" srcSet={asset + "-detail.webp"} />
          <img
            src={asset + "-1280.webp"}
            srcSet={asset + "-720.webp 720w, " + asset + "-1280.webp 1280w"}
            sizes="(max-width: 900px) 92vw, 55vw"
            width="1920"
            height="1200"
            fetchPriority="high"
            alt={
              en
                ? "Argus process view with CPU, memory, disk, network and GPU readings"
                : "Argus 进程视图及 CPU、内存、磁盘、网络与 GPU 读数"
            }
          />
        </picture>
      </Lens>
      <figcaption className="preview-caption">
        <span>Argus {tag}</span>
        <a href={asset + ".png"}>
          {en ? "Open full screenshot" : "打开完整截图"}{" "}
          <span aria-hidden="true">↗</span>
        </a>
      </figcaption>
    </figure>
  );
}
