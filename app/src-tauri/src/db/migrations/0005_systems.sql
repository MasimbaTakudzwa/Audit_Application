-- Module 4: System Inventory.

CREATE TABLE SystemTemplate (
    id                       TEXT PRIMARY KEY,
    name                     TEXT NOT NULL,
    type                     TEXT NOT NULL,
    default_control_ids_json TEXT,
    default_test_ids_json    TEXT,
    ui_hints_json            TEXT,
    library_version          TEXT NOT NULL,
    is_builtin               INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE System (
    id                  TEXT PRIMARY KEY,
    engagement_id       TEXT NOT NULL,
    name                TEXT NOT NULL,
    type                TEXT NOT NULL,
    template_id         TEXT,
    environment         TEXT NOT NULL DEFAULT 'prod',
    criticality         TEXT NOT NULL DEFAULT 'medium',
    business_owner      TEXT,
    it_owner            TEXT,
    metadata_json       TEXT,
    derived_from        TEXT,
    created_at          INTEGER NOT NULL,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (template_id) REFERENCES SystemTemplate(id),
    FOREIGN KEY (derived_from) REFERENCES System(id)
);

CREATE TABLE CustomSystem (
    id                          TEXT PRIMARY KEY,
    system_id                   TEXT NOT NULL,
    architecture_notes_blob_id  TEXT,
    data_flow_diagram_blob_id   TEXT,
    FOREIGN KEY (system_id) REFERENCES System(id),
    FOREIGN KEY (architecture_notes_blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (data_flow_diagram_blob_id) REFERENCES EncryptedBlob(id)
);

-- Seed a small set of built-in system templates. Full catalogue ships via library bundle.
INSERT INTO SystemTemplate (id, name, type, library_version, is_builtin) VALUES
    ('tmpl-ad',       'Active Directory',   'AD',           '0.1.0', 1),
    ('tmpl-entra',    'Microsoft Entra ID', 'Entra',        '0.1.0', 1),
    ('tmpl-sap-ecc',  'SAP ECC',            'SAP',          '0.1.0', 1),
    ('tmpl-oracle',   'Oracle EBS',         'Oracle_EBS',   '0.1.0', 1),
    ('tmpl-sql',      'Microsoft SQL',      'SQL',          '0.1.0', 1),
    ('tmpl-oracledb', 'Oracle Database',    'Oracle_DB',    '0.1.0', 1),
    ('tmpl-t24',      'Temenos T24',        'core_banking', '0.1.0', 1);
