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
use crate::robotd::Control;
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
/// A mapping step must end at least this far from a mapped wall ahead —
/// the sensor's blind band is 10 cm and the gait wanders.
/// `QK_WALL_MARGIN_M`: 0.18 since 2026-09-16 (was 0.25) (the user's rule: shrink the
/// margins, the refusals must be nearly none) — the flank 8 cm from the
/// wall at the leg's end, and the leg is shortened before it is refused.
pub struct Robot {
    /// The request lane; `None` while robotd is unreachable.
    pub control: Option<Control>,
    /// The `[gait]` section: yaw trim and per-side gains for every walk.
    pub gait: crate::config::GaitConfig,
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
        let control = match Control::connect(&config.robotd_socket) {
            Ok(control) => Some(control),
            Err(e) => {
                tracing::warn!(error = %e, "robotd unreachable — running without the robot");
                None
            }
        };
        Self {
            control,
            gait: config.gait.clone(),
            nav: NavLane::probe(&config.nav),
        }
    }

    /// No robotd, no navigation (tests, dry runs).
    pub fn detached() -> Self {
        Self { control: None, gait: crate::config::GaitConfig::default(), nav: None }
    }
}

/// The catalog announced in `session.start`. JSON-Schema parameters,
/// directly projectable to OpenAI tools and MCP listings.
pub fn catalog(nav: Option<&NavLane>) -> Value {
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
        toggles), roulade does a somersault.",
            "parameters": {
                "type": "object",
                "properties": {"name": {"type": "string", "enum": ["ground_pick", "kick_left", "kick_right", "sit_toggle", "roulade"]}},
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
    if let Some(nav) = nav {
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
    let control = &mut robot.control;
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
            let skill: proto::Skill = serde_json::from_value(json!(skill_name))
                .map_err(|_| format!("unknown skill `{skill_name}`"))?;
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


    #[test]
    fn catalog_matches_the_executor_allowlist() {
        // Without a navigation daemon the satellite announces its own
        // tools and nothing else; with one, the daemon's catalog is
        // spliced in (see `crate::nav_client`).
        let catalog = catalog(None);
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
