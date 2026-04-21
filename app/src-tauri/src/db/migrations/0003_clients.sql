-- Module 2: Client Management.

CREATE TABLE Industry (
    id                         TEXT PRIMARY KEY,
    name                       TEXT NOT NULL,
    default_system_types_json  TEXT,
    is_builtin                 INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE Client (
    id                  TEXT PRIMARY KEY,
    firm_id             TEXT NOT NULL,
    name                TEXT NOT NULL,
    industry_id         TEXT,
    country             TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'active',
    created_at          INTEGER NOT NULL,
    FOREIGN KEY (firm_id) REFERENCES Firm(id),
    FOREIGN KEY (industry_id) REFERENCES Industry(id)
);

CREATE TABLE ClientContact (
    id                  TEXT PRIMARY KEY,
    client_id           TEXT NOT NULL,
    name                TEXT NOT NULL,
    role                TEXT,
    email               TEXT,
    phone               TEXT,
    is_portal_enabled   INTEGER NOT NULL DEFAULT 0,
    notes_blob_id       TEXT,
    FOREIGN KEY (client_id) REFERENCES Client(id),
    FOREIGN KEY (notes_blob_id) REFERENCES EncryptedBlob(id)
);

CREATE TABLE ClientSettings (
    id                         TEXT PRIMARY KEY,
    client_id                  TEXT NOT NULL,
    evidence_retention_years   INTEGER NOT NULL DEFAULT 7,
    portal_logo_blob_id        TEXT,
    portal_accent_colour       TEXT,
    custom_fields_json         TEXT,
    FOREIGN KEY (client_id) REFERENCES Client(id),
    FOREIGN KEY (portal_logo_blob_id) REFERENCES EncryptedBlob(id)
);

-- Seed built-in industries with sensible default system types.
INSERT INTO Industry (id, name, default_system_types_json, is_builtin) VALUES
    ('ind-banking',       'Banking',              '["AD","core_banking","SQL","Oracle_DB"]', 1),
    ('ind-insurance',     'Insurance',            '["AD","policy_admin","SQL"]', 1),
    ('ind-retail',        'Retail',               '["AD","SAP","Oracle_EBS","POS"]', 1),
    ('ind-telecoms',      'Telecommunications',   '["AD","billing","OSS","BSS"]', 1),
    ('ind-public-sector', 'Public Sector',        '["AD","Entra","SAP"]', 1),
    ('ind-mining',        'Mining',               '["AD","SAP","Oracle_EBS","SCADA"]', 1),
    ('ind-nonprofit',     'Non-profit',           '["AD","Entra"]', 1),
    ('ind-other',         'Other',                '["AD"]', 1);
