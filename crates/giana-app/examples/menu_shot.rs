//! Pictures of every menu page without a window: `menu_shot <out_dir>` writes PPM files (convert
//! them to PNG with python3 PIL). Every page is drawn with keyboard hints and with the hints of each
//! controller type (Xbox, PlayStation, Switch, neutral), the controller pages also with a custom
//! binding; `toasts` shows the messages over the running game.
use giana_app::bindings::{Bindings, PadInput};
use giana_app::hints::InputState;
use giana_app::menu::{Device, Menu, Page, Search};
use giana_app::menu_draw::{self, DrawCtx};
use giana_app::padicons::{icon_for, Icon, PadKind, DOWN, LEFT, RIGHT, UP};
use giana_app::result::{decide, Standing, Winner};
use giana_app::scores::{Entry, Scores};
use giana_app::settings::Scaling;
use giana_app::ui::{self, PillTone};
use giana_core::state::{ControllerInput, GameState};
use giana_render::framebuffer::{Framebuffer, HEIGHT, WIDTH};
use giana_render::vic::{render_vic, VicView};
use sdl2::controller::{Axis, Button};
use std::io::Write;

fn save(fb: &Framebuffer, path: String) {
    let mut f = std::fs::File::create(path).unwrap();
    writeln!(f, "P6\n{} {}\n255", WIDTH, HEIGHT).unwrap();
    for p in &fb.pixels {
        f.write_all(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]).unwrap();
    }
}

struct Shots {
    dir: String,
    fb: Framebuffer,
    game: Framebuffer,
    bindings: Bindings,
    custom: Bindings,
    scores: Scores,
    full: Scores,
}

impl Shots {
    /// One picture of a page with the hints of `input`.
    fn page(&mut self, name: &str, tag: &str, m: &Menu, input: InputState, custom: bool, scores_full: bool) {
        // the pause menu and its question are drawn over the game
        self.fb.pixels.copy_from_slice(&self.game.pixels);
        let b = if custom { &self.custom } else { &self.bindings };
        let scores = if scores_full { &self.full } else { &self.scores };
        let ctx = DrawCtx { bindings: b, pad_name: matches!(input.device, giana_app::hints::InputDevice::Pad).then_some("Controller Name"), scores, input, nudge: 0, view: giana_app::ui::View::FULL };
        menu_draw::draw(&mut self.fb, m, &ctx);
        save(&self.fb, format!("{}/{name}_{tag}.ppm", self.dir));
    }
}

