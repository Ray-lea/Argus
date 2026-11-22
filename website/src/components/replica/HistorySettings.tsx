/** @jsxImportSource @/components/replica/i18n */
"use client";
import { useState } from "react";
import { Icon } from "./icons";
import { Button, Select } from "./widgets";
import { store, useShared } from "./store";

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
export default function HistorySettings() {
  const { recording: r } = useShared();
  const save = (patch: Partial<typeof r>) => store.set({ recording: { ...store.get().recording, ...patch } });
  const [location, setLocation] = useState<string | null>(null);
  return (
    <>
      <div className="a-setting w-setting is-wide h-record-card">
        <Icon name="storage" size={20} />
        <div className="a-setting-text">
          <div>记录内容</div>
          <span>需要特权服务的读数在服务可用后开始记录。</span>
        </div>
        <div className="h-record-options">
          <label>
            <input type="checkbox" checked disabled />
            <span>
              系统读数 <small>始终记录</small>
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
                {g}
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
      </div>
      <div className="a-setting">
        <div className="a-setting-text">
          <div>保留上限</div>
          <span>两项都设置时，先达到的上限生效；超出时按日期从早到晚删除。</span>
        </div>
        <Select
          label="时间"
          value={r.days}
          onChange={(days) => save({ days })}
          options={[7, 14, 30, 90, 180, 365, 0].map((value) => ({ value, label: value ? value + " 天" : "不限" }))}
        />
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
      <div className="a-setting">
        <div className="a-setting-text">
          <div>较早记录的精度</div>
          <span>合并后保留每分钟的平均值、最高值与占用最高的进程，约为原大小的八分之一。</span>
        </div>
        <Select
          label="较早记录的精度"
          value={r.detail}
          options={details.map((value) => ({ value, label: value }))}
          onChange={(detail) => save({ detail })}
        />
      </div>
      <div className="a-setting">
        <div className="a-setting-text">
          <div>保存位置</div>
          <span>{r.location}</span>
        </div>
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
            <Button onClick={() => setLocation(r.location)}>更改…</Button>
            <Button onClick={() => save({ location: "%LOCALAPPDATA%\\dev.argus.taskmanager\\history" })}>
              恢复默认
            </Button>
          </>
        )}
      </div>
    </>
  );
}
