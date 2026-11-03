import { useState } from "react";
import { Lens } from "./magicui/Lens";
import { useInteractive } from "../lib/use-interactive";
import type { Language } from "../lib/site";
type Scene = "window" | "processes" | "signals";
const scenes = {
  zh: {
    window: ["窗口概览", "按应用查看运行中的进程"],
    processes: ["进程列表", "对照进程名称和资源列"],
    signals: ["资源读数", "聚焦 CPU、内存与磁盘"],
  },
  en: {
    window: ["Overview", "Processes grouped by app"],
    processes: ["Process table", "Process names and resource columns"],
    signals: ["Resource readings", "Inspect CPU, memory and disk"],
  },
} as const;
export default function ProductPreview({
  lang,
  framework,
}: {
  lang: Language;
  framework: string;
}) {
  const en = lang === "en",
    labels = scenes[lang];
  const interactive = useInteractive();
  const [scene, setScene] = useState<Scene>("window"),
    [theme, setTheme] = useState<"dark" | "light">("dark");
  const asset = `/assets/preview-${lang}-${theme}`;
  const edition = en ? framework + " development build" : framework + " 开发版";
  const isFull = scene === "window";
  const src = isFull ? asset + "-1280.webp" : asset + "-" + scene + ".webp";
  return (
    <figure className="preview">
      <div
        className="scene-rail"
        role="group"
        aria-label={en ? "Preview region" : "预览区域"}
      >
        {(Object.keys(labels) as Scene[]).map((key) => (
          <button
            type="button"
            key={key}
            className="scene-button"
            disabled={!interactive}
            aria-pressed={scene === key}
            title={labels[key][1]}
            onClick={() => setScene(key)}
          >
            <img
              src={asset + "-thumb-" + key + ".webp"}
              alt=""
              width="100"
              height="62"
              loading="lazy"
            />
            <span>
              <strong>{labels[key][0]}</strong>
              <small>{labels[key][1]}</small>
            </span>
            <i aria-hidden="true">›</i>
          </button>
        ))}
      </div>
      <div className="studio-stack">
        <div className="preview-workspace">
          <div className="preview-tools">
            <span className="preview-title">
              <i /> {labels[scene][0]}
            </span>
            <div className="screenshot-tools">
              <span>{en ? "Screenshot" : "截图外观"}</span>
              <div
                className="preview-switch"
                role="group"
                aria-label={en ? "Screenshot appearance" : "截图主题"}
              >
                {(["dark", "light"] as const).map((t) => (
                  <button
                    key={t}
                    disabled={!interactive}
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
          </div>
          <Lens
            label={
              en
                ? "Inspect screenshot. Move the pointer or use arrow keys to magnify."
                : "查看界面截图。移动指针或按方向键放大，按 Esc 收起。"
            }
          >
            <picture>
              {isFull && (
                <source
                  media="(max-width:600px)"
                  srcSet={asset + "-detail.webp"}
                />
              )}
              <img
                src={src}
                srcSet={
                  isFull
                    ? asset + "-720.webp 720w, " + asset + "-1280.webp 1280w"
                    : undefined
                }
                sizes={isFull ? "(max-width:720px) 90vw, 70vw" : undefined}
                width="1280"
                height="800"
                loading="eager"
                fetchPriority="high"
                alt={
                  en
                    ? labels[scene][0] + " in Argus for Windows"
                    : "Argus " + labels[scene][0] + "截图"
                }
              />
            </picture>
          </Lens>
          <div className="preview-caption">
            <span>Argus · {edition}</span>
            <a href={asset + ".png"}>
              {en ? "Open full screenshot" : "查看完整原图"} ↗
            </a>
          </div>
        </div>
      </div>
      <figcaption className="sr-only">
        {en
          ? "Argus process view with a minute of recorded CPU, memory, disk, network and GPU readings."
          : "Argus 进程页，顶部五张曲线回放一分钟的 CPU、内存、磁盘、网络与 GPU 采样。"}
      </figcaption>
    </figure>
  );
}
