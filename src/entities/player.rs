//! Player-specific entity field accessors.
//!
//! In C++ these are methods on CCSPlayerController / C_CSPlayerPawn generated
//! by the SCHEMA macro. Here they're free functions that take an address +
//! schema ref and call mem.read() at the correct offset.

use crate::memory::Memory;
use crate::math::Vec3;
use crate::schema::{SchemaOffsets, fnv1a_const};
use crate::globals::GameState;
use crate::entities::{EntityObject, get_base_entity};

// ─── CBaseHandle helpers ──────────────────────────────────────────────────────

const INVALID_EHANDLE_INDEX: u32   = 0xFFFF_FFFF;
const ENT_ENTRY_MASK:         u32   = 0x7FFF;
const NUM_SERIAL_NUM_SHIFT:   u32   = 15;

pub fn handle_entry_index(handle: u32) -> i32 {
    if handle == INVALID_EHANDLE_INDEX { return -1; }
    (handle & ENT_ENTRY_MASK) as i32
}

pub fn handle_is_valid(handle: u32) -> bool {
    handle != INVALID_EHANDLE_INDEX
}

// ─── PlayerController — wraps a remote CCSPlayerController address ────────────

pub struct PlayerController<'a> {
    pub addr: usize,
    mem: &'a Memory,
    schema: &'a SchemaOffsets,
}

impl<'a> PlayerController<'a> {
    pub fn new(addr: usize, mem: &'a Memory, schema: &'a SchemaOffsets) -> Self {
        Self { addr, mem, schema }
    }

    fn schema_off(&self, key: u64) -> usize {
        self.schema.get(key) as usize
    }

    // CBasePlayerController->m_bIsLocalPlayerController
    pub fn is_local(&self) -> bool {
        let off = self.schema_off(fnv1a_const("CBasePlayerController->m_bIsLocalPlayerController"));
        self.mem.read::<bool>(self.addr + off)
    }

    // CBasePlayerController->m_hPawn  (CHandle<C_BasePlayerPawn>)
    pub fn pawn_handle(&self) -> u32 {
        let off = self.schema_off(fnv1a_const("CBasePlayerController->m_hPawn"));
        self.mem.read::<u32>(self.addr + off)
    }

    // CCSPlayerController->m_hPlayerPawn
    pub fn cs_pawn_handle(&self) -> u32 {
        let off = self.schema_off(fnv1a_const("CCSPlayerController->m_hPlayerPawn"));
        self.mem.read::<u32>(self.addr + off)
    }

    /// Resolve the pawn handle to a remote address using the entity list.
    pub fn pawn_addr(&self, state: &GameState) -> usize {
        let handle = self.cs_pawn_handle();
        if !handle_is_valid(handle) { return 0; }
        get_base_entity(self.mem, state.entity_list, handle_entry_index(handle))
    }

    /// Read the sanitized player name.
    pub fn player_name(&self) -> String {
        let off = self.schema_off(fnv1a_const("CCSPlayerController->m_sSanitizedPlayerName"));
        let name_ptr = self.mem.read::<u64>(self.addr + off);
        if name_ptr == 0 { return String::new(); }
        self.mem.read_string(name_ptr as usize)
    }
}

// ─── PlayerPawn — wraps a remote C_CSPlayerPawn address ──────────────────────

pub struct PlayerPawn<'a> {
    pub addr: usize,
    mem: &'a Memory,
    schema: &'a SchemaOffsets,
}

impl<'a> PlayerPawn<'a> {
    pub fn new(addr: usize, mem: &'a Memory, schema: &'a SchemaOffsets) -> Self {
        Self { addr, mem, schema }
    }

    fn schema_off(&self, key: u64) -> usize {
        self.schema.get(key) as usize
    }

    // C_BaseEntity->m_lifeState
    pub fn life_state(&self) -> u8 {
        let off = self.schema_off(fnv1a_const("C_BaseEntity->m_lifeState"));
        self.mem.read::<u8>(self.addr + off)
    }

    // C_BaseEntity->m_iHealth
    pub fn health(&self) -> i32 {
        let off = self.schema_off(fnv1a_const("C_BaseEntity->m_iHealth"));
        self.mem.read::<i32>(self.addr + off)
    }

    pub fn is_alive(&self) -> bool {
        self.life_state() == 0 || self.health() > 0
    }

    // C_BaseEntity->m_iTeamNum
    pub fn team(&self) -> u8 {
        let off = self.schema_off(fnv1a_const("C_BaseEntity->m_iTeamNum"));
        self.mem.read::<u8>(self.addr + off)
    }

    // C_CSPlayerPawn->m_ArmorValue
    pub fn armor(&self) -> i32 {
        let off = self.schema_off(fnv1a_const("C_CSPlayerPawn->m_ArmorValue"));
        self.mem.read::<i32>(self.addr + off)
    }

    // C_CSPlayerPawn->m_bIsScoped
    pub fn is_scoped(&self) -> bool {
        let off = self.schema_off(fnv1a_const("C_CSPlayerPawn->m_bIsScoped"));
        self.mem.read::<bool>(self.addr + off)
    }

    // C_BaseEntity->m_fFlags
    pub fn flags(&self) -> u32 {
        let off = self.schema_off(fnv1a_const("C_BaseEntity->m_fFlags"));
        self.mem.read::<u32>(self.addr + off)
    }

    // C_BaseEntity->m_pGameSceneNode  (pointer to CGameSceneNode)
    pub fn game_scene_node_ptr(&self) -> usize {
        let off = self.schema_off(fnv1a_const("C_BaseEntity->m_pGameSceneNode"));
        self.mem.read::<usize>(self.addr + off)
    }

    /// World-space origin from CGameSceneNode->m_vecAbsOrigin.
    pub fn abs_origin(&self) -> Vec3 {
        let node_ptr = self.game_scene_node_ptr();
        if node_ptr == 0 { return Vec3::ZERO; }
        let off = self.schema_off(fnv1a_const("CGameSceneNode->m_vecAbsOrigin"));
        self.mem.read::<Vec3>(node_ptr + off)
    }

    // C_BaseModelEntity->m_vecViewOffset
    pub fn view_offset(&self) -> Vec3 {
        let off = self.schema_off(fnv1a_const("C_BaseModelEntity->m_vecViewOffset"));
        self.mem.read::<Vec3>(self.addr + off)
    }

    /// Eye position = abs_origin + view_offset  (mirrors C++ GetEyePosition).
    pub fn eye_position(&self) -> Vec3 {
        self.abs_origin() + self.view_offset()
    }

    // C_BaseEntity->m_vecVelocity
    pub fn velocity(&self) -> Vec3 {
        let off = self.schema_off(fnv1a_const("C_BaseEntity->m_vecVelocity"));
        self.mem.read::<Vec3>(self.addr + off)
    }
}

// ─── Convenience builder from EntityObject ────────────────────────────────────

impl EntityObject {
    pub fn as_controller<'a>(
        &self,
        mem: &'a Memory,
        schema: &'a SchemaOffsets,
    ) -> PlayerController<'a> {
        PlayerController::new(self.address, mem, schema)
    }
}
