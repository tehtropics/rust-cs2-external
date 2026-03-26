//! Standalone Win32 GDI overlay window.
//!
//! Runs on its own thread.  Creates a `WS_EX_LAYERED | WS_EX_TRANSPARENT |
//! WS_EX_TOPMOST | WS_EX_NOACTIVATE` popup window that covers the entire
//! primary monitor, then uses `SetLayeredWindowAttributes(LWA_COLORKEY)` with
//! magenta (RGB 255,0,255) as the transparent colour.
//!
//! GDI renders exact integer colours with no gamma or anti-aliasing rounding,
//! so the colour-key pixels are always a perfect match — unlike egui's GPU
//! path which may produce off-by-one values after sRGB conversion.

use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, Ellipse, FillRect, GetDC, LineTo, MoveToEx, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, TextOutW,
    BACKGROUND_MODE, GET_STOCK_OBJECT_FLAGS, HBITMAP, HBRUSH, HGDIOBJ, PEN_STYLE, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetSystemMetrics,
    PeekMessageW, PostQuitMessage, RegisterClassExW, SetLayeredWindowAttributes,
    SetWindowLongPtrW, ShowWindow, GWLP_USERDATA, LWA_COLORKEY, MSG, PM_REMOVE,
    SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, WM_DESTROY, WNDCLASSEXW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};

use crate::config::Config;
use crate::entities::{EntityObject, EntityType};
use crate::globals::GameState;

// ─── Colour key ──────────────────────────────────────────────────────────────
// Magenta: R=255, G=0, B=255.
// COLORREF format is 0x00BBGGRR, so magenta = 0x00FF00FF.

const CK: u32 = 0x00FF00FF;

fn colorref(r: u8, g: u8, b: u8) -> u32 {
    (b as u32) << 16 | (g as u32) << 8 | r as u32
}

// ─── Window proc ─────────────────────────────────────────────────────────────

