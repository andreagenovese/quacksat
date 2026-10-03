//! What the duck says on its own, outside a turn.
//!
//! `robot.go_to` and `robot.map_explore` answer at once ("started") and
//! the journey runs on in quack-navd; the turn ends there, so without
//! this the duck reached the kitchen in silence (the MuJoCo twin,
//! 2026-10-02: "vai in cucina" → arrived 2 min 14 s later, not a word).
//! quack-navd has no channel to push events to the satellite, so the
//! satellite asks: a thread of its own polls `robot.map_status` — every
//! [`POLL_ACTIVE`] while something moves or a job is followed, every
//! [`POLL_IDLE`] otherwise — and a [`Tracker`] turns what it reads into
//! [`Event`]s, each said once, in a short fixed phrase ([`Event::phrase`]):
//!
//! - how a job the satellite itself started ended (arrived, failed and
//!   why, stopped; an exploration's minutes and share mapped);
//! - when the duck moves on its own (`explore.self_started`: the
//!   homecoming's search or exploration at boot, the relocalization a
//!   job asked for on an untrusted pose), and how that ended.
//!
//! The events wait in a queue until the backend is between turns
//! (half-duplex: never over the user, never over a reply): the `direct`
//! backend speaks them with its own TTS, the `agent` backend hands them
//! to the bridge as a `say` (docs/agent-protocol.md).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::config::Config;
use crate::nav_client::NavLane;

/// How often `robot.map_status` is read while a job is followed or the
/// duck moves on its own. The call costs quack-navd a map snapshot and
/// a clearance under its tool lock (milliseconds); there is no cheaper
/// read that carries `explore`.
pub const POLL_ACTIVE: Duration = Duration::from_secs(2);
/// ... and while nothing moves: only to notice the duck starting on its
/// own (a restarted quack-navd's homecoming).
pub const POLL_IDLE: Duration = Duration::from_secs(10);
/// A job reported started that never shows up in the status (it ended
/// before the first poll with the same outcome as the one before it) is
/// let go after this long, unsaid.
const PENDING_TIMEOUT: Duration = Duration::from_secs(20);
/// The same sentence twice within this long is said once.
const REPEAT_WINDOW: Duration = Duration::from_secs(30);
/// Sentences waiting for the end of a long conversation: the oldest go.
const QUEUE_MAX: usize = 4;

/// The language of the fixed phrases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    It,
    En,
}

impl Lang {
    /// "it", "it-IT", "it_IT" → Italian; anything else → English.
    pub fn from_code(code: &str) -> Self {
        if code.trim().to_ascii_lowercase().starts_with("it") { Lang::It } else { Lang::En }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::It => "it",
            Lang::En => "en",
        }
    }

    /// `[announce] language`, else `[direct.stt] language`, else what the
    /// bridge said it speaks, else English.
    pub fn resolve(config: &Config, bridge: Option<&str>) -> Self {
        [config.announce.language.as_str(), config.direct.stt.language.as_str(), bridge.unwrap_or_default()]
            .into_iter()
            .find(|code| !code.trim().is_empty())
            .map_or(Lang::En, Lang::from_code)
    }
}

/// A job the satellite started through one of its tool calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// `robot.go_to` with a `place`: the name as the agent gave it.
    GoToPlace(String),
    /// `robot.go_to` with `x`, `y`.
    GoToPoint,
    /// `robot.map_explore`.
    Explore,
}

impl Job {
    /// The job a successful tool call started, if it started one: an
    /// answer with `started: true` (a relocalization first counts — the
    /// job is underway), not a watch.
    pub fn started_by(name: &str, args: &Value, answer: &Value) -> Option<Job> {
        if answer.get("started").and_then(Value::as_bool) != Some(true)
            || answer.get("watch").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        match name {
            "robot.go_to" => Some(match args.get("place").and_then(Value::as_str) {
                Some(place) if !place.trim().is_empty() => Job::GoToPlace(place.trim().to_owned()),
                _ => Job::GoToPoint,
            }),
            "robot.map_explore" => Some(Job::Explore),
            _ => None,
        }
    }
}

/// Why a job did not get where it was going, from quack-navd's reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// "no way to (x, y) on the map".
    NoWay,
    /// "is not mapped floor", "a part it has not mapped yet".
    NotMapped,
    /// "the way to … took too long", "time budget of … spent".
    TooLong,
    /// The pose: "may have been moved and could not find where it is",
    /// "could not find its position again", "position lost", "not trusted".
    Lost,
    /// "the duck is seated or fallen and did not get up".
    Fallen,
    /// "battery at …".
    Battery,
    /// A drop or a hole near the goal.
    Drop,
    Other,
}

