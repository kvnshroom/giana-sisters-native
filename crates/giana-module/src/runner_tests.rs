use super::*;
use giana_extract::d64::build_d64;

/// BASIC stub `SYS 2061` -> `JMP $0900`; the code there loads file `name` to its own address (`SETLFS 1,8,1`) and
/// jumps to `$0820` (kept clear of the file that gets loaded there).
fn loader_prg(name: &str) -> Vec<u8> {
    let mut p = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'1', 0, 0, 0];
    p.extend_from_slice(&[0x4C, 0x00, 0x09]); // JMP $0900
    p.resize(2 + 0x0900 - 0x0801, 0);
    let name_at = 0x0900 + 26;
    p.extend_from_slice(&[0xA9, 0x01, 0xA2, 0x08, 0xA0, 0x01, 0x20, 0xBA, 0xFF]); // SETLFS 1,8,1
    p.extend_from_slice(&[0xA9, name.len() as u8, 0xA2, name_at as u8, 0xA0, (name_at >> 8) as u8, 0x20, 0xBD, 0xFF]); // SETNAM
    p.extend_from_slice(&[0xA9, 0x00, 0x20, 0xD5, 0xFF]); // LOAD
    p.extend_from_slice(&[0x4C, 0x20, 0x08]); // JMP $0820
    p.extend_from_slice(name.as_bytes());
    p
}

fn game_prg() -> Vec<u8> {
    let mut g = vec![0x20, 0x08, 0x4C, 0x20, 0x08]; // JMP $0820: spins at the entry
    g.extend((0..150u32).map(|i| (i * 31 % 251) as u8));
    g
}

#[test]
fn boots_basic_stub_that_loads_the_game() {
    let g = game_prg();
    let disk = build_d64(&[("BOOT", &loader_prg("G")), ("G", &g)]);
    let r = boot_disk(&disk, "t.d64").unwrap();
    assert!(r.entry_reached, "{}", r.stop);
    assert_eq!(&r.ram[0x0820..0x0820 + g.len() - 2], &g[2..]);
    assert!(r.files.iter().any(|f| f.starts_with("LOAD \"G\" -> $0820")), "{:?}", r.files);
    assert!(r.stop.contains("game entry $0820"), "{}", r.stop);
}

#[test]
fn single_prg_without_disk() {
    // the load of "G" fails (no disk) but the program still runs
    let r = boot_prg(&loader_prg("G"), "x.prg").unwrap();
    assert!(r.files.iter().any(|f| f.contains("no disk")), "{:?}", r.files);
}

#[test]
fn stack_autostart_file_runs() {
    // file at $0100 filled with $02 up to $01FF, then code at $0203 that writes a marker and spins
    let mut prg = vec![0x00, 0x01];
    prg.extend(std::iter::repeat(0x02).take(0x100));
    prg.extend_from_slice(&[0x00, 0x00, 0x00]); // $0200..$0202
    prg.extend_from_slice(&[0xA9, 0x42, 0x8D, 0x00, 0x30, 0x4C, 0x08, 0x02]); // LDA #$42 STA $3000 JMP *
    let disk = build_d64(&[("STACK", &prg)]);
    let r = boot_disk(&disk, "s.d64").unwrap();
    assert_eq!(r.ram[0x3000], 0x42);
    assert!(r.stop.contains("idle"), "{}", r.stop);
}

#[test]
fn rejects_fast_loader_by_serial_bus_traffic() {
    // toggles $DD00 bits 3..5 (ATN/CLK/DATA) over and over
    let prg = vec![
        0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'1', 0, 0, 0, //
        0xA9, 0x00, 0x8D, 0x00, 0xDD, 0xA9, 0x38, 0x8D, 0x00, 0xDD, 0x4C, 0x0D, 0x08,
    ];
    let e = boot_disk(&build_d64(&[("FAST", &prg)]), "f.d64").unwrap_err();
    assert!(e.contains("FAST LOADER"), "{e}");
}

#[test]
fn rejects_drive_memory_write() {
    // OPEN 15,8,15,"M-W..."
    let mut p = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'1', 0, 0, 0];
    let name_at = 0x080D + 24;
    p.extend_from_slice(&[0xA9, 15, 0xA2, 8, 0xA0, 15, 0x20, 0xBA, 0xFF]); // SETLFS 15,8,15
    p.extend_from_slice(&[0xA9, 5, 0xA2, name_at as u8, 0xA0, (name_at >> 8) as u8, 0x20, 0xBD, 0xFF]); // SETNAM
    p.extend_from_slice(&[0x20, 0xC0, 0xFF, 0x4C, 0x1D, 0x08]); // OPEN, then spin
    p.extend_from_slice(b"M-W\x00\x03");
    assert_eq!(p.len() - 2, 12 + 24 + 5);
    let e = boot_disk(&build_d64(&[("DRV", &p)]), "d.d64").unwrap_err();
    assert!(e.contains("FAST LOADER"), "{e}");
}

