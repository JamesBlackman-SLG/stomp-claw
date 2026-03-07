use midir::{Ignore, MidiInput};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::config;
use crate::events::{Event, EventSender};

pub fn run(tx: EventSender) {
    let pedal_down = Arc::new(AtomicBool::new(false));

    let mut midi_in = MidiInput::new("stomp_claw").expect("Failed to create MIDI input");
    midi_in.ignore(Ignore::None);

    let port = midi_in.ports().into_iter().find(|p| {
        midi_in.port_name(p).map(|n| n.contains("FS-1-WL")).unwrap_or(false)
    }).expect("FS-1-WL not found — is the pedal connected?");

    tracing::info!("Connected to: {}", midi_in.port_name(&port).unwrap_or_default());

    let _conn = midi_in.connect(
        &port,
        "stomp_claw_read",
        move |_, msg, _| {
            if msg.len() >= 3 && (msg[0] & 0xF0) == 0xB0 && msg[1] == config::PEDAL_CC {
                if msg[2] == 127 && !pedal_down.load(Ordering::Relaxed) {
                    pedal_down.store(true, Ordering::Relaxed);
                    let _ = tx.send(Event::PedalDown);
                } else if msg[2] == 0 && pedal_down.load(Ordering::Relaxed) {
                    pedal_down.store(false, Ordering::Relaxed);
                    let _ = tx.send(Event::PedalUp);
                }
            }
        },
        (),
    ).expect("Failed to connect to MIDI device");

    tracing::info!("MIDI listener ready. Hold foot pedal to record.");

    // Keep the connection alive
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
