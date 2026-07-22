# H2AC-RS

**Helldivers 2 Auto Stratagem Caller — Rust Rewrite**

> 绝地潜兵2 自动战备呼叫终端 — 沉浸式 HUD 风格桌面工具

一个 Windows 桌面应用，通过模拟键盘输入一键呼叫轨道火力、飞鹰空袭、支援武器等 100+ 种战备。支持插件扩展、Wiki 数据自动拉取、分类编辑、Profile 管理。

---

![主界面](screenshots/main.png)
![紧凑模式](screenshots/compact.png)

---

## 功能

- **战备数据库** — 75 条内置战备 + 插件扩展 + Wiki 自动拉取，覆盖 Orbital/Eagle/Support/Backpacks/Vehicles/Sentries/Emplacements/Mission 8 个功能分类
- **点击式配装** — 10 个槽位，点击待命 + 战备库装入，自动推进下一空槽；插件/Wiki 战备也可装入
- **分类筛选 + 搜索** — 侧边栏分类 Rail 切换类别，搜索框支持名称/型号子串实时过滤
- **双击 / 热键执行** — 双击槽位或按全局热键，通过 `SendInput` API 注入方向键序列；支持预延迟（Ctrl→面板就绪）和可调按键间隔
- **右键菜单** — 库行右键：装入/更改分类/编辑/删除；槽位右键：执行/清除/设热键
- **战备编辑** — 弹窗编辑插件战备的图标 key、指令序列、描述；更改即时同步回 JSON
- **分类修改** — 详情条分类 chip 点击弹出 ComboBox，选已有分类或新建；所有修改持久化到 `config.json`
- **Profile 管理** — 保存/▶加载/删除多套配装方案，含插件战备槽位数据
- **双模式**
  - **主界面** (1100×640) — 完整战术终端：自定义标题栏、2×5 槽位网格、详情面板、战备库、日志栏
  - **紧凑模式** (554×56) — 无边框悬浮迷你条，始终置顶，点击图标即执行
- **Wiki 数据拉取** — 一键从 Stratagem Hero Trainer 拉取最新战备数据，自动差集比对，仅新增写入 `plugins/_wiki_new.json`
- **插件系统** — JSON 文件放入 `plugins/` 即可扩展战备和主题，启动时自动加载
- **内置 UI 创建器** — 免手写 JSON：序列录制器（方向键/WASD 捕获）+ 战备录入 + 主题颜色选择器
- **按键设置** — 方向键映射（WASD / ESDF / 箭头）、激活键（支持 `lctrl`/`rctrl`/`lalt`/`ralt` 手动输入）、按键延迟 + 预延迟可调
- **执行闪光** — 全局热键或双击触发时金色闪光（0.7s 衰减）
- **监听开关** — 状态灯一键启停全局热键，呼吸脉冲动画
- **运行时图标加载** — exe 旁 `assets/icons/` 下新增 PNG 自动发现，无需重编译

---

## 安装

### 便携版

下载 `h2ac-rs-portable.zip`，解压到任意目录，运行 `h2ac-rs.exe`。

### 安装程序

