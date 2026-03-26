# rust-cs2-external

An external CS2 cheat written in Rust. Runs as a separate process, reads and writes game memory via the Windows API, and renders a transparent GDI overlay on top of the game window.

## Features

### ESP
- **Bounding boxes** — 2D boxes drawn around enemy/teammate players
- **Health bars** — vertical bar indicating current HP
- **Player names** — drawn above the bounding box
- **Skeleton ESP** — bone connections drawn over player models, configurable colors for enemies and teammates
- **Team filter** — option to skip rendering teammates

### Aimbot
- **FOV-based target selection** — locks onto the enemy closest to your crosshair within a configurable FOV
- **Smoothing** — configurable interpolation speed to reduce snapping
- **Configurable aim key** — any virtual key (mouse buttons, keyboard keys), set via the GUI binding button
- **Configurable bone target** — Head, Neck, Chest, Stomach, or Pelvis
- **FOV circle** — optional on-screen circle visualising the current aimbot FOV

### Triggerbot
- **Auto-fire** — shoots when crosshair is over an enemy
- **Configurable delay** — pre/post shot delay in milliseconds

### RCS (Recoil Control System)
- **Punch-angle compensation** — reads `m_aimPunchAngle` delta each tick and counteracts it
- **Configurable scale** — 0.0–1.0, step 0.01

## Architecture

| Module | Responsibility |
|---|---|
| `memory` | `OpenProcess` / `ReadProcessMemory` / `WriteProcessMemory` wrapper |
| `schema` | Live in-process schema dump — walks `schemasystem.dll` type scopes at startup, no external JSON |
| `globals` | Single-tick game state snapshot (local player, view matrix, view angle) |
| `entities` | Entity list walk, `PlayerSnapshot` construction including bone reads |
| `features` | Aimbot, triggerbot, RCS logic |
| `overlay` | Transparent layered window, GDI double-buffered rendering |
| `gui` | egui settings panel (config read/write via `Arc<RwLock<Config>>`) |
| `config` | `serde` JSON config persisted to `%APPDATA%\cs2_tool\config.json` |

**No external offset files.** Offsets are resolved at startup via pattern scanning (`Offsets::setup`) and live schema walking (`SchemaOffsets::setup`). The tool will log `[schema] OK` / `[schema] MISS` for every required field at launch.

## Requirements

- Windows 10/11
- Rust stable (tested on 1.78+)
- CS2 running in a supported window mode (windowed or borderless windowed recommended for overlay visibility)
- Run as **Administrator** — required for `OpenProcess` with `PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION`

## Build

```bash
cargo build --release
```

The binary will be at `target/release/untitled.exe`.

## Usage

1. Launch CS2
2. Run the tool as Administrator
3. The overlay will appear over the game window
4. Press the GUI toggle key (default: **Insert**) to open the settings panel
5. Configure features and save — settings persist across restarts

## Configuration

Config is saved automatically to `%APPDATA%\cs2_tool\config.json`. Notable options:

| Key | Default | Description |
|---|---|---|
| `aimbot.enabled` | `false` | Master aimbot toggle |
| `aimbot.fov` | `5.0` | Aim FOV in degrees |
| `aimbot.smooth` | `5.0` | Smoothing factor (1 = instant) |
| `aimbot.aim_key` | `0xA0` (LShift) | Virtual key code to activate aimbot |
| `aimbot.bone_target` | `"Head"` | Bone to aim at |
| `aimbot.rcs_enabled` | `false` | Recoil control toggle |
| `aimbot.rcs_scale` | `1.0` | RCS compensation strength (0.0–1.0) |
| `visuals.boxes` | `true` | Bounding box ESP |
| `visuals.skeletons` | `false` | Skeleton ESP |
| `visuals.fov_circle` | `false` | Aimbot FOV circle overlay |
| `visuals.skeleton_enemy_color` | red | RGBA skeleton color for enemies |
| `visuals.skeleton_team_color` | green | RGBA skeleton color for teammates |

## Disclaimer

This project is for **educational purposes only**. Using cheats in online games violates the game's terms of service and may result in a permanent ban. The authors take no responsibility for any consequences of using this software.
