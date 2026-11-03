//! The few KERNAL calls the game makes (all of them outside play: the high-score file at start-up and when it is saved),
//! as hooks. The oracle has no ROMs; `LOAD` reads from the player's `.d64`, everything else does nothing and succeeds.

use crate::machine::Machine;
use giana_module::disk::Disk;

/// KERNAL entry points the game calls: SETLFS, SETNAM, LOAD, SAVE, LISTEN, SECOND, CIOUT, UNLSN, IOINIT, GETIN.
pub const ENTRIES: [u16; 10] = [0xFFBA, 0xFFBD, 0xFFD5, 0xFFD8, 0xFFB1, 0xFF93, 0xFFA8, 0xFFAE, 0xFF84, 0xFFE4];

pub(crate) fn install(m: &mut Machine, d64: Option<Vec<u8>>) {
    for a in ENTRIES {
        m.hook(a, |m| {
            m.cpu.c = false;
            m.rts();
        });
    }
    // SETNAM: A = length, X/Y = address of the name
    m.hook(0xFFBD, |m| {
        m.kernal_name = (m.cpu.a, m.cpu.x as u16 | (m.cpu.y as u16) << 8);
        m.cpu.c = false;
        m.rts();
    });
    // LOAD with secondary address 0: the file's data goes to X/Y (its own two-byte load address is skipped)
    let disk = d64.and_then(|d| Disk::parse(&d).ok());
    m.hook(0xFFD5, move |m| {
        let (n, p) = m.kernal_name;
        let name: Vec<u8> = (0..n as u16).map(|i| m.peek(p.wrapping_add(i))).collect();
        let file = disk.as_ref().and_then(|d| d.find(&name).ok().flatten().and_then(|e| d.read_entry(&e).ok()));
        match file {
            Some(data) if data.len() >= 2 => {
                let to = m.cpu.x as u16 | (m.cpu.y as u16) << 8;
                for (i, b) in data[2..].iter().enumerate() {
                    m.poke(to.wrapping_add(i as u16), *b);
                }
                let end = to.wrapping_add(data.len() as u16 - 2);
                m.cpu.x = end as u8;
                m.cpu.y = (end >> 8) as u8;
                m.cpu.c = false;
            }
            _ => {
                m.cpu.a = 4; // FILE NOT FOUND
                m.cpu.c = true;
            }
        }
        m.rts();
    });
    m.hook(0xFFE4, |m| {
        m.cpu.a = 0;
        m.cpu.z = true;
        m.cpu.c = false;
        m.rts();
    });
}
