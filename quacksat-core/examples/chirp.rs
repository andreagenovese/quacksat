//! Dev probe for the robotd client: connect, ask for health, chirp, and
//! print the first state frames. Works against `robotd --fake` locally or
//! the real duck over `ssh -L /tmp/robotd.sock:/run/robotd.sock <duck>`.
//!
//! Usage: cargo run -p quacksat-core --example chirp -- /tmp/robotd.sock

use duck_ipc_proto as proto;
use quacksat_core::robotd::{Control, StreamEvent, run_state_stream};

fn main() -> anyhow::Result<()> {
    let socket = std::env::args()
        .nth(1)
        .unwrap_or_else(|| proto::socket::ROBOT.to_string());

    let mut control = Control::connect(&socket)?;

    let health = control.request(&proto::Call::RobotHealth)?;
    println!("health: {}", serde_json::to_string(&health.result)?);

    let chirp = control.intent(&proto::Call::RobotSound(proto::SoundParams {
        tag: proto::SoundTag::Chirp,
        hold: None,
    }))?;
    println!(
        "chirp: accepted={} reason={:?}",
        chirp.accepted, chirp.reason
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let stream = std::thread::spawn(move || run_state_stream(&socket, Some(5), &tx));
    for event in rx.iter().take(4) {
        match event {
            StreamEvent::Subscribed(r) => {
                println!("subscribed: accepted={} walk={:?}", r.accepted, r.walk)
            }
            StreamEvent::State(s) => println!(
                "state: t={:.2} policy={} fallen={} loop_hz={:.0}",
                s.t, s.policy, s.safety.fallen, s.control_loop.hz
            ),
        }
    }
    drop(rx);
    let _ = stream.join();
    Ok(())
}
