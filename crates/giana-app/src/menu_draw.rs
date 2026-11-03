//! Drawing of every menu page in the clean panel style (see `ui`): the same panel, the same row
//! grid, the same colours and a footer of hints on every page.

use crate::bindings::{key_label, Action, Bindings, PadInput, SLOTS};
use crate::hints::{fit_in, InputDevice, InputState};
use crate::menu::{clip, safe_text, Device, Menu, Nav, Page, Search, BACK_ROW, ENHANCED_ITEMS, NETWORK_ITEMS, OPTION_ITEMS, RESET_ROW, SOUND_ITEMS};
use crate::menu::{AuthenticAvail, DetailItem, EnhancedItem, OptionItem, SoundItem, CODE_NOTICE};
use crate::padicons::Icon;
use crate::result::Winner;
use crate::scores::{Scores, MAX_ENTRIES, NAME_LEN};
use crate::settings::{MenuSoundMode, Presentation, Scaling, MAX_VOLUME};
use crate::ui::{self, Hint, ItemState, Panel, PillTone, Pitch, View};
use giana_render::framebuffer::{Framebuffer, WIDTH};
use giana_render::palette::c64_color;
use giana_render::text::{draw_text_px, fill_rect_px};

/// What the pages need besides the menu itself.
pub struct DrawCtx<'a> {
    pub bindings: &'a Bindings,
    /// name of the connected controller
    pub pad_name: Option<&'a str>,
    pub scores: &'a Scores,
    /// the last used device and the glyph set: decides which hints are shown
    pub input: InputState,
    /// 0 or 1, a blink phase that moves the markers by a pixel
    pub nudge: usize,
    /// the part of the frame the window shows: a page over a running game keeps the (cut) view of the game
    pub view: View,
}

/// Width of the panels of the pages that are never shown over a running game (network, result).
const WIDE: usize = 38;
/// Columns of the panels that carry notes and value rows (narrower when the picture is cut down).
fn wide(ctx: &DrawCtx) -> usize {
    ctx.view.wide()
}
/// Width of a value column.
const VALUE_W: usize = 14;

fn state(i: usize, sel: usize) -> ItemState {
    if i == sel {
        ItemState::Selected
    } else {
        ItemState::Normal
    }
}

fn px(i: u8) -> u32 {
    c64_color(i)
}

/// Draw a page over what is in the frame buffer. The main menu draws the title picture itself;
/// the pause menu and its question expect the game in the frame buffer and darken it; everything
/// else is on black.
pub fn draw(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    match (menu.page, menu.paused) {
        (Page::Root, false) => fb.draw_title_screen(),
        (Page::Root, true) | (Page::ConfirmQuit, _) => ui::dim(fb),
        _ => ui::backdrop_black(fb),
    }
    draw_foreground(fb, menu, ctx);
}

/// A page over a picture that stays visible (dimmed), whatever the page: the settings overlay of the AUTHENTIC presentation.
pub fn draw_over(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    ui::dim(fb);
    draw_foreground(fb, menu, ctx);
}

/// The page itself and the footer, on whatever the frame buffer holds.
pub(crate) fn draw_foreground(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let mut strip = false;
    match menu.page {
        Page::Root => {
            draw_root(fb, menu, ctx);
            strip = true;
        }
        Page::Controls(device) => draw_controls(fb, menu, ctx, device),
        Page::Network => draw_network(fb, menu, ctx),
        Page::Hosting => draw_hosting(fb, menu, ctx),
        Page::Join => draw_join(fb, menu, ctx),
        Page::Scores => draw_scores(fb, menu, ctx),
        Page::EnterName => draw_name(fb, menu, ctx),
        Page::Options => draw_options(fb, menu, ctx),
        Page::Enhanced => draw_enhanced(fb, menu, ctx),
        Page::Sound => draw_sound(fb, menu, ctx),
        Page::ConfirmQuit => draw_confirm(fb, menu, ctx),
        Page::Modules => draw_modules(fb, menu, ctx),
        Page::ModuleDetail => draw_module_detail(fb, menu, ctx),
        Page::ConfirmRemove => draw_confirm_remove(fb, menu, ctx),
        Page::Result => draw_result(fb, menu, ctx),
    }
    let note = (menu.page == Page::Root && !menu.paused).then(|| format!("V{}", env!("CARGO_PKG_VERSION")));
    let view = ctx.view;
    let notice = menu.notice.as_ref().map(|n| clip(&safe_text(n), if view.is_crop() { (view.w - 16) / 8 - 2 } else { 40 })).or_else(|| page_warns_about_sound(menu).then(|| "NO SOUND DEVICE FOUND".to_string()));
    // in the cut-down picture a notice takes the place of the hints in the footer bar
    let hints = if notice.is_some() && view.is_crop() { Vec::new() } else { footer_hints(menu, ctx) };
    ui::footer_in(fb, view, &hints, note.as_deref());
    // a notice takes the place of the controls strip
    let key = if ctx.input.device == InputDevice::Pad { "X" } else { "U" };
    let update_line = (menu.page == Page::Root && !menu.paused).then(|| menu.update.line(key)).flatten();
    if let Some(n) = &notice {
        ui::pill_in(fb, view, view.notice_y(), n, PillTone::Notice);
    } else if let Some(line) = update_line {
        ui::pill_in(fb, view, view.notice_y(), &clip(&line, 46), PillTone::Notice);
    } else if strip {
        ui::strip_in(fb, view, &ctx.input.play_hints_in(ctx.bindings, view));
    }
}

fn page_warns_about_sound(menu: &Menu) -> bool {
    !menu.audio_ok && matches!(menu.page, Page::Options | Page::Sound)
}

