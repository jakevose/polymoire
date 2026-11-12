//! The media player's screens: the library window (sign-in, folder choice, then the
//! browser) and the now-playing bar. Shown only in media player mode, with the HUD.

use super::hud::{label, mono, pill, DIM, INK, LINE, PANEL};
use super::library::Library;
use super::PolymoireApp;
use eframe::egui::{self, Align2, Color32, CornerRadius, RichText, Stroke};
use polymoire::library::catalog::View;

fn dim(text: impl Into<String>) -> RichText {
    RichText::new(text).font(mono(9.0)).color(DIM)
}

fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(14, 12))
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn show(ctx: &egui::Context, app: &mut PolymoireApp) {
    if !app.hud_visible {
        return;
    }
    let screen = ctx.content_rect();
    let bar_h = 74.0;
    egui::Area::new(egui::Id::new("now_playing"))
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(egui::pos2(screen.right() - 16.0, screen.bottom() - 16.0))
        .show(ctx, |ui| {
            frame().show(ui, |ui| now_playing(ui, &mut app.library))
        });
    if !app.library.ui.open {
        return;
    }
    // Right of the HUD panel, between the transport row and the now-playing bar.
    let width = (screen.width() - 340.0).clamp(320.0, 820.0);
    let height = (screen.height() - 70.0 - bar_h - 40.0).max(200.0);
    egui::Area::new(egui::Id::new("library"))
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(egui::pos2(screen.right() - 16.0, screen.top() + 54.0))
        .show(ctx, |ui| {
            frame().show(ui, |ui| {
                ui.set_width(width);
                ui.set_height(height);
                let lib = &mut app.library;
                if lib.dbx.is_none() {
                    setup(ui, lib);
                } else if lib.browse.is_some() {
                    folder_picker(ui, lib);
                } else {
                    browser(ui, lib, height);
                }
            });
        });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(mono(13.0)).strong().color(INK));
}

fn status_line(ui: &mut egui::Ui, lib: &Library) {
    if !lib.status.is_empty() {
        ui.add(egui::Label::new(dim(&lib.status)).wrap());
    }
}

fn setup(ui: &mut egui::Ui, lib: &mut Library) {
    heading(ui, "MEDIA PLAYER");
    ui.add_space(6.0);
    ui.add(
        egui::Label::new(label(
            "Plays mp3 and m4a files from a Dropbox folder. Polymoire reads your music and \
             only ever writes one file: a backup of its catalog, in the folder you choose.",
        ))
        .wrap(),
    );
    ui.add_space(10.0);
    if lib.settings.app_key().is_none() {
        ui.label(dim(
            "Dropbox app key (from dropbox.com/developers, with PKCE allowed)",
        ));
        ui.text_edit_singleline(&mut lib.key_input);
        ui.add_space(6.0);
    }
    if lib.auth.is_none() {
        if pill(ui, "Connect Dropbox", true).clicked() {
            lib.begin_auth();
        }
    } else {
        ui.label(dim(
            "1. Approve Polymoire in the browser window that opened.",
        ));
        if let Some(url) = lib.auth_url() {
            ui.hyperlink_to(dim("Didn't open? Open the link."), url);
        }
        ui.label(dim("2. Paste the code Dropbox shows you:"));
        let r = ui.text_edit_singleline(&mut lib.code_input);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            if pill(ui, "Finish", true).clicked() || enter {
                lib.finish_auth();
            }
            if pill(ui, "Cancel", false).clicked() {
                lib.auth = None;
            }
        });
    }
    ui.add_space(6.0);
    status_line(ui, lib);
}

fn folder_picker(ui: &mut egui::Ui, lib: &mut Library) {
    heading(ui, "CHOOSE YOUR MUSIC FOLDER");
    let path = lib
        .browse
        .as_ref()
        .map(|b| b.path.clone())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        if !path.is_empty() && pill(ui, "Up", false).clicked() {
            let parent = path.rsplit_once('/').map_or("", |(p, _)| p).to_string();
            lib.browse_to(&parent);
        }
        let shown = if path.is_empty() { "Dropbox" } else { &path };
        ui.add(egui::Label::new(label(shown)).truncate());
    });
    // Only a folder Dropbox actually listed can become the library.
    let listed = matches!(
        lib.browse.as_ref().and_then(|b| b.folders.as_ref()),
        Some(Ok(_))
    );
    ui.horizontal(|ui| {
        let r = ui
            .add_enabled_ui(listed, |ui| pill(ui, "Use this folder", true))
            .inner;
        if r.clicked() {
            lib.choose_root(&path);
        }
        if lib.settings.root.is_some() && pill(ui, "Cancel", false).clicked() {
            lib.browse = None;
        }
    });
    ui.add_space(4.0);
    let mut go = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            match lib.browse.as_ref().and_then(|b| b.folders.as_ref()) {
                None => {
                    ui.label(dim("Loading…"));
                }
                Some(Err(e)) => {
                    ui.label(dim(e));
                }
                Some(Ok(f)) if f.is_empty() => {
                    ui.label(dim("No folders here."));
                }
                Some(Ok(folders)) => {
                    for f in folders {
                        let name = f.rsplit('/').next().unwrap_or(f);
                        if ui
                            .selectable_label(false, label(&format!("▸ {name}")))
                            .clicked()
                        {
                            go = Some(f.clone());
                        }
                    }
                }
            }
        });
    if let Some(f) = go {
        lib.browse_to(&f);
    }
}

