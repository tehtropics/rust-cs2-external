//! Mirrors C++ CGlobals + CInterfaces + CGlobalVars layout.
//!
//! `Offsets`    — pattern-scanned addresses, set up once at startup.
//! `GameState`  — runtime snapshot updated every tick (replaces g_Globals + g_Interfaces).

use crate::math::{Vec3, ViewMatrix};
use crate::memory::Memory;
use crate::schema::SchemaOffsets;

// ─── Game module names ────────────────────────────────────────────────────────

pub const CLIENT_DLL: &str      = "client.dll";
pub const ENGINE2_DLL: &str     = "engine2.dll";
pub const SCHEMASYSTEM_DLL: &str = "schemasystem.dll";
pub const NAVSYSTEM_DLL: &str   = "navsystem.dll";


// ─── CGlobalVars — repr(C) copy read from the game ───────────────────────────
//
// Offsets verified against C++ struct with MEM_PAD macros.

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GlobalVars {
    pub real_time: f32,        // 0x0000
    pub frame_count: i32,      // 0x0004
    pub frame_time: f32,       // 0x0008
    pub frame_time2: f32,      // 0x000C
    pub max_clients: i32,      // 0x0010
    _pad0: [u8; 0x1C],         // 0x0014
    pub current_time: f32,     // 0x0030
    pub frame_time3: f32,      // 0x0034
    pub tick_fraction: f32,    // 0x0038
    pub tick_fraction2: f32,   // 0x003C
    _pad1: [u8; 0x8],          // 0x0040
    pub tick_count: i32,       // 0x0048
    _pad2: [u8; 0xC],          // 0x004C
    _pad3: [u8; 0x8],          // 0x0058
    _pad4: [u8; 0x118],        // 0x0060
    pub map_name: u64,         // 0x0178 — pointer to map name string
    pub map_name_short: u64,   // 0x0180 — pointer to short map name string
}

// repr(C) structs with large pad arrays can't derive Default (stable Rust only
// auto-impls Default for arrays up to len 32). Use zeroed() instead.
impl Default for GlobalVars {
    fn default() -> Self { unsafe { std::mem::zeroed() } }
}

/// QAngle — same memory layout as Vec3 (3 floats: pitch, yaw, roll).
pub type QAngle = Vec3;

// CCSGOInput: only field we use is m_angViewAngle at 0x688.
// Read it directly instead of copying the whole 0x688+ byte struct.

// CNetworkGameClient: signon_state at 0x230.

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SignonState {
    #[default]
    None = 0,
    Challenge    = 1,
    Connected    = 2,
    New          = 3,
    Prespawn     = 4,
    Spawn        = 5,
    Full         = 6,
    Changelevel  = 7,
}

impl SignonState {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::Challenge,
            2 => Self::Connected,
            3 => Self::New,
            4 => Self::Prespawn,
            5 => Self::Spawn,
            6 => Self::Full,
            7 => Self::Changelevel,
            _ => Self::None,
        }
    }

    pub fn is_in_game(self) -> bool { self == Self::Full }
    pub fn is_connected(self) -> bool { (self as i32) >= (Self::Connected as i32) }
    pub fn is_changing_level(self) -> bool { self == Self::Changelevel }
}

// ─── Offsets — all pattern-scanned once at startup ────────────────────────────

#[derive(Default, Debug)]
pub struct Offsets {
    // ── Data pointers (fatal — setup fails if any of these miss) ─────────────
    // client.dll
    pub entity_list:             usize,
    pub view_matrix:             usize,
    pub local_player_controller: usize,
    pub planted_c4:              usize,
    pub auto_accept_array:       usize,
    pub global_vars:             usize,
    pub csgo_input:              usize,
    pub entity_system:           usize,
    pub sensitivity:             usize,
    // engine2.dll
    pub network_game_client:     usize,

    // ── Additional data pointers (non-fatal — 0 if pattern not found) ────────
    pub swapchain:               usize, // IDXGISwapChain** — engine2.dll
    pub fn_get_view_angles:      usize, // data ptr resolved from GetViewAngles LEA

