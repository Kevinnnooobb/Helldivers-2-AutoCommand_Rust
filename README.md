# H2AC-RS

**Helldivers 2 Auto Stratagem Caller — Rust Rewrite**

> 绝地潜兵2 自动战备呼叫终端 — 沉浸式 HUD 风格桌面工具

一个 Windows 桌面应用，帮助你在《绝地潜兵2》中快速配置、切换和执行战备指令。通过模拟键盘输入，一键呼叫轨道火力、飞鹰空袭、支援武器等 75+ 种战备。

---

![主界面](screenshots/main.png)
![紧凑模式](screenshots/compact.png)

> 截图通过以下脚本捕获运行中的窗口：
> ```powershell
> powershell -Command "Add-Type -AssemblyName System.Drawing; $src = @'
> using System; using System.Runtime.InteropServices;
> namespace W {
>     public struct RECT { public int Left, Top, Right, Bottom; }
>     public static class U32 {
>         [DllImport(\"user32.dll\")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
>         [DllImport(\"user32.dll\")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
>     }
> }
> '@; Add-Type -TypeDefinition $src -ReferencedAssemblies System.Drawing;
> $p = Get-Process h2ac-rs; $r = New-Object W.RECT;
> [W.U32]::GetWindowRect($p.MainWindowHandle, [ref] $r) | Out-Null;
> $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top;
> $bmp = New-Object System.Drawing.Bitmap $w, $h;
> $g = [System.Drawing.Graphics]::FromImage($bmp); $dc = $g.GetHdc();
> [W.U32]::PrintWindow($p.MainWindowHandle, $dc, 2) | Out-Null;
> $g.ReleaseHdc($dc); $g.Dispose();
> $bmp.Save('screenshots/main.png'); $bmp.Dispose(); echo \"shot ${w}x${h}\""
> ```

---

## 功能

- **战备数据库** — 内置 75+ 条战备，覆盖 8 个分类（任务、轨道火力、飞鹰、支援武器、哨戒炮、雷盾、背包、载具）
- **拖放式配装** — 10 个可配置槽位，点击槽位进入待命状态，再点战备库完成装入，自动推进下一空槽
- **分类筛选 + 搜索** — 左侧分类栏切换类别，搜索框支持中文名/型号子串跨分类实时过滤
- **双击执行** — 双击已装入的槽位，通过 `SendInput` API 模拟游戏内方向键序列，精确输入战备指令
- **全局热键** — 为槽位绑定单个按键（通过 `WH_KEYBOARD_LL` 底层键盘钩子），在任何窗口下按热键即刻执行
- **Profile 管理** — 保存/加载/删除多套配装方案（JSON 文件），快速切换不同任务配置
- **双模式切换**
  - **主界面** (1100×640) — 完整战术终端：顶栏、槽位网格、详情面板、战备库、日志栏
  - **紧凑模式** (554×56) — 无边框悬浮迷你条，始终置顶，点击图标即执行
- **按键设置** — 自定义方向键映射（WASD / 方向键）和战备激活键（Ctrl / 自定义）
- **可调延迟** — 按键间隔从 0.01s 到 0.5s，适配不同的游戏指令接收速度
- **执行闪光** — 槽位执行时金色闪光动画（0.7s 衰减），确认指令已发送
- **监听开关** — 顶部状态灯一键启停全局热键监听，带呼吸脉冲动画
- **无边框窗口** — 自定义标题栏，可拖拽移动，关闭/最小化/紧凑切换控件

---

## 安装

### 从源码构建

