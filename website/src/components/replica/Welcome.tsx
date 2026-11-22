/** @jsxImportSource @/components/replica/i18n */
"use client";
import { Fragment, useRef, useState, type ReactNode } from "react";
import { isLanguage, languages, localized, preferredLanguage, rememberLanguage, type Language } from "@/lib/languages";
import type { Motion } from "./Curve";
import Hundred from "./Hundred";
import { language, translate } from "./i18n";
import { Icon, type UiIcon } from "./icons";
import { appVersion } from "./model";
import { setWidgets, useShared } from "./store";
import { Button, FeatureCard, Segments, Select, slideIn, Switch, useIsoLayoutEffect, useRefreshRate } from "./widgets";

// The onboarding (welcome.rs `onboarding_dialog`), as the demo runs it: as
// administrator, with the privileged service and the kernel sensors on.
const steps = ["welcome", "display", "background", "update", "administrator", "privilege", "sensors", "taskManager", "tips"] as const;
type Step = (typeof steps)[number];

// A step's heading: its icon on an accent tile, the title and the introduction.
function Heading({ icon, title, body }: { icon: UiIcon; title: string; body: string }) {
  return (
    <div className="a-onboarding-heading">
      <span className="a-onboarding-glyph">
        <Icon name={icon} size={22} />
      </span>
      <h3 id="a-dialog-title">{title}</h3>
      <p>{body}</p>
    </div>
  );
}
// A setting of the step: what it is and a hint, its control on the right.
function Row({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <div className="a-onboarding-row">
      <div className="a-onboarding-text">
        <span>{title}</span>
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </div>
  );
}
// A state with a check once done, and what it means below.
function State({ text, hint, done, children }: { text: string; hint: string; done: boolean; children?: ReactNode }) {
  return (
    <div className="a-onboarding-row">
      <div className="a-onboarding-text">
        <span className="a-onboarding-state">
          {done && <Icon name="check" size={14} />}
          {text}
        </span>
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </div>
  );
}
const Note = ({ children, warning }: { children: ReactNode; warning?: boolean }) =>
  warning ? (
    <p className="a-onboarding-note is-warning">
      <Icon name="warning" size={14} />
      <span>{children}</span>
    </p>
  ) : (
    <p className="a-onboarding-note">{children}</p>
  );
const List = ({ items }: { items: ReactNode[] }) => (
  <ul className="a-onboarding-list">
    {items.map((item, i) => (
      <li key={i}>{item}</li>
    ))}
  </ul>
);
const Keycaps = ({ keys }: { keys: string[] }) => (
  <span className="a-keys" translate="no">
    {keys.map((key) => (
      <kbd key={key}>{key}</kbd>
    ))}
  </span>
);
// A sentence with keys in it: its placeholders become keycaps.
function Rich({ text, parts }: { text: string; parts: Record<string, ReactNode> }) {
  return (
    <>
      {translate(text)
        .split(/\{(\w+)\}/)
        .map((piece, i) => (i % 2 ? <Fragment key={i}>{parts[piece]}</Fragment> : <span key={i} translate="no">{piece}</span>))}
    </>
  );
}
const seconds = (ms: number) => `每 ${ms / 1000} 秒`;
const background = [500, 1000, 2000, 5000, 10000, 30000].map((ms) => ({ value: ms, label: seconds(ms) }));

