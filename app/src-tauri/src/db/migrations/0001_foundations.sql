-- Module 12: Sync & Storage foundations.
-- FK references to User and Engagement are allowed before those tables exist;
-- SQLite defers enforcement until the referenced table is created.

CREATE TABLE KeychainEntry (
    id                  TEXT PRIMARY KEY,
    purpose             TEXT NOT NULL,
    scope_entity_type   TEXT,
    scope_entity_id     TEXT,
    os_keychain_ref     TEXT NOT NULL,
    wrapped_key         BLOB,
    algorithm           TEXT NOT NULL,
    kdf                 TEXT,
    kdf_params_json     TEXT,
    created_at          INTEGER NOT NULL,
    rotated_from        TEXT,
    FOREIGN KEY (rotated_from) REFERENCES KeychainEntry(id)
);

CREATE TABLE EncryptedBlob (
    id                  TEXT PRIMARY KEY,
    owning_entity_type  TEXT,
    owning_entity_id    TEXT,
    filename            TEXT,
    mime_type           TEXT,
    nonce               BLOB NOT NULL,
    ciphertext_path     TEXT NOT NULL,
    auth_tag            BLOB NOT NULL,
    plaintext_size      INTEGER NOT NULL,
    key_id              TEXT NOT NULL,
    sha256_plaintext    TEXT,
    created_at          INTEGER NOT NULL,
    FOREIGN KEY (key_id) REFERENCES KeychainEntry(id)
);
CREATE INDEX idx_EncryptedBlob_owner ON EncryptedBlob (owning_entity_type, owning_entity_id);
CREATE INDEX idx_EncryptedBlob_sha ON EncryptedBlob (sha256_plaintext);

CREATE TABLE SyncRecord (
    id                  TEXT PRIMARY KEY,
    entity_type         TEXT NOT NULL,
    entity_id           TEXT NOT NULL,
    last_modified_at    INTEGER NOT NULL,
    last_modified_by    TEXT,
    version             INTEGER NOT NULL,
    deleted             INTEGER NOT NULL DEFAULT 0,
    sync_state          TEXT NOT NULL DEFAULT 'local_only',
    remote_version      INTEGER
);
CREATE INDEX idx_SyncRecord_entity ON SyncRecord (entity_type, entity_id);
CREATE INDEX idx_SyncRecord_state ON SyncRecord (sync_state);

CREATE TABLE ChangeLog (
    id                  TEXT PRIMARY KEY,
    sync_record_id      TEXT NOT NULL,
    occurred_at         INTEGER NOT NULL,
    user_id             TEXT,
    field_name          TEXT NOT NULL,
    old_value_json      TEXT,
    new_value_json      TEXT,
    FOREIGN KEY (sync_record_id) REFERENCES SyncRecord(id)
);
CREATE INDEX idx_ChangeLog_record ON ChangeLog (sync_record_id, occurred_at);

CREATE TABLE ConflictResolution (
    id                  TEXT PRIMARY KEY,
    sync_record_id      TEXT NOT NULL,
    detected_at         INTEGER NOT NULL,
    local_version       INTEGER NOT NULL,
    remote_version      INTEGER NOT NULL,
    resolution          TEXT,
    resolved_by         TEXT,
    resolved_at         INTEGER,
    FOREIGN KEY (sync_record_id) REFERENCES SyncRecord(id)
);
