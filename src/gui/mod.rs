//! egui front-end.
//!
//! Two windows:
//!   • Main window   — the menu (normal, opaque, interactive, always-on-top).
//!   • Overlay viewport — fullscreen transparent, always-on-top, draws ESP only.
//!     `with_mouse_passthrough(true)` tells winit to set WS_EX_TRANSPARENT so
//!     all clicks/keys pass through to the game automatically.  No Win32 hacks
//!     required.

use std::sync::{Arc, RwLock};

use eframe::egui::{self, Color32, Vec2};

use crate::config::{self, Config};
use crate::entities::{EntityObject, EntityType};
use crate::globals::GameState;

// ─── Shared state ─────────────────────────────────────────────────────────────

pub struct SharedState {
    pub entities:   Arc<RwLock<Vec<EntityObject>>>,
    pub game_state: Arc<RwLock<GameState>>,
    pub config:     Arc<RwLock<Config>>,
}

// ─── App ─────────────────────────────────────────────────────────────────────

pub struct App {
    shared:          SharedState,
    current_tab:     usize,
    config_files:    Vec<String>,
    selected_config: i32,
    config_name_buf: String,
    binding_aim_key: bool,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>, shared: SharedState) -> Self {
        Self {
            shared,
            current_tab:     0,
            config_files:    config::list_files(),
            selected_config: -1,
            config_name_buf: String::new(),
            binding_aim_key: false,
        }
    }

    // ── Menu tabs ─────────────────────────────────────────────────────────────

    fn tab_aimbot(&mut self, ui: &mut egui::Ui) {
        // ── Key binding capture (runs every frame while waiting) ──────────────
        if self.binding_aim_key {
            // Scan all VK codes for the first one held down (skip unbound=0).
            let pressed = (1u32..=254).find(|&vk| unsafe {
                windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) as u16
                    & 0x8000 != 0
            });
            if let Some(vk) = pressed {
                self.shared.config.write().unwrap().aimbot.aim_key = vk;
                self.binding_aim_key = false;
            }
            ui.ctx().request_repaint();
        }

        let mut cfg = self.shared.config.write().unwrap();
        ui.checkbox(&mut cfg.aimbot.enabled, "Enable aimbot");
        ui.add_enabled_ui(cfg.aimbot.enabled, |ui| {
            ui.add(egui::Slider::new(&mut cfg.aimbot.fov, 0.5..=45.0).step_by(0.5).text("FOV"));
            ui.add(egui::Slider::new(&mut cfg.aimbot.smooth, 1.0..=20.0).text("Smooth"));
            ui.horizontal(|ui| {
                ui.label("Bone:");
                let current_label = crate::config::BONE_TARGETS.iter()
                    .find(|&&(_, idx)| idx == cfg.aimbot.target_bone)
                    .map(|&(name, _)| name)
                    .unwrap_or("Custom");
                egui::ComboBox::from_id_salt("bone_target")
                    .selected_text(current_label)
                    .show_ui(ui, |ui| {
                        for &(name, idx) in crate::config::BONE_TARGETS {
                            ui.selectable_value(&mut cfg.aimbot.target_bone, idx, name);
                        }
                    });
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Aim key:");
                let btn_label = if self.binding_aim_key {
                    "Press any key...".to_string()
                } else if cfg.aimbot.aim_key == 0 {
                    "Always on".to_string()
                } else {
                    vk_name(cfg.aimbot.aim_key)
                };
                if ui.button(&btn_label).clicked() && !self.binding_aim_key {
                    self.binding_aim_key = true;
                }
                if ui.small_button("✕").on_hover_text("Clear (always-on)").clicked() {
                    cfg.aimbot.aim_key = 0;
                    self.binding_aim_key = false;
                }
            });
            ui.separator();
            ui.checkbox(&mut cfg.aimbot.rcs_enabled, "Recoil Control (RCS)");
            ui.add_enabled_ui(cfg.aimbot.rcs_enabled, |ui| {
                ui.add(
                    egui::Slider::new(&mut cfg.aimbot.rcs_scale, 0.0..=1.0)
                        .text("RCS Scale")
                        .step_by(0.01),
                );
            });
        });
    }

    fn tab_trigger(&mut self, ui: &mut egui::Ui) {
        let mut cfg = self.shared.config.write().unwrap();
        ui.checkbox(&mut cfg.trigger.enabled, "Enable triggerbot");
        ui.add_enabled_ui(cfg.trigger.enabled, |ui| {
            ui.add(
                egui::Slider::new(&mut cfg.trigger.delay_ms, 0..=500)
                    .text("Delay (ms)"),
            );
        });
    }

    fn tab_visuals(&mut self, ui: &mut egui::Ui) {
        let mut cfg = self.shared.config.write().unwrap();
        ui.checkbox(&mut cfg.visuals.enabled, "Enable ESP");
        ui.add_enabled_ui(cfg.visuals.enabled, |ui| {
            ui.checkbox(&mut cfg.visuals.boxes,      "Bounding boxes");
            ui.checkbox(&mut cfg.visuals.health_bar, "Health bar");
            ui.checkbox(&mut cfg.visuals.names,      "Names");
            ui.checkbox(&mut cfg.visuals.team_check, "Skip teammates");
            ui.checkbox(&mut cfg.visuals.fov_circle, "Aimbot FOV circle");
            ui.checkbox(&mut cfg.visuals.skeletons,  "Skeletons");
            ui.separator();
            color_edit(ui, "Box color",             &mut cfg.visuals.box_color);
            color_edit(ui, "Name color",            &mut cfg.visuals.name_color);
            color_edit(ui, "Skeleton (enemy)",      &mut cfg.visuals.skeleton_enemy_color);
            color_edit(ui, "Skeleton (teammate)",   &mut cfg.visuals.skeleton_team_color);
        });
    }

    fn tab_config(&mut self, ui: &mut egui::Ui) {
        // ── Exit ──────────────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            if ui
                .add_sized(
                    Vec2::new(80.0, 24.0),
                    egui::Button::new(egui::RichText::new("Exit").color(Color32::RED)),
                )
                .clicked()
            {
                let cfg = self.shared.config.read().unwrap().clone();
                crate::config::save(&cfg, "default");
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.separator();

        // ── Config file list ──────────────────────────────────────────────────
        ui.columns(2, |cols| {
            egui::ScrollArea::vertical()
                .id_salt("cfg_list")
                .show(&mut cols[0], |ui| {
                    for (i, name) in self.config_files.iter().enumerate() {
                        if ui
                            .selectable_label(self.selected_config == i as i32, name)
                            .clicked()
                        {
                            self.selected_config = i as i32;
                        }
                    }
                });

            let ui = &mut cols[1];
            ui.text_edit_singleline(&mut self.config_name_buf);

            if ui.button("Create / Save as").clicked() && !self.config_name_buf.is_empty() {
                let cfg = self.shared.config.read().unwrap().clone();
                config::save(&cfg, &self.config_name_buf);
                self.config_name_buf.clear();
                self.config_files = config::list_files();
            }

            if ui.button("Refresh").clicked() {
                self.config_files = config::list_files();
            }

            if self.selected_config >= 0 {
                let idx = self.selected_config as usize;
                if let Some(name) = self.config_files.get(idx).cloned() {
                    if ui.button("Load").clicked() {
                        if let Some(loaded) = config::load(&name) {
                            *self.shared.config.write().unwrap() = loaded;
                        }
                    }
                    if ui.button("Save").clicked() {
                        let cfg = self.shared.config.read().unwrap().clone();
                        config::save(&cfg, &name);
                    }
                    if ui.button("Remove").clicked() {
                        config::remove(&name);
                        self.config_files = config::list_files();
                        self.selected_config = -1;
                    }
                }
            }
        });
    }

    // ── Status bar ────────────────────────────────────────────────────────────

    fn draw_status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            let state    = self.shared.game_state.read().unwrap();
            let entities = self.shared.entities.read().unwrap();
            let players  = entities.iter().filter(|e| e.entity_type == EntityType::Player).count();
            ui.horizontal(|ui| {
                ui.label(format!("Map: {}", state.map_name));
                ui.separator();
                ui.label(format!("Players: {}", players));
                ui.separator();
                ui.label(if state.is_in_game() { "In Game" } else { "Menu" });
                ui.separator();
                ui.label(format!("FPS: {:.0}", 1.0 / ctx.input(|i| i.unstable_dt)));
            });
        });
    }
}