/// The hints of the footer for the page that is shown.
pub fn footer_hints(menu: &Menu, ctx: &DrawCtx) -> Vec<Hint> {
    let i = &ctx.input;
    let mut h = match menu.page {
        Page::Root if menu.paused => vec![i.move_hint("MOVE"), i.select_hint("SELECT"), i.back_hint("RESUME")],
        Page::Root => vec![i.move_hint("MOVE"), i.select_hint("SELECT")],
        Page::Controls(_) if menu.capturing => vec![i.cancel_hint("CANCEL")],
        Page::Controls(_) => vec![i.change_hint("SLOT"), i.select_hint("BIND"), i.clear_hint("CLEAR"), i.back_hint("BACK")],
        Page::Network => vec![i.move_hint("MOVE"), i.select_hint("SELECT"), i.back_hint("BACK")],
        Page::Hosting => vec![i.change_hint("WINNER"), i.back_hint("CANCEL")],
        Page::Join if menu.connecting => vec![i.back_hint("CANCEL")],
        Page::Join if i.device == InputDevice::Keyboard => vec![i.move_hint("PICK"), Hint::new("0-9", "TYPE"), i.select_hint("GO"), i.back_hint("BACK")],
        Page::Join => vec![i.move_hint("PICK"), i.select_hint("JOIN"), i.back_hint("BACK")],
        Page::Scores => vec![i.back_hint("BACK")],
        Page::EnterName if i.device == InputDevice::Keyboard => vec![Hint::new("A-Z", "TYPE"), Hint::new(key_label(sdl2::keyboard::Keycode::Backspace), "DELETE"), i.select_hint("SAVE")],
        Page::EnterName => vec![i.move_hint("LETTER"), i.dir_hint(Nav::Right, "NEXT"), i.dir_hint(Nav::Left, "DELETE"), i.select_hint("SAVE")],
        Page::Options | Page::Enhanced | Page::Sound => vec![i.move_hint("MOVE"), i.change_hint("CHANGE"), i.back_hint("BACK")],
        Page::ConfirmQuit | Page::ConfirmRemove => vec![i.move_hint("MOVE"), i.select_hint("SELECT"), i.back_hint("NO")],
        Page::Modules | Page::ModuleDetail => vec![i.move_hint("MOVE"), i.select_hint("SELECT"), i.back_hint("BACK")],
        Page::Result if menu.result_hold == 0 => vec![i.select_hint("CONTINUE")],
        Page::Result => Vec::new(),
    };
    // the options page has a confirm key that only opens a page from SOUND and MODE
    if menu.page == Page::Options && matches!(OPTION_ITEMS[menu.cursor.min(OPTION_ITEMS.len() - 1)], OptionItem::Mode | OptionItem::Sound) {
        h.insert(2, i.select_hint("OPEN"));
    }
    fit_in(h, ctx.view)
}

// ---------- main and pause menu ----------

fn draw_root(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let items = menu.root_items();
    let title = if menu.paused { "PAUSED" } else { "MAIN MENU" };
    let p = Panel::for_list(items.len(), 20, Pitch::Roomy, true, 0);
    // the main menu sits high so that the strip of controls fits below it
    let p = if menu.paused { p.in_view(ctx.view) } else { p.at_row(1) };
    p.draw(fb, Some(title));
    for (n, item) in items.iter().enumerate() {
        p.item(fb, p.item_y(n), item.label(), state(n, menu.cursor), ctx.nudge);
    }
    // the main menu says which ROM module is played: a dim line in the empty border above the picture
    if !menu.paused {
        if let Some(m) = menu.modules.iter().find(|m| !menu.active_module.is_empty() && m.id == menu.active_module) {
            let text = clip(&format!("ROM MODULE: {}", m.name), 40);
            let x = (WIDTH as isize - 8 * text.chars().count() as isize) / 2;
            draw_text_px(fb, x, 14, &text, px(ui::color::DIM));
        }
    }
}

// ---------- confirmation ----------

fn draw_confirm(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let p = Panel::for_list(4, 30, Pitch::Roomy, false, 0).in_view(ctx.view);
    p.draw(fb, None);
    p.message(fb, 2, "QUIT GAME?");
    for (n, label) in ["YES", "NO"].iter().enumerate() {
        p.item_centered(fb, p.item_y(1 + n), label, state(n, menu.cursor), ctx.nudge);
    }
    let note = if menu.in_network { "THE OTHER PLAYER IS TOLD" } else { "THE GAME IS LOST" };
    p.message_colored(fb, 8, note, ui::color::DIM);
}

// ---------- ROM modules ----------

/// Entries of the modules page that are shown at once; the list moves along with the cursor.
const MODULE_ROWS: usize = 8;

fn draw_modules(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let total = menu.module_rows();
    let rows = total.min(MODULE_ROWS);
    let p = Panel::for_list(rows, WIDE, Pitch::Tight, true, 5);
    p.draw(fb, Some("ROM MODULES"));
    let first = (menu.cursor + 1).saturating_sub(MODULE_ROWS).min(total - rows);
    let last_module = menu.modules.len();
    for (n, i) in (first..first + rows).enumerate() {
        let y = p.item_y(n);
        let st = state(i, menu.cursor);
        let (label, active) = match i {
            0 => ("ORIGINAL".to_string(), menu.active_module.is_empty()),
            i if i <= last_module => (menu.modules[i - 1].list_label(p.max_label_len() - 2), menu.modules[i - 1].id == menu.active_module),
            i if i == last_module + 1 => ("ADD ROM MODULE...".to_string(), false),
            _ => ("BACK".to_string(), false),
        };
        let fg = p.row_colors(fb, y, st, ctx.nudge);
        draw_text_px(fb, p.col_x(Panel::LABEL_COL), y, &label, px(fg));
        if active {
            // the module that is played: a star between the marker and the name
            draw_text_px(fb, p.col_x(Panel::LABEL_COL - 1), y, "*", px(if st == ItemState::Selected { fg } else { ui::color::YELLOW }));
        }
    }
    let after = Panel::head_rows(true) + Panel::list_rows(rows, Pitch::Tight);
    p.separator(fb, after + 1);
    match &menu.module_error {
        Some(e) => {
            for (n, line) in crate::firstrun::wrap(&safe_text(&e.to_ascii_uppercase()), WIDE - 4).iter().take(3).enumerate() {
                p.note_colored(fb, after + 2 + n, line, ui::color::YELLOW);
            }
        }
        None => {
            p.note(fb, after + 2, "* IS THE MODULE THAT IS PLAYED");
            p.note(fb, after + 4, "DROP A .GROM FILE TO ADD IT");
        }
    }
}

