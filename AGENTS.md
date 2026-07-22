# H2AC-RS — AGENTS.md

> 绝地潜兵2 自动战备呼叫器 · Rust + egui 0.31 · Windows 专用
>
> **构建**：`cargo build --release` → `target/release/h2ac-rs.exe` (~4.8MB)
> **运行**：`cargo run`

---

## 架构总览

```
main.rs (320 行)
  H2ACApp — 应用状态与入口
    slots[10], armed, listening, flash, icons, compact, logs...
    update(): 热键轮询 → 闪光时间戳更新 → show_main() / show_compact()

  ├── main_view.rs (908 行)         主界面 (1100×640)
  │   render_topbar()                自定义标题栏 + 监听灯 + 控件
  │   render_grid()                  5×2 槽位网格
  │   render_slot_tile()             单槽位块（图标+名称+箭头+热键角标）
  │   render_detail()                详情面板（图标|文字列|箭头区|按钮）
  │   render_library()               战备库（分类栏 + 搜索框 + 列表）
  │   render_bottombar()             日志条 + Profile 管理
  │   render_context_menu()          右键菜单
  │   render_capture_modal()         热键捕获（使用 key_capture_modal）
  │   render_settings_modal()        按键设置（含方向键捕获）

  ├── compact_view.rs (160 行)      紧凑模式 (554×56)
  │   show_compact()                 10 槽位 + 监听灯 + 还原按钮
  │   compact_width()                宽度计算

  ├── theme.rs (150 行)             设计系统
  │   配色常量、分类色、字体安装、全局样式
  │   字体回退链: Saira MD → CJK(微软雅黑) → egui 内置

  ├── widgets.rs (496 行)           HUD 自绘组件库
  │   chamfer / corner_brackets / scanlines
  │   arrow / arrow_strip
  │   status_lamp (呼吸脉冲)
  │   Glyph (9 种铬件字形) + glyph_button
  │   hud_button / hud_panel
  │   key_capture_modal (通用热键捕获组件，含 Ctrl/Alt/Shift 修饰键检测)
  │   egui_key_to_name / format_key_name

  ├── stratagems.rs (75 条战备)     战备数据库
  │   8 分类, icon 字段, search(), get_by_category()
  │   常量: CAT_MISSION, CAT_ORBITAL, …

  ├── icons.rs                       图标嵌入 (include_bytes! + image 解码)
  │   IconStore — 75 个 128px 纹理

  ├── executor.rs (188 行)          Win32 SendInput 键盘模拟
  │   扫描码映射表 (LazyLock)
  │   执行序列: 按激活键 → 方向序列 → 释放激活键 → 解锁卡住的修饰键

  ├── hotkey.rs                     WH_KEYBOARD_LL 全局钩子
  │   start(map, tx, running)

  └── config.rs                     配置与 Profile (JSON)
      Config / Profile 结构体, SLOT_COUNT=10
```

---

## 核心状态 (H2ACApp)

| 字段 | 说明 |
|---|---|
| `slots[10]` | 槽位内 STRATAGEMS 索引 |
| `armed` | 待命槽位 —— 点击库条目装入此格 |
| `detail_slot` | 详情条当前展示的槽位 |
| `listening` | 全局热键监听开关 |
| `compact` | 紧凑模式标志 |
| `flash` | `HashMap<usize, f64>` —— 槽位执行闪光触发时刻 |
| `lib_category` / `lib_search` | 战备库当前分类与搜索词 |
| `icons` | `IconStore` —— 纹理句柄缓存 |
| `context` | 右键菜单状态 (slot + pos) |
| `capturing` / `captured` | 槽位热键捕获 |
| `settings_capture` | 设置弹窗内的按键捕获 (方向键 / 激活键) |
| `logs` | `VecDeque<LogEntry>` 最多 32 条 |

### 关键交互

