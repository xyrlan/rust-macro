use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::input::{KeyCode, Modifier, MouseButton, MoveMode, Point};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    KeyPress {
        key: KeyCode,
        hold_ms: u32,
    },
    KeyDown {
        key: KeyCode,
    },
    KeyUp {
        key: KeyCode,
    },
    MouseClick {
        button: MouseButton,
        hold_ms: u32,
        at: Option<Point>,
    },
    MouseMove {
        to: Point,
        mode: MoveMode,
    },
    MouseScroll {
        delta: i32,
    },
    Wait {
        min_ms: u32,
        max_ms: u32,
    },
}

impl Step {
    /// Validates a `Wait` step has min <= max. Returns Err with a human message otherwise.
    pub fn validate(&self) -> Result<(), String> {
        if let Step::Wait { min_ms, max_ms } = self {
            if min_ms > max_ms {
                return Err(format!("Wait: min_ms ({min_ms}) > max_ms ({max_ms})"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Hotkey {
        key: KeyCode,
        modifiers: Vec<Modifier>,
    },
    MouseButton {
        button: MouseButton,
        modifiers: Vec<Modifier>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PlaybackMode {
    Once,
    Repeat { count: u32 },
    Loop,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub trigger: Trigger,
    pub playback: PlaybackMode,
    pub steps: Vec<Step>,
}

impl Macro {
    /// Create a new macro with generated id and current timestamps.
    pub fn new(name: impl Into<String>, trigger: Trigger, playback: PlaybackMode) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            created_at: now,
            updated_at: now,
            trigger,
            playback,
            steps: Vec::new(),
        }
    }

    /// Validate every step. Returns Err with the first failure.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Macro name cannot be empty".into());
        }
        for (i, step) in self.steps.iter().enumerate() {
            step.validate().map_err(|e| format!("step #{i}: {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::*;

    #[test]
    fn step_keypress_serde_roundtrip() {
        let s = Step::KeyPress {
            key: KeyCode::W,
            hold_ms: 250,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"type\":\"key_press\""));
        assert!(j.contains("\"key\":\"w\""));
        assert!(j.contains("\"hold_ms\":250"));
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn step_wait_serde_roundtrip() {
        let s = Step::Wait {
            min_ms: 100,
            max_ms: 300,
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn step_mouse_click_optional_at() {
        let s = Step::MouseClick {
            button: MouseButton::Left,
            hold_ms: 50,
            at: None,
        };
        let j = serde_json::to_string(&s).unwrap();
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);

        let s2 = Step::MouseClick {
            button: MouseButton::Right,
            hold_ms: 80,
            at: Some(Point { x: 10, y: 20 }),
        };
        let j2 = serde_json::to_string(&s2).unwrap();
        let back2: Step = serde_json::from_str(&j2).unwrap();
        assert_eq!(s2, back2);
    }

    #[test]
    fn step_wait_validates_min_le_max() {
        assert!(Step::Wait {
            min_ms: 100,
            max_ms: 100
        }
        .validate()
        .is_ok());
        assert!(Step::Wait {
            min_ms: 100,
            max_ms: 200
        }
        .validate()
        .is_ok());
        assert!(Step::Wait {
            min_ms: 200,
            max_ms: 100
        }
        .validate()
        .is_err());
    }

    #[test]
    fn macro_new_sets_timestamps_and_id() {
        let m = Macro::new(
            "test",
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![],
            },
            PlaybackMode::Once,
        );
        assert_eq!(m.name, "test");
        assert_eq!(m.created_at, m.updated_at);
        assert_eq!(m.steps.len(), 0);
        // Sanity check that id is not nil
        assert_ne!(m.id, Uuid::nil());
    }

    #[test]
    fn macro_full_roundtrip() {
        let mut m = Macro::new(
            "greet",
            Trigger::Hotkey {
                key: KeyCode::F2,
                modifiers: vec![Modifier::Ctrl],
            },
            PlaybackMode::Repeat { count: 3 },
        );
        m.steps = vec![
            Step::KeyPress {
                key: KeyCode::H,
                hold_ms: 80,
            },
            Step::Wait {
                min_ms: 50,
                max_ms: 150,
            },
            Step::KeyPress {
                key: KeyCode::I,
                hold_ms: 80,
            },
        ];
        let j = serde_json::to_string_pretty(&m).unwrap();
        let back: Macro = serde_json::from_str(&j).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn macro_validate_rejects_empty_name() {
        let m = Macro::new(
            "   ",
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![],
            },
            PlaybackMode::Once,
        );
        assert!(m.validate().is_err());
    }

    #[test]
    fn playback_mode_repeat_count_in_json() {
        let p = PlaybackMode::Repeat { count: 5 };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"mode\":\"repeat\""));
        assert!(j.contains("\"count\":5"));
        let back: PlaybackMode = serde_json::from_str(&j).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn trigger_mouse_button_serde_roundtrip() {
        let t = Trigger::MouseButton {
            button: MouseButton::X1,
            modifiers: vec![Modifier::Ctrl],
        };
        let j = serde_json::to_string(&t).unwrap();
        assert!(j.contains("\"type\":\"mouse_button\""));
        assert!(j.contains("\"button\":\"x1\""));
        let back: Trigger = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn legacy_hotkey_trigger_still_parses() {
        // Vintage 3a/3b on-disk format. Must continue to load after the
        // MouseButton variant is added.
        let j = r#"{"type":"hotkey","key":"f1","modifiers":["ctrl"]}"#;
        let back: Trigger = serde_json::from_str(j).unwrap();
        assert_eq!(
            back,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            }
        );
    }
}