fn draw_module_detail(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let (name, byline, text, changes) = match menu.selected_module() {
        Some(m) => {
            let mut by = String::new();
            if !m.author.is_empty() {
                by = format!("BY {}", m.author);
            }
            if !m.version.is_empty() {
                by = format!("{by}{}V{}", if by.is_empty() { "" } else { "  " }, m.version);
            }
            (m.name.clone(), by, if m.description.is_empty() { "NO DESCRIPTION".to_string() } else { m.description.clone() }, m.changes_code)
        }
        None => ("ORIGINAL".to_string(), String::new(), "THE GAME AS IT WAS ON THE C64".to_string(), false),
    };
    let lines: Vec<String> = crate::firstrun::wrap(&text, WIDE - 4).into_iter().take(6).collect();
    let items = menu.detail_items();
    // rows: head (3), name, blank, by line, blank, description, blank + notice, blank, separator, items
    let notice_rows = if changes { 1 + CODE_NOTICE.len() } else { 0 };
    let items_row = 3 + 1 + 1 + 1 + 1 + lines.len() + notice_rows + 1 + 1;
    let item_rows = ((items.len() - 1) * 16 + 8).div_ceil(8);
    let p = Panel::centered(WIDE, items_row + item_rows + 2, true);
    p.draw(fb, Some("ROM MODULE"));
    p.message_colored(fb, 3, &clip(&name, WIDE - 4), ui::color::YELLOW);
    if !byline.is_empty() {
        p.message_colored(fb, 5, &clip(&byline, WIDE - 4), ui::color::DIM);
    }
    for (n, line) in lines.iter().enumerate() {
        p.text_at(fb, 2, 7 + n, line, ui::color::TEXT);
    }
    if changes {
        for (n, line) in CODE_NOTICE.iter().enumerate() {
            p.message_colored(fb, 7 + lines.len() + 1 + n, line, ui::color::YELLOW);
        }
    }
    p.separator(fb, items_row - 1);
    for (n, item) in items.iter().enumerate() {
        let y = p.row_y(items_row) + (n * 16) as isize;
        let label = if *item == DetailItem::Play && menu.selected_is_active() { "PLAYED NOW" } else { item.label() };
        p.item(fb, y, label, state(n, menu.cursor), ctx.nudge);
    }
}

fn draw_confirm_remove(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let p = Panel::for_list(4, 30, Pitch::Roomy, false, 0).in_view(ctx.view);
    p.draw(fb, None);
    p.message(fb, 2, "REMOVE THIS MODULE?");
    for (n, label) in ["YES", "NO"].iter().enumerate() {
        p.item_centered(fb, p.item_y(1 + n), label, state(n, menu.cursor), ctx.nudge);
    }
    let name = menu.selected_module().map_or(String::new(), |m| clip(&m.name, 26));
    p.message_colored(fb, 8, &name, ui::color::DIM);
}

// ---------- network ----------

fn draw_network(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let p = Panel::for_list(NETWORK_ITEMS.len(), WIDE, Pitch::Roomy, true, 5);
    p.draw(fb, Some("NETWORK GAME"));
    for (n, label) in NETWORK_ITEMS.iter().enumerate() {
        p.item(fb, p.item_y(n), label, state(n, menu.cursor), ctx.nudge);
    }
    let after = Panel::head_rows(true) + Panel::list_rows(NETWORK_ITEMS.len(), Pitch::Roomy);
    p.separator(fb, after + 1);
    let hint = match menu.cursor {
        0 => "BOTH PLAY - WHO IS OUT WATCHES",
        1 => "THE TURN CHANGES EVERY LOST LIFE",
        2 => "THE OTHER COMPUTER HOSTS",
        _ => "",
    };
    p.note(fb, after + 2, hint);
    p.note(fb, after + 4, "HOST IS GIANA, THE OTHER IS MARIA");
}

fn draw_hosting(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let p = Panel::centered(WIDE, 19, true);
    p.draw(fb, Some("HOSTING"));
    p.message_colored(fb, 3, "WAITING FOR THE OTHER PLAYER", ui::color::TEXT);
    p.message_colored(fb, 5, "YOUR ADDRESS", ui::color::DIM);
    let ip = clip(&safe_text(&menu.my_ip), 21);
    let fw = ip.chars().count().max(15);
    p.field(fb, p.row_y(7), (WIDE - fw) / 2, fw, &ip, false);
    p.message_colored(fb, 9, &format!("TCP+UDP {} MUST BE OPEN", menu.host_port), ui::color::TEXT);
    p.message_colored(fb, 11, "OTHER COMPUTER: NETWORK, JOIN GAME", ui::color::DIM);
    if !menu.host_findable {
        p.message_colored(fb, 13, "NO UDP SEARCH - TYPE THE ADDRESS", ui::color::YELLOW);
    }
    p.separator(fb, 14);
    let y = p.row_y(16);
    p.value_row(fb, y, "WINNER", menu.settings.winner.label(), 8, ItemState::Selected, ctx.nudge);
}

fn draw_join(fb: &mut Framebuffer, menu: &Menu, _ctx: &DrawCtx) {
    if menu.connecting {
        let p = Panel::centered(WIDE, 10, true);
        p.draw(fb, Some("JOIN GAME"));
        p.message_colored(fb, 4, "CONNECTING...", ui::color::YELLOW);
        p.message_colored(fb, 6, &clip(&connect_target(menu), 30), ui::color::TEXT);
        return;
    }
    let p = Panel::centered(WIDE, 18, true);
    p.draw(fb, Some("JOIN GAME"));
    let entries = menu.join_entries();
    if menu.hosts.is_empty() {
        let (text, color) = match menu.search {
            Search::Looking => ("SEARCHING THE NETWORK", ui::color::DIM),
            Search::NothingFound => ("NOTHING FOUND - TYPE THE ADDRESS", ui::color::YELLOW),
            Search::Unavailable => ("SEARCH FAILED - TYPE THE ADDRESS", ui::color::YELLOW),
        };
        p.message_colored(fb, 3, text, color);
    } else {
        p.message_colored(fb, 3, "GAMES FOUND", ui::color::DIM);
    }
    // three entries fit; the list moves along with the cursor
    let first = menu.cursor.saturating_sub(2).min(entries.len().saturating_sub(3));
    for (i, (addr, last)) in entries.iter().enumerate().skip(first).take(3) {
        let selected = i == menu.cursor && menu.address.is_empty();
        let label = if *last { format!("{} (LAST)", clip(addr, 21)) } else { clip(addr, 29) };
        p.item(fb, p.row_y(5) + ((i - first) * 16) as isize, &label, if selected { ItemState::Selected } else { ItemState::Normal }, _ctx.nudge);
    }
    p.separator(fb, 11);
    p.message_colored(fb, 13, "OR TYPE THE ADDRESS", ui::color::DIM);
    let fw = 22;
    p.field(fb, p.row_y(15), (WIDE - fw) / 2, fw, &menu.address, true);
}

/// What the join page connects to, for the message while it waits.
fn connect_target(menu: &Menu) -> String {
    if !menu.address.is_empty() {
        return menu.address.clone();
    }
    menu.join_entries().get(menu.cursor).map(|e| e.0.clone()).unwrap_or_default()
}

// ---------- scores, name, result ----------