    // ── Function addresses (non-fatal) — useful for hook detection ────────────
    // client.dll
    pub fn_set_view_angles:      usize,
    pub fn_get_entity_index:     usize,
    pub fn_construct_input_data: usize,
    pub fn_input_update:         usize, // "poo" in referenced source
    pub fn_automake_user_cmd:    usize,
    pub fn_set_mesh_group_mask:  usize,
    pub fn_get_local_player_idx: usize,
    pub fn_get_local_pawn:       usize,
    pub fn_get_base_entity:      usize,
    pub fn_get_matrix_for_view:  usize,
    pub fn_screen_transform:     usize,
    pub fn_create_move:          usize,
    // engine2.dll
    pub fn_get_is_in_game:       usize,
    pub fn_get_is_connected:     usize,
}

impl Offsets {
    /// Run all pattern scans. Mirrors C++ `CGlobals::Update()` first call.
    pub fn setup(mem: &mut Memory) -> Option<Self> {
        // Fatal — whole setup returns None if any miss.
        macro_rules! scan_rip {
            ($module:expr, $sig:expr) => {
                mem.pattern_scan_rip($module, $sig, 0x3, 0x7)?
            };
        }
        // Non-fatal RIP-relative resolve (standard rva=3, rip=7).
        macro_rules! try_rip {
            ($module:expr, $sig:expr) => {
                mem.pattern_scan_rip($module, $sig, 0x3, 0x7).unwrap_or(0)
            };
            ($module:expr, $sig:expr, $rva:expr, $rip:expr) => {
                mem.pattern_scan_rip($module, $sig, $rva, $rip).unwrap_or(0)
            };
        }
        // Non-fatal function-start scan (returns virtual address of match).
        macro_rules! try_fn {
            ($module:expr, $sig:expr) => {
                mem.pattern_scan($module, $sig).unwrap_or(0)
            };
        }

        let o = Self {
            // ── Fatal data pointers ───────────────────────────────────────────
            entity_list:             scan_rip!(CLIENT_DLL,  "48 8B 0D ? ? ? ? 48 89 7C 24 ?? 8B FA C1 EB"),
            view_matrix:             scan_rip!(CLIENT_DLL,  "48 8D 0D ? ? ? ? 48 C1 E0 06"),
            local_player_controller: scan_rip!(CLIENT_DLL,  "48 8B 05 ? ? ? ? 41 89 BE"),
            planted_c4:              scan_rip!(CLIENT_DLL,  "4C 8B 0D ? ? ? ? 8B C8 4A 39 3C 09"),
            auto_accept_array:       scan_rip!(CLIENT_DLL,  "48 89 05 ? ? ? ? E8 ? ? ? ? 48 85 DB"),
            global_vars:             scan_rip!(CLIENT_DLL,  "48 89 15 ? ? ? ? 48 89 42"),
            csgo_input:              scan_rip!(CLIENT_DLL,  "48 8B 0D ? ? ? ? 4C 8B C6 8B 10 E8"),
            entity_system:           scan_rip!(CLIENT_DLL,  "48 8B 0D ? ? ? ? 8B D3 E8 ? ? ? ? 48 8B F0"),
            sensitivity:             mem.pattern_scan_rip(CLIENT_DLL, "48 8D 0D ? ? ? ? 66 0F 6E CD", 0x3, 0x7)? + 0x8,
            network_game_client:     scan_rip!(ENGINE2_DLL, "48 89 3D ? ? ? ? FF 87"),

            // ── Non-fatal data pointers ───────────────────────────────────────
            // engine2.dll — IDXGISwapChain*: "48 89 2D ? ? ? ? 66 0F 7F 05 ? ? ? ?"
            swapchain:               try_rip!(ENGINE2_DLL, "48 89 2D ? ? ? ? 66 0F 7F 05 ? ? ? ?"),
            // GetViewAngles in client.dll ends with LEA RAX,[RIP+?] (rva=10, rip=14)
            fn_get_view_angles:      try_rip!(CLIENT_DLL,  "4C 8B C1 85 D2 74 ? 48 8D 05", 0xA, 0xE),

            // ── Non-fatal function addresses ──────────────────────────────────
            // SetViewAngles — mid-function pattern, match addr IS the instruction
            fn_set_view_angles:      try_fn!(CLIENT_DLL, "85 D2 75 ? 48 63 81"),
            // GetEntityIndex — call-relative E8 (rva=1, rip=5)
            fn_get_entity_index:     try_rip!(CLIENT_DLL, "E8 ? ? ? ? 8B 8D ? ? ? ? 8D 51", 0x1, 0x5),
            // ConstructInputData — call-relative E8
            fn_construct_input_data: try_rip!(CLIENT_DLL, "E8 ? ? ? ? 48 8B CF 48 8B F0 44 8B B0 10 59 00 00", 0x1, 0x5),
            // Input update ("poo") — E8 is at byte 7 within the pattern (rva=8, rip=12)
            fn_input_update:         try_rip!(CLIENT_DLL, "48 8B 0D ? ? ? ? E8 ? ? ? ? 48 8B CF 48 8B F0 44 8B B0 10 59 00 00", 0x8, 0xC),
            // AutoMakeUserCmd — call-relative E8
            fn_automake_user_cmd:    try_rip!(CLIENT_DLL, "E8 ? ? ? ? 48 89 44 24 40 48 8D 4D F0", 0x1, 0x5),
            // SetMeshGroupMask — function prologue
            fn_set_mesh_group_mask:  try_fn!(CLIENT_DLL, "48 89 5C 24 ? 48 89 74 24 ? 57 48 83 EC ? 48 8D 99 ? ? ? ? 48 8B 71"),
            // GetLocalPlayerIndex — function prologue
            fn_get_local_player_idx: try_fn!(CLIENT_DLL, "40 53 48 83 EC ? 48 8B 05 ? ? ? ? 48 8D 0D ? ? ? ? 48 8B DA FF 90 ? ? ? ? 48 8B C3 48 83 C4 ? 5B C3"),
            // GetLocalPawn — function prologue
            fn_get_local_pawn:       try_fn!(CLIENT_DLL, "40 53 48 83 EC ? 33 C9 E8 ? ? ? ? 48 8B D8 48 85 C0 74 ? 48 8B 00 48 8B CB FF 90 ? ? ? ? 84 C0 74 ? 48 8B C3"),
            // GetBaseEntity — entity-index to pointer translation stub
            fn_get_base_entity:      try_fn!(CLIENT_DLL, "4C 8D 49 10 81 FA ? ? 00 00 77 ? 8B CA C1 F9 09"),
            // GetMatrixForView — function prologue
            fn_get_matrix_for_view:  try_fn!(CLIENT_DLL, "48 8B C4 48 89 68 ? 48 89 70 ? 57 48 81 EC ? ? ? ? 0F 29 70 ? 49 8B F1"),
            // ScreenTransform — function prologue
            fn_screen_transform:     try_fn!(CLIENT_DLL, "48 89 5C 24 08 57 48 83 EC ? 48 83 3D ? ? ? ? ? 48 8B DA"),
            // CreateMove — function prologue
            fn_create_move:          try_fn!(CLIENT_DLL, "48 8B C4 4C 89 40 ? 48 89 48 ? 55 53 41 54"),
            // GetIsInGame / GetIsConnected — engine2.dll NetworkGameClient checks
            fn_get_is_in_game:       try_rip!(ENGINE2_DLL, "48 8B ? ? ? ? ? 48 85 C0 74 15 80 B8 ? ? ? ? ? 75 0C 83 B8 ? ? ? ? 06"),
            fn_get_is_connected:     try_rip!(ENGINE2_DLL, "48 8B 05 ? ? ? ? 48 85 C0 74 ? 80 B8 ? ? ? ? ? 75 ? 83 B8 ? ? ? ? ? 7C"),
        };

        println!("[offsets] entity_list             @ 0x{:X}", o.entity_list);
        println!("[offsets] view_matrix             @ 0x{:X}", o.view_matrix);
        println!("[offsets] local_player_controller @ 0x{:X}", o.local_player_controller);
        println!("[offsets] entity_system           @ 0x{:X}", o.entity_system);
        println!("[offsets] csgo_input              @ 0x{:X}", o.csgo_input);
        println!("[offsets] swapchain               @ 0x{:X}", o.swapchain);
        println!("[offsets] fn_get_view_angles      @ 0x{:X}", o.fn_get_view_angles);
        println!("[offsets] fn_set_view_angles      @ 0x{:X}", o.fn_set_view_angles);
        println!("[offsets] fn_construct_input_data @ 0x{:X}", o.fn_construct_input_data);
        println!("[offsets] fn_input_update         @ 0x{:X}", o.fn_input_update);
        println!("[offsets] fn_create_move          @ 0x{:X}", o.fn_create_move);
        println!("[offsets] fn_screen_transform     @ 0x{:X}", o.fn_screen_transform);
        println!("[offsets] fn_get_local_pawn       @ 0x{:X}", o.fn_get_local_pawn);

        Some(o)
    }
}