fn main() {
    let dir = std::env::args().nth(1).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let mut scores = Scores::default();
    for (n, v, st) in [("KEVIN", 123450, 12), ("MARIA", 45600, 4), ("GIANA", 3200, 1)] {
        scores.insert(Entry { name: n.into(), score: v, stage: st });
    }
    let mut full = Scores::default();
    for (i, n) in ["KEVIN", "WWWWWWWW", "MARIA", "GIANA", "A", "BOB 1", "ZED", "X Y", "QQ", "LAST"].iter().enumerate() {
        full.insert(Entry { name: (*n).into(), score: 999_999 - i as u32 * 111_111, stage: 33 - i as u32 * 3 });
    }
    // the running game, for the pause menu
    let mut g = GameState::new_port_game();
    for _ in 0..120 {
        g.update(&ControllerInput::new());
    }
    let mut game = Framebuffer::new();
    render_vic(&mut game, &VicView::from_game(&g));
    // custom bindings: keys on other places, the controller with bumper, trigger and right stick
    let mut custom = Bindings::default();
    custom.set_key(3, 0, sdl2::keyboard::Keycode::J);
    custom.set_key(4, 0, sdl2::keyboard::Keycode::RShift);
    custom.set_key(0, 0, sdl2::keyboard::Keycode::Q);
    custom.set_key(1, 0, sdl2::keyboard::Keycode::E);
    custom.set_pad(3, 0, PadInput::Button(Button::LeftShoulder));
    custom.set_pad(3, 1, PadInput::Button(Button::Start));
    custom.set_pad(4, 0, PadInput::AxisPos(Axis::TriggerRight));
    custom.set_pad(4, 1, PadInput::Button(Button::LeftStick));
    custom.set_pad(0, 0, PadInput::AxisNeg(Axis::RightX));
    custom.set_pad(1, 0, PadInput::AxisPos(Axis::RightX));
    custom.set_pad(5, 0, PadInput::Button(Button::Back));
    custom.set_pad(5, 2, PadInput::AxisNeg(Axis::TriggerLeft));
    let mut s = Shots { dir, fb: Framebuffer::new(), game, bindings: Bindings::default(), custom, scores, full };
    // the first-start page (no game data needed to draw it): empty, a typed path, working, and the error messages
    {
        use giana_app::firstrun::{self, Setup, Status};
        let out_dir = s.dir.clone();
        let shot = |name: &str, setup: &Setup, fb: &mut Framebuffer| {
            firstrun::draw(fb, setup, 0, 0);
            save(fb, format!("{out_dir}/firstrun_{name}.ppm"));
        };
        let mut fb = Framebuffer::new();
        let mut setup = Setup::new("/tmp/giana-native/data".into());
        shot("idle", &setup, &mut fb);
        setup.type_text("~/Downloads/Great_Giana_Sisters_The.g64");
        shot("typed", &setup, &mut fb);
        setup.status = Status::Working("SEARCHING FOR YOUR DISK IMAGE");
        shot("working", &setup, &mut fb);
        let dest = std::path::Path::new("/nonexistent/x");
        for (name, r) in [
            ("err_not_g64", firstrun::import_bytes(b"hello", dest, "txt")),
            ("err_d64", firstrun::import_bytes(b"hello", dest, "d64")),
            ("err_unknown", firstrun::import_bytes(b"GCR-1541 and then some other disk", dest, "g64")),
            ("err_missing", firstrun::import_file(std::path::Path::new("/home/me/Downloads/nothing.g64"), dest)),
        ] {
            setup.status = Status::Failed(r.unwrap_err());
            shot(name, &setup, &mut fb);
        }
        setup.cursor = 1;
        setup.status = Status::Failed(firstrun::wrap("NO KNOWN G64 FOUND IN DOWNLOADS, DOCUMENTS, DESKTOP, YOUR HOME FOLDER OR NEXT TO THE GAME. DROP THE FILE ON THE WINDOW OR TYPE ITS PATH.", 34));
        shot("err_search", &setup, &mut fb);
    }

    let res = |rule, a: (&str, u32, u32), b: (&str, u32, u32)| {
        let p = |t: (&str, u32, u32)| Standing { name: t.0.into(), score: t.1, stage: t.2 };
        decide(rule, [p(a), p(b)])
    };
    let pages: Vec<(&str, Menu, bool)> = {
        let mut v: Vec<(&str, Menu, bool)> = Vec::new();
        let main = |cursor: usize| {
            let mut m = Menu::main();
            m.cursor = cursor;
            m
        };
        v.push(("main", main(0), false));
        v.push(("main_sel4", main(4), false));
        v.push(("main_notice", { let mut m = main(0); m.notice = Some("OTHER PLAYER LEFT THE GAME".into()); m }, false));
        for (name, view) in [
            ("main_update", giana_app::update::View::Available("0.2.0".into())),
            ("main_updating", giana_app::update::View::Installing),
            ("main_updated", giana_app::update::View::Done),
            ("main_update_manual", giana_app::update::View::Manual),
            ("main_update_failed", giana_app::update::View::Failed("CHECKSUM MISMATCH".into())),
        ] {
            let mut m = main(0);
            m.update = view;
            v.push((name, m, false));
        }
        let mut m = Menu::pause();
        m.cursor = 0;
        v.push(("pause", m, false));
        let mut m = Menu::pause();
        m.in_network = false;
        m.page = Page::ConfirmQuit;
        m.cursor = 1;
        v.push(("quit", m, false));
        let mut m = Menu::pause();
        m.in_network = true;
        m.page = Page::ConfirmQuit;
        m.cursor = 0;
        v.push(("quit_net", m, false));
        for (name, cursor, notice) in [("net", 1, None), ("net_error", 0, Some("NOBODY IS HOSTING AT THAT ADDRESS"))] {
            let mut m = main(cursor);
            m.page = Page::Network;
            m.notice = notice.map(String::from);
            v.push((name, m, false));
        }
        let mut m = main(0);
        m.page = Page::Hosting;
        m.my_ip = "192.168.178.23".into();
        v.push(("host", m.clone(), false));
        m.settings.winner = Winner::Furthest;
        m.host_findable = false;
        m.notice = Some("ANOTHER VERSION TRIED TO JOIN".into());
        v.push(("host_warn", m, false));
        let mut m = main(0);
        m.page = Page::Join;
        m.hosts = vec!["192.168.178.23".into(), "192.168.178.40".into()];
        m.settings.last_address = "10.0.0.7".into();
        v.push(("join", m.clone(), false));
        m.address = "192.168.178".into();
        v.push(("join_typing", m, false));
        let mut m = main(0);
        m.page = Page::Join;
        m.search = Search::NothingFound;
        m.settings.last_address = "192.168.178.40".into();
        v.push(("join_nothing", m.clone(), false));
        m.connecting = true;
        m.address = "192.168.178.40".into();
        v.push(("join_connecting", m, false));
        let mut m = main(0);
        m.page = Page::Join;
        m.notice = Some("NOBODY IS HOSTING AT THAT ADDRESS".into());
        v.push(("join_error", m, false));
        let mut m = main(0);
        m.page = Page::Scores;
        m.latest = Some(1);
        v.push(("scores", m.clone(), false));
        v.push(("scores_full", m, true));
        let mut m = main(0);
        m.page = Page::Scores;
        v.push(("scores_empty", m, false));
        let mut m = main(0);
        m.finish_game(vec![("GIANA".into(), 12300, 3)], &Scores::default());
        m.type_name('K');
        m.type_name('E');
        v.push(("name", m, false));
        for (name, rule, a, b) in [
            ("result_score", Winner::Score, ("GIANA", 12300, 3), ("MARIA", 8450, 5)),
            ("result_furthest", Winner::Furthest, ("GIANA", 12300, 3), ("MARIA", 8450, 5)),
            ("result_draw", Winner::Score, ("GIANA", 4000, 2), ("MARIA", 4000, 2)),
        ] {
            let mut m = main(0);
            m.show_result(res(rule, a, b));
            m.result_hold = 0;
            v.push((name, m, false));
        }
        let mut m = main(0);
        m.show_result(res(Winner::Score, ("GIANA", 12300, 3), ("MARIA", 8450, 5)));
        v.push(("result_hold", m, false));
        for (name, paused, audio_ok, scaling, cursor) in [("options", false, true, Scaling::NoBorder, 1), ("options_nosound", true, false, Scaling::Fill, 2)] {
            let mut m = main(cursor);
            m.paused = paused;
            m.page = Page::Options;
            m.audio_ok = audio_ok;
            m.settings.volume = 6;
            m.settings.music_volume = 7;
            m.settings.effects_volume = 4;
            m.settings.scaling = scaling;
            v.push((name, m, false));
        }
        for (name, cursor, muted, audio_ok) in [("sound_volume", 0, false, true), ("sound_music", 1, false, true), ("sound_menu", 3, true, false), ("sound_back", 4, false, true)] {
            let mut m = main(cursor);
            m.page = Page::Sound;
            m.audio_ok = audio_ok;
            m.settings.muted = muted;
            m.settings.volume = 6;
            m.settings.music_volume = 7;
            m.settings.effects_volume = 4;
            v.push((name, m, false));
        }
        let mut m = main(5);
        m.page = Page::Options;
        m.settings.enhancements.enhanced = true;
        m.settings.winner = Winner::Furthest;
        v.push(("options_mode", m, false));
        let mut m = main(3);
        m.page = Page::Options;
        v.push(("options_sound", m, false));
        let mut m = main(7);
        m.page = Page::Options;
        v.push(("options_back", m, false));
        for (name, enhanced, cursor) in [("enhanced_original", false, 0), ("enhanced_off_smooth", false, 1), ("enhanced_on", true, 1)] {
            let mut m = main(cursor);
            m.page = Page::Enhanced;
            m.settings.enhancements.enhanced = enhanced;
            v.push((name, m, false));
        }
        for (name, device) in [("keys", Device::Keyboard), ("pad", Device::Pad)] {
            let mut m = main(3);
            m.page = Page::Controls(device);
            m.slot = 1;
            v.push((name, m.clone(), false));
            m.capturing = true;
            v.push((if device == Device::Keyboard { "keys_capture" } else { "pad_capture" }, m, false));
        }
        let mut m = main(7);
        m.page = Page::Controls(Device::Pad);
        v.push(("pad_reset", m, false));
        v
    };

    let kinds = [("xbox", PadKind::Xbox), ("ps", PadKind::PlayStation), ("switch", PadKind::Switch), ("generic", PadKind::Generic)];
    for (name, m, full) in &pages {
        s.page(name, "kb", m, InputState::keyboard(), false, *full);
        // the keyboard with the custom bindings, on the pages that show them
        if matches!(*name, "main" | "pause" | "keys") {
            s.page(name, "kb_custom", m, InputState::keyboard(), true, *full);
        }
        for (tag, kind) in kinds {
            let all = tag == "xbox";
            let some = matches!(*name, "main" | "pause" | "pad" | "pad_capture" | "name" | "join" | "options_mode" | "result_score" | "quit" | "host" | "main_update");
            if all || some {
                s.page(name, tag, m, InputState::pad(kind), false, *full);
            }
            if matches!(*name, "main" | "pause" | "pad" | "pad_reset") {
                s.page(name, &format!("{tag}_custom"), m, InputState::pad(kind), true, *full);
            }
        }
    }

    // the messages over the running game: lanes in the border, and in the cut picture
    for (tag, y1, y2) in [("border", 240, 256), ("cropped", 224, 208)] {
        s.fb.pixels.copy_from_slice(&s.game.pixels);
        ui::pill(&mut s.fb, y1, "YOU ARE GIANA - YOUR TURN", PillTone::Info);
        ui::pill(&mut s.fb, y2, "PRESS JUMP OR FIRE", PillTone::Info);
        save(&s.fb, format!("{}/toast_status_{tag}.ppm", s.dir));
        s.fb.pixels.copy_from_slice(&s.game.pixels);
        ui::pill(&mut s.fb, y1, "SOUND OFF", PillTone::Info);
        save(&s.fb, format!("{}/toast_sound_{tag}.ppm", s.dir));
        s.fb.pixels.copy_from_slice(&s.game.pixels);
        ui::pill(&mut s.fb, y1, "OTHER PLAYER PAUSED", PillTone::Notice);
        save(&s.fb, format!("{}/toast_paused_{tag}.ppm", s.dir));
    }

    // the glyph sets: every input of the catalogue on black, in the footer and on a key cap row
    for (tag, kind) in kinds {
        s.fb.clear(0xFF000000);
        let inputs: Vec<(&str, PadInput)> = vec![
            ("A", PadInput::Button(Button::A)),
            ("B", PadInput::Button(Button::B)),
            ("X", PadInput::Button(Button::X)),
            ("Y", PadInput::Button(Button::Y)),
            ("LB", PadInput::Button(Button::LeftShoulder)),
            ("RB", PadInput::Button(Button::RightShoulder)),
            ("LT", PadInput::AxisPos(Axis::TriggerLeft)),
            ("RT", PadInput::AxisPos(Axis::TriggerRight)),
            ("START", PadInput::Button(Button::Start)),
            ("BACK", PadInput::Button(Button::Back)),
            ("GUIDE", PadInput::Button(Button::Guide)),
            ("L3", PadInput::Button(Button::LeftStick)),
            ("R3", PadInput::Button(Button::RightStick)),
            ("LS UP", PadInput::AxisNeg(Axis::LeftY)),
            ("LS DN", PadInput::AxisPos(Axis::LeftY)),
            ("LS L", PadInput::AxisNeg(Axis::LeftX)),
            ("RS R", PadInput::AxisPos(Axis::RightX)),
            ("PAD1", PadInput::Button(Button::Paddle1)),
        ];
        let mut icons: Vec<(String, Icon)> = inputs.iter().map(|(n, i)| (n.to_string(), icon_for(kind, *i))).collect();
        for (n, d) in [("DP UP", UP), ("DP DN", DOWN), ("DP L", LEFT), ("DP R", RIGHT), ("DP UD", UP | DOWN), ("DP LR", LEFT | RIGHT)] {
            icons.push((n.into(), Icon::DPad(d)));
        }
        for (i, (name, icon)) in icons.iter().enumerate() {
            let (col, row) = (i % 6, i / 6);
            let (x, y) = (24 + col as isize * 58, 30 + row as isize * 40);
            icon.draw(&mut s.fb, x, y);
            giana_render::text::draw_text_px(&mut s.fb, x, y + 14, &name[..name.len().min(6)], 0xFF7B7B7B);
        }
        giana_render::text::draw_text_px(&mut s.fb, 24, 8, &format!("GLYPHS: {}", kind.name()), 0xFFFFFFFF);
        save(&s.fb, format!("{}/icons_{tag}.ppm", s.dir));
    }
}
