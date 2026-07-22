# H2AC-RS 新版系统 — 实施计划

> 基于 Python 参考实现 (PYTHON-REFERENCE.md) 的设计，为 Rust/egui 版本新增四大子系统。
> 以下按**实施优先级**排列，每完成一项提交。

---

## 优先级 1：执行器修复 —— 按键宏在 GUI 显示生效但游戏内无反应

### 根因分析 (executor.rs 审查)

| 问题 | 当前值 | 游戏需要 | 说明 |
|------|--------|----------|------|
| 预延迟 | 无 | **~120ms** | Ctrl 按下后游戏弹出指令面板需要时间；当前无延迟直接发箭头→面板未就绪 |
| 按键间隔 | 0.05s | **0.08-0.12s** | 50ms 对游戏输入采样来说过短，部分帧会丢失 |
| 管理员权限 | 未检测 | **建议** | GameGuard/nProtect 反作弊可能拦截非管理员进程注入的键盘事件 |

### 修改方案

1. **`config.rs`** — 新增 `stratagem_pre_delay: f64` 字段（默认 0.12s）
2. **`executor.rs`** — `execute_stratagem` 增加 `pre_delay` 参数：
   ```
   press_key(stratagem_key) → sleep(pre_delay) → 箭头序列 → release_key
   ```
3. **`main_view.rs` 设置弹窗** — 增加 `预延迟(秒)` 控件（range 0.05..0.4）
4. **启动时管理员检测** — 弹出提示建议以管理员身份运行（Win32 `IsUserAnAdmin`，`windows` crate 已有）

---

## 优先级 2：插件 JSON 系统 —— 动态扩展战备/主题/图标

### 插件格式

```json
// plugins/urban_legends.json
{
  "id": "urban_legends",
  "name": "Urban Legends Warbond",
  "enabled": true,
  "stratagems_by_department": {
    "城市传奇": {
      "定向护盾": ["down", "up", "left", "down", "up", "down"],
      "反坦克炮台": ["down", "left", "up", "up", "down"]
    }
  },
  "icon_overrides": {
    "定向护盾": "directional_shield",
    "反坦克炮台": "anti_tank_emplacement"
  },
  "themes": [
    {
      "name": "Urban Legend",
      "colors": {
        "background_color": "#0d0d0d",
        "border_color": "#ff6644",
        "accent_color": "#ff8844"
      }
    }
  ]
}
```

### 实施

1. **`plugin.rs`** — `PluginManager`：扫描 `plugins/*.json`，反序列化，合并到运行时
2. **`stratagems.rs`** — 运行时 `Vec<Stratagem>` 可变集合（`RUNTIME_STRATAGEMS: LazyLock<Mutex<Vec<Stratagem>>>`），插件加载时追加
3. **`theme.rs`** — 运行时 `THEME_OVERRIDES: LazyLock<Mutex<HashMap<String, ThemeColors>>>`，插件主题合并
4. **`icons.rs`** — 插件图标仅引用已有 key（同目录下 SVG→PNG 需用户自行提供），覆盖 `IconStore.get()` 的 fallback 逻辑
5. **`main.rs`** — 启动时调用 `PluginManager::load_all()`，log 加载结果

---

## 优先级 3：Wiki + 图标自动拉取 —— 在线更新战备数据库

### 数据源

- **战备数据**（JSON）：社区 Wiki API / 静态 JSON 文件（由维护者定期更新）
- **图标**：nvigneux/Helldivers-2-Stratagems-icons-svg GitHub releases

### 实施

1. **`Cargo.toml`** — 新增 `ureq = { version = "2", default-features = false, features = ["tls"] }`
2. **`wiki_fetcher.rs`**：
   - `fetch_stratagems(url: &str, cache_path: &Path)` → `Result<Vec<Stratagem>>`
   - `fetch_icons(repo: &str, output_dir: &Path)` → `Result<usize>`
   - `has_newer_content(cache_path: &Path) -> bool` (ETag / Last-Modified)
   - 缓存到 `app_dir()/wiki_cache/`
3. **`main_view.rs` 战备库** — "刷新数据"按钮（带独立线程 + 加载动画）
4. **设置弹窗** — "清除缓存并重新拉取"

---

## 优先级 4：内置 UI 创建器 —— 免手写插件 JSON

### 功能

- **创建战备插件**：填写插件名、部门名，逐条录入战备（名称+序列录制/手动输入+图标选择），保存即生成 JSON
- **创建主题插件**：填写主题名、背景色/边框色/强调色（带颜色选择器），保存即生成 JSON
- **序列录制器**：`SequenceRecorderModal` — 键盘按键捕获↑↓←→（支持 WASD/ESDF 模式），实时预览，Backspace 撤销

### 实施

1. **`main_view.rs`** — 新增 `render_plugin_creator_modal`：
   - 插件名输入
   - 勾选创建类型（战备 / 主题）
   - 战备录入区域（`StratagemEntryWidget` 等价：名称 + 序列录制按钮 + 图标下拉选择）
   - 主题输入区域（三色选择，用 egui `color_edit_button_rgb`）
   - 生成并保存 JSON 到 `plugins/` 目录
2. **`widgets.rs`** — 序列录制器 `sequence_recorder_modal`（复用 `key_capture_modal` 的 Area + 按键捕获模式，方向过滤）
3. **左侧导航或设置入口** — "管理插件"按钮打开 creator / 插件列表

---

## 实施顺序

```
1a. executor 修复（预延迟 + 默认延迟 0.08 + 设置 UI）
1b. 管理员权限检测提示
  → cargo build --release 验证游戏内可用性
  → 提交

2a. 插件 JSON 格式设计 + plugin.rs 模块
2b. theme.rs / stratagems.rs / icons.rs 运行时合并
2c. 启动加载 + 日志
  → 提交

3a. ureq 依赖 + wiki_fetcher.rs 基础实现
3b. 战备库"刷新"按钮
3c. 图标仓库拉取逻辑
  → 提交

4a. 序列录制器（widgets.rs 新增）
4b. 插件创建器弹窗（main_view.rs 新增）
4c. 主题创建器（颜色选择器）
  → 提交
```
