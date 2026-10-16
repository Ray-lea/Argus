# 全局快捷键

显示或隐藏 Argus 的全局快捷键由 `src-tauri/src/hotkey.rs` 在主窗口上调用 Windows 的 `RegisterHotKey` 注册，录制在设置页完成（`ui/src/lib/hotkey.ts` 的 `record`）。可用的键和不允许的组合写在 `ui/src/lib/hotkey-table.json`，原生侧与设置页共用，两边的测试读取同一份用例（`ui/tests/hotkey-cases.json`）。用法见[设置](../guide/settings.md#快捷键)。

## RegisterHotKey 的行为

以下结论在 Windows 11（26300）上实测得到：用 `SendInput` 注入带标记的按键，以 F20–F23 作主键，同一线程上的低级键盘钩子记录系统看到的事件。

| 情形 | 结果 |
|---|---|
| 先按住 CapsLock，或 CapsLock 处于开启状态，再按组合 | 照常触发 |
| 先按主键、后按修饰键 | 不触发：修饰键集合在主键按下的那一刻比较 |
| 以 `VK_RCONTROL` 为主键 | 从不触发；以 `VK_CONTROL` 为主键时，单按任一侧 Ctrl 就触发 |
| 左 Ctrl、右 Ctrl、左 Alt 依次按下（仅修饰键） | 只有 `Ctrl+Alt+VK_MENU` 触发，且在 Alt 按下的瞬间，会抢在所有 Ctrl+Alt+字母之前 |
| 注册 Ctrl+CapsLock | 必须先按 Ctrl；被热键吃掉的 CapsLock 仍会切换大写锁定 |
| 主键已经按住时才注册，随后出现一次自动重复 | 触发，带 `MOD_NOREPEAT` 也一样（不带时每次重复都触发） |
| 低级键盘钩子吞掉主键 | 已注册的热键不触发：钩子先于热键处理 |

另外，`Shift+A`、`Ctrl+C`、`Ctrl+1`、`Alt+Space`、`Alt+PrintScreen` 都能注册成功；Alt+F4、Alt+Tab、Ctrl+Esc、Ctrl+Shift+Esc、无修饰键的 F12 和大多数 Win+字母返回 `ERROR_HOTKEY_ALREADY_REGISTERED`。

## 由此做出的决定

- **只用 RegisterHotKey**：它不区分左右、不支持仅修饰键或 CapsLock 作修饰键，也不支持两个普通键的组合（只匹配修饰键加一个键），但没有常驻开销，也不让每一次按键经过 Argus。低级键盘钩子能做到这些，代价是全系统的每次按键都要等 Argus 的回调，回调超过 `LowLevelHooksTimeout` 时会被系统静默移除，前台为管理员窗口时收不到按键，也容易被安全软件当作键盘记录。
- **规则由 Argus 拦截**：系统接受的组合里有会破坏正常使用的（只含 Shift 的组合让所有程序无法输入大写字母，Ctrl+C 让复制失效），所以新组合须含 Ctrl、Alt 或 Win（F13–F24 例外，Shift+F1–F9、Shift+F11 需确认），并拒绝表中的保留与编辑组合和锁定键。带 Ctrl 的 Pause 组合也拒绝：按住 Ctrl 时 Pause 键发出 Break，Windows 报为 `VK_CANCEL`，注册在 `VK_PAUSE` 上的热键不会触发。已保存的旧值启动时照常注册，规则收紧不会让已有的快捷键失效。
- **主键松开时才保存**：按下时保存，快捷键就在主键还按着时注册，按住稍久就会被自动重复触发，窗口当场隐藏。
- **录制期间暂停当前快捷键**：注册着的组合到不了页面，再按一次当前快捷键只会隐藏窗口。窗口隐藏或 60 秒后，原生侧自动恢复。
- **按虚拟键码录制**：录制器按页面的 `keyCode`（Chromium 在 Windows 上给出的虚拟键码）查表，注册用的也是虚拟键码；只有输入法处理中的按键（229）和没有键码的事件按物理位置（`code`）查表。表中没有的键码（例如 `VK_OEM_8`）不能使用，不按物理位置换成美式键盘上的另一个键。
- **页面收不到的按键**：被其他程序注册的组合，主键按下不会到达页面；只按着修饰键 1.5 秒仍没有主键时，设置页提示该组合可能已被占用。PrintScreen 在 Windows 上只有 keyup，修饰键按住时按 keyup 补录。
- **更改是事务**：先注册新组合，保存成功后再注销旧组合；保存失败时撤销新组合。