fn score_line(rank: &str, name: &str, score: &str, stage: &str) -> String {
    format!("{rank:>2}  {name:<8} {score:>6}  {stage:>2}")
}

fn draw_scores(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let p = Panel::for_list(MAX_ENTRIES, 30, Pitch::Tight, true, 2).in_view(ctx.view);
    p.draw(fb, Some("HIGH SCORES"));
    // the table is centred in the panel to the pixel
    let head = score_line("#", "NAME", "SCORE", "ST");
    draw_text_px(fb, p.centered_x(head.chars().count()), p.row_y(3), &head, px(ui::color::DIM));
    p.separator(fb, 4);
    let entries = &ctx.scores.entries;
    if entries.is_empty() {
        p.message_colored(fb, 11, "NO SCORES YET", ui::color::TEXT);
    }
    for (n, e) in entries.iter().enumerate().take(MAX_ENTRIES) {
        let y = p.row_y(5) + (n * Pitch::Tight.px()) as isize;
        let line = score_line(&n.saturating_add(1).to_string(), &clip(&safe_text(&e.name), NAME_LEN), &e.score.min(999_999).to_string(), &e.stage.min(99).to_string());
        p.item_centered(fb, y, &line, if menu.latest == Some(n) { ItemState::Selected } else { ItemState::Normal }, ctx.nudge);
    }
}

fn draw_name(fb: &mut Framebuffer, menu: &Menu, _ctx: &DrawCtx) {
    let (who, score, _) = menu.unnamed.first().cloned().unwrap_or_default();
    let p = Panel::centered(30, 13, true);
    p.draw(fb, Some("NEW HIGH SCORE"));
    p.message_colored(fb, 3, &who, ui::color::YELLOW);
    p.message_colored(fb, 5, &format!("{score:06} POINTS"), ui::color::TEXT);
    p.message_colored(fb, 7, "YOUR NAME", ui::color::DIM);
    let fw = NAME_LEN + 1;
    p.field(fb, p.row_y(9), (30 - fw) / 2, fw, &menu.name, true);
}

fn draw_result(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let Some(r) = &menu.result else { return };
    let p = Panel::centered(WIDE, 16, true);
    p.draw(fb, Some("RESULT"));
    // columns: name from 5, score ends at 25, stage ends at 33 (5 columns free on both sides)
    let head = |p: &Panel, fb: &mut Framebuffer, name: &str, score: &str, stage: &str, row: usize, color: u8| {
        p.text_at(fb, 5, row, name, color);
        p.text_right(fb, 25, row, score, color);
        p.text_right(fb, 33, row, stage, color);
    };
    head(&p, fb, "NAME", "SCORE", "STAGE", 3, ui::color::DIM);
    p.separator(fb, 4);
    for (n, s) in r.players.iter().enumerate() {
        let row = 6 + 2 * n;
        let y = p.row_y(row);
        let st = if r.winner == Some(n) { ItemState::Selected } else { ItemState::Normal };
        let fg = p.row_colors(fb, y, st, ctx.nudge);
        head(&p, fb, &clip(&s.name, NAME_LEN), &s.score.min(999_999).to_string(), &s.stage.min(99).to_string(), row, fg);
    }
    p.message_colored(fb, 11, &r.text, ui::color::YELLOW);
    p.message_colored(fb, 13, r.rule.rule_text(), ui::color::DIM);
}

// ---------- options ----------

fn on_off(on: bool) -> &'static str {
    if on {
        "ON"
    } else {
        "OFF"
    }
}

/// Rows below the list of the options page: a blank row and the note.
fn option_extra_rows(_view: View) -> usize {
    2
}

/// Rows below the list of the sound page: the separator and two lines of notes (as on the page of the mode).
const SOUND_EXTRA_ROWS: usize = 5;

fn draw_options(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let s = &menu.settings;
    let p = Panel::for_list(OPTION_ITEMS.len(), wide(ctx), Pitch::Tight, true, option_extra_rows(ctx.view)).in_view(ctx.view);
    p.draw(fb, Some("OPTIONS"));
    for (n, item) in OPTION_ITEMS.iter().enumerate() {
        let (y, st) = (p.item_y(n), state(n, menu.cursor));
        let label = item.label();
        match item {
            OptionItem::Fullscreen => p.value_row(fb, y, label, on_off(s.fullscreen), VALUE_W, st, ctx.nudge),
            OptionItem::Scaling => p.value_row(fb, y, label, s.scaling.label(), VALUE_W, st, ctx.nudge),
            OptionItem::Crt => p.value_row(fb, y, label, if s.crt { "CRT" } else { "SHARP" }, VALUE_W, st, ctx.nudge),
            OptionItem::Display50 => p.value_row(fb, y, label, if s.display_50 { "AUTO" } else { "OFF" }, VALUE_W, st, ctx.nudge),
            OptionItem::Sound => p.item(fb, y, label, st, ctx.nudge),
            OptionItem::Winner => p.value_row(fb, y, label, s.winner.label(), VALUE_W, st, ctx.nudge),
            OptionItem::UpdateCheck => p.value_row(fb, y, label, if s.update_check { "AUTO" } else { "OFF" }, VALUE_W, st, ctx.nudge),
            OptionItem::Mode => p.value_row(fb, y, label, if s.enhancements.enhanced { "ENHANCED" } else { "ORIGINAL" }, VALUE_W, st, ctx.nudge),
            OptionItem::Presentation => {
                // greyed out while AUTHENTIC cannot be started (it can always be left)
                let st = if menu.authentic != AuthenticAvail::Ready && s.presentation == Presentation::Modern && st != ItemState::Selected { ItemState::Disabled } else { st };
                p.value_row(fb, y, label, s.presentation.label(), VALUE_W, st, ctx.nudge)
            }
            OptionItem::Back => p.item(fb, y, label, st, ctx.nudge),
        }
    }
    let note = match OPTION_ITEMS[menu.cursor.min(OPTION_ITEMS.len() - 1)] {
        OptionItem::Fullscreen => "ALSO F11 OR ALT+ENTER",
        OptionItem::Scaling => match s.scaling {
            Scaling::Whole => "SHARP PIXELS - BLACK BARS",
            Scaling::NoBorder => "CUTS THE BORDER WHILE PLAYING",
            Scaling::Fill => "FULL HEIGHT - EDGES ARE SMOOTHED",
        },
        OptionItem::Crt => {
            if s.crt {
                "SCANLINES AND GLOW LIKE A TV"
            } else {
                "SHARP PIXELS"
            }
        }
        OptionItem::Display50 => {
            if s.display_50 {
                "50-200 HZ FULLSCREEN IF POSSIBLE"
            } else {
                "FULLSCREEN KEEPS THE DESKTOP MODE"
            }
        }
        OptionItem::Sound => "VOLUMES AND MENU SOUNDS",
        OptionItem::Winner => match s.winner {
            Winner::Score => "HIGHER SCORE WINS (TIE: STAGE)",
            Winner::Furthest => "FURTHEST STAGE WINS (TIE: SCORE)",
        },
        OptionItem::Mode => "ORIGINAL OR ENHANCED GAME",
        OptionItem::Presentation => match (s.presentation, menu.authentic) {
            (Presentation::Authentic, _) => "EXACTLY WHAT THE C64 SHOWED",
            (_, AuthenticAvail::Ready) => "THE ORIGINAL, AS ON THE C64",
            (_, AuthenticAvail::NeedsData) => "NEEDS DISK IMAGE AGAIN: CONFIRM",
            (_, AuthenticAvail::ModuleActive) => "NOT FOR ROM MODULES",
        },
        OptionItem::UpdateCheck => {
            if s.update_check {
                "ASKS GITHUB ONCE A DAY"
            } else {
                "NEVER ASKS THE INTERNET"
            }
        }
        OptionItem::Back => "",
    };
    p.note(fb, Panel::head_rows(true) + Panel::list_rows(OPTION_ITEMS.len(), Pitch::Tight) + option_extra_rows(ctx.view) - 1, note);
}