#[test]
fn rejects_non_disk_and_empty_disk() {
    assert!(boot_disk(&[1, 2, 3], "x").is_err());
    let empty = build_d64(&[]);
    assert!(boot_disk(&empty, "e.d64").unwrap_err().contains("no program"));
}

#[test]
fn sys_parsing() {
    let mut ram = vec![0u8; 65536];
    ram[0x0801..0x0801 + 12].copy_from_slice(&[0x0B, 0x08, 0x0A, 0x00, 0x9E, b' ', b'2', b'0', b'6', b'4', 0, 0]);
    assert_eq!(sys_address(&ram), Some(2064));
    ram[0x0805] = 0x99;
    assert_eq!(sys_address(&ram), None);
}

/// The original disk loads its game through a custom loader: it must be refused with the fast loader message (the
/// existing extractor handles that disk). Skipped without the private disk image.
#[test]
fn original_disk_is_refused_clearly() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(g64) = std::fs::read(root.join("orig/Great_Giana_Sisters_The.g64")) else {
        eprintln!("skipped: orig/ not present");
        return;
    };
    let e = boot_disk(&g64, "orig.g64").unwrap_err();
    assert!(e.contains("FAST LOADER"), "{e}");
}

#[test]
fn detect_boots_disks_and_prgs() {
    use crate::make::{detect, Input};
    let g = game_prg();
    let disk = build_d64(&[("BOOT", &loader_prg("G")), ("G", &g)]);
    match detect(&disk, "hack.d64").unwrap() {
        Input::Boot(last) => assert_eq!(&last[0x0820..0x0820 + g.len() - 2], &g[2..]),
        other => panic!("expected a boot result, got {other:?}"),
    }
    // a PRG with a BASIC stub that never reaches the game is refused with a hint
    let prg = [1u8, 8, 0x0B, 8, 0x0A, 0, 0x9E, b'2', b'0', b'6', b'1', 0, 0, 0, 0x60];
    assert!(detect(&prg, "x.prg").is_err());
    // the old path for disk file 1 still works
    assert!(matches!(detect(&[0x20, 0x08, 1, 2], "f1.prg").unwrap(), Input::Prg(_)));
}

#[test]
fn make_refuses_an_unrelated_image() {
    use crate::{make::{make, Input}, Meta};
    let mut r = crate::test_rng(5);
    let base: Vec<u8> = (0..65536).map(|_| r()).collect();
    let other: Vec<u8> = (0..65536).map(|_| r()).collect();
    let f2 = vec![1u8, 8, 0, 0];
    let e = make(&base, &f2, Input::Image(other), None, Meta::new("X", "me", "1", "d")).unwrap_err();
    assert!(e.contains("does not look like a hack"), "{e}");
}

/// A PRG that loads at `$0900` and starts there (no BASIC stub), made of `code`.
fn prg_at_0900(code: &[u8]) -> Vec<u8> {
    let mut p = vec![0x00, 0x09];
    p.extend_from_slice(code);
    p
}

#[test]
fn frame_interrupts_reach_a_handler_that_ends_in_the_kernal_exit() {
    // SEI; vector $0314 -> $0930; LDA #1; STA $D01A (raster interrupt on); CLI; JMP *
    // handler: INC $3000 up to 5, then JMP $EA81 (the KERNAL's way out)
    let mut code = vec![0x78, 0xA9, 0x30, 0x8D, 0x14, 0x03, 0xA9, 0x09, 0x8D, 0x15, 0x03, 0xA9, 0x01, 0x8D, 0x1A, 0xD0, 0x58, 0x4C, 0x11, 0x09];
    code.resize(0x30, 0xEA);
    code.extend_from_slice(&[0xAD, 0x00, 0x30, 0xC9, 0x05, 0xB0, 0x03, 0xEE, 0x00, 0x30, 0x4C, 0x81, 0xEA]);
    let r = boot_prg(&prg_at_0900(&code), "irq.prg").unwrap();
    assert_eq!(r.ram[0x3000], 5, "{}", r.stop);
}

#[test]
fn no_interrupt_without_a_source() {
    // CLI but the CIA interrupt is turned off and the raster one never on: the handler never runs
    let mut code = vec![0xA9, 0x7F, 0x8D, 0x0D, 0xDC, 0xA9, 0x30, 0x8D, 0x14, 0x03, 0xA9, 0x09, 0x8D, 0x15, 0x03, 0x58, 0x4C, 0x10, 0x09];
    code.resize(0x30, 0xEA);
    code.extend_from_slice(&[0xEE, 0x00, 0x30, 0x4C, 0x81, 0xEA]);
    let r = boot_prg(&prg_at_0900(&code), "noirq.prg").unwrap();
    assert_eq!(r.ram[0x3000], 0, "{}", r.stop);
}

