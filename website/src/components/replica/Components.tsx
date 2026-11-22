/** @jsxImportSource @/components/replica/i18n */
"use client";
import SectionPage from "./SectionPage";
import { type ReactNode } from "react";
import { Segments, Select, Switch } from "./widgets";
import { Icon, type UiIcon } from "./icons";
import { Readings, Styles } from "./MonitorReadings";
import { setWidgets, setMonitor, useShared, type MonitorKind } from "./store";
const sections: [string, string, UiIcon][] = [
  ["托盘", "托盘", "tray"],
  ["桌面监控", "桌面监控", "computer"],
  ["任务栏监控", "任务栏监控", "gauge"],
  ["更多组件", "更多组件", "apps"],
];
const rowIcons: Record<string, UiIcon> = {
  托盘图标: "tray",
  悬停卡片: "pointer",
  右键单击托盘图标: "apps",
  显示方式: "panel",
  分组: "apps",
  鼠标穿透: "pointer",
  置顶显示: "pin",
  锁定位置: "lock",
  不透明度: "eye",
  位置: "fitWidth",
  显示器: "computer",
  样式: "pulse",
  后台常驻时的刷新间隔: "timer",
  从托盘打开: "tray",
  关闭窗口时保留在托盘: "dismiss",
  桌面监控: "computer",
  任务栏监控: "gauge",
  显示的读数: "list",
  瞭望台: "apps",
};
const descriptions: Record<string, string> = {
  托盘图标: "走势按列显示最近几次采样的 CPU 占用，占比以点阵显示当前占用，数字显示百分比；占用达到 80% 时变为琥珀色。",
  悬停卡片: "指针停留在托盘图标上时显示 CPU、内存、GPU 与网络的实时读数；关闭后显示系统提示。",
  右键单击托盘图标: "快捷面板显示实时读数、占用最高的进程与常用操作；菜单仅列出常用操作。",
  显示方式: "面板按设备分组竖排；横条横向排列，宽度不足时在组之间换行。",
  分组: "列表与曲线样式中区分各设备读数的方式。",
  鼠标穿透: "单击和拖动作用于下方的窗口。开启后仅可在此页关闭。",
  位置: "“任务栏左侧”仅适用于按钮居中的 Windows 11 任务栏，其他情况显示在托盘旁。",
  显示器: "所选显示器未连接或未显示任务栏时，显示在主显示器上。",
  后台常驻时的刷新间隔:
    "间隔越长，后台开销越低，但规则对新启动程序的生效最多延后一个间隔；长于 5 秒时，历史记录也随之变稀。",
  从托盘打开: "通过托盘或快捷键重新打开窗口时生效。",
  关闭窗口时保留在托盘: "隐藏期间历史记录与规则照常运行。",
  显示的读数: "拖动类别或读数可调整顺序。有多块显卡、磁盘或多个网卡时，可为每个设备单独选择。",
};
function Row({
  title,
  desc,
  children,
  wide = false,
}: {
  title: string;
  desc?: string;
  children?: ReactNode;
  wide?: boolean;
}) {
  const description = desc ?? descriptions[title];
  return (
    <div className={"a-setting w-setting" + (wide ? " is-wide" : "")}>
      <Icon name={rowIcons[title] ?? "apps"} size={20} />
      <div className="a-setting-text">
        <div>{title}</div>
        {description && <span>{description}</span>}
      </div>
      <div className="a-setting-controls">{children}</div>
    </div>
  );
}
export default function Components() {
  const { widgets: w, request } = useShared(),
    { section } = w;
  const opts = (list: string[]) => list.map((value) => ({ value, label: value }));

  return (
    <SectionPage
      key={request?.page === "components" ? request.at : "components"}
      entries={sections}
      title="组件"
      target={section}
    >
      {sections.map(([key]) => {
        const section = key;
        const kind: MonitorKind = key === "桌面监控" ? "desktop" : "taskbar";
        return (
          <section
            key={key}
            data-component-section={
              key === "桌面监控" ? "desktop" : key === "任务栏监控" ? "taskbar" : key === "托盘" ? "tray" : "planned"
            }
          >
            <h2>{section}</h2>
            {section === "托盘" ? (
              <>
                <Row
                  title="托盘图标"
                  desc="走势按列显示最近几次采样的 CPU 占用，占比以点阵显示当前占用，数字显示百分比；占用达到 80% 时变为琥珀色。"
                >
                  <Segments
                    label="托盘图标"
                    value={w.icon}
                    onChange={(icon) => setWidgets({ icon })}
                    items={opts(["应用图标", "走势", "占比", "数字"])}
                  />
                </Row>
                <Row
                  title="悬停卡片"
                  desc="指针停留在托盘图标上时显示 CPU、内存、GPU 与网络的实时读数；关闭后显示系统提示。"
                >
                  <Switch state label="悬停卡片" checked={w.card} onChange={(card) => setWidgets({ card })} />
                </Row>
                <Row title="右键单击托盘图标">
                  <Segments
                    label="右键单击托盘图标"
                    value={w.trayMenu}
                    onChange={(trayMenu) => setWidgets({ trayMenu })}
                    items={opts(["快捷面板", "菜单"])}
                  />
                </Row>
                <Row title="关闭窗口时保留在托盘">
                  <Switch
                    state
                    label="关闭窗口时保留在托盘"
                    checked={w.closeToTray}
                    onChange={(closeToTray) => setWidgets({ closeToTray })}
                  />
                </Row>
                <Row title="从托盘打开">
                  <Select
                    label="从托盘打开"
                    value={w.restore}
                    onChange={(restore) => setWidgets({ restore })}
                    options={opts(["保留当前页面", "打开默认页面"])}
                  />
                </Row>
                <Row title="后台常驻时的刷新间隔">
                  <Select
                    label="后台常驻时的刷新间隔"
                    value={w.slow}
                    onChange={(slow) => setWidgets({ slow })}
                    options={[1000, 2000, 5000, 10000, 30000].map((value) => ({
                      value,
                      label: "每 " + value / 1000 + " 秒",
                    }))}
                  />
                </Row>
              </>
            ) : section === "更多组件" ? (
              <Row title="瞭望台" desc="在屏幕顶部以胶囊形式显示实时读数与通知。">
                规划中
              </Row>
            ) : (
              <>
                <Row
                  title={section}
                  desc={
                    kind === "desktop"
                      ? "在桌面上以小窗口显示所选读数。双击打开 Argus，右键单击打开菜单。"
                      : "在任务栏上显示所选读数。单击显示或隐藏窗口，右键单击打开面板，可调整样式与显示的读数。"
                  }
                >
                  <Switch
                    state
                    label={section}
                    checked={w[kind].enabled}
                    onChange={(enabled) => setMonitor(kind, { enabled })}
                  />
                </Row>
                <Row title="显示的读数" wide>
                  <Readings kind={kind} />
                </Row>
                {kind === "desktop" && (
                  <Row title="显示方式">
                    <Segments
                      label="显示方式"
                      items={opts(["面板", "横条"])}
                      value={w.desktop.layout}
                      onChange={(layout) => setMonitor("desktop", { layout })}
                    />
                  </Row>
                )}
                {(kind === "taskbar" || w.desktop.layout === "面板") && (
                  <Row title="样式" wide desc={kind === "taskbar" ? "曲线、胶囊与大数字需要两行文字的高度，任务栏较矮时显示为标签。" : undefined}>
                    <Styles kind={kind} />
                  </Row>
                )}
                {kind === "desktop" && w.desktop.layout === "面板" && w.desktop.style !== "仪表" && (
                  <Row title="分组">
                    <Segments
                      label="分组"
                      items={opts(["卡片", "色条", "分隔线"])}
                      value={w.desktop.grouping}
                      onChange={(grouping) => setMonitor("desktop", { grouping })}
                    />
                  </Row>
                )}

                {kind === "desktop" ? (
                  <>
                    {(
                      [
                        ["topmost", "置顶显示"],
                        ["locked", "锁定位置"],
                        ["through", "鼠标穿透"],
                      ] as const
                    ).map(([key, label]) => (
                      <Row key={key} title={label}>
                        <Switch
                          state
                          label={label}
                          checked={w.desktop[key]}
                          onChange={(on) => setMonitor("desktop", { [key]: on })}
                        />
                      </Row>
                    ))}
                    <Row title="不透明度">
                      <Select
                        label="不透明度"
                        value={w.desktop.opacity}
                        onChange={(opacity) => setMonitor("desktop", { opacity })}
                        options={[100, 90, 80, 70, 60].map((value) => ({ value, label: value + "%" }))}
                      />
                    </Row>
                  </>
                ) : (
                  <>
                    <Row title="位置">
                      <Segments
                        label="位置"
                        value={w.taskbar.place}
                        onChange={(place) => setMonitor("taskbar", { place })}
                        items={opts(["托盘旁", "任务栏左侧"])}
                      />
                    </Row>
                    <Row title="显示器">
                      <Select
                        label="显示器"
                        value="primary"
                        onChange={() => {}}
                        options={[{ value: "primary", label: "主显示器" }]}
                      />
                    </Row>
                  </>
                )}
              </>
            )}
          </section>
        );
      })}
    </SectionPage>
  );
}