impl Why {
    pub fn of(reason: &str) -> Self {
        let r = reason.to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|w| r.contains(w));
        if has(&["seated or fallen", "fallen"]) {
            Why::Fallen
        } else if has(&["may have been moved", "could not find", "position lost", "not trusted", "relocator"]) {
            Why::Lost
        } else if has(&["not mapped", "has not mapped"]) {
            Why::NotMapped
        } else if has(&["no way"]) {
            Why::NoWay
        } else if has(&["too long", "time budget"]) {
            Why::TooLong
        } else if has(&["battery"]) {
            Why::Battery
        } else if has(&["drop", "hole"]) {
            Why::Drop
        } else {
            Why::Other
        }
    }
}

/// What moves the duck when nobody asked (`explore.self_started`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// The homecoming's walk-and-look at boot (`searching`).
    Search,
    /// A job asked for on an untrusted pose finds the pose first
    /// (`relocalizing`).
    Relocalize,
    /// The homecoming's own exploration (`running`).
    Explore,
}

/// One thing to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Arrived(Job),
    /// A job (or the duck's own exploration) ended short of its goal.
    Failed(Option<Job>, Why),
    Stopped,
    /// An exploration ended with the house done.
    Mapped,
    /// An exploration ended on its budget (or the battery, or stuck).
    Explored { minutes: u64, percent: Option<u64> },
    /// The duck started moving on its own.
    OnMyOwn(Motion),
    /// The pose is found again.
    Found,
    /// The search ended without finding it.
    NotFound,
}

impl Event {
    /// The fixed phrase, in `lang`. Short on purpose: said by the duck,
    /// heard across a room, no model round trip.
    pub fn phrase(&self, lang: Lang) -> String {
        let it = lang == Lang::It;
        match self {
            Event::Arrived(Job::GoToPlace(place)) => {
                if it { format!("Sono arrivata in {place}.") } else { format!("I made it to {place}.") }
            }
            Event::Arrived(_) => pick(it, "Sono arrivata.", "I've arrived."),
            Event::Failed(job, why) => failed(job.as_ref(), *why, it),
            Event::Stopped => pick(it, "Mi sono fermata.", "I've stopped."),
            Event::Mapped => pick(it, "Ho finito di esplorare: la casa è mappata.", "I've finished exploring: the house is mapped."),
            Event::Explored { minutes, percent } => {
                let time = match (it, minutes) {
                    (true, 1) => "per un minuto".to_string(),
                    (true, n) => format!("per {n} minuti"),
                    (false, 1) => "for a minute".to_string(),
                    (false, n) => format!("for {n} minutes"),
                };
                match (it, percent) {
                    (true, Some(p)) => format!("Ho esplorato {time}, la casa è mappata al {p} per cento."),
                    (true, None) => format!("Ho esplorato {time}."),
                    (false, Some(p)) => format!("I explored {time}; the house is {p} percent mapped."),
                    (false, None) => format!("I explored {time}."),
                }
            }
            Event::OnMyOwn(Motion::Search) => pick(it, "Non sono sicura di dove sono: mi guardo intorno.", "I'm not sure where I am: I'm looking around."),
            Event::OnMyOwn(Motion::Relocalize) => pick(
                it,
                "Prima di partire mi guardo intorno per ritrovarmi.",
                "Before I set off, I'm looking around to find where I am.",
            ),
            Event::OnMyOwn(Motion::Explore) => pick(it, "Riprendo a esplorare la casa.", "I'm going on exploring the house."),
            Event::Found => pick(it, "Mi sono ritrovata.", "I know where I am again."),
            Event::NotFound => pick(it, "Non sono riuscita a ritrovarmi.", "I couldn't find where I am."),
        }
    }
}

fn pick(it: bool, italian: &str, english: &str) -> String {
    if it { italian } else { english }.to_string()
}

