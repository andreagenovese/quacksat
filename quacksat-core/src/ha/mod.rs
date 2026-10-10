//! The duck's body as a Home Assistant device, over MQTT discovery
//! (ADR 0007). Opt-in with `[mqtt] enabled`, on every backend: the
//! wyoming path is the one that needs it, the other two get automations
//! and dashboards from it.
//!
//! Two threads. This one owns the broker socket: it reads commands,
//! answers `stop` itself, hands everything else to the worker
//! ([`worker`]), and publishes what comes back — results, the state the
//! sensors read, the discovery payload when the lists change. The worker
//! owns the robot's lanes.
//!
//! The rules of ADR 0007 §4, enforced here: a retained command is
//! dropped (the broker hands it over on every reconnect, and a retained
//! "forward" would walk the duck after each broker restart), and so is
//! an empty one (clearing that retained message sends it); presses
//! do not queue (one arriving while the worker is busy is answered
//! "busy"); stop always wins (`body::halt`, at once, whatever the worker
//! is doing).

pub mod discovery;
pub mod mqtt;
pub mod worker;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::announce::{Announcer, Lang};
use crate::config::Config;
use discovery::{Device, Lists};
use mqtt::{Client, Options, Packet, Will};
use worker::{Snapshot, Worker};

const KEEP_ALIVE_S: u16 = 30;
const PING_EVERY: Duration = Duration::from_secs(15);
/// How often the sensors are refreshed: often while the duck moves,
/// rarely otherwise — the cadence the announcer already uses.
const POLL_MOVING: Duration = Duration::from_secs(2);
const POLL_IDLE: Duration = Duration::from_secs(10);
const BACKOFF_MIN: Duration = Duration::from_secs(2);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// Start the MQTT side when the config asks for it. Refuses without
/// credentials: the topics drive a robot that walks (ADR 0007 §5).
pub fn start(config: &Config, announcer: Option<Announcer>) -> anyhow::Result<()> {
    if !config.mqtt.enabled {
        return Ok(());
    }
    anyhow::ensure!(
        !config.mqtt.username.is_empty() && !config.mqtt.password.is_empty(),
        "[mqtt] username and password are mandatory when MQTT is enabled \
         (the topics drive a robot that walks)"
    );
    let config = config.clone();
    std::thread::Builder::new()
        .name("ha-mqtt".into())
        .spawn(move || run(config, announcer))?;
    Ok(())
}

/// The topics of one node.
pub struct Topics {
    pub base: String,
    pub discovery: String,
}

impl Topics {
    pub fn new(config: &Config) -> Self {
        let node = config.mqtt_node();
        Topics {
            base: format!("{}/{node}", config.mqtt.base_topic.trim_end_matches('/')),
            discovery: format!("{}/device/{node}/config", config.mqtt.discovery_prefix.trim_end_matches('/')),
        }
    }
    fn availability(&self) -> String {
        format!("{}/availability", self.base)
    }
    fn robot(&self) -> String {
        format!("{}/robot", self.base)
    }
    fn state(&self) -> String {
        format!("{}/state", self.base)
    }
    fn commands(&self) -> String {
        format!("{}/cmd/+", self.base)
    }
    fn result(&self, command: &str) -> String {
        format!("{}/result/{command}", self.base)
    }
    /// The command a topic carries, if it is one of ours.
    fn command_of<'a>(&self, topic: &'a str) -> Option<&'a str> {
        topic.strip_prefix(&self.base)?.strip_prefix("/cmd/").filter(|c| !c.is_empty() && !c.contains('/'))
    }
}

enum Job {
    Command(String, String),
    Poll,
}

enum Out {
    Result(String, Value),
    State(Snapshot),
}

