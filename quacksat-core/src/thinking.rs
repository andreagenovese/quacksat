//! The thinking pose: body language while the duck waits for its answer.
//!
//! Between a command and the reply several seconds can pass with the duck
//! silent and still — indistinguishable from a duck that did not hear.
//! The timeline (agreed in docs/agent-backend-plan.md):
//!
//! - utterance closed → nothing (fast replies deserve no theatrics);
//! - after `[thinking] delay_s` → a slow head sway via `robot.head`,
//!   held until the reply arrives or the wait is abandoned;
//! - reply (`tts.start` / `audio-start`) → the head recenters, the duck
//!   speaks;
//! - timeout or error → a low "peck" tock instead of silence.
//!
//! `robot.head` is a continuous intent (a notification): the sway is a
//! throttled resend, the same shape as the `robot.move` pump in `tools`.
//! If the agent starts acting mid-wait (a tool call arrives), the pose
//! yields the body with [`ThinkingPose::release`] — no recenter, so a
//! tool-driven `robot.look` is not clobbered.

use std::time::{Duration, Instant};

use duck_ipc_proto as proto;

use crate::config::ThinkingConfig;
use crate::robotd::Control;

/// Resend cadence for the sway — 10 Hz is fluid and a rounding error of
/// bus traffic next to the 25 Hz move pump.
const TICK: Duration = Duration::from_millis(100);
/// One full sway cycle. Slow reads as pensive; fast reads as agitated.
const SWAY_PERIOD_S: f32 = 2.4;
/// Sway amplitude (rad): head_yaw wanders, head_roll holds a slight tilt.
const SWAY_YAW: f64 = 0.15;
const TILT_ROLL: f64 = 0.10;

pub struct ThinkingPose {
    enabled: bool,
    delay: Duration,
    /// Waiting for a reply since — `None` when idle.
    waiting_since: Option<Instant>,
    /// Whether any sway was actually sent (recenter is owed only then).
    swaying: bool,
    /// The body was ceded to the agent (a tool call arrived): no more
    /// sway this wait, but the clock keeps running for the timeout.
    yielded: bool,
    last_tick: Instant,
}

impl ThinkingPose {
    pub fn from_config(config: &ThinkingConfig) -> Self {
        Self {
            enabled: config.enabled,
            delay: Duration::from_secs_f32(config.delay_s.max(0.0)),
            waiting_since: None,
            swaying: false,
            yielded: false,
            last_tick: Instant::now(),
        }
    }

    /// The utterance closed: start the clock. No sound, no motion yet.
    pub fn begin(&mut self) {
        self.waiting_since = Some(Instant::now());
        self.swaying = false;
        self.yielded = false;
    }

    /// How long the current wait has run, if one is in progress. The
    /// backend owns the timeout decision; this is its input.
    pub fn waited(&self) -> Option<Duration> {
        self.waiting_since.map(|since| since.elapsed())
    }

    /// Call often (every poll iteration is fine — resends are throttled
    /// internally). Past the delay, holds the pensive pose: a slight roll
    /// tilt with a slow yaw sway.
    pub fn tick(&mut self, control: &mut Option<Control>) {
        let Some(since) = self.waiting_since else {
            return;
        };
        let elapsed = since.elapsed();
        if !self.enabled || self.yielded || elapsed < self.delay || self.last_tick.elapsed() < TICK
        {
            return;
        }
        self.last_tick = Instant::now();
        let t = (elapsed - self.delay).as_secs_f32();
        let phase = 2.0 * std::f32::consts::PI * t / SWAY_PERIOD_S;
        let params = proto::HeadParams {
            neck_pitch: 0.0,
            head_pitch: 0.0,
            head_yaw: SWAY_YAW * f64::from(phase.sin()),
            head_roll: TILT_ROLL,
        };
        if notify_head(control, params) {
            if !self.swaying {
                tracing::info!("thinking pose: swaying while the answer cooks");
            }
            self.swaying = true;
        }
    }

    /// The reply arrived: recenter the head (if the pose ever moved it)
    /// and stop waiting.
    pub fn end(&mut self, control: &mut Option<Control>) {
        if std::mem::take(&mut self.swaying) {
            notify_head(control, proto::HeadParams::default());
            tracing::info!("thinking pose: answer arrived, head recentered");
        }
        self.waiting_since = None;
        self.yielded = false;
    }

    /// The agent started acting (a tool call arrived): the body is the
    /// agent's now. Stop swaying — without recentering, so a tool-driven
    /// head move is not clobbered — but keep the clock running: a bridge
    /// that dies after a tool call must still hit the timeout.
    pub fn release(&mut self) {
        if self.swaying {
            tracing::debug!("thinking pose: yielding the body to the agent");
        }
        self.swaying = false;
        self.yielded = true;
    }