fn draw_sound(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let s = &menu.settings;
    let p = Panel::for_list(SOUND_ITEMS.len(), wide(ctx), Pitch::Roomy, true, SOUND_EXTRA_ROWS).in_view(ctx.view);
    p.draw(fb, Some("SOUND"));
    for (n, item) in SOUND_ITEMS.iter().enumerate() {
        let (y, st) = (p.item_y(n), state(n, menu.cursor));
        let label = item.label();
        match item {
            SoundItem::Volume if s.muted => p.value_row(fb, y, label, "MUTED", VALUE_W, st, ctx.nudge),
            SoundItem::Volume => p.volume_row_in(fb, y, label, s.volume as usize, MAX_VOLUME as usize, VALUE_W, st, ctx.nudge),
            SoundItem::Music => p.volume_row_in(fb, y, label, s.music_volume as usize, MAX_VOLUME as usize, VALUE_W, st, ctx.nudge),
            SoundItem::Effects => p.volume_row_in(fb, y, label, s.effects_volume as usize, MAX_VOLUME as usize, VALUE_W, st, ctx.nudge),
            SoundItem::MenuSounds => p.value_row(fb, y, label, s.menu_sounds.label(), VALUE_W, st, ctx.nudge),
            SoundItem::Back => p.item(fb, y, label, st, ctx.nudge),
        }
    }
    let after = Panel::head_rows(true) + Panel::list_rows(SOUND_ITEMS.len(), Pitch::Roomy);
    p.separator(fb, after + 1);
    let lines: [&str; 2] = match SOUND_ITEMS[menu.cursor.min(SOUND_ITEMS.len() - 1)] {
        SoundItem::Volume => ["M MUTES AND UNMUTES", ""],
        SoundItem::Music | SoundItem::Effects => {
            if s.split_sound() {
                ["MUSIC AND EFFECTS PLAY APART", ""]
            } else {
                ["BOTH FULL: THE ORIGINAL SOUND", ""]
            }
        }
        SoundItem::MenuSounds => match s.menu_sounds {
            MenuSoundMode::Off => ["NO SOUNDS IN THE MENUS", ""],
            MenuSoundMode::Soft => ["QUIET TICKS", ""],
            MenuSoundMode::Classic => ["EFFECTS OF THE GAME", ""],
        },
        SoundItem::Back => ["", ""],
    };
    for (n, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            p.note(fb, after + 2 + 2 * n, line);
        }
    }
}

fn draw_enhanced(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx) {
    let e = &menu.settings.enhancements;
    let p = Panel::for_list(ENHANCED_ITEMS.len(), wide(ctx), Pitch::Roomy, true, 9).in_view(ctx.view);
    p.draw(fb, Some("MODE"));
    for (n, item) in ENHANCED_ITEMS.iter().enumerate() {
        let y = p.item_y(n);
        let sel = n == menu.cursor;
        let label = item.label();
        match item {
            EnhancedItem::Mode => p.value_row(fb, y, label, if e.enhanced { "ENHANCED" } else { "ORIGINAL" }, VALUE_W, state(n, menu.cursor), ctx.nudge),
            EnhancedItem::SmoothMotion => {
                // the single switches do nothing in the original mode
                let st = if sel {
                    ItemState::Selected
                } else if !e.enhanced {
                    ItemState::Disabled
                } else {
                    ItemState::Normal
                };
                p.value_row(fb, y, label, on_off(e.smooth_motion), VALUE_W, st, ctx.nudge)
            }
            EnhancedItem::Back => p.item(fb, y, label, state(n, menu.cursor), ctx.nudge),
        }
    }
    let after = Panel::head_rows(true) + Panel::list_rows(ENHANCED_ITEMS.len(), Pitch::Roomy);
    p.separator(fb, after + 1);
    let lines: [&str; 3] = match ENHANCED_ITEMS[menu.cursor.min(ENHANCED_ITEMS.len() - 1)] {
        EnhancedItem::Mode => ["ORIGINAL IS THE GAME AS IT WAS", "ENHANCED TURNS ON THE SWITCHES", ""],
        EnhancedItem::SmoothMotion => ["ON 60, 120, 144 HZ SCREENS THE", "PICTURE IS SHOWN BETWEEN GAME", "STEPS. THE GAME IS NOT CHANGED"],
        EnhancedItem::Back => ["", "", ""],
    };
    for (n, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            p.note(fb, after + 2 + 2 * n, line);
        }
    }
    if !e.enhanced {
        p.message_colored(fb, after + 8, "SWITCHES NEED ENHANCED MODE", ui::color::YELLOW);
    }
}

// ---------- controls ----------

/// Width of one slot cell in pixels, and its first column in the panel.
const CELL_W: usize = 64;
/// The three slot cells start at column 9 and end as far from the right edge of the panel as the labels (column 4)
/// are from the left one: 3 x 64 pixels and two gaps of 4.
const SLOT_COL: usize = 9;
const SLOT_STEP: usize = CELL_W + 4;

enum Cell {
    Empty,
    Key(String),
    Pad(Icon),
}