- **装入**: 点击槽位 → armed = slot → 点击库行 → assign_stratagem → auto advance
- **执行**: 双击槽位 / 右键→执行 / 热门栏热键 → execute_slot → 新线程 SendInput
- **模式**: 标题栏 Compact 按钮 → set_compact(true) → 紧凑条 → Restore 按钮 → set_compact(false)
- **拖拽**: 标题栏 (main) / 全局 (compact) 注册 `Sense::drag` → `ViewportCommand::StartDrag`
- **闪光**: 0.7s 金色衰减动画, `request_repaint()` 驱动

---

## 依赖理由

| 依赖 | 用途 |
|---|---|
| egui/eframe 0.31 | GUI 框架 |
| image 0.25 (png only) | 编译时解码嵌入的战备图标 PNG |
| serde/serde_json | Config/Profile JSON 持久化 |
| windows 0.58 | SendInput + 全局钩子 + GetLocalTime |

无其他第三方依赖。所有 HUD 视觉（切角面板、角括号、箭头、字形）均用 egui Painter 自绘。

---

## 渲染惯例

### 坐标与布局
- **主界面** (1100×640): 固定尺寸, 无边框 (`with_decorations(false)` + `with_resizable(false)`)
- **布局**：顶栏 (48) → 内容区（左 664 + 右库 388）→ 底栏 (52)
- **紧凑模式** (554×56): 置顶 (`WindowLevel::AlwaysOnTop`)
- **面板**: `hud_panel` 从 ui 光标分配固定尺寸, 创建 child 内含子 UI

### 绘图模式
- 自绘组件统一用 `allocate_painter(sense)` + `Painter` 直接绘制
- 槽位块 / 分类行用 `ui.interact(rect, id, sense)` 注册交互区 + `ui.painter_at(rect)` 绘制
- 色值全部来自 `theme` 模块常量
- 字体: `hud(sz)` / `hud_b(sz)` 获取 FontId

### 分类色体系
从图标 PNG 采样得出:
- 任务战备 → `GOLD_MID` (#C9B269)
- 轨道/飞鹰 → `CAT_FIRE` 赤红 (#DE7B6C)
- 支援/背包/载具 → `CAT_EQUIP` 青色 (#49ADC9)
- 哨戒/雷盾 → `CAT_DEF` 军绿 (#679552)

---

## 资产嵌入

- **图标**: `assets/icons/*.png` (256×256, SVG→PNG 经 resvg 转换) — 用 `include_bytes!` 宏嵌入, `IconStore::load` 解码为 128px 纹理
- **字体**: `assets/fonts/SairaCondensed-{Medium,Bold}.ttf` — 内嵌, 构建入 exe
- **原始 SVG**: nvigneux/Helldivers-2-Stratagems-icons-svg (MIT 类许可)
- 转换脚本在 `C:\Users\Rin\AppData\Local\Temp\opencode\svg2png\`

---

## 配置

- `target/<profile>/config.json` — 启动时自动加载/保存 (exe 同目录)
- `target/<profile>/profiles/*.json` — Profile 文件
- Profile 结构: `{ loadout: Vec<Option<usize>>, slot_hotkeys: HashMap }`

---

## 已知问题

1. **分类栏点击不生效** (跳过)——详见 `UI-REFACTOR-STATUS.md` 第 5 节，含完整调试证据与 5 条排查路径。`hud_panel` 的 `Sense::hover()` 分配已移除以消除可疑诱因。
2. **screenshots/ 目录空**——README 引用的 `main.png` 和 `compact.png` 需捕获后放入。

---

## 常用命令

```bash
cargo build                 # debug
cargo build --release       # release (25s, 4.8MB)
cargo run                   # 运行 debug

# PowerShell 截屏 (运行中):
$src = '...'; Add-Type -TypeDefinition $src
$p = Get-Process h2ac-rs; $r = New-Object W.RECT
[W.U32]::GetWindowRect($p.MainWindowHandle, [ref]$r)
# 然后 CopyFromScreen 或 PrintWindow

# 查看崩溃日志:
cat target/debug/panic.log
cat target/release/panic.log
```
