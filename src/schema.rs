//! Schema system — mirrors C++ SchemaSystem::Setup().
//!
//! Walks schemasystem.dll's type scopes to build a map of
//!   FNV1A("ClassName->fieldName") → byte offset
//! which is then used by entity field accessors everywhere.

use std::collections::HashMap;
use crate::memory::{Memory, fnv1a};

// ─── Compile-time FNV-1a for const schema keys ───────────────────────────────

pub const fn fnv1a_const(s: &str) -> u64 {
    const BASIS: u64 = 0xCBF29CE484222325;
    const PRIME: u64 = 0x100000001B3;
    let bytes = s.as_bytes();
    let mut hash = BASIS;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

// ─── Raw layout constants (offsets within target-process structs) ─────────────

// CSchemaSystem
const SCHEMA_SYSTEM_SCOPE_SIZE_OFF: usize   = 0x190; // i32
const SCHEMA_SYSTEM_SCOPE_ARRAY_OFF: usize  = 0x198; // ptr → ptr[]

// CSchemaSystemTypeScope
const TYPE_SCOPE_NAME_OFF: usize             = 0x008; // char[256]
const TYPE_SCOPE_NUM_CLASSES_OFF: usize      = 0x470; // u16
const TYPE_SCOPE_CLASSES_PTR_OFF: usize      = 0x478; // ptr → CSchemaDeclaredClassEntry[]

// CSchemaDeclaredClassEntry  (24 bytes each)
const ENTRY_DECLARED_CLASS_PTR_OFF: usize    = 0x010; // ptr → CSchemaDeclaredClass
const ENTRY_SIZE: usize                      = 0x018;

// CSchemaDeclaredClass
const DECLARED_CLASS_NAME_PTR_OFF: usize     = 0x008; // ptr → const char*
const DECLARED_CLASS_CLASS_PTR_OFF: usize    = 0x020; // ptr → CSchemaClass

// CSchemaClass
const SCHEMA_CLASS_NUM_FIELDS_OFF: usize     = 0x01C; // u16
const SCHEMA_CLASS_FIELDS_PTR_OFF: usize     = 0x028; // ptr → CSchemaField[]

// CSchemaField  (32 bytes each)
const SCHEMA_FIELD_NAME_PTR_OFF: usize       = 0x000; // ptr → const char*
const SCHEMA_FIELD_TYPE_PTR_OFF: usize       = 0x008; // ptr → void (null check only)
const SCHEMA_FIELD_OFFSET_OFF: usize         = 0x010; // u32
const SCHEMA_FIELD_SIZE: usize               = 0x020;

// ─── Public API ──────────────────────────────────────────────────────────────

pub struct SchemaOffsets {
    map:     HashMap<u64, u32>,
    str_map: HashMap<String, u32>,
}

impl SchemaOffsets {
    /// Look up a field offset by its pre-hashed key.
    #[inline]
    pub fn get(&self, hash: u64) -> u32 {
        self.map.get(&hash).copied().unwrap_or(0)
    }

    /// Look up by raw string (runtime hash). Prefer the const hash variant.
    #[inline]
    pub fn get_str(&self, key: &str) -> u32 {
        self.get(fnv1a(key))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Check and log all field lookups the features depend on.
    /// Call once at startup so missing offsets are visible immediately.
    pub fn check_required(&self) {
        let fields = [
            "CEntityIdentity->m_pNext",
            "CBasePlayerController->m_hPawn",
            "CCSPlayerController->m_hPlayerPawn",
            "CCSPlayerController->m_sSanitizedPlayerName",
            "C_BaseEntity->m_iHealth",
            "C_BaseEntity->m_iTeamNum",
            "C_BaseEntity->m_lifeState",
            "C_BaseEntity->m_pGameSceneNode",
            "C_BaseEntity->m_vecVelocity",
            "CGameSceneNode->m_vecAbsOrigin",
            "C_BaseModelEntity->m_vecViewOffset",
            "C_BasePlayerPawn->v_angle",
            "CSkeletonInstance->m_modelState",
            "C_CSPlayerPawn->m_iIDEntIndex",
            "C_CSPlayerPawn->m_ArmorValue",
            "C_CSPlayerPawn->m_bIsScoped",
            "C_CSPlayerPawn->m_aimPunchAngle",
            "C_CSPlayerPawn->m_iShotsFired",
        ];
        println!("[schema] required field check:");
        for key in &fields {
            let off = self.get_str(key);
            if off == 0 {
                eprintln!("[schema]   MISS  {}", key);
            } else {
                println!("[schema]   OK    {:<60} 0x{:X}", key, off);
            }
        }
    }

    /// Print all known fields for `class_name` — call this when a lookup misses
    /// to discover the actual registered name.
    pub fn dump_class(&self, class_name: &str) {
        let prefix = format!("{}->", class_name);
        let mut fields: Vec<(&str, u32)> = self.str_map.iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, &v)| (k.as_str(), v))
            .collect();
        if fields.is_empty() {
            eprintln!("[schema] class '{}' — not found in schema (no fields registered)", class_name);
        } else {
            fields.sort_by_key(|&(_, off)| off);
            eprintln!("[schema] class '{}' fields:", class_name);
            for (key, off) in fields {
                eprintln!("  {:<60} 0x{:X}", key, off);
            }
        }
    }

    /// Parse schemasystem.dll type scopes and collect all client.dll offsets.
    /// Mirrors C++ `SchemaSystem::Setup()`.
    pub fn setup(mem: &mut Memory) -> Option<Self> {
        // Signature scans schemasystem.dll for the CSchemaSystem instance pointer.
        let schema_iface = mem.pattern_scan_rip(
            "schemasystem.dll",
            "48 89 05 ? ? ? ? 4C 8D 0D ? ? ? ? 33 C0 48 C7 05 ? ? ? ? ? ? ? ? 89 05",
            0x3,
            0x7,
        )?;

        let scope_size = mem.read::<i32>(schema_iface + SCHEMA_SYSTEM_SCOPE_SIZE_OFF);
        let scope_array_ptr = mem.read::<usize>(schema_iface + SCHEMA_SYSTEM_SCOPE_ARRAY_OFF);

        if scope_size <= 0 || scope_array_ptr == 0 {
            eprintln!("[schema] invalid scope array");
            return None;
        }

        // Read array of scope pointers (void*[scope_size]).
        let mut scope_ptrs = vec![0usize; scope_size as usize];
        mem.read_raw(scope_array_ptr, unsafe {
            std::slice::from_raw_parts_mut(
                scope_ptrs.as_mut_ptr() as *mut u8,
                scope_size as usize * 8,
            )
        });

        let mut map:     HashMap<u64, u32>     = HashMap::new();
        let mut str_map: HashMap<String, u32> = HashMap::new();

        for &scope_ptr in &scope_ptrs {
            if scope_ptr == 0 {
                continue;
            }

            // Read scope name (inline char[256] at +0x8).
            let scope_name = mem.read_string(scope_ptr + TYPE_SCOPE_NAME_OFF);
            if scope_name != "client.dll" {
                continue;
            }

            let num_classes = mem.read::<u16>(scope_ptr + TYPE_SCOPE_NUM_CLASSES_OFF);
            let classes_ptr = mem.read::<usize>(scope_ptr + TYPE_SCOPE_CLASSES_PTR_OFF);
            if classes_ptr == 0 || num_classes == 0 {
                continue;
            }

            parse_scope(mem, classes_ptr, num_classes, &mut map, &mut str_map);
            break; // only need client.dll
        }

        if map.is_empty() {
            eprintln!("[schema] no offsets collected");
            return None;
        }

        println!("[schema] loaded {} offsets", map.len());
        Some(Self { map, str_map })
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn parse_scope(
    mem: &Memory,
    classes_ptr: usize,
    num_classes: u16,
    map:     &mut HashMap<u64, u32>,
    str_map: &mut HashMap<String, u32>,
) {
    for j in 0..num_classes as usize {
        let entry_ptr = classes_ptr + j * ENTRY_SIZE;

        let declared_class_ptr = mem.read::<usize>(entry_ptr + ENTRY_DECLARED_CLASS_PTR_OFF);
        if declared_class_ptr == 0 {
            continue;
        }

        let class_name_ptr = mem.read::<usize>(declared_class_ptr + DECLARED_CLASS_NAME_PTR_OFF);
        let schema_class_ptr = mem.read::<usize>(declared_class_ptr + DECLARED_CLASS_CLASS_PTR_OFF);
        if class_name_ptr == 0 || schema_class_ptr == 0 {
            continue;
        }

        let class_name = mem.read_string(class_name_ptr);
        if class_name.is_empty() {
            continue;
        }

        let num_fields = mem.read::<u16>(schema_class_ptr + SCHEMA_CLASS_NUM_FIELDS_OFF);
        let fields_ptr = mem.read::<usize>(schema_class_ptr + SCHEMA_CLASS_FIELDS_PTR_OFF);
        if fields_ptr == 0 || num_fields == 0 {
            continue;
        }

        for k in 0..num_fields as usize {
            let field_ptr = fields_ptr + k * SCHEMA_FIELD_SIZE;

            let type_ptr = mem.read::<usize>(field_ptr + SCHEMA_FIELD_TYPE_PTR_OFF);
            if type_ptr == 0 {
                continue;
            }

            let field_name_ptr = mem.read::<usize>(field_ptr + SCHEMA_FIELD_NAME_PTR_OFF);
            if field_name_ptr == 0 {
                continue;
            }

            let field_name = mem.read_string(field_name_ptr);
            if field_name.is_empty() {
                continue;
            }

            let offset = mem.read::<u32>(field_ptr + SCHEMA_FIELD_OFFSET_OFF);

            let key = format!("{}->{}", class_name, field_name);
            map.insert(fnv1a(&key), offset);
            str_map.insert(key, offset);
        }
    }
}
