-- Module 5: Risk & Control Library.

CREATE TABLE LibraryRisk (
    id                           TEXT PRIMARY KEY,
    code                         TEXT NOT NULL,
    title                        TEXT NOT NULL,
    description                  TEXT NOT NULL,
    applicable_system_types_json TEXT,
    default_inherent_rating      TEXT,
    library_version              TEXT NOT NULL,
    superseded_by                TEXT,
    UNIQUE (code, library_version),
    FOREIGN KEY (superseded_by) REFERENCES LibraryRisk(id)
);

CREATE TABLE LibraryControl (
    id                           TEXT PRIMARY KEY,
    code                         TEXT NOT NULL,
    title                        TEXT NOT NULL,
    description                  TEXT NOT NULL,
    objective                    TEXT NOT NULL,
    applicable_system_types_json TEXT,
    control_type                 TEXT NOT NULL,
    frequency                    TEXT,
    related_risk_ids_json        TEXT,
    library_version              TEXT NOT NULL,
    superseded_by                TEXT,
    UNIQUE (code, library_version),
    FOREIGN KEY (superseded_by) REFERENCES LibraryControl(id)
);

CREATE TABLE ExpectedEvidenceChecklist (
    id                  TEXT PRIMARY KEY,
    test_procedure_id   TEXT,
    items_json          TEXT NOT NULL,
    library_version     TEXT NOT NULL
);

CREATE TABLE TestProcedure (
    id                              TEXT PRIMARY KEY,
    control_id                      TEXT NOT NULL,
    code                            TEXT NOT NULL,
    name                            TEXT NOT NULL,
    objective                       TEXT NOT NULL,
    steps_json                      TEXT NOT NULL,
    expected_evidence_checklist_id  TEXT,
    sampling_default                TEXT NOT NULL,
    automation_hint                 TEXT NOT NULL,
    library_version                 TEXT NOT NULL,
    UNIQUE (code, library_version),
    FOREIGN KEY (control_id) REFERENCES LibraryControl(id),
    FOREIGN KEY (expected_evidence_checklist_id) REFERENCES ExpectedEvidenceChecklist(id)
);

CREATE TABLE FrameworkMapping (
    id                  TEXT PRIMARY KEY,
    entity_type         TEXT NOT NULL,
    entity_id           TEXT NOT NULL,
    framework           TEXT NOT NULL,
    reference           TEXT NOT NULL,
    library_version     TEXT NOT NULL
);
CREATE INDEX idx_FrameworkMapping_entity ON FrameworkMapping (entity_type, entity_id);
CREATE INDEX idx_FrameworkMapping_framework ON FrameworkMapping (framework, reference);

CREATE TABLE FirmOverride (
    id                      TEXT PRIMARY KEY,
    firm_id                 TEXT NOT NULL,
    base_entity_type        TEXT NOT NULL,
    base_entity_code        TEXT NOT NULL,
    base_library_version    TEXT NOT NULL,
    override_json           TEXT NOT NULL,
    disabled                INTEGER NOT NULL DEFAULT 0,
    created_by              TEXT,
    created_at              INTEGER NOT NULL,
    FOREIGN KEY (firm_id) REFERENCES Firm(id),
    FOREIGN KEY (created_by) REFERENCES User(id)
);
CREATE INDEX idx_FirmOverride_firm_entity ON FirmOverride (firm_id, base_entity_type, base_entity_code);