    /// The tool finished and did not pose the head itself: thinking
    /// continues, so the sway may resume on the next tick.
    pub fn resume(&mut self) {
        self.yielded = false;
    }
}

fn notify_head(control: &mut Option<Control>, params: proto::HeadParams) -> bool {
    let Some(c) = control else { return false };
    match c.notify(&proto::Call::RobotHead(params)) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(error = %e, "robotd lost — continuing without it");
            *control = None;
            false
        }
    }
}

/// The give-up sound: a low "peck" tock from robotd's voice bank — the
/// closest thing the closed enum has to disappointment. Returns whether
/// the robot played it; a `false` means the caller should fall back to
/// the local synthesized sigh ([`crate::playback::sad_pcm`]).
pub fn sad_tock(control: &mut Option<Control>) -> bool {
    let Some(c) = control else { return false };
    let call = proto::Call::RobotSound(proto::SoundParams {
        tag: proto::SoundTag::Peck,
        hold: None,
    });
    match c.intent(&call) {
        Ok(result) if result.accepted => true,
        Ok(result) => {
            tracing::debug!(reason = ?result.reason, "sad tock refused");
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "robotd lost — continuing without it");
            *control = None;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::net::UnixListener;

    fn instant_config() -> ThinkingConfig {
        ThinkingConfig {
            enabled: true,
            delay_s: 0.0,
            timeout_s: 30.0,
        }
    }

    #[test]
    fn no_motion_before_the_delay() {
        let config = ThinkingConfig {
            delay_s: 60.0,
            ..instant_config()
        };
        let mut pose = ThinkingPose::from_config(&config);
        pose.begin();
        // control = None: state must still be trackable without a robot.
        let mut control = None;
        pose.tick(&mut control);
        assert!(!pose.swaying);
        assert!(pose.waited().is_some());
        pose.end(&mut control);
        assert!(pose.waited().is_none());
    }

    #[test]
    fn disabled_pose_still_tracks_the_wait() {
        let config = ThinkingConfig {
            enabled: false,
            ..instant_config()
        };
        let mut pose = ThinkingPose::from_config(&config);
        pose.begin();
        let mut control = None;
        pose.tick(&mut control);
        assert!(!pose.swaying);
        assert!(pose.waited().is_some(), "timeout logic needs the clock");
    }

    #[test]
    fn sways_after_delay_and_recenters_on_end() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("robotd.sock");
        let listener = UnixListener::bind(&socket).unwrap();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream);
            let mut lines = Vec::new();
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                lines.push(line);
            }
            lines
        });

        let mut control = Some(Control::connect(socket.to_str().unwrap()).unwrap());
        let mut pose = ThinkingPose::from_config(&instant_config());
        pose.begin();
        // Force the throttle open so the first tick sends immediately.
        pose.last_tick = Instant::now() - TICK;
        pose.tick(&mut control);
        assert!(pose.swaying);
        pose.end(&mut control);

        let lines = server.join().unwrap();
        for line in &lines {
            let request: proto::Request = serde_json::from_str(line).unwrap();
            assert!(request.is_notification());
            assert_eq!(request.method, "robot.head");
        }
        // The second line is the recenter: all angles zero.
        let recenter: proto::Request = serde_json::from_str(&lines[1]).unwrap();
        let params = recenter.params.unwrap();
        assert_eq!(params["head_yaw"].as_f64(), Some(0.0));
        assert_eq!(params["head_roll"].as_f64(), Some(0.0));
    }

    #[test]
    fn release_stops_without_recentering() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("robotd.sock");
        let listener = UnixListener::bind(&socket).unwrap();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = std::io::BufReader::new(stream);
            let mut count = 0;
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 {
                count += 1;
                line.clear();
            }
            count
        });

        let mut control = Some(Control::connect(socket.to_str().unwrap()).unwrap());
        let mut pose = ThinkingPose::from_config(&instant_config());
        pose.begin();
        pose.last_tick = Instant::now() - TICK;
        pose.tick(&mut control);
        assert!(pose.swaying);
        pose.release();
        assert!(
            pose.waited().is_some(),
            "the timeout clock survives release"
        );
        pose.last_tick = Instant::now() - TICK;
        pose.tick(&mut control); // must send nothing after release
        assert!(!pose.swaying);
        pose.resume();
        pose.last_tick = Instant::now() - TICK;
        pose.tick(&mut control); // sways again after resume
        assert!(pose.swaying);
        drop(control); // close the socket so the server thread finishes

        // Two sways (before release, after resume) and no recenter.
        assert_eq!(server.join().unwrap(), 2);
    }
}
