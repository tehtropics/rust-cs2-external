//! Aimbot + triggerbot — runs on the background thread every tick.
//!
//! Both features read from the already-populated `[EntityObject]` snapshot so
//! they never need their own memory reads beyond a few targeted lookups
//! (view-angle write for aimbot, crosshair-entity read for triggerbot).
//!
//! Aimbot approach (matches common high-starred CS2 externals):
//!   1. Find local eye position from cached snapshot.
//!   2. For every living enemy, compute angle + FOV distance.
//!   3. Smooth toward the closest target inside cfg.aimbot.fov.
//!   4. Write the new QAngle back to CCSGOInput::m_angViewAngle.
//!
//! Triggerbot approach:
//!   1. Read C_CSPlayerPawnBase->m_iIDEntIndex from the local pawn.
//!   2. If that entity is a living enemy, arm a timer.
//!   3. After cfg.trigger.delay_ms, hold LMB.
//!   4. Release as soon as the crosshair leaves the enemy.

use std::mem::size_of;
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput,
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSE_EVENT_FLAGS, MOUSEINPUT,
};

use crate::config::Config;
use crate::entities::{EntityObject, EntityType};
use crate::entities::player::PlayerController;
use crate::globals::{GameState, Offsets};
use crate::math::Vec3;
use crate::memory::Memory;
use crate::schema::{SchemaOffsets, fnv1a_const};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Persistent state for features that need to track across ticks.
pub struct FeatureState {
    trigger:   TriggerState,
    tick_n:    u64,
    old_punch: Vec3,
}

impl FeatureState {
    pub fn new() -> Self {
        Self { trigger: TriggerState::default(), tick_n: 0, old_punch: Vec3::ZERO }
    }

    /// Called once per background tick after entities have been updated.
    pub fn tick(
        &mut self,
        mem:      &Memory,
        offsets:  &Offsets,
        schema:   &SchemaOffsets,
        state:    &GameState,
        entities: &[EntityObject],
        cfg:      &Config,
    ) {
        if !state.is_in_game() {
            self.trigger.release_if_holding();
            self.old_punch = Vec3::ZERO;
            return;
        }
        let log = self.tick_n % 60 == 0;
        self.tick_n += 1;
        run_aimbot(mem, offsets, schema, state, entities, cfg, log);
        run_rcs(mem, offsets, schema, state, cfg, &mut self.old_punch);
        self.trigger.tick(mem, schema, state, entities, cfg, log);
    }
}

// ─── Aimbot ───────────────────────────────────────────────────────────────────
//
// Smoothed RCS-style angle write — same pattern seen in sAIMbot, ow-pasted, etc.
// FOV check prevents locking onto targets far from the crosshair.

