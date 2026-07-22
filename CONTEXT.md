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
| **Bottombar** | Bottom strip (52px): terminal-style log on the left, profile management (select/save/delete) on the right. |
| **Compact Bar** | A narrow always-on-top horizontal strip (554×56px) with 10 mini slot icons. Click to execute. Used during gameplay. |

## System Components

| Term | Definition |
|------|-----------|
| **Executor** | Module that simulates keyboard input via Windows `SendInput` API. Presses the stratagem activation key, then types the arrow sequence, then releases. |
| **Hotkey Listener** | Module that installs a `WH_KEYBOARD_LL` global hook. On keypress, checks against the hotkey map and sends the slot index to the main thread via a channel. |
| **Icon Store** | In-memory cache of 128×128 PNG textures for all stratagem icons, loaded at startup from embedded bytes. |
| **Plugin** | A JSON file in `plugins/` containing additional stratagems and/or themes. Loaded at startup and merged into the runtime data. |
| **Wiki Fetcher** | Background HTTP fetch that parses the Stratagem Hero Trainer JS data source and caches stratagem data locally as JSON. |
| **Plugin Creator** | Modal UI for creating plugin JSON files without manual editing. Has tabs for stratagem entry, theme entry, and Wiki fetch. |
| **Sequence Recorder** | Sub-component of the Plugin Creator that captures arrow key (↑↓←→) / WASD input to record a stratagem command sequence in real-time. |

## State Model Terms

| Term | Definition |
|------|-----------|
| **Config** | Persisted app settings: key bindings, stratagem key, delays, slot hotkeys, current loadout, listening state. |
| **Listening** | Whether the global hotkey hook is active. Toggled via the lamp in the topbar or compact bar. |
| **Flash** | Per-slot timestamp of the last execution. Drives a 0.7s gold decay animation on the slot tile. |
| **Armed** | The currently armed slot index, or None. Set by clicking a slot tile; cleared by ESC or after all slots are filled. |
