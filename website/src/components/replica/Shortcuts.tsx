/** @jsxImportSource @/components/replica/i18n */
"use client";
import { Fragment, useState, type ReactNode } from "react";
import { Icon, type UiIcon } from "./icons";
import { Button } from "./widgets";

// Keys as keycaps: chords split by " / ", keys by "+" (settings/shortcuts.rs `shortcut_keys`).
export function Keys({ keys }: { keys: string }) {
  return (
    <span className="a-shortcut-keys" translate="no">
      {keys.split(" / ").map((chord, i) => (
        <Fragment key={i}>
          {i > 0 && <small>/</small>}
          <span>
            {(chord === "+" ? [chord] : chord.split("+")).map((key, j) => (
              <Fragment key={j}>
                {j > 0 && <small>+</small>}
                <kbd>{key}</kbd>
              </Fragment>
            ))}
          </span>
        </Fragment>
      ))}
    </span>
  );
}

const common: [UiIcon, string, string, string][] = [
  ["pause", "临时暂停刷新", "在进程页或性能页按住时暂停刷新，松开后恢复原状态。", "Ctrl"],
  ["end", "结束进程", "结束选中的进程或应用组，操作前确认。", "Alt+E"],
  ["search", "搜索进程", "转到进程搜索。未输入文字时，也可按 /；在进程页直接键入字母或数字也会开始搜索。", "Ctrl+F"],
  ["runTask", "运行新任务", "在任意页面启动程序，或打开文件、文件夹；输入文字时不生效。", "Ctrl+N"],
];
// The groups that fold, and their keys (or gestures, named in words).
const groups: [string, UiIcon, string, [string, string, boolean?][]][] = [
  [
    "table",
    "processes",
    "列表获得焦点时",
    [
      ["↑ / ↓", "选择上一项 / 下一项"],
      ["PageUp / PageDown", "向上 / 向下选择一页"],
      ["Home / End", "选择第一项 / 最后一项"],
      ["← / →", "进程列表：折叠 / 展开进程组或分组，或选择父项 / 子项"],
      ["Enter", "进程列表：打开详细信息"],
      ["Ctrl+C", "复制选中项的名称"],
      ["Ctrl+Shift+C", "复制选中项的详细信息"],
      ["Delete", "进程列表：结束选中项，操作前确认"],
      ["Shift+Delete", "进程列表：直接结束选中项，跳过确认"],
      ["Esc", "取消选择；进程列表有搜索条件时先清空搜索。"],
      ["Shift + 滚轮", "指针位于列表上时横向滚动，查看其余的列", true],
    ],
  ],
  [
    "history",
    "history",
    "历史曲线获得焦点时",
    [
      ["← / →", "选择前一个 / 后一个时间点"],
      ["Shift+← / Shift+→", "向前 / 向后移动较大范围"],
      ["PageUp / PageDown", "向前 / 向后移动较大范围"],
      ["Home", "选择图表中最早的时间点"],
      ["End", "回到最新读数并继续更新"],
      ["+ / −", "放大 / 缩小时间范围"],
      ["0", "显示整个范围"],
      ["Esc", "清除游标与选区"],
    ],
  ],
  [
    "historyMouse",
    "pointer",
    "历史图表中的鼠标操作",
    [
      ["悬停", "查看读数与当时的进程", true],
      ["单击", "固定游标；曲线按平均值显示时选择所单击的一段", true],
      ["拖动图表", "选择区间", true],
      ["右键单击图表", "回到最新读数并继续更新", true],
      ["单击游标标记", "清除游标与选区", true],
      ["滚轮", "缩放时间范围；显示最新读数时以右端为准", true],
      ["Shift + 滚轮", "平移时间范围，并暂停自动更新", true],
      ["双击图表", "显示整个范围", true],
      ["拖动导航条", "平移或缩放；仅暂停自动更新，不会自动回到最新", true],
      ["拖动轨道间的分隔线", "调整轨道高度", true],
      ["拖动轨道标题左侧的把手", "调整轨道顺序", true],
      ["将读数拖到图表上", "叠加到轨道，或新建轨道", true],
    ],
  ],
];

