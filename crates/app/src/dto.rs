//! Wire-format DTOs for Tauri commands. Mirror `rm-macro-model` shapes but
//! kept separate so the wire format can evolve independently from the
//! internal domain types.

use chrono::{DateTime, Utc};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
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
        }
    }
}

impl From<TriggerDto> for Trigger {
    fn from(t: TriggerDto) -> Self {
        match t {
            TriggerDto::Hotkey { key, modifiers } => Trigger::Hotkey { key, modifiers },
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
}