fn browser(ui: &mut egui::Ui, lib: &mut Library, height: f32) {
    ui.horizontal(|ui| {
        heading(ui, "LIBRARY");
        let root = lib.settings.root.clone().unwrap_or_default();
        let who = lib.account.clone().unwrap_or_default();
        ui.label(dim(format!(
            "{who} · {}",
            if root.is_empty() { "/" } else { &root }
        )));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if pill(ui, "Disconnect", false).clicked() {
                lib.disconnect();
            }
            if pill(ui, "Folder", false).clicked() {
                let at = lib.settings.root.clone().unwrap_or_default();
                lib.browse_to(&at);
            }
            if pill(ui, if lib.syncing { "Syncing…" } else { "Sync" }, false).clicked() {
                lib.sync();
            }
        });
    });
    ui.horizontal(|ui| {
        let r = ui.add(
            egui::TextEdit::singleline(&mut lib.ui.query)
                .hint_text("Search titles, artists, albums, genres, folders")
                .desired_width(ui.available_width() * 0.6),
        );
        if r.changed() {
            lib.ui.group = None;
        }
        let left = lib.untagged_left();
        let info = if let Some(p) = &lib.scan {
            p.describe()
        } else if left > 0 {
            format!(
                "{} tracks · reading tags, {left} to go",
                lib.catalog.tracks.len()
            )
        } else {
            format!("{} tracks", lib.catalog.tracks.len())
        };
        ui.label(dim(info));
    });
    status_line(ui, lib);
    if let Some(p) = &lib.scan {
        // Indeterminate (animated) until the folder count is known.
        let bar = egui::ProgressBar::new(if p.total > 0 {
            p.done as f32 / p.total as f32
        } else {
            0.0
        })
        .desired_height(4.0)
        .animate(p.total == 0);
        ui.add(bar);
    }
    ui.add_space(4.0);

    let col_h = height - 90.0;
    let views_w = 118.0;
    let groups_w = if lib.ui.view.grouped() { 200.0 } else { 0.0 };
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(views_w);
            for v in View::ALL {
                if ui
                    .selectable_label(lib.ui.view == v, label(v.label()))
                    .clicked()
                    && lib.ui.view != v
                {
                    lib.ui.view = v;
                    lib.ui.group = None;
                }
            }
        });
        if lib.ui.view.grouped() {
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(groups_w);
                groups(ui, lib, col_h);
            });
        }
        ui.separator();
        ui.vertical(|ui| tracks(ui, lib, col_h));
    });
}

/// A change to make once the list is drawn (it borrows the library while drawing).
type Deferred = Box<dyn FnOnce(&mut Library)>;

