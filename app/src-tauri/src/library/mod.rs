//! Library bundle handling: Ed25519 verification + idempotent loader.
//!
//! The risk/control/test-procedure library ships as a signed JSON bundle
//! embedded in the app binary via `include_bytes!`. At DB open time the
//! loader verifies the signature, parses the bundle, and inserts its rows
//! into the library tables if the version is not already present.
//!
//! See `NOTES.md` ("Library bundle format") for the design rationale and
//! `tools/sign-library-bundle/` for the offline signer CLI.

pub mod loader;
pub mod verify;
