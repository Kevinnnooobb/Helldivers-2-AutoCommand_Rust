# H2AC-RS

**Helldivers 2 Auto Stratagem Caller — Rust Rewrite**

> 绝地潜兵2 自动战备呼叫终端 — 沉浸式 HUD 风格桌面工具

一个 Windows 桌面应用，通过模拟键盘输入一键呼叫轨道火力、飞鹰空袭、支援武器等 100+ 种战备。支持插件扩展、战备数据在线自动获取、分类编辑、Profile 管理。

---

![主界面](screenshots/main.png)
![紧凑模式](screenshots/compact.png)

---

## 功能

- **战备数据库** — 75 条内置战备 + 插件扩展 + 在线自动获取，分类与顺序以 [helldivers.wiki.gg/wiki/Stratagems](https://helldivers.wiki.gg/wiki/Stratagems) 页面为准：Orbital/Eagle/Support/Backpacks/Vehicles/Sentries/Emplacements/Mission/Objective/Unavailable 10 个分类
- **强化（Booster）分类** — 独立分类，数据与图标来自 [helldivers.wiki.gg/wiki/Boosters](https://helldivers.wiki.gg/wiki/Boosters)：名称、描述、所属债券、图标（页面 SVG → 自动栅格化为 PNG 落盘）；强化没有呼叫指令，因此不会被当作战备注入按键
- **点击式配装** — 10 个槽位，点击待命 + 战备库装入，自动推进下一空槽；插件/自动获取战备也可装入
- **分类筛选 + 搜索** — 侧边栏分类 Rail 切换类别，搜索框支持名称/型号子串实时过滤
- **双击 / 热键执行** — 双击槽位或按全局热键，通过 `SendInput` API 注入方向键序列；支持预延迟（Ctrl→面板就绪）和可调按键间隔
- **右键菜单** — 库行右键：装入/更改分类/编辑/删除；槽位右键：执行/清除/设热键
- **战备编辑** — 弹窗编辑插件战备的图标 key、指令序列、描述；更改即时同步回 JSON
- **分类修改** — 详情条分类 chip 点击弹出 ComboBox，选已有分类或新建；所有修改持久化到 `config.json`
- **Profile 管理** — 保存/▶加载/删除多套配装方案，含插件战备槽位数据
- **两种窗口形态**
  - **主界面** (1100×640) — 完整战术终端：自定义标题栏、2×5 槽位网格、详情面板、战备库、日志栏
  - **紧凑模式** (384×536) — 游戏内配装预设浮窗，取代了原来的 554×56 迷你执行条：一屏列出全部 10 个槽位（上排 TASK + 下排 LOADOUT），右键选择战备/强化，按自动装配键后**立即隐藏**并后台完成游戏内装配（详见下方「紧凑模式（配装预设 Overlay）」）
- **战备数据在线获取** — 一键从 [helldivers.wiki.gg/wiki/Stratagems](https://helldivers.wiki.gg/wiki/Stratagems) 页面拉取最新战备表格（Name / Stratagem Code 列），自动差集比对，新增战备按页面分类直接入库（不再附加 New/Wiki 标签），缓存写入 `plugins/_wiki.json`；同一按钮会继续拉取 [helldivers.wiki.gg/wiki/Boosters](https://helldivers.wiki.gg/wiki/Boosters) 的强化表格（Icon / Booster / Description / Warbond / Price）写入 `plugins/_boosters.json`；启动时自动检测缓存，支持一键清除
- **插件系统** — JSON 文件放入 `plugins/` 即可扩展战备，启动时自动加载
- **内置 UI 创建器** — 免手写 JSON：序列录制器（方向键/WASD 捕获）+ 战备录入（指令自动规范为 `up/down/left/right` 格式）
- **按键设置** — 方向键映射（WASD / ESDF / 箭头）、激活键（支持 `lctrl`/`rctrl`/`lalt`/`ralt` 手动输入）、按键延迟 + 预延迟可调；槽位快捷键支持字母、数字、F1–F24 与 `,` `.` `/` 等标点
- **执行闪光** — 全局热键或双击触发时金色闪光（0.7s 衰减）
- **监听开关** — 状态灯一键启停全局热键，呼吸脉冲动画；可为监听开关绑定全局快捷键快速启停；热键与 Profile 修改即时生效，无需重启应用
- **运行时图标加载** — exe 旁 `assets/icons/` 下新增 PNG 自动发现，无需重编译
- **自动装配（Loadout Sync）** — 下排 Slot 06~10 作为「游戏 Loadout 配置」（S1~S4 + Booster），按全局快捷键后由 H2AC-RS 截图识别游戏配装界面，用正常鼠标输入自动选中四个战备与可选 Booster；不修改游戏快捷键/文件/内存，也不注入 DLL

---

## 安装

### 便携版

下载 `h2ac-rs-portable.zip`，解压到任意目录，运行 `h2ac-rs.exe`。

### 安装程序

安装 [Inno Setup 6](https://jrsoftware.org/isinfo.php) 后，双击 `installer.iss` 编译生成安装包。

### 从源码构建

**前置需求：** [Rust](https://www.rust-lang.org/tools/install) 1.75+ · Windows 10/11 64-bit

```bash
git clone https://github.com/Kevinnnooobb/Helldivers-2-AutoCommand_Rust.git
cd Helldivers-2-AutoCommand_Rust
cargo build --release
```

产物 `target/release/h2ac-rs.exe` (~7MB，无外部运行时依赖)。

---

## 使用

### 基本操作

| 操作 | 方法 |
|------|------|
| **装入战备** | 点击槽位块（待命金色高亮）→ 点击右侧战备库条目 |
| **执行战备** | 双击已装填的槽位，或右键→执行 |
| **清除槽位** | 右键槽位→清除 |
| **切换分类** | 点击战备库左侧分类标签（Orbital / Eagle / Support / Sentries / …） |
| **搜索战备** | 战备库顶部搜索框输入名称或型号 |
| **修改分类** | 详情条分类名点击 → ComboBox 选已有或新建 |
| **编辑战备** | 库行右键→设置 → 弹窗编辑图标/指令/描述 |
| **删除战备** | 库行右键→删除（仅插件/自动获取战备） |
| **设置热键** | 右键槽位→设热键→按下目标按键→确认 |
| **自动获取战备数据** | 战备库头部🔍按钮（或创建器→📡拉取数据页签）——一次同时拉取战备页与强化页：战备 → `plugins/_wiki.json`，强化 → `plugins/_boosters.json` |
| **创建插件** | 战备库头部💾按钮 → 创建器弹窗 |
| **切换紧凑模式** | 标题栏 ▦ 按钮或 Ctrl+Shift+F7（显示/隐藏配装预设浮窗） |
| **还原主界面** | 浮窗标题栏还原按钮（只切视图，不影响浮窗可见性状态） |
| **拖拽窗口** | 标题栏区域按住拖拽 |
| **监听开关** | 点击顶栏或紧凑条状态灯；右键状态灯绑定全局快捷键 |
| **自动装配** | 下排装好 S1~S4（+ 可选 Booster）→ 游戏停在配装主界面且四个战备槽为空 → 按自动装配快捷键（默认 F7）或点顶栏「自动装配」；**普通模式与浮窗模式下都可用，且与浮窗是否可见无关** |
| **唤出配装预设浮窗** | 默认 Ctrl+Shift+F7（**唯一**能改变浮窗显示状态的开关）；浮窗内右键槽位选择战备/强化 |
| **取消自动装配** | 默认 Ctrl+Shift+F9（全局取消键，普通模式 / 浮窗模式 / 浮窗隐藏时都生效） |

### 按键设置

点击标题栏齿轮图标打开设置面板：

| 设置项 | 默认值 | 说明 |
|--------|--------|------|
| ↑ / ↓ / ← / → | W / S / A / D | 方向键映射（🎬捕获或手动输入） |
| 激活键 | Ctrl | `rctrl` / `lalt` 等手动输入以区分左右修饰键 |
| 按键延迟 | 0.08s | 每次按键间隔 |
| 预延迟 | 0.12s | 激活键按下后等待指令面板弹出 |
| 监听开关 | 无 | 全局快捷键快速启停监听（🎬捕获或手动输入，留空清除） |
| 浮窗热键 | Ctrl+Shift+F7 | **唯一**能显示/隐藏配装预设 Overlay 的开关（支持修饰键组合，可改绑） |
| 自动装配 | F7 | 触发自动装配（普通模式 / 浮窗模式 / 浮窗隐藏时都可用；只管装配，不切换浮窗） |
| 取消装配 | Ctrl+Shift+F9 | 全局取消：立即停止后续自动化并释放输入，回到 Idle |
| 浮窗透明度 | 0.92 | 0.35 ~ 1.0；浮窗标题栏「透明」按钮可循环切换 |

> 设置面板内容较多，弹窗中部可滚动（滚轮或拖动条），「保存 / 取消」固定在底部始终可见。

### 自动装配（Loadout Sync）

下排五个槽位对应游戏 Loadout：

| H2AC 槽位 | 游戏目标 |
|-----------|----------|
| Slot 06 | Stratagem 1 |
| Slot 07 | Stratagem 2 |
| Slot 08 | Stratagem 3 |
| Slot 09 | Stratagem 4 |
| Slot 10 | Booster（可留空 = 跳过；建议从战备库的「Booster」分类里选） |

执行前提：

1. HELLDIVERS 2 处于前台，并停在配装主界面（Loadout Home）；
2. 四个战备槽为空（默认策略；不会自动清空，可在设置里开启「已有战备时允许覆盖」）；
3. H2AC-RS 权限不低于游戏（否则鼠标注入会被系统拒绝，日志会提示以管理员运行）。

流程：截图 → 识别配装界面与槽位 → 点开槽位 → 识别战备列表（可滚动 Viewport）→ 图标匹配定位目标 → 悬停验证 → 点击 → 校验槽位已选中 → 下一个目标 → 最终逐槽校验。

截图后端：**主用 Windows Graphics Capture（WGC）**，失败时回退到 GDI 桌面 DC。WGC 由系统合成器直接交付窗口内容，
**独占全屏 / 无边框全屏 / 窗口模式三种显示模式都能抓到真实画面**，并且只抓游戏窗口本身（H2AC 自己的浮窗、
其他覆盖窗口都不会混进识别画面）。GDI 的窗口 DC 抓取对 DX11/DX12 + flip model 交换链恒为全黑，
因此只作为兜底（并且改用桌面 DC + 屏幕坐标）。实际使用的后端会打印在应用日志里（`截图后端: WGC` / `GDI(桌面DC)`）。
两条路径都会检测全黑帧并明确报错，绝不基于黑屏做识别与点击。WGC 需要 Windows 10 1903 及以上。
执行期间按取消键（默认 Ctrl+Shift+F9）即可停止。全部失败/取消路径都会释放鼠标与按键，不会留下按下状态。

**滚动与位移验证**：单次滚动被限制在「不足一页」——可见行数的一半（至少 1 格），
这样滚动前后一定有共同行可以对齐，才能确认「滚动真的生效」。
（实机上 600 滚轮单位 ≈ 整页；整页滚动会把可见行全部换掉，位移无法测量，
会被误判成「滚动未生效」。）如果某次滚动确实超过可见行数、测不出位移，
则按「已移动」继续搜索并自动缩小步长，不会卡死。
`loadout_sync.max_scroll_attempts` 默认 24：步长变小后，遍历同一段列表需要更多次滚动。

**图标识别精度（已知问题）**：实机列表/槽位上的字形是**分类色的彩色字形 + 网点底纹**，
而 H2AC 图标是彩色美术图；当前的「亮度二值剪影 + 多尺度 Dice」在实机画面上区分度不足
（实机 13 个标注格子 top-1 准确率 0，榴弹发射器最高仅 0.55 → 会报
`RecognitionUnsupported / 无法可靠识别`）。**失败是安全的**：分数不够时不会点击，
不会把战备装错。识别失败时应用日志会打印该格子的**整库前三名**（分数 + 名称），
便于判断是「被相似图标挤掉」还是「完全没对上」。
标定用的实机标注样本、12 种替代方案的实测结论与复现命令见
`src/fixtures/loadout_sync/README.md`。

**光标位置**：游戏里滚轮与识别都不依赖光标位置（实测鼠标在界面任何位置都能滚动列表），
因此除「悬停验证」本身以外，**每次截图前都会先把光标停到列表右侧的空白处** ——
光标停在列表上会触发悬停高亮与说明面板，直接改变行签名与图标外观，明显影响识别。
停靠点始终位于游戏客户区内，滚轮事件照常发给游戏窗口。

**列表行检测**：真实游戏列表里夹着分类标题（"补给"、"支援"…），行距会被标题顶开，
被高亮选中的整块行边线也会变弱。因此行检测先用**强边线聚类**定骨架，再用峰追踪/等距网格兜底，
保证屏幕上可见的行都能被识别出来（实机实测：只认等距网格时 6 行画面只认出 2 行，
滚动验证必然失败）。

日志前缀 `[LoadoutSync]`；开启「保存调试图」后失败帧会写入 `screenshots/loadout_sync/`。

### 紧凑模式（配装预设 Overlay）

面向「人已经在游戏配装界面」的场景：浮窗只作为配装辅助控制层，不需要打开主窗口。

```
游戏 Loadout Home
  → Ctrl+Shift+F7      显示浮窗（再按一次 = 隐藏；顶栏 ▦ 按钮同效）
  → 右键上排 01~05     TASK 战备（战斗中呼叫用，确认后立即保存）
  → 右键下排 06~09     游戏 Stratagem 1~4（写入预设草稿）
  → 右键下排 10        游戏 Booster（默认只列 Booster 分类）
  → F7                 执行自动装配（浮窗正在显示时「立即」隐藏，结束后按原状态恢复）
  → 想查看进度：装配期间再按 Ctrl+Shift+F7 可以打开浮窗（只读）
  → Ctrl+Shift+F9      随时取消
```

要点：

- 浮窗一屏显示**全部 10 个槽位**（行高按可用高度反推，Slot10 = Booster 不会被挤出可视区）：上排 01~05 是 TASK（战斗中呼叫，编辑后立即写盘，与主界面一致），下排 06~10 是 LOADOUT（S1~S4 + Booster，只有这 5 个参与自动装配）；
- **按键职责严格正交**：`Ctrl+Shift+F7` 只管浮窗显示/隐藏；`F7` 只管自动装配（永远不切换浮窗）；`Ctrl+Shift+F9` 只管取消。三个键互相不调用，也不根据游戏状态自动判断 Save / Apply；
- **自动装配与模式无关**：普通模式、浮窗模式、浮窗隐藏时按 F7 都会执行；它记录「开始前浮窗是否可见」，结束后按该记录恢复（成功 / 失败 / 取消 / 异常都会恢复），若用户在装配期间自己动过浮窗，则以用户操作为准；
- 普通模式下自动装配**完全不触碰窗口**（不切视图、不改透明度、不置顶）；
- 浮窗模式下的临时隐藏＝不透明度置 0 + 点击穿透：游戏画面完全无遮挡，但进程不退出、不最小化、不抢焦点、不 Alt+Tab；「隐藏」不是把窗口 Visible(false)（那会让渲染线程停止，全局热键随之失效）；
- 运行期间浮窗**允许查看、禁止编辑**（右键选择器会被拒绝并给出提示）；
- 编辑只改内存草稿，**不会**每次右键就写磁盘；只有最终校验成功后才写回 Slot06~10 与当前 Profile，失败 / 取消不覆盖原预设；
- 本地校验（4 个 Stratagem 是否齐全、是否已有任务在跑）失败时**不启动自动化**，错误直接显示在浮窗状态栏与应用日志里；
- `Ctrl+Shift+F9` 取消：停止后续步骤、释放所有按键与鼠标、回到 Idle；已选进游戏的部分不会自动清空；
- 浮窗置顶是真正的 Windows Topmost（`SetWindowPos(HWND_TOPMOST, …, SWP_NOACTIVATE)`，不抢焦点），每次渲染都会读回校验透明度 / 点击穿透 / 置顶状态；
- 浮窗位置可拖动（自动记住），透明度可在设置面板或用标题栏「透明」按钮调整；
- 视觉识别仍然完全走 Loadout Sync 的「截图 → 状态识别 → 图标匹配 → Hover → 点击 → 验证」链路，没有固定坐标 + 固定 Sleep 的实现。

### Profile 管理

底部栏：输入名称 → 💾保存 / ▶加载 / 🗑删除

Profile 含槽位分配 + 插件战备 + 热键绑定，存储在 `profiles/` 目录。加载后槽位配置同步持久化到 `config.json`，重启保持。

---

## 插件系统

在 `plugins/` 目录放入 JSON 文件，启动时自动加载。格式：

```json
{
  "id": "my_plugin",
  "name": "自定义插件",
  "enabled": true,
  "stratagems": [
    {
      "name": "自定义战备",
      "category": "Support Weapons",
      "model": "CUSTOM",
      "command": ["up", "down", "left", "right"],
      "description": "描述文字",
      "icon": "reinforce"
    }
  ]
}
```

内置 UI 创建器可免手写 JSON（战备库 💾 按钮）。

---

## 配置

`config.json`（exe 同目录）：

```json
{
  "key_bindings": { "↑": "w", "↓": "s", "←": "a", "→": "d" },
  "stratagem_key": "ctrl",
  "key_delay": 0.08,
  "pre_delay": 0.12,
  "slot_hotkeys": { "0": "f1", "1": "," },
  "listen_hotkey": "f8",
  "loadout": [0, 25, 48, null, null, null, null, null, null, null],
  "listening_enabled": true,
  "last_profile": "bots_v1",
  "category_overrides": { "增援": "Mission Stratagems" },
  "loadout_sync_hotkey": "f7",
  "loadout_sync_cancel_hotkey": "ctrl+shift+f9",
  "compact_mode": {
    "enabled": true,
    "toggle_hotkey": "ctrl+shift+f7",
    "opacity": 0.92,
    "position_x": -1,
    "position_y": -1
  },
  "loadout_sync": {
    "scroll_delta": 600,
    "scroll_probe_delta": 120,
    "recognition_threshold": 0.6,
    "max_scroll_attempts": 12,
    "max_retry_attempts": 3,
    "allow_overwrite_filled": false,
    "debug_screenshots": false
  }
}
```

缺字段自动使用默认值；`loadout` 中的非法索引在加载时自动视为空槽。序列化失败时不会覆盖现有文件。

> **旧版本配置自动迁移**：v1.1 及更早把「自动装配 / 取消」写在 `compact_mode.auto_loadout_hotkey` 与 `compact_mode.cancel_hotkey`。
> 新版加载时会自动把它们迁移到全局字段 `loadout_sync_hotkey` / `loadout_sync_cancel_hotkey`（仅在全局字段为空时），
> 并删除 `compact_mode` 下的旧键，老用户的自定义快捷键不会丢。紧凑模式不再拥有自动装配快捷键。

---

## 项目结构

```
h2ac-rs/
├── Cargo.toml
├── build.rs                         # winres — exe 文件图标嵌入
├── installer.iss                    # Inno Setup 安装脚本
├── assets/
│   ├── fonts/                       # Saira Condensed 内嵌
│   ├── icons/                       # 107 PNG（内置嵌入 + 运行时发现）
│   └── icon-removebg.png            # 应用图标
├── src/
│   ├── main.rs                      # 入口 / 主循环 / 热键与 Wiki 装配
│   ├── main_view.rs                 # 主界面组装（薄层，调度 ui/）
│   ├── compact_mode/                # 紧凑模式（配装预设 Overlay）：config.rs / preset.rs
│   ├── overlay_win.rs               # 浮窗窗口行为（分层透明度 / 点击穿透 / 定位，只操作自己的窗口）
│   ├── state.rs                     # AppModel / LibraryState / CaptureState / ...
│   ├── config.rs                    # Config / Profile JSON
│   ├── executor.rs                  # SendInput（execute_command 核心）
│   ├── hotkey.rs                    # WH_KEYBOARD_LL 全局钩子（显式生命周期 + 热更新）
│   ├── plugin.rs                    # 插件扫描 / 加载 / 统一改写
│   ├── wiki_fetcher.rs              # 页面 HTML 表格解析与在线异步拉取（本地快照仅作结构参考/测试）
│   ├── icons.rs                     # IconStore（嵌入 + 磁盘兜底）
│   ├── theme.rs                     # 设计系统
│   ├── widgets.rs                   # HUD 组件库
│   ├── stratagems.rs                # 战备数据库 + PluginStratagem 类型
│   ├── util.rs                      # 基础设施：app_dir / save_json / 后台日志
│   ├── fixtures/                    # 单元测试数据（wiki_stratagems.html / wiki_boosters.html / loadout_sync 截图夹具）
│   ├── loadout_sync/                # 视觉自动化子系统（截图/识别/图标匹配/视口/输入/状态机）
│   │   ├── capture.rs               # 截图入口：WGC 优先 + GDI 桌面 DC 兜底 + 全黑帧检测
│   │   └── wgc.rs                   # Windows Graphics Capture 后端（D3D11 + WinRT，含 HDR→SDR 映射）
│   ├── model/                       # H2ACApp 方法分拆
│   │   ├── slots.rs / library.rs / category.rs / plugins.rs / wiki.rs
│   │   └── loadout.rs / preset.rs   # Loadout Sync 与紧凑预设浮窗的 GUI 胶水层
│   └── ui/                          # 视图面板
│       ├── common.rs / topbar.rs / grid.rs / detail.rs / library.rs / bottombar.rs
│       ├── plugin_creator.rs / preset_overlay.rs   # 紧凑模式浮窗（全部 10 槽位 + 选择器）
│       └── modals/                  # 弹窗分拆
│           ├── context.rs / library_context.rs / capture.rs / settings.rs / stratagem_settings.rs
├── plugins/                         # 用户插件目录（_wiki.json = 战备，_boosters.json = 强化）
├── profiles/                        # Profile 存储
└── screenshots/
```

---

## 开发

```bash
cargo test                                      # 24 个单元测试（解析器 / 配置 / 方向映射 / 槽位状态机等）
cargo clippy --all-targets -- -W clippy::all    # 静态检查（当前 0 警告）
cargo build --release                           # 产物 target/release/h2ac-rs.exe
```

测试覆盖：页面 HTML 表格解析器（details/summary 与 big/b 标签分类、fixture 进仓库不依赖外部文件、债券表不误解析；仓库根目录的本地页面快照存在时额外校验真实结构，但运行时不读取它）、强化页面解析器（Booster/Description 表头识别、导航表排除、图标地址规范化、_boosters.json 落盘与回读）、方向转换契约、Config/Profile 序列化与值域校验、扫描码映射与别名、热键键名、指令文本解析、槽位推进状态机，以及 Loadout Sync 的完整视觉/状态机测试（真实截图夹具 + 合成列表夹具）。

联网测试默认 ignore（不依赖网络）：`cargo test -- --ignored` 会实际拉取两页并校验解析结果与全部强化图标栅格化。

---

## 设计风格

- **配色** — 超级地球金 (`#F5C842`) · 深太空黑 (`#05070C`) · 分类色：红(Offensive) / 青(Supply) / 绿(Defensive) / 金(Mission)
- **形状** — 切角面板（chamfer）、角括号、扫描线
- **动画** — 状态灯呼吸脉冲、执行金色闪光
- **字体** — Saira Condensed（拉丁）+ 微软雅黑（CJK 兜底）

---

## FAQ

**Q: 游戏内没有反应？**
- 确保按键映射与游戏一致；激活键区分左右需手动输入 `rctrl`
- 调整延迟：按键 0.08-0.12s，预延迟 0.12-0.2s
- 以管理员身份运行（反作弊可能拦截非提权 SendInput）
- 检查日志栏执行记录确认指令已发送

**Q: 插件战备不显示图标？**
- 图标 key 必须在 `assets/icons/{key}.png` 存在
- 内置 107 个图标已嵌入，新增 PNG 放 exe 旁 `assets/icons/` 即可

**Q: 如何更新数据？**
- 点击战备库头部🔍按钮 → 自动从 [helldivers.wiki.gg/wiki/Stratagems](https://helldivers.wiki.gg/wiki/Stratagems) 在线拉取并差集比对
- 新增战备按页面上的分类直接入库（不再附加 New/Wiki 标签），写入 `plugins/_wiki.json`，下次启动自动加载；本次拉取全部命中内置时自动删除旧缓存
- 同一次拉取还会从 [helldivers.wiki.gg/wiki/Boosters](https://helldivers.wiki.gg/wiki/Boosters) 获取强化（名称/描述/债券/图标），写入 `plugins/_boosters.json` 并归入「Booster」分类；缺少的强化图标会按页面 SVG 自动下载栅格化为 `assets/icons/{key}.png`
- 仓库根目录的本地页面快照仅作解析器结构参考与测试，绝不作为运行时数据源（避免过期数据）
- 创建器 📡 页签可查看拉取进度，并在「已缓存」时一键清除缓存

## 致谢

- 图标素材 [nvigneux/Helldivers-2-Stratagems-icons-svg](https://github.com/nvigneux/Helldivers-2-Stratagems-icons-svg)
- 战备数据 [helldivers.wiki.gg/wiki/Stratagems](https://helldivers.wiki.gg/wiki/Stratagems)
- 强化数据与图标 [helldivers.wiki.gg/wiki/Boosters](https://helldivers.wiki.gg/wiki/Boosters)
- 字体 [Saira Condensed](https://fonts.google.com/specimen/Saira+Condensed) (OFL)

## 许可

MIT License
