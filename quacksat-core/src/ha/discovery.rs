//! The device Home Assistant builds from one discovery payload
//! (ADR 0007 §3, §6): every component is an entity, every command entity
//! a topic the worker turns into one call of the tool table.
//!
//! Entity ids are English and derived from the node and the component
//! key, so they stay put when the language changes; the names a person
//! reads follow `[announce] language`.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::announce::Lang;

/// What the payload depends on besides the config: the lists the robot
/// and the navigation answered with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lists {
    pub skills: Vec<String>,
    /// `None` when no navigation daemon answers: no place buttons, no
    /// "where" and "journey" sensors.
    pub places: Option<Vec<String>>,
}

pub struct Device<'a> {
    pub node: &'a str,
    pub name: &'a str,
    pub area: Option<&'a str>,
    pub base: &'a str,
    pub lang: Lang,
}

pub const SOUND_TAGS: [&str; 6] = ["alarm", "greet", "inquire", "peck", "chirp", "coo"];

/// The head's clamps, as `robot.head` applies them (`tools.rs`).
const HEAD_LIMITS: [(&str, f64); 3] = [("head_pitch", 0.6), ("head_yaw", 1.2), ("head_roll", 0.5)];

/// A name safe for a component key and an entity id.
pub fn slug(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let slug = slug.split('_').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("_");
    if slug.is_empty() { "x".into() } else { slug }
}

/// The components, keyed by their stable key.
pub fn components(device: &Device, lists: &Lists) -> BTreeMap<String, Value> {
    let it = device.lang == Lang::It;
    let pick = |a: &str, b: &str| if it { a.to_owned() } else { b.to_owned() };
    let cmd = |name: &str| format!("{}/cmd/{name}", device.base);
    let id = |platform: &str, key: &str| format!("{platform}.{}_{key}", device.node);
    let mut out = BTreeMap::new();
    let mut button = |key: String, name: String, command: &str, press: Option<&str>, icon: &str, enabled: bool| {
        let mut c = json!({
            "p": "button",
            "name": name,
            "uniq_id": format!("{}_{key}", device.node),
            "default_entity_id": id("button", &key),
            "cmd_t": cmd(command),
            "ic": icon,
        });
        if let Some(press) = press {
            c["pl_prs"] = json!(press);
        }
        if !enabled {
            c["en"] = json!(false);
        }
        out.insert(key, c);
    };

    button("forward".into(), pick("avanti", "forward"), "forward", None, "mdi:arrow-up-bold", true);
    button("turn_left".into(), pick("gira a sinistra", "turn left"), "turn_left", None, "mdi:arrow-left-top-bold", true);
    button("turn_right".into(), pick("gira a destra", "turn right"), "turn_right", None, "mdi:arrow-right-top-bold", true);
    button("stop".into(), "stop".into(), "stop", None, "mdi:stop", true);
    button("head_center".into(), pick("testa al centro", "center head"), "head_center", None, "mdi:target", true);
    for skill in &lists.skills {
        let label = skill.replace('_', " ");
        button(format!("skill_{}", slug(skill)), format!("skill {label}"), "skill", Some(skill), "mdi:human-handsup", true);
    }
    for tag in SOUND_TAGS {
        // Six of them would crowd the device page: there, off until wanted.
        button(format!("sound_{tag}"), pick(&format!("verso {tag}"), &format!("sound {tag}")), "sound", Some(tag), "mdi:bullhorn", false);
    }
    if let Some(places) = &lists.places {
        for place in places {
            button(format!("place_{}", slug(place)), pick(&format!("vai: {place}"), &format!("go to {place}")), "go_to", Some(place), "mdi:map-marker", true);
        }
    }

    for (key, limit) in HEAD_LIMITS {
        let name = match key {
            "head_pitch" => pick("testa su e giù", "head pitch"),
            "head_yaw" => pick("testa a destra e sinistra", "head yaw"),
            _ => pick("testa inclinata", "head roll"),
        };
        out.insert(
            key.to_owned(),
            json!({
                "p": "number",
                "name": name,
                "uniq_id": format!("{}_{key}", device.node),
                "default_entity_id": id("number", key),
                "cmd_t": cmd(key),
                "min": -limit,
                "max": limit,
                "step": 0.05,
                "mode": "slider",
                "unit_of_meas": "rad",
                "ic": "mdi:head",
            }),
        );
    }

    let state = format!("{}/state", device.base);
    let mut sensor = |key: &str, platform: &str, name: String, extra: Value| {
        let mut c = json!({
            "p": platform,
            "name": name,
            "uniq_id": format!("{}_{key}", device.node),
            "default_entity_id": id(platform, key),
            "stat_t": state,
        });
        if let (Value::Object(c), Value::Object(extra)) = (&mut c, extra) {
            c.extend(extra);
        }
        out.insert(key.to_owned(), c);
    };
    sensor("battery", "sensor", pick("batteria", "battery"), json!({
        "dev_cla": "battery", "unit_of_meas": "%", "stat_cla": "measurement",
        "val_tpl": "{{ value_json.battery }}",
    }));
    sensor("problem", "binary_sensor", pick("problema", "problem"), json!({
        "dev_cla": "problem",
        "val_tpl": "{{ 'OFF' if value_json.healthy else 'ON' }}",
        "json_attr_t": state, "json_attr_tpl": "{{ {'reason': value_json.reason} | tojson }}",
    }));
    // The last command's answer, as the duck would say it. Also what
    // keeps Home Assistant subscribed to the result topics at all times:
    // an automation that presses and then waits subscribes only after
    // the press, and an answer faster than that would be missed.
    sensor("last_answer", "sensor", pick("ultima risposta", "last answer"), json!({
        "stat_t": format!("{}/result/+", device.base),
        "val_tpl": "{{ 'ok' if value_json.ok else value_json.error }}",
        "ic": "mdi:message-reply-text",
    }));
    sensor("mode", "sensor", pick("modo", "mode"), json!({
        "val_tpl": "{{ value_json.mode }}", "ic": "mdi:walk",
    }));
    if lists.places.is_some() {
        sensor("where", "sensor", pick("dove", "where"), json!({
            "val_tpl": "{{ value_json.place }}", "ic": "mdi:map-marker-radius",
        }));
        sensor("journey", "sensor", pick("viaggio", "journey"), json!({
            "val_tpl": "{{ value_json.journey }}", "ic": "mdi:map-marker-path",
            "json_attr_t": state, "json_attr_tpl": "{{ {'reason': value_json.journey_reason} | tojson }}",
        }));
    }
    out
}

