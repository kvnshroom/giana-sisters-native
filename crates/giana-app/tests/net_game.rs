//! Two games over a real local TCP connection, on two threads, like two computers.
use giana_app::net::{join, Host, NetMode, Session};
use giana_core::state::{ControllerInput, GameState};
use std::time::{Duration, Instant};

/// Plays with made-up inputs until `frames` steps or the end of the game. Returns the checksums that agreed, the steps per
/// player and how it ended.
fn play(mut s: Session, frames: usize, seed: u32, tamper_at: Option<usize>) -> (u64, [usize; 2], Option<&'static str>) {
    let mut g = s.new_games().0;
    let (mut turns, mut done) = ([0usize; 2], 0usize);
    let mut rng = seed;
    let mut tampered = false;
    let deadline = Instant::now() + Duration::from_secs(60);
    while done < frames && !g.game_over && s.ended.is_none() && Instant::now() < deadline {
        rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
        let mut i = ControllerInput::new();
        i.right = rng >> 20 & 7 != 0;
        i.jump = rng >> 16 & 7 == 0;
        i.fire = rng >> 24 & 15 == 0;
        let player = g.player as usize;
        let n = s.tick(&mut g, &i, false);
        turns[player] += n;
        done += n;
        // Change the score once: toggling it on every call could undo itself by the next checksum
        if tamper_at.is_some_and(|t| done >= t) && !tampered {
            g.score_bcd[0] ^= 0x11;
            tampered = true;
        }
        if n == 0 {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
    // the last checksums of the other side are still on their way
    let t = Instant::now();
    while s.ended.is_none() && t.elapsed() < Duration::from_millis(200) {
        s.tick(&mut g, &ControllerInput::new(), true);
        std::thread::sleep(Duration::from_millis(5));
    }
    (s.verified, turns, s.ended)
}

fn connect(mode: NetMode, port: u16) -> (Session, Session) {
    let mut host = Host::on_port(mode, port).unwrap();
    let client = std::thread::spawn(move || join(&format!("127.0.0.1:{port}")).unwrap());
    let a = loop {
        if let Some(r) = host.poll() {
            break r.unwrap();
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let b = client.join().unwrap();
    assert_eq!((a.me, b.me, b.mode), (0, 1, mode));
    (a, b)
}

fn one_shared_game(port: u16) {
    let (a, b) = connect(NetMode::TakeTurns, port);
    // a different seed: the watching side must not depend on its own inputs
    let t = std::thread::spawn(move || play(b, 20000, 77, None));
    let a = play(a, 20000, 5, None);
    let b = t.join().unwrap();
    println!("host {a:?}, other {b:?}");
    // The made-up inputs depend on timing, so the game can end early when both lose all lives. Every checksum that was due
    // has to agree: one each 64 steps, the last one can still be on its way.
    let steps = (a.1[0] + a.1[1]) as u64;
    assert!(steps >= 500, "the game ran: {steps} steps");
    assert_eq!(a.1, b.1, "both computers count the same steps per player");
    assert!(a.0 + 2 >= steps / 64, "checksums agreed: {} of {}", a.0, steps / 64);
    assert!(a.2.is_none() || a.2 == Some("CONNECTION LOST") || a.2 == Some("THE OTHER PLAYER LEFT"), "{:?}", a.2);
    assert_ne!(a.2, Some("GAMES OUT OF SYNC"));
    assert_ne!(b.2, Some("GAMES OUT OF SYNC"));
    assert!(a.1[0] > 0 && a.1[1] > 0, "both players had a turn: {:?}", a.1);
}

#[test]
fn a_game_that_drifts_apart_is_noticed() {
    giana_core::skip_without_data!();
    let (a, b) = connect(NetMode::TakeTurns, 47105);
    let t = std::thread::spawn(move || play(b, 3000, 77, Some(200)));
    let a = play(a, 3000, 5, None);
    let b = t.join().unwrap();
    println!("host {a:?}, other {b:?}");
    assert!(a.2 == Some("GAMES OUT OF SYNC") || b.2 == Some("GAMES OUT OF SYNC"));
}

#[test]
fn both_computers_show_the_same_game_taking_turns() {
    giana_core::skip_without_data!();
    one_shared_game(47101);
}

/// Inputs from a seed, the same until the end whoever runs them.
fn made_up(rng: &mut u32, steps: &mut usize) -> ControllerInput {
    *rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
    *steps += 1;
    let mut i = ControllerInput::new();
    i.right = *rng >> 20 & 7 != 0;
    i.jump = *rng >> 16 & 7 == 0;
    i.fire = *rng >> 24 & 15 == 0;
    // now and then RESTART STAGE of the pause menu: it travels with the input, so the copy must follow it too
    i.restart_stage = *rng >> 4 & 255 == 0;
    i
}

/// Plays until both games are over: (my game, copy of the other, checksums that agreed, the other game was seen ahead of mine)
fn play_same_time(mut s: Session, seed: u32, stop_after: usize) -> (GameState, GameState, u64, bool) {
    let (mut mine, other) = s.new_games();
    let mut other = other.unwrap();
    let (mut rng, mut steps, mut watched) = (seed, 0usize, false);
    let deadline = Instant::now() + Duration::from_secs(90);
    while !(mine.game_over && other.game_over) && s.ended.is_none() && Instant::now() < deadline {
        let i = made_up(&mut rng, &mut steps);
        let i = if steps > stop_after { ControllerInput::new() } else { i };
        s.tick_same_time(&mut mine, &mut other, &i, false);
        watched |= mine.game_over && !other.game_over;
        if mine.game_over {
            std::thread::sleep(Duration::from_micros(100));
        }
    }
    let t = Instant::now();
    while s.ended.is_none() && t.elapsed() < Duration::from_millis(200) {
        s.tick_same_time(&mut mine, &mut other, &ControllerInput::new(), false);
        std::thread::sleep(Duration::from_millis(5));
    }
    (mine, other, s.verified, watched)
}

#[test]
fn each_computer_plays_its_own_game_and_the_copy_stays_true() {
    giana_core::skip_without_data!();
    let (a, b) = connect(NetMode::SameTime, 47102);
    let t = std::thread::spawn(move || play_same_time(b, 77, usize::MAX));
    let a = play_same_time(a, 5, usize::MAX);
    let b = t.join().unwrap();
    println!("host out {} checks {} watched {}, other out {} checks {} watched {}", a.0.game_over, a.2, a.3, b.0.game_over, b.2, b.3);
    assert!(a.0.game_over && b.0.game_over, "both games ended");
    assert_eq!(a.0.player, 0);
    assert_eq!(b.0.player, 1);
    assert_eq!(a.0.score_bcd, b.1.score_bcd, "the copy of Giana's game on Maria's computer");
    assert_eq!(b.0.score_bcd, a.1.score_bcd, "the copy of Maria's game on Giana's computer");
    assert_eq!((a.0.stage, a.0.lives_c64), (b.1.stage, b.1.lives_c64));
    assert_eq!(a.0.screen_ram, b.1.screen_ram);
    assert_eq!(b.0.screen_ram, a.1.screen_ram);
    assert!(a.2 > 2 && b.2 > 2, "checksums agreed: {} and {}", a.2, b.2);
}

#[test]
fn the_games_differ_because_each_player_has_their_own() {
    giana_core::skip_without_data!();
    let (a, b) = connect(NetMode::SameTime, 47106);
    let t = std::thread::spawn(move || play_same_time(b, 77, usize::MAX));
    let a = play_same_time(a, 5, usize::MAX);
    let b = t.join().unwrap();
    assert_ne!(a.0.screen_ram, b.0.screen_ram, "not the same game");
}

#[test]
fn leaving_tells_the_other_player() {
    giana_core::skip_without_data!();
    let mut host = Host::on_port(NetMode::TakeTurns, 47103).unwrap();
    let c = std::thread::spawn(|| join("127.0.0.1:47103").unwrap());
    let mut s = loop {
        if let Some(r) = host.poll() {
            break r.unwrap();
        }
    };
    let mut other = c.join().unwrap();
    other.leave();
    let mut g = s.new_games().0;
    let i = ControllerInput::new();
    let t = Instant::now();
    while s.ended.is_none() && t.elapsed() < Duration::from_secs(3) {
        s.tick(&mut g, &i, false);
    }
    assert_eq!(s.ended, Some("THE OTHER PLAYER LEFT"));
}

#[test]
fn a_wrong_program_is_turned_away() {
    use std::io::Write;
    let mut host = Host::on_port(NetMode::TakeTurns, 47104).unwrap();
    let mut c = std::net::TcpStream::connect("127.0.0.1:47104").unwrap();
    c.write_all(b"SOMETHIN").unwrap();
    let r = loop {
        if let Some(r) = host.poll() {
            break r;
        }
    };
    assert!(r.is_err());
}

#[test]
fn another_build_is_turned_away_and_told_so() {
    use giana_app::net::NetError;
    use std::io::{Read, Write};
    let mut host = Host::on_port(NetMode::TakeTurns, 47107).unwrap();
    let mut c = std::net::TcpStream::connect("127.0.0.1:47107").unwrap();
    // the right program, a build id that is not ours
    c.write_all(b"GIANANT5").unwrap();
    c.write_all(&[0, 0, 0, 0]).unwrap();
    let r = loop {
        if let Some(r) = host.poll() {
            break r;
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(matches!(r, Err(NetError::WrongVersion)));
    let mut answer = [0u8; 1];
    c.read_exact(&mut answer).unwrap();
    assert_eq!(answer[0], 0xFF);
}

#[test]
fn a_silent_client_does_not_hold_up_the_host() {
    let mut host = Host::on_port(NetMode::TakeTurns, 47108).unwrap();
    let _silent = std::net::TcpStream::connect("127.0.0.1:47108").unwrap();
    let t = Instant::now();
    for _ in 0..50 {
        assert!(host.poll().is_none());
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(t.elapsed() < Duration::from_secs(1), "polling never waits for the handshake");
    // a real player still gets in while the silent one waits
    let c = std::thread::spawn(|| join("127.0.0.1:47108").unwrap());
    let s = loop {
        if let Some(r) = host.poll() {
            break r.unwrap();
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(s.me, 0);
    c.join().unwrap();
}

#[test]
fn the_joiner_connects_in_the_background() {
    use giana_app::net::Joiner;
    let mut host = Host::on_port(NetMode::SameTime, 47109).unwrap();
    let mut j = Joiner::start("127.0.0.1:47109", "");
    let (mut a, mut b) = (None, None);
    let deadline = Instant::now() + Duration::from_secs(5);
    while (a.is_none() || b.is_none()) && Instant::now() < deadline {
        a = a.or_else(|| host.poll().map(|r| r.unwrap()));
        b = b.or_else(|| j.poll().map(|r| r.unwrap()));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!((a.unwrap().me, b.unwrap().me), (0, 1));
}

#[test]
fn a_pause_is_seen_by_the_other_player() {
    giana_core::skip_without_data!();
    let (mut a, mut b) = connect(NetMode::TakeTurns, 47110);
    let (mut ga, mut gb) = (a.new_games().0, b.new_games().0);
    let i = ControllerInput::new();
    // Giana (the host) has the first turn and opens the pause menu
    a.tick(&mut ga, &i, true);
    let t = Instant::now();
    while !b.peer_paused() && t.elapsed() < Duration::from_secs(2) {
        b.tick(&mut gb, &i, false);
    }
    assert!(b.peer_paused());
    a.tick(&mut ga, &i, false);
    while b.peer_paused() && t.elapsed() < Duration::from_secs(2) {
        b.tick(&mut gb, &i, false);
    }
    assert!(!b.peer_paused());
}

#[test]
fn the_joiner_gets_the_rule_for_the_winner_from_the_host() {
    use giana_app::result::Winner;
    for (port, rule) in [(47111, Winner::Score), (47112, Winner::Furthest)] {
        let mut host = Host::on_port(NetMode::SameTime, port).unwrap();
        host.set_winner(rule);
        let client = std::thread::spawn(move || join(&format!("127.0.0.1:{port}")).unwrap());
        let a = loop {
            if let Some(r) = host.poll() {
                break r.unwrap();
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let b = client.join().unwrap();
        assert_eq!((a.winner, b.winner, b.mode), (rule, rule, NetMode::SameTime));
    }
}

#[test]
fn the_rules_of_the_host_reach_the_other_side_and_unknown_ones_are_refused() {
    use giana_app::net::NetError;
    // no enhancement changes the game yet: the rules are 0 on both sides
    let (a, b) = connect(NetMode::TakeTurns, 47113);
    assert_eq!((a.rules, b.rules), (0, 0));
    // a host of a newer build that plays by a rule this build does not know is refused
    let mut host = Host::on_port(NetMode::TakeTurns, 47114).unwrap();
    host.set_rules(0x01);
    let client = std::thread::spawn(|| join("127.0.0.1:47114"));
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(3) && !client.is_finished() {
        let _ = host.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(client.join().unwrap(), Err(NetError::WrongVersion)));
}

// ---- ROM modules over the network ----

mod module_transfer {
    use giana_app::modules::{Base, Store};
    use giana_app::net::{join_receiving, Host, ModuleSink, NetError, NetMode};
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::Duration;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("giana-nettest-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sink(tag: &str, data_dir: PathBuf) -> (ModuleSink, PathBuf) {
        let dir = scratch(tag);
        (ModuleSink { store: Store::new(dir.join("modules")), data_dir }, dir)
    }

    fn stored(s: &ModuleSink) -> usize {
        s.store.list(None).len()
    }

    /// A host that is a plain socket: reads the hello, answers with a module offer, and sends `payload` if asked.
    /// The thread returns whether the guest asked for the file.
    fn fake_host(announced_size: u32, sha: [u8; 32], payload: Vec<u8>) -> (u16, std::thread::JoinHandle<bool>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut hello = [0u8; 20];
            c.read_exact(&mut hello).unwrap();
            let mut msg = vec![0xFE];
            msg.extend_from_slice(&announced_size.to_be_bytes());
            msg.extend_from_slice(&0x0123_4567_89ab_cdefu64.to_be_bytes());
            msg.extend_from_slice(&sha);
            c.write_all(&msg).unwrap();
            let mut ask = [0u8; 1];
            if c.read_exact(&mut ask).is_err() || ask[0] != 1 {
                return false;
            }
            let _ = c.write_all(&payload);
            let mut end = [0u8; 1];
            let _ = c.read(&mut end);
            true
        });
        (port, t)
    }

    #[test]
    fn an_oversized_offer_is_refused_without_asking_for_the_file() {
        let (s, dir) = sink("big", PathBuf::from("/nonexistent"));
        let (port, host) = fake_host((1 << 20) + 1, [0; 32], vec![]);
        let r = join_receiving(&format!("127.0.0.1:{port}"), "", Some(&s));
        assert!(matches!(r, Err(NetError::OtherModule)));
        assert!(!host.join().unwrap(), "the guest must not ask for it");
        assert_eq!(stored(&s), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupted_bytes_are_refused_and_not_stored() {
        let (s, dir) = sink("bad", PathBuf::from("/nonexistent"));
        let good = vec![7u8; 5000];
        let sha: [u8; 32] = Sha256::digest(&good).into();
        let mut corrupt = good.clone();
        corrupt[100] ^= 1;
        let (port, host) = fake_host(good.len() as u32, sha, corrupt);
        let r = join_receiving(&format!("127.0.0.1:{port}"), "", Some(&s));
        assert!(matches!(r, Err(NetError::BadModule)));
        assert!(host.join().unwrap());
        assert_eq!(stored(&s), 0);
        // intact bytes that are not a module at all are refused too
        let (port, host) = fake_host(good.len() as u32, sha, good);
        assert!(matches!(join_receiving(&format!("127.0.0.1:{port}"), "", Some(&s)), Err(NetError::BadModule)));
        host.join().unwrap();
        assert_eq!(stored(&s), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_transfer_that_ends_early_is_refused() {
        // the host announces 4000 bytes, sends 10 and closes
        let (s, dir) = sink("stall", PathBuf::from("/nonexistent"));
        let (port, host) = fake_host(4000, [0; 32], vec![1; 10]);
        assert!(matches!(join_receiving(&format!("127.0.0.1:{port}"), "", Some(&s)), Err(NetError::BadModule)));
        host.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn host_original_and_guest_module_is_a_plain_refusal() {
        let (s, dir) = sink("orig", PathBuf::from("/nonexistent"));
        let mut host = Host::on_port(NetMode::TakeTurns, 47131).unwrap();
        host.set_module("");
        let c = std::thread::spawn({
            let s = s.clone();
            move || join_receiving("127.0.0.1:47131", "0123456789abcdef", Some(&s))
        });
        let h = loop {
            if let Some(r) = host.poll() {
                break r;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(matches!(h, Err(NetError::OtherModule)));
        assert!(matches!(c.join().unwrap(), Err(NetError::OtherModule)));
        assert_eq!(stored(&s), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_host_sends_its_module_and_the_guest_stores_it() {
        giana_core::skip_without_data!();
        let Some(data_dir) = giana_core::data::candidate_dirs(None).into_iter().find(|d| giana_extract::verify_dir(d).is_empty()) else { return };
        let base = Base::read(&data_dir).unwrap();
        let work = scratch("ok");
        let mut hack = base.ingame.clone();
        hack[0xAC10] ^= 0x55;
        let image = work.join("net hack.bin");
        std::fs::write(&image, &hack).unwrap();
        let host_store = Store::new(work.join("host"));
        let added = host_store.add(&image, &data_dir).unwrap();

        let mut host = Host::on_port(NetMode::TakeTurns, 47132).unwrap();
        host.set_module(&added.id);
        host.share_module(&host_store);
        let (s, dir) = sink("guest", data_dir.clone());
        let c = std::thread::spawn({
            let s = s.clone();
            move || join_receiving("127.0.0.1:47132", "", Some(&s))
        });
        let h = loop {
            if let Some(r) = host.poll() {
                break r;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(matches!(h, Err(NetError::OtherModule)));
        match c.join().unwrap() {
            Err(NetError::ModuleReceived(id)) => assert_eq!(id, added.id),
            other => panic!("unexpected {:?}", other.map(|_| ())),
        }
        let list = s.store.list(Some(&data_dir));
        assert_eq!((list.len(), list[0].id.as_str()), (1, added.id.as_str()));
        assert_eq!(s.store.read_module(&added.id), host_store.read_module(&added.id));
        // now both play it: the join works
        let id = added.id.clone();
        let c = std::thread::spawn(move || join_receiving("127.0.0.1:47132", &id, Some(&s)));
        let h = loop {
            if let Some(r) = host.poll() {
                break r;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(h.is_ok() && c.join().unwrap().is_ok());
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(work);
    }
}
