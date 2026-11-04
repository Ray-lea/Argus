# 官网开发

页面、组件和发布入口见 [README](README.md)。

- 视觉方向以用户确认的方案和应用自身的视觉体系（`resources/theme.json`：字体、强调色、材质、描边、数据曲线颜色）为准。不引入外部设计规则库或设计类技能，用户指定的参考站与视觉取舍记录在当次复核中。
- 改动页面或文档站的视觉与交互后，构建并预览，再运行 `pnpm --dir website check:browser <预览地址>`。它在 1440、768 和 390 px 检查主题、首页界面演示、文档导航与搜索，截图保存在 `target/official-web/review/`；逐张查看截图后再报告。
- 首页界面演示（`src/components/replica/`）沿用应用的文案、列、交互和主题令牌；应用的界面或文案变化较大时同步更新演示。演示数据使用虚构值和文档地址段（RFC 5737/3849）。
- 首页的演示桌面按 1920×1080 布局、按页面宽度等比缩放，任务栏按 Windows 11 的 48 px 样式绘制；窗口默认 1600×900，居中于任务栏以上的 1920×1032，可拖动边与角调整大小（不小于应用的最小窗口 `resources/app.json` 的 `window.minWidth`、`window.minHeight`，现为 1240×800），也可最大化（`src/components/home/DeskWindow.tsx`）；桌面下方的“全屏预览”让浏览器全屏显示整个桌面。改了应用的最小窗口，要同步 `DeskWindow.tsx` 中的 `MIN`。演示以管理员身份运行、特权服务与内核传感器均已开启，以展示应用能显示的全部数据。演示里读取位置与尺寸的代码用 `widgets.tsx` 的 `localRect`、`scaleOf` 换算成窗口自身的像素。桌面下方的导览（`src/components/home/HeroTour.tsx`）按章节逐页演示菜单、对话框与托盘，访客可选章节；步骤按 `data-tour`、按钮标题或文案定位元素，找不到的步骤会被跳过而不报错，因此改了演示窗口的结构或文案后，要在无头浏览器里把每个章节走一遍，确认每一步都找到了目标。
- 用户的 `start-website.bat` 通常已在运行。代理自测优先用 `start-website.bat build` 加 `preview --port <端口>`：构建写入的 `website/content/` 与开发服务器相同，不影响用户的实例。需要第二个开发服务器时，设 `ARGUS_NEXT_DIR=.next-<名称>` 并换端口，测完删除该目录，并还原 Next.js 写入 `tsconfig.json` 的对应 `include` 项。
- 文档的外观与交互以 fumadocs.dev 为准：沿用它的 Spacious 布局和 Fumadocs 组件，不自行设计布局；`src/styles/docs.css` 只换强调色、字体和中文排版细节。侧栏图标在 `scripts/content.mjs` 的 `pageIcons` 中按文件指定，并在 `src/lib/source.ts` 中逐个引入（整包引入 lucide 会拖慢开发服务器的首次编译）。首页样式位于 `site` 层，类名不要与 Tailwind 工具类同名（如 `grid`、`table`），否则会作用到 Fumadocs 的元素上。
