# rust-macro — Plan 3: Tauri GUI (stub)

**Goal:** Bring the GUI online. Tauri (Rust backend) + Svelte/TS frontend, wired to the existing backend crates.

**Architecture:** New crate `rm-app` (Tauri main): registers commands that call into `rm-recorder`, `rm-player`, `rm-hotkey`, `rm-storage`. Frontend has views: Macro list, Editor (step-by-step), Recording overlay, Hotkey config, Settings.

**Tech Stack:** Tauri 2.x, Vite, Svelte 5 + TypeScript.

(Detailed tasks to be written when Plan 2 is merged and verified.)
