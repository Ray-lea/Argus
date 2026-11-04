# 官网开发

页面、组件和发布入口见 [README](README.md)。

- 视觉方向以用户确认的方案和应用自身的视觉体系（`resources/theme.json`：字体、强调色、材质、描边、数据曲线颜色）为准。不引入外部设计规则库或设计类技能，用户指定的参考站与视觉取舍记录在当次复核中。
- 改动页面或文档站的视觉与交互后，构建并预览，再运行 `pnpm --dir website check:browser <预览地址>`。它在 1440、768 和 390 px 检查主题、首页界面演示、文档导航与搜索，截图保存在 `target/official-web/review/`；逐张查看截图后再报告。
- 首页界面演示（`src/components/replica/`）沿用应用的文案、列、交互和主题令牌；应用的界面或文案变化较大时同步更新演示。演示数据使用虚构值和文档地址段（RFC 5737/3849）。
- 演示的文字以中文写在组件里，英文首页在创建元素时翻译（`replica/i18n/`：演示文件用 `jsxImportSource` 指向它，文本子节点和 title、aria-label 等属性按 `en.ts` 查表，带数值的消息用模式）。新增或修改演示文案时在 `en.ts` 补英文，与应用同义的文字取 `locales/en-US` 的译文；同一个词在不同位置译法不同时给元素加 `data-i18n` 上下文。改完在无头浏览器里把英文首页的导览走一遍，读取页面上的 `globalThis.__argusMissing`，确认没有漏译的中文。
- 首页的演示桌面按 1920×1080 布局、按页面宽度等比缩放，任务栏按 Windows 11 的 48 px 样式绘制；窗口默认 1600×900，居中于任务栏以上的 1920×1032，可拖动边与角调整大小（不小于应用的最小窗口 `resources/app.json` 的 `window.minWidth`、`window.minHeight`，现为 1240×800），也可最大化（`src/components/home/DeskWindow.tsx`）；桌面下方的“全屏预览”让浏览器全屏显示整个桌面。改了应用的最小窗口，要同步 `DeskWindow.tsx` 中的 `MIN`。演示以管理员身份运行、特权服务与内核传感器均已开启，以展示应用能显示的全部数据。演示里读取位置与尺寸的代码用 `widgets.tsx` 的 `localRect`、`scaleOf` 换算成窗口自身的像素。桌面下方的导览（`src/components/home/HeroTour.tsx`）按章节逐页演示菜单、对话框与托盘，访客可选章节；步骤按 `data-tour`、按钮标题或文案定位元素，找不到的步骤会被跳过而不报错，因此改了演示窗口的结构或文案后，要在无头浏览器里把每个章节走一遍，确认每一步都找到了目标。
- 用户的 `start-website.bat` 通常已在运行。代理自测优先用 `start-website.bat build` 加 `preview --port <端口>`：构建写入的 `website/content/` 与开发服务器相同，不影响用户的实例。需要第二个开发服务器时，设 `ARGUS_NEXT_DIR=.next-<名称>` 并换端口，测完删除该目录，并还原 Next.js 写入 `tsconfig.json` 的对应 `include` 项。
- 文档的外观与交互以 fumadocs.dev 为准：沿用它的 Spacious 布局和 Fumadocs 组件，不自行设计布局；`src/styles/docs.css` 只换强调色、字体和中文排版细节。侧栏图标在 `scripts/content.mjs` 的 `pageIcons` 中按文件指定，并在 `src/lib/source.ts` 中逐个引入（整包引入 lucide 会拖慢开发服务器的首次编译）。首页样式位于 `site` 层，类名不要与 Tailwind 工具类同名（如 `grid`、`table`），否则会作用到 Fumadocs 的元素上。
- 动效：页面切换由 `SiteFrame` 与文档布局外层的 React `<ViewTransition>` 负责（样式在 `global.css`），文档内部翻页保持 fumadocs.dev 的方式，不加过渡。可展开的 `<details>` 加 `data-disclose`，由 `Disclosure.tsx` 统一做高度动画，不依赖只有 Chromium 支持的 `interpolate-size`。演示曲线的滚动按 `Curve.tsx` 的采样时钟（`advanceClock`）进行：新采样从上一个采样的起点顺延一个间隔，不要改回按定时器触发的时刻计时，否则定时器晚到时曲线会往回跳。
- 性能：循环播放的装饰动画只改 transform 与 opacity，交给合成线程；所在区块加 `data-motion`，并把选择器加入 `global.css` 中 `[data-still]` 的暂停列表，区块滚出视口后由 `MotionGate` 暂停。不要把持续过渡或动画的图层放进带圆角的 `overflow: hidden` 裁切里，浏览器会每帧重建图层树。首屏以下的首页区块用 `content-visibility: auto` 延后布局与绘制。只属于首页或内页的样式写进 `home.css`、`replica.css` 或 `pages.css`（经路由样式加载），几页共用的才放 `global.css`；演示窗口里首屏用不到的页面按需加载。改动可能影响流畅度或加载时，在无头浏览器里与改动前的构建交替测量主线程每秒耗时、最长帧，以及 CPU 节流 4 倍时的总阻塞时间。