fn run_aimbot(
    mem:      &Memory,
    offsets:  &Offsets,
    schema:   &SchemaOffsets,
    state:    &GameState,
    entities: &[EntityObject],
    cfg:      &Config,
    log:      bool,
) {
    if !cfg.aimbot.enabled { return; }
    // aim_key == 0 means always-on; otherwise the key must be held.
    if cfg.aimbot.aim_key != 0 {
        let held = unsafe { GetAsyncKeyState(cfg.aimbot.aim_key as i32) as u16 & 0x8000 != 0 };
        if !held { return; }
    }

    // Grab local player from the cached snapshot.
    let local = entities.iter()
        .filter_map(|e| e.player.as_ref())
        .find(|p| p.is_local);
    let Some(local_snap) = local else {
        eprintln!("[aimbot] no local player in entity snapshot (total entities: {})", entities.len());
        return;
    };

    if log {
        println!("[aimbot] local: alive={} team={} eye={:?} gsn=0x{:X}",
            local_snap.is_alive, local_snap.team, local_snap.eye_pos, local_snap.game_scene_node);
    }

    if !local_snap.is_alive { return; }

    let eye        = local_snap.eye_pos;
    let local_team = local_snap.team;

    // Use the pawn's v_angle for FOV selection — this reflects the player's
    // actual mouse direction and is not polluted by our CCSGOInput writes.
    let v_angle_off = schema.get(fnv1a_const("C_BasePlayerPawn->v_angle")) as usize;
    let view = if v_angle_off != 0 && local_snap.pawn_addr != 0 {
        mem.read::<Vec3>(local_snap.pawn_addr + v_angle_off)
    } else {
        state.view_angle
    };

    let input_ptr = mem.read::<usize>(offsets.csgo_input);
    if log { println!("[aimbot] csgo_input ptr=0x{:X} v_angle={:?}", input_ptr, view); }

    // Find the enemy closest to the crosshair within the configured FOV.
    let mut best_fov   = cfg.aimbot.fov;
    let mut best_angle: Option<Vec3> = None;

    let enemies: Vec<_> = entities.iter()
        .filter(|e| e.entity_type == EntityType::Player)
        .filter_map(|e| e.player.as_ref().map(|p| (e, p)))
        .filter(|(_, p)| !p.is_local && p.is_alive && p.team != local_team)
        .collect();

    if log { println!("[aimbot] eligible enemies: {}", enemies.len()); }

    for (_, snap) in &enemies {
        let bone = get_bone_pos(mem, schema, snap.game_scene_node, cfg.aimbot.target_bone as usize);
        if log {
            println!("[aimbot]   enemy gsn=0x{:X} bone={:?}", snap.game_scene_node, bone);
        }
        let Some(target_pos) = bone else { continue };

        let angle_to = world_to_angles(target_pos - eye);
        let fov      = angle_fov(view, angle_to);

        if fov < best_fov {
            best_fov   = fov;
            best_angle = Some(angle_to);
        }
    }

    if log { println!("[aimbot] best_angle={:?} fov_limit={}", best_angle, cfg.aimbot.fov); }

    let Some(target) = best_angle else { return };

    let smooth = cfg.aimbot.smooth.max(1.0);
    let mut new_angle = view;
    new_angle.x += angle_delta(target.x, view.x) / smooth;
    new_angle.y += angle_delta(target.y, view.y) / smooth;
    clamp_angle(&mut new_angle);

    if input_ptr != 0 {
        if log { println!("[aimbot] writing angle {:?} to 0x{:X}+0x688", new_angle, input_ptr); }
        mem.write::<Vec3>(input_ptr + 0x688, new_angle);
    } else {
        eprintln!("[aimbot] csgo_input ptr is 0 — cannot write angle");
    }
}

// ─── RCS ─────────────────────────────────────────────────────────────────────
//
// Each tick:
//   1. Read m_iShotsFired — if == 0 the gun is idle, reset old_punch and bail.
//   2. Read m_aimPunchAngle (the raw punch the engine applies).
//      CS2 visually doubles the punch, so full compensation = punch_delta * 2.
//   3. Subtract the per-tick punch delta (scaled) from the current view angle.
//   4. Store current punch for next tick.