function Card({ icon, title, keys, body, badge }: { icon: UiIcon; title: string; keys: ReactNode; body: string; badge?: string }) {
  return (
    <div className="a-shortcut-card">
      <div className="a-shortcut-card-head">
        <Icon name={icon} />
        <span>{title}</span>
        {badge && <small>{badge}</small>}
      </div>
      {keys}
      <p>{body}</p>
    </div>
  );
}

// "快捷键说明" (settings/shortcuts.rs `shortcuts_dialog`): the common keys as cards, the
// pages, the keys of the lists and of the history chart in groups that fold
// (opened on the history page), and the system's own shortcut.
export default function Shortcuts({
  pages,
  history,
  onClose,
}: {
  pages: [string, string, UiIcon][];
  history: boolean;
  onClose: () => void;
}) {
  const [open, setOpen] = useState<Record<string, boolean>>({ history, historyMouse: history });
  const cards = [
    ...common.map(([icon, title, body, keys]) => <Card key={title} icon={icon} title={title} keys={<Keys keys={keys} />} body={body} />),
    <Card key="hotkey" icon="keyboard" title="显示或隐藏 Argus" keys={<Keys keys="Ctrl+Alt+Esc" />} body="当前配置，在其他程序中也可使用。" badge="全局" />,
  ];
  return (
    <div className="a-dialog is-shortcuts" role="dialog" aria-modal="true" aria-labelledby="a-dialog-title" tabIndex={-1}>
      <div className="a-dialog-body a-scroll">
        <header className="a-shortcuts-head">
          <span className="a-shortcuts-glyph">
            <Icon name="keyboard" size={24} />
          </span>
          <div>
            <h3 id="a-dialog-title">快捷键说明</h3>
            <p>常用操作、页面导航与列表快捷键。</p>
          </div>
          <Button icon="dismiss" title="关闭" label="关闭" onClick={onClose} />
        </header>
        <div className="a-shortcuts-body">
          <div className="a-shortcut-cards">{cards}</div>
          <div className="a-shortcuts-pages-head">
            <b>切换页面</b>
            <Keys keys={`Ctrl+1–${pages.length}`} />
          </div>
          <div className="a-shortcuts-pages">
            {pages.map(([id, name, icon], i) => (
              <span key={id}>
                <Icon name={icon} size={15} />
                <span data-i18n="nav">{name}</span>
                <Keys keys={`Ctrl+${i + 1}`} />
              </span>
            ))}
          </div>
          {groups.map(([key, icon, title, items]) => (
            <section key={key} className="a-shortcut-group">
              <button type="button" aria-expanded={!!open[key]} onClick={() => setOpen((o) => ({ ...o, [key]: !o[key] }))}>
                <Icon name={icon} />
                <b>{title}</b>
                <Icon name={open[key] ? "chevronUp" : "chevronDown"} size={14} />
              </button>
              {open[key] && (
                <ul>
                  {items.map(([keys, text, gesture]) => (
                    <li key={keys + text}>
                      <span>{text}</span>
                      {gesture ? <span className="a-shortcut-gesture">{keys}</span> : <Keys keys={keys} />}
                    </li>
                  ))}
                </ul>
              )}
            </section>
          ))}
          <p className="a-shortcuts-system">
            <Keys keys="Ctrl+Shift+Esc" />
            启用“替换系统任务管理器”后打开 Argus。
          </p>
          <p className="a-shortcuts-note">窗口内的快捷键不影响其他程序；输入框、弹窗或菜单中不触发结束进程。</p>
        </div>
      </div>
      <div className="a-dialog-foot is-shortcuts">
        <span>
          <Keys keys="Esc" />
          关闭说明
        </span>
        <button type="button" className="a-btn is-accent is-wide" onClick={onClose} data-primary>
          关闭
        </button>
      </div>
    </div>
  );
}
