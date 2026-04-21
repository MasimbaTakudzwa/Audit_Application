-- Module 1: Identity & Licensing.

CREATE TABLE Firm (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    country             TEXT NOT NULL,
    default_locale      TEXT NOT NULL DEFAULT 'en-GB',
    license_id          TEXT,
    library_version     TEXT,
    settings_json       TEXT,
    created_at          INTEGER NOT NULL
);

CREATE TABLE Role (
    id                  TEXT PRIMARY KEY,
    firm_id             TEXT,
    name                TEXT NOT NULL,
    permissions_json    TEXT NOT NULL,
    is_builtin          INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (firm_id) REFERENCES Firm(id)
);

CREATE TABLE User (
    id                  TEXT PRIMARY KEY,
    firm_id             TEXT NOT NULL,
    email               TEXT NOT NULL UNIQUE,
    display_name        TEXT NOT NULL,
    role_id             TEXT NOT NULL,
    argon2_hash         TEXT NOT NULL,
    master_key_wrapped  BLOB NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    last_seen_at        INTEGER,
    created_at          INTEGER NOT NULL,
    FOREIGN KEY (firm_id) REFERENCES Firm(id),
    FOREIGN KEY (role_id) REFERENCES Role(id)
);

CREATE TABLE License (
    id                  TEXT PRIMARY KEY,
    firm_id             TEXT NOT NULL,
    tier                TEXT NOT NULL,
    seats               INTEGER NOT NULL,
    hardware_binding    TEXT,
    issued_at           INTEGER NOT NULL,
    expires_at          INTEGER NOT NULL,
    grace_until         INTEGER,
    signature           BLOB NOT NULL,
    last_validated_at   INTEGER,
    FOREIGN KEY (firm_id) REFERENCES Firm(id)
);

CREATE TABLE SubscriptionPlan (
    id                  TEXT PRIMARY KEY,
    license_id          TEXT NOT NULL,
    monthly_quota_tokens INTEGER NOT NULL,
    tokens_used_cycle   INTEGER NOT NULL DEFAULT 0,
    cycle_resets_at     INTEGER NOT NULL,
    overage_policy      TEXT NOT NULL,
    FOREIGN KEY (license_id) REFERENCES License(id)
);

CREATE TABLE PrepaidBalance (
    id                  TEXT PRIMARY KEY,
    license_id          TEXT NOT NULL,
    tokens_remaining    INTEGER NOT NULL,
    last_topup_at       INTEGER,
    last_topup_tokens   INTEGER,
    FOREIGN KEY (license_id) REFERENCES License(id)
);

CREATE TABLE BYOKeyConfig (
    id                  TEXT PRIMARY KEY,
    user_id             TEXT NOT NULL,
    provider            TEXT NOT NULL,
    key_label           TEXT NOT NULL,
    key_ciphertext      BLOB NOT NULL,
    nonce               BLOB NOT NULL,
    auth_tag            BLOB NOT NULL,
    created_at          INTEGER NOT NULL,
    last_used_at        INTEGER,
    FOREIGN KEY (user_id) REFERENCES User(id)
);

-- Seed the four built-in roles.
INSERT INTO Role (id, firm_id, name, permissions_json, is_builtin) VALUES
    ('role-partner',   NULL, 'Partner',   '["*"]', 1),
    ('role-manager',   NULL, 'Manager',   '["engagement:*","review:*","finding:*"]', 1),
    ('role-senior',    NULL, 'Senior',    '["engagement:work","test:*","evidence:*","finding:draft"]', 1),
    ('role-associate', NULL, 'Associate', '["engagement:work","test:execute","evidence:upload"]', 1),
    ('role-readonly',  NULL, 'ReadOnly',  '["*:read"]', 1);