安装 [Inno Setup 6](https://jrsoftware.org/isinfo.php) 后，双击 `installer.iss` 编译生成安装包。

### 从源码构建

**前置需求：** [Rust](https://www.rust-lang.org/tools/install) 1.75+ · Windows 10/11 64-bit

```bash
git clone https://github.com/your-username/h2ac-rs.git
cd h2ac-rs
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
| **删除战备** | 库行右键→删除（仅插件/Wiki 战备） |
| **设置热键** | 右键槽位→设热键→按下目标按键→确认 |
| **拉取 Wiki 数据** | 战备库头部🔍按钮（或创建器→📡拉取数据页签） |
| **创建插件** | 战备库头部💾按钮 → 创建器弹窗 |
| **切换紧凑模式** | 标题栏▦按钮 |
| **还原主界面** | 紧凑条右侧还原按钮 |
| **拖拽窗口** | 标题栏区域按住拖拽 |
| **监听开关** | 点击顶栏或紧凑条状态灯 |

### 按键设置

点击标题栏齿轮图标打开设置面板：

| 设置项 | 默认值 | 说明 |
|--------|--------|------|
| ↑ / ↓ / ← / → | W / S / A / D | 方向键映射（🎬捕获或手动输入） |
| 激活键 | Ctrl | `rctrl` / `lalt` 等手动输入以区分左右修饰键 |
| 按键延迟 | 0.08s | 每次按键间隔 |
| 预延迟 | 0.12s | 激活键按下后等待指令面板弹出 |

### Profile 管理

底部栏：输入名称 → 💾保存 / ▶加载 / 🗑删除

Profile 含槽位分配 + 插件战备 + 热键绑定，存储在 `profiles/` 目录。

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
  ],
  "themes": [
    {
      "name": "自定义主题",
      "background_color": "#0d0d0d",
      "border_color": "#ff6644",
      "accent_color": "#ff8844"
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
  "slot_hotkeys": { "0": "f1" },
  "loadout": [0, 25, 48, null, null, null, null, null, null, null],
  "listening_enabled": true,
  "last_profile": "bots_v1",
  "category_overrides": { "增援": "Mission Stratagems" }
}
```

---

## 项目结构

```
h2ac-rs/
├── Cargo.toml
├── build.rs                         # winres — exe 文件图标嵌入
├── installer.iss                    # Inno Setup 安装脚本
├── assets/
│   ├── fonts/                       # Saira Condensed 内嵌
│   ├── icons/                       # 106 PNG（内置嵌入 + 运行时发现）
│   └── icon-removebg.png            # 应用图标
├── src/
│   ├── main.rs                      # 入口 / 窗口装配
│   ├── main_view.rs                 # 主界面组装（薄层，调度 ui/）
│   ├── compact_view.rs              # 紧凑模式
│   ├── state.rs                     # AppModel / LibraryState / CaptureState / ...
│   ├── config.rs                    # Config / Profile JSON
│   ├── executor.rs                  # SendInput（execute_command 核心）
│   ├── hotkey.rs                    # WH_KEYBOARD_LL 全局钩子
│   ├── plugin.rs                    # 插件扫描 & 加载
│   ├── wiki_fetcher.rs              # Wiki JS 解析 & 差集比对
│   ├── icons.rs                     # IconStore（嵌入 + 磁盘兜底）
│   ├── theme.rs                     # 设计系统
│   ├── widgets.rs                   # HUD 组件库
│   ├── stratagems.rs                # 战备数据库 + PluginStratagem 类型
│   ├── model/                       # H2ACApp 方法分拆
│   │   ├── slots.rs / library.rs / category.rs / plugins.rs / wiki.rs
│   └── ui/                          # 视图面板
│       ├── common.rs / topbar.rs / grid.rs / detail.rs / library.rs / bottombar.rs
│       ├── plugin_creator.rs
│       └── modals/                  # 弹窗分拆
│           ├── context.rs / library_context.rs / capture.rs / settings.rs / stratagem_settings.rs
├── plugins/                         # 用户插件目录
├── profiles/                        # Profile 存储
└── screenshots/
```

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
- 内置 106 个图标已嵌入，新增 PNG 放 exe 旁 `assets/icons/` 即可

**Q: 如何更新数据？**
- 点击战备库头部🔍按钮 → 自动从 Wiki 拉取并差集比对
- 新战备写入 `plugins/_wiki_new.json`，下次启动自动加载

## 致谢

- 图标素材 [nvigneux/Helldivers-2-Stratagems-icons-svg](https://github.com/nvigneux/Helldivers-2-Stratagems-icons-svg)
- 战备数据 [Stratagem Hero Trainer](https://github.com/nvigneux/Stratagem-Hero-Trainer)
- 字体 [Saira Condensed](https://fonts.google.com/specimen/Saira+Condensed) (OFL)
- Wiki 数据 [helldivers.wiki.gg](https://helldivers.wiki.gg/wiki/Stratagems)

## 许可

MIT License
