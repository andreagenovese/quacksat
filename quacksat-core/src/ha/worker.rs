//! The thread that touches the robot for Home Assistant: every command
//! becomes one call of the tool table (ADR 0007 §3), and the state the
//! sensors read is gathered here. It holds lanes of its own, so a walk
//! pressed from a phone never waits on the voice loop, and the voice
//! loop never waits on it; the one-walk rule lives in `body`.

use serde_json::{Value, json};

use crate::announce::{Lang, Status};
use crate::body::MAX_MOVE_DURATION_S;
use crate::config::Config;
use crate::nav_client::NavLane;
use crate::tools::{self, Robot};

use super::discovery::Lists;

/// The speed and turn rate of one press: what `robot.move` documents
/// for a walk the gait will start, and a 90° turn in about two seconds.
const WALK_VX: f64 = 0.3;
const TURN_VYAW: f64 = 0.7;

/// What the sensors and the discovery payload are built from.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub state: Value,
    pub robot_online: bool,
    pub lists: Lists,
    /// A walk or a journey is on: the state is read more often.
    pub moving: bool,
}

pub struct Worker {
    robot: Robot,
    config: Config,
    lang: Lang,
    head: [f64; 3],
    polls: u32,
    /// The places as last read. A read that fails — the daemon
    /// restarting, a slow answer — keeps them: dropping them would
    /// remove the buttons from Home Assistant, and with them whatever
    /// the user customized, only to add them back ten seconds later.
    last_places: Option<Vec<String>>,
}

impl Worker {
    pub fn new(config: &Config, announcer: Option<crate::announce::Announcer>) -> Self {
        let mut robot = Robot::connect(config);
        // A journey sent from a button is followed like one sent by voice.
        robot.announcer = announcer;
        Worker { robot, config: config.clone(), lang: Lang::resolve(config, None), head: [0.0; 3], polls: 0, last_places: None }
    }

    /// Run one command; the answer goes to its result topic as is.
    pub fn command(&mut self, name: &str, payload: &str) -> Value {
        match self.run(name, payload.trim()) {
            Ok(mut answer) => {
                answer["ok"] = json!(true);
                answer
            }
            Err(detail) => json!({"ok": false, "error": self.say(&detail), "detail": detail}),
        }
    }

    fn run(&mut self, name: &str, payload: &str) -> Result<Value, String> {
        let step = self.config.mqtt.step_s.clamp(0.1, MAX_MOVE_DURATION_S);
        let turn = self.config.mqtt.turn_s.clamp(0.1, MAX_MOVE_DURATION_S);
        match name {
            "forward" => self.walk(json!({"vx": WALK_VX, "duration_s": step})),
            "turn_left" => self.walk(json!({"vx": WALK_VX, "vyaw": TURN_VYAW, "duration_s": turn})),
            "turn_right" => self.walk(json!({"vx": WALK_VX, "vyaw": -TURN_VYAW, "duration_s": turn})),
            "head_pitch" | "head_yaw" | "head_roll" => {
                let value: f64 = payload.parse().map_err(|_| format!("not a number: `{payload}`"))?;
                let index = ["head_pitch", "head_yaw", "head_roll"].iter().position(|k| *k == name).unwrap_or(0);
                self.head[index] = value;
                let [pitch, yaw, roll] = self.head;
                tools::execute("robot.head", &json!({"pitch": pitch, "yaw": yaw, "roll": roll}), &mut self.robot)
            }
            "head_center" => {
                self.head = [0.0; 3];
                tools::execute("robot.head", &json!({}), &mut self.robot)
            }
            "skill" => tools::execute("robot.skill", &json!({"name": payload}), &mut self.robot),
            "sound" => tools::execute("robot.sound", &json!({"tag": payload}), &mut self.robot),
            "go_to" => self.go_to(payload),
            other => Err(format!("unknown command `{other}`")),
        }
    }

    /// A press is one bounded walk, never during a journey: the journey
    /// is the navigation's, and two drivers on one pair of legs is the
    /// thing ADR 0006 §4 forbids.
    fn walk(&mut self, args: Value) -> Result<Value, String> {
        if self.journey().is_some_and(|status| status.moving()) {
            return Err("a journey is running".into());
        }
        tools::execute("robot.move", &args, &mut self.robot)
    }

    fn go_to(&mut self, spoken: &str) -> Result<Value, String> {
        let places = self.places().ok_or("no navigation daemon")?;
        let Some(place) = match_place(spoken, &places) else {
            return Err(format!("no place called {spoken}"));
        };
        let answer = tools::execute("robot.go_to", &json!({"place": place}), &mut self.robot)?;
        Ok(json!({"place": place, "answer": answer}))
    }

