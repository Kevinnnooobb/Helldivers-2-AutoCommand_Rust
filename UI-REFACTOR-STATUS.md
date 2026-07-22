# H2AC-RS UI 重构 — 技术状态文档

> 本文档记录 HD2 沉浸式 HUD 重构的设计决策、实施进度、已验证功能、未解决问题的完整调试证据。
> 供后续会话/代理接续工作。**最后更新：本次重构会话结束时。**

---

## 1. 设计决策（已与用户确认）

| 决策点 | 结论 |
|---|---|
| 美学方向 | 沉浸式 HD2 游戏 HUD 风（切角面板/角括号/扫描线/金黑配色） |
| 重构范围 | 完全重新设计（布局与交互均可推翻） |
| 使用场景 | 主界面配置台 + 紧凑模式（两者都要） |
| 战备图标 | 真实图标嵌入 exe（nvigneux/Helldivers-2-Stratagems-icons-svg，作者允许自由使用） |
| 字体 | Saira Condensed（拉丁/数字，内嵌）+ 系统微软雅黑（CJK 兜底） |
| 主界面布局 | 常显战备库（游戏配装界面风）：点槽位待命 → 点库条目装入，选择器弹窗已退役 |
| 紧凑模式 | 无边框横向迷你条（~554×56，置顶，点击执行，可拖动） |
| 配装交互 | 待命 + 自动推进：装入后自动跳到下一个空槽位待命，ESC 解除 |
| 搜索 | 分类侧栏 + 搜索框（中文名/型号子串匹配，跨分类实时过滤） |
| 装饰剂量 | 全套克制版：切角+角括号+低透明扫描线+状态灯脉冲+执行闪光（闪光兼具功能性） |
| 窗口 | 无边框自定义标题栏，主窗口固定 1100×640（`with_resizable(false)`） |
| 代码组织 | `theme / widgets / icons / main_view / compact_view` 模块拆分 |
| 依赖 | 仅新增 `image`（png feature）；`windows` 增 `Win32_System_SystemInformation` feature |
| 其他 | 彩色 emoji 全部移除，chrome 图标用 painter 自绘矢量字形；功能一个不删 |

---

## 2. 文件结构与职责

```
assets/
  icons/            106 个 256×256 PNG（resvg 离线转换，白/金/分类色，透明底）
  fonts/            SairaCondensed-Medium.ttf / SairaCondensed-Bold.ttf（OFL）
src/
  main.rs           应用状态 H2ACApp、模式切换 set_compact、日志 log、执行 execute_slot、
                    热键管理、panic hook（写 target/debug/panic.log）、窗口装配
  theme.rs          设计系统：配色常量（含分类色 category_color）、尺寸常量、
                    字体族 FAM_HUD/FAM_HUD_B（Saira→雅黑回退链）、install_fonts、apply_style
  widgets.rs        自绘组件：chamfer_points/paint_chamfer（左上+右下切角）、corner_brackets、
                    scanlines、paint_arrow/arrow_strip、status_lamp（脉冲）、
                    Glyph 枚举+paint_glyph+glyph_button、hud_button、hud_panel
  icons.rs          IconStore：75 个图标 include_bytes 嵌入 + image 解码 + 128px 纹理
  main_view.rs      主界面：render_topbar（可拖标题栏）、render_grid/render_slot_tile、
                    render_detail、render_library/render_library_row、render_bottombar、
                    render_context_menu / render_capture_modal / render_settings_modal
  compact_view.rs   紧凑条：10 槽位小图块 + 监听灯 + 还原按钮；compact_width()
  stratagems.rs     战备数据库（75 条，新增 icon 字段）、search()、get_by_category()
  config.rs         未改动（Config/Profile 持久化，SLOT_COUNT=10）
  hotkey.rs         未改动（全局键盘钩子）
  executor.rs       未改动（模拟按键执行）
```

---

## 3. 已完成的修复（本次会话）

1. **模块路径**：Rust 2018 兄弟模块引用全部改为 `crate::` 前缀。
2. **egui 0.31 API**：`ViewportCommand::Minimized/InnerSize(Vec2)/WindowLevel(WindowLevel)`；
   `windows 0.58` 的 `GetLocalTime()` 无参返回 SYSTEMTIME。
3. **预乘 alpha 非法值 ×4**（R/G/B 通道 > alpha 导致颜色爆表）：
   - `GOLD_GHOST` → `from_rgba_premultiplied(17, 14, 5, 18)`（待命槽位曾渲染成纯金块）
   - 主界面/紧凑条执行闪光 → `from_rgba_unmultiplied`
   - `status_lamp` 光环 → unmultiplied + 降低 alpha
   - `scanlines` → unmultiplied alpha=2（曾满屏亮线）