#[test]
fn getin_returns_the_key_the_boot_presses() {
    // loop: JSR GETIN; BEQ loop; STA $3001; JMP *
    let code = [0x20, 0xE4, 0xFF, 0xF0, 0xFB, 0x8D, 0x01, 0x30, 0x4C, 0x08, 0x09];
    let r = boot_prg(&prg_at_0900(&code), "getin.prg").unwrap();
    assert_eq!(r.ram[0x3001], b'N', "{}", r.stop);
}

#[test]
fn jmp_to_basic_run_starts_the_sys_target_of_the_stub() {
    // stub `SYS 2064`; $0810: rewrite the stub to SYS 2080 and JMP $A871 (RUN); $0820: marker, JMP *
    let mut p = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'4', 0, 0, 0, 0, 0, 0];
    p.extend_from_slice(&[0xA9, b'8', 0x8D, 0x08, 0x08, 0xA9, b'0', 0x8D, 0x09, 0x08, 0x4C, 0x71, 0xA8]);
    p.resize(2 + 0x0820 - 0x0801, 0xEA);
    p.extend_from_slice(&[0xA9, 0x42, 0x8D, 0x03, 0x30, 0x4C, 0x25, 0x08]);
    let r = boot_prg(&p, "run.prg").unwrap();
    assert_eq!(r.ram[0x3003], 0x42, "{:?} {}", r.files, r.stop);
    assert!(r.files.iter().any(|f| f.starts_with("RUN (BASIC stub")), "{:?}", r.files);
}

#[test]
fn calls_into_the_kernal_rom_return() {
    // JSR $E544 (clear screen), JSR $E000 (some routine inside the ROM), then a marker
    let code = [0x20, 0x44, 0xE5, 0x20, 0x00, 0xE0, 0xA9, 0x77, 0x8D, 0x04, 0x30, 0x4C, 0x0B, 0x09];
    let r = boot_prg(&prg_at_0900(&code), "rom.prg").unwrap();
    assert_eq!(r.ram[0x3004], 0x77, "{}", r.stop);
}

#[test]
fn boot_candidates_lists_basic_programs_in_directory_order() {
    let g = game_prg();
    let disk = build_d64(&[("BOOT", &loader_prg("G")), ("G", &g), ("TOOL", &loader_prg("G"))]);
    let c = boot_candidates(&disk).unwrap();
    assert_eq!(c, ["BOOT", "TOOL"], "G has the load address $0820 and is not a BASIC program");
    assert!(boot_disk_nth(&disk, "t.d64", 1, &BootOptions::default()).unwrap().entry_reached);
    assert!(boot_disk_nth(&disk, "t.d64", 5, &BootOptions::default()).is_err());
}

#[test]
fn intro_waits_for_a_key_in_the_keyboard_buffer_and_hands_over_with_run() {
    // like the intro of a TNW disk: wait for `$C6` (the KERNAL's IRQ keyboard scan fills it), then type RUN and
    // `JMP ($A002)` into BASIC; the stub at `$0801` starts the program again, which this time writes a marker
    let mut p = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'1', 0, 0, 0];
    p.extend_from_slice(&[0xAD, 0x00, 0x30, 0xD0, 0x29]); // LDA $3000; BNE second
    p.extend_from_slice(&[0xA9, 0x01, 0x8D, 0x00, 0x30]); // LDA #1; STA $3000
    p.extend_from_slice(&[0xA9, 0x00, 0x85, 0xC6, 0x58]); // LDA #0; STA $C6; CLI
    p.extend_from_slice(&[0xA5, 0xC6, 0xF0, 0xFC]); // wait: LDA $C6; BEQ wait
    for (i, c) in b"RUN\r".iter().enumerate() {
        p.extend_from_slice(&[0xA9, *c, 0x8D, 0x77 + i as u8, 0x02]); // LDA #c; STA $0277+i
    }
    p.extend_from_slice(&[0xA9, 0x04, 0x85, 0xC6, 0x6C, 0x02, 0xA0]); // LDA #4; STA $C6; JMP ($A002)
    p.extend_from_slice(&[0xA9, 0x42, 0x8D, 0x01, 0x30, 0x4C, 0x40, 0x08]); // second: LDA #$42; STA $3001; JMP *
    let r = boot_prg(&p, "intro.prg").unwrap();
    assert_eq!(r.ram[0x3001], 0x42, "{:?} {}", r.files, r.stop);
    assert!(r.files.iter().any(|f| f.starts_with("RUN (BASIC stub")), "{:?}", r.files);
}