// ─── eframe::App impl ────────────────────────────────────────────────────────

impl eframe::App for App {
    /// Transparent GPU clear so the overlay framebuffer starts at alpha=0.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Menu content — the GDI overlay runs on its own thread (see overlay.rs).
        self.draw_status_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, label) in ["Aimbot", "Trigger", "Visuals", "Config"].iter().enumerate() {
                    if ui
                        .add_sized(
                            Vec2::new(100.0, 28.0),
                            egui::SelectableLabel::new(self.current_tab == i, *label),
                        )
                        .clicked()
                    {
                        self.current_tab = i;
                    }
                }
            });

            ui.separator();

            match self.current_tab {
                0 => self.tab_aimbot(ui),
                1 => self.tab_trigger(ui),
                2 => self.tab_visuals(ui),
                3 => self.tab_config(ui),
                _ => {}
            }
        });

        ctx.request_repaint();
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn vk_name(vk: u32) -> String {
    match vk {
        0x01 => "LMB".into(),
        0x02 => "RMB".into(),
        0x04 => "MMB".into(),
        0x05 => "Mouse4".into(),
        0x06 => "Mouse5".into(),
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x10 => "Shift".into(),
        0x11 => "Ctrl".into(),
        0x12 => "Alt".into(),
        0x14 => "CapsLock".into(),
        0x1B => "Escape".into(),
        0x20 => "Space".into(),
        0x70..=0x7B => format!("F{}", vk - 0x6F),
        0x41..=0x5A => format!("{}", char::from(vk as u8)),
        0x30..=0x39 => format!("{}", char::from(vk as u8)),
        0xA0 => "LShift".into(),
        0xA1 => "RShift".into(),
        0xA2 => "LCtrl".into(),
        0xA3 => "RCtrl".into(),
        0xA4 => "LAlt".into(),
        0xA5 => "RAlt".into(),
        _ => format!("VK 0x{:02X}", vk),
    }
}

fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3])
}

fn color_edit(ui: &mut egui::Ui, label: &str, c: &mut [u8; 4]) {
    ui.horizontal(|ui| {
        let mut col = [
            c[0] as f32 / 255.0, c[1] as f32 / 255.0,
            c[2] as f32 / 255.0, c[3] as f32 / 255.0,
        ];
        if ui.color_edit_button_rgba_unmultiplied(&mut col).changed() {
            c[0] = (col[0] * 255.0) as u8;
            c[1] = (col[1] * 255.0) as u8;
            c[2] = (col[2] * 255.0) as u8;
            c[3] = (col[3] * 255.0) as u8;
        }
        ui.label(label);
    });
}