fn failed(job: Option<&Job>, why: Why, it: bool) -> String {
    match why {
        Why::NoWay => match job {
            Some(Job::GoToPlace(place)) => {
                if it { format!("Non trovo una strada per {place}.") } else { format!("I can't find a way to {place}.") }
            }
            _ => pick(it, "Non trovo una strada fin lì.", "I can't find a way there."),
        },
        Why::NotMapped => pick(
            it,
            "Non ci arrivo: quella parte della casa non è ancora mappata.",
            "I can't get there: that part of the house isn't mapped yet.",
        ),
        Why::TooLong => pick(it, "Ci stavo mettendo troppo, mi sono fermata.", "It was taking too long, so I stopped."),
        Why::Lost => pick(it, "Forse mi hanno spostata: non riesco a ritrovarmi.", "I may have been moved: I can't find where I am."),
        Why::Fallen => pick(it, "Sono caduta e non riesco a rialzarmi.", "I've fallen and I can't get up."),
        Why::Battery => pick(it, "Ho poca batteria, mi fermo qui.", "My battery is low, so I'm stopping here."),
        Why::Drop => pick(it, "C'è un gradino o un buco sulla strada, mi fermo.", "There's a step or a hole in the way, so I stopped."),
        Why::Other => match job {
            Some(Job::Explore) => pick(it, "Ho dovuto smettere di esplorare.", "I had to stop exploring."),
            Some(_) => pick(it, "Non sono riuscita ad arrivare.", "I couldn't get there."),
            None => pick(it, "Non ci sono riuscita.", "I couldn't make it."),
        },
    }
}

/// The part of `robot.map_status` the tracker reads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// `explore.state`: idle, relocalizing, searching, running, done,
    /// stopped, failed.
    pub state: String,
    pub reason: Option<String>,
    pub self_started: bool,
    pub elapsed_s: Option<u64>,
    /// `explore.progress.done`: the house is mapped.
    pub done: bool,
    /// `explore.progress.percent`, else `house.percent_mapped`.
    pub percent: Option<f64>,
}

impl Status {
    pub fn from_map_status(answer: &Value) -> Option<Self> {
        let explore = answer.get("explore")?;
        let progress = explore.get("progress");
        Some(Status {
            state: explore.get("state").and_then(Value::as_str)?.to_owned(),
            reason: explore.get("reason").and_then(Value::as_str).map(str::to_owned),
            self_started: explore.get("self_started").and_then(Value::as_bool).unwrap_or(false),
            elapsed_s: explore.get("elapsed_s").and_then(Value::as_u64),
            done: progress.and_then(|p| p.get("done")).and_then(Value::as_bool).unwrap_or(false),
            percent: progress
                .and_then(|p| p.get("percent"))
                .and_then(Value::as_f64)
                .or_else(|| answer.pointer("/house/percent_mapped").and_then(Value::as_f64)),
        })
    }

    fn moving(&self) -> bool {
        matches!(self.state.as_str(), "running" | "relocalizing" | "searching")
    }

    fn ended(&self) -> bool {
        matches!(self.state.as_str(), "done" | "stopped" | "failed")
    }

    fn reason_has(&self, words: &str) -> bool {
        self.reason.as_deref().is_some_and(|r| r.contains(words))
    }

    /// The relocalization before a job confirmed the pose: `idle` for a
    /// moment, then the job starts (quack-navd's `relocalized`).
    fn job_about_to_start(&self) -> bool {
        self.state == "idle" && self.reason_has("the pose is confirmed")
    }

    fn motion(&self) -> Option<Motion> {
        if !self.self_started || !self.moving() {
            return None;
        }
        Some(match self.state.as_str() {
            "searching" => Motion::Search,
            "relocalizing" => Motion::Relocalize,
            _ => Motion::Explore,
        })
    }
}

#[derive(Debug, Clone)]
struct Followed {
    job: Job,
    since: Instant,
    /// The status as it was when the job was reported started: an ended
    /// status equal to it is the job before, not this one.
    before: Option<Status>,
    /// The job has been seen running (or relocalizing).
    seen: bool,
}

/// The state machine: statuses in, events out, each once. Pure (time
/// comes in), so the tests drive it without a daemon.
#[derive(Debug, Default)]
pub struct Tracker {
    job: Option<Followed>,
    own: Option<Motion>,
    last: Option<Status>,
    /// The satellite itself sent the stop (`stop: true` through a tool
    /// call): the agent answers that turn, the duck need not say it again.
    we_stopped: bool,
    pub journeys: bool,
    pub own_motion: bool,
}

impl Tracker {
    pub fn new(journeys: bool, own_motion: bool) -> Self {
        Self { journeys, own_motion, ..Self::default() }
    }

    /// Whether to poll at the busy rate.
    pub fn watching(&self) -> bool {
        self.job.is_some() || self.own.is_some() || self.last.as_ref().is_some_and(Status::moving)
    }

