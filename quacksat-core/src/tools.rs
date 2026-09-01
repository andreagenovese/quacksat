//! The robot tool surface (ADR 0004 §4, protocol spec "Tool surface v1"):
//! declared in session.start, executed here behind an exhaustive
//! allowlist with satellite-side clamps. A new tool does not exist on the
//! wire until it is added to BOTH `catalog()` and `execute()`.

use std::time::{Duration, Instant};

use crate::robotd::Control;
use duck_ipc_proto as proto;
use serde_json::{Value, json};

/// Hard caps an LLM can never exceed, whatever it asks for.
const MAX_MOVE_DURATION_S: f64 = 3.0;
const MAX_SPEED_M_S: f64 = 0.2;
const MAX_YAW_RAD_S: f64 = 1.0;
const MAX_HEAD_PITCH_RAD: f64 = 0.6;
const MAX_HEAD_YAW_RAD: f64 = 1.2;
const MAX_HEAD_ROLL_RAD: f64 = 0.5;
/// Intent cadence while a timed move runs (well inside the 500 ms deadman).
const MOVE_TICK: Duration = Duration::from_millis(40);

/// The catalog announced in `session.start`. JSON-Schema parameters,
/// directly projectable to OpenAI tools and MCP listings.
pub fn catalog() -> Value {
    json!([
        {
            "name": "robot.sound",
            "description": "Play an expressive duck sound. Tags: alarm, greet, inquire, peck, chirp, coo.",
            "parameters": {
                "type": "object",
                "properties": {"tag": {"type": "string", "enum": ["alarm", "greet", "inquire", "peck", "chirp", "coo"]}},
                "required": ["tag"]
            }
        },
        {
            "name": "robot.head",
            "description": "Point the duck's head. Angles in radians, clamped satellite-side.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pitch": {"type": "number", "description": "up/down, + looks down"},
                    "yaw": {"type": "number", "description": "left/right, + looks left"},
                    "roll": {"type": "number", "description": "head tilt"}
                }
            }
        },
        {
            "name": "robot.skill",
            "description": "Run a one-shot skill.",
            "parameters": {
                "type": "object",
                "properties": {"name": {"type": "string", "enum": ["ground_pick", "kick_left", "kick_right", "sit_toggle", "roulade"]}},
                "required": ["name"]
            }
        },
        {
            "name": "robot.move",
            "description": "Walk for a bounded time, then stop automatically. Speeds are clamped; max 3 seconds per call — call again to continue.",
            "parameters": {
                "type": "object",
                "properties": {
                    "vx": {"type": "number", "description": "m/s forward (+) / backward (-)"},
                    "vy": {"type": "number", "description": "m/s left (+) / right (-)"},
                    "vyaw": {"type": "number", "description": "rad/s turn, + counterclockwise"},
                    "duration_s": {"type": "number", "minimum": 0.1, "maximum": MAX_MOVE_DURATION_S}
                },
                "required": ["duration_s"]
            }
        },
        {
            "name": "robot.state",
            "description": "Current robot status: health, battery, mode.",
            "parameters": {"type": "object", "properties": {}}
        },
        {
            "name": "robot.get_frame",
            "description": "Grab a camera frame. Not supported yet on this robot.",
            "parameters": {"type": "object", "properties": {}}
        }
    ])
}

/// Execute one tool call. `Err(text)` becomes `tool.result {ok: false}`
/// with the text as the LLM-readable reason.
pub fn execute(name: &str, args: &Value, control: &mut Option<Control>) -> Result<Value, String> {
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
            let params = proto::MoveParams {
                vx: clamp(number(args, "vx"), MAX_SPEED_M_S),
                vy: clamp(number(args, "vy"), MAX_SPEED_M_S),
                vyaw: clamp(number(args, "vyaw"), MAX_YAW_RAD_S),
            };
            // Timed walk: pump the continuous intent for the duration,
            // then go silent — robotd's deadman remains the backstop.
            let end = Instant::now() + Duration::from_secs_f64(duration);
            while Instant::now() < end {
                notify(control, &proto::Call::RobotMove(params))?;
                std::thread::sleep(MOVE_TICK);
            }
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

fn number(args: &Value, key: &str) -> f64 {
    args.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn clamp(value: f64, limit: f64) -> f64 {
    value.clamp(-limit, limit)
}

fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} is required"))
}

fn with_robot(control: &mut Option<Control>) -> Result<&mut Control, String> {
    control
        .as_mut()
        .ok_or_else(|| "robot unreachable".to_string())
}

fn notify(control: &mut Option<Control>, call: &proto::Call) -> Result<(), String> {
    let robot = with_robot(control)?;
    robot.notify(call).map_err(|e| {
        *control = None;
        format!("robot lost: {e}")
    })
}

fn request(control: &mut Option<Control>, call: &proto::Call) -> Result<proto::Response, String> {
    let robot = with_robot(control)?;
    robot.request(call).map_err(|e| {
        *control = None;
        format!("robot lost: {e}")
    })
}

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
        let mut control = None;
        assert_eq!(
            execute("robot.fly", &json!({}), &mut control),
            Err("unknown tool `robot.fly`".to_string())
        );
        assert_eq!(
            execute("robot.get_frame", &json!({}), &mut control),
            Err("unsupported".to_string())
        );
    }

    #[test]
    fn robot_tools_without_a_robot_say_so() {
        let mut control = None;
        assert_eq!(
            execute("robot.sound", &json!({"tag": "chirp"}), &mut control),
            Err("robot unreachable".to_string())
        );
        assert_eq!(
            execute("robot.move", &json!({"duration_s": 1.0}), &mut control),
            Err("robot unreachable".to_string())
        );
    }

    #[test]
    fn bad_arguments_are_rejected_before_touching_the_robot() {
        let mut control = None;
        assert_eq!(
            execute("robot.sound", &json!({"tag": "explosion"}), &mut control),
            Err("unknown sound tag `explosion`".to_string())
        );
        assert_eq!(
            execute("robot.sound", &json!({"tag": "wheee"}), &mut control),
            Err("unknown sound tag `wheee`".to_string())
        );
        assert_eq!(
            execute("robot.skill", &json!({"name": "backflip"}), &mut control),
            Err("unknown skill `backflip`".to_string())
        );
        assert_eq!(
            execute("robot.move", &json!({}), &mut control),
            Err("duration_s is required".to_string())
        );
    }

    #[test]
    fn catalog_matches_the_executor_allowlist() {
        let catalog = catalog();
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
                "robot.head",
                "robot.skill",
                "robot.move",
                "robot.state",
                "robot.get_frame"
            ]
        );
        // Every cataloged tool must be dispatched (not fall through to
        // `unknown tool`): with no robot, the distinguishing error is
        // "robot unreachable" or "unsupported", never "unknown tool".
        let mut control = None;
        for name in names {
            let args = json!({"tag": "chirp", "name": "sit_toggle", "duration_s": 0.1});
            let err = execute(name, &args, &mut control)
                .err()
                .into_iter()
                .chain(Some(String::new()))
                .next()
                .unwrap();
            assert!(
                !err.starts_with("unknown tool"),
                "{name} is cataloged but not executable"
            );
        }
    }
}
