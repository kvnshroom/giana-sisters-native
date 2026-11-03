// Release builds on Windows open no console window next to the game (debug builds keep it for the log)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use giana_app::authentic::{self, AuthenticRun};
use giana_app::bindings::{config_path, Bindings, Held, PadInput, Source};
use giana_app::cli::{self, Command, Options};
use giana_app::display;
use giana_app::firstrun;
use giana_app::hints::InputState;
use giana_app::menu::{self, nav_for_button, nav_for_key, nav_for_stick, AuthenticAvail, GameMode, Menu, ModuleInfo, Nav, OptionChange, Outcome, Search};
use giana_app::modules::{self, Store};
use giana_app::menu_draw::{self, DrawCtx};
use giana_app::menu_sound::{cue_for, Cue, MenuSounds};
use giana_app::padicons::{sdl_type_for_index, PadKind};
use giana_app::net::{self, Finder, Host, Joiner, NetError, NetMode, Session};
use giana_app::present::{self, Presenter};
use giana_app::ui::{self, PillTone};
use giana_app::result::{decide, MatchResult, Standing, Winner};
use giana_app::scores::{score_value, scores_path, Entry, Scores};
use giana_app::settings::{settings_path, Presentation, Settings};
use giana_audio::mixer::Mixer;
use giana_audio::player::MusicTrack;
use giana_audio::split::SplitDrivers;
use giana_core::state::{ControllerInput, GameScreen, GameState, stage_palettes};
use giana_render::framebuffer::{Framebuffer, BORDER_X, BORDER_Y, HEIGHT, WIDTH};
use giana_render::palette::c64_color;
use giana_render::parallax::render_stage;
use giana_render::vic::{render_vic, render_vic_between, VicSnapshot, VicView};
use giana_render::sprites::{render_sprite_hires, render_sprite_multicolor};
use sdl2::audio::{AudioCallback, AudioSpecDesired};
use sdl2::controller::Button;
use sdl2::event::{Event, WindowEvent};
use sdl2::controller::GameController;
use sdl2::keyboard::{Keycode, Mod};
use sdl2::mouse::MouseUtil;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::Canvas;
use sdl2::surface::Surface;
use sdl2::video::{FullscreenType, Window};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Set when the player switched the ROM module: `main` starts the game again once the window and the sound are closed.
static RESTART: AtomicBool = AtomicBool::new(false);

struct SidAudioCallback {
    mixer: Mixer,
}

impl AudioCallback for SidAudioCallback {
    type Channel = i16;

    fn callback(&mut self, out: &mut [i16]) {
        self.mixer.fill(out);
    }
}

/// The volumes of the settings to the mixer (256 is full volume, 0 is silence).
fn apply_gains(mixer: &mut Mixer, settings: &Settings) {
    mixer.gain = settings.gain();
    mixer.music_gain = Settings::part_gain(settings.music_volume);
    mixer.effects_gain = Settings::part_gain(settings.effects_volume);
}

/// A message for a failed start: what went wrong, and what the player can do about it.
fn startup_error(what: &str, e: impl std::fmt::Display, hint: &str) -> String {
    format!("{what}: {e}\n{hint}")
}

/// Shows a fatal error in a message box (also when no console is open) and writes it to `last-error.txt` next to the
/// settings, so that it can be found and sent along with a bug report.
fn report_fatal(text: &str) {
    let mut shown = text.to_string();
    if let Some(dir) = settings_path().as_deref().and_then(std::path::Path::parent) {
        let file = dir.join("last-error.txt");
        if std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&file, format!("{text}\n"))).is_ok() {
            shown.push_str(&format!("\n\nThis message was saved in {}", file.display()));
        }
    }
    let _ = sdl2::messagebox::show_simple_message_box(sdl2::messagebox::MessageBoxFlag::ERROR, "The Great Giana Sisters", &shown, None);
}

const SDL_HINT: &str = "Is the SDL2 library installed? Fedora: sudo dnf install SDL2 - Debian/Ubuntu: sudo apt install libsdl2-2.0-0 - macOS: brew install sdl2";
const DISPLAY_HINT: &str = "Is a display connected, and is a desktop session (X11 or Wayland) running? Without a display the game cannot start.";

/// Fullscreen on or off. With `want_50hz` the fullscreen takes a display mode of 50, 100, 150 or 200 Hz if the display has
/// one (see `giana_app::display` for why), otherwise it stays the desktop mode. Returns how many frames of the display one
/// game step takes when the display now runs at such a rate (1 at 50 Hz, 2 at 100 Hz ...), 0 when it does not.
fn apply_fullscreen(canvas: &mut Canvas<Window>, video: &sdl2::VideoSubsystem, mouse: &MouseUtil, on: bool, want_50hz: bool) -> u32 {
    let window = canvas.window_mut();
    let mut locked = false;
    let mut mode = if on { FullscreenType::Desktop } else { FullscreenType::Off };
    if on && want_50hz {
        let found = (|| {
            let display = window.display_index().ok()?;
            let desktop = video.desktop_display_mode(display).ok()?;
            let count = video.num_display_modes(display).ok()?;
            let modes: Vec<_> = (0..count).filter_map(|i| video.display_mode(display, i).ok()).collect();
            let list: Vec<display::Mode> = modes.iter().map(|m| (m.w, m.h, m.refresh_rate)).collect();
            let (i, _) = display::pick_mode(&list, (desktop.w, desktop.h))?;
            Some(modes[i])
        })();
        if let Some(m) = found {
            // The mode has to be set before the exclusive fullscreen is switched on
            if window.set_display_mode(Some(m)).is_ok() {
                mode = FullscreenType::True;
                locked = true;
                println!("Fullscreen at {} x {}, {} Hz", m.w, m.h, m.refresh_rate);
            }
        }
    }
    if let Err(e) = window.set_fullscreen(mode) {
        eprintln!("Could not change the fullscreen mode: {e}");
        // an exclusive mode that fails: the desktop mode still works
        if locked {
            locked = false;
            let _ = window.set_fullscreen(FullscreenType::Desktop);
        }
    }
    mouse.show_cursor(!on);
    // Whether the window really runs at such a rate now (the display can refuse the mode)
    if locked {
        window.display_mode().ok().and_then(|m| display::refresh_factor(m.refresh_rate)).unwrap_or(0)
    } else {
        0
    }
}

/// The glyph set of the controller at a device index: SDL's type, then the name.
fn pad_kind(index: u32, name: &str) -> PadKind {
    let kind = PadKind::detect(sdl_type_for_index(index), name);
    println!("Controller type: {} ({})", kind.name(), name);
    kind
}

/// A controller that is connected and not `skip` (the one that was just unplugged), with its glyph set.
fn open_controller(mgr: &sdl2::GameControllerSubsystem, skip: Option<u32>) -> Option<(GameController, PadKind)> {
    (0..mgr.num_joysticks().unwrap_or(0)).filter(|&id| mgr.is_game_controller(id)).find_map(|id| {
        let c = mgr.open(id).map_err(|e| eprintln!("Failed to open controller {id}: {e}")).ok()?;
        (Some(c.instance_id()) != skip).then(|| {
            let kind = pad_kind(id, &c.name());
            (c, kind)
        })
    })
}

/// Write the scores; when it fails the player sees a notice in the menu.
fn save_scores(scores: &Scores, module: &str, open: &mut Option<Menu>) {
    if let Some(path) = scores_path(module) {
        if let Err(e) = scores.save(&path) {
            eprintln!("Could not save the scores to {}: {}", path.display(), e);
            if let Some(m) = open.as_mut() {
                m.notice = Some("COULD NOT SAVE THE SCORES".to_string());
            }
        }
    }
}

fn save_settings(settings: &Settings, open: &mut Option<Menu>) {
    if let Some(path) = settings_path() {
        if let Err(e) = settings.save(&path) {
            eprintln!("Could not save the settings to {}: {}", path.display(), e);
            if let Some(m) = open.as_mut() {
                m.notice = Some("COULD NOT SAVE THE SETTINGS".to_string());
            }
        }
    }
}

/// Menu steps that the main loop carries out at its next logic tick.
#[derive(Default)]
struct Pending {
    start: Option<GameMode>,
    host: Option<NetMode>,
    join: Option<String>,
    search: bool,
    cancel_join: bool,
    options: Vec<OptionChange>,
    update: bool,
    stop_network: bool,
    new_score: Option<giana_app::scores::Entry>,
    to_title: bool,
    quit: bool,
    /// `ADD ROM MODULE...`: ask for a file
    add_module: bool,
    /// play this ROM module (its id, empty for the original game): save the choice and start again
    play_module: Option<String>,
    remove_module: Option<String>,
    /// `F` / `Y` on a module: favorite on or off (module ids)
    favorite_modules: Vec<String>,
    /// the next order of the modules page
    sort_modules: bool,
    /// `PRESENTATION` on the options page asked for the disk image (AUTHENTIC needs a file older data folders lack)
    authentic_data: bool,
}

/// Carry out what a menu step asks for.
fn apply_outcome(outcome: Outcome, open: &mut Option<Menu>, pending: &mut Pending, bindings: &Bindings) {
    match outcome {
        Outcome::None => {}
        Outcome::StartGame(mode) => {
            pending.start = Some(mode);
            *open = None;
        }
        Outcome::Resume => *open = None,
        Outcome::QuitToTitle => {
            pending.to_title = true;
            *open = Some(Menu::main());
        }
        Outcome::QuitApp => pending.quit = true,
        Outcome::Host(mode) => pending.host = Some(mode),
        Outcome::Search => pending.search = true,
        Outcome::Join(addr) => pending.join = Some(addr),
        Outcome::StopNetwork => pending.stop_network = true,
        Outcome::CancelJoin => pending.cancel_join = true,
        Outcome::Option(change) => pending.options.push(change),
        Outcome::Update => pending.update = true,
        Outcome::ModuleAdd => pending.add_module = true,
        Outcome::ModulePlay(id) => pending.play_module = Some(id),
        Outcome::ModuleRemove(id) => pending.remove_module = Some(id),
        Outcome::ModuleFavorite(id) => pending.favorite_modules.push(id),
        Outcome::ModuleSort => pending.sort_modules = true,
        Outcome::AuthenticData => pending.authentic_data = true,
        Outcome::AddScore(entry) => pending.new_score = Some(entry),
        Outcome::Changed => {
            if let Some(path) = config_path() {
                if let Err(e) = bindings.save(&path) {
                    eprintln!("Could not save the controls to {}: {}", path.display(), e);
                    if let Some(m) = open.as_mut() {
                        m.notice = Some("COULD NOT SAVE THE CONTROLS".to_string());
                    }
                }
            }
        }
    }
}

