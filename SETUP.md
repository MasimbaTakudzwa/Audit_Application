# Audit Application — first-run setup

One-off per development machine. After this, `npm run tauri dev` from inside `app/` launches the desktop application.

## Prerequisites

1. **Rust** (stable). Install via <https://rustup.rs> (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`).
2. **Node.js** ≥ 20. On macOS: `brew install node`. On Windows/Linux: use nvm or a distro package.
3. **Platform build dependencies** — see <https://tauri.app/start/prerequisites/>.
   - **macOS**: `xcode-select --install`.
   - **Windows**: Microsoft C++ Build Tools + Microsoft Edge WebView2 (bundled on Windows 11).
   - **Linux** (Debian/Ubuntu): `webkit2gtk-4.1`, `libssl-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`.

## First run

```bash
cd app
npm install
npm run tauri dev
```

The first compilation builds Rust, SQLCipher, and OpenSSL from source and takes 5–15 minutes. Subsequent launches are fast.

## What you should see

A window titled **Audit Application** opens with the sidebar (`Dashboard`, `Clients`, `Engagements`, `Library`, `Settings`). The Dashboard shows:
- **Backend**: green dot + `audit-app v0.1.0` once the Rust side responds to the `ping` command.
- **Database**: "Initialised" once the seven migrations are applied (check `~/Library/Application Support/com.simba.auditapp/audit.db`).

## Project layout

- `app/` — the Tauri desktop project.
  - `src/` — Svelte 5 frontend (TypeScript, Vite, no component framework beyond Svelte).
  - `src-tauri/` — Rust backend.
    - `src/commands/` — Tauri commands, grouped by module.
    - `src/db/` — SQLite connection, migrations, and schema.
    - `src/db/migrations/*.sql` — versioned schema applied at launch.
    - `src/crypto/` — AES-256-GCM cipher, Argon2id KDF, OS keychain.
    - `src/models/` — Rust structs matching tables.
  - `package.json` — Node / Svelte / Vite deps.
  - `vite.config.ts`, `tsconfig.json` — frontend build config.
  - `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` — Rust and Tauri config.

## Development workflow

- `npm run tauri dev` — run the app with hot-reload on the frontend and auto-rebuild on Rust changes.
- `npm run check` — type-check the frontend.
- `cargo check --manifest-path app/src-tauri/Cargo.toml` — fast Rust compile check without running.
- `cargo test --manifest-path app/src-tauri/Cargo.toml` — run unit tests (currently: crypto round-trip, KDF determinism).

## Where the database lives

| Platform | Location |
|---|---|
| macOS   | `~/Library/Application Support/com.simba.auditapp/audit.db` |
| Windows | `%APPDATA%\com.simba.auditapp\audit.db` |
| Linux   | `~/.local/share/com.simba.auditapp/audit.db` |

Delete it to start from a clean schema on next launch.

## Next development steps

See `PROGRESS.md` for up-to-date status. The scaffold is ready for:

1. **Auth flow** — wire the Argon2id verifier into a first-run user creation UI; unlock the SQLCipher key.
2. **Engagement creation** — first real mutation path; exercises `SyncRecord`, `ChangeLog`, `ActivityLog` together.
3. **User access review vertical slice** — the recommended first prototype module (exercises 10 of 13 modules).
