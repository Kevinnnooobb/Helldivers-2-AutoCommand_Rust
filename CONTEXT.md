# H2AC-RS — CONTEXT.md

> Domain glossary for architecture discussions. Use these terms exactly.

## Core Domain

| Term | Definition |
|------|-----------|
| **Stratagem** | A call-in request in Helldivers 2, identified by an arrow sequence and invoked via keyboard input. Has a name, model (e.g. "MG-43"), category, and icon. |
| **Slot** | One of 10 fixed positions in the loadout grid. A slot can be empty or hold a Stratagem reference. |
| **Loadout** | The 10-slot configuration of stratagems, equivalent to the game's pre-mission loadout screen. |
| **Arm** | The act of selecting a slot to prepare it for assignment. The armed slot is highlighted with a gold border. |
| **Assign** | The act of mapping a stratagem from the library to an armed slot. Auto-advances to the next empty slot. |
| **Execute** | Sending a stratagem's arrow sequence to the game via SendInput. The app does NOT interact with the game directly — it types the keys into whatever window has focus. |
| **Command** | The directional sequence of a stratagem, e.g. [↑, →, ↓, ↓, ↓] for Eagle 500KG. |
| **Hotkey** | A single keyboard key bound to a specific slot. Pressing the hotkey anywhere triggers execution of that slot's stratagem. |
| **Profile** | A named saved loadout (slot contents + hotkey bindings), stored as JSON. |

## UI Anatomy

| Term | Definition |
|------|-----------|
| **Topbar** | Custom title bar (48px): app branding, drag region, listening lamp toggle, gear/compact/minimize/close buttons. |
| **Grid** | 5-column × 2-row layout of slot tiles (124×150px each). Displays stratagem icon, name, command arrows, and hotkey badge. |
| **Detail Panel** | Bottom-left panel showing the selected slot's full stratagem info: large icon, name, model, description, command arrows, and action buttons. |
| **Library** | Right-side panel showing all stratagems, organized by category rail + search box + scrollable list. Clicking a stratagem row assigns it to the armed slot. |
| **Bottombar** | Bottom strip (52px): terminal-style log on the left, profile management (select/save/load/delete) on the right. |
| **Compact Bar** | A narrow always-on-top horizontal strip (554×56px) with 10 mini slot icons. Click to execute. Used during gameplay. |

## System Components

| Term | Definition |
|------|-----------|
| **Executor** | Module that simulates keyboard input via Windows `SendInput` API. The core function is `execute_command(config, directions)`. |
| **Hotkey Listener** | Module that installs a `WH_KEYBOARD_LL` global hook. |
| **Icon Store** | In-memory cache of 128×128 PNG textures, loaded from embedded bytes + runtime disk scan. |
| **Plugin** | A JSON file in `plugins/` containing additional stratagems and/or themes. |
| **Wiki Fetcher** | Background HTTP fetch + hand-rolled JS parser for Stratagem Hero Trainer data. |
| **Drag Handle** | A `≡` glyph on each library row. Dragging picks up the stratagem; releasing over a slot assigns it; releasing over a category rail changes category. |
| **Scale Factor** | `AppModel.scale` — uniform multiplier (0.7–2.0) for all painter coordinates, computed from window size vs 1100×640 reference. |

## State Model Terms

| Term | Definition |
|------|-----------|
| **Config** | Persisted app settings: key bindings, delays, pre_delay, slot hotkeys, loadout, category_overrides. |
| **Listening** | Whether the global hotkey hook is active. |
| **Flash** | Per-slot timestamp of last execution; drives 0.7s gold decay animation. |
| **Armed** | The currently armed slot index, or None. |
| **Scale** | `AppModel.scale` — multiplier for proportional window resizing. |