/// A menu step with its sound: a cursor move only sounds when the cursor (or a value) moved, confirm and back always.
fn menu_step(m: &mut Menu, nav: Nav, bindings: &mut Bindings, sound: &mut MenuSounds) -> Outcome {
    let before = (m.page, m.cursor, m.slot, m.name.len(), m.result_hold);
    let outcome = m.step(nav, bindings);
    let after = (m.page, m.cursor, m.slot, m.name.len(), m.result_hold);
    match cue_for(nav) {
        // keys that the result page ignores for a while make no sound either
        Some(_) if m.page == menu::Page::Result && m.result_hold > 0 => {}
        Some(Cue::Move) if before == after && outcome == Outcome::None => {}
        Some(cue) => sound.cue(cue),
        None => {}
    }
    outcome
}

/// Where the sound comes from
#[derive(Clone, Copy, PartialEq, Eq)]
enum SoundSource {
    /// the title tune and the menu sounds
    Title,
    /// the game is paused: only the menu sounds
    Paused,
    /// the sound driver of the running game
    Game,
}

/// The SID registers of the music chip and of the effects chip for the game that is heard (`which`: 0 yours, 1 the other
/// player's). The sound driver of that game records its calls (`Driver::record`), and they are played on two drivers of our
/// own, one without the effect requests and one without the music requests. Both are started again from a copy of the
/// game's driver when the game that is heard changes or is a new one (its driver does not record yet).
fn follow_split(split_game: &mut Option<(SplitDrivers, u8)>, game: &mut GameState, other: Option<&mut GameState>, which: u8) -> ([u8; 25], [u8; 25]) {
    let (shown, hidden) = match (which, other) {
        (1, Some(o)) => (o, Some(game)),
        (_, o) => (game, o),
    };
    if let Some(h) = hidden {
        h.sound.record = None;
    }
    let fresh = shown.sound.record.is_none() || split_game.as_ref().map_or(true, |(_, w)| *w != which);
    if fresh {
        shown.sound.record = Some(Vec::new());
        *split_game = Some((SplitDrivers::from_game(&shown.sound), which));
    } else if let Some((drivers, _)) = split_game.as_mut() {
        drivers.feed(&shown.sound.take_calls());
    }
    split_game.as_ref().map_or(([0; 25], [0; 25]), |(d, _)| d.regs())
}

/// The games stop recording the calls of their sound driver (nothing listens, and the list would only grow).
fn stop_recording(split_game: &mut Option<(SplitDrivers, u8)>, game: &mut GameState, other: Option<&mut GameState>) {
    if split_game.take().is_some() || game.sound.record.is_some() || other.as_ref().map_or(false, |o| o.sound.record.is_some()) {
        game.sound.record = None;
        if let Some(o) = other {
            o.sound.record = None;
        }
    }
}

fn player_name(player: u8) -> &'static str {
    if player == 0 {
        "GIANA"
    } else {
        "MARIA"
    }
}

/// What a player reached in a game, for the result.
fn standing(g: &GameState) -> Standing {
    Standing { name: player_name(g.player).to_string(), score: score_value(g.score_bcd), stage: g.stage }
}

/// The result of two players who played at the same time, one game each.
fn same_time_result(rule: Winner, mine: &GameState, other: &GameState) -> MatchResult {
    let mut players = [standing(mine), standing(other)];
    // Giana is always the first
    if mine.player != 0 {
        players.reverse();
    }
    decide(rule, players)
}

/// The result of one game with two players that take turns.
fn turns_result(rule: Winner, game: &GameState) -> MatchResult {
    let mut r = game_results(game).into_iter().map(|(name, score, stage)| Standing { name, score, stage });
    decide(rule, [r.next().unwrap(), r.next().unwrap()])
}

/// What both players reached: who, score, stage. The player whose turn it is has the live values, the other one the saved ones.
fn game_results(game: &GameState) -> Vec<(String, u32, u32)> {
    let p = game.player as usize;
    (0..2)
        .map(|i| {
            let name = player_name(i as u8).to_string();
            if i == p {
                (name, score_value(game.score_bcd), game.stage)
            } else {
                let s = &game.player_save;
                (name, score_value([s[i], s[2 + i], s[4 + i]]), s[8 + i] as u32)
            }
        })
        .collect()
}

/// A digit, dot or colon for the address on the join page.
fn address_char(key: Keycode) -> Option<char> {
    let name = key.name();
    let last = name.strip_prefix("Keypad ").unwrap_or(&name);
    (last.len() == 1).then(|| last.chars().next().unwrap()).filter(|c| c.is_ascii_digit() || *c == '.' || *c == ':')
}

/// The modules for the menu.
fn module_infos(store: Option<&Store>, data_dir: &std::path::Path, settings: &Settings) -> Vec<ModuleInfo> {
    let Some(store) = store else { return Vec::new() };
    let mut list = store.list(Some(data_dir));
    modules::order(&mut list, settings.module_sort, &settings.favorites, &settings.last_played);
    list.iter()
        .map(|i| ModuleInfo::new(&i.id, &i.meta.name, &i.meta.author, &i.meta.version, &i.meta.description, i.code_changes.unwrap_or(0) > 0))
        .collect()
}

/// Adds a file to the modules. The menu shows the module, or says in plain words why it could not be added.
fn add_module_file(store: Option<&Store>, data_dir: &std::path::Path, file: &std::path::Path, settings: &Settings, open: &mut Option<Menu>, infos: &mut Vec<ModuleInfo>) {
    let Some(store) = store else {
        if let Some(m) = open.as_mut() {
            m.module_added(None, Some("there is no folder for the modules".to_string()));
        }
        return;
    };
    match store.add(file, data_dir) {
        Ok(added) => {
            println!("ROM module added: {} ({})", added.meta.name, added.id);
            *infos = module_infos(Some(store), data_dir, settings);
            if let Some(m) = open.as_mut() {
                let active = m.active_module.clone();
                m.set_modules(infos, &active);
                m.module_added(Some(&added.id), None);
            }
        }
        Err(e) => {
            eprintln!("Could not add {}: {e}", file.display());
            if let Some(m) = open.as_mut() {
                m.module_added(None, Some(e));
            }
        }
    }
}

/// `--list-modules`
/// `--add-module FILE`: checks the module (or turns a hack's disk image into one) against the game data and stores it.
fn add_module_command(file: &std::path::Path, data: Option<&std::path::Path>) {
    let Some(store) = Store::default_location() else {
        eprintln!("giana-app: there is no folder for the modules.");
        std::process::exit(1);
    };
    let Some(data_dir) = firstrun::locate(data) else {
        eprintln!("giana-app: the game data is not there yet: import your .g64 first (giana-app --import FILE.g64).");
        std::process::exit(1);
    };
    match store.add(file, &data_dir) {
        Ok(m) => println!("Added ROM module {} ({})", m.meta.name, m.id),
        Err(e) => {
            eprintln!("giana-app: {}: {e}", file.display());
            std::process::exit(1);
        }
    }
}

fn list_modules(data: Option<&std::path::Path>) {
    let Some(store) = Store::default_location() else {
        eprintln!("giana-app: there is no folder for the modules.");
        std::process::exit(1);
    };
    let data_dir = firstrun::locate(data);
    let list = store.list(data_dir.as_deref());
    println!("ROM modules in {}:", store.dir().display());
    if list.is_empty() {
        println!("  none. Add one with: giana-app FILE.grom");
    }
    for m in list {
        let notes = match m.code_changes {
            Some(n) if n > 0 => "  (changes game code)",
            _ => "",
        };
        let kind = match m.kind_text() {
            k if k.is_empty() => String::new(),
            k => format!("  ({k})"),
        };
        let version = if m.meta.version.is_empty() { String::new() } else { format!("  V{}", m.meta.version) };
        println!("  {}  {}{version}  by {}{notes}{kind}", m.id, m.meta.name, m.meta.author);
    }
}

/// `--make-module`
fn make_module_command(m: cli::MakeModule) {
    let fail = |text: String| -> ! {
        eprintln!("giana-app: {text}");
        std::process::exit(1);
    };
    let Some(data_dir) = firstrun::locate(m.data.as_deref()) else {
        fail("a module is made against your original game data, which was not found. Start the game once to extract it, or give the folder with --data DIR.".into());
    };
    let base = modules::Base::read(&data_dir).unwrap_or_else(|e| fail(e));
    let meta = giana_module::Meta::new(&m.name, &m.author, &m.version, &m.description);
    let (module, applied) = modules::make_module(&base, &m.input, m.title.as_deref(), meta).unwrap_or_else(|e| fail(e));
    let bytes = module.write();
    if let Err(e) = std::fs::write(&m.out, &bytes) {
        fail(format!("cannot write {}: {e}", m.out.display()));
    }
    println!("ROM module '{}' ({}) written to {} ({} bytes)", module.meta.name, applied.id, m.out.display(), bytes.len());
    let uncovered = applied.uncovered_code_changes();
    if applied.engine.is_some() && applied.family != giana_module::Family::Tnw1989 {
        println!(
            "It is a game of the 7A3 construction-set engine, with {} stage, monster and treasure room files from the disk. \
             The port plays it with its native version of that engine.",
            applied.files.len()
        );
    } else if applied.code_changes.is_empty() {
        println!("It changes game data only.");
    } else if uncovered.is_empty() {
        println!("It changes game code the way {} hacks do; the port supports that family, so it plays as intended.", applied.family.name());
    } else {
        let total: usize = uncovered.iter().map(|c| c.1).sum();
        println!("It changes game CODE: {} range(s), {total} bytes. The port runs its own version of the code, so these changes have no effect and the module may not play as intended:", uncovered.len());
        for (start, len) in uncovered.iter().take(12) {
            println!("  ${start:04X}..${:04X} ({len} bytes)", start + len - 1);
        }
        if uncovered.len() > 12 {
            println!("  ... and {} more", uncovered.len() - 12);
        }
    }
    println!("Add it with: giana-app {}", m.out.display());
}

fn main() {
    let command = cli::parse(std::env::args().skip(1)).unwrap_or_else(|message| {
        eprintln!("giana-app: {message}\nTry 'giana-app --help'.");
        std::process::exit(2);
    });
    match command {
        Command::Help => println!("{}", cli::HELP),
        Command::Version => println!("{}", cli::version_text()),
        Command::ListModules(data) => list_modules(data.as_deref()),
        Command::AddModule(file, data) => add_module_command(&file, data.as_deref()),
        Command::MakeModule(m) => make_module_command(m),
        Command::CheckUpdate => {
            let (text, ok) = giana_app::update::check_report();
            println!("{text}");
            std::process::exit(if ok { 0 } else { 1 });
        }
        Command::Import(file, data) => {
            let Some(dest) = firstrun::destination(data.as_deref()) else {
                eprintln!("giana-app: there is no folder for the game data. Give one with --data DIR.");
                std::process::exit(1);
            };
            match firstrun::import_file(&file, &dest) {
                Ok(()) => println!("Game data extracted to {}", dest.display()),
                Err(lines) => {
                    eprintln!("giana-app: {}", lines.join(" "));
                    std::process::exit(1);
                }
            }
        }
        Command::Run(options) => {
            // Started from Finder or the Windows Explorer there is no console: an error must not just close the game
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                default_hook(info);
                // macOS shows windows only from the main thread; other threads' panics go to the console only
                if std::thread::current().name() == Some("main") {
                    report_fatal(&format!("The game stopped because of an internal error:\n{info}"));
                }
            }));
            if let Err(e) = run(options) {
                eprintln!("giana-app: {e}");
                report_fatal(&e.to_string());
                std::process::exit(1);
            }
            // another ROM module was chosen: `run` has closed the window and the sound, now the game starts again
            if RESTART.load(Ordering::SeqCst) {
                if let Err(e) = modules::spawn_restart() {
                    eprintln!("giana-app: could not start the game again: {e}");
                    report_fatal(&format!("The game could not start itself again: {e}\nStart it once more by hand."));
                    std::process::exit(1);
                }
            }
        }
    }
}