    /// A tool call of ours answered: follow the job it started, or note
    /// the stop it sent.
    pub fn tool_called(&mut self, name: &str, args: &Value, answer: &Value, now: Instant) {
        if !matches!(name, "robot.go_to" | "robot.map_explore") {
            return;
        }
        if args.get("stop").and_then(Value::as_bool) == Some(true) {
            self.we_stopped = true;
            return;
        }
        if let Some(job) = Job::started_by(name, args, answer) {
            tracing::info!(?job, "following the job");
            self.we_stopped = false;
            self.job = Some(Followed { job, since: now, before: self.last.clone(), seen: false });
        }
    }

    /// One reading of `robot.map_status`; what to say about it.
    pub fn observe(&mut self, s: &Status, now: Instant) -> Vec<Event> {
        let mut said = Vec::new();
        let ours = self.job.is_some();

        // The duck's own motion: its start, and its end.
        let motion = s.motion();
        if let Some(motion) = motion
            && self.own != Some(motion)
        {
            // A stop of ours was for what moved before this.
            self.we_stopped = false;
            if self.own_motion {
                said.push(Event::OnMyOwn(motion));
            }
        } else if motion.is_none()
            && let Some(was) = self.own
        {
            let event = match (was, s.state.as_str()) {
                // The relocalization confirmed the pose: the job starts
                // (or already has, between two polls).
                (Motion::Relocalize, "idle" | "running") => Some(Event::Found),
                (Motion::Search, "idle") if s.reason_has("found where it is") => Some(Event::Found),
                (Motion::Search, "idle") if s.reason_has("without finding") => Some(Event::NotFound),
                (Motion::Search, "running") => Some(Event::Found),
                // How it ended, when it is not a job of ours (that one
                // is said below, with the job's own words).
                (_, _) if s.ended() && !ours => {
                    let job = (was == Motion::Explore).then_some(Job::Explore);
                    self.ending(job.as_ref(), s, was == Motion::Explore)
                }
                _ => None,
            };
            if self.own_motion {
                said.extend(event);
            }
        }
        self.own = motion;

        // A job of ours.
        if let Some(followed) = &mut self.job {
            if s.moving() {
                followed.seen = true;
            } else if s.ended() {
                if followed.seen || followed.before.as_ref() != Some(s) {
                    let job = followed.job.clone();
                    self.job = None;
                    if self.journeys {
                        said.extend(self.ending(Some(&job), s, true));
                    }
                } else if now.duration_since(followed.since) > PENDING_TIMEOUT {
                    tracing::info!("the job never showed up in the status; not following it");
                    self.job = None;
                }
            } else if s.job_about_to_start() {
                // Between the relocalization and the job: keep waiting.
            } else if followed.seen || now.duration_since(followed.since) > PENDING_TIMEOUT {
                // Idle with no outcome: quack-navd restarted under the
                // job (its status starts over). Nothing true to say.
                tracing::info!(state = %s.state, "the job left no outcome; not following it");
                self.job = None;
            }
        }
        self.last = Some(s.clone());
        said
    }

    /// The sentence for an ended status. `exploring` says whether the
    /// ending is an exploration's (its minutes and its share mapped).
    fn ending(&self, job: Option<&Job>, s: &Status, exploring: bool) -> Option<Event> {
        let reason = s.reason.as_deref().unwrap_or_default();
        match s.state.as_str() {
            "stopped" => (!self.we_stopped).then_some(Event::Stopped),
            "failed" => Some(Event::Failed(job.cloned(), Why::of(reason))),
            "done" => match job {
                Some(Job::GoToPlace(_) | Job::GoToPoint) => Some(if reason.starts_with("arrived") {
                    Event::Arrived(job.cloned().expect("matched"))
                } else {
                    Event::Failed(job.cloned(), Why::of(reason))
                }),
                _ if exploring => Some(if s.done {
                    Event::Mapped
                } else {
                    Event::Explored {
                        minutes: s.elapsed_s.map_or(1, |secs| ((secs + 30) / 60).max(1)),
                        percent: s.percent.map(|p| p.round().max(0.0) as u64),
                    }
                }),
                _ => None,
            },
            _ => None,
        }
    }
}

struct Shared {
    tracker: Tracker,
    queue: VecDeque<Event>,
    recent: Vec<(Event, Instant)>,
}

impl Shared {
    fn push(&mut self, events: Vec<Event>, now: Instant) {
        self.recent.retain(|(_, at)| now.duration_since(*at) < REPEAT_WINDOW);
        for event in events {
            if self.recent.iter().any(|(said, _)| *said == event) {
                tracing::debug!(?event, "said a moment ago; not again");
                continue;
            }
            tracing::info!(?event, "to announce");
            self.recent.push((event.clone(), now));
            self.queue.push_back(event);
            while self.queue.len() > QUEUE_MAX {
                self.queue.pop_front();
            }
        }
    }
}