fn groups(ui: &mut egui::Ui, lib: &mut Library, h: f32) {
    if lib.ui.view == View::Playlists {
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut lib.ui.new_playlist)
                    .hint_text("New playlist")
                    .desired_width(150.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.small_button("+").clicked() || enter) && !lib.ui.new_playlist.trim().is_empty() {
                let name = std::mem::take(&mut lib.ui.new_playlist);
                let mut i = 0;
                lib.edit_playlists(|c| i = c.new_playlist(name.trim()));
                lib.ui.group = Some(i.to_string());
            }
        });
    }
    let list = lib.catalog.groups(lib.ui.view, &lib.ui.query);
    if list.is_empty() {
        ui.label(dim(if lib.ui.view == View::Playlists {
            "No playlists yet."
        } else {
            "Nothing here yet."
        }));
    }
    let mut action: Option<Deferred> = None;
    egui::ScrollArea::vertical()
        .id_salt("groups")
        .max_height(h)
        .auto_shrink(false)
        .show(ui, |ui| {
            for g in list {
                let selected = lib.ui.group.as_deref() == Some(&g.key);
                if let Some((i, name)) = lib
                    .ui
                    .renaming
                    .as_mut()
                    .filter(|(i, _)| i.to_string() == g.key)
                {
                    let i = *i;
                    let r = ui.text_edit_singleline(name);
                    if r.lost_focus() {
                        let name = name.clone();
                        action = Some(Box::new(move |lib| {
                            lib.edit_playlists(|c| c.playlists[i].name = name);
                            lib.ui.renaming = None;
                        }));
                    }
                    r.request_focus();
                    continue;
                }
                let text = RichText::new(format!("{}  ", g.label))
                    .font(mono(10.0))
                    .color(INK);
                let r = ui
                    .selectable_label(selected, text)
                    .on_hover_text(format!("{} tracks", g.count));
                if r.clicked() {
                    lib.ui.group = Some(g.key.clone());
                }
                if r.double_clicked() {
                    let ids = lib.catalog.list(lib.ui.view, Some(&g.key), &lib.ui.query);
                    action = Some(Box::new(move |lib| lib.play(ids, 0)));
                }
                let key = g.key.clone();
                let view = lib.ui.view;
                r.context_menu(|ui| {
                    if ui.button("Play").clicked() {
                        let ids = lib.catalog.list(view, Some(&key), &lib.ui.query);
                        action = Some(Box::new(move |lib| lib.play(ids, 0)));
                    }
                    if view == View::Playlists {
                        let i: usize = key.parse().unwrap_or(0);
                        if ui.button("Rename").clicked() {
                            lib.ui.renaming = Some((i, g.label.clone()));
                        }
                        if ui.button("Delete playlist").clicked() {
                            action = Some(Box::new(move |lib| {
                                lib.edit_playlists(|c| {
                                    c.playlists.remove(i);
                                });
                                lib.ui.group = None;
                            }));
                        }
                    }
                });
            }
        });
    if let Some(f) = action {
        f(lib);
    }
}

enum TrackAction {
    Play(usize),
    Next(String),
    Queue(String),
    AddTo(usize, String),
    NewWith(String),
    Move(usize, i32),
    Remove(usize),
}

fn tracks(ui: &mut egui::Ui, lib: &mut Library, h: f32) {
    let view = lib.ui.view;
    let group = lib.ui.group.clone();
    if view.grouped() && group.is_none() {
        ui.label(dim(
            "Pick a group on the left. Double-click a group or track to play.",
        ));
        return;
    }
    let ids = lib.catalog.list(view, group.as_deref(), &lib.ui.query);
    if ids.is_empty() {
        ui.label(dim(match view {
            View::MostPlayed => "Nothing played yet.",
            View::Playlists => "Empty. Right-click any track to add it here.",
            _ if lib.catalog.tracks.is_empty() && lib.syncing => {
                "Reading the library folder. Tracks appear here as each folder is read."
            }
            _ => "No tracks.",
        }));
        return;
    }
    let playlist = (view == View::Playlists)
        .then(|| group.as_deref().and_then(|g| g.parse::<usize>().ok()))
        .flatten();
    let names: Vec<String> = lib
        .catalog
        .playlists
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let current = lib.player.current.clone();
    let mut act = None;
    let row_h = 30.0;
    egui::ScrollArea::vertical()
        .id_salt("tracks")
        .max_height(h)
        .auto_shrink(false)
        .show_rows(ui, row_h, ids.len(), |ui, range| {
            for i in range {
                let Some(t) = lib.catalog.tracks.get(&ids[i]) else {
                    continue;
                };
                let playing = current.as_deref() == Some(&t.id);
                let (rect, r) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), row_h),
                    egui::Sense::click(),
                );
                let p = ui.painter_at(rect);
                if r.hovered() || playing {
                    p.rect_filled(
                        rect,
                        CornerRadius::same(4),
                        Color32::from_white_alpha(if playing { 30 } else { 14 }),
                    );
                }
                let x = rect.left() + 6.0;
                let title = if playing {
                    format!("▶ {}", t.title())
                } else {
                    t.title().to_string()
                };
                p.text(
                    egui::pos2(x, rect.top() + 9.0),
                    Align2::LEFT_CENTER,
                    title,
                    mono(10.5),
                    INK,
                );
                let sub = format!("{} · {}", t.artist(), t.album());
                p.text(
                    egui::pos2(x, rect.top() + 22.0),
                    Align2::LEFT_CENTER,
                    sub,
                    mono(8.5),
                    DIM,
                );
                let mut meta = String::new();
                if let Some(bpm) = t.bpm() {
                    meta.push_str(&format!("{bpm:.0} bpm   "));
                }
                if t.duration > 0.0 {
                    meta.push_str(&clock(t.duration as f64));
                }
                p.text(
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    meta,
                    mono(9.0),
                    DIM,
                );
                if r.double_clicked() {
                    act = Some(TrackAction::Play(i));
                }
                let id = t.id.clone();
                r.context_menu(|ui| {
                    if ui.button("Play").clicked() {
                        act = Some(TrackAction::Play(i));
                    }
                    if ui.button("Play next").clicked() {
                        act = Some(TrackAction::Next(id.clone()));
                    }
                    if ui.button("Add to queue").clicked() {
                        act = Some(TrackAction::Queue(id.clone()));
                    }
                    ui.menu_button("Add to playlist", |ui| {
                        for (k, n) in names.iter().enumerate() {
                            if ui.button(n).clicked() {
                                act = Some(TrackAction::AddTo(k, id.clone()));
                            }
                        }
                        if ui.button("New playlist").clicked() {
                            act = Some(TrackAction::NewWith(id.clone()));
                        }
                    });
                    if playlist.is_some() {
                        ui.separator();
                        if ui.button("Move up").clicked() {
                            act = Some(TrackAction::Move(i, -1));
                        }
                        if ui.button("Move down").clicked() {
                            act = Some(TrackAction::Move(i, 1));
                        }
                        if ui.button("Remove from playlist").clicked() {
                            act = Some(TrackAction::Remove(i));
                        }
                    }
                });
            }
        });
    // Moves and removals index the playlist itself; they're only offered unfiltered.
    let unfiltered = lib.ui.query.is_empty();
    match act {
        Some(TrackAction::Play(i)) => lib.play(ids, i),
        Some(TrackAction::Next(id)) => lib.player.play_next(id),
        Some(TrackAction::Queue(id)) => lib.player.enqueue(id),
        Some(TrackAction::AddTo(k, id)) => lib.edit_playlists(|c| c.playlists[k].tracks.push(id)),
        Some(TrackAction::NewWith(id)) => lib.edit_playlists(|c| {
            let k = c.new_playlist(&format!("Playlist {}", c.playlists.len() + 1));
            c.playlists[k].tracks.push(id);
        }),
        Some(TrackAction::Move(i, d)) if unfiltered => {
            if let Some(pl) = playlist {
                lib.edit_playlists(|c| {
                    let t = &mut c.playlists[pl].tracks;
                    let j = i as i64 + d as i64;
                    if j >= 0 && (j as usize) < t.len() {
                        t.swap(i, j as usize);
                    }
                });
            }
        }
        Some(TrackAction::Remove(i)) if unfiltered => {
            if let Some(pl) = playlist {
                lib.edit_playlists(|c| {
                    c.playlists[pl].tracks.remove(i);
                });
            }
        }
        _ => {}
    }
}