/// The first-start page: asks for the disk image until the data is extracted. False when the window was closed first.
fn first_run_page<C>(
    canvas: &mut Canvas<Window>,
    presenter: &mut Presenter<'_, C>,
    events: &mut sdl2::EventPump,
    video: &sdl2::VideoSubsystem,
    settings: &Settings,
    dest: std::path::PathBuf,
    open: Option<std::path::PathBuf>,
) -> Result<bool, Box<dyn std::error::Error>>
where
    Canvas<Window>: Sized,
    C: 'static,
    Window: sdl2::render::RenderTarget<Context = C>,
{
    let mut setup = firstrun::Setup::new(dest);
    // a file given on the command line (Open with, dragged onto the program) is imported right away
    if let Some(file) = open {
        setup.drop_file(&file.display().to_string());
    }
    let text = video.text_input();
    text.start();
    let clipboard = video.clipboard();
    let started = Instant::now();
    let mut fb = Framebuffer::new();
    let mut tick = 0u32;
    let done = 'page: loop {
        let mut choose = false;
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'page false,
                Event::DropFile { filename, .. } => setup.drop_file(&filename),
                Event::DropText { filename, .. } => setup.drop_file(&filename),
                Event::TextInput { text, .. } => setup.type_text(&text),
                Event::KeyDown { keycode: Some(k), keymod, .. } => match k {
                    Keycode::Backspace => setup.backspace(),
                    Keycode::Up => setup.move_cursor(false),
                    Keycode::Down | Keycode::Tab => setup.move_cursor(true),
                    Keycode::Return | Keycode::KpEnter => {
                        match setup.activate() {
                            firstrun::Action::Quit => break 'page false,
                            firstrun::Action::ChooseFile => choose = true,
                            firstrun::Action::None => {}
                        }
                    }
                    Keycode::Escape => break 'page false,
                    Keycode::V if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD | Mod::LGUIMOD | Mod::RGUIMOD) => {
                        if let Ok(t) = clipboard.clipboard_text() {
                            setup.type_text(t.lines().next().unwrap_or(""));
                        }
                    }
                    Keycode::Insert if keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD) => {
                        if let Ok(t) = clipboard.clipboard_text() {
                            setup.type_text(t.lines().next().unwrap_or(""));
                        }
                    }
                    Keycode::F11 => {
                        let on = canvas.window().fullscreen_state() == FullscreenType::Off;
                        let _ = canvas.window_mut().set_fullscreen(if on { FullscreenType::Desktop } else { FullscreenType::Off });
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if choose {
            // the dialog must run on this (main) thread; it blocks the window while it is open
            text.stop();
            let file = firstrun::choose_file(firstrun::FileKind::DiskImage);
            text.start();
            setup.picked(file);
            // keys and clicks from while the dialog was open are not meant for this page
            events.pump_events();
            for e in events.poll_iter() {
                if let Event::DropFile { filename, .. } = e {
                    setup.drop_file(&filename);
                }
            }
        }
        if setup.poll() {
            break true;
        }
        firstrun::draw(&mut fb, &setup, tick, (started.elapsed().as_millis() / 400) as usize & 1);
        tick = tick.wrapping_add(1);
        let size = canvas.output_size().unwrap_or((WIDTH as u32, HEIGHT as u32));
        presenter.upload(&fb)?;
        presenter.crt = settings.crt;
        presenter.present(canvas, present::layout(settings.scaling, false, size))?;
        canvas.present();
        std::thread::sleep(Duration::from_millis(8));
    };
    text.stop();
    Ok(done)
}

/// How the AUTHENTIC presentation ended.
enum AuthenticExit {
    Quit,
    /// back to the modern presentation (chosen in the overlay)
    ToModern,
    /// the program could not be started or stopped on something the slice cannot do
    Failed(String),
}

/// `PRESENTATION: AUTHENTIC`: runs the original program (see `giana_app::authentic`) in its own loop until the player leaves
/// it. The picture is whole pixels with the border, 50 Hz; the only thing of this app on top is the settings overlay (F1, F10 or
/// the controller's Back button), which stands over the paused picture.
#[allow(clippy::too_many_arguments)]
fn run_authentic<C>(
    canvas: &mut Canvas<Window>,
    presenter: &mut Presenter<'_, C>,
    event_pump: &mut sdl2::EventPump,
    video: &sdl2::VideoSubsystem,
    mouse: &MouseUtil,
    settings: &mut Settings,
    bindings: &mut Bindings,
    input_state: &mut InputState,
    audio: &mut Option<sdl2::audio::AudioDevice<SidAudioCallback>>,
    lock_factor: &mut u32,
    data_dir: &std::path::Path,
    pad_name: Option<String>,
    started: Instant,
) -> Result<AuthenticExit, Box<dyn std::error::Error>>
where
    Window: sdl2::render::RenderTarget<Context = C>,
    C: 'static,
{
    let mut run = match AuthenticRun::new(data_dir, authentic::scores_path()) {
        Ok(r) => r,
        Err(e) => return Ok(AuthenticExit::Failed(e)),
    };
    let mut held = Held::default();
    let mut fb = Framebuffer::new();
    let no_scores = Scores::default();
    let mut menu_sound = MenuSounds::new();
    menu_sound.set_mode(menu_sound_off());
    // the picture of the machine runs at the PAL rate; a display locked to 50 Hz gets 20 ms steps
    const LOGIC_STEP: Duration = Duration::from_micros(19950);
    const LOCKED_STEP: Duration = Duration::from_micros(20000);
    const LOCKED_SLACK: Duration = Duration::from_micros(2000);
    let mut last = Instant::now();
    let mut accumulator = Duration::ZERO;
    let mut pending = Pending::default();
    println!("AUTHENTIC presentation: the original program runs from its title program");
    loop {
        let frame_start = Instant::now();
        accumulator += frame_start.duration_since(last);
        last = frame_start;
        let (step, slack) = if *lock_factor > 0 { (LOCKED_STEP, LOCKED_SLACK.min(LOCKED_STEP / *lock_factor / 4)) } else { (LOGIC_STEP, Duration::ZERO) };
        accumulator = accumulator.min(Duration::from_millis(100));

        if let Some(m) = run.overlay.as_mut() {
            m.settings = settings.clone();
            m.authentic = AuthenticAvail::Ready;
        }
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => return Ok(AuthenticExit::Quit),
                Event::Window { win_event: WindowEvent::FocusLost, .. } => {
                    held.clear();
                    if run.overlay.is_none() {
                        run.overlay = Some(Menu::pause());
                    }
                }
                Event::KeyDown { keycode: Some(k), keymod, repeat: false, .. }
                    if k == Keycode::F11 || (k == Keycode::Return && keymod.intersects(Mod::LALTMOD | Mod::RALTMOD)) =>
                {
                    pending.options.push(OptionChange::Fullscreen);
                }
                Event::KeyDown { keycode: Some(k), repeat, .. } => {
                    input_state.key_used();
                    match run.overlay.as_mut() {
                        Some(m) if m.capturing => {
                            if !repeat {
                                let outcome = if k == Keycode::Escape { m.step(Nav::Back, bindings) } else { m.capture_key(k, bindings) };
                                apply_authentic_outcome(outcome, &mut run, &mut pending, bindings);
                            }
                        }
                        Some(m) => {
                            if let Some(nav) = nav_for_key(k, repeat) {
                                let outcome = menu_step(m, nav, bindings, &mut menu_sound);
                                apply_authentic_outcome(outcome, &mut run, &mut pending, bindings);
                            }
                        }
                        None => {
                            // F1 is the C64 key where the program asks for it ("PRESS F1 TO SAVE HIGHSCORES"); F10 always opens the overlay
                            if (k == Keycode::F1 && !run.f1_is_for_the_program()) || k == Keycode::F10 {
                                if !repeat {
                                    run.overlay = Some(Menu::pause());
                                    held.clear();
                                }
                            } else if !repeat {
                                held.press(Source::Key(k));
                            }
                        }
                    }
                }
                Event::KeyUp { keycode: Some(k), .. } => held.release(Source::Key(k)),
                Event::ControllerButtonDown { button, .. } => {
                    input_state.pad_used();
                    match run.overlay.as_mut() {
                        Some(m) if m.capturing => {
                            let outcome = if button == Button::Start { m.step(Nav::Back, bindings) } else { m.capture_pad(PadInput::Button(button), bindings) };
                            apply_authentic_outcome(outcome, &mut run, &mut pending, bindings);
                        }
                        Some(m) => {
                            if let Some(nav) = nav_for_button(button) {
                                let outcome = menu_step(m, nav, bindings, &mut menu_sound);
                                apply_authentic_outcome(outcome, &mut run, &mut pending, bindings);
                            }
                        }
                        None if button == Button::Back => {
                            run.overlay = Some(Menu::pause());
                            held.clear();
                        }
                        None => held.press(Source::Pad(PadInput::Button(button))),
                    }
                }
                Event::ControllerButtonUp { button, .. } => held.release(Source::Pad(PadInput::Button(button))),
                Event::ControllerAxisMotion { axis, value, .. } => {
                    if value.unsigned_abs() > 16000 {
                        input_state.pad_used();
                    }
                    if let Some(pressed) = held.axis(axis, value) {
                        if let Some(m) = run.overlay.as_mut() {
                            let outcome = if m.capturing {
                                m.capture_pad(pressed, bindings)
                            } else if let Some(nav) = nav_for_stick(pressed) {
                                menu_step(m, nav, bindings, &mut menu_sound)
                            } else {
                                Outcome::None
                            };
                            apply_authentic_outcome(outcome, &mut run, &mut pending, bindings);
                        }
                    }
                }
                _ => {}
            }
        }
        // what the overlay asked for
        for change in std::mem::take(&mut pending.options) {
            match change {
                OptionChange::Fullscreen => {
                    settings.fullscreen = !settings.fullscreen;
                    *lock_factor = apply_fullscreen(canvas, video, mouse, settings.fullscreen, settings.display_50);
                }
                OptionChange::VolumeDown | OptionChange::VolumeUp => settings.change_volume(change == OptionChange::VolumeUp),
                OptionChange::MusicDown | OptionChange::MusicUp => settings.change_music(change == OptionChange::MusicUp),
                OptionChange::EffectsDown | OptionChange::EffectsUp => settings.change_effects(change == OptionChange::EffectsUp),
                OptionChange::MenuSounds(forward) => settings.menu_sounds = settings.menu_sounds.next(forward),
                OptionChange::Scaling(forward) => settings.scaling = settings.scaling.next(forward),
                OptionChange::Mode => settings.enhancements.enhanced = !settings.enhancements.enhanced,
                OptionChange::SmoothMotion => settings.enhancements.smooth_motion = !settings.enhancements.smooth_motion,
                OptionChange::Winner => settings.winner = settings.winner.other(),
                OptionChange::Display50 => {
                    settings.display_50 = !settings.display_50;
                    if settings.fullscreen {
                        *lock_factor = apply_fullscreen(canvas, video, mouse, true, settings.display_50);
                    }
                }
                OptionChange::UpdateCheck => settings.update_check = !settings.update_check,
                OptionChange::Presentation => settings.presentation = settings.presentation.next(),
                OptionChange::Crt => settings.crt = !settings.crt,
            }
            if let Some(d) = audio.as_mut() {
                apply_gains(&mut d.lock().mixer, settings);
            }
            if let Some(path) = settings_path() {
                let _ = settings.save(&path);
            }
        }
        if settings.presentation == Presentation::Modern {
            return Ok(AuthenticExit::ToModern);
        }
        if std::mem::take(&mut pending.to_title) {
            if let Err(e) = run.restart() {
                return Ok(AuthenticExit::Failed(e));
            }
        }

        // machine steps (paused while the overlay is open)
        let mut steps = 0;
        while run.overlay.is_none() && accumulator + slack >= step && steps < 5 {
            accumulator = accumulator.saturating_sub(step);
            steps += 1;
            if let Err(e) = run.frame(&held, bindings) {
                return Ok(AuthenticExit::Failed(e));
            }
            if let Some(dev) = audio.as_mut() {
                let mut d = dev.lock();
                d.mixer.split = false;
                d.mixer.main.live_regs = Some(run.sid());
                d.mixer.main.queue_timed_writes(run.take_sid_writes());
            }
        }
        if run.overlay.is_some() {
            accumulator = Duration::ZERO;
            if let Some(dev) = audio.as_mut() {
                let mut d = dev.lock();
                d.mixer.split = false;
                d.mixer.main.live_regs = Some([0; 25]);
            }
        }

        run.draw(&mut fb);
        let window_size = canvas.output_size().unwrap_or((WIDTH as u32, HEIGHT as u32));
        let layout = present::layout(giana_app::settings::Scaling::Whole, false, window_size);
        if let Some(m) = &run.overlay {
            let nudge = (started.elapsed().as_millis() / 400) as usize & 1;
            let ctx = DrawCtx { bindings, pad_name: pad_name.as_deref(), scores: &no_scores, input: *input_state, nudge, view: ui::View::new(false) };
            menu_draw::draw_over(&mut fb, m, &ctx);
        }
        presenter.upload(&fb)?;
        presenter.crt = settings.crt;
        presenter.present(canvas, layout)?;
        canvas.present();
        if frame_start.elapsed() < Duration::from_millis(1) && accumulator + slack < step {
            std::thread::sleep(step.saturating_sub(accumulator).min(Duration::from_millis(1)));
        }
    }
}

