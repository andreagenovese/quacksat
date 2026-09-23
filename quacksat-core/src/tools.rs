//! The robot tool surface (ADR 0004 §4, protocol spec "Tool surface v1"):
//! declared in session.start, executed here behind an exhaustive
//! allowlist with satellite-side clamps. A new tool does not exist on the
//! wire until it is added to BOTH `catalog()` and `execute()`.
//!
//! Tools act on a [`Robot`]: the robotd request lane, and — when the
//! navigation daemon is listening on its socket — the nav lane, whose
//! catalog is spliced here
//! and routed back to it by name.

use crate::config::Config;
use crate::body::{clamp, move_params, notify, number, request, require_str, timed_move, trimmed, with_robot};
use crate::nav_client::NavLane;
use crate::robotd::{Control, Lane};
use duck_ipc_proto as proto;
use serde_json::{Value, json};

/// Hard caps an LLM can never exceed, whatever it asks for. The speed
/// and yaw caps live with the body's lane in [`crate::body`], which is
/// what actually builds the wire command.
const MAX_MOVE_DURATION_S: f64 = 3.0;
const MAX_LOOK_XY_M: f64 = 3.0;
const MIN_LOOK_Z_M: f64 = -0.2;
const MAX_LOOK_Z_M: f64 = 2.0;
const MAX_HEAD_PITCH_RAD: f64 = 0.6;
const MAX_HEAD_YAW_RAD: f64 = 1.2;
const MAX_HEAD_ROLL_RAD: f64 = 0.5;

/// What `robot.do` answered to before daemon 0.14 made the skill table
/// config, and what a stock robot still answers to. The fallback when the
/// robot does not say: an older daemon refuses `robot.skills` with
/// METHOD_NOT_FOUND, and an unreachable one says nothing at all.
pub const STOCK_SKILLS: [&str; 5] = [
    "ground_pick",
    "kick_left",
    "kick_right",
    "sit_toggle",
    "roulade",
];

fn stock_skills() -> Vec<String> {
    STOCK_SKILLS.iter().map(|name| (*name).to_string()).collect()
}

/// Ask the robot what it can do, once, at connect time.
///
/// Since daemon 0.14 (API v22) a skill is a `[[policy.skill]]` entry, so
/// which ones exist is this robot's business and not ours to assume —
/// `proto::Skill` stopped being an enum for the same reason. The answer
/// carries the configured table and the two the daemon drives itself
/// (`ground_pick`, `sit_toggle`); an agent choosing a skill needs both.
fn skills_of(control: &mut Option<Control>) -> Vec<String> {
    let reported = request(control, &proto::Call::RobotSkills)
        .ok()
        .filter(|response| response.error.is_none())
        .and_then(|response| response.result_as::<proto::SkillsResult>().ok())
        .map(|result| {
            result
                .skills
                .into_iter()
                .map(|skill| skill.name)
                .chain(result.built_in)
                .collect::<Vec<String>>()
        })
        .filter(|names| !names.is_empty());
    match reported {
        Some(names) => {
            tracing::info!(skills = ?names, "the robot listed its skills");
            names
        }
        None => {
            tracing::info!("the robot did not list its skills — assuming the stock five");
            stock_skills()
        }
    }
}

pub struct Robot {
    /// The request lane: discrete intents and queries, dialled again
    /// when it dies (`Lane`).
    pub lane: Lane,
    /// The `[gait]` section: yaw trim and per-side gains for every walk.
    pub gait: crate::config::GaitConfig,
    /// What `robot.skill` may be asked for: the robot's own list, or the
    /// stock five when it does not say. See [`skills_of`].
    pub skills: Vec<String>,
    /// The navigation daemon, when one answers on its socket: its tools
    /// are announced beside the satellite's and executed there
    /// (`quack-navd`, the split of 2026-09-22).
    pub nav: Option<NavLane>,
}

impl Robot {
    /// Connect every lane the config asks for. Nothing here is fatal: a
    /// missing robotd or navigation daemon degrades to "the tool says
    /// so" at call time.
    pub fn connect(config: &Config) -> Self {
        let mut lane = Lane::connect(&config.robotd_socket);
        let skills = skills_of(&mut lane.control);
        Self {
            lane,
            gait: config.gait.clone(),
            skills,
            nav: NavLane::probe(&config.nav),
        }
    }

