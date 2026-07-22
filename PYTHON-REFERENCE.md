# HD2 Numpad Commander — 实现方法与功能文档

> 绝地潜兵2 小键盘战备指挥官 · Python + PyQt6 · Windows 专用
>
> 一个通过可配置小键盘网格来快速调用《绝地潜兵2》战备指令的桌面工具。

---

## 一、技术栈

| 组件 | 技术 | 用途 |
|------|------|------|
| GUI | PyQt6 | 桌面窗口、布局、自定义控件 |
| 图标渲染 | PyQt6.QtSvg (QSVGRenderer) | SVG 图标实时渲染 |
| 键盘模拟 | ctypes + Win32 API | 全局钩子监听 + SendInput/PostMessage 序列注入 |
| 序列化 | json | 配置文件、Profile、插件持久化 |
| 插件 | JSON 文件 + 运行时加载 | 自定义战备、主题扩展 |
| 系统托盘 | PyQt6.QSystemTrayIcon | 后台驻留、快捷控制 |
| 数据源 | wiki_fetcher (HTTP) | 从社区 Wiki 拉取最新战备数据 |

---

## 二、核心架构

```
main.py (StratagemApp, ~1500 行)
│
├── 状态层
│   ├── slots: Dict[str, NumpadSlot]       # 按键→槽位映射
│   ├── stratagems: Dict[str, List]        # 战备名→指令序列
│   ├── stratagems_by_department           # 按部门（Warbond）分组
│   ├── slot_layouts                        # 布局定义（默认小键盘 + 自定义网格）
│   └── global_settings                     # 全局配置（延迟、键位模式、主题...）
│
├── UI 层
│   ├── 左侧导航栏      可折叠导航（☠Helldivers / ⌗Slots / ⚙Settings）
│   ├── 顶栏           延迟滑块（1-200ms）、Profile 下拉选择/导入导出、撤销/保存/测试/清空
│   ├── Sidebar         搜索框 + 分类组合框 + 展开/折叠 + 战备图标列表（DraggableIcon）
│   ├── 小键盘网格      可配置的 NumpadSlot 网格（支持拖入战备、宏执行）
│   ├── 底部栏          宏开关 + 状态指示
│   ├── 模态弹窗        设置、热键捕获、序列录制、SVG 关联
│   └── 紧凑条          可选最小化到系统托盘
│
├── 管理层
│   ├── ProfileManager      读写 JSON Profile（映射: 按键码→战备名）
│   ├── PluginManager       加载/启用/禁用插件 JSON（战备/主题/图标覆盖）
│   ├── MacroEngine          全局键盘钩子 + 按键序列注入
│   ├── wiki_fetcher         HTTP 拉取 → 本地缓存 JSON
│   └── TrayManager          系统托盘图标 + 右键菜单
│
└── 配置层
    ├── constants.py        小键盘默认布局、键位映射（WASD/ESDF/arrows）、主题列表
    ├── config.py            设置读写、主题 QSS 生成、管理权限检测
    └── version.py           版本号
```

---

## 三、功能清单

### 3.1 核心战备系统

**槽位网格**
- **默认模式**：经典 5×4 小键盘布局（Num / * - + 7 8 9 4 5 6 1 2 3 0 . Enter），部分槽位隐藏（0/11/17/19）。
- **自定义网格模式**：用户可自由选择行×列（1-5×1-10，最大 20 键），每个按键独立绑定扫描码和标签。
- 槽位支持三种状态：空（提示文本）、已分配（显示战备名+图标+拖动接受）、执行中（视觉反馈）。
- **拖放装入**：从 Sidebar 拖动战备图标到目标槽位完成装入。
- **宏执行**：按下对应物理按键 → `MacroEngine` 按序注入方向键 + 延迟。

**战备数据库**
- 主数据源：`wiki_fetcher` 从社区 Wiki 拉取 JSON → 本地缓存 → 运行时加载。
- 插件扩展：JSON 插件可追加自定义战备（同名覆盖）。
- 数据结构：`{ 部门名: { 战备名: [方向序列...] } }`。
- 支持按 Warbond 或按游戏内类型（支援武器/轨道/飞鹰/哨戒/载具...）两种分类模式。

**Sidebar 图标列表**
- 部门折叠头（CollapsibleDepartmentHeader）：可展开/折叠 + 一键全部展开/折叠。
- 实时搜索过滤：输入文本按战备名字串匹配，搜索时自动展开所有部门。
- 侧边栏宽自适应：窗口大小调整时部门头自动拉伸到可视宽度。

