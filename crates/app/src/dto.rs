//! Wire-format DTOs for Tauri commands. Mirror `rm-macro-model` shapes but
//! kept separate so the wire format can evolve independently from the
//! internal domain types.

use chrono::{DateTime, Utc};
use rm_macro_model::{KeyCode, Macro, Modifier, MouseButton, PlaybackMode, Trigger};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct MacroDto {
    pub id: Uuid,
    pub name: String,
    pub trigger: TriggerDto,
    pub playback: PlaybackModeDto,
    pub step_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDto {
    Hotkey { key: KeyCode, modifiers: Vec<Modifier> },
    MouseButton { button: MouseButton, modifiers: Vec<Modifier> },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlaybackModeDto {
    Once,
    Repeat { value: u32 },
    Loop,
    Toggle,
}

impl From<&Trigger> for TriggerDto {
    fn from(t: &Trigger) -> Self {
        match t {
            Trigger::Hotkey { key, modifiers } => TriggerDto::Hotkey {
                key: *key,
                modifiers: modifiers.clone(),
            },
            Trigger::MouseButton { button, modifiers } => TriggerDto::MouseButton {
                button: *button,
                modifiers: modifiers.clone(),
            },
        }
    }
}

impl From<TriggerDto> for Trigger {
    fn from(t: TriggerDto) -> Self {
        match t {
            TriggerDto::Hotkey { key, modifiers } => Trigger::Hotkey { key, modifiers },
            TriggerDto::MouseButton { button, modifiers } => Trigger::MouseButton { button, modifiers },
        }
    }
}

impl From<&PlaybackMode> for PlaybackModeDto {
    fn from(p: &PlaybackMode) -> Self {
        match p {
            PlaybackMode::Once => PlaybackModeDto::Once,
            PlaybackMode::Repeat { count } => PlaybackModeDto::Repeat { value: *count },
            PlaybackMode::Loop => PlaybackModeDto::Loop,
            PlaybackMode::Toggle => PlaybackModeDto::Toggle,
        }
    }
}

impl From<PlaybackModeDto> for PlaybackMode {
    fn from(p: PlaybackModeDto) -> Self {
        match p {
            PlaybackModeDto::Once => PlaybackMode::Once,
            PlaybackModeDto::Repeat { value } => PlaybackMode::Repeat { count: value },
            PlaybackModeDto::Loop => PlaybackMode::Loop,
            PlaybackModeDto::Toggle => PlaybackMode::Toggle,
        }
    }
}

impl From<&Macro> for MacroDto {
    fn from(m: &Macro) -> Self {
        MacroDto {
            id: m.id,
            name: m.name.clone(),
            trigger: (&m.trigger).into(),
            playback: (&m.playback).into(),
            step_count: m.steps.len(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct PointDto {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoveModeDto {
    Absolute,
    Relative,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDto {
    KeyPress { key: rm_macro_model::KeyCode, hold_ms: u32 },
    KeyDown { key: rm_macro_model::KeyCode },
    KeyUp { key: rm_macro_model::KeyCode },
    MouseClick { button: rm_macro_model::MouseButton, hold_ms: u32, at: Option<PointDto> },
    MouseMove { to: PointDto, mode: MoveModeDto },
    MouseScroll { delta: i32 },
    Wait { min_ms: u32, max_ms: u32 },
}

impl From<&rm_macro_model::Point> for PointDto {
    fn from(p: &rm_macro_model::Point) -> Self { PointDto { x: p.x, y: p.y } }
}
impl From<PointDto> for rm_macro_model::Point {
    fn from(p: PointDto) -> Self { rm_macro_model::Point { x: p.x, y: p.y } }
}

impl From<&rm_macro_model::MoveMode> for MoveModeDto {
    fn from(m: &rm_macro_model::MoveMode) -> Self {
        match m {
            rm_macro_model::MoveMode::Absolute => MoveModeDto::Absolute,
            rm_macro_model::MoveMode::Relative => MoveModeDto::Relative,
        }
    }
}
impl From<MoveModeDto> for rm_macro_model::MoveMode {
    fn from(m: MoveModeDto) -> Self {
        match m {
            MoveModeDto::Absolute => rm_macro_model::MoveMode::Absolute,
            MoveModeDto::Relative => rm_macro_model::MoveMode::Relative,
        }
    }
}

impl From<&rm_macro_model::Step> for StepDto {
    fn from(s: &rm_macro_model::Step) -> Self {
        use rm_macro_model::Step;
        match s {
            Step::KeyPress { key, hold_ms } => StepDto::KeyPress { key: *key, hold_ms: *hold_ms },
            Step::KeyDown { key } => StepDto::KeyDown { key: *key },
            Step::KeyUp { key } => StepDto::KeyUp { key: *key },
            Step::MouseClick { button, hold_ms, at } => StepDto::MouseClick {
                button: *button,
                hold_ms: *hold_ms,
                at: at.as_ref().map(PointDto::from),
            },
            Step::MouseMove { to, mode } => StepDto::MouseMove {
                to: PointDto::from(to),
                mode: MoveModeDto::from(mode),
            },
            Step::MouseScroll { delta } => StepDto::MouseScroll { delta: *delta },
            Step::Wait { min_ms, max_ms } => StepDto::Wait { min_ms: *min_ms, max_ms: *max_ms },
        }
    }
}

impl From<StepDto> for rm_macro_model::Step {
    fn from(s: StepDto) -> Self {
        use rm_macro_model::Step;
        match s {
            StepDto::KeyPress { key, hold_ms } => Step::KeyPress { key, hold_ms },
            StepDto::KeyDown { key } => Step::KeyDown { key },
            StepDto::KeyUp { key } => Step::KeyUp { key },
            StepDto::MouseClick { button, hold_ms, at } => Step::MouseClick {
                button,
                hold_ms,
                at: at.map(Into::into),
            },
            StepDto::MouseMove { to, mode } => Step::MouseMove {
                to: to.into(),
                mode: mode.into(),
            },
            StepDto::MouseScroll { delta } => Step::MouseScroll { delta },
            StepDto::Wait { min_ms, max_ms } => Step::Wait { min_ms, max_ms },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingsDto {
    pub stop_key: rm_macro_model::KeyCode,
    pub storage_root_override: Option<String>,
}

impl From<&crate::settings::Settings> for SettingsDto {
    fn from(s: &crate::settings::Settings) -> Self {
        Self {
            stop_key: s.stop_key,
            storage_root_override: s
                .storage_root_override
                .as_ref()
                .map(|p| p.display().to_string()),
        }
    }
}

impl From<SettingsDto> for crate::settings::Settings {
    fn from(s: SettingsDto) -> Self {
        crate::settings::Settings {
            stop_key: s.stop_key,
            storage_root_override: s.storage_root_override.map(std::path::PathBuf::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_dto_roundtrips_through_json() {
        let t = TriggerDto::Hotkey {
            key: KeyCode::F1,
            modifiers: vec![Modifier::Ctrl, Modifier::Shift],
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"type\":\"hotkey\""));
        let back: TriggerDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn playback_mode_dto_serializes_with_tagged_repeat() {
        let p = PlaybackModeDto::Repeat { value: 7 };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"type\":\"repeat\""));
        assert!(json.contains("\"value\":7"));
        let back: PlaybackModeDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn macro_dto_from_macro_omits_steps_but_keeps_count() {
        let mut m = Macro::new(
            "demo",
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            },
            PlaybackMode::Once,
        );
        m.steps = vec![
            rm_macro_model::Step::Wait { min_ms: 100, max_ms: 100 },
            rm_macro_model::Step::Wait { min_ms: 50, max_ms: 50 },
        ];
        let dto: MacroDto = (&m).into();
        assert_eq!(dto.id, m.id);
        assert_eq!(dto.name, "demo");
        assert_eq!(dto.step_count, 2);
    }

    #[test]
    fn trigger_roundtrip_dto_to_domain() {
        let dto = TriggerDto::Hotkey {
            key: KeyCode::Enter,
            modifiers: vec![Modifier::Alt],
        };
        let domain: Trigger = dto.clone().into();
        let back: TriggerDto = (&domain).into();
        assert_eq!(back, dto);
    }

    #[test]
    fn playback_mode_roundtrip_dto_to_domain() {
        for dto in [
            PlaybackModeDto::Once,
            PlaybackModeDto::Repeat { value: 5 },
            PlaybackModeDto::Loop,
            PlaybackModeDto::Toggle,
        ] {
            let domain: PlaybackMode = dto.into();
            let back: PlaybackModeDto = (&domain).into();
            assert_eq!(back, dto);
        }
    }

    #[test]
    fn step_dto_key_press_roundtrips() {
        let s = StepDto::KeyPress { key: KeyCode::A, hold_ms: 80 };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"key_press\""));
        let back: StepDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn step_dto_wait_roundtrips() {
        let s = StepDto::Wait { min_ms: 50, max_ms: 150 };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"wait\""));
        let back: StepDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn step_dto_mouse_move_with_point_roundtrips() {
        let s = StepDto::MouseMove {
            to: PointDto { x: 10, y: -5 },
            mode: MoveModeDto::Relative,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StepDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn step_roundtrip_dto_to_domain_all_variants() {
        use rm_macro_model::{KeyCode, MouseButton, Step};
        let cases: Vec<Step> = vec![
            Step::KeyPress { key: KeyCode::W, hold_ms: 80 },
            Step::KeyDown { key: KeyCode::LShift },
            Step::KeyUp { key: KeyCode::LShift },
            Step::MouseClick { button: MouseButton::Left, hold_ms: 50, at: None },
            Step::MouseClick {
                button: MouseButton::Right,
                hold_ms: 80,
                at: Some(rm_macro_model::Point { x: 100, y: 200 }),
            },
            Step::MouseMove {
                to: rm_macro_model::Point { x: 5, y: -3 },
                mode: rm_macro_model::MoveMode::Relative,
            },
            Step::MouseScroll { delta: 120 },
            Step::Wait { min_ms: 100, max_ms: 100 },
        ];
        for domain in cases {
            let dto = StepDto::from(&domain);
            let back: Step = dto.into();
            assert_eq!(back, domain);
        }
    }
}