// ─── GameState — runtime snapshot updated every tick ─────────────────────────

#[derive(Default, Debug, Clone)]
pub struct GameState {
    pub global_vars:             GlobalVars,
    pub signon_state:            SignonState,
    pub view_angle:              QAngle,        // from CCSGOInput
    pub view_matrix:             ViewMatrix,
    pub entity_list:             usize,         // base of entity list
    pub entity_system_first:     usize,         // CEntityIdentity* m_pFirst
    pub local_controller_ptr:    usize,         // remote ptr to local CCSPlayerController
    pub local_pawn_ptr:          usize,         // derived from local controller's m_hPawn
    pub map_name:                String,
}

impl GameState {
    pub fn is_in_game(&self) -> bool {
        self.signon_state.is_in_game()
    }

    pub fn is_connected(&self) -> bool {
        self.signon_state.is_connected()
    }

    /// Update all runtime state. Mirrors C++ `CGlobals::Update()` + `CInterfaces::Update()`.
    pub fn update(mem: &Memory, offsets: &Offsets, schema: &SchemaOffsets) -> Self {
        let mut state = Self::default();

        // ── GlobalVars ──────────────────────────────────────────────────────
        let gv_ptr = mem.read::<usize>(offsets.global_vars);
        if gv_ptr != 0 {
            state.global_vars = mem.read::<GlobalVars>(gv_ptr);

            if state.global_vars.map_name_short != 0 {
                state.map_name = mem.read_string(state.global_vars.map_name_short as usize);
            }
        }

        // ── NetworkGameClient → signon state ────────────────────────────────
        let ngc_ptr = mem.read::<usize>(offsets.network_game_client);
        if ngc_ptr != 0 {
            let signon = mem.read::<i32>(ngc_ptr + 0x230);
            state.signon_state = SignonState::from_i32(signon);
        }

        // ── CCSGOInput → view angle ─────────────────────────────────────────
        // CCSGOInput is not registered in schemasystem — offset is hardcoded.
        let input_ptr = mem.read::<usize>(offsets.csgo_input);
        if input_ptr != 0 {
            state.view_angle = mem.read::<QAngle>(input_ptr + 0x688);
        }

        // ── Entity list / entity system / view matrix / local player ────────
        state.entity_list          = mem.read::<usize>(offsets.entity_list);
        state.view_matrix          = mem.read::<ViewMatrix>(offsets.view_matrix);
        state.local_controller_ptr = mem.read::<usize>(offsets.local_player_controller);

        let entity_system_ptr = mem.read::<usize>(offsets.entity_system);
        if entity_system_ptr != 0 {
            // CGameEntitySystem::m_pFirst at +0x210
            state.entity_system_first = mem.read::<usize>(entity_system_ptr + 0x210);
        }

        state
    }
}