fn draw_cell(p: &Panel, fb: &mut Framebuffer, slot: usize, y: isize, cell: &Cell, active: bool) {
    let x = p.col_x(SLOT_COL) + (slot * SLOT_STEP) as isize;
    if active {
        fill_rect_px(fb, x - 1, y - 2, CELL_W + 2, 12, px(ui::color::YELLOW));
    }
    fill_rect_px(fb, x, y - 1, CELL_W, 10, px(ui::color::BLACK));
    match cell {
        Cell::Empty => draw_text_px(fb, x + (CELL_W as isize - 8) / 2, y, "-", px(ui::color::DIM)),
        Cell::Key(t) => {
            let w = ui::cap_width(t.chars().count());
            ui::cap_text(fb, x + (CELL_W as isize - w as isize) / 2, y, t);
        }
        Cell::Pad(i) => i.draw(fb, x + (CELL_W as isize - i.width() as isize) / 2, y),
    }
}

fn draw_controls(fb: &mut Framebuffer, menu: &Menu, ctx: &DrawCtx, device: Device) {
    let p = Panel::centered(wide(ctx), 21, true).in_view(ctx.view);
    let title = if device == Device::Keyboard { "KEYBOARD" } else { "CONTROLLER" };
    p.draw(fb, Some(title));
    if device == Device::Pad {
        let text = ctx.pad_name.map_or("NO CONTROLLER CONNECTED".to_string(), |n| clip(&safe_text(&n.to_uppercase()), wide(ctx) - 4));
        p.message_colored(fb, 3, &text, ui::color::DIM);
    }
    // (no "ACTION" header: the first slot cell starts right after the longest action name)
    for s in 0..SLOTS {
        let x = p.col_x(SLOT_COL) + (s * SLOT_STEP + (CELL_W - 48) / 2) as isize;
        draw_text_px(fb, x, p.row_y(5), &format!("SLOT {}", s + 1), px(ui::color::DIM));
    }
    p.separator(fb, 6);
    let pitch = Pitch::Tight.px();
    let row_y = |n: usize| p.row_y(7) + (n * pitch) as isize;
    for (a, action) in Action::ALL.iter().enumerate() {
        let y = row_y(a);
        let selected = a == menu.cursor;
        // the bar only covers the name; the slot of the cursor gets a yellow frame
        p.row_colors_w(fb, y, if selected { ItemState::Selected } else { ItemState::Normal }, ctx.nudge, 8);
        let fg = if selected { ui::color::SELECT_FG } else { ui::color::TEXT };
        draw_text_px(fb, p.col_x(Panel::LABEL_COL), y, action.label(), px(fg));
        for slot in 0..SLOTS {
            let cell = match device {
                Device::Keyboard => bindings_key(ctx.bindings, a, slot).map_or(Cell::Empty, |k| Cell::Key(clip(&safe_text(&key_label(k)), 7))),
                Device::Pad => ctx.bindings.pad[a][slot].map_or(Cell::Empty, |input: PadInput| Cell::Pad(ctx.input.icon(input))),
            };
            draw_cell(&p, fb, slot, y, &cell, selected && slot == menu.slot);
        }
    }
    for (n, (cursor, label)) in [(RESET_ROW, "RESET TO DEFAULTS"), (BACK_ROW, "BACK")].into_iter().enumerate() {
        p.item(fb, row_y(Action::ALL.len() + n), label, state(cursor, menu.cursor), ctx.nudge);
    }
    if menu.capturing {
        draw_capture(fb, menu, device);
    }
}

fn bindings_key(b: &Bindings, action: usize, slot: usize) -> Option<sdl2::keyboard::Keycode> {
    b.keys[action][slot]
}

