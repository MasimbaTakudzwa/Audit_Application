# Audit Application

An IT audit platform for small-to-mid audit firms in Africa, combining structured ITGC / ITAC testing with a Huddle-like client document portal.

## Two sides

- **Auditor desktop app** (paid) — fieldwork, risk assessment, testing, working papers, findings, reports
- **Client web portal** (free) — document exchange, evidence submission, attestations, status updates

## Why

Existing platforms (TeamMate, Workiva, AuditBoard) are expensive, cloud-first, and poorly suited to the low-bandwidth environments where much African audit work happens. Clients often have disorganised or paper-based records; the tooling has to meet them where they are. This project fills that gap with an offline-capable desktop app and a simple browser portal for clients.

## Status

Scaffold phase. Cross-platform Tauri project exists under `app/` with database migrations applied at launch. No real engagement flow yet. See `CLAUDE.md` for the architecture, `PROGRESS.md` for live status, and `SETUP.md` for how to run it.

## Platform

Cross-platform (Windows, macOS, Linux) built with Tauri. Local-first with encrypted storage (SQLCipher + AES-256-GCM). Sync to the cloud when online; encrypted blobs only — the server never sees plaintext.

## Automation approach

Rule-based scripts and classical ML handle most of the work (user access reconciliation, sampling, ITAC tests, anomaly detection, semantic finding matching). Hosted LLMs are reserved for tasks that genuinely need judgement — drafting finding prose, extracting controls from unstructured policies, summarising interview notes.

## Three consumer paths for AI features

- **Subscription** — included LLM quota through the project's API account, low per-query cost
- **Prepaid** — pay-as-you-go top-ups, higher markup, smaller packs cost more per unit
- **BYO-key** — bring your own Claude / Gemini / OpenAI / Meta key; the app adds value through rule-based query refinement before dispatch to your provider

## Author

Simba Gangaidzo — IT Auditor, Baker Tilly Zimbabwe.
