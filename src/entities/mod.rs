//! Entity system — mirrors C++ EntityList + CEntityIdentity walk.
//!
//! In C++ entities are raw pointers into the target process.
//! Here they are plain usize addresses; all field access goes through Memory.

pub mod player;

use crate::math::Vec3;
use crate::memory::Memory;
use crate::schema::{SchemaOffsets, fnv1a_const};
use crate::globals::GameState;

// ─── Compile-time hashes for class name comparisons ──────────────────────────

const HASH_PLAYER_CONTROLLER: u64 = fnv1a_const("CCSPlayerController");

// ─── Entity type ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityType {
    Player,
    Unknown,
}

// ─── PlayerSnapshot — data cached per tick so the GUI never needs Memory ─────

#[derive(Clone, Debug)]
pub struct PlayerSnapshot {
    pub name:      String,
    pub health:    i32,
    pub team:      u8,
    pub armor:     i32,
    pub is_alive:  bool,
    pub is_local:  bool,
    pub is_scoped: bool,
    pub origin:          Vec3,
    pub head:            Vec3,
    pub eye_pos:         Vec3,
    pub velocity:        Vec3,
    pub game_scene_node: usize,
    pub pawn_addr:       usize,
    /// World-space positions for each bone index 0..SKELETON_BONE_COUNT.
    /// Zero vector means the bone wasn't readable.
    pub bones:      Vec<Vec3>,
    /// Entity index of the pawn (lower 15 bits of m_hPlayerPawn handle).
    /// This is what m_iIDEntIndex reports when the crosshair is on this player.
    pub pawn_index: i32,
}

// ─── EntityObject — mirrors C++ EntityObject_t ───────────────────────────────

#[derive(Clone, Debug)]
pub struct EntityObject {
    /// Remote address of CEntityInstance (== C_BaseEntity base).
    pub address: usize,
    /// Entity index (lower 15 bits of nIndex).
    pub index: i32,
    pub entity_type: EntityType,
    /// Populated only for `EntityType::Player` — None if pawn couldn't be resolved.
    pub player: Option<PlayerSnapshot>,
}

// ─── CEntityIdentity field offsets ───────────────────────────────────────────

const IDENTITY_INSTANCE_OFF:  usize = 0x000; // CEntityInstance* m_pInstance  (OFFSET macro)
const IDENTITY_INDEX_OFF:     usize = 0x010; // uint32_t nIndex               (OFFSET macro)
const ENT_ENTRY_MASK:         u32   = 0x7FFF;
const INVALID_EHANDLE_INDEX:  u32   = 0xFFFFFFFF;

// ─── Entity list update ───────────────────────────────────────────────────────

/// Walk the CEntityIdentity linked list and collect all known entity types.
/// Mirrors C++ `EntityList::UpdateEntities()`.
pub fn update_entities(
    mem: &Memory,
    state: &GameState,
    schema: &SchemaOffsets,
) -> Vec<EntityObject> {
    let mut entities = Vec::with_capacity(64);

    // Schema offset for CEntityIdentity->m_pNext (linked list pointer).
    let next_off = schema.get(fnv1a_const("CEntityIdentity->m_pNext")) as usize;
    if next_off == 0 {
        eprintln!("[entities] missing schema offset for CEntityIdentity->m_pNext");
        return entities;
    }

    let mut current = state.entity_system_first;

    while current != 0 {
        // Validity: check nIndex != INVALID_EHANDLE_INDEX.
        let raw_index = mem.read::<u32>(current + IDENTITY_INDEX_OFF);
        if raw_index == INVALID_EHANDLE_INDEX {
            current = mem.read::<usize>(current + next_off);
            continue;
        }

        let instance_addr = mem.read::<usize>(current + IDENTITY_INSTANCE_OFF);
        if instance_addr == 0 {
            current = mem.read::<usize>(current + next_off);
            continue;
        }

        // Read schema class name via pointer chain from CEntityInstance.
        // GetSchemaName: *(*(*(this+0x10)+0x8)+0x78)+0x8
        let name_ptr = mem.read_chain(instance_addr + 0x10, &[0x8, 0x78, 0x8]);
        if name_ptr == 0 {
            current = mem.read::<usize>(current + next_off);
            continue;
        }

        let schema_name = mem.read_string(name_ptr);
        let name_hash = crate::memory::fnv1a(&schema_name);

        let entity_type = match name_hash {
            HASH_PLAYER_CONTROLLER => EntityType::Player,
            _ => EntityType::Unknown,
        };

        if entity_type != EntityType::Unknown {
            let index = (raw_index & ENT_ENTRY_MASK) as i32;

            let player = if entity_type == EntityType::Player {
                build_player_snapshot(mem, instance_addr, state, schema)
            } else {
                None
            };

            entities.push(EntityObject { address: instance_addr, index, entity_type, player });
        }

        current = mem.read::<usize>(current + next_off);
    }

    entities
}