/// The small panel over the darkened page while a key or button is awaited.
fn draw_capture(fb: &mut Framebuffer, menu: &Menu, device: Device) {
    ui::dim(fb);
    let p = Panel::centered(28, 7, false);
    p.draw(fb, None);
    p.message_colored(fb, 2, if device == Device::Keyboard { "PRESS A KEY" } else { "PRESS A BUTTON" }, ui::color::YELLOW);
    let action = Action::ALL[menu.cursor.min(Action::ALL.len() - 1)].label();
    p.message_colored(fb, 4, &format!("FOR {action} - SLOT {}", menu.slot + 1), ui::color::DIM);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::{decide, Standing};
    use crate::scores::Entry;
    use giana_render::framebuffer::{HEIGHT, WIDTH};

    fn sample_result() -> crate::result::MatchResult {
        let p = |name: &str, score, stage| Standing { name: name.into(), score, stage };
        decide(Winner::Furthest, [p("GIANA", 999_999, 33), p("MARIA", 999_998, 33)])
    }

    fn full_scores() -> Scores {
        let mut full = Scores::default();
        for i in 0..10 {
            full.insert(Entry { name: "WWWWWWWW".into(), score: 999_999 - i, stage: 33 });
        }
        full
    }

    /// ROM modules with texts as long as they may be (the limits of `giana-module`), `n` of them.
    fn sample_modules(n: usize) -> Vec<crate::menu::ModuleInfo> {
        (0..n)
            .map(|i| {
                let long = i % 2 == 0;
                crate::menu::ModuleInfo::new(
                    &format!("{i:016x}"),
                    &if long { "W".repeat(24) } else { "Short".into() },
                    &if long { "W".repeat(24) } else { "me".into() },
                    &if long { "W".repeat(12) } else { "1.0".into() },
                    &if long { "WWWW WWWW ".repeat(20) } else { String::new() },
                    i % 3 == 0,
                )
            })
            .collect()
    }

    fn all_pages() -> Vec<Menu> {
        let mut out = Vec::new();
        for paused in [false, true] {
            for page in [
                Page::Root,
                Page::ConfirmQuit,
                Page::Result,
                Page::Controls(Device::Keyboard),
                Page::Controls(Device::Pad),
                Page::Network,
                Page::Hosting,
                Page::Join,
                Page::Scores,
                Page::EnterName,
                Page::Options,
                Page::Enhanced,
                Page::Sound,
                Page::Modules,
                Page::ModuleDetail,
                Page::ConfirmRemove,
            ] {
                for cursor in 0..10 {
                    for capturing in [false, true] {
                        let mut m = Menu::main();
                        m.paused = paused;
                        m.page = page;
                        m.cursor = cursor;
                        m.capturing = capturing;
                        m.slot = cursor % 3;
                        m.result = Some(sample_result());
                        m.my_ip = "192.168.178.100".into();
                        m.hosts = vec!["192.168.178.100".into(); 3];
                        m.settings.last_address = "10.0.0.1".into();
                        m.address = "192.168.178.100:47064".into();
                        m.unnamed = vec![("GIANA".into(), 999_999, 33)];
                        m.name = "WWWWWWWW".into();
                        m.latest = Some(3);
                        m.modules = sample_modules(cursor % 4 + 1 + cursor / 4 * 4);
                        m.active_module = m.modules[cursor % m.modules.len()].id.clone();
                        m.module_sel = if cursor % 5 == 4 { None } else { Some(cursor % m.modules.len()) };
                        m.module_error = (cursor % 2 == 1).then(|| "this module was made for a different version of the game data, and it is quite long".to_string());
                        out.push(m);
                    }
                }
            }
        }
        out
    }

    fn contexts() -> Vec<InputState> {
        use crate::padicons::PadKind::*;
        vec![InputState::keyboard(), InputState::pad(Xbox), InputState::pad(PlayStation), InputState::pad(Switch), InputState::pad(Generic)]
    }

    /// Every page in every state draws without a text leaving its panel (the drawing code asserts
    /// that in debug builds), and nothing but the footer and a notice is drawn below the grid.
    #[test]
    fn every_page_draws_and_fits() {
        giana_core::skip_without_data!();
        let mut b = Bindings::default();
        b.set_key(0, 2, sdl2::keyboard::Keycode::KpMultiply);
        b.set_key(4, 1, sdl2::keyboard::Keycode::RShift);
        let scores = full_scores();
        let mut fb = Framebuffer::new();
        for input in contexts() {
            let ctx = DrawCtx { bindings: &b, pad_name: Some("A controller with a really very long name indeed, yes"), scores: &scores, input, nudge: 1, view: View::FULL };
            for mut m in all_pages() {
                for notice in [None, Some("NOBODY IS HOSTING AT THAT ADDRESS".to_string())] {
                    m.notice = notice;
                    m.audio_ok = m.cursor % 2 == 0;
                    m.host_findable = m.cursor % 2 == 1;
                    m.search = [Search::Looking, Search::NothingFound, Search::Unavailable][m.cursor % 3];
                    m.connecting = m.cursor == 9;
                    draw(&mut fb, &m, &ctx);
                    let hints = footer_hints(&m, &ctx);
                    assert!(ui::hints_width(&hints) <= ui::FOOTER_MAX, "{:?} {hints:?}", m.page);
                }
            }
        }
    }

    /// Pages that can be open over a paused game.
    /// The main menu with nine items stays above the strip of controls, the other new panels inside the grid.
    #[test]
    fn the_new_panels_fit_the_screen() {
        let root = Panel::for_list(9, 20, Pitch::Roomy, true, 0).at_row(1);
        assert!(36 + (root.row + root.h) * 8 + ui::SHADOW <= ui::NOTICE_Y as usize, "the main menu reaches the strip of controls");
        let list = Panel::for_list(MODULE_ROWS, WIDE, Pitch::Tight, true, 5);
        assert!(list.h <= ui::ROWS && list.row >= 1);
        // the tallest detail page: six lines of description, the notice, three items
        let items_row = 3 + 1 + 1 + 1 + 6 + 3 + 1 + 1;
        let tall = Panel::centered(WIDE, items_row + (2 * 16 + 8) / 8 + 2, true);
        assert!(tall.h <= ui::ROWS, "{} rows", tall.h);
    }

    #[test]
    fn module_labels_are_cut_to_the_list() {
        let m = &sample_modules(1)[0];
        let label = m.list_label(Panel::for_list(8, WIDE, Pitch::Tight, true, 5).max_label_len() - 2);
        assert!(label.chars().count() <= 32 && label.contains("  VWWWW"), "{label}");
        let short = crate::menu::ModuleInfo::new("0", "Short", "me", "1.0", "", false);
        assert_eq!(short.list_label(32), "SHORT  V1.0");
        assert_eq!(crate::menu::ModuleInfo::new("0", "Short", "me", "", "", false).list_label(32), "SHORT");
    }

    fn pause_pages() -> Vec<Menu> {
        all_pages().into_iter().filter(|m| m.paused && matches!(m.page, Page::Root | Page::ConfirmQuit | Page::Controls(_) | Page::Scores | Page::Options | Page::Enhanced | Page::Sound)).collect()
    }

    /// In the cut-down picture (NO BORDER, FILL SCREEN) every page that can be open over a paused game, with its footer,
    /// notices and panel shadows, draws inside the visible rectangle and nothing outside of it, and the footer is
    /// at the bottom edge of it.
    #[test]
    fn pages_over_a_game_stay_inside_the_cut_down_picture() {
        giana_core::skip_without_data!();
        let view = View::CROP;
        assert_eq!((view.x, view.y, view.w, view.h), (crate::present::PLAY.x as usize, crate::present::PLAY.y as usize, crate::present::PLAY.w as usize, crate::present::PLAY.h as usize));
        assert_eq!(view.footer_y() + view.footer_h(), view.y + view.h, "the footer ends at the bottom of the picture");
        let b = Bindings::default();
        let scores = full_scores();
        const MARK: u32 = 0x00AB_CDEF;
        let mut fb = Framebuffer::new();
        for input in contexts() {
            let ctx = DrawCtx { bindings: &b, pad_name: Some("A controller with a really very long name indeed, yes"), scores: &scores, input, nudge: 1, view };
            for mut m in pause_pages() {
                for notice in [None, Some("NOBODY IS HOSTING AT THAT ADDRESS, TRY AGAIN LATER OK".to_string())] {
                    m.notice = notice;
                    m.audio_ok = m.cursor % 2 == 0;
                    fb.clear(MARK);
                    draw_foreground(&mut fb, &m, &ctx);
                    let (mut lo, mut hi) = ((usize::MAX, usize::MAX), (0, 0));
                    let mut footer_rows = 0;
                    for y in 0..HEIGHT {
                        for x in 0..WIDTH {
                            if fb.get_pixel(x, y) != MARK {
                                lo = (lo.0.min(x), lo.1.min(y));
                                hi = (hi.0.max(x), hi.1.max(y));
                                if y >= view.footer_y() {
                                    footer_rows += 1;
                                }
                            }
                        }
                    }
                    let what = format!("{:?} paused {} notice {:?}", m.page, m.paused, m.notice);
                    assert!(lo.0 >= view.x && lo.1 >= view.y && hi.0 < view.x + view.w && hi.1 < view.y + view.h, "{what}: drawn {lo:?}..{hi:?} leaves {view:?}");
                    assert!(footer_rows > 0 && hi.1 == view.y + view.h - 1, "{what}: no footer at the bottom");
                    let hints = footer_hints(&m, &ctx);
                    assert!(ui::hints_width_in(&hints, view) <= view.footer_max(), "{what}: {hints:?}");
                    assert!(hints.last().is_some_and(|h| matches!(h.action.as_str(), "BACK" | "RESUME" | "NO" | "CANCEL")), "{what}: the way back is kept: {hints:?}");
                }
            }
        }
    }

    /// The panels of those pages, by their frame (not the shadow, the footer may cover that), in both views.
    #[test]
    fn panels_over_a_game_leave_room_for_the_footer_and_the_edges() {
        for view in [View::FULL, View::CROP] {
            let w = view.wide();
            let pages = [
                Panel::for_list(5, 20, Pitch::Roomy, true, 0).in_view(view),
                Panel::for_list(4, 30, Pitch::Roomy, false, 0).in_view(view),
                Panel::for_list(OPTION_ITEMS.len(), w, Pitch::Tight, true, option_extra_rows(view)).in_view(view),
                Panel::for_list(ENHANCED_ITEMS.len(), w, Pitch::Roomy, true, 9).in_view(view),
                Panel::for_list(SOUND_ITEMS.len(), w, Pitch::Roomy, true, SOUND_EXTRA_ROWS).in_view(view),
                Panel::for_list(MAX_ENTRIES, 30, Pitch::Tight, true, 2).in_view(view),
                Panel::centered(w, 21, true).in_view(view),
            ];
            for p in pages {
                let (x, y, pw, ph) = p.pixel_rect();
                let (x1, y1) = (x + pw as isize - ui::SHADOW as isize, y + ph as isize - ui::SHADOW as isize);
                assert!(x >= view.x as isize && x1 + ui::SHADOW as isize <= (view.x + view.w) as isize, "{view:?} {p:?} sideways");
                assert!(y >= view.y as isize && y1 <= view.panel_bottom() as isize, "{view:?} {p:?} vertically ends at {y1}");
            }
        }
    }

    #[test]
    fn the_panel_sizes_leave_room_for_the_notice() {
        // frame bottom (+ shadow) of the tallest pages
        let end = |p: Panel| p.pixel_rect().1 + p.pixel_rect().3 as isize;
        let pages = [
            Panel::for_list(8, 20, Pitch::Roomy, true, 0).at_row(2),
            Panel::for_list(NETWORK_ITEMS.len(), WIDE, Pitch::Roomy, true, 5),
            Panel::for_list(OPTION_ITEMS.len(), WIDE, Pitch::Tight, true, 2).at_row(0),
            Panel::for_list(ENHANCED_ITEMS.len(), WIDE, Pitch::Roomy, true, 9),
            Panel::for_list(SOUND_ITEMS.len(), WIDE, Pitch::Roomy, true, SOUND_EXTRA_ROWS),
            Panel::for_list(MAX_ENTRIES, 30, Pitch::Tight, true, 2),
            Panel::centered(WIDE, 21, true),
            Panel::centered(WIDE, 19, true),
        ];
        for p in pages {
            assert!(end(p) <= ui::NOTICE_Y, "{p:?} reaches y {}", end(p));
        }
        // the strip under the main menu
        assert!(end(Panel::for_list(8, 20, Pitch::Roomy, true, 0).at_row(2)) <= ui::STRIP_Y as isize);
    }

    #[test]
    fn the_hints_follow_the_device() {
        let b = Bindings::default();
        let scores = Scores::default();
        let m = Menu::main();
        let keys = DrawCtx { bindings: &b, pad_name: None, scores: &scores, input: InputState::keyboard(), nudge: 0, view: View::FULL };
        assert_eq!(footer_hints(&m, &keys).iter().map(Hint::key_text).collect::<Vec<_>>(), ["UP/DN", "ENTER"]);
        let pad = DrawCtx { input: InputState::pad(crate::padicons::PadKind::PlayStation), ..keys };
        assert_eq!(footer_hints(&m, &pad).iter().map(Hint::key_text).collect::<Vec<_>>(), ["DPADUD", "CROSS"]);
        // capturing: cancel is Escape or Start
        let mut m = Menu::main();
        m.page = Page::Controls(Device::Pad);
        m.capturing = true;
        assert_eq!(footer_hints(&m, &pad)[0].key_text(), "OPTIONS");
        let kb = DrawCtx { input: InputState::keyboard(), ..pad };
        assert_eq!(footer_hints(&m, &kb)[0].key_text(), "ESC");
        // the result page shows no hint while it ignores the keys
        let mut m = Menu::main();
        m.page = Page::Result;
        m.result_hold = 10;
        assert!(footer_hints(&m, &kb).is_empty());
        m.result_hold = 0;
        assert_eq!(footer_hints(&m, &kb)[0].action, "CONTINUE");
    }

    #[test]
    fn the_binding_page_shows_what_is_bound() {
        giana_core::skip_without_data!();
        // a rebound pad slot changes the picture on the page
        let scores = Scores::default();
        let mut m = Menu::main();
        m.page = Page::Controls(Device::Pad);
        let input = InputState::pad(crate::padicons::PadKind::Xbox);
        let mut b = Bindings::default();
        let mut fb1 = Framebuffer::new();
        draw(&mut fb1, &m, &DrawCtx { bindings: &b, pad_name: None, scores: &scores, input, nudge: 0, view: View::FULL });
        b.set_pad(3, 0, PadInput::Button(sdl2::controller::Button::RightShoulder));
        let mut fb2 = Framebuffer::new();
        draw(&mut fb2, &m, &DrawCtx { bindings: &b, pad_name: None, scores: &scores, input, nudge: 0, view: View::FULL });
        assert_ne!(fb1.pixels, fb2.pixels);
        // and so does the controller type
        let mut fb3 = Framebuffer::new();
        draw(&mut fb3, &m, &DrawCtx { bindings: &b, pad_name: None, scores: &scores, input: InputState::pad(crate::padicons::PadKind::PlayStation), nudge: 0, view: View::FULL });
        assert_ne!(fb2.pixels, fb3.pixels);
    }

    #[test]
    fn the_longest_texts_have_room() {
        // the longest labels of the main and pause menu
        for l in ["HIGH SCORES", "CONTROLLER", "QUIT TO TITLE", "RESUME", "1 PLAYER"] {
            assert!(l.len() <= Panel::for_list(8, 20, Pitch::Roomy, true, 0).max_label_len(), "{l}");
        }
    }
}