4. **布局定位 bug**：`hud_panel` 用 `allocate_exact_size` 依赖父 ui 光标，而 show_main 开头
   `advance_cursor_after_rect(full)` 把光标推到了末尾 → 详情条/战备库消失。
   修复：调用处先 `ui.new_child(max_rect)` 再调 hud_panel。
5. **紧凑条控制区溢出**：`horizontal` 默认 item_spacing.x=6 与手动 add_space 叠加，
   还原按钮被挤出 554px 窗口外（还原曾失效）→ `ui.spacing_mut().item_spacing.x = 0.0`。
6. **标题溢出**：顶栏内嵌套 vertical 布局溢出 48px → 改为 painter.text 绝对定位。
7. **搜索框放大镜定位**：改用 `ui.add(search)` 返回的 response.rect。

---

## 4. 已验证功能（截屏确认）

| 功能 | 验证方式 | 结果 |
|---|---|---|
| 主界面骨架 | 截屏 | ✅ 顶栏/网格/详情/战备库/日志栏全部就位 |
| 图标显示 | 截屏 | ✅ 槽位与库行均正确显示游戏图标（含分类色：红/青/绿/金） |
| 待命→装入→自动推进 | 模拟点击 | ✅ 点击槽位+库行，装入并推进到下一空槽，日志正确 |
| 双击执行 | 模拟双击 | ✅ 详情条更新（闪光未截到，衰减快） |
| 右键菜单 | 模拟右键 | ✅ 执行/清除/设热键三键面板弹出 |
| 搜索过滤 | 输入 "500" | ✅ 跨分类过滤出"飞鹰500KG炸弹·飞鹰" |
| 设置弹窗 | 截屏 | ✅ 方向键/激活键/延迟/保存取消正常渲染 |
| 紧凑↔主界面 | 两轮往返 | ✅ 1100×640 ↔ 554×56 双向切换、置顶切换正常 |
| 紧凑条渲染 | 截屏 | ✅ 图标、空槽序号、监听灯、还原按钮正常 |
| 崩溃排查 | panic hook | 早期一次无声崩溃未复现；panic.log 机制已就位 |

---

## 5. 【未解决】核心 bug：战备库分类栏点击不生效

### 现象
点击左侧分类行（任务/轨道/飞鹰/武器/哨戒/雷盾/背包/载具），`resp.clicked()` 不触发，
`lib_category` 不变，列表不切换。DBG 日志（已加在行点击 handler 里）从未出现。
**同一面板右侧的列表行点击正常**（装入功能就是靠它验证的）；**槽位块各种点击正常**。

### 已排除的假设（全部有实证）
1. **几何错位** — 给 interact rect 画红色半透明框（代码中标记 `// DBG`，未删除），
   红框与标签位置完全一致，点击坐标 (726,237) 确实落在"武器"行框内。排除。
2. **实现模式** — 两种写法都失败：
   - `new_child(max_rect)` + `.vertical()` + `allocate_painter(Sense::click)`
   - 绝对坐标 `ui.interact(row, id, Sense::click())`（与槽位块同款，当前代码）
   排除实现模式问题。
3. **egui 命中裁剪** — 读 egui 0.31.1 源码：
   - `Ui::interact`：`interact_rect = self.clip_rect().intersect(rect)`，负矩形会被 hit_test 跳过
   - `new_child`：clip 继承父 painter clip 不变（本例=全窗口）→ 行的 interact_rect=行 rect 为正
   排除裁剪。
4. **hit_test 顺序** — `find_closest_within`：同层 dist 相同取后注册者；行在面板之后注册，
   面板仅 `Sense::hover()` 不参与 click 过滤。理论上行应胜出。与观测矛盾。
5. **DPI 缩放** — 窗口物理尺寸=逻辑尺寸 1100×640，scale=1.0。排除。
6. **id 冲突** — 行 id 用 `ui.id().with(("cat", i))`，未见冲突证据（未彻底排除）。

### 关键证据（复现方法）
`main.rs` 的 `update()` 里留有 **临时调试代码**：
```rust
#[cfg(debug_assertions)]
ctx.set_debug_on_hover(true); // 临时：命中测试调试 — 修复后删除
```
启动后把光标悬停在分类行上 4 秒，egui 调试覆盖层显示**顶层 hover 部件是整个
404×524 面板**（即 `hud_panel` 内 `allocate_exact_size(size, Sense::hover())` 的分配），
而不是分类行。但分类行 Sense::click 包含 hover，且注册更晚，理论上应是顶层。

