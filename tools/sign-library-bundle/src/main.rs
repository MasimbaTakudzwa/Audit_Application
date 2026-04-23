//! Offline CLI for generating, signing, and verifying Audit Application
//! library bundles. Kept separate from the app binary so the private key never
//! touches the shipping build.
//!
//! Subcommands:
//!   keygen  --out <dir>                      Generate an Ed25519 keypair.
//!   sign    --key <path> --bundle <path>     Sign a bundle; writes <bundle>.sig.
//!   verify  --pubkey <hex> --bundle <path>   Verify <bundle>.sig against the bundle.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let sub = match args.first() {
        Some(s) => s.as_str(),
        None => {
            eprintln!("{}", USAGE);
            return ExitCode::from(2);
        }
    };
    let rest = &args[1..];

    let result = match sub {
        "keygen" => keygen(rest),
        "sign" => sign(rest),
        "verify" => verify(rest),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => {
            eprintln!("unknown subcommand: {other}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "\
sign-library-bundle — offline library bundle signer

usage:
  sign-library-bundle keygen  --out <dir>
  sign-library-bundle sign    --key <path> --bundle <path>
  sign-library-bundle verify  --pubkey <hex32> --bundle <path>

notes:
  keygen writes <dir>/library.key (0600, hex-encoded 32-byte seed) and prints
  the public key as hex to stdout. The public key must be baked into the app.
  sign reads the private key file and writes <bundle>.sig next to <bundle>.
  verify reads <bundle> and <bundle>.sig, checking the signature against the
  provided hex-encoded public key.
";

fn keygen(args: &[String]) -> Result<(), String> {
    let out_dir = flag(args, "--out").ok_or("missing --out <dir>")?;
    let out_dir = PathBuf::from(out_dir);
    fs::create_dir_all(&out_dir).map_err(|e| format!("create dir: {e}"))?;

    let mut rng = OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();

    let priv_path = out_dir.join("library.key");
    write_private_key(&priv_path, signing_key.as_bytes())?;

    println!("private key: {}", priv_path.display());
    println!("public key (hex, 32 bytes):");
    println!("{}", hex::encode(verifying_key.as_bytes()));
    println!();
    println!("Bake the public key into app/src-tauri/src/library/verify.rs as the LIBRARY_PUBLIC_KEY const.");
    println!("Keep the private key file offline. Do not check it in.");
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let key_path = flag(args, "--key").ok_or("missing --key <path>")?;
    let bundle_path = flag(args, "--bundle").ok_or("missing --bundle <path>")?;

    let seed = read_private_key(Path::new(&key_path))?;
    let signing_key = SigningKey::from_bytes(&seed);

    let bundle_bytes =
        fs::read(&bundle_path).map_err(|e| format!("read bundle: {e}"))?;
    let sig: Signature = signing_key.sign(&bundle_bytes);

    let sig_path = format!("{}.sig", bundle_path);
    fs::write(&sig_path, hex::encode(sig.to_bytes()))
        .map_err(|e| format!("write signature: {e}"))?;
    println!("signed: {sig_path}");
    Ok(())
}

fn verify(args: &[String]) -> Result<(), String> {
    let pubkey_hex = flag(args, "--pubkey").ok_or("missing --pubkey <hex>")?;
    let bundle_path = flag(args, "--bundle").ok_or("missing --bundle <path>")?;

    let pk_bytes = hex::decode(pubkey_hex.trim())
        .map_err(|e| format!("pubkey hex decode: {e}"))?;
    let pk_array: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "public key must be 32 bytes".to_string())?;
    let verifying_key = VerifyingKey::from_bytes(&pk_array)
        .map_err(|e| format!("public key parse: {e}"))?;

    let bundle_bytes =
        fs::read(&bundle_path).map_err(|e| format!("read bundle: {e}"))?;
    let sig_path = format!("{}.sig", bundle_path);
    let sig_hex = fs::read_to_string(&sig_path)
        .map_err(|e| format!("read signature: {e}"))?;
    let sig_bytes = hex::decode(sig_hex.trim())
        .map_err(|e| format!("signature hex decode: {e}"))?;
    let sig_array: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    let sig = Signature::from_bytes(&sig_array);

    verifying_key
        .verify(&bundle_bytes, &sig)
        .map_err(|e| format!("signature invalid: {e}"))?;
    println!("signature valid");
    Ok(())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == name {
            return iter.next().cloned();
        }
    }
    None
}

fn write_private_key(path: &Path, seed: &[u8; 32]) -> Result<(), String> {
    let hex_seed = hex::encode(seed);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("create private key file: {e}"))?;
        f.write_all(hex_seed.as_bytes())
            .map_err(|e| format!("write private key: {e}"))?;
    }
    #[cfg(not(unix))]
    {
        if path.exists() {
            return Err(format!("private key file already exists: {}", path.display()));
        }
        fs::write(path, hex_seed.as_bytes())
            .map_err(|e| format!("write private key: {e}"))?;
    }
    Ok(())
}

fn read_private_key(path: &Path) -> Result<[u8; 32], String> {
    let s = fs::read_to_string(path)
        .map_err(|e| format!("read private key: {e}"))?;
    let bytes = hex::decode(s.trim())
        .map_err(|e| format!("private key hex decode: {e}"))?;
    bytes
        .as_slice()
        .try_into()
        .map_err(|_| "private key must be 32 bytes".to_string())
}