/// The whole device payload. `previous` holds what was published last
/// (key → platform): a component that is gone is sent as its platform
/// alone, which is how Home Assistant is told to remove it.
pub fn payload(device: &Device, lists: &Lists, previous: &BTreeMap<String, String>) -> (Value, BTreeMap<String, String>) {
    let components = components(device, lists);
    let published: BTreeMap<String, String> = components
        .iter()
        .map(|(key, c)| (key.clone(), c["p"].as_str().unwrap_or_default().to_owned()))
        .collect();
    let mut cmps: Map<String, Value> = components.into_iter().collect();
    for (key, platform) in previous {
        if !published.contains_key(key) {
            cmps.insert(key.clone(), json!({"p": platform}));
        }
    }
    let mut dev = json!({
        "ids": [format!("quacksat_{}", device.node)],
        "name": device.name,
        "mf": "quacksat",
        "mdl": "Microduck",
        "sw": env!("CARGO_PKG_VERSION"),
    });
    if let Some(area) = device.area {
        dev["sa"] = json!(area);
    }
    let payload = json!({
        "dev": dev,
        "o": {"name": "quacksat", "sw": env!("CARGO_PKG_VERSION"), "url": "https://github.com/andreagenovese/quacksat"},
        "avty": [{"t": format!("{}/availability", device.base)}, {"t": format!("{}/robot", device.base)}],
        "avty_mode": "all",
        "qos": 0,
        "cmps": cmps,
    });
    (payload, published)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(lang: Lang) -> Device<'static> {
        Device { node: "duck", name: "Papera", area: Some("Soggiorno"), base: "quacksat/duck", lang }
    }

    fn lists() -> Lists {
        Lists { skills: vec!["roulade".into(), "sit_toggle".into()], places: Some(vec!["cucina".into(), "Camera da letto".into()]) }
    }

    #[test]
    fn every_command_component_has_a_topic_under_the_node() {
        let components = components(&device(Lang::It), &lists());
        for (key, c) in &components {
            if matches!(c["p"].as_str(), Some("button" | "number")) {
                assert!(c["cmd_t"].as_str().unwrap().starts_with("quacksat/duck/cmd/"), "{key}");
            }
        }
        assert_eq!(components["place_camera_da_letto"]["pl_prs"], "Camera da letto");
        assert_eq!(components["place_camera_da_letto"]["cmd_t"], "quacksat/duck/cmd/go_to");
        assert_eq!(components["skill_sit_toggle"]["pl_prs"], "sit_toggle");
        assert_eq!(components["forward"]["default_entity_id"], "button.duck_forward");
        assert_eq!(components["forward"]["name"], "avanti");
        assert_eq!(components["sound_coo"]["en"], false);
    }

    #[test]
    fn the_names_follow_the_language_and_the_ids_do_not() {
        let it = components(&device(Lang::It), &lists());
        let en = components(&device(Lang::En), &lists());
        assert_eq!(en["turn_left"]["name"], "turn left");
        assert_eq!(it["turn_left"]["name"], "gira a sinistra");
        assert_eq!(it["turn_left"]["default_entity_id"], en["turn_left"]["default_entity_id"]);
    }

    #[test]
    fn without_navigation_there_are_no_places_and_no_journey() {
        let lists = Lists { places: None, ..lists() };
        let components = components(&device(Lang::En), &lists);
        assert!(!components.keys().any(|k| k.starts_with("place_")));
        assert!(!components.contains_key("journey") && !components.contains_key("where"));
    }

    #[test]
    fn a_forgotten_place_is_sent_as_its_platform_alone() {
        let (_, first) = payload(&device(Lang::It), &lists(), &BTreeMap::new());
        let fewer = Lists { places: Some(vec!["cucina".into()]), ..lists() };
        let (payload, second) = payload(&device(Lang::It), &fewer, &first);
        assert_eq!(payload["cmps"]["place_camera_da_letto"], json!({"p": "button"}));
        assert!(!second.contains_key("place_camera_da_letto"));
        assert_eq!(payload["dev"]["sa"], "Soggiorno");
        assert_eq!(payload["avty_mode"], "all");
    }

    #[test]
    fn slugs_are_safe() {
        assert_eq!(slug("Camera da letto"), "camera_da_letto");
        assert_eq!(slug("  L'ingresso! "), "l_ingresso");
        assert_eq!(slug("??"), "x");
    }
}