测试脚本在 `C:\Users\Rin\AppData\Local\Temp\opencode\`：
`hover_test.ps1`（悬停截屏）、`phase3_test.ps1`（分类+右键）、`full_test.ps1`（模式切换）、
`uitest.ps1`（装入流程）。均用 SetCursorPos+mouse_event 模拟输入、GetWindowRect+CopyFromScreen 截屏。

### 下一步排查建议（按优先级）
1. **对照实验**：把光标悬停在**列表行**（点击正常的行）上看 debug overlay 显示什么。
   - 若也显示"面板"→ overlay 读数误导，真问题在别处（重点查 clicked 判定）。
   - 若显示"列表行"→ 面板 hover 分配确实压制了分类行，转 2。
2. **去掉 hud_panel 的 Sense**：面板分配改 `Sense::hover()` → 不 sense 的方式
   （如拆成 `allocate_exact_size(size, Sense::focusable_noninteractive())` 或只用
   `advance_cursor_after_rect` + 手动 rect），验证分类行是否恢复。
3. **标准部件对照**：把分类行临时换成 `egui::Button`/SelectableLabel，若标准部件
   点击正常 → 问题在自绘/interact 注册环节；若也不正常 → 面板/区域结构问题。
4. **打印 contains_pointer**：临时在行处 `eprintln!` 输出
   `ctx.input(|i| i.pointer.interact_pos())` 与 `resp.hovered()`，确认 hover 状态。
5. **检查 id clash**：`create_widget` 里有 `check_for_id_clash`，看 stderr 是否有
   "id clash" 警告（`ui.id()` 的派生可能与兄弟 child 撞车）。

### 修复后必做清理
- 删除 `main_view.rs` 分类行的红色 DBG 填充行（`p.rect_filled(row, ..., 255,0,0,40) // DBG`）
- 删除分类行 handler 里的 DBG 日志（`DBG 分类切换 →`）
- 删除 `main.rs` update() 里的 `ctx.set_debug_on_hover(true)`
- `panic hook` 建议保留（对无声崩溃有用）

---

## 6. 其他未完成项（按优先级）

### 高
1. **详情条布局重叠**：大箭头串与"型号·分类"文字重叠（p2 截屏可见）。
   方案：固定区域划分 — 图标 96 | 文字列 190 | 箭头区（居中）| 按钮列 88，
   用 `ui.max_rect()` 显式算 Rect + `new_child` 分区，弃用 right_to_left 排布。
2. **分类切换验证**（依赖 bug 5 修复）：切到"武器"验证青色图标行、"哨戒"验证绿色行。
3. **全局热键→闪光联动验证**：给槽位绑热键（右键→设热键→按 5），按全局热键，
   确认槽位闪光 + 日志记录 + executor 正常（executor 会向系统注入按键，测试时注意焦点）。

### 中
4. **热键捕获弹窗视觉验证**：截屏确认键盘字形/确认取消按钮。
5. **监听开关在紧凑条的验证**：点击状态灯切换，确认热键监听启停。
6. **Profile 保存/加载/删除全流程**：底栏控件，含 ComboBox 下拉样式确认。

### 低
7. **release 构建**：`cargo build --release`（lto=fat 编译较慢），确认体积与运行。
8. **`.gitignore`**：评估是否加入 `panic.log`、`assets/`（或保留入库以便直接编译）。
9. **抛光项**：列表行长名称截断（fit_font 到最小字号后仍超宽时截断加省略号）；
   详情条空态文案；紧凑条热键角标在有绑定时的截屏确认。

---

## 7. 环境备忘

- 本机 GitHub 直连不稳定；raw.githubusercontent / cdn.jsdelivr.net 可直连；
  本地代理 127.0.0.1:7897 但对 curl schannel 不友好。图标/字体均经 jsdelivr 获取。
- SVG→PNG 转换器在 `C:\Users\Rin\AppData\Local\Temp\opencode\svg2png\`（resvg 0.45），
  可复跑：`cargo run --release -- <svg根目录> <输出目录>`（输出 256×256，文件名转 snake_case）。
- 测试截图与脚本均在 `C:\Users\Rin\AppData\Local\Temp\opencode\`。
- 调试构建每次约 2s（增量）；应用配置在 exe 旁 `target/debug/config.json`。