export default function Welcome({
  tick,
  interval,
  navLeft,
  onNav,
  onInterval,
  motion,
  onMotion,
  uiMotion,
  onUiMotion,
  close,
}: {
  tick: number;
  interval: number;
  navLeft: boolean;
  onNav: (left: boolean) => void;
  onInterval: (v: number) => void;
  motion: Motion;
  onMotion: (m: Motion) => void;
  uiMotion: boolean;
  onUiMotion: (on: boolean) => void;
  close: () => void;
}) {
  const [step, setStep] = useState(0);
  const dir = useRef(0);
  const body = useRef<HTMLDivElement>(null);
  const { widgets } = useShared();
  const [values, setValues] = useState({ autostart: false, minimized: 5000, check: true, channel: "stable", route: "auto", taskManager: false });
  const set = <K extends keyof typeof values>(key: K, value: (typeof values)[K]) => setValues((v) => ({ ...v, [key]: value }));
  const [languageChoice, setLanguageChoice] = useState<Language | "system">(language);
  const hz = useRefreshRate();
  // A step slides 24 px in from the side it comes from while fading in.
  useIsoLayoutEffect(() => {
    if (dir.current) slideIn(body.current, { x: dir.current * 24, fade: true, duration: 250 });
    body.current?.parentElement?.scrollTo({ top: 0 });
  }, [step]);
  const go = (to: number) => {
    dir.current = Math.sign(to - step);
    setStep(to);
  };
  const last = step === steps.length - 1;
  const content: Record<Step, ReactNode> = {
    welcome: (
      <>
        <div className="a-onboarding-mark">
          <Hundred tick={tick} />
        </div>
        <h3 id="a-dialog-title" className="a-onboarding-title">
          欢迎使用 Argus
        </h3>
        <p className="a-onboarding-body">
          Argus 是一款现代的 Windows 任务管理器，可以查看进程、性能、服务、启动项与网络连接，并记录资源占用的历史。接下来用几步完成常用设置；每一步都可以跳过，之后也能在设置中修改。
        </p>
        <Row title="界面语言">
          <span className="a-onboarding-language">
            <Icon name="globe" />
            <Select
              label="界面语言"
              value={languageChoice}
              onChange={(value) => {
                if (value !== "system" && !isLanguage(value)) return;
                setLanguageChoice(value);
                const next = value === "system" ? preferredLanguage(navigator.languages) : value;
                rememberLanguage(next);
                if (next !== language()) location.assign(localized(next) + "#demo");
              }}
              options={[{ value: "system", label: "跟随系统" }, ...languages.map((item) => ({ value: item.id, label: item.name }))]}
            />
          </span>
        </Row>
      </>
    ),
    display: (
      <>
        <Heading icon="theme" title="显示与刷新" body="选择导航栏的位置、动画效果与数据的刷新频率。" />
        <Row title="导航栏位置" hint="侧边导航栏默认仅显示图标，可展开显示页面名称。">
          <Segments
            label="导航栏位置"
            value={navLeft ? "side" : "top"}
            onChange={(v) => onNav(v === "side")}
            items={[
              { value: "side", label: "侧边", icon: "navSide" },
              { value: "top", label: "顶部", icon: "navTop" },
            ]}
          />
        </Row>
        <Row title="界面动画" hint="切换页面、打开菜单和对话框时的过渡效果。">
          <Switch state label="界面动画" checked={uiMotion} onChange={onUiMotion} />
        </Row>
        <Row title="曲线动画" hint="帧率越高，曲线越流畅，资源占用也越高。">
          <Select
            label="曲线动画"
            value={motion}
            onChange={onMotion}
            options={[
              { value: "smooth", label: `流畅（${hz} 帧/秒）` },
              { value: "balanced", label: `均衡（${Math.round(hz / Math.max(1, Math.floor(hz / 60)))} 帧/秒）` },
              { value: "pixel", label: "省电（逐像素移动）" },
              { value: "step", label: "关闭（每次采样跳一格）" },
            ]}
          />
        </Row>
        <Row title="刷新频率" hint="间隔越短，数据越及时，Argus 自身的开销也越高。">
          <Select label="刷新频率" value={interval} onChange={onInterval} options={[500, 1000, 2000, 5000].map((ms) => ({ value: ms, label: seconds(ms) }))} />
        </Row>
      </>
    ),
    background: (
      <>
        <Heading icon="tray" title="常驻后台" body="Argus 可以常驻在托盘中，持续记录历史并执行规则，需要时立即打开。" />
        <Row title="登录 Windows 时启动" hint="在托盘中静默启动，不显示窗口。">
          <Switch state label="登录 Windows 时启动" checked={values.autostart} onChange={(v) => set("autostart", v)} />
        </Row>
        <Row title="关闭窗口时保留在托盘" hint="关闭此项后，关闭窗口即退出 Argus。">
          <Switch state label="关闭窗口时保留在托盘" checked={widgets.closeToTray} onChange={(closeToTray) => setWidgets({ closeToTray })} />
        </Row>
        <Row title="最小化时的刷新间隔" hint="默认 5 秒；更短的间隔会增加后台开销。">
          <Select label="最小化时的刷新间隔" value={values.minimized} onChange={(v) => set("minimized", v)} options={background} />
        </Row>
        <Row title="后台常驻时的刷新间隔" hint="长于 5 秒时，历史记录随之变稀。">
          <Select label="后台常驻时的刷新间隔" value={widgets.slow} onChange={(slow) => setWidgets({ slow })} options={background} />
        </Row>
        <p className="a-onboarding-note is-rich">
          <Rich text="按 {hotkey} 可随时显示或隐藏 Argus，可在设置中更改。" parts={{ hotkey: <Keycaps keys={["Ctrl", "Alt", "Esc"]} /> }} />
        </p>
      </>
    ),
    update: (
      <>
        <Heading icon="update" title="保持最新" body="Argus 从 GitHub Releases 获取新版本。安装包须通过签名校验，安装前需手动确认。" />
        <Row title="自动检查更新" hint="启动约 1 分钟后检查一次，此后每天检查一次。">
          <Switch state label="自动检查更新" checked={values.check} onChange={(v) => set("check", v)} />
        </Row>
        <Row title="更新通道" hint={values.channel === "beta" ? "测试版同时接收预发布版本，提前提供新功能，稳定性可能不及稳定版。" : undefined}>
          <Segments
            label="更新通道"
            value={values.channel}
            onChange={(v) => set("channel", v)}
            items={[
              { value: "stable", label: "稳定版" },
              { value: "beta", label: "测试版" },
            ]}
          />
        </Row>
        <Row title="下载线路">
          <Select
            label="下载线路"
            value={values.route}
            onChange={(v) => set("route", v)}
            options={[
              { value: "auto", label: "自动" },
              { value: "direct", label: "仅连接 GitHub" },
              { value: "proxy", label: "仅使用加速代理" },
            ]}
          />
        </Row>
        <Note>加速代理由第三方提供，可获知本机的 IP 地址及所下载的文件；安装包须通过签名与版本号校验。</Note>
      </>
    ),
    administrator: (
      <>
        <Heading
          icon="shield"
          title="管理员运行"
          body="可允许 Argus 自动以管理员身份运行，以管理其他账户的进程与启动项。默认关闭；首次开启时由 Windows 请求授权。"
        />
        <FeatureCard
          icon="shield"
          title="管理员运行"
          desc="授权一次后，登录启动、快捷键及任务管理器入口将以管理员身份运行。"
          state="已授权"
          tone="ok"
          note={
            <>
              <span>当前进程：管理员权限</span>
              <span>授权后可继续完成引导，结束引导时重新启动并应用权限。</span>
            </>
          }
          controls={<Switch state label="允许以管理员身份运行" checked onChange={() => {}} />}
        />
      </>
    ),
    privilege: (
      <>
        <Heading
          icon="shield"
          title="特权服务（可选）"
          body="Argus 默认以标准权限运行，部分系统进程的信息无法读取，也无法结束或调整。可选的特权服务在后台以 SYSTEM 身份运行，安装后可以："
        />
        <List
          items={[
            "结束、暂停或恢复其他用户的进程（系统关键进程除外）",
            "设置进程优先级与效率模式，并由规则自动应用",
            "查询所有进程的文件占用，采集每进程网络流量与磁盘读写",
            "控制 Windows 服务，断开网络连接",
            "为内核传感器提供采集通道（驱动另行启用）",
          ]}
        />
        <Note warning>
          授权属于当前 Windows 账户：此账户下的任何程序都能调用这些功能，相当于获得部分管理员权限。请只在自己使用的账户上安装。
        </Note>
        <Note>Argus 当前以管理员身份运行，进程管理操作无需特权服务；内核传感器和任务管理器替换仍需安装服务。</Note>
        <State text={`已连接 · ${appVersion}`} hint="接下来可以选择内核传感器和系统任务管理器替换。" done />
      </>
    ),
    sensors: (
      <>
        <Heading icon="bolt" title="内核传感器（可选）" body="读取处理器内部传感器，补充温度与功耗数据。" />
        <Note>需要安装或启动 PawnIO 时，将请求管理员授权。</Note>
        <State text="已开启" hint="可在性能页查看读数，在“设置 → 权限”中关闭；关闭不卸载共享驱动。" done />
      </>
    ),
    taskManager: (
      <>
        <Heading
          icon="keyboard"
          title="替换系统任务管理器（可选）"
          body="开启后，按 Ctrl+Shift+Esc、在任务栏右键菜单中选择“任务管理器”等打开任务管理器的方式，都会打开 Argus。"
        />
        <Note>此更改作用于整台电脑，其他账户仍打开 Windows 任务管理器；Argus 无法启动时改为打开 Windows 任务管理器。</Note>
        <Note>可随时在“设置 → 权限”中恢复；界面或服务无法运行时，还可使用独立的恢复程序。</Note>
        <State
          text={values.taskManager ? "已替换为 Argus" : "使用 Windows 任务管理器"}
          hint={values.taskManager ? "任务管理器入口已交给 Argus；启动失败时尝试打开原生任务管理器。" : "开启需要管理员授权；也可跳过，之后在“设置 → 权限”中开启。"}
          done={values.taskManager}
        >
          {!values.taskManager && <Button onClick={() => set("taskManager", true)}>启用替换</Button>}
        </State>
      </>
    ),
    tips: (
      <>
        <Heading icon="sparkle" title="准备就绪" body="以下操作可以帮助更快地使用 Argus：" />
        <List
          items={[
            <Rich
              key="pages"
              text="{keys} 切换页面，{search} 搜索进程"
              parts={{
                keys: (
                  <span className="a-onboarding-keys">
                    <Keycaps keys={["Ctrl", "1"]} />–<Keycaps keys={["7"]} />
                  </span>
                ),
                search: <Keycaps keys={["Ctrl", "F"]} />,
              }}
            />,
            "右键单击进程，可结束进程、调整优先级、开启效率模式或添加规则",
            "将文件或文件夹拖放到窗口，查找占用它的进程",
            "历史页记录资源占用以及当时最忙的进程",
          ]}
        />
        <Note>可随时在“关于”页重新打开本引导。</Note>
      </>
    ),
  };
  return (
    <div className="a-dialog is-onboarding" role="dialog" aria-modal="true" aria-labelledby="a-dialog-title" tabIndex={-1}>
      <div className="a-dialog-body a-scroll">
        <div ref={body} key={step} className="a-onboarding-step">
          {content[steps[step]]}
        </div>
      </div>
      <div className="a-dialog-foot">
        <span className="a-onboarding-dots" aria-hidden="true">
          {steps.map((s, i) => (
            <i key={s} className={i === step ? "is-current" : undefined} />
          ))}
        </span>
        {!last && (
          <Button tone="ghost" onClick={close}>
            跳过
          </Button>
        )}
        {step > 0 && (
          <Button wide onClick={() => go(step - 1)}>
            上一步
          </Button>
        )}
        <button type="button" className="a-btn is-accent is-wide" data-primary onClick={() => (last ? close() : go(step + 1))}>
          {last ? "开始使用" : "下一步"}
        </button>
      </div>
    </div>
  );
}