fn menu_sound_off() -> giana_app::settings::MenuSoundMode {
    giana_app::settings::MenuSoundMode::Off
}

/// What a step of the overlay asks for.
fn apply_authentic_outcome(outcome: Outcome, run: &mut AuthenticRun, pending: &mut Pending, bindings: &Bindings) {
    match outcome {
        Outcome::Resume => run.overlay = None,
        Outcome::QuitToTitle => {
            // "QUIT TO TITLE": the computer is switched off and on again
            pending.to_title = true;
            run.overlay = None;
        }
        Outcome::Option(change) => pending.options.push(change),
        Outcome::AuthenticData => pending.authentic_data = true,
        Outcome::Changed => {
            if let Some(path) = config_path() {
                if let Err(e) = bindings.save(&path) {
                    eprintln!("Could not save the controls to {}: {}", path.display(), e);
                }
            }
        }
        _ => {}
    }
}

/// What the main loop does after AUTHENTIC.
enum AuthenticNext {
    Continue,
    Quit,
}

/// Starts AUTHENTIC: asks for the disk image first when the data folder lacks the extra file, then runs the original.
#[allow(clippy::too_many_arguments)]
fn authentic_session<C>(
    canvas: &mut Canvas<Window>,
    presenter: &mut Presenter<'_, C>,
    event_pump: &mut sdl2::EventPump,
    video: &sdl2::VideoSubsystem,
    mouse: &MouseUtil,
    settings: &mut Settings,
    bindings: &mut Bindings,
    input_state: &mut InputState,
    audio: &mut Option<sdl2::audio::AudioDevice<SidAudioCallback>>,
    lock_factor: &mut u32,
    data_dir: &std::path::Path,
    module_active: bool,
    pad_name: Option<String>,
    started: Instant,
    notice: &mut Option<String>,
) -> Result<AuthenticNext, Box<dyn std::error::Error>>
where
    Window: sdl2::render::RenderTarget<Context = C>,
    C: 'static,
{
    let mut leave = |settings: &mut Settings, text: &str, notice: &mut Option<String>| {
        settings.presentation = Presentation::Modern;
        if let Some(path) = settings_path() {
            let _ = settings.save(&path);
        }
        *notice = Some(text.to_string());
    };
    match authentic::availability(data_dir, module_active) {
        AuthenticAvail::Ready => {}
        AuthenticAvail::ModuleActive => {
            leave(settings, "AUTHENTIC NEEDS THE ORIGINAL GAME", notice);
            return Ok(AuthenticNext::Continue);
        }
        AuthenticAvail::NeedsData => {
            // the data folder was made before AUTHENTIC existed: reading the disk image once more adds the file
            if !first_run_page(canvas, presenter, event_pump, video, settings, data_dir.to_path_buf(), None)? || authentic::availability(data_dir, false) != AuthenticAvail::Ready {
                leave(settings, "AUTHENTIC NEEDS THE DISK IMAGE", notice);
                return Ok(AuthenticNext::Continue);
            }
        }
    }
    match run_authentic(canvas, presenter, event_pump, video, mouse, settings, bindings, input_state, audio, lock_factor, data_dir, pad_name, started)? {
        AuthenticExit::Quit => Ok(AuthenticNext::Quit),
        AuthenticExit::ToModern => Ok(AuthenticNext::Continue),
        AuthenticExit::Failed(e) => {
            eprintln!("giana-app: AUTHENTIC stopped: {e}");
            leave(settings, "AUTHENTIC STOPPED - SEE THE LOG", notice);
            Ok(AuthenticNext::Continue)
        }
    }
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting The Great Giana Sisters - Native PC Edition (Rust/SDL2)...");

    let sdl_context = sdl2::init().map_err(|e| startup_error("Could not start SDL", e, SDL_HINT))?;
    let video_subsystem = sdl_context.video().map_err(|e| startup_error("Could not start the video system", e, DISPLAY_HINT))?;
    let game_controller_subsystem = sdl_context.game_controller().ok();
    let audio_subsystem = sdl_context.audio().ok();
    let mouse = sdl_context.mouse();

    let mut settings = settings_path().map_or_else(Settings::default, |p| Settings::load(&p));
    // the leftover of a Windows update goes; a check for a new version runs in the background (at most once a day)
    giana_app::update::cleanup_old();
    let mut updater = giana_app::update::Updater::start(settings.update_check && !options.no_update_check, settings.last_update_check, &settings.update_seen);

    // Check for connected gamepads
    let mut input_state = InputState::default();
    let mut active_controller = None;
    if let Some((c, kind)) = game_controller_subsystem.as_ref().and_then(|mgr| open_controller(mgr, None)) {
        println!("Connected gamepad: {}", c.name());
        input_state.pad_connected(kind);
        active_controller = Some(c);
    }
    let started = Instant::now();

    let mut window = video_subsystem
        .window("The Great Giana Sisters", (WIDTH * 3) as u32, (HEIGHT * 3) as u32)
        .position_centered()
        .resizable()
        .build()
        .map_err(|e| startup_error("Could not open the window", e, DISPLAY_HINT))?;
    let _ = window.set_minimum_size(WIDTH as u32, HEIGHT as u32);
    // window and taskbar icon (64x64 RGBA, drawn by assets/icon/make_icon.py)
    let mut icon = include_bytes!("../../../assets/icon/window-icon.rgba").to_vec();
    if let Ok(surface) = Surface::from_data(&mut icon, 64, 64, 64 * 4, PixelFormatEnum::RGBA32) {
        window.set_icon(surface);
    }

    // No `accelerated()`: SDL takes the best renderer it finds, also the software one on computers without a working GPU driver
    let mut canvas = window
        .into_canvas()
        .present_vsync()
        .build()
        .map_err(|e| startup_error("Could not create the renderer", e, "The graphics driver may be missing or broken. Try updating it, or set SDL_RENDER_DRIVER=software."))?;
    // Where the picture goes in the window is worked out per frame (see `present`): the scaling mode decides
    let mut lock_factor = apply_fullscreen(&mut canvas, &video_subsystem, &mouse, settings.fullscreen, settings.display_50);
    let texture_creator = canvas.texture_creator();
    let mut presenter = Presenter::new(&texture_creator).map_err(|e| startup_error("Could not create the picture buffer", e, DISPLAY_HINT))?;
    // The refresh rate of the display the window is on, for SMOOTH MOTION (0: unknown); looked up now and then
    let mut display_hz = canvas.window().display_mode().map_or(0, |m| m.refresh_rate);
    let mut display_hz_checked = Instant::now();
    // The last two game steps as the video chip drew them, for SMOOTH MOTION; `vic_steps` counts how many are valid
    let mut vic_prev = VicSnapshot::default();
    let mut vic_curr = VicSnapshot::default();
    let mut vic_steps = 0u8;

    let mut event_pump = sdl_context.event_pump().map_err(|e| startup_error("Could not read the keyboard and mouse", e, DISPLAY_HINT))?;

    // The original game data: found in a data folder, or asked for (the first start). The game itself contains none of it.
    let data_dir = match firstrun::locate(options.data.as_deref()) {
        Some(dir) => dir,
        None => {
            let dest = firstrun::destination(options.data.as_deref()).ok_or("There is no folder for the game data. Give one with --data DIR.")?;
            println!("No game data found. Asking for the disk image; the data will go to {}", dest.display());
            if !first_run_page(&mut canvas, &mut presenter, &mut event_pump, &video_subsystem, &settings, dest.clone(), options.open.clone())? {
                return Ok(());
            }
            dest
        }
    };
    // The ROM module to play: from the command line (this run only), else the one chosen in the settings. Any problem
    // plays the original game and says so in the menu.
    let store = Store::default_location();
    let mut module_id = String::new();
    let mut module_notice: Option<String> = None;
    let wanted: Option<String> = {
        let mut wanted = None;
        let mut add = |file: &std::path::Path, settings: &mut Settings, remember: bool| match store.as_ref().ok_or_else(|| "there is no folder for the modules".to_string()).and_then(|s| s.add(file, &data_dir)) {
            Ok(added) => {
                println!("ROM module added: {} ({})", added.meta.name, added.id);
                if remember {
                    settings.module = added.id.clone();
                    if let Some(path) = settings_path() {
                        let _ = settings.save(&path);
                    }
                }
                Some(added.id)
            }
            Err(e) => {
                eprintln!("giana-app: could not add {}: {e}", file.display());
                module_notice = Some("ROM MODULE COULD NOT BE LOADED".to_string());
                None
            }
        };
        if let Some(file) = options.open_module.as_deref() {
            // "Open with" and a file dropped on the program: added and chosen for good
            wanted = add(file, &mut settings, true);
        } else if let Some(arg) = options.module.as_deref() {
            let path = std::path::Path::new(arg);
            if path.is_file() {
                wanted = add(path, &mut settings, false);
            } else if arg.is_empty() || arg.eq_ignore_ascii_case("original") {
                // the original game
            } else if let Some(found) = store.as_ref().and_then(|s| s.find(arg)) {
                wanted = Some(found.id);
            } else {
                eprintln!("giana-app: there is no ROM module '{arg}'. See --list-modules.");
                module_notice = Some("ROM MODULE COULD NOT BE LOADED".to_string());
            }
        } else if !settings.module.is_empty() {
            wanted = Some(settings.module.clone());
        }
        wanted
    };
    let mut loaded = false;
    if let (Some(id), Some(store)) = (wanted.as_deref(), store.as_ref()) {
        match store.load_selected(id, &data_dir) {
            Ok((data, applied, meta)) => {
                giana_core::data::install(data);
                giana_core::features::set_construction_kit(match applied.family {
                    giana_module::Family::ConstructionKit1989 { variant: giana_module::CkVariant::Basic } => giana_core::features::ConstructionKit::Basic,
                    giana_module::Family::ConstructionKit1989 { variant: giana_module::CkVariant::Full } => giana_core::features::ConstructionKit::Full,
                    _ => giana_core::features::ConstructionKit::Off,
                });
                // family B (the 7A3 construction-set engine): the module's disk files go to engine B, `None` = original engine
                giana_core::features::set_engine_b((applied.engine.as_deref() == Some("b")).then(|| applied.files.clone()));
                // family T (TNW Giana Sisters 2): its stage files (the original stages already restored)
                giana_core::features::set_tnw((applied.engine.as_deref() == Some("t")).then(|| applied.files.clone()));
                println!("ROM module: {} ({id})", meta.name);
                module_id = id.to_string();
                // LAST PLAYED on the modules page
                settings.note_played(id, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()));
                if let Some(path) = settings_path() {
                    let _ = settings.save(&path);
                }
                loaded = true;
            }
            Err(e) => {
                eprintln!("giana-app: could not load the ROM module {id}: {e}");
                module_notice = Some("ROM MODULE COULD NOT BE LOADED".to_string());
            }
        }
    }
    if !loaded {
        firstrun::load(&data_dir).map_err(|e| startup_error("Could not load the game data", e, "Delete the data folder and start the game again to extract it anew."))?;
    }
    println!("Game data: {}", data_dir.display());
    let mut module_list = module_infos(store.as_ref(), &data_dir, &settings);
    let mut authentic_avail = authentic::availability(&data_dir, !module_id.is_empty());

    // Initialize Native Chris Hülsbeck Sound Synthesizer via SDL2 Audio
    let mut audio_device = if let Some(ref audio_sub) = audio_subsystem {
        let desired_spec = AudioSpecDesired {
            freq: Some(44100),
            channels: Some(1), // Mono
            // 512 samples (11.6 ms) per callback: half the delay of 1024, still no underruns on PipeWire
            samples: Some(512),
        };

        match audio_sub.open_playback(None, &desired_spec, |_spec| {
            let mut mixer = Mixer::new();
            apply_gains(&mut mixer, &settings);
            SidAudioCallback { mixer }
        }) {
            Ok(device) => {
                device.resume();
                println!("Chris Hülsbeck SID synthesizer initialized @ 44.1 kHz");
                Some(device)
            }
            Err(e) => {
                eprintln!("Warning: Failed to open audio device: {}", e);
                None
            }
        }
    } else {
        None
    };

    let audio_ok = audio_device.is_some();

    let mut fb = Framebuffer::new();
    let mut game = GameState::new();
    let mut input = ControllerInput::new();
    let mut bindings = config_path().map_or_else(Bindings::default, |p| Bindings::load(&p));
    let mut held = Held::default();
    let mut open_menu = Some(Menu::main());
    if let Some(m) = open_menu.as_mut() {
        m.notice = module_notice.take();
    }
    let mut pending = Pending::default();
    let mut scores = scores_path(&module_id).map_or_else(Scores::default, |p| Scores::load(&p));
    let mut hosting: Option<Host> = None;
    let mut finder: Option<Finder> = None;
    let mut search_started = Instant::now();
    let mut joining: Option<Joiner> = None;
    let mut join_address = String::new();
    // A short message at the bottom while playing, e.g. when the sound is switched off
    let mut toast: Option<(&'static str, Instant)> = None;
    let mut session: Option<Session> = None;
    // The other player's game when each has their own
    let mut other_game: Option<GameState> = None;
    let mut network_start = false;
    let mut last_mode = Some(GameMode::OnePlayer);

    // Start on Title screen
    game.screen = GameScreen::Title;
    // `--stage N` skips the menu and starts in that stage
    if let Some(stage) = options.stage {
        game = GameState::new_port_game_at(stage);
        open_menu = None;
    }
    let mut active_track = MusicTrack::Title;
    // The sounds of the menus, made by a sound driver of their own (it also plays the title tune outside a game)
    let mut menu_sound = MenuSounds::new();
    menu_sound.set_mode(settings.menu_sounds);
    menu_sound.set_split(settings.split_sound(), true);
    menu_sound.start_title();
    let mut sound_source = SoundSource::Title;
    // Music and effects on chips of their own (a slider is down): the drivers that the game's calls are replayed on, and
    // which of the two games (0 yours, 1 the other player's) they follow
    let mut split_game: Option<(SplitDrivers, u8)> = None;

    // Logic tick rate: authentic 50 Hz PAL (19950 µs per frame as in the Oracle: 312 lines * 63 cycles / 985248 Hz)
    const LOGIC_STEP: Duration = Duration::from_micros(19950);
    // On a display locked to exactly 50 Hz every frame is 20000 µs. A step of 19950 µs would then need a second step in the
    // same frame every 400 frames (a visible hiccup), and jitter of the frame time would sometimes skip one. So there one
    // step is one frame: 20000 µs (0.25 % slower than the PAL rate, which nobody can hear or see), and a step is also taken
    // when the frame arrives up to 2 ms early. At 100, 150 or 200 Hz the step is 2, 3 or 4 frames long, and the slack is a
    // quarter of a frame at most (so that a step never starts a frame too early).
    const LOCKED_STEP: Duration = Duration::from_micros(20000);
    const LOCKED_SLACK: Duration = Duration::from_micros(2000);

    let mut last_frame_time = Instant::now();
    let mut accumulator = Duration::ZERO;

    // `PRESENTATION: AUTHENTIC` in the settings: the original from its title program on, until the player leaves it
    if settings.presentation == Presentation::Authentic {
        let pad_name = active_controller.as_ref().map(|c| c.name());
        let mut notice = None;
        let next = authentic_session(&mut canvas, &mut presenter, &mut event_pump, &video_subsystem, &mouse, &mut settings, &mut bindings, &mut input_state, &mut audio_device, &mut lock_factor, &data_dir, !module_id.is_empty(), pad_name, started, &mut notice)?;
        if let AuthenticNext::Quit = next {
            return Ok(());
        }
        if let Some(m) = open_menu.as_mut() {
            m.notice = notice.or(m.notice.take());
        }
        authentic_avail = authentic::availability(&data_dir, !module_id.is_empty());
        last_frame_time = Instant::now();
    }

    'running: loop {
        let frame_start = Instant::now();
        let frame_delta = frame_start.duration_since(last_frame_time);
        last_frame_time = frame_start;
        accumulator += frame_delta;

        let (logic_step, slack) = if lock_factor > 0 { (LOCKED_STEP, LOCKED_SLACK.min(LOCKED_STEP / lock_factor / 4)) } else { (LOGIC_STEP, Duration::ZERO) };
        if display_hz_checked.elapsed() > Duration::from_secs(1) {
            display_hz_checked = Instant::now();
            display_hz = canvas.window().display_mode().map_or(0, |m| m.refresh_rate);
        }

        // Cap accumulator to avoid spiral of death (max 5 logic frames = ~100 ms)
        if accumulator > Duration::from_millis(100) {
            accumulator = Duration::from_millis(100);
        }

        // Reset single-frame action triggers
        input.next_stage = false;
        input.prev_stage = false;
        input.toggle_invulnerable = false;
        input.toggle_lives = false;

        if let Some(m) = open_menu.as_mut() {
            m.settings = settings.clone();
            m.audio_ok = audio_ok;
            m.in_network = session.is_some();
            m.update = updater.view.clone();
            m.set_modules(&module_list, &module_id);
            m.authentic = authentic_avail;
        }
        if let Some(change) = updater.poll() {
            settings.last_update_check = change.last_check.unwrap_or(settings.last_update_check);
            settings.update_seen = change.seen.unwrap_or_default();
            save_settings(&settings, &mut open_menu);
        }
        if std::mem::take(&mut pending.update) {
            updater.install();
        }

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,

                // A file dropped on the window while the page of the ROM modules is open is added
                Event::DropFile { filename, .. } if open_menu.as_ref().is_some_and(|m| m.page == menu::Page::Modules) => {
                    add_module_file(store.as_ref(), &data_dir, std::path::Path::new(&filename), &settings, &mut open_menu, &mut module_list);
                }

                Event::Window { win_event: WindowEvent::FocusLost, .. } => {
                    // Held keys stay down for ever if the release goes to another window
                    held.clear();
                    // A game that runs on its own would go on without the player. A network game cannot wait.
                    if open_menu.is_none() && game.screen == GameScreen::InGame && session.is_none() {
                        open_menu = Some(Menu::pause());
                    }
                }

                // Fullscreen on F11 and on Alt+Enter, wherever the player is
                Event::KeyDown { keycode: Some(k), keymod, repeat: false, .. }
                    if k == Keycode::F11 || (k == Keycode::Return && keymod.intersects(Mod::LALTMOD | Mod::RALTMOD)) =>
                {
                    pending.options.push(OptionChange::Fullscreen);
                }

                // Mute on M, unless M is typed or bound
                Event::KeyDown { keycode: Some(Keycode::M), repeat: false, .. }
                    if open_menu.as_ref().map_or(true, |m| !m.capturing && m.page != menu::Page::EnterName)
                        && !bindings.keys.iter().flatten().any(|&b| b == Some(Keycode::M)) =>
                {
                    settings.muted = !settings.muted;
                    if let Some(d) = audio_device.as_mut() {
                        apply_gains(&mut d.lock().mixer, &settings);
                    }
                    toast =Some((if settings.muted { "SOUND OFF" } else { "SOUND ON" }, Instant::now()));
                    save_settings(&settings, &mut open_menu);
                }

                Event::KeyDown { keycode: Some(k), repeat, .. } => {
                    input_state.key_used();
                    if !repeat {
                        held.press(Source::Key(k));
                    }
                    match open_menu.as_mut() {
                        Some(m) if m.capturing => {
                            if !repeat {
                                let outcome = if k == Keycode::Escape { m.step(Nav::Back, &mut bindings) } else { m.capture_key(k, &mut bindings) };
                                apply_outcome(outcome, &mut open_menu, &mut pending, &bindings);
                            }
                        }
                        Some(m) => {
                            let letter = k.name().chars().next().filter(|c| k.name().len() == 1 && c.is_ascii_alphanumeric());
                            if let Some(c) = address_char(k).filter(|_| m.page == menu::Page::Join) {
                                m.type_char(c);
                            } else if let Some(c) = letter.filter(|_| m.page == menu::Page::EnterName) {
                                m.type_name(c);
                            } else if k == Keycode::Backspace && m.delete_char() {
                                // the character is gone, nothing else to do
                            } else if k == Keycode::U && !repeat && m.request_update() == Outcome::Update {
                                apply_outcome(Outcome::Update, &mut open_menu, &mut pending, &bindings);
                            } else if let Some(nav) = nav_for_key(k, repeat) {
                                let outcome = menu_step(m, nav, &mut bindings, &mut menu_sound);
                                apply_outcome(outcome, &mut open_menu, &mut pending, &bindings);
                            }
                        }
                        None => match k {
                            Keycode::Escape if !repeat => open_menu = Some(Menu::pause()),
                            // The debug keys are for testing: they spoil a normal game
                            Keycode::F1 if options.debug => input.toggle_lives = true,
                            Keycode::F2 if options.debug => input.toggle_invulnerable = true,
                            Keycode::F3 if options.debug => input.prev_stage = true,
                            Keycode::F4 if options.debug => input.next_stage = true,
                            _ => {}
                        },
                    }
                }
                Event::KeyUp { keycode: Some(k), .. } => held.release(Source::Key(k)),

                // Gamepad input
                Event::ControllerButtonDown { button, .. } => {
                    input_state.pad_used();
                    held.press(Source::Pad(PadInput::Button(button)));
                    match open_menu.as_mut() {
                        Some(m) if m.capturing => {
                            let outcome = if button == Button::Start { m.step(Nav::Back, &mut bindings) } else { m.capture_pad(PadInput::Button(button), &mut bindings) };
                            apply_outcome(outcome, &mut open_menu, &mut pending, &bindings);
                        }
                        Some(m) => {
                            if button == Button::X && m.request_update() == Outcome::Update {
                                apply_outcome(Outcome::Update, &mut open_menu, &mut pending, &bindings);
                            } else if let Some(nav) = nav_for_button(button) {
                                let outcome = menu_step(m, nav, &mut bindings, &mut menu_sound);
                                apply_outcome(outcome, &mut open_menu, &mut pending, &bindings);
                            }
                        }
                        None if button == Button::Start => open_menu = Some(Menu::pause()),
                        None => {}
                    }
                }
                Event::ControllerButtonUp { button, .. } => held.release(Source::Pad(PadInput::Button(button))),
                Event::ControllerAxisMotion { axis, value, .. } => {
                    // a stick at rest drifts a little: only a real push counts as using the controller
                    if value.unsigned_abs() > 16000 {
                        input_state.pad_used();
                    }
                    if let (Some(pressed), Some(m)) = (held.axis(axis, value), open_menu.as_mut()) {
                        let outcome = if m.capturing {
                            m.capture_pad(pressed, &mut bindings)
                        } else if let Some(nav) = nav_for_stick(pressed) {
                            menu_step(m, nav, &mut bindings, &mut menu_sound)
                        } else {
                            Outcome::None
                        };
                        apply_outcome(outcome, &mut open_menu, &mut pending, &bindings);
                    }
                }

                // Controllers can come and go while the game runs
                Event::ControllerDeviceAdded { which, .. } if active_controller.is_none() => {
                    if let Some(c) = game_controller_subsystem.as_ref().and_then(|mgr| mgr.open(which).ok()) {
                        println!("Connected gamepad: {}", c.name());
                        input_state.pad_connected(pad_kind(which, &c.name()));
                        active_controller = Some(c);
                    }
                }
                Event::ControllerDeviceRemoved { which, .. } => {
                    if active_controller.as_ref().map_or(false, |c| c.instance_id() == which) {
                        drop(active_controller.take());
                        held.clear_pad();
                        input_state.pad_removed();
                        // Another controller that is already connected takes over
                        if let Some((c, kind)) = game_controller_subsystem.as_ref().and_then(|mgr| open_controller(mgr, Some(which))) {
                            println!("Connected gamepad: {}", c.name());
                            input_state.pad_connected(kind);
                            active_controller = Some(c);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(entry) = pending.new_score.take() {
            let place = scores.insert(entry);
            if let Some(m) = open_menu.as_mut() {
                m.score_added(place);
            }
            save_scores(&scores, &module_id, &mut open_menu);
        }
        let mut launch_authentic = std::mem::take(&mut pending.authentic_data);
        for change in std::mem::take(&mut pending.options) {
            match change {
                OptionChange::Presentation => {
                    settings.presentation = settings.presentation.next();
                    launch_authentic = settings.presentation == Presentation::Authentic;
                }
                OptionChange::Fullscreen => {
                    settings.fullscreen = !settings.fullscreen;
                    lock_factor = apply_fullscreen(&mut canvas, &video_subsystem, &mouse, settings.fullscreen, settings.display_50);
                    display_hz = canvas.window().display_mode().map_or(0, |m| m.refresh_rate);
                }
                OptionChange::VolumeDown | OptionChange::VolumeUp => settings.change_volume(change == OptionChange::VolumeUp),
                OptionChange::MusicDown | OptionChange::MusicUp => settings.change_music(change == OptionChange::MusicUp),
                OptionChange::EffectsDown | OptionChange::EffectsUp => settings.change_effects(change == OptionChange::EffectsUp),
                OptionChange::MenuSounds(forward) => {
                    settings.menu_sounds = settings.menu_sounds.next(forward);
                    menu_sound.set_mode(settings.menu_sounds);
                }
                OptionChange::Scaling(forward) => settings.scaling = settings.scaling.next(forward),
                OptionChange::Crt => settings.crt = !settings.crt,
                OptionChange::Mode | OptionChange::SmoothMotion => {
                    if change == OptionChange::Mode {
                        settings.enhancements.enhanced = !settings.enhancements.enhanced;
                    } else {
                        settings.enhancements.smooth_motion = !settings.enhancements.smooth_motion;
                    }
                    // A game that is waiting for the other player carries the rules of the host to the one who joins
                    if let Some(h) = hosting.as_mut() {
                        h.set_rules(settings.enhancements.rules());
                    }
                }
                OptionChange::Winner => {
                    settings.winner = settings.winner.other();
                    // A game that is waiting for the other player carries the rule to the one who joins
                    if let Some(h) = hosting.as_mut() {
                        h.set_winner(settings.winner);
                    }
                }
                OptionChange::UpdateCheck => settings.update_check = !settings.update_check,
                OptionChange::Display50 => {
                    settings.display_50 = !settings.display_50;
                    if settings.fullscreen {
                        lock_factor = apply_fullscreen(&mut canvas, &video_subsystem, &mouse, true, settings.display_50);
                        display_hz = canvas.window().display_mode().map_or(0, |m| m.refresh_rate);
                    }
                }
            }
            if let Some(d) = audio_device.as_mut() {
                apply_gains(&mut d.lock().mixer, &settings);
            }
            if let Some(m) = open_menu.as_mut() {
                m.settings = settings.clone();
            }
            save_settings(&settings, &mut open_menu);
        }
        // PRESENTATION: AUTHENTIC was chosen (or its missing data asked for): run the original until the player leaves it
        if launch_authentic {
            held.clear();
            let pad_name = active_controller.as_ref().map(|c| c.name());
            let mut notice = None;
            if authentic_avail == AuthenticAvail::NeedsData && settings.presentation != Presentation::Authentic {
                // the disk image is asked for first; AUTHENTIC starts when it worked
                settings.presentation = Presentation::Authentic;
            }
            let next = authentic_session(&mut canvas, &mut presenter, &mut event_pump, &video_subsystem, &mouse, &mut settings, &mut bindings, &mut input_state, &mut audio_device, &mut lock_factor, &data_dir, !module_id.is_empty(), pad_name, started, &mut notice)?;
            if let AuthenticNext::Quit = next {
                break 'running;
            }
            authentic_avail = authentic::availability(&data_dir, !module_id.is_empty());
            if let Some(m) = open_menu.as_mut() {
                m.settings = settings.clone();
                m.notice = notice;
            }
            save_settings(&settings, &mut open_menu);
            last_frame_time = Instant::now();
            accumulator = Duration::ZERO;
            display_hz = canvas.window().display_mode().map_or(0, |m| m.refresh_rate);
        }
        if std::mem::take(&mut pending.add_module) {
            // the dialog must run on this (main) thread; it blocks the window while it is open. The music is driven by
            // the game's frames, so without new frames the sound device would loop its last notes: it pauses meanwhile.
            if let Some(d) = audio_device.as_mut() {
                d.pause();
            }
            let file = firstrun::choose_file(firstrun::FileKind::RomModule);
            if let Some(d) = audio_device.as_mut() {
                d.resume();
            }
            // keys and clicks from while the dialog was open are not meant for the game
            held.clear();
            event_pump.pump_events();
            let mut dropped = None;
            for e in event_pump.poll_iter() {
                if let Event::DropFile { filename, .. } = e {
                    dropped = Some(std::path::PathBuf::from(filename));
                }
            }
            if let Some(file) = file.or(dropped) {
                add_module_file(store.as_ref(), &data_dir, &file, &settings, &mut open_menu, &mut module_list);
            }
        }
        if let Some(id) = pending.remove_module.take() {
            if let Some(store) = store.as_ref() {
                match store.remove(&id) {
                    Ok(()) => {
                        module_list = module_infos(Some(store), &data_dir, &settings);
                        // a module that was chosen but not loaded (it failed) is not chosen any more
                        if settings.module == id {
                            settings.module.clear();
                            save_settings(&settings, &mut open_menu);
                        }
                    }
                    Err(e) => {
                        eprintln!("Could not remove the ROM module: {e}");
                        if let Some(m) = open_menu.as_mut() {
                            m.module_added(None, Some(e));
                        }
                    }
                }
            }
        }
        let favorites = std::mem::take(&mut pending.favorite_modules);
        let sort = std::mem::take(&mut pending.sort_modules);
        if sort || !favorites.is_empty() {
            for id in &favorites {
                settings.toggle_favorite(id);
            }
            if sort {
                settings.module_sort = settings.module_sort.next();
            }
            module_list = module_infos(store.as_ref(), &data_dir, &settings);
            save_settings(&settings, &mut open_menu);
        }
        if let Some(id) = pending.play_module.take() {
            // Another module means another set of data: the game starts again with it (see `modules::spawn_restart`)
            settings.module = id;
            save_settings(&settings, &mut open_menu);
            RESTART.store(true, Ordering::SeqCst);
            break 'running;
        }
        if pending.quit {
            if let Some(s) = session.as_mut() {
                s.leave();
            }
            break 'running;
        }
        bindings.apply(&held, &mut input);

        // Network: open a game, look for one, connect
        if std::mem::take(&mut pending.stop_network) {
            hosting = None;
            finder = None;
            joining = None;
        }
        if std::mem::take(&mut pending.cancel_join) {
            joining = None;
        }
        if let Some(mode) = pending.host.take() {
            let m = open_menu.as_mut();
            let host = Host::new(mode).map(|mut h| {
                h.set_winner(settings.winner);
                h.set_rules(settings.enhancements.rules());
                h.set_module(&module_id);
                if let Some(s) = store.as_ref() {
                    h.share_module(s);
                }
                h
            });
            match (host, m) {
                (Ok(h), Some(m)) => {
                    m.host_port = h.port();
                    m.host_findable = h.can_be_found();
                    hosting = Some(h);
                    m.page = menu::Page::Hosting;
                    m.my_ip = net::local_ip().map_or("NO NETWORK".to_string(), |ip| ip.to_string());
                }
                (Err(e), Some(m)) => {
                    eprintln!("Could not open a network game: {}", e);
                    m.notice = Some("COULD NOT OPEN A GAME".to_string());
                }
                _ => {}
            }
        }
        if std::mem::take(&mut pending.search) {
            search_started = Instant::now();
            finder = Finder::new().map_err(|e| eprintln!("Could not search for games: {e}")).ok();
            if let (None, Some(m)) = (&finder, open_menu.as_mut()) {
                m.search = Search::Unavailable;
            }
        }
        if let Some(f) = finder.as_mut() {
            f.poll();
            if let Some(m) = open_menu.as_mut() {
                m.hosts = f.hosts.clone();
                if f.broken() {
                    m.search = Search::Unavailable;
                } else if f.hosts.is_empty() && search_started.elapsed() > Duration::from_secs(5) && m.search == Search::Looking {
                    m.search = Search::NothingFound;
                }
            }
        }
        let mut connected = None;
        if let Some(addr) = pending.join.take().filter(|_| joining.is_none()) {
            // The connection is made on a thread: a host that does not answer must not freeze the window
            joining = Some(Joiner::start_receiving(&addr, &module_id, store.clone().map(|store| net::ModuleSink { store, data_dir: data_dir.clone() })));
            join_address = addr;
            if let Some(m) = open_menu.as_mut() {
                m.connecting = true;
            }
        }
        if let Some(j) = joining.as_mut() {
            match j.poll() {
                Some(Ok(s)) => {
                    joining = None;
                    settings.last_address = join_address.clone();
                    save_settings(&settings, &mut open_menu);
                    connected = Some(s);
                }
                Some(Err(e)) => {
                    joining = None;
                    eprintln!("Could not join {}: {}", join_address, e);
                    // a received ROM module is only stored (checked against our own data); the player picks it under
                    // ROM MODULES if they want to - a host must not switch our game or restart it
                    if let Some(m) = open_menu.as_mut() {
                        m.connecting = false;
                        m.notice = Some(e.text().to_string());
                    }
                }
                None => {}
            }
        }
        if let Some(h) = hosting.as_mut() {
            match h.poll() {
                Some(Ok(s)) => connected = Some(s),
                Some(Err(e)) => {
                    eprintln!("A connection failed: {}", e);
                    match (&e, open_menu.as_mut()) {
                        (NetError::WrongVersion, Some(m)) => m.notice = Some("ANOTHER VERSION TRIED TO JOIN".to_string()),
                        (NetError::OtherModule, Some(m)) => m.notice = Some("ANOTHER ROM MODULE TRIED TO JOIN".to_string()),
                        _ => {}
                    }
                }
                None => {}
            }
        }
        if let Some(s) = connected {
            network_start = true;
            hosting = None;
            finder = None;
            joining = None;
            open_menu = None;
            session = Some(s);
        }

        // Logic tick(s) via time accumulator at exact 50 Hz
        while accumulator + slack >= logic_step {
            accumulator = accumulator.saturating_sub(logic_step);

            let prev_screen = game.screen;
            if let Some(m) = open_menu.as_mut() {
                m.tick();
            }
            if let Some(s) = session.as_mut().filter(|_| network_start) {
                // Both computers start the same game
                network_start = false;
                last_mode = None;
                (game, other_game) = s.new_games();
            } else if let (Some(s), Some(other)) = (session.as_mut(), other_game.as_mut()) {
                s.tick_same_time(&mut game, other, &input, open_menu.is_some());
            } else if let Some(s) = session.as_mut() {
                s.tick(&mut game, &input, open_menu.is_some());
            } else if let Some(mode) = pending.start.take() {
                last_mode = Some(mode);
                // The ported game takes over (stage 1 start of the original)
                game = match mode {
                    GameMode::OnePlayer => GameState::new_port_game(),
                    GameMode::TwoPlayers => GameState::new_port_game_two(),
                };
            } else if open_menu.is_none() {
                game.update(&input);
            }
            // The best stage reached with this module (or the original game) for the modules page. A game started
            // with `--stage` or played with the debug keys says nothing about how far the player got.
            if game.screen == GameScreen::InGame && !options.debug && options.stage.is_none() && settings.note_stage(&module_id, game.stage) {
                save_settings(&settings, &mut open_menu);
            }

            let ended = session.as_ref().and_then(|s| s.ended);
            if pending.to_title {
                if let Some(s) = session.as_mut() {
                    s.leave();
                }
            }
            // Each with a game of their own: the connection ends while this player still plays, then the game goes on alone
            if ended.is_some() && !game.game_over && other_game.is_some() && !pending.to_title {
                session = None;
                other_game = None;
            }
            let ended = ended.filter(|_| session.is_some());
            let both_out = game.game_over && other_game.as_ref().map_or(true, |o| o.game_over || ended.is_some());
            if both_out || pending.to_title || ended.is_some() {
                let early = std::mem::take(&mut pending.to_title);
                // Who played here (the host is Giana), and the rule of the winner: in a network game the host's
                let me = session.as_ref().map(|s| s.me);
                let rule = session.as_ref().map_or(settings.winner, |s| s.winner);
                let was_network = session.take().is_some();
                let other = other_game.take();
                let finished = ended.is_none() && game.game_over && !early;
                let mut m = open_menu.take().unwrap_or_else(Menu::main);
                if let Some(text) = ended {
                    m = Menu::main();
                    m.notice = Some(text.to_string());
                }
                m.select_mode(last_mode);
                // Points that were played for count, also when the game was left early
                if game.screen == GameScreen::InGame {
                    let mut results = game_results(&game);
                    if let Some(me) = me {
                        // Each computer keeps its own player's score, whether the players play at the same time or take turns
                        results.retain(|r| r.0 == player_name(me));
                    }
                    m.finish_game(results, &scores);
                    // The original names no winner; a game of two players ends with a result, which stays for a while
                    if finished {
                        if let Some(o) = &other {
                            m.show_result(same_time_result(rule, &game, o));
                        } else if was_network || last_mode == Some(GameMode::TwoPlayers) {
                            m.show_result(turns_result(rule, &game));
                        }
                    }
                }
                open_menu = Some(m);
                game = GameState::new();
                game.screen = GameScreen::Title;
            }

            // The ported sound driver feeds the SID while the port runs. Outside a game, and while it is paused, the sound
            // driver of the menu does: title tune and menu sounds, or the menu sounds alone.
            let in_game = game.screen == GameScreen::InGame && !game.legacy_model;
            let source = match (in_game, open_menu.is_some()) {
                (true, false) => SoundSource::Game,
                (true, true) => SoundSource::Paused,
                (false, _) => SoundSource::Title,
            };
            if source != sound_source {
                match source {
                    SoundSource::Title => menu_sound.start_title(),
                    SoundSource::Paused => menu_sound.silence(),
                    SoundSource::Game => menu_sound.clear(),
                }
                sound_source = source;
            }
            // MUSIC and EFFECTS: with a slider down, two chips play (see `giana_audio::split`), otherwise the one chip of the original
            let split = settings.split_sound();
            if split != menu_sound.is_split() {
                menu_sound.set_split(split, source == SoundSource::Title);
            }
            if let Some(ref mut dev) = audio_device {
                if source == SoundSource::Game {
                    let which = u8::from(other_game.is_some() && game.game_over);
                    if split {
                        let (music, effects) = follow_split(&mut split_game, &mut game, other_game.as_mut(), which);
                        let mut d = dev.lock();
                        d.mixer.split = true;
                        d.mixer.main.live_regs = Some(music);
                        d.mixer.effects.live_regs = Some(effects);
                    } else {
                        stop_recording(&mut split_game, &mut game, other_game.as_mut());
                        let shown = other_game.as_ref().filter(|_| game.game_over).unwrap_or(&game);
                        let mut regs = [0u8; 25];
                        regs.copy_from_slice(&shown.sound.sid[..25]);
                        let mut d = dev.lock();
                        d.mixer.split = false;
                        d.mixer.main.live_regs = Some(regs);
                    }
                } else {
                    stop_recording(&mut split_game, &mut game, other_game.as_mut());
                    let mut d = dev.lock();
                    if game.legacy_model && game.screen == GameScreen::InGame {
                        d.mixer.split = false;
                        d.mixer.main.live_regs = None;
                    } else {
                        let (main, fx) = menu_sound.step();
                        d.mixer.split = fx.is_some();
                        d.mixer.main.live_regs = Some(main);
                        if let Some(fx) = fx {
                            d.mixer.effects.live_regs = Some(fx);
                        }
                    }
                }
            }

            // Music track transition
            if game.screen != prev_screen {
                let next_track = match game.screen {
                    GameScreen::Intro | GameScreen::Title => MusicTrack::Title,
                    GameScreen::InGame | GameScreen::StageClear => MusicTrack::Stage1,
                    GameScreen::GameOver | GameScreen::GameWon => MusicTrack::Title,
                };
                if next_track != active_track {
                    active_track = next_track;
                    if let Some(ref mut dev) = audio_device {
                        dev.lock().mixer.main.load_track(next_track);
                    }
                }
            }

            // SMOOTH MOTION: keep the picture of this step and the one before it
            if settings.enhancements.smooth_motion() && game.screen == GameScreen::InGame && !game.legacy_model && open_menu.is_none() {
                let shown = other_game.as_ref().filter(|_| game.game_over).unwrap_or(&game);
                std::mem::swap(&mut vic_prev, &mut vic_curr);
                vic_curr.set(&VicView::from_game(shown));
                vic_steps = (vic_steps + 1).min(2);
            } else {
                vic_steps = 0;
            }
        }

        // Render pass (per vsync)
        match game.screen {
            GameScreen::Intro => {
                fb.draw_intro_screen();
            }
            GameScreen::Title => {
                fb.draw_title_screen();
            }
            GameScreen::InGame if !game.legacy_model => {
                let shown = other_game.as_ref().filter(|_| game.game_over).unwrap_or(&game);
                // SMOOTH MOTION on a display that is not a multiple of 50 Hz: the picture between the last two steps
                let smooth = vic_steps >= 2 && open_menu.is_none() && lock_factor == 0 && display::is_odd_rate(display_hz) && settings.enhancements.smooth_motion();
                if smooth {
                    let t = accumulator.as_secs_f64() / logic_step.as_secs_f64();
                    render_vic_between(&mut fb, &vic_prev.view(), &vic_curr.view(), t);
                } else {
                    render_vic(&mut fb, &VicView::from_game(shown));
                }
            }
            GameScreen::InGame | GameScreen::StageClear => {
                let stage_idx = (game.stage.clamp(1, 33) - 1) as usize;
                let bg0 = stage_palettes()[stage_idx * 4];
                let mc1 = stage_palettes()[stage_idx * 4 + 1];
                let mc2 = stage_palettes()[stage_idx * 4 + 2];
                let is_underground = stage_palettes()[stage_idx * 4 + 3] != 0;

                // Clear border and background to stage sky color / black
                fb.clear(c64_color(if is_underground { 0 } else { bg0 }));

                // Render current Stage tiles, HUD and Parallax Clouds
                render_stage(
                    &mut fb,
                    &game.current_map,
                    game.scroll_x,
                    game.cloud_scroll_x,
                    game.score,
                    game.diamonds,
                    game.time,
                    game.stage,
                    bg0,
                    mc1,
                    mc2,
                    is_underground,
                );

                // Base playfield Y offset (Row 0 of map starts after the 5 HUD/sky rows: 40 + 40 = 80)
                let playfield_y = (BORDER_Y + 5 * 8) as isize;

                // Render active enemies
                for enemy in &game.enemies {
                    if enemy.active {
                        let ex = BORDER_X as isize + (enemy.x as i32 - game.scroll_x as i32) as isize;
                        let ey = playfield_y + enemy.y as isize;
                        let (main_col, mc0, mc1) = match enemy.enemy_type {
                            giana_core::entities::EnemyType::Monster => (1, 10, 2),    // Authentic Stage 1 monster (White, Skin, Red)
                            giana_core::entities::EnemyType::RollingEye => (7, 10, 2),  // Yellow rolling eye
                            giana_core::entities::EnemyType::Crab => (7, 10, 2),       // Yellow crab
                            giana_core::entities::EnemyType::Piranha => (5, 10, 2),    // Green plant
                            giana_core::entities::EnemyType::Dragon => (3, 7, 0),      // Cyan dragon boss
                        };
                        render_sprite_multicolor(&mut fb, enemy.sprite_pointer, ex, ey, main_col, mc0, mc1);
                    }
                }

                // Render active projectiles (Giana's Dream Bubbles / Fireballs)
                for proj in &game.projectiles {
                    if proj.active {
                        let px = BORDER_X as isize + (proj.x as i32 - game.scroll_x as i32) as isize;
                        let py = playfield_y + proj.y as isize;
                        render_sprite_multicolor(&mut fb, proj.sprite_pointer, px, py, 1, 10, 2);
                    }
                }

                // Render popping items / diamonds
                for item in &game.items {
                    if item.active {
                        let ix = BORDER_X as isize + (item.x as i32 - game.scroll_x as i32) as isize;
                        let iy = playfield_y + item.y as isize;
                        render_sprite_multicolor(&mut fb, 0xF7, ix, iy, 2, 1, 7);
                    }
                }

                // Render debris particles
                for p in &game.particles {
                    if p.active {
                        let px = BORDER_X as isize + (p.x as i32 - game.scroll_x as i32) as isize;
                        let py = playfield_y + p.y as isize;
                        if px >= 40 && px + 3 < 344 && py >= 40 && py + 3 < 240 {
                            for dy in 0..3 {
                                for dx in 0..3 {
                                    fb.set_pixel((px + dx) as usize, (py + dy) as usize, c64_color(8));
                                }
                            }
                        }
                    }
                }

                // Render Giana multi-part sprite (with invulnerability blinking)
                let should_draw_giana = game.invulnerable_timer == 0 || (game.invulnerable_timer % 4 < 2);
                if should_draw_giana {
                    let sprites = game.get_giana_sprites();
                    let gx = BORDER_X as isize + (game.player_x - game.scroll_x as i32) as isize;
                    let gy = playfield_y + game.player_y as isize;

                    if game.has_punker {
                        // Punker: Sprite 0 = 13 (light green hair), Sprite 1 = 4 (purple dress), Sprite 2 = 1 (white HIRES)
                        render_sprite_multicolor(&mut fb, sprites[0], gx, gy, 13, 10, 2);
                        render_sprite_multicolor(&mut fb, sprites[1], gx, gy, 4, 10, 2);
                        render_sprite_hires(&mut fb, sprites[2], gx, gy, 1);
                    } else {
                        // Authentic C64 Giana:
                        // MC0 = 10 (skin), MC1 = 2 (red ribbon/collar)
                        // Sprite 0 = 7 (yellow hair)
                        // Sprite 1 = 6 (blue dress)
                        // Sprite 2 = 1 (white eye/shoes HIRES)
                        render_sprite_multicolor(&mut fb, sprites[0], gx, gy, 7, 10, 2);
                        render_sprite_multicolor(&mut fb, sprites[1], gx, gy, 6, 10, 2);
                        render_sprite_hires(&mut fb, sprites[2], gx, gy, 1);
                    }
                }

                // Draw outer borders in black (38-column C64 PAL clipping: X=40..343, Y=40..239)
                for y in 0..HEIGHT {
                    for x in 0..WIDTH {
                        if x < 40 || x > 343 || y < 40 || y >= 240 {
                            fb.set_pixel(x, y, c64_color(0)); // C64 black border
                        }
                    }
                }
            }
            GameScreen::GameOver | GameScreen::GameWon => {
                fb.clear(c64_color(0));
            }
        }

        // Where the picture goes in the window. While a game is shown the scaling modes can cut the empty border off (see
        // `present`); the messages below that stand in the border then move up into the last rows of the game picture.
        // A pause menu (and the pages behind it) stands over the running game and keeps its view, so the picture does not
        // jump when it opens. The main menu and the pages after a game have no game behind them: the whole frame.
        let playing = game.screen == GameScreen::InGame && !game.legacy_model && open_menu.as_ref().map_or(true, |m| m.paused);
        let window_size = canvas.output_size().unwrap_or((WIDTH as u32, HEIGHT as u32));
        let layout = present::layout(settings.scaling, playing, window_size);
        let cropped = layout.src == present::PLAY;
        let view = ui::View::new(cropped);
        // Two lanes for the small messages: in the bottom border, or over the last rows of the picture when it is cut
        let (info_y, hint_y) = if cropped { (224, 208) } else { (240, 256) };
        let toast_on = toast.map_or(false, |(_, since)| since.elapsed() < Duration::from_millis(1500));
        if let (Some(s), None) = (&session, &open_menu) {
            // who plays on this computer, and how the other one is doing; a toast takes the place for a moment
            let name = player_name(s.me);
            let text = match &other_game {
                _ if s.peer_paused() => "OTHER PLAYER PAUSED".to_string(),
                Some(o) if game.game_over => format!("YOU ARE OUT - WATCHING {}", player_name(o.player)),
                Some(o) => {
                    let lives = if o.lives_c64 & 0x80 != 0 { 0 } else { o.lives_c64 }; // $FF: the last life is gone
                    format!("{} {:06} LIVES {} STAGE {}", player_name(o.player), score_value(o.score_bcd), lives, o.stage)
                }
                None if s.my_turn(&game) => format!("YOU ARE {name} - YOUR TURN"),
                None => format!("YOU ARE {name} - WATCHING"),
            };
            if !toast_on {
                ui::pill_in(&mut fb, view, info_y, &text, if s.peer_paused() { PillTone::Notice } else { PillTone::Info });
            }
        }
        if let (Some((text, _)), None, true) = (toast, &open_menu, toast_on) {
            ui::pill_in(&mut fb, view, info_y, text, PillTone::Info);
        }
        if open_menu.is_none() && !game.game_over && game.waiting_for_confirm() {
            // under the banner text of the original, which stands a little left of the middle
            match ui::content_center_x(&fb, 36, info_y.min(hint_y) as usize) {
                Some(cx) => ui::pill_at_in(&mut fb, view, cx, hint_y, "PRESS JUMP OR FIRE", PillTone::Info),
                None => ui::pill_in(&mut fb, view, hint_y, "PRESS JUMP OR FIRE", PillTone::Info),
            }
        }
        if let Some(m) = open_menu.as_mut() {
            m.settings = settings.clone();
        }
        if let Some(m) = &open_menu {
            let pad_name = active_controller.as_ref().map(|c| c.name());
            let nudge = (started.elapsed().as_millis() / 400) as usize & 1;
            let ctx = DrawCtx { bindings: &bindings, pad_name: pad_name.as_deref(), scores: &scores, input: input_state, nudge, view };
            menu_draw::draw(&mut fb, m, &ctx);
        }

        presenter.upload(&fb)?;
        presenter.crt = settings.crt;
        presenter.present(&mut canvas, layout)?;
        canvas.present();

        // If vsync is not present or returned immediately and no logic tick is pending, sleep briefly so CPU does not spin at 100%
        let loop_elapsed = frame_start.elapsed();
        if loop_elapsed < Duration::from_millis(1) && accumulator + slack < logic_step {
            let sleep_needed = logic_step.saturating_sub(accumulator).min(Duration::from_millis(1));
            std::thread::sleep(sleep_needed);
        }
    }

    // The window was closed, or the game quit: a game in progress still counts, and the other player is told
    if let Some(s) = session.as_mut() {
        s.leave();
    }
    let mut results = Vec::new();
    if game.screen == GameScreen::InGame {
        results = game_results(&game);
        if let Some(s) = &session {
            // Each computer keeps its own player's score
            results.retain(|r| r.0 == player_name(s.me));
        }
    }
    // Scores that were waiting for a name get the name of the player
    results.extend(open_menu.iter().flat_map(|m| m.unnamed.iter().cloned()));
    if !results.is_empty() {
        for (name, score, stage) in results {
            scores.insert(Entry { name, score, stage });
        }
        save_scores(&scores, &module_id, &mut None);
    }

    Ok(())
}