fn run_rcs(
    mem:       &Memory,
    offsets:   &Offsets,
    schema:    &SchemaOffsets,
    state:     &GameState,
    cfg:       &Config,
    old_punch: &mut Vec3,
) {
    if !cfg.aimbot.rcs_enabled { return; }

    // Resolve local pawn.
    if state.local_controller_ptr == 0 { return; }
    let ctrl       = crate::entities::player::PlayerController::new(state.local_controller_ptr, mem, schema);
    let local_pawn = ctrl.pawn_addr(state);
    if local_pawn == 0 { return; }

    // Check shots fired — reset when not shooting.
    let shots_off = schema.get(crate::schema::fnv1a_const("C_CSPlayerPawn->m_iShotsFired")) as usize;
    if shots_off == 0 { return; }
    let shots_fired: i32 = mem.read(local_pawn + shots_off);
    if shots_fired <= 1 {
        *old_punch = Vec3::ZERO;
        return;
    }

    let punch_off = schema.get(crate::schema::fnv1a_const("C_CSPlayerPawn->m_aimPunchAngle")) as usize;
    if punch_off == 0 { return; }
    let punch: Vec3 = mem.read(local_pawn + punch_off);

    // Per-tick delta * 2 (engine doubles the punch visually).
    let delta_x = (punch.x - old_punch.x) * 2.0 * cfg.aimbot.rcs_scale;
    let delta_y = (punch.y - old_punch.y) * 2.0 * cfg.aimbot.rcs_scale;
    *old_punch = punch;

    if delta_x == 0.0 && delta_y == 0.0 { return; }

    let input_ptr = mem.read::<usize>(offsets.csgo_input);
    if input_ptr == 0 { return; }

    let mut view: Vec3 = mem.read(input_ptr + 0x688);
    view.x -= delta_x;
    view.y -= delta_y;
    clamp_angle(&mut view);
    mem.write::<Vec3>(input_ptr + 0x688, view);
}

// ─── Triggerbot ───────────────────────────────────────────────────────────────
//
// Reads m_iIDEntIndex to know what the crosshair is over.
// Arms a per-tick timer; presses LMB once the delay elapses.
// Holds LMB for as long as the crosshair stays on an enemy.

#[derive(Default)]
struct TriggerState {
    /// When armed: the Instant at which we should start pressing.
    fire_at: Option<Instant>,
    /// Whether we are currently holding LMB down.
    holding: bool,
}

impl TriggerState {
    fn release_if_holding(&mut self) {
        if self.holding {
            mouse_event(MOUSEEVENTF_LEFTUP);
            self.holding = false;
        }
        self.fire_at = None;
    }

    fn tick(
        &mut self,
        mem:      &Memory,
        schema:   &SchemaOffsets,
        state:    &GameState,
        entities: &[EntityObject],
        cfg:      &Config,
        log:      bool,
    ) {
        if !cfg.trigger.enabled {
            self.release_if_holding();
            return;
        }

        let on_enemy = crosshair_on_enemy(mem, schema, state, entities, log);

        if on_enemy {
            // Arm if not already armed and not already holding.
            if self.fire_at.is_none() && !self.holding {
                self.fire_at = Some(
                    Instant::now() + Duration::from_millis(cfg.trigger.delay_ms as u64)
                );
            }

            // Press once the delay elapses.
            if let Some(t) = self.fire_at {
                if Instant::now() >= t {
                    mouse_event(MOUSEEVENTF_LEFTDOWN);
                    self.holding  = true;
                    self.fire_at  = None;
                }
            }
        } else {
            // Crosshair left enemy — cancel arm + release.
            self.fire_at = None;
            self.release_if_holding();
        }
    }
}

// ─── Crosshair entity check ───────────────────────────────────────────────────

fn crosshair_on_enemy(
    mem:      &Memory,
    schema:   &SchemaOffsets,
    state:    &GameState,
    entities: &[EntityObject],
    log:      bool,
) -> bool {
    if state.local_controller_ptr == 0 {
        if log { eprintln!("[trigger] local_controller_ptr is 0"); }
        return false;
    }

    // Resolve local pawn from controller handle.
    let ctrl       = PlayerController::new(state.local_controller_ptr, mem, schema);
    let local_pawn = ctrl.pawn_addr(state);
    if log { println!("[trigger] ctrl=0x{:X} pawn=0x{:X}", state.local_controller_ptr, local_pawn); }
    if local_pawn == 0 { return false; }

    // Local team from snapshot.
    let local_team = entities.iter()
        .filter_map(|e| e.player.as_ref())
        .find(|p| p.is_local)
        .map(|p| p.team)
        .unwrap_or(0);

    let id_off = schema.get(fnv1a_const("C_CSPlayerPawn->m_iIDEntIndex")) as usize;
    if id_off == 0 { return false; }

    let crosshair_idx: i32 = mem.read(local_pawn + id_off);
    if log { println!("[trigger] crosshair_idx={} local_team={}", crosshair_idx, local_team); }
    if crosshair_idx <= 0 { return false; }

    // crosshair_idx is the pawn entity index — compare against pawn_index, not controller index.
    let result = entities.iter()
        .filter_map(|e| e.player.as_ref())
        .find(|p| p.pawn_index == crosshair_idx)
        .map(|snap| snap.is_alive && snap.team != local_team)
        .unwrap_or(false);

    if log { println!("[trigger] on_enemy={}", result); }
    result
}

