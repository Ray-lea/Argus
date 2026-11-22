/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useState } from "react";
import { Icon } from "./icons";
import { Button, Select } from "./widgets";
import { store, useShared } from "./store";

// The history's cards between "记录历史" and "已保存的历史" (history/settings.rs):
// what is recorded, how long and how much is kept, how older days are merged,
// and where the records are kept.
const groups = [
  "处理器",
  "传感器",
  "内存细分",
  "系统计数",
  "每块磁盘",
  "每个网络适配器",
  "每块显卡",
  "电池细分",
  "进程：磁盘读写",
  "进程：网络",
  "进程：显存与计数",
];
// Read through the privileged service under standard rights; the demo runs as
// administrator, so the service's part is ready.
const byService = new Set(["传感器", "进程：网络"]);
const description: Record<string, string> = {
  系统读数: "CPU、内存、磁盘、网络、GPU 与电池的总量",
  处理器: "内核时间、频率、封装功耗",
  传感器: "CPU 温度、核心电压、风扇转速与各设备的温度",
  内存细分: "已提交、缓存、压缩、分页与非分页缓冲池",
  系统计数: "进程数、线程数、句柄数",
  每块磁盘: "读写速率、活动时间、平均响应时间",
  每个网络适配器: "接收与发送速率",
  每块显卡: "使用率、专用显存、功耗",
  电池细分: "电压与温度",
  "进程：磁盘读写": "每个记录的进程读取与写入磁盘的速率",
  "进程：网络": "每个记录的进程的发送与接收速率",
  "进程：显存与计数": "专用显存、线程数、句柄数",
};
const details = ["保留全部 5 秒记录", "7 天后合并为每分钟", "30 天后合并为每分钟"];
const defaultPlace = "%LOCALAPPDATA%\\dev.argus.taskmanager\\history";
// What a day of records takes now (MB), and a merged day beside it (12.8 %).
const PER_DAY = 2.4,
  MERGED = 0.13;
const kept = (days: number, merge: number | null) =>
  merge === null ? days * PER_DAY : Math.min(days, merge) * PER_DAY + Math.max(0, days - merge) * PER_DAY * MERGED;
const mb = (value: number) => (value >= 1024 ? (value / 1024).toFixed(1) + " GB" : value.toFixed(1) + " MB");
const Badge = ({ children, service }: { children: string; service?: boolean }) => (
  <span className="h-record-badge">
    {service && <Icon name="shieldCheck" size={12} />}
    {children}
  </span>
);

