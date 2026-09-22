//! The body's own commands on the wire: what a walk becomes once the
//! gait's trim is applied, the timed move, and the small helpers every
//! tool needs to read an argument or reach robotd.
//!
//! The navigation keeps its own copy of these (ADR 0006). Both send the
//! same `robot.move` to the same daemon; what binds them is robotd's
//! protocol, pinned by `duck-ipc-proto`.

use std::time::{Duration, Instant};

use crate::gait::GaitConfig;
use crate::robotd::Control;
use duck_ipc_proto as proto;
use serde_json::Value;

pub const MAX_MOVE_DURATION_S: f64 = 3.0;
pub const MAX_SPEED_M_S: f64 = 0.3;
pub const MAX_YAW_RAD_S: f64 = 1.0;
pub const MOVE_TICK: Duration = Duration::from_millis(40);

/// An optional number from a tool's arguments; absent reads as zero.
pub fn number(args: &Value, key: &str) -> f64 {
    args.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

pub fn clamp(value: f64, limit: f64) -> f64 {
    value.clamp(-limit, limit)
}

pub fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} is required"))
}

pub fn with_robot(control: &mut Option<Control>) -> Result<&mut Control, String> {
    control
        .as_mut()
        .ok_or_else(|| "robot unreachable".to_string())
}

pub fn notify(control: &mut Option<Control>, call: &proto::Call) -> Result<(), String> {
    let robot = with_robot(control)?;
    robot.notify(call).map_err(|e| {
        *control = None;
        format!("robot lost: {e}")
    })
}

pub fn request(control: &mut Option<Control>, call: &proto::Call) -> Result<proto::Response, String> {
    let robot = with_robot(control)?;
    robot.request(call).map_err(|e| {
        *control = None;
        format!("robot lost: {e}")
    })
}

pub fn move_params(args: &Value) -> proto::MoveParams {
    proto::MoveParams {
        vx: clamp(number(args, "vx"), MAX_SPEED_M_S),
        vy: clamp(number(args, "vy"), MAX_SPEED_M_S),
        vyaw: clamp(number(args, "vyaw"), MAX_YAW_RAD_S),
    }
}

/// The `[gait]` corrections, applied last, to what is actually sent.
pub fn trimmed(gait: &GaitConfig, mut params: proto::MoveParams) -> proto::MoveParams {
    params.vyaw = clamp(gait.yaw(params.vx, params.vyaw), MAX_YAW_RAD_S);
    params
}

/// Timed walk: pump the continuous intent for the duration, then go
/// silent — robotd's deadman remains the backstop. The heading hold
/// (odometry read back, taps the other way) went to the navigation with
/// the rest of what needs to know where the duck is: a voice assistant
/// that walks for three seconds does not need it.
pub fn timed_move(
    control: &mut Option<Control>,
    params: proto::MoveParams,
    duration_s: f64,
) -> Result<(), String> {
    let end = Instant::now() + Duration::from_secs_f64(duration_s);
    while Instant::now() < end {
        notify(control, &proto::Call::RobotMove(params))?;
        std::thread::sleep(MOVE_TICK);
    }
    Ok(())
}