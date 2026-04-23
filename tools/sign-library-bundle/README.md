# sign-library-bundle

Offline CLI for signing Audit Application library bundles. Kept separate from
the app binary so the private key never ships with the product.

## First-time setup

One-off per signer. Generates a keypair, writes the private key to a path
outside the repo, prints the public key for you to bake into the app.

```bash
cd tools/sign-library-bundle
mkdir -p ~/.config/audit-app/signing
cargo run --release -- keygen --out ~/.config/audit-app/signing

# copy the printed hex public key into:
#   app/src-tauri/src/library/verify.rs  (LIBRARY_PUBLIC_KEY constant)
```

The private key lives at `~/.config/audit-app/signing/library.key` with 0600
permissions. Back it up to encrypted offline storage. Losing it means every
future bundle must be re-signed under a new key and the app rebuilt.

## Signing a bundle

```bash
cargo run --release -- sign \
  --key ~/.config/audit-app/signing/library.key \
  --bundle ../../app/src-tauri/resources/library/v0.1.0.json
```

Writes `v0.1.0.json.sig` alongside the bundle, as hex-encoded ASCII.

## Verifying a bundle (sanity check)

```bash
cargo run --release -- verify \
  --pubkey <32-byte hex from verify.rs> \
  --bundle ../../app/src-tauri/resources/library/v0.1.0.json
```

The app itself verifies on load; this subcommand is for local troubleshooting.