// ─── PlayerSnapshot builder ───────────────────────────────────────────────────

fn build_player_snapshot(
    mem: &Memory,
    controller_addr: usize,
    state: &GameState,
    schema: &SchemaOffsets,
) -> Option<PlayerSnapshot> {
    use player::{PlayerController, PlayerPawn};

    let ctrl = PlayerController::new(controller_addr, mem, schema);

    let pawn_handle = ctrl.cs_pawn_handle();
    let pawn_addr   = ctrl.pawn_addr(state);
    if pawn_addr == 0 {
        return None;
    }

    let pawn             = PlayerPawn::new(pawn_addr, mem, schema);
    let origin           = pawn.abs_origin();
    let game_scene_node  = pawn.game_scene_node_ptr();

    // Read bone positions for skeleton ESP.
    let mut bones = vec![Vec3::ZERO; crate::config::SKELETON_BONE_COUNT];
    let model_state_off = schema.get(fnv1a_const("CSkeletonInstance->m_modelState")) as usize;
    if model_state_off != 0 && game_scene_node != 0 {
        let bone_array = mem.read::<usize>(game_scene_node + model_state_off + 0x80);
        if bone_array != 0 {
            for i in 0..crate::config::SKELETON_BONE_COUNT {
                let pos = mem.read::<Vec3>(bone_array + i * 0x20);
                if pos != Vec3::ZERO {
                    bones[i] = pos;
                }
            }
        }
    }

    Some(PlayerSnapshot {
        name:            ctrl.player_name(),
        health:          pawn.health(),
        team:            pawn.team(),
        armor:           pawn.armor(),
        is_alive:        pawn.is_alive(),
        is_local:        controller_addr == state.local_controller_ptr,
        is_scoped:       pawn.is_scoped(),
        origin,
        head:            Vec3 { x: origin.x, y: origin.y, z: origin.z + 70.0 },
        eye_pos:         pawn.eye_position(),
        velocity:        pawn.velocity(),
        game_scene_node,
        pawn_addr,
        bones,
        pawn_index:      (pawn_handle & 0x7FFF) as i32,
    })
}

// ─── Index-based entity lookup (mirrors C_BaseEntity::GetBaseEntity) ──────────

/// Resolve a handle index to the remote address of a C_BaseEntity.
/// Mirrors C++ `C_BaseEntity::GetBaseEntity(nIdx)`.
pub fn get_base_entity(mem: &Memory, entity_list: usize, idx: i32) -> usize {
    if entity_list == 0 {
        return 0;
    }
    let list_entry = mem.read::<usize>(entity_list + 0x8 * ((idx as usize & 0x7FFF) >> 0x9) + 0x10);
    if list_entry == 0 {
        return 0;
    }
    mem.read::<usize>(list_entry + 0x70 * (idx as usize & 0x1FF))
}
