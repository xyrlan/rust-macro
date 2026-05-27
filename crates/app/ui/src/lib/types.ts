// Mirror of crates/app/src/dto.rs. Keep in sync manually — runtime errors
// from a stale mirror will surface as "missing field" deserialisation errors
// in the Rust backend, which become WireError toasts in the UI.

// Values are the snake_case serde renames from rm_macro_model::KeyCode /
// Modifier. Keep this list in sync with crates/macro_model/src/input.rs.
// Display the user-facing label via the `keyCodeLabel(key)` helper below.
export type KeyCode =
  | "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
  | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z"
  | "num0" | "num1" | "num2" | "num3" | "num4"
  | "num5" | "num6" | "num7" | "num8" | "num9"
  | "f1" | "f2" | "f3" | "f4" | "f5" | "f6"
  | "f7" | "f8" | "f9" | "f10" | "f11" | "f12"
  | "l_shift" | "r_shift" | "l_ctrl" | "r_ctrl"
  | "l_alt" | "r_alt" | "l_win" | "r_win"
  | "space" | "enter" | "tab" | "backspace" | "escape" | "caps_lock"
  | "up" | "down" | "left" | "right"
  | "insert" | "delete" | "home" | "end" | "page_up" | "page_down"
  | "minus" | "equals" | "l_bracket" | "r_bracket" | "backslash" | "semicolon"
  | "apostrophe" | "backtick" | "comma" | "period" | "slash";

export type Modifier = "ctrl" | "shift" | "alt" | "win";

/** Format a snake_case KeyCode/Modifier for display. */
export function inputLabel(s: KeyCode | Modifier): string {
  return s
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export type Trigger = { type: "hotkey"; key: KeyCode; modifiers: Modifier[] };

export type PlaybackMode =
  | { type: "once" }
  | { type: "repeat"; value: number }
  | { type: "loop" }
  | { type: "toggle" };

export type MacroDto = {
  id: string;             // Uuid serialises as string
  name: string;
  trigger: Trigger;
  playback: PlaybackMode;
  step_count: number;
  created_at: string;     // RFC3339 datetime
  updated_at: string;
};

export type WireError = {
  kind:
    | "DriverNotInstalled"
    | "DriverNotRunning"
    | "DriverIo"
    | "MacroNotFound"
    | "RecordingActive"
    | "PlaybackActive"
    | "Io"
    | "Serde"
    | "Other";
  message: string;
};

export function isWireError(e: unknown): e is WireError {
  return (
    typeof e === "object" &&
    e !== null &&
    "kind" in e &&
    "message" in e &&
    typeof (e as Record<string, unknown>).kind === "string" &&
    typeof (e as Record<string, unknown>).message === "string"
  );
}