fn run(config: Config, announcer: Option<Announcer>) {
    let topics = Topics::new(&config);
    let (jobs, job_rx) = mpsc::channel::<Job>();
    let (out_tx, out) = mpsc::channel::<Out>();
    let busy = Arc::new(AtomicBool::new(false));
    {
        let config = config.clone();
        let busy = busy.clone();
        std::thread::Builder::new()
            .name("ha-worker".into())
            .spawn(move || {
                let mut worker = Worker::new(&config, announcer);
                for job in job_rx {
                    let out = match job {
                        Job::Command(name, payload) => {
                            let answer = worker.command(&name, &payload);
                            busy.store(false, Ordering::SeqCst);
                            Out::Result(name, answer)
                        }
                        Job::Poll => Out::State(worker.poll()),
                    };
                    if out_tx.send(out).is_err() {
                        break;
                    }
                }
            })
            .expect("spawning the MQTT worker");
    }
    let mut backoff = BACKOFF_MIN;
    loop {
        let started = Instant::now();
        match session(&config, &topics, &jobs, &out, &busy) {
            Ok(()) => {}
            Err(e) => tracing::warn!(error = %e, host = %config.mqtt.host, "mqtt: the broker connection ended"),
        }
        // A session that lived a while earns a quick reconnect; one that
        // failed at once waits longer each time.
        if started.elapsed() > BACKOFF_MAX {
            backoff = BACKOFF_MIN;
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

fn session(config: &Config, topics: &Topics, jobs: &Sender<Job>, out: &Receiver<Out>, busy: &AtomicBool) -> std::io::Result<()> {
    let node = config.mqtt_node();
    let mut client = Client::connect(&Options {
        host: config.mqtt.host.clone(),
        port: config.mqtt.port,
        client_id: format!("quacksat-{node}"),
        username: config.mqtt.username.clone(),
        password: config.mqtt.password.clone(),
        keep_alive_s: KEEP_ALIVE_S,
        will: Will { topic: topics.availability(), payload: "offline".into(), retain: true },
    })?;
    tracing::info!(host = %config.mqtt.host, port = config.mqtt.port, base = %topics.base, "mqtt: connected to the broker");
    client.subscribe(&topics.commands())?;
    client.publish(&topics.availability(), b"online", true)?;

    let lang = Lang::resolve(config, None);
    let device_name = match config.mqtt.device_name.trim() {
        "" if lang == Lang::It => "Papera".to_owned(),
        "" => "Duck".to_owned(),
        name => name.to_owned(),
    };
    let device = Device {
        node: &node,
        name: &device_name,
        area: config.wyoming.area.as_deref(),
        base: &topics.base,
        lang,
    };
    // Republished on every connect: the broker may have lost it.
    let mut lists: Option<Lists> = None;
    let mut published: BTreeMap<String, String> = BTreeMap::new();
    let mut last_ping = Instant::now();
    let mut next_poll = Instant::now();
    let mut poll_pending = false;

    loop {
        while let Ok(message) = out.try_recv() {
            match message {
                Out::Result(command, answer) => {
                    tracing::info!(%command, %answer, "mqtt: command answered");
                    client.publish(&topics.result(&command), answer.to_string().as_bytes(), false)?;
                }
                Out::State(snapshot) => {
                    poll_pending = false;
                    next_poll = Instant::now() + if snapshot.moving { POLL_MOVING } else { POLL_IDLE };
                    if lists.as_ref() != Some(&snapshot.lists) {
                        let (payload, now) = discovery::payload(&device, &snapshot.lists, &published);
                        client.publish(&topics.discovery, payload.to_string().as_bytes(), true)?;
                        tracing::info!(entities = now.len(), "mqtt: discovery published");
                        published = now;
                        lists = Some(snapshot.lists.clone());
                    }
                    client.publish(&topics.state(), snapshot.state.to_string().as_bytes(), true)?;
                    let robot = if snapshot.robot_online { "online" } else { "offline" };
                    client.publish(&topics.robot(), robot.as_bytes(), true)?;
                }
            }
        }

        if let Some(Packet::Publish { topic, payload, retain }) = client.read()?
            && let Some(command) = topics.command_of(&topic)
        {
            let payload = String::from_utf8_lossy(&payload).into_owned();
            if command_arrived(&mut client, topics, jobs, busy, lang, command, payload, retain)? {
                // A walk may be about to start: read the state soon after.
                next_poll = next_poll.min(Instant::now() + POLL_MOVING);
            }
        }

        if last_ping.elapsed() >= PING_EVERY {
            client.ping()?;
            last_ping = Instant::now();
        }
        if Instant::now() >= next_poll && !poll_pending && !busy.load(Ordering::SeqCst) {
            poll_pending = true;
            if jobs.send(Job::Poll).is_err() {
                return Err(std::io::Error::other("the MQTT worker is gone"));
            }
        }
    }
}

/// One command off the wire. Returns whether it went to the worker.
#[allow(clippy::too_many_arguments)]
fn command_arrived(
    client: &mut Client,
    topics: &Topics,
    jobs: &Sender<Job>,
    busy: &AtomicBool,
    lang: Lang,
    command: &str,
    payload: String,
    retain: bool,
) -> std::io::Result<bool> {
    if retain {
        // Never acted on: see the module's comment.
        tracing::warn!(command, "mqtt: a retained command was dropped");
        return Ok(false);
    }
    if payload.is_empty() {
        // How a retained message is cleared: the broker forwards the
        // empty message to every subscriber. Home Assistant's buttons
        // send "PRESS", so an empty payload is never a command.
        tracing::info!(command, "mqtt: an empty command (a retained message being cleared) was dropped");
        return Ok(false);
    }
    if command == "stop" {
        crate::body::halt();
        client.publish(&topics.result("stop"), json!({"ok": true}).to_string().as_bytes(), false)?;
        return Ok(false);
    }
    if busy.swap(true, Ordering::SeqCst) {
        tracing::info!(command, "mqtt: a press while busy was dropped");
        let error = if lang == Lang::It { "sono occupata" } else { "I'm busy" };
        let answer = json!({"ok": false, "error": error, "detail": "busy"});
        client.publish(&topics.result(command), answer.to_string().as_bytes(), false)?;
        return Ok(false);
    }
    tracing::info!(command, %payload, "mqtt: command");
    jobs.send(Job::Command(command.to_owned(), payload))
        .map_err(|_| std::io::Error::other("the MQTT worker is gone"))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_our_command_topics_are_commands() {
        let config: Config = toml::from_str("backend = \"wyoming\"\n[wyoming]\nname = \"duck\"").unwrap();
        let topics = Topics::new(&config);
        assert_eq!(topics.base, "quacksat/duck");
        assert_eq!(topics.discovery, "homeassistant/device/duck/config");
        assert_eq!(topics.command_of("quacksat/duck/cmd/forward"), Some("forward"));
        assert_eq!(topics.command_of("quacksat/duck/cmd/"), None);
        assert_eq!(topics.command_of("quacksat/duck/cmd/a/b"), None);
        assert_eq!(topics.command_of("quacksat/other/cmd/forward"), None);
        assert_eq!(topics.command_of("quacksat/duck/state"), None);
    }

    #[test]
    fn mqtt_without_credentials_does_not_start() {
        let config: Config = toml::from_str("backend = \"wyoming\"\n[mqtt]\nenabled = true").unwrap();
        let error = start(&config, None).unwrap_err().to_string();
        assert!(error.contains("mandatory"), "{error}");
    }
}