unsafe extern "system" fn wnd_proc(
    hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM,
) -> LRESULT {
    if msg == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// ─── Drawing ─────────────────────────────────────────────────────────────────

unsafe fn draw(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    sw: i32, sh: i32,
    entities: &[EntityObject],
    state: &GameState,
    cfg: &Config,
) {
    // Fill background with colour key → becomes transparent.
    let ck_brush = CreateSolidBrush(COLORREF(CK));
    let full = RECT { left: 0, top: 0, right: sw, bottom: sh };
    FillRect(hdc, &full, ck_brush);
    DeleteObject(HGDIOBJ(ck_brush.0));

    if !cfg.visuals.enabled || !state.is_in_game() {
        return;
    }

    let sw_f = sw as f32;
    let sh_f = sh as f32;

    let [br, bg, bb, _] = cfg.visuals.box_color;
    let [nr, ng, nb, _] = cfg.visuals.name_color;
    let [ser, seg, seb, _] = cfg.visuals.skeleton_enemy_color;
    let [str_, stg, stb, _] = cfg.visuals.skeleton_team_color;

    // Transparent text background so names don't have a filled rect.
    SetBkMode(hdc, BACKGROUND_MODE(1)); // 1 = TRANSPARENT
    SetTextColor(hdc, COLORREF(colorref(nr, ng, nb)));

    let box_pen = CreatePen(PEN_STYLE(0), 1, COLORREF(colorref(br, bg, bb))); // PS_SOLID=0
    // NULL_BRUSH stock object (index 5) so Rectangle() doesn't fill.
    let null_brush = windows::Win32::Graphics::Gdi::GetStockObject(
        GET_STOCK_OBJECT_FLAGS(5), // NULL_BRUSH
    );

    for entity in entities {
        if entity.entity_type != EntityType::Player { continue; }
        let Some(snap) = entity.player.as_ref() else { continue };
        if !snap.is_alive { continue; }
        if cfg.visuals.team_check && snap.is_local { continue; }

        let Some((fx, fy)) = state.view_matrix.world_to_screen(snap.origin, sw_f, sh_f) else { continue };
        let Some((hx, hy)) = state.view_matrix.world_to_screen(snap.head,   sw_f, sh_f) else { continue };

        if fx < 0.0 || fx > sw_f || fy < 0.0 || fy > sh_f { continue; }

        let box_h = fy - hy;
        let box_w = box_h * 0.4;

        let left   = (hx - box_w / 2.0) as i32;
        let right  = (hx + box_w / 2.0) as i32;
        let top    = hy as i32;
        let bottom = fy as i32;

        // ── Bounding box ──────────────────────────────────────────────────────
        if cfg.visuals.boxes {
            let prev_pen   = SelectObject(hdc, HGDIOBJ(box_pen.0));
            let prev_brush = SelectObject(hdc, null_brush);
            windows::Win32::Graphics::Gdi::Rectangle(hdc, left, top, right, bottom);
            SelectObject(hdc, prev_pen);
            SelectObject(hdc, prev_brush);
        }

        // ── Health bar ────────────────────────────────────────────────────────
        if cfg.visuals.health_bar {
            let hp_frac = (snap.health.clamp(0, 100) as f32) / 100.0;
            let bar_l   = left - 5;
            let bar_r   = left - 2;

            // Dark background strip.
            let bg_rect  = RECT { left: bar_l, top, right: bar_r, bottom };
            let bg_brush: HBRUSH = CreateSolidBrush(COLORREF(colorref(20, 20, 20)));
            FillRect(hdc, &bg_rect, bg_brush);
            DeleteObject(HGDIOBJ(bg_brush.0));

            // Coloured fill (green → red).
            let fill_top = top + ((box_h * (1.0 - hp_frac)) as i32);
            let r = ((1.0 - hp_frac) * 220.0) as u8;
            let g = (hp_frac * 200.0) as u8;
            let hp_rect  = RECT { left: bar_l, top: fill_top, right: bar_r, bottom };
            let hp_brush: HBRUSH = CreateSolidBrush(COLORREF(colorref(r, g, 20)));
            FillRect(hdc, &hp_rect, hp_brush);
            DeleteObject(HGDIOBJ(hp_brush.0));
        }

        // ── Name tag ─────────────────────────────────────────────────────────
        if cfg.visuals.names {
            let label: Vec<u16> = if snap.name.is_empty() {
                format!("[{}]", entity.index).encode_utf16().collect()
            } else {
                snap.name.encode_utf16().collect()
            };
            let tx = (hx as i32).saturating_sub(label.len() as i32 * 3);
            let _ = TextOutW(hdc, tx, top - 14, &label);
        }

        // ── Skeleton ─────────────────────────────────────────────────────────
        if cfg.visuals.skeletons {
            let local_team = entities.iter()
                .filter_map(|e| e.player.as_ref())
                .find(|p| p.is_local)
                .map(|p| p.team)
                .unwrap_or(0);
            let (sr, sg, sb) = if snap.is_local || snap.team == local_team {
                (str_, stg, stb)
            } else {
                (ser, seg, seb)
            };

            let skel_pen  = CreatePen(PEN_STYLE(0), 1, COLORREF(colorref(sr, sg, sb)));
            let prev_pen  = SelectObject(hdc, HGDIOBJ(skel_pen.0));

            for &(a, b) in crate::config::SKELETON_CONNECTIONS {
                let pa = snap.bones.get(a).copied().unwrap_or(crate::math::Vec3::ZERO);
                let pb = snap.bones.get(b).copied().unwrap_or(crate::math::Vec3::ZERO);
                if pa == crate::math::Vec3::ZERO || pb == crate::math::Vec3::ZERO { continue; }

                let Some((ax, ay)) = state.view_matrix.world_to_screen(pa, sw_f, sh_f) else { continue };
                let Some((bx, by)) = state.view_matrix.world_to_screen(pb, sw_f, sh_f) else { continue };

                MoveToEx(hdc, ax as i32, ay as i32, None);
                let _ = LineTo(hdc, bx as i32, by as i32);
            }

            SelectObject(hdc, prev_pen);
            DeleteObject(HGDIOBJ(skel_pen.0));
        }
    }

    DeleteObject(HGDIOBJ(box_pen.0));

    // ── Aimbot FOV circle ─────────────────────────────────────────────────────
    // Convert FOV degrees to pixels using tangent projection.
    // CS2 h-fov at 16:9 is ~106°, so half-fov at screen edge = tan(53°)*half_width.
    // radius = tan(fov_deg/2) / tan(h_fov/2) * (screen_width/2)
    if cfg.visuals.fov_circle && cfg.aimbot.enabled {
        let cx = sw / 2;
        let cy = sh / 2;
        let half_hfov_tan = (106.26_f32 / 2.0).to_radians().tan();
        let radius = ((cfg.aimbot.fov / 2.0).to_radians().tan() / half_hfov_tan * (sw_f / 2.0)) as i32;

        let fov_pen   = CreatePen(PEN_STYLE(0), 1, COLORREF(colorref(255, 255, 255)));
        let null_brush = windows::Win32::Graphics::Gdi::GetStockObject(GET_STOCK_OBJECT_FLAGS(5));
        let prev_pen   = SelectObject(hdc, HGDIOBJ(fov_pen.0));
        let prev_brush = SelectObject(hdc, null_brush);
        Ellipse(hdc, cx - radius, cy - radius, cx + radius, cy + radius);
        SelectObject(hdc, prev_pen);
        SelectObject(hdc, prev_brush);
        DeleteObject(HGDIOBJ(fov_pen.0));
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn run(
    entities:   Arc<RwLock<Vec<EntityObject>>>,
    game_state: Arc<RwLock<GameState>>,
    config:     Arc<RwLock<Config>>,
) {
    thread::spawn(move || unsafe {
        let hmod = GetModuleHandleW(None).expect("GetModuleHandleW failed");
        let hinst = HINSTANCE(hmod.0);

        let class_name = windows::core::w!("ESP_OVL");
        let wc = WNDCLASSEXW {
            cbSize:        std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpszClassName: class_name,
            lpfnWndProc:   Some(wnd_proc),
            hInstance:     hinst,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class_name,
            windows::core::w!(""),
            WS_POPUP | WS_VISIBLE,
            0, 0, sw, sh,
            None, None, hinst, None,
        ).expect("CreateWindowExW failed");

        // Magenta pixels → transparent, everything else drawn normally.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(CK), 0, LWA_COLORKEY);

        // Dummy userdata write (keeps the GWLP_USERDATA slot initialised).
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = ShowWindow(hwnd, SW_SHOW);

        // ── Main loop: double-buffered GDI render ~60 fps ─────────────────────
        let mut msg = MSG::default();
        loop {
            // Drain message queue.
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == 0x0012 { return; } // WM_QUIT
                DispatchMessageW(&msg);
            }

            // Snapshot shared state (keep lock time minimal).
            let state    = game_state.read().unwrap().clone();
            let ents     = entities.read().unwrap().clone();
            let cfg      = config.read().unwrap().clone();

            // Double-buffer: draw into off-screen DC then blit to window.
            let hdc      = GetDC(hwnd);
            let mem_dc   = CreateCompatibleDC(hdc);
            let mem_bmp: HBITMAP = CreateCompatibleBitmap(hdc, sw, sh);
            let old_obj  = SelectObject(mem_dc, HGDIOBJ(mem_bmp.0));

            draw(mem_dc, sw, sh, &ents, &state, &cfg);

            BitBlt(hdc, 0, 0, sw, sh, mem_dc, 0, 0, SRCCOPY).unwrap_or(());

            SelectObject(mem_dc, old_obj);
            DeleteObject(HGDIOBJ(mem_bmp.0));
            let _ = DeleteDC(mem_dc);
            ReleaseDC(hwnd, hdc);

            thread::sleep(Duration::from_millis(16)); // ~60 fps
        }
    });
}