**前置需求：**
- [Rust](https://www.rust-lang.org/tools/install) 1.75+
- Windows 10/11 64-bit

```bash
git clone https://github.com/your-username/h2ac-rs.git
cd h2ac-rs
cargo build --release
```

编译产物 `target/release/h2ac-rs.exe` (约 4.8MB，无外部依赖)。

### 预构建版本

下载 [Releases](https://github.com/your-username/h2ac-rs/releases) 页面的 `h2ac-rs.exe`，直接运行。

---

## 使用

### 基本操作

| 操作 | 方法 |
|------|------|
| **装入战备** | 点击槽位块 → 槽位进入待命状态（金色边框）→ 点击右侧战备库条目 |
| **执行战备** | 双击已装填的槽位，或右键→执行 |
| **清除槽位** | 右键槽位→清除，或底部"清空全部" |
| **切换分类** | 点击战备库左侧分类标签（任务/轨道/飞鹰/武器/…） |
| **搜索战备** | 在战备库顶部搜索框输入名称或型号子串 |
| **设置热键** | 右键槽位→设热键→按下目标按键→确认 |
| **切换紧凑模式** | 点击标题栏右下角四角箭头图标 |
| **还原主界面** | 紧凑条右侧还原按钮 |
| **拖拽窗口** | 在标题栏（"H2AC-RS SUPER DESTROYER TERMINAL"）区域按住拖拽 |
| **监听开关** | 点击顶栏或紧凑条的状态灯 |

### 按键设置

点击标题栏齿轮图标打开设置面板：

| 设置项 | 默认值 | 说明 |
|--------|--------|------|
| ↑ / ↓ / ← / → | W / S / A / D | 方向键映射 |
| 激活键 | Ctrl | 按住激活键进入指令输入模式 |
| 按键延迟 | 0.05s | 每次按键之间的间隔 |

### Profile 管理

- **保存**：在底部栏输入名称，点击保存图标
- **加载**：在下拉框中选择已有 Profile
- **删除**：加载后点击垃圾桶图标
- Profile 文件存储在 `profiles/` 目录下（JSON 格式），位于 exe 同目录

---

## 配置

运行时配置自动保存在 exe 同目录的 `config.json`：

```json
{
  "key_bindings": {
    "↑": "w",
    "↓": "s",
    "←": "a",
    "→": "d"
  },
  "stratagem_key": "ctrl",
  "key_delay": 0.05,
  "slot_hotkeys": {
    "0": "f1",
    "1": "f2"
  },
  "loadout": [0, 25, 48, null, null, null, null, null, null, null],
  "listening_enabled": true,
  "last_profile": "bots_v1"
}
```

- `slot_hotkeys` — 键名用小写字母（a-z）、数字（0-9）、功能键（f1-f12）、`space`、`enter` 等
- `loadout` — 数字索引对应 `crate::stratagems::STRATAGEMS` 数组位置
- `listening_enabled` — 启动时是否自动开启热键监听

---

## 项目结构

```
h2ac-rs/
├── Cargo.toml
├── assets/
│   ├── fonts/                        # Saira Condensed (Medium + Bold, OFL)
│   └── icons/                        # 106 PNG 战备图标 (256×256, resvg 渲染)
├── src/
│   ├── main.rs                       # 入口 / 应用状态 H2ACApp / 窗口装配
│   ├── main_view.rs                  # 主界面: 顶栏 / 网格 / 详情 / 战备库 / 日志栏 / 右键菜单 / 模态框
│   ├── compact_view.rs               # 紧凑模式迷你条
│   ├── stratagems.rs                 # 战备数据库 (75+ 条) / 搜索 / 分类
│   ├── config.rs                     # JSON 配置与 Profile 持久化
│   ├── executor.rs                   # Win32 SendInput 键盘序列执行
│   ├── hotkey.rs                     # WH_KEYBOARD_LL 全局键盘钩子
│   ├── icons.rs                      # 图标嵌入 (include_bytes!) 与纹理加载
│   ├── theme.rs                      # 设计系统: 配色 / 字体 / 尺寸 / 分类色
│   └── widgets.rs                    # HUD 组件库: 切角面板 / 角括号 / 扫描线 / 箭头 / 字形
└── screenshots/
    ├── main.png
    └── compact.png
```

### 核心架构

```
main.rs (H2ACApp)
  ├── 状态: slots[10], armed, listening, flash, profiles, logs
  ├── update() → show_main() / show_compact()
  │
  ├── main_view.rs
  │   ├── render_topbar()       # 标题栏 + 监听灯 + 控件按钮
  │   ├── render_grid()         # 5×2 槽位网格
  │   ├── render_detail()        # 详情面板 (图标|文字|箭头|按钮)
  │   ├── render_library()       # 战备库 (分类栏 + 搜索 + 列表)
  │   ├── render_bottombar()     # 日志 + Profile 管理
  │   ├── render_context_menu()  # 右键菜单 (执行/清除/设热键)
  │   ├── render_capture_modal() # 热键捕获弹窗
  │   └── render_settings_modal()# 按键设置弹窗
  │
  ├── compact_view.rs
  │   ├── show_compact()         # 全局拖拽 + 10 槽位 + 控制区
  │   └── render_compact_tile()  # 单槽位小方块 (40×40)
  │
  ├── executor.rs                # 独立线程执行指令序列
  ├── hotkey.rs                  # 独立线程监听全局按键
  └── config.rs                  # 同步读写 JSON
```

---

## 技术栈

| 组件 | 技术 | 说明 |
|------|------|------|
| GUI | egui 0.31 (eframe) | 即时模式 GUI，无边框窗口 |
| 图标 | image 0.25 (PNG) | `include_bytes!` 编译时嵌入 106 个图标纹理 |
| 键盘模拟 | Win32 `SendInput` | 扫描码级别精确模拟 |
| 热键钩子 | Win32 `WH_KEYBOARD_LL` | 全局底层键盘钩子 |
| 序列化 | serde / serde_json | Config 与 Profile 持久化 |
| 字体 | Saira Condensed (OFL) | 内嵌 Medium + Bold |

---

## 设计风格

界面模仿《绝地潜兵2》游戏内 HUD 风格：

- **配色** — 超级地球金 (`#F5C842`) 为主色调，深太空黑 (`#05070C`) 背景
- **形状** — 左上+右下斜切面板（chamfer），四角 L 形括号装饰
- **纹理** — 低透明度扫描线覆盖层
- **分类色** — 轨道/飞鹰（赤红）、支援/背包（青色）、哨戒/地雷（军绿）
- **动画** — 状态灯呼吸脉冲、执行金色闪光衰减
- **字体** — Saira Condensed（拉丁/数字）+ 微软雅黑（CJK 兜底）

---

## FAQ

**Q: 游戏内没有反应？**
- 确保按键映射与游戏内设置一致（默认 WASD + Ctrl）
- 调整按键延迟到 0.08-0.12s（部分机器需要更高延迟）
- 确认战备激活键在游戏中绑定为 Ctrl（默认设置→战备键）

**Q: 热键不生效？**
- 确认顶栏状态灯显示"监听中"（绿色呼吸灯）
- 热键仅捕获单键（不支持组合键如 Ctrl+1）
- 某些全屏游戏可能抢占按键，尝试无边窗口或窗口全屏模式

**Q: 如何更新战备数据？**
- 编辑 `src/stratagems.rs`，添加/修改 `STRATAGEMS` 数组条目
- 如需新图标，放入 `assets/icons/` 并更新 `icon` 字段

---

## 致谢

- 图标素材来自 [nvigneux/Helldivers-2-Stratagems-icons-svg](https://github.com/nvigneux/Helldivers-2-Stratagems-icons-svg)，作者允许自由使用
- 字体 [Saira Condensed](https://fonts.google.com/specimen/Saira+Condensed) 采用 OFL 开源字体许可
- 灵感来自 Reddit 社区的原版 H2AC Python 工具

---

## 许可

MIT License