/// The handle the backends and the tools share: the tool calls report
/// the jobs they start, the poller feeds statuses, the backend takes the
/// sentences when it is free to speak.
#[derive(Clone)]
pub struct Announcer {
    shared: Arc<Mutex<Shared>>,
    poke: Option<mpsc::Sender<()>>,
}

impl Announcer {
    /// Start following quack-navd, when `[announce]` and `[nav]` are on:
    /// a thread with its own lane to the socket, so a slow answer never
    /// holds the voice loop.
    pub fn start(config: &Config) -> Option<Self> {
        if !config.announce.enabled || !config.nav.enabled {
            return None;
        }
        let (poke, woken) = mpsc::channel();
        let announcer = Self { poke: Some(poke), ..Self::detached_with(&config.announce) };
        let shared = announcer.shared.clone();
        let mut lane = NavLane::unprobed(&config.nav.socket);
        std::thread::Builder::new()
            .name("announce".into())
            .spawn(move || poll(&mut lane, &shared, &woken))
            .ok()?;
        tracing::info!(socket = %config.nav.socket, "following the navigation to say how its journeys end");
        Some(announcer)
    }

    /// No poller: statuses come from [`Announcer::observe`] (tests).
    pub fn detached() -> Self {
        Self::detached_with(&crate::config::AnnounceConfig::default())
    }