### 3.2 宏引擎 (MacroEngine)

```
用户按下 numpad 按键
  → 全局键盘钩子捕获扫描码
  → 查找对应槽位的战备序列 [up,down,left,right,...]
  → 通过键位映射（arrows/WASD/ESDF）转为实际按键
  → 按可配置延迟（1-200ms）逐键注入
  → 注入前临时解锁卡住的修饰键（Alt/Shift）
```

- 使用 ctypes 调用 Win32 `GetAsyncKeyState` 检测卡键。
- 宏开关可全局启停（底部栏复选框 + 系统托盘菜单）。

### 3.3 键位捕获 (KeyCaptureDialog)

- 打开对话框后 `grabKeyboard()`，按下任意键捕获其 `nativeScanCode`。
- 支持检测 Ctrl/Alt/Shift 修饰键（通过帧间 modifier 状态变化）。
- 扫描码→标签映射表（`format_key_name`）：标准化显示名（如 L Ctrl→"L Ctrl"）。
- 用于：槽位热键绑定、设置弹窗的方向键/激活键重绑定。

### 3.4 序列录制 (SequenceRecorderDialog)

- 实时录制方向键序列：支持物理方向键（↑↓←→）、WASD 模式、ESDF 模式。
- 动态显示：箭头符号（↑ ↓ ← →）+ 文字列表。
- Backspace 撤销最后一步，Enter 确认保存。

### 3.5 Profile 管理

- **保存/加载**：JSON 文件存入 `profiles/` 目录，包含速度设置 + 槽位映射。
- **导入/导出**：任意路径 JSON 文件，自动处理重名冲突。
- **自动保存提醒**：顶栏保存按钮红框提示未保存变更。
- **撤销**：恢复到上次保存状态，支持全量回退。
- **自动加载**：启动时加载上次使用的 Profile（设置项 `autoload_profile`）。
- **顶栏删除**：`DeletableComboBox` 右侧 × 按钮直接删除当前 Profile。

### 3.6 插件系统

- **插件格式**：独立 JSON 文件（`plugins/*.json`），可包含战备、主题、图标覆盖。
- **战备插件**：`stratagems_by_department` + `icon_overrides`（SVG 路径映射）。
- **主题插件**：`themes: [{ name, colors: { background_color, border_color, accent_color } }]`。
- **UI 创建器**：应用内置 "Create Plugin" 表单，无需手动编辑 JSON：
  - 勾选创建类型（战备/主题）
  - 填写插件名、部门名
  - 战备条目：名+序列录制+SVG 关联（文件路径或粘贴代码），逐条保存
  - 主题：背景色/边框色/强调色（带颜色选择器）
  - 一键生成 JSON + 自动导出 SVG 文件
- **启用/禁用**：插件页列表带复选框，实时应用或注销。
- **插件重载**：重新加载运行时数据并重建侧边栏/主题。

### 3.7 槽位布局系统

- **预设**：`Default Numpad`（经典 5×4 小键盘）。
- **自定义网格** (New Layout...)：网格尺寸选择器（悬浮预览高亮，1-10列 × 1-5行）。
- 每个预览槽位可独立绑键（`KeyCaptureDialog`），支持清除/重新分配。
- 按键冲突检测：同一布局内不允许重复扫描码。
- 保存后立即应用（`apply_slot_layout`），主界面网格自动重建。
- 布局可删除（默认布局受保护）。

### 3.8 主题系统

- 内置主题列表（Dark/Light/Custom...），QSS 样式表驱动。
- 自定义主题创建（内置表单），实时预览颜色选择器。
- 主题可删除（内置主题除外），切换即时生效。
- 插件可携带主题（`themes` 字段），加载后合并入运行时。

### 3.9 系统托盘

- 最小化到托盘（设置项 `minimize_to_tray`）。
- 托盘菜单：显示窗口 / 启用宏 / 退出。
- 托盘图标：应用主图标。

### 3.10 设置面板

- **按键模式**：arrows / WASD / ESDF（切换战备方向键映射）。
- **按键延迟**：1-200ms 范围，实时反馈到顶栏显示。
- **管理权限**：可配置以管理员身份运行（确保全局钩子在游戏中生效）。
- **自动加载 Profile**、**最小化到托盘**、**自动检查更新**。
- **主题选择**：ComboBox 显示所有可用主题（含插件主题，标注来源）。
- **Wiki 缓存管理**：清除缓存 + 强制重新拉取。
- **测试环境** (TestEnvironment)：独立窗口验证宏执行。

