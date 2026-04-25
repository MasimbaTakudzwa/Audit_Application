-- Modules 6 + 8 (minimal subset): enough tables to run a rule-based user
-- access review end-to-end. Sampling, Connector, WorkingPaper, and the
-- fuller Finding CCCER blob set arrive in later migrations alongside the
-- flows that actually exercise them.

-- Module 6: engagement-level methodology clones.

CREATE TABLE EngagementRisk (
    id                           TEXT PRIMARY KEY,
    engagement_id                TEXT NOT NULL,
    derived_from                 TEXT,
    source_library_version       TEXT,
    prior_engagement_risk_id     TEXT,
    code                         TEXT NOT NULL,
    title                        TEXT NOT NULL,
    description                  TEXT NOT NULL,
    inherent_rating              TEXT NOT NULL,
    residual_rating              TEXT,
    applicable_system_ids_json   TEXT,
    notes_blob_id                TEXT,
    created_by                   TEXT,
    created_at                   INTEGER NOT NULL,
    UNIQUE (engagement_id, code),
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (derived_from) REFERENCES LibraryRisk(id),
    FOREIGN KEY (prior_engagement_risk_id) REFERENCES EngagementRisk(id),
    FOREIGN KEY (notes_blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (created_by) REFERENCES User(id)
);
CREATE INDEX idx_EngagementRisk_engagement ON EngagementRisk (engagement_id);

CREATE TABLE EngagementControl (
    id                                 TEXT PRIMARY KEY,
    engagement_id                      TEXT NOT NULL,
    derived_from                       TEXT,
    source_library_version             TEXT,
    prior_engagement_control_id        TEXT,
    code                               TEXT NOT NULL,
    title                              TEXT NOT NULL,
    description                        TEXT NOT NULL,
    objective                          TEXT NOT NULL,
    control_type                       TEXT NOT NULL,
    frequency                          TEXT,
    design_assessment                  TEXT,
    operating_assessment               TEXT,
    related_engagement_risk_ids_json   TEXT,
    applicable_system_ids_json         TEXT,
    notes_blob_id                      TEXT,
    created_by                         TEXT,
    created_at                         INTEGER NOT NULL,
    UNIQUE (engagement_id, code),
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (derived_from) REFERENCES LibraryControl(id),
    FOREIGN KEY (prior_engagement_control_id) REFERENCES EngagementControl(id),
    FOREIGN KEY (notes_blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (created_by) REFERENCES User(id)
);
CREATE INDEX idx_EngagementControl_engagement ON EngagementControl (engagement_id);

CREATE TABLE Test (
    id                       TEXT PRIMARY KEY,
    engagement_id            TEXT NOT NULL,
    engagement_control_id    TEXT NOT NULL,
    system_id                TEXT,
    derived_from             TEXT,
    source_library_version   TEXT,
    prior_test_id            TEXT,
    code                     TEXT NOT NULL,
    name                     TEXT NOT NULL,
    objective                TEXT NOT NULL,
    steps_json               TEXT NOT NULL,
    automation_tier          TEXT NOT NULL,
    assigned_to              TEXT,
    status                   TEXT NOT NULL DEFAULT 'not_started',
    planned_start_date       TEXT,
    planned_end_date         TEXT,
    actual_started_at        INTEGER,
    actual_completed_at      INTEGER,
    notes_blob_id            TEXT,
    created_by               TEXT,
    created_at               INTEGER NOT NULL,
    UNIQUE (engagement_id, code, system_id),
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (engagement_control_id) REFERENCES EngagementControl(id),
    FOREIGN KEY (system_id) REFERENCES System(id),
    FOREIGN KEY (derived_from) REFERENCES TestProcedure(id),
    FOREIGN KEY (prior_test_id) REFERENCES Test(id),
    FOREIGN KEY (assigned_to) REFERENCES User(id),
    FOREIGN KEY (notes_blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (created_by) REFERENCES User(id)
);
CREATE INDEX idx_Test_engagement_status ON Test (engagement_id, status);
CREATE INDEX idx_Test_engagement_control ON Test (engagement_control_id);

CREATE TABLE DataImport (
    id                   TEXT PRIMARY KEY,
    engagement_id        TEXT NOT NULL,
    system_id            TEXT,
    connector_id         TEXT,
    source_kind          TEXT NOT NULL,
    filename             TEXT,
    blob_id              TEXT,
    row_count            INTEGER,
    sha256_plaintext     TEXT NOT NULL,
    schema_json          TEXT,
    purpose_tag          TEXT,
    imported_by          TEXT,
    imported_at          INTEGER NOT NULL,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (system_id) REFERENCES System(id),
    FOREIGN KEY (blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (imported_by) REFERENCES User(id)
);
CREATE INDEX idx_DataImport_engagement ON DataImport (engagement_id);
CREATE INDEX idx_DataImport_purpose ON DataImport (engagement_id, purpose_tag);

CREATE TABLE TestResult (
    id                   TEXT PRIMARY KEY,
    test_id              TEXT NOT NULL,
    sample_id            TEXT,
    outcome              TEXT NOT NULL,
    exception_summary    TEXT,
    evidence_count       INTEGER NOT NULL DEFAULT 0,
    performed_by         TEXT,
    performed_at         INTEGER NOT NULL,
    notes_blob_id        TEXT,
    population_ref       TEXT,
    population_ref_label TEXT,
    detail_json          TEXT,
    FOREIGN KEY (test_id) REFERENCES Test(id),
    FOREIGN KEY (performed_by) REFERENCES User(id),
    FOREIGN KEY (notes_blob_id) REFERENCES EncryptedBlob(id)
);
CREATE INDEX idx_TestResult_test ON TestResult (test_id);
CREATE INDEX idx_TestResult_test_outcome ON TestResult (test_id, outcome);

-- Module 8: findings.
-- MVP carries condition/recommendation as inline text; the full CCCER blob
-- split arrives when the finding-drafting UI needs it. `Finding.test_id` is
-- nullable to accommodate engagement-level findings that don't tie to a
-- single test.

CREATE TABLE FindingSeverity (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL UNIQUE,
    sort_order   INTEGER NOT NULL,
    description  TEXT,
    is_builtin   INTEGER NOT NULL DEFAULT 1
);

INSERT INTO FindingSeverity (id, name, sort_order, description, is_builtin) VALUES
    ('sev-critical',    'Critical',    1, 'Immediate risk of material misstatement or significant control failure', 1),
    ('sev-high',        'High',        2, 'Significant control deficiency requiring timely remediation',             1),
    ('sev-medium',      'Medium',      3, 'Control deficiency with moderate impact; remediation expected',           1),
    ('sev-low',         'Low',         4, 'Minor control weakness; remediation at management discretion',            1),
    ('sev-observation', 'Observation', 5, 'Process improvement suggestion; not a control deficiency',                1);

CREATE TABLE Finding (
    id                     TEXT PRIMARY KEY,
    engagement_id          TEXT NOT NULL,
    test_id                TEXT,
    engagement_control_id  TEXT,
    code                   TEXT NOT NULL,
    title                  TEXT NOT NULL,
    condition_text         TEXT,
    recommendation_text    TEXT,
    severity_id            TEXT,
    status                 TEXT NOT NULL DEFAULT 'draft',
    identified_by          TEXT,
    identified_at          INTEGER NOT NULL,
    first_communicated_at  INTEGER,
    closed_at              INTEGER,
    UNIQUE (engagement_id, code),
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (test_id) REFERENCES Test(id),
    FOREIGN KEY (engagement_control_id) REFERENCES EngagementControl(id),
    FOREIGN KEY (severity_id) REFERENCES FindingSeverity(id),
    FOREIGN KEY (identified_by) REFERENCES User(id)
);
CREATE INDEX idx_Finding_engagement_status ON Finding (engagement_id, status);
CREATE INDEX idx_Finding_engagement_control ON Finding (engagement_control_id);

CREATE TABLE FindingTestResultLink (
    id             TEXT PRIMARY KEY,
    finding_id     TEXT NOT NULL,
    test_result_id TEXT NOT NULL,
    UNIQUE (finding_id, test_result_id),
    FOREIGN KEY (finding_id) REFERENCES Finding(id),
    FOREIGN KEY (test_result_id) REFERENCES TestResult(id)
);
CREATE INDEX idx_FindingTestResultLink_result ON FindingTestResultLink (test_result_id);