export default function HistorySettings({ onSay }: { onSay: (text: string) => void }) {
  const { recording: r } = useShared();
  const save = (patch: Partial<typeof r>) => store.set({ recording: { ...store.get().recording, ...patch } });
  const [location, setLocation] = useState<string | null>(null);
  const merge = r.detail === details[0] ? null : r.detail === details[2] ? 30 : 7;
  // How many days the limits keep at today's rate, and which one sets it.
  let fit = 0;
  if (r.size) while (fit < 3650 && kept(fit + 1, merge) <= r.size) fit++;
  const days = r.days && r.size ? Math.min(r.days, fit) : r.days || fit;
  const bySpace = !!r.size && (!r.days || fit < r.days);
  return (
    <>
      <div className="a-setting w-setting is-wide h-record-card">
        <Icon name="layers" size={20} />
        <div className="a-setting-text">
          <div>记录内容</div>
          <span>需要特权服务的读数在服务可用后开始记录。</span>
        </div>
        <div className="h-record-options">
          <label>
            <input type="checkbox" checked disabled />
            <span>
              <span className="h-record-name">
                系统读数 <Badge>始终记录</Badge>
              </span>
              <small>{description["系统读数"]}</small>
            </span>
          </label>
          {groups.map((g) => (
            <label key={g}>
              <input
                type="checkbox"
                checked={!r.off.includes(g)}
                onChange={(e) => save({ off: e.target.checked ? r.off.filter((k) => k !== g) : [...r.off, g] })}
              />
              <span>
                <span className="h-record-name">
                  {g}
                  {byService.has(g) && <Badge service>由特权服务提供</Badge>}
                </span>
                <small>{description[g]}</small>
              </span>
            </label>
          ))}
        </div>
        <div className="h-record-count">
          <span>
            每帧记录的进程<small>进程越多，区间排行越完整，记录也越大</small>
          </span>
          <Select
            label="每帧记录的进程"
            value={r.count}
            onChange={(count) => save({ count })}
            options={[
              { value: 20, label: "约 20 个（默认）" },
              { value: 40, label: "约 40 个" },
              { value: 0, label: "全部活跃进程" },
            ]}
          />
        </div>
        <p className="h-record-estimate">{`按当前选择，连续运行每天约占用 ${mb(PER_DAY * (r.count === 40 ? 1.6 : r.count === 0 ? 2.5 : 1))}。`}</p>
      </div>
      <div className="a-setting">
        <Icon name="calendarRange" />
        <div className="a-setting-text">
          <div>保留上限</div>
          {!r.days && !r.size ? (
            <span className="is-error">两项都不限时，历史记录会持续增长。</span>
          ) : (
            <>
              <span>两项都设置时，先达到的上限生效；超出时按日期从早到晚删除。</span>
              <span>{`按当前记录内容约可保存 ${days} 天（${bySpace ? "受空间上限限制" : "受时间上限限制"}，约 ${mb(kept(days, merge))}）。`}</span>
            </>
          )}
        </div>
        <div className="a-setting-controls">
          <small className="h-limit-caption">时间</small>
          <Select
            label="时间"
            value={r.days}
            onChange={(days) => save({ days })}
            options={[7, 14, 30, 90, 180, 365, 0].map((value) => ({
              value,
              label: !value ? "不限" : value === 365 ? "1 年" : value + " 天",
            }))}
          />
          <small className="h-limit-caption">空间</small>
          <Select
            label="空间"
            value={r.size}
            onChange={(size) => save({ size })}
            options={[256, 512, 1024, 2048, 5120, 10240, 0].map((value) => ({
              value,
              label: !value ? "不限" : value < 1024 ? value + " MB" : value / 1024 + " GB",
            }))}
          />
        </div>
      </div>
      <div className="a-setting">
        <Icon name="chartLine" />
        <div className="a-setting-text">
          <div>较早记录的精度</div>
          {/* Keeping every record needs no words beyond the option's own name. */}
          {merge !== null && <span>合并后保留每分钟的平均值、最高值与占用最高的进程，约为原大小的八分之一。</span>}
        </div>
        <div className="a-setting-controls">
          <Select
            label="较早记录的精度"
            value={r.detail}
            options={details.map((value) => ({ value, label: value }))}
            onChange={(detail) => save({ detail })}
          />
        </div>
      </div>
      <div className="a-setting">
        <Icon name="folder" />
        <div className="a-setting-text">
          <div>保存位置</div>
          <span className="h-place">
            <span translate="no">{r.location}</span>
            {r.location === defaultPlace && <span className="h-place-default">（默认）</span>}
          </span>
        </div>
        <div className="a-setting-controls">
          {location !== null ? (
            <>
              <input
                className="a-input"
                aria-label="新位置"
                autoFocus
                value={location}
                onChange={(e) => setLocation(e.target.value)}
              />
              <Button
                disabled={!location.trim()}
                onClick={() => {
                  save({ location: location.trim() });
                  setLocation(null);
                }}
              >
                更改位置
              </Button>
              <Button onClick={() => setLocation(null)}>取消</Button>
            </>
          ) : (
            <>
              <Button icon="folderOpen" onClick={() => onSay("演示窗口不会打开文件资源管理器。")}>
                打开
              </Button>
              <Button onClick={() => setLocation(r.location)}>更改…</Button>
              {r.location !== defaultPlace && <Button onClick={() => save({ location: defaultPlace })}>恢复默认</Button>}
            </>
          )}
        </div>
      </div>
    </>
  );
}
