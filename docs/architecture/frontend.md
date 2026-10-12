# 前端

`ui/` 是 SolidJS + TypeScript + Tailwind v4 的单页界面，由 Vite 构建。不在 Tauri 中运行时（`start.bat ui`），`lib/ipc.ts` 改用 `lib/mock.ts` 的模拟数据，方便在浏览器里调样式；模拟数据不会进入 Tauri 构建的运行路径。

## 目录

| 路径 | 内容 |
|---|---|
| `src/App.tsx` | 外壳：页面切换与过渡、全局键盘快捷键、文件拖放 |
| `src/components/` | 页面：`ProcessTable`、`Performance`、`ServicesPage`、`StartupPage`、`ConnectionsPage`、`HistoryPage`、`Settings`。外壳与通用组件：`Toolbar`、`Vitals`（概况带）、`StatusBar`、`DetailsPanel`、`DataTable`（虚拟化表格）、`Trace`（实时曲线）、`Overlays`（右键菜单及一级子菜单、确认框、提示）、`FileLockDialog` |
| `src/lib/store.ts` | 应用状态：实时快照、滚动历史、视图偏好 |
| `src/lib/ipc.ts` | 与宿主通信 |
| `src/lib/rows.ts` | 把进程快照整理成三种视图要渲染的行 |
| `src/lib/motion.ts` | 动画偏好（界面动画、曲线动画四档）、屏幕刷新率测量、Fluent 缓动曲线与进出场辅助 |
| `src/lib/actions.ts` | 面向用户的进程操作：确认、执行、反馈 |
| `src/lib/prefs.ts` | 存在 localStorage 的偏好（按 WebView 配置隔离，开发版与正式版各一份） |
| `src/lib/types.ts` | 与 `argus-core` 的 serde 类型一一对应，需要保持同步 |
| `src/styles.css`、`pages.css`、`tables.css` | 样式与设计令牌 |

## 历史曲线

`components/HistoryPage.tsx` 的曲线是静态的：按查询（切换范围、窗口宽度停止变化后、跟随最新数据时每 10 秒到 5 分钟）各画一次，悬停只移动光标元素。每次查询按图表宽度约每两个像素一个点分桶，点与点之间远超分桶间隔的地方是 Argus 没有运行的时段，留出断档。

## 实时曲线

`components/Trace.tsx` 在 canvas 上绘制曲线：每次采样只画一次，之后由合成器沿时间轴连续移动画布，主线程不参与。实现要点见[曲线动画](../design/charts.md)。