fn now_playing(ui: &mut egui::Ui, lib: &mut Library) {
    ui.set_width(420.0);
    let current = lib
        .player
        .current
        .as_ref()
        .and_then(|id| lib.catalog.tracks.get(id))
        .cloned();
    ui.horizontal(|ui| {
        if pill(ui, "⏮", false).clicked() {
            lib.skip(-1);
        }
        let playing = lib.player.playing();
        if pill(ui, if playing { "⏸" } else { "▶" }, playing).clicked() {
            lib.player.toggle();
        }
        if pill(ui, "⏭", false).clicked() {
            lib.skip(1);
        }
        ui.vertical(|ui| match &current {
            Some(t) => {
                ui.add(egui::Label::new(label(t.title())).truncate());
                let beat = match &lib.player.map {
                    Some(m) => format!("{:.0} bpm · beat-mapped", m.bpm),
                    None => "live beat tracking".into(),
                };
                ui.add(egui::Label::new(dim(format!("{} · {beat}", t.artist()))).truncate());
            }
            None => {
                ui.label(label("Nothing playing"));
                ui.label(dim("L opens the library"));
            }
        });
    });
    ui.horizontal(|ui| {
        let (pos, dur) = (lib.player.position(), lib.player.duration());
        ui.label(dim(clock(pos)));
        let mut v = pos;
        ui.spacing_mut().slider_width = 220.0;
        let r = ui.add(egui::Slider::new(&mut v, 0.0..=dur.max(1.0)).show_value(false));
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            lib.player.seek(v);
        }
        ui.label(dim(clock(dur)));
        let mut vol = lib.player.volume();
        ui.spacing_mut().slider_width = 50.0;
        if ui
            .add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false))
            .on_hover_text("Volume")
            .changed()
        {
            lib.player.set_volume(vol);
        }
        let shuffle = lib.player.shuffle;
        if pill(ui, "Shuffle", shuffle).clicked() {
            lib.player.shuffle = !shuffle;
            lib.save_settings();
        }
    });
}