    /// Give the lane its chance to come back, and re-read the skill
    /// list when it does: a robot that restarted under us — which is
    /// what an update looks like from here — may not have the same one.
    pub fn redial(&mut self) {
        if !self.lane.redial() {
            return;
        }
        let skills = skills_of(&mut self.lane.control);
        if skills != self.skills {
            tracing::info!(?skills, "the robot came back with a different skill list");
        }
        self.skills = skills;
    }

    /// No robotd, no navigation (tests, dry runs).
    pub fn detached() -> Self {
        Self {
            lane: Lane::detached(),
            gait: crate::config::GaitConfig::default(),
            skills: stock_skills(),
            nav: None,
        }
    }
}

/// The catalog announced in `session.start`. JSON-Schema parameters,
/// directly projectable to OpenAI tools and MCP listings.
pub fn catalog(robot: &Robot) -> Value {
    let mut tools = vec![
        json!({
            "name": "robot.sound",
            "description": "Play an expressive duck sound. Use for reactions or when asked to \
        quack or make a sound. Tags: alarm (loud alert), greet (hello), inquire (questioning), \
        peck, chirp (short acknowledgement), coo (affectionate).",
            "parameters": {
                "type": "object",
                "properties": {"tag": {"type": "string", "enum": ["alarm", "greet", "inquire", "peck", "chirp", "coo"]}},
                "required": ["tag"]
            }
        }),
        json!({
            "name": "robot.look",
            "description": "Aim the duck's gaze at a point in space. Use when asked to look at \
        something or somewhere. Coordinates in meters from the duck's chest: x forward, y left, \
        z up (the floor is about 0.12 m below; a standing person's face is around z=1.5 at their \
        distance). The gaze holds until changed. Example: look at something on the floor one \
        meter ahead: x=1.0, z=-0.1.",
            "parameters": {
                "type": "object",
                "properties": {
                    "x": {"type": "number", "description": "meters forward of the duck"},
                    "y": {"type": "number", "description": "meters to the duck's left (negative = right)"},
                    "z": {"type": "number", "description": "meters above the duck's chest (floor is -0.12)"}
                },
                "required": ["x"]
            }
        }),
        json!({
            "name": "robot.head",
            "description": "Strike an expressive head pose (for looking AT something use \
        robot.look instead): roll tilts the head sideways like a curious dog, yaw turns it, \
        pitch nods it. Angles in radians, clamped; omitted angles return to center. The pose \
        holds until the next call; call with no arguments to re-center.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pitch": {"type": "number", "description": "nod up/down, about -0.6 to 0.6"},
                    "yaw": {"type": "number", "description": "turn left/right, about -1.2 to 1.2, + is left"},
                    "roll": {"type": "number", "description": "sideways tilt, about -0.5 to 0.5"}
                }
            }
        }),
        json!({
            "name": "robot.skill",
            "description": "Run a one-shot skill; it takes a few seconds. ground_pick pecks at \
        the ground, kick_left/kick_right kick, sit_toggle sits down or stands back up (it \
        toggles), roulade does a somersault. The list below is this robot's own — since daemon \
        0.14 its skills are configurable, so a name that is not there does not exist here.",
            "parameters": {
                "type": "object",
                "properties": {"name": {"type": "string", "enum": &robot.skills}},
                "required": ["name"]
            }
        }),
        json!({
            "name": "robot.move",
            "description": "Walk or turn for a bounded time, then stop automatically. Use when \
        asked to move, approach, back away, or turn. Command vx=0.3 to walk: the gait does not \
        start below about 0.25 m/s, and 0.3 commanded moves the duck roughly 10 cm per second, \
        so vx=0.3 with duration_s=3 walks about 30 cm. The duck cannot turn in place: to turn, \
        walk with vx=0.3 and vyaw=0.7 (left) or -0.7 (right) for about 2 s per 90 degrees, \
        moving 15-20 cm meanwhile. Nothing watches where this walk goes — the cliff guard rides \
        with the navigation's own journeys, not with this tool — so keep it short and look first. \
        Values are clamped (0.3 m/s, 1.0 rad/s, 3 s max). For longer distances \
        call repeatedly, checking robot.state in between.",
            "parameters": {
                "type": "object",
                "properties": {
                    "vx": {"type": "number", "description": "m/s forward (+) / backward (-)"},
                    "vy": {"type": "number", "description": "m/s sidestep left (+) / right (-)"},
                    "vyaw": {"type": "number", "description": "rad/s turn, + is left/counterclockwise"},
                    "duration_s": {"type": "number", "minimum": 0.1, "maximum": MAX_MOVE_DURATION_S}
                },
                "required": ["duration_s"]
            }
        }),
        json!({
            "name": "robot.state",
            "description": "Current robot status: health, battery, mode. Use it before and \
        after moving, or when asked how the robot is doing.",
            "parameters": {"type": "object", "properties": {}}
        }),
    ];
    tools.push(json!({
        "name": "robot.get_frame",
        "description": "Grab a camera frame. Not supported yet on this robot — if it \
    fails, tell the user you cannot see yet.",
        "parameters": {"type": "object", "properties": {}}
    }));
    if let Some(nav) = robot.nav.as_ref() {
        // The navigation daemon's own tools, announced as if they were
        // ours: the agent sees one robot, `execute` routes by name.
        tools.extend(nav.catalog());
    }
    Value::Array(tools)
}

