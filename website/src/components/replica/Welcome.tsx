/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useEffect, useState } from "react";
import Hundred from "./Hundred";
import { Button, Segments, Select, Switch } from "./widgets";
import { setWidgets, useShared } from "./store";

const steps = [
  [
    "欢迎使用 Argus",
    "Argus 是一款现代的 Windows 任务管理器，可以查看进程、性能、服务、启动项与网络连接，并记录资源占用的历史。接下来用几步完成常用设置；每一步都可以跳过，之后也能在设置中修改。",
  ],
  ["显示与刷新", "选择导航栏的位置、动画效果与数据的刷新频率。"],
  ["常驻后台", "Argus 可以常驻在托盘中，持续记录历史并执行规则，需要时立即打开。"],
  ["保持最新", "Argus 从 GitHub Releases 获取新版本。安装包须通过签名校验，安装前需手动确认。"],
  [
    "特权服务（可选）",
    "Argus 当前以管理员身份运行，进程管理操作无需特权服务；内核传感器和任务管理器替换仍需安装服务。",
  ],
  ["内核传感器（可选）", "读取处理器内部传感器，补充温度与功耗数据。"],
  [
    "替换系统任务管理器（可选）",
    "开启后，按 Ctrl+Shift+Esc、在任务栏右键菜单中选择“任务管理器”等打开任务管理器的方式，都会打开 Argus。",
  ],
  ["准备就绪", "右键单击进程，可结束进程、调整优先级、开启效率模式或添加规则"],
];
export default function Welcome({
  tick,
  interval,
  navLeft,
  onNav,
  onInterval,
  close,
}: {
  tick: number;
  interval: number;
  navLeft: boolean;
  onNav: (left: boolean) => void;
  onInterval: (v: number) => void;
  close: () => void;
}) {
  const [step, setStep] = useState(0);
  const { widgets } = useShared();
  useEffect(() => {
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [close]);
  return (
    <div className="a-scrim">
      <div className="a-dialog" role="dialog" aria-modal="true" aria-label={steps[step][0]}>
        <div className="a-dialog-body">
          {step === 0 && <Hundred tick={tick} />}
          <h3>{steps[step][0]}</h3>
          <p>{steps[step][1]}</p>
          {step === 1 && (
            <div className="a-watch-buttons">
              <Segments
                label="导航栏位置"
                value={navLeft ? "left" : "top"}
                onChange={(v) => onNav(v === "left")}
                items={[
                  { value: "top", label: "顶部" },
                  { value: "left", label: "左侧" },
                ]}
              />
              <Select
                label="刷新频率"
                value={interval}
                onChange={onInterval}
                options={[500, 1000, 2000, 5000].map((value) => ({ value, label: "每 " + value / 1000 + " 秒" }))}
              />
            </div>
          )}
          {step === 2 && (
            <label>
              关闭窗口时保留在托盘{" "}
              <Switch
                label="关闭窗口时保留在托盘"
                checked={widgets.closeToTray}
                onChange={(closeToTray) => setWidgets({ closeToTray })}
              />
            </label>
          )}
        </div>
        <div className="a-dialog-foot">
          <Button onClick={close}>跳过</Button>
          {step > 0 && <Button onClick={() => setStep(step - 1)}>上一步</Button>}
          <Button tone="accent" onClick={() => (step === steps.length - 1 ? close() : setStep(step + 1))}>
            {step === steps.length - 1 ? "开始使用" : "下一步"}
          </Button>
        </div>
      </div>
    </div>
  );
}
