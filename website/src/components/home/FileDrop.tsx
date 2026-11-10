/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useState, type DragEvent } from "react";
import FileLock, { inspect, type LockResult } from "../replica/FileLock";
import { processes } from "../replica/model";
import { store } from "../replica/store";

// Only the names of dropped files are read, in the browser; nothing is uploaded.
export default function FileDrop() {
  const [result, setResult] = useState<LockResult | null>(null);
  const [checking, setChecking] = useState(false);
  const [over, setOver] = useState(false);
  const files = (names: string[], folder: boolean) => {
    setChecking(true);
    setTimeout(() => {
      setResult(inspect(names, folder, processes));
      setChecking(false);
    }, 420);
  };
  const drop = (event: DragEvent) => {
    event.preventDefault();
    setOver(false);
    const folder = [...event.dataTransfer.items].some((i) => i.webkitGetAsEntry?.()?.isDirectory);
    const names = [...event.dataTransfer.files].map((f) => f.name);
    if (names.length) files(names, folder);
  };
  return (
    <div
      className={"argus-app file-drop" + (over ? " is-over" : "")}
      data-ui
      onDragOver={(e) => {
        if (e.dataTransfer.types.includes("Files")) {
          e.preventDefault();
          setOver(true);
        }
      }}
      onDragLeave={(e) => e.currentTarget === e.target && setOver(false)}
      onDrop={drop}
    >
      {!result && !checking && <span className="file-hint" aria-hidden="true" />}
      <div className="a-dialog file-drop-dialog">
        <h3>谁在占用文件</h3>
        <p className="a-dialog-text">删除或移动文件时提示“文件正在使用”，可在此查找占用该文件的进程。</p>
        <FileLock
          result={result}
          checking={checking}
          onFiles={files}
          onShow={() => store.set({ request: { page: "processes", view: "list", at: Date.now() } })}
          onEnd={(proc) =>
            setResult((r) => r && { ...r, holders: r.holders.filter((h) => h.proc.pid !== proc.pid) })
          }
        />
      </div>
    </div>
  );
}