/// Run one tool call. Every name here is in `catalog()`, or belongs to
/// the navigation daemon.
pub fn execute(name: &str, args: &Value, robot: &mut Robot) -> Result<Value, String> {
    // Anything the navigation daemon answers for goes there: the map,
    // the places, the steps and the journeys live in `quack-navd` since
    // the split of 2026-09-22, and the satellite only carries the wire.
    if let Some(nav) = &mut robot.nav
        && nav.handles(name)
    {
        return nav.call(name, args);
    }
    // Everything below needs the robot, so this is where a lane that
    // died gets one attempt to come back.
    robot.redial();
    let control = &mut robot.lane.control;
    match name {
        "robot.sound" => {
            let tag = require_str(args, "tag")?;
            let tag: proto::SoundTag = serde_json::from_value(json!(tag))
                .map_err(|_| format!("unknown sound tag `{tag}`"))?;
            if tag == proto::SoundTag::Wheee {
                // A held ride makes no sense as a one-shot LLM tool.
                return Err("unknown sound tag `wheee`".to_string());
            }
            let result = intent(
                control,
                &proto::Call::RobotSound(proto::SoundParams { tag, hold: None }),
            )?;
            intent_outcome(result)
        }
        "robot.look" => {
            let params = proto::LookParams {
                x: number(args, "x").clamp(-MAX_LOOK_XY_M, MAX_LOOK_XY_M),
                y: number(args, "y").clamp(-MAX_LOOK_XY_M, MAX_LOOK_XY_M),
                z: number(args, "z").clamp(MIN_LOOK_Z_M, MAX_LOOK_Z_M),
                neck_pitch: 0.0,
            };
            let response = request(control, &proto::Call::RobotLook(params))?;
            if let Some(error) = &response.error {
                return Err(format!("robot refused look: {error}"));
            }
            let clamped = response
                .result_as::<proto::LookResult>()
                .map(|r| r.clamped)
                .unwrap_or(false);
            // `clamped` tells the agent the point is beyond the head's
            // reach — the gaze is the closest approximation, not a lock.
            Ok(json!({"done": true, "clamped": clamped}))
        }
        "robot.head" => {
            let params = proto::HeadParams {
                neck_pitch: 0.0,
                head_pitch: clamp(number(args, "pitch"), MAX_HEAD_PITCH_RAD),
                head_yaw: clamp(number(args, "yaw"), MAX_HEAD_YAW_RAD),
                head_roll: clamp(number(args, "roll"), MAX_HEAD_ROLL_RAD),
            };
            // Head is a persistent slot in robotd and deliberately not
            // deadmanned: one notification is enough.
            notify(control, &proto::Call::RobotHead(params))?;
            Ok(json!({"done": true}))
        }
        "robot.skill" => {
            let skill_name = require_str(args, "name")?;
            // Checked against what this robot answered `robot.skills`
            // with, not against a list compiled in: `proto::Skill` is a
            // plain name since daemon 0.14, so nothing on the wire
            // refuses a typo before it reaches the robot any more.
            if !robot.skills.iter().any(|skill| skill == skill_name) {
                return Err(format!("unknown skill `{skill_name}`"));
            }
            let skill: proto::Skill = skill_name.to_string();
            let result = intent(control, &proto::Call::RobotDo(proto::DoParams { skill }))?;
            intent_outcome(result)
        }
        "robot.move" => {
            let duration = args
                .get("duration_s")
                .and_then(Value::as_f64)
                .ok_or("duration_s is required")?
                .clamp(0.1, MAX_MOVE_DURATION_S);
            let params = trimmed(&robot.gait, move_params(args));
            // No heading hold here: the yaw it closes on is the cliff
            // guard's, and the guard belongs to the navigation daemon.
            timed_move(control, params, duration)?;
            Ok(json!({"done": true, "walked_s": duration}))
        }
        "robot.state" => {
            let health = request(control, &proto::Call::RobotHealth)?;
            let mode = request(control, &proto::Call::RobotMode)
                .ok()
                .and_then(|r| r.result_as::<proto::ModeResult>().ok())
                .map(|m| m.mode);
            let health: Value = health.result.unwrap_or(Value::Null);
            Ok(json!({
                "healthy": health.get("healthy"),
                "reason": health.get("reason"),
                "battery": health.get("battery"),
                "mode": mode,
            }))
        }
        "robot.get_frame" => Err("unsupported".to_string()),
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// Start or stop the "map everything" job.
fn intent(
    control: &mut Option<Control>,
    call: &proto::Call,
) -> Result<proto::IntentResult, String> {
    let robot = with_robot(control)?;
    robot.intent(call).map_err(|e| {
        *control = None;
        format!("robot lost: {e}")
    })
}

fn intent_outcome(result: proto::IntentResult) -> Result<Value, String> {
    if result.accepted {
        Ok(json!({"done": true}))
    } else {
        Err(result
            .reason
            .unwrap_or_else(|| "refused by the robot".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_and_unsupported_are_soft_errors() {
        let mut robot = Robot::detached();
        assert_eq!(
            execute("robot.fly", &json!({}), &mut robot),
            Err("unknown tool `robot.fly`".to_string())
        );
        assert_eq!(
            execute("robot.get_frame", &json!({}), &mut robot),
            Err("unsupported".to_string())
        );
    }

    #[test]
    fn robot_tools_without_a_robot_say_so() {
        let mut robot = Robot::detached();
        assert_eq!(
            execute("robot.sound", &json!({"tag": "chirp"}), &mut robot),
            Err("robot unreachable".to_string())
        );
        assert_eq!(
            execute("robot.move", &json!({"duration_s": 1.0}), &mut robot),
            Err("robot unreachable".to_string())
        );
    }

    #[test]
    fn bad_arguments_are_rejected_before_touching_the_robot() {
        let mut robot = Robot::detached();
        assert_eq!(
            execute("robot.sound", &json!({"tag": "explosion"}), &mut robot),
            Err("unknown sound tag `explosion`".to_string())
        );
        assert_eq!(
            execute("robot.sound", &json!({"tag": "wheee"}), &mut robot),
            Err("unknown sound tag `wheee`".to_string())
        );
        assert_eq!(
            execute("robot.skill", &json!({"name": "backflip"}), &mut robot),
            Err("unknown skill `backflip`".to_string())
        );
        assert_eq!(
            execute("robot.move", &json!({}), &mut robot),
            Err("duration_s is required".to_string())
        );
    }
    // The map, the places and the steps are the navigation daemon's:
    // their tests moved to `quack-nav` with the code (2026-09-22).

    /// One fake robotd that answers a single `robot.skills` request with
    /// `answer`, so the two skew cases below differ only in that answer.
    fn skills_against(answer: fn(Option<proto::Id>) -> proto::Response) -> Vec<String> {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("robotd.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: proto::Request = serde_json::from_str(&line).unwrap();
            assert_eq!(request.method, "robot.skills");
            let mut out = serde_json::to_vec(&answer(request.id)).unwrap();
            out.push(b'\n');
            let mut writer = stream;
            writer.write_all(&out).unwrap();
            writer.flush().unwrap();
        });

        let mut control = Some(Control::connect(socket.to_str().unwrap()).unwrap());
        let skills = skills_of(&mut control);
        server.join().unwrap();
        skills
    }

    #[test]
    fn the_skills_are_the_robots_own_table_plus_its_built_ins() {
        let skills = skills_against(|id| {
            proto::Response::ok(
                id,
                &proto::SkillsResult {
                    skills: vec![proto::SkillParams {
                        name: "bow".to_string(),
                        ..Default::default()
                    }],
                    built_in: vec!["sit_toggle".to_string()],
                },
            )
        });
        // A robot with a configured `bow` and nothing else has a bow and a
        // sit toggle — not the five this satellite used to assume.
        assert_eq!(skills, vec!["bow".to_string(), "sit_toggle".to_string()]);
    }

    #[test]
    fn a_daemon_that_does_not_know_robot_skills_leaves_the_stock_five() {
        let skills = skills_against(|id| {
            proto::Response::err(
                id,
                proto::Error::new(-32601, "method not found: robot.skills"),
            )
        });
        assert_eq!(skills, stock_skills());
    }

    #[test]
    fn a_skill_the_robot_never_listed_is_refused_before_the_wire() {
        let mut robot = Robot::detached();
        robot.skills = vec!["bow".to_string()];
        // `proto::Skill` is a plain string since daemon 0.14, so this is
        // the only place left that can tell a typo from a skill.
        assert_eq!(
            execute("robot.skill", &json!({"name": "roulade"}), &mut robot),
            Err("unknown skill `roulade`".to_string())
        );
        // One the robot did list gets as far as the missing lane.
        assert_eq!(
            execute("robot.skill", &json!({"name": "bow"}), &mut robot),
            Err("robot unreachable".to_string())
        );
        // And the agent is told the same list the executor enforces.
        let catalog = catalog(&robot);
        let skill = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "robot.skill")
            .unwrap();
        assert_eq!(skill["parameters"]["properties"]["name"]["enum"], json!(["bow"]));
    }

    #[test]
    fn a_lane_that_died_is_dialled_again_with_the_list_the_robot_has_now() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::{UnixListener, UnixStream};

        fn read_request(reader: &mut BufReader<UnixStream>) -> proto::Request {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            serde_json::from_str(&line).unwrap()
        }
        fn answer(stream: &mut UnixStream, response: &proto::Response) {
            let mut out = serde_json::to_vec(response).unwrap();
            out.push(b'\n');
            stream.write_all(&out).unwrap();
            stream.flush().unwrap();
        }
        fn listing(name: &str) -> proto::SkillsResult {
            proto::SkillsResult {
                skills: vec![proto::SkillParams {
                    name: name.to_string(),
                    ..Default::default()
                }],
                built_in: Vec::new(),
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("robotd.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            // First life: it can bow. Then it goes away mid-session,
            // which is what an update doing `systemctl restart robotd`
            // looks like from here.
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let request = read_request(&mut reader);
            assert_eq!(request.method, "robot.skills");
            answer(&mut stream, &proto::Response::ok(request.id, &listing("bow")));
            drop(reader);
            drop(stream);

            // Second life, same socket, another skill table — and the
            // `robot.do` that the satellite could only send because it
            // dialled again.
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let request = read_request(&mut reader);
            assert_eq!(request.method, "robot.skills");
            answer(
                &mut stream,
                &proto::Response::ok(request.id, &listing("roulade")),
            );
            let request = read_request(&mut reader);
            assert_eq!(request.method, "robot.do");
            answer(
                &mut stream,
                &proto::Response::ok(request.id, &proto::IntentResult::accepted()),
            );
        });

        let config: Config = toml::from_str(&format!(
            "backend = \"direct\"\nrobotd_socket = \"{}\"\n[nav]\nenabled = false\n",
            socket.display()
        ))
        .unwrap();
        let mut robot = Robot::connect(&config);
        assert_eq!(robot.skills, vec!["bow".to_string()]);

        // The robot is gone: the call that finds out loses the lane.
        assert!(execute("robot.skill", &json!({"name": "bow"}), &mut robot).is_err());
        assert!(robot.lane.control.is_none());

        // The next one dials again and asks what this robot can do now,
        // so a skill that did not exist a second ago goes through.
        assert_eq!(
            execute("robot.skill", &json!({"name": "roulade"}), &mut robot),
            Ok(json!({"done": true}))
        );
        assert_eq!(robot.skills, vec!["roulade".to_string()]);
        // And the one it used to have is refused, rather than sent to a
        // robot that would not know it.
        assert_eq!(
            execute("robot.skill", &json!({"name": "bow"}), &mut robot),
            Err("unknown skill `bow`".to_string())
        );
        server.join().unwrap();
    }

    #[test]
    fn catalog_matches_the_executor_allowlist() {
        // Without a navigation daemon the satellite announces its own
        // tools and nothing else; with one, the daemon's catalog is
        // spliced in (see `crate::nav_client`).
        let catalog = catalog(&Robot::detached());
        let names: Vec<&str> = catalog
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "robot.sound",
                "robot.look",
                "robot.head",
                "robot.skill",
                "robot.move",
                "robot.state",
                "robot.get_frame",
            ]
        );
        let mut robot = Robot::detached();
        for name in names {
            let err = execute(name, &json!({}), &mut robot).err().unwrap_or_default();
            assert!(!err.starts_with("unknown tool"), "{name}: {err}");
        }
        assert!(execute("robot.go_to", &json!({}), &mut robot).unwrap_err().starts_with("unknown tool"));

    }
}
