# 全局快捷键

Windows 宿主在主窗口上调用 `RegisterHotKey` 注册显示或隐藏窗口的组合，通过 `WM_HOTKEY` 接收通知。宿主和设置页共用[键位与保留组合表](../../resources/hotkey-table.json)和[校验用例](../../src-gpui/tests/hotkey-cases.json)。操作方法见[设置](../guide/settings.md#快捷键)。

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

## 录制与注册

- 设置页将 GPUI 的按键名称、当前布局字符和修饰键交给 [`Recording`](../../src-gpui/src/frontend/recorder.rs)，映射为共享键表中的主键。左右修饰键共用标记，修饰键先于主键按下，松开主键时提交组合。
- 录制时暂停原快捷键并忽略自动重复。仅按住修饰键 1.5 秒时提示主键可能已被其他程序占用。结束或取消录制时恢复注册，窗口隐藏或暂停达到 60 秒时宿主也会恢复。
- 新组合通常须含 Ctrl、Alt 或 Win。F13–F24 可单独使用，部分 Shift 加功能键的组合需确认。设置页拦截系统保留键、锁定键和常用编辑组合。Ctrl 加 Pause 会产生 Break 的虚拟键码，录制器拒绝这一组合。
- [`hotkey.rs`](../../src-gpui/src/hotkey.rs) 把共享键表中的主键转换为 Windows 虚拟键码，以 `MOD_NOREPEAT` 注册。更改时先注册新组合，写入设置成功后释放旧组合，写入失败则撤销新注册。启动时从设置恢复已保存的组合。
