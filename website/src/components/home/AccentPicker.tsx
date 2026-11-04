"use client";
import { store, useShared, type Accent } from "../replica/store";

// Windows accent colours in the light and dark variants the app uses for each mode.
const accents: { name: string; accent: Accent | null; swatch: string }[] = [
  { name: "跟随系统（默认蓝）", accent: null, swatch: "#0078d4" },
  { name: "橙色", accent: { light: "#b55400", dark: "#ffb36b" }, swatch: "#f7630c" },
  { name: "红色", accent: { light: "#b81d34", dark: "#ff99a4" }, swatch: "#e81123" },
  { name: "海沫绿", accent: { light: "#00786a", dark: "#5fe0c4" }, swatch: "#00b294" },
  { name: "石板灰", accent: { light: "#4c5a66", dark: "#b4c3cf" }, swatch: "#69797e" },
];

export default function AccentPicker() {
  const { accent } = useShared();
  const style = accent
    ? ({ "--a-accent-l": accent.light, "--a-accent-d": accent.dark } as Record<string, string>)
    : undefined;
  return (
    <div className="accent">
      <div className="accent-swatches" role="radiogroup" aria-label="强调色">
        {accents.map((a) => (
          <button
            key={a.name}
            role="radio"
            aria-checked={(accent?.light ?? null) === (a.accent?.light ?? null)}
            aria-label={a.name}
            title={a.name}
            style={{ background: a.swatch }}
            onClick={() => store.set({ accent: a.accent })}
          />
        ))}
      </div>
      <div className="argus-app accent-sample" data-ui style={style} aria-hidden="true">
        <div className="accent-tabs">
          <span className="accent-tab is-current">进程</span>
          <span className="accent-tab">性能</span>
          <span className="accent-tab">服务</span>
          <span className="accent-tab">连接</span>
        </div>
        {[
          ["Windows 资源管理器", "explorer.exe", "0.4", "169.4 MB", false],
          ["Argus", "argus.exe", "0.3", "17.8 MB", true],
          ["记事本", "Notepad.exe", "0.1", "48.0 MB", false],
        ].map(([name, exe, cpu, mem, selected]) => (
          <div className={"accent-row a-tr" + (selected ? " is-selected" : "")} key={String(exe)}>
            <span className="a-td a-td-name">
              {selected ? (
                <img className="pico" src="/assets/argus.svg" width="16" height="16" alt="" />
              ) : (
                <span className="pico accent-glyph" />
              )}
              <span className="a-name">{name}</span>
              <span className="a-exe">{exe}</span>
            </span>
            <span className="a-td is-end">{cpu}</span>
            <span className="a-td is-end">{mem}</span>
          </div>
        ))}
        <div className="accent-controls">
          <span className="a-seg" aria-hidden="true">
            <button aria-checked="true" role="radio" tabIndex={-1}>
              整体
            </button>
            <button aria-checked="false" role="radio" tabIndex={-1}>
              多核
            </button>
          </span>
          <span className="a-switch" aria-checked="true" role="switch">
            <i />
          </span>
          <span className="a-button is-accent">确定</span>
        </div>
      </div>
    </div>
  );
}
