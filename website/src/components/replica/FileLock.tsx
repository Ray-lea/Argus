"use client";
import { useRef } from "react";
import { Icon, ProcessIcon } from "./icons";
import type { Process } from "./model";

export interface Holder {
  proc: Process;
  handles: number;
  directory?: boolean;
}
export interface LockResult {
  names: string[];
  folder: boolean;
  holders: Holder[];
}

// A demonstration answer: which of the running mock processes would plausibly hold this kind of file.
export function inspect(names: string[], folder: boolean, alive: Process[]): LockResult {
  const byPid = new Map(alive.map((p) => [p.pid, p]));
  const ext = (names[0]?.split(".").pop() ?? "").toLowerCase();
  const picks: [number, number, boolean?][] = folder
    ? [
        [13004, 1, true],
        [6120, 2],
      ]
    : ext === "pdf"
      ? [[9876, 1]]
      : ["png", "jpg", "jpeg", "gif", "bmp", "webp", "heic"].includes(ext)
        ? [
            [14868, 1],
            [6120, 1],
          ]
        : ["txt", "md", "log", "json", "ts", "tsx", "js", "rs", "toml", "yaml", "csv", "ini"].includes(ext)
          ? [
              [11324, 2],
              [15204, 1],
            ]
          : [
              [6120, 1],
              [8920, 1],
            ];
  return {
    names,
    folder,
    holders: picks
      .filter(([pid]) => byPid.has(pid))
      .map(([pid, handles, directory]) => ({ proc: byPid.get(pid)!, handles, directory })),
  };
}

// The body of the "谁在占用文件" dialog: the chosen paths (or the drop zone),
// the pickers, then what holds them.
export default function FileLock({
  result,
  checking,
  onFiles,
  onShow,
  onEnd,
  footer = true,
}: {
  result: LockResult | null;
  checking: boolean;
  onFiles: (names: string[], folder: boolean) => void;
  onShow?: (pid: number) => void;
  onEnd: (proc: Process) => void;
  footer?: boolean;
}) {
  const file = useRef<HTMLInputElement>(null);
  const folder = useRef<HTMLInputElement>(null);
  const picked = (input: HTMLInputElement, isFolder: boolean) => {
    const files = [...(input.files ?? [])];
    if (!files.length) return;
    const names = isFolder ? [files[0].webkitRelativePath.split("/")[0] || files[0].name] : files.map((f) => f.name);
    onFiles(names, isFolder);
    input.value = "";
  };
  const base = (name: string) => (result?.folder ? "C:\\Users\\demo\\" : "C:\\Users\\demo\\Documents\\") + name;
  return (
    <div className="a-lock">
      <div className="a-lock-paths">
        {result ? (
          result.names.map((name) => (
            <div className="a-drop-names" key={name}>
              <b>{name}</b>
              <small>{base(name)}</small>
            </div>
          ))
        ) : (
          <div className="a-drop">
            <Icon name="lock" size={20} />
            <span>将文件或文件夹拖放到窗口，或</span>
          </div>
        )}
        <div className="a-lock-actions">
          <button type="button" className="a-btn" onClick={() => file.current?.click()}>
            选择文件
          </button>
          <button type="button" className="a-btn" onClick={() => folder.current?.click()}>
            <Icon name="folder" />
            选择文件夹
          </button>
          {result && (
            <button type="button" className="a-btn is-ghost" disabled={checking} onClick={() => onFiles(result.names, result.folder)}>
              重新检查
            </button>
          )}
          <input ref={file} type="file" hidden multiple onChange={(e) => picked(e.currentTarget, false)} />
          <input ref={folder} type="file" hidden {...{ webkitdirectory: "" }} onChange={(e) => picked(e.currentTarget, true)} />
        </div>
      </div>
      <div className="a-lock-result">
        {checking && (
          <>
            <p className="a-lock-status">正在检查…</p>
            <div className="a-progress" />
          </>
        )}
        {!checking && result && (
          <>
            <p className="a-lock-status">
              {result.holders.length
                ? `找到 ${result.holders.length} 个进程正在使用这些文件或文件夹。`
                : "本次查询未发现可访问的占用进程。"}
            </p>
            <ul className="a-lock-list">
              {result.holders.map(({ proc, handles, directory }) => (
                <li key={proc.pid}>
                  <ProcessIcon glyph={proc.glyph} />
                  <span className="a-lock-proc">
                    <span>{proc.name}</span>
                    <small>
                      {proc.exe} PID {proc.pid}
                      {directory && " · 当前工作目录"}
                    </small>
                  </span>
                  {proc.pid === 6120 && <span className="a-tag">资源管理器</span>}
                  <span className="a-tag a-badge">{handles} 个句柄</span>
                  {onShow && (
                    <button
                      type="button"
                      className="a-btn is-ghost is-small is-icon"
                      aria-label="在进程列表中查看"
                      title="在进程列表中查看"
                      onClick={() => onShow(proc.pid)}
                    >
                      <Icon name="goTo" size={14} />
                    </button>
                  )}
                  <button type="button" className="a-btn is-danger is-small" onClick={() => onEnd(proc)}>
                    <Icon name="end" size={14} />
                    结束
                  </button>
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
      {footer && <p className="a-lock-foot">句柄查询可发现当前目录占用；受保护进程仍可能无法检查。</p>}
    </div>
  );
}
