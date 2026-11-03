// Release builds on Windows open no console window next to the game (debug builds keep it for the log)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use giana_app::bindings::{config_path, Bindings, Held, PadInput, Source};
use giana_app::cli::{self, Command, Options};
use giana_app::display;
use giana_app::firstrun;
use giana_app::hints::InputState;
use giana_app::menu::{self, nav_for_button, nav_for_key, nav_for_stick, GameMode, Menu, Nav, OptionChange, Outcome, Search};
use giana_app::menu_draw::{self, DrawCtx};
use giana_app::menu_sound::{cue_for, Cue, MenuSounds};
use giana_app::padicons::{sdl_type_for_index, PadKind};
use giana_app::net::{self, Finder, Host, Joiner, NetError, NetMode, Session};
use giana_app::present::{self, Presenter};
use giana_app::ui::{self, PillTone};
use giana_app::result::{decide, MatchResult, Standing, Winner};
use giana_app::scores::{score_value, scores_path, Entry, Scores};
use giana_app::settings::{settings_path, Settings};
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
use sdl2::render::Canvas;
use sdl2::video::{FullscreenType, Window};
use std::time::{Duration, Instant};

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
fn save_scores(scores: &Scores, open: &mut Option<Menu>) {
    if let Some(path) = scores_path() {
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

fn main() {
    let command = cli::parse(std::env::args().skip(1)).unwrap_or_else(|message| {
        eprintln!("giana-app: {message}\nTry 'giana-app --help'.");
        std::process::exit(2);
    });
    match command {
        Command::Help => println!("{}", cli::HELP),
        Command::Version => println!("{}", cli::version_text()),
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
            if let Err(e) = run(options) {
                eprintln!("giana-app: {e}");
                std::process::exit(1);
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
) -> Result<bool, Box<dyn std::error::Error>>
where
    Canvas<Window>: Sized,
    C: 'static,
    Window: sdl2::render::RenderTarget<Context = C>,
{
    let mut setup = firstrun::Setup::new(dest);
    let text = video.text_input();
    text.start();
    let clipboard = video.clipboard();
    let started = Instant::now();
    let mut fb = Framebuffer::new();
    let mut tick = 0u32;
    let done = 'page: loop {
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
                        if setup.activate() == firstrun::Action::Quit {
                            break 'page false;
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
        if setup.poll() {
            break true;
        }
        firstrun::draw(&mut fb, &setup, tick, (started.elapsed().as_millis() / 400) as usize & 1);
        tick = tick.wrapping_add(1);
        let size = canvas.output_size().unwrap_or((WIDTH as u32, HEIGHT as u32));
        presenter.upload(&fb)?;
        presenter.present(canvas, present::layout(settings.scaling, false, size))?;
        canvas.present();
        std::thread::sleep(Duration::from_millis(8));
    };
    text.stop();
    Ok(done)
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
            if !first_run_page(&mut canvas, &mut presenter, &mut event_pump, &video_subsystem, &settings, dest.clone())? {
                return Ok(());
            }
            dest
        }
    };
    firstrun::load(&data_dir).map_err(|e| startup_error("Could not load the game data", e, "Delete the data folder and start the game again to extract it anew."))?;
    println!("Game data: {}", data_dir.display());

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
    let mut pending = Pending::default();
    let mut scores = scores_path().map_or_else(Scores::default, |p| Scores::load(&p));
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
            save_scores(&scores, &mut open_menu);
        }
        for change in std::mem::take(&mut pending.options) {
            match change {
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
            joining = Some(Joiner::start(&addr));
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
                    if let (NetError::WrongVersion, Some(m)) = (&e, open_menu.as_mut()) {
                        m.notice = Some("ANOTHER VERSION TRIED TO JOIN".to_string());
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
        save_scores(&scores, &mut None);
    }

    Ok(())
}