// ─── Mouse helpers ────────────────────────────────────────────────────────────

fn mouse_event(flags: MOUSE_EVENT_FLAGS) {
    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], size_of::<INPUT>() as i32);
    }
}

// ─── Bone position reading ────────────────────────────────────────────────────
//
// Layout (matches FullyExternalCS2 / aci1337/CS2-External and most high-starred repos):
//
//   CGameSceneNode (= CSkeletonInstance base)
//     + 0x170  →  CModelState  (inline struct, not a pointer)
//       + 0x80  →  bone_array_ptr  (usize pointer to heap-allocated array)
//          + bone_index * 0x20  →  { x: f32, y: f32, z: f32, ... }
//
// Bone indices for CS2 player models (approximate, model-dependent):
//   0  = pelvis/root       5  = neck
//   1-4 = spine            6  = head   ← default cfg.aimbot.target_bone
//

fn get_bone_pos(mem: &Memory, schema: &SchemaOffsets, game_scene_node: usize, bone_index: usize) -> Option<Vec3> {
    if game_scene_node == 0 { return None; }
    let model_state_off = schema.get(fnv1a_const("CSkeletonInstance->m_modelState")) as usize;
    if model_state_off == 0 {
        schema.dump_class("CSkeletonInstance");
        return None;
    }
    // CModelState bone array pointer is not exposed in schemasystem — offset 0x80 is hardcoded.
    let bone_array = mem.read::<usize>(game_scene_node + model_state_off + 0x80);
    if bone_array == 0 {
        eprintln!("[bone] bone_array is 0 (gsn=0x{:X} model_state_off=0x{:X})", game_scene_node, model_state_off);
        return None;
    }
    let pos = mem.read::<Vec3>(bone_array + bone_index * 0x20);
    if pos == Vec3::ZERO { None } else { Some(pos) }
}

// ─── Angle math ───────────────────────────────────────────────────────────────

/// World-space displacement → QAngle (pitch, yaw, 0).
fn world_to_angles(delta: Vec3) -> Vec3 {
    let yaw   = delta.y.atan2(delta.x).to_degrees();
    let horiz = (delta.x * delta.x + delta.y * delta.y).sqrt();
    let pitch = (-delta.z).atan2(horiz).to_degrees();
    Vec3 { x: pitch, y: yaw, z: 0.0 }
}

/// Shortest signed delta between two angles, normalized to [-180, 180].
fn angle_delta(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d >  180.0 { d -= 360.0; }
    while d < -180.0 { d += 360.0; }
    d
}

/// Angular distance between two QAngles (pitch + yaw only) — used for FOV check.
fn angle_fov(view: Vec3, target: Vec3) -> f32 {
    let dp = angle_delta(target.x, view.x);
    let dy = angle_delta(target.y, view.y);
    (dp * dp + dy * dy).sqrt()
}

/// Clamp to valid CS2 view angle range.
fn clamp_angle(a: &mut Vec3) {
    a.x = a.x.clamp(-89.0, 89.0);
    while a.y >  180.0 { a.y -= 360.0; }
    while a.y < -180.0 { a.y += 360.0; }
    a.z = 0.0;
}