    /// Read everything the sensors show. Cheap enough for every 2 s
    /// while the duck moves: a few lines over two unix sockets.
    pub fn poll(&mut self) -> Snapshot {
        self.polls += 1;
        // A daemon that started after us is looked for again, now and
        // then, instead of never.
        if self.robot.nav.is_none() && self.polls % 6 == 1 {
            self.robot.nav = NavLane::probe(&self.config.nav);
        }
        let health = tools::execute("robot.state", &json!({}), &mut self.robot);
        let robot_online = health.is_ok();
        let health = health.unwrap_or(Value::Null);
        let mut state = json!({
            "healthy": health.get("healthy").cloned().unwrap_or(Value::Null),
            "reason": health.get("reason").cloned().unwrap_or(Value::Null),
            "battery": health.pointer("/battery/percent").and_then(Value::as_f64).map_or(Value::Null, |p| json!(p.round())),
            "volts": health.pointer("/battery/volts").cloned().unwrap_or(Value::Null),
            "mode": health.get("mode").cloned().unwrap_or(Value::Null),
            "walking": crate::body::walking(),
        });
        if let Some(places) = self.places() {
            self.last_places = Some(places);
        }
        let places = self.last_places.clone();
        let journey = self.journey();
        if self.robot.nav.is_some() {
            let here = self.robot.nav.as_mut().and_then(|nav| nav.call("robot.where_am_i", &json!({})).ok());
            state["place"] = here.as_ref().and_then(|h| h.get("place")).cloned().unwrap_or(Value::Null);
            state["at_place"] = here.as_ref().and_then(|h| h.get("at_place")).cloned().unwrap_or(Value::Null);
            state["journey"] = journey.as_ref().map_or(Value::Null, |s| json!(s.state));
            state["journey_reason"] = journey.as_ref().and_then(|s| s.reason.clone()).map_or(Value::Null, Value::from);
        }
        Snapshot {
            moving: crate::body::walking() || journey.as_ref().is_some_and(Status::moving),
            state,
            robot_online,
            lists: Lists { skills: self.robot.skills.clone(), places },
        }
    }

    fn journey(&mut self) -> Option<Status> {
        let answer = self.robot.nav.as_mut()?.call("robot.map_status", &json!({})).ok()?;
        Status::from_map_status(&answer)
    }

    fn places(&mut self) -> Option<Vec<String>> {
        let nav = self.robot.nav.as_mut()?;
        let answer = nav.call("robot.list_places", &json!({})).ok()?;
        Some(
            answer
                .get("places")
                .and_then(Value::as_array)
                .map(|places| places.iter().filter_map(|p| p.get("name").and_then(Value::as_str)).map(str::to_owned).collect())
                .unwrap_or_default(),
        )
    }

    /// The error as Home Assistant will speak it: the example
    /// automations read `error` aloud, so the ones a person meets are in
    /// their language. `detail` keeps the original.
    fn say(&self, detail: &str) -> String {
        let it = self.lang == Lang::It;
        let pick = |a: &str, b: &str| if it { a.to_owned() } else { b.to_owned() };
        if detail == "already walking" {
            pick("sto già camminando", "I'm already walking")
        } else if detail == "a journey is running" {
            pick("sto già andando da qualche parte", "I'm already on my way somewhere")
        } else if detail == "no navigation daemon" {
            pick("non so ancora andare nei posti", "I can't go to places yet")
        } else if let Some(place) = detail.strip_prefix("no place called ") {
            if it { format!("non conosco nessun posto chiamato {place}") } else { format!("I don't know any place called {place}") }
        } else if detail.starts_with("robot unreachable") || detail.starts_with("robot lost") {
            pick("il robot non risponde", "the robot isn't answering")
        } else {
            detail.to_owned()
        }
    }
}

/// The place a person named, among the ones the duck knows: case,
/// spaces and a leading article do not count ("la Cucina" is "cucina").
pub fn match_place(spoken: &str, places: &[String]) -> Option<String> {
    let wanted = normalize(spoken);
    if wanted.is_empty() {
        return None;
    }
    places.iter().find(|place| normalize(place) == wanted).cloned()
}

fn normalize(name: &str) -> String {
    let lower = name.trim().trim_end_matches(['.', '!', '?', ',']).to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let mut text = words.join(" ");
    for article in ["il ", "lo ", "la ", "i ", "gli ", "le ", "the ", "l'", "l’"] {
        if let Some(rest) = text.strip_prefix(article) {
            text = rest.trim_start().to_owned();
            break;
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places() -> Vec<String> {
        vec!["cucina".into(), "Camera da letto".into(), "ingresso".into()]
    }

    #[test]
    fn a_place_matches_without_case_article_or_punctuation() {
        assert_eq!(match_place("Cucina", &places()).as_deref(), Some("cucina"));
        assert_eq!(match_place("la camera  da letto.", &places()).as_deref(), Some("Camera da letto"));
        assert_eq!(match_place("l'ingresso", &places()).as_deref(), Some("ingresso"));
        assert_eq!(match_place("garage", &places()), None);
        assert_eq!(match_place("  ", &places()), None);
    }

    #[test]
    fn errors_are_said_in_the_duck_s_language() {
        let mut config: Config = toml::from_str("backend = \"none\"\n[nav]\nenabled = false").unwrap();
        config.robotd_socket = "/nonexistent/robotd.sock".into();
        config.announce.language = "it".into();
        let worker = Worker::new(&config, None);
        assert_eq!(worker.say("no place called garage"), "non conosco nessun posto chiamato garage");
        assert_eq!(worker.say("already walking"), "sto già camminando");
        assert_eq!(worker.say("unknown skill `fly`"), "unknown skill `fly`");
    }

    #[test]
    fn without_a_robot_a_command_says_so_and_go_to_needs_the_navigation() {
        let mut config: Config = toml::from_str("backend = \"none\"\n[nav]\nenabled = false").unwrap();
        config.robotd_socket = "/nonexistent/robotd.sock".into();
        let mut worker = Worker::new(&config, None);
        let answer = worker.command("forward", "PRESS");
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["detail"], "robot unreachable");
        let answer = worker.command("go_to", "cucina");
        assert_eq!(answer["detail"], "no navigation daemon");
        let answer = worker.command("head_yaw", "left");
        assert_eq!(answer["detail"], "not a number: `left`");
        let answer = worker.command("fly", "");
        assert_eq!(answer["detail"], "unknown command `fly`");
        let snapshot = worker.poll();
        assert!(!snapshot.robot_online);
        assert!(snapshot.lists.places.is_none());
    }
}
