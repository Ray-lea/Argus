import { useId, useState } from "react";
import type { Language } from "../lib/site";
import { useInteractive } from "../lib/use-interactive";
type Sample = { ms: number; cpu: number; memory: number; gpu: number };
type Metric = "cpu" | "memory" | "gpu";
export default function ResourceReplay({
  lang,
  samples,
}: {
  lang: Language;
  samples: Sample[];
}) {
  const en = lang === "en",
    [metric, setMetric] = useState<Metric>("cpu");
  const interactive = useInteractive();
  const [index, setIndex] = useState(() =>
    samples.reduce((best, p, i) => (p.cpu > samples[best].cpu ? i : best), 0),
  );
  const [first] = samples,
    last = samples[samples.length - 1],
    span = Math.max(1, last.ms - first.ms);
  const label = { cpu: "CPU", memory: en ? "Memory" : "内存", gpu: "GPU" },
    point = samples[index],
    id = useId();
  const x = (s: Sample) => ((s.ms - first.ms) * 560) / span;
  // A narrow, labeled scale keeps small changes in memory use readable.
  const values = samples.map((s) => s[metric]);
  const low = metric === "memory" ? Math.floor(Math.min(...values)) : 0;
  const high =
    metric === "memory"
      ? Math.max(low + 1, Math.ceil(Math.max(...values)))
      : 100;
  const y = (s: Sample) => 130 - ((s[metric] - low) / (high - low)) * 120;
  const path = samples
    .map((s, i) => (i ? "L" : "M") + x(s).toFixed(1) + " " + y(s).toFixed(1))
    .join(" ");
  return (
    <figure className={"replay-frame metric-" + metric}>
      <figcaption className="sr-only">
        {en
          ? "Recorded resource utilization over one minute"
          : "一分钟实测资源使用率"}
      </figcaption>
      <div className="replay-bar">
        <span>
          <i />
          {en ? "Recorded sample" : "采样回放"}
        </span>
        <div role="group" aria-label={en ? "Resource" : "资源类型"}>
          {(Object.keys(label) as Metric[]).map((m) => (
            <button
              type="button"
              key={m}
              disabled={!interactive}
              aria-pressed={m === metric}
              onClick={() => setMetric(m)}
            >
              {label[m]}
            </button>
          ))}
        </div>
      </div>
      <div className="replay-readout">
        <strong>
          {point[metric].toFixed(1)}
          <small>%</small>
        </strong>
        <span>
          {label[metric]} · {((point.ms - first.ms) / 1000).toFixed(0)} s
        </span>
      </div>
      <div className="replay-chart">
        <div className="replay-axis" aria-hidden="true">
          <span>{high}%</span>
          <span>{low}%</span>
        </div>
        <svg
          viewBox="0 0 560 140"
          preserveAspectRatio="none"
          role="img"
          aria-label={
            en
              ? label[metric] +
                " recorded utilization, scale " +
                low +
                "–" +
                high +
                "%"
              : label[metric] +
                " 使用率记录，纵轴 " +
                low +
                "% 至 " +
                high +
                "%"
          }
        >
          <path d="M0 10H560M0 70H560M0 130H560" className="plot-grid" />
          <path d={path + " L560 130H0Z"} className="plot-fill" />
          <path d={path} className="plot-line" />
          <path d={"M" + x(point) + " 5V130"} className="plot-cursor" />
          <circle r="4.8" cx={x(point)} cy={y(point)} className="plot-dot" />
        </svg>
      </div>
      <div className="replay-time">
        <span>0 s</span>
        <label htmlFor={id}>{en ? "Sample time" : "采样时刻"}</label>
        <span>{Math.round(span / 1000)} s</span>
      </div>
      <input
        id={id}
        type="range"
        disabled={!interactive}
        min="0"
        max={samples.length - 1}
        value={index}
        onChange={(e) => setIndex(Number(e.target.value))}
        aria-valuetext={
          Math.round((point.ms - first.ms) / 1000) +
          " s, " +
          label[metric] +
          " " +
          point[metric].toFixed(1) +
          "%"
        }
      />
    </figure>
  );
}