    fn detached_with(config: &crate::config::AnnounceConfig) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                tracker: Tracker::new(config.journeys, config.own_motion),
                queue: VecDeque::new(),
                recent: Vec::new(),
            })),
            poke: None,
        }
    }

    /// A tool call answered: follow the job it started, and read the
    /// status at once (a job that fails in its first second still says so).
    pub fn tool_called(&self, name: &str, args: &Value, answer: &Value) {
        lock(&self.shared).tracker.tool_called(name, args, answer, Instant::now());
        if let Some(poke) = &self.poke {
            let _ = poke.send(());
        }
    }

    /// One `robot.map_status` answer, read.
    pub fn observe(&self, answer: &Value) {
        let Some(status) = Status::from_map_status(answer) else { return };
        let now = Instant::now();
        let mut shared = lock(&self.shared);
        let events = shared.tracker.observe(&status, now);
        shared.push(events, now);
    }

    /// The next sentence, when there is one. Called by the backend only
    /// between turns: the queue is what keeps the duck from talking over
    /// anyone.
    pub fn next(&self) -> Option<Event> {
        lock(&self.shared).queue.pop_front()
    }

    /// Put a sentence in the queue directly (tests, and the backends'
    /// own tests).
    pub fn push(&self, event: Event) {
        lock(&self.shared).push(vec![event], Instant::now());
    }
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The poller: `robot.map_status` at the rate the tracker asks for, at
/// once when a tool call started a job. Ends when every handle is gone.
fn poll(lane: &mut NavLane, shared: &Arc<Mutex<Shared>>, woken: &mpsc::Receiver<()>) {
    let mut reachable = true;
    loop {
        match lane.call("robot.map_status", &json!({})) {
            Ok(answer) => {
                if !reachable {
                    tracing::info!("the navigation daemon answers again");
                    reachable = true;
                }
                match Status::from_map_status(&answer) {
                    Some(status) => {
                        let now = Instant::now();
                        let mut s = lock(shared);
                        let events = s.tracker.observe(&status, now);
                        s.push(events, now);
                    }
                    None => tracing::debug!("map_status without `explore`: nothing to follow"),
                }
            }
            Err(e) => {
                if reachable {
                    tracing::info!(error = %e, "the navigation daemon does not answer; asking again later");
                    reachable = false;
                }
            }
        }
        let interval = if lock(shared).tracker.watching() { POLL_ACTIVE } else { POLL_IDLE };
        match woken.recv_timeout(interval) {
            Ok(()) => {
                // The job's first status is set before the tool answers;
                // a short pause lets a failure in its first moment land too.
                std::thread::sleep(Duration::from_millis(300));
                while woken.try_recv().is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(state: &str, reason: Option<&str>, self_started: bool) -> Status {
        Status { state: state.into(), reason: reason.map(str::to_owned), self_started, ..Status::default() }
    }

    fn go_to_place(tracker: &mut Tracker, place: &str, now: Instant) {
        tracker.tool_called("robot.go_to", &json!({"place": place}), &json!({"started": true, "to": format!("`{place}`")}), now);
    }

    #[test]
    fn a_journey_is_said_once_when_it_arrives() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        assert!(tracker.observe(&status("idle", None, false), t0).is_empty());
        go_to_place(&mut tracker, "cucina", t0);
        assert!(tracker.watching());
        assert!(tracker.observe(&status("running", None, false), t0).is_empty());
        assert!(tracker.observe(&status("running", None, false), t0).is_empty());
        let said = tracker.observe(&status("done", Some("arrived at (1.20, -0.40)"), false), t0);
        assert_eq!(said, vec![Event::Arrived(Job::GoToPlace("cucina".into()))]);
        assert_eq!(said[0].phrase(Lang::It), "Sono arrivata in cucina.");
        // The status stays `done` until the next job: nothing more.
        assert!(tracker.observe(&status("done", Some("arrived at (1.20, -0.40)"), false), t0).is_empty());
        assert!(!tracker.watching());
    }

    #[test]
    fn a_point_a_failure_and_a_stop() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.tool_called("robot.go_to", &json!({"x": 1.0, "y": 2.0}), &json!({"started": true}), t0);
        tracker.observe(&status("running", None, false), t0);
        let said = tracker.observe(&status("done", Some("arrived at (1.00, 2.00)"), false), t0);
        assert_eq!(said[0].phrase(Lang::It), "Sono arrivata.");

        go_to_place(&mut tracker, "studio", t0);
        tracker.observe(&status("running", None, false), t0);
        let said = tracker.observe(&status("failed", Some("no way to (3.00, 1.00) on the map"), false), t0);
        assert_eq!(said, vec![Event::Failed(Some(Job::GoToPlace("studio".into())), Why::NoWay)]);
        assert_eq!(said[0].phrase(Lang::It), "Non trovo una strada per studio.");
        assert_eq!(said[0].phrase(Lang::En), "I can't find a way to studio.");

        // A STOP from elsewhere (quack-control): said.
        go_to_place(&mut tracker, "studio", t0);
        tracker.observe(&status("running", None, false), t0);
        let said = tracker.observe(&status("stopped", Some("stopped on request"), false), t0);
        assert_eq!(said, vec![Event::Stopped]);
        assert_eq!(said[0].phrase(Lang::It), "Mi sono fermata.");

        // A STOP the agent itself sent: the agent's reply says it.
        go_to_place(&mut tracker, "studio", t0);
        tracker.observe(&status("running", None, false), t0);
        tracker.tool_called("robot.go_to", &json!({"stop": true}), &json!({"stopped": true}), t0);
        assert!(tracker.observe(&status("stopped", Some("stopped on request"), false), t0).is_empty());
    }

    #[test]
    fn a_job_that_ends_before_the_first_poll_still_counts_but_the_old_ending_does_not() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        let old = status("done", Some("arrived at (0.00, 0.00)"), false);
        tracker.observe(&old, t0);
        go_to_place(&mut tracker, "bagno", t0);
        // The old ending, unchanged: not this job's.
        assert!(tracker.observe(&old, t0).is_empty());
        // A new ending the poll never saw running: this job's.
        let mut failed = status("failed", Some("robotd unreachable: refused"), false);
        failed.elapsed_s = Some(0);
        assert_eq!(
            tracker.observe(&failed, t0),
            vec![Event::Failed(Some(Job::GoToPlace("bagno".into())), Why::Other)]
        );
        // And a job that never shows up is let go, unsaid.
        go_to_place(&mut tracker, "bagno", t0);
        assert!(tracker.observe(&failed, t0 + PENDING_TIMEOUT + Duration::from_secs(1)).is_empty());
        assert!(!tracker.watching());
    }

    #[test]
    fn a_relocalization_before_the_journey_is_said_and_so_is_its_end() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.observe(&status("idle", None, false), t0);
        tracker.tool_called(
            "robot.go_to",
            &json!({"place": "cucina"}),
            &json!({"started": true, "relocalizing": true}),
            t0,
        );
        let finding = status("relocalizing", Some("the duck may have been moved: finding where it is first"), true);
        let said = tracker.observe(&finding, t0);
        assert_eq!(said, vec![Event::OnMyOwn(Motion::Relocalize)]);
        assert_eq!(said[0].phrase(Lang::It), "Prima di partire mi guardo intorno per ritrovarmi.");
        assert!(tracker.observe(&finding, t0).is_empty(), "said once");
        // Confirmed: idle for a moment, then the journey.
        let said = tracker.observe(&status("idle", Some("the pose is confirmed; the job starts"), false), t0);
        assert_eq!(said, vec![Event::Found]);
        assert!(tracker.observe(&status("running", None, false), t0).is_empty());
        let said = tracker.observe(&status("done", Some("arrived at (1.00, 1.00)"), false), t0);
        assert_eq!(said, vec![Event::Arrived(Job::GoToPlace("cucina".into()))]);
    }

    #[test]
    fn a_relocalization_that_fails_is_said_once_with_the_jobs_words() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.observe(&status("idle", None, false), t0);
        tracker.tool_called("robot.go_to", &json!({"x": 1, "y": 1}), &json!({"started": true, "relocalizing": true}), t0);
        tracker.observe(&status("relocalizing", Some("the duck may have been moved: finding where it is first"), true), t0);
        let said = tracker.observe(
            &status("failed", Some("the duck may have been moved and could not find where it is within 180 s; it did not walk toward the goal"), false),
            t0,
        );
        assert_eq!(said, vec![Event::Failed(Some(Job::GoToPoint), Why::Lost)]);
        assert_eq!(said[0].phrase(Lang::It), "Forse mi hanno spostata: non riesco a ritrovarmi.");
    }

    #[test]
    fn the_homecomings_search_is_said_when_it_starts_and_when_it_ends() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        assert!(!tracker.watching());
        let searching = status("searching", Some("the duck woke up somewhere: finding where it is"), true);
        let said = tracker.observe(&searching, t0);
        assert_eq!(said, vec![Event::OnMyOwn(Motion::Search)]);
        assert_eq!(said[0].phrase(Lang::It), "Non sono sicura di dove sono: mi guardo intorno.");
        assert!(tracker.watching());
        assert!(tracker.observe(&searching, t0).is_empty());
        let said = tracker.observe(&status("idle", Some("found where it is"), false), t0);
        assert_eq!(said, vec![Event::Found]);
        assert_eq!(said[0].phrase(Lang::It), "Mi sono ritrovata.");

        // Not found.
        tracker.observe(&searching, t0);
        let said = tracker.observe(&status("idle", Some("the search ended without finding where it is"), false), t0);
        assert_eq!(said, vec![Event::NotFound]);

        // Stopped by the user from elsewhere (stopped_by_user).
        tracker.observe(&searching, t0);
        let said = tracker.observe(&status("stopped", Some("stopped by the user; not searching again until asked"), false), t0);
        assert_eq!(said, vec![Event::Stopped]);
    }

    #[test]
    fn the_homecoming_going_on_exploring_and_its_end() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.observe(&status("searching", Some("finding where it is"), true), t0);
        let said = tracker.observe(&status("running", Some("resuming the exploration"), true), t0);
        assert_eq!(said, vec![Event::OnMyOwn(Motion::Explore)]);
        let mut done = status("done", Some("time budget of 600 s spent"), false);
        done.elapsed_s = Some(600);
        done.percent = Some(72.0);
        let said = tracker.observe(&done, t0);
        assert_eq!(said, vec![Event::Explored { minutes: 10, percent: Some(72) }]);
        assert_eq!(said[0].phrase(Lang::It), "Ho esplorato per 10 minuti, la casa è mappata al 72 per cento.");
    }

    #[test]
    fn an_exploration_ends_mapped_or_on_its_budget() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.observe(&status("idle", None, false), t0);
        tracker.tool_called("robot.map_explore", &json!({}), &json!({"started": true, "map_name": "casa"}), t0);
        tracker.observe(&status("running", None, false), t0);
        let mut done = status("done", Some("no frontier left: 12 submaps"), false);
        done.done = true;
        let said = tracker.observe(&done, t0);
        assert_eq!(said, vec![Event::Mapped]);
        assert_eq!(said[0].phrase(Lang::It), "Ho finito di esplorare: la casa è mappata.");

        tracker.tool_called("robot.map_explore", &json!({"max_s": 60}), &json!({"started": true}), t0);
        tracker.observe(&status("running", None, false), t0);
        let mut budget = status("done", Some("time budget of 60 s spent"), false);
        budget.elapsed_s = Some(64);
        budget.percent = Some(41.0);
        let said = tracker.observe(&budget, t0);
        assert_eq!(said[0].phrase(Lang::It), "Ho esplorato per un minuto, la casa è mappata al 41 per cento.");
        assert_eq!(said[0].phrase(Lang::En), "I explored for a minute; the house is 41 percent mapped.");
    }

    #[test]
    fn a_watch_a_refusal_and_other_tools_start_nothing() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.tool_called("robot.map_explore", &json!({"watch": true}), &json!({"started": true, "watch": true}), t0);
        tracker.tool_called("robot.map_explore", &json!({}), &json!({"started": false, "done": true}), t0);
        tracker.tool_called("robot.where_am_i", &json!({}), &json!({"started": true}), t0);
        assert!(!tracker.watching());
    }

    #[test]
    fn a_restart_under_the_job_says_nothing() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        go_to_place(&mut tracker, "cucina", t0);
        tracker.observe(&status("running", None, false), t0);
        assert!(tracker.observe(&status("idle", None, false), t0).is_empty());
        assert!(!tracker.watching());
    }

    #[test]
    fn the_switches_silence_their_half() {
        let t0 = Instant::now();
        let mut tracker = Tracker::new(false, true);
        go_to_place(&mut tracker, "cucina", t0);
        tracker.observe(&status("running", None, false), t0);
        assert!(tracker.observe(&status("done", Some("arrived at (0, 0)"), false), t0).is_empty());
        let mut tracker = Tracker::new(true, false);
        assert!(tracker.observe(&status("searching", Some("x"), true), t0).is_empty());
        assert!(tracker.observe(&status("idle", Some("found where it is"), false), t0).is_empty());
    }

    #[test]
    fn reasons_map_to_a_brief_why() {
        for (reason, why) in [
            ("no way to (1.00, 2.00) on the map", Why::NoWay),
            ("(1.00, 2.00) is not mapped floor: pick a point the map knows", Why::NotMapped),
            ("the way to (1.00, 2.00) took too long", Why::TooLong),
            ("time budget of 300 s spent", Why::TooLong),
            ("the duck may have been moved and could not find where it is within 180 s", Why::Lost),
            ("the duck could not find its position again", Why::Lost),
            ("position lost: the map and the depth sensor disagreed at 3 stands in a row", Why::Lost),
            ("the pose is not trusted on the way", Why::Lost),
            ("the duck is seated or fallen and did not get up", Why::Fallen),
            ("battery at 20 %: the journey ends here", Why::Battery),
            ("a hole seen 0.30 m from the frontier at (1.00, 1.00)", Why::Drop),
            ("robotd unreachable: refused", Why::Other),
        ] {
            assert_eq!(Why::of(reason), why, "{reason}");
        }
        // A go_to that ran out of time ends `done`, not arrived.
        let t0 = Instant::now();
        let mut tracker = Tracker::new(true, true);
        tracker.tool_called("robot.go_to", &json!({"x": 1, "y": 1}), &json!({"started": true}), t0);
        tracker.observe(&status("running", None, false), t0);
        let said = tracker.observe(&status("done", Some("time budget of 300 s spent"), false), t0);
        assert_eq!(said[0].phrase(Lang::It), "Ci stavo mettendo troppo, mi sono fermata.");
        assert_eq!(said[0].phrase(Lang::En), "It was taking too long, so I stopped.");
    }

    #[test]
    fn languages_resolve_from_the_config_and_the_bridge() {
        assert_eq!(Lang::from_code("it-IT"), Lang::It);
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("de"), Lang::En);
        let mut config: Config = toml::from_str("backend = \"agent\"").unwrap();
        assert_eq!(Lang::resolve(&config, None), Lang::En);
        assert_eq!(Lang::resolve(&config, Some("it")), Lang::It);
        config.direct.stt.language = "en".into();
        assert_eq!(Lang::resolve(&config, Some("it")), Lang::En);
        config.announce.language = "it".into();
        assert_eq!(Lang::resolve(&config, None), Lang::It);
    }

    #[test]
    fn the_queue_says_each_thing_once_and_waits_for_the_backend() {
        let announcer = Announcer::detached();
        let answer = |state: &str, reason: &str, own: bool| {
            json!({"mapping": true, "explore": {"state": state, "reason": reason, "self_started": own}})
        };
        announcer.observe(&answer("idle", "", false));
        announcer.tool_called("robot.go_to", &json!({"place": "cucina"}), &json!({"started": true}));
        announcer.observe(&answer("running", "", false));
        // The turn is still on: nothing taken, the sentence waits.
        announcer.observe(&answer("done", "arrived at (1.00, 1.00)", false));
        announcer.observe(&answer("done", "arrived at (1.00, 1.00)", false));
        assert_eq!(announcer.next(), Some(Event::Arrived(Job::GoToPlace("cucina".into()))));
        assert_eq!(announcer.next(), None);
        // The same sentence pushed again at once is not repeated.
        announcer.push(Event::Stopped);
        announcer.push(Event::Stopped);
        assert_eq!(announcer.next(), Some(Event::Stopped));
        assert_eq!(announcer.next(), None);
    }
}