---

## 四、关键实现细节

### 4.1 自定义 Qt Widgets

| Widget | 说明 |
|--------|------|
| `NumpadSlot` | 小键盘槽位：接受拖放（dragEnterEvent/dropEvent）、显示战备名+序列、宏执行时视觉反馈、右键清除 |
| `DraggableIcon` | Sidebar 战备图标：开始拖拽时携带战备数据（QMimeData）、渲染 SVG（路径或内嵌代码） |
| `CollapsibleDepartmentHeader` | 可折叠部门头：点击切换展开/收缩、更新 ↓/▶ 符号 |
| `DeletableComboBox` | 带删除按钮的下拉框：右侧 × 按钮，用于 Profile/布局删除 |
| `KeyCaptureDialog` | 按键捕获对话框：grabKeyboard + nativeScanCode |
| `SequenceRecorderDialog` | 序列录制对话框：方向键 + 键位模式动态映射 + 实时预览 |

### 4.2 拖放机制

使用 PyQt6 标准拖放 API：
- `DraggableIcon.mousePressEvent` → `QDrag` → `QMimeData`（存储战备名称）
- `NumpadSlot.dragEnterEvent` → 接受 `text/plain` MIME 类型
- `NumpadSlot.dropEvent` → 提取战备名 → `assign(stratagem_name)`

### 4.3 宏注入时序

```
按下激活键 (Ctrl / 自定义)
  → 遍历序列 [up, down, left, right, ...]
  → 每步: sleep(delay) → press(key) → sleep(delay) → release(key)
  → 释放激活键
  → 检测并临时释放卡住的修饰键 → 5ms 后恢复
```

### 4.4 数据流

```
启动
  → load_settings() → 全局配置
  → wiki_fetcher.load_cache_with_metadata() → 战备数据 + 类型映射
  → PluginManager.list_plugins() → 插件列表
  → _merge_custom_themes_into_runtime() → 合并自定义主题
  → _autoload_last_profile() → 加载上次 Profile
  ↓
用户交互
  → 拖入战备 → NumpadSlot.assign()
  → 保存 Profile → ProfileManager.save_profile()
  → 启用宏 → MacroEngine.enable()
  → 按下小键盘 → 全局钩子 → MacroEngine 执行序列
```

### 4.5 窗口自适应

- 左侧导航栏展开/收缩：自动增长/收缩窗口宽度（可逆）。
- 主标签页（Helldivers）：网格+列表水平/垂直布局自动切换（宽矮布局→垂直堆叠）。
- 插槽标签页（Slots）：内容尺寸变更后自动调整窗口（`slots_resize_timer` 防抖）。
- 最小窗口宽度：900px。

---

## 五、文件结构

```
project/
├── main.py                      # 入口 + StratagemApp 主窗口
├── src/
│   ├── config/
│   │   ├── __init__.py          # 导出
│   │   ├── config.py            # load/save_settings, get_theme_stylesheet, 管理权限
│   │   ├── constants.py         # 小键盘布局, 键位映射, 主题列表
│   │   └── version.py           # APP_NAME, VERSION
│   ├── core/
│   │   └── macro_engine.py      # 全局钩子 + 序列注入
│   ├── managers/
│   │   ├── profile_manager.py   # Profile JSON 读写
│   │   ├── plugin_manager.py    # 插件 JSON 管理
│   │   ├── update_manager.py    # 自动更新检查
│   │   └── wiki_fetcher.py      # Wiki 数据拉取/缓存
│   ├── ui/
│   │   ├── dialogs.py           # SettingsWindow, TestEnvironment
│   │   ├── tray_manager.py      # 系统托盘
│   │   └── widgets.py           # 自定义 Qt Widgets
│   └── assets/                  # 内置 SVG 图标, 字体
│       ├── icons/
│       ├── fonts/
│       └── themes/
├── profiles/                    # Profile JSON 文件
└── plugins/                     # 插件 JSON 文件
```

---

## 六、依赖

```
PyQt6 >= 6.5
```
无其他第三方 pip 依赖。键盘模拟和全局钩子通过 ctypes 调用 Win32 API。需要 Windows 10/11。

---

## 七、构建与运行

```bash
pip install PyQt6
python main.py
```

打包为 exe（PyInstaller）：
```bash
pyinstaller --onefile --windowed --icon=assets/icons/icon.ico main.py
```
