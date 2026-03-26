mod config;
mod entities;
mod features;
mod globals;
mod gui;
mod math;
mod memory;
mod overlay;
mod schema;

use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use config::Config;
use entities::{update_entities, EntityType};
use features::FeatureState;
use globals::{GameState, Offsets, NAVSYSTEM_DLL};
use gui::{App, SharedState};
use memory::Memory;
use schema::SchemaOffsets;


// ─── Background thread ────────────────────────────────────────────────────────
//
// Mirrors C++ UpdateThread + EntityThread collapsed into one thread.
// Runs independently of the egui event loop.

fn background_thread(
    entities:   Arc<RwLock<Vec<entities::EntityObject>>>,
    game_state: Arc<RwLock<GameState>>,
    config:     Arc<RwLock<Config>>,
) {
    // ── Attach ────────────────────────────────────────────────────────────────
    println!("[bg] attaching to cs2.exe...");
    let mut mem = Memory::new();
    mem.initialize("cs2.exe");
    println!("[bg] attached.");

    // Wait for last module (navsystem.dll).
    loop {
        if mem.get_module(NAVSYSTEM_DLL).is_some() { break; }
        println!("[bg] waiting for navsystem.dll...");
        thread::sleep(Duration::from_millis(500));
    }
    println!("[bg] all modules loaded.");

    // ── One-time setup — scan live game memory directly ──────────────────────
    println!("[bg] scanning offsets from live game memory...");
    let offsets = match Offsets::setup(&mut mem) {
        Some(o) => o,
        None => { eprintln!("[bg] offset scan failed — exiting background thread."); return; }
    };
    let schema = match SchemaOffsets::setup(&mut mem) {
        Some(s) => s,
        None => { eprintln!("[bg] schema scan failed — exiting background thread."); return; }
    };

    println!("[bg] ready. schema offsets: {}", schema.len());
    schema.check_required();

    let mut features = FeatureState::new();

    loop {
        // Update game state (GlobalVars, signon, view matrix, entity system ptr).
        let state = GameState::update(&mem, &offsets, &schema);
        let in_game = state.is_in_game();

        *game_state.write().unwrap() = state;

        if !in_game {
            thread::sleep(Duration::from_millis(3000));
            continue;
        }

        // Update entity list.
        let state_snap = game_state.read().unwrap().clone();
        let new_entities = update_entities(&mem, &state_snap, &schema);

        // Log player count for debugging.
        let player_count = new_entities.iter().filter(|e| e.entity_type == EntityType::Player).count();
        println!("[bg] tick — {} players", player_count);

        // Run features (aimbot + triggerbot) with a snapshot of the config.
        {
            let cfg = config.read().unwrap().clone();
            features.tick(&mem, &offsets, &schema, &state_snap, &new_entities, &cfg);
        }

        *entities.write().unwrap() = new_entities;
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() {
    // Shared state between background thread and UI.
    let entities   = Arc::new(RwLock::new(Vec::new()));
    let game_state = Arc::new(RwLock::new(GameState::default()));
    let config     = Arc::new(RwLock::new(
        config::load("default").unwrap_or_default()
    ));

    // Spawn background memory thread.
    {
        let e = entities.clone();
        let g = game_state.clone();
        let c = config.clone();
        thread::spawn(move || background_thread(e, g, c));
    }

    // Spawn GDI overlay thread (transparent always-on-top Win32 window).
    overlay::run(entities.clone(), game_state.clone(), config.clone());

    // Menu window — plain opaque window, always-on-top so it stays above the game.
    // The transparent ESP overlay is a *secondary* viewport spawned per-frame by
    // gui::App::draw_overlay() using ctx.show_viewport_immediate().
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([830.0, 560.0])
            .with_resizable(false)
            .with_window_level(egui::viewport::WindowLevel::AlwaysOnTop)
            .with_title("CS2 External"),
        ..Default::default()
    };

    let shared = SharedState {
        entities:   entities.clone(),
        game_state: game_state.clone(),
        config:     config.clone(),
    };

    eframe::run_native(
        "External Base",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, shared)))),
    )
    .expect("eframe failed to start");

    // Save config on exit.
    if let Ok(cfg) = config.read() {
        config::save(&cfg, "default");
    }
}
