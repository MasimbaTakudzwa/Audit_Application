-- Module 7: Evidence. Minimal subset — Evidence + Test/Finding link tables
-- plus a provenance chain. PBC, PriorYearEvidenceLink, and EvidenceTag are
-- deferred until the flows that exercise them land.

CREATE TABLE Evidence (
    id                     TEXT PRIMARY KEY,
    engagement_id          TEXT NOT NULL,
    test_id                TEXT,                          -- primary test linkage (may be null for engagement-level evidence)
    test_result_id         TEXT,                          -- set when evidence is produced by a specific test run
    engagement_control_id  TEXT,                          -- convenience denormalisation for listing
    blob_id                TEXT NOT NULL,                 -- decrypted on demand via blobs::read_blob
    data_import_id         TEXT,                          -- set when the evidence wraps a DataImport
    title                  TEXT NOT NULL,
    description            TEXT,
    source                 TEXT NOT NULL,                 -- auditor_upload | data_import | matcher_report | client_portal | prior_year_link
    obtained_at            INTEGER NOT NULL,
    obtained_from          TEXT,                          -- contact name / system reference
    created_by             TEXT,
    created_at             INTEGER NOT NULL,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (test_id) REFERENCES Test(id),
    FOREIGN KEY (test_result_id) REFERENCES TestResult(id),
    FOREIGN KEY (engagement_control_id) REFERENCES EngagementControl(id),
    FOREIGN KEY (blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (data_import_id) REFERENCES DataImport(id),
    FOREIGN KEY (created_by) REFERENCES User(id)
);
CREATE INDEX idx_Evidence_engagement ON Evidence (engagement_id);
CREATE INDEX idx_Evidence_test ON Evidence (test_id);
CREATE INDEX idx_Evidence_test_result ON Evidence (test_result_id);
CREATE INDEX idx_Evidence_data_import ON Evidence (data_import_id);

-- Secondary (non-primary) test-evidence links. The primary linkage is
-- Evidence.test_id; this table lets a single piece of evidence back several
-- tests without duplicating the row.
CREATE TABLE TestEvidenceLink (
    id          TEXT PRIMARY KEY,
    test_id     TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    relevance   TEXT NOT NULL DEFAULT 'supporting',     -- primary | supporting | cross_reference
    created_at  INTEGER NOT NULL,
    UNIQUE (test_id, evidence_id),
    FOREIGN KEY (test_id) REFERENCES Test(id),
    FOREIGN KEY (evidence_id) REFERENCES Evidence(id)
);
CREATE INDEX idx_TestEvidenceLink_evidence ON TestEvidenceLink (evidence_id);

CREATE TABLE FindingEvidenceLink (
    id          TEXT PRIMARY KEY,
    finding_id  TEXT NOT NULL,
    evidence_id TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    UNIQUE (finding_id, evidence_id),
    FOREIGN KEY (finding_id) REFERENCES Finding(id),
    FOREIGN KEY (evidence_id) REFERENCES Evidence(id)
);
CREATE INDEX idx_FindingEvidenceLink_evidence ON FindingEvidenceLink (evidence_id);

-- Append-only chain of custody. chain_ordinal starts at 1 (origin upload /
-- import) and increments on each transformation (OCR, extraction, redaction,
-- prior-year link). Reviewers walk this chain to see how evidence reached
-- its current form.
CREATE TABLE EvidenceProvenance (
    id             TEXT PRIMARY KEY,
    evidence_id    TEXT NOT NULL,
    chain_ordinal  INTEGER NOT NULL,
    action         TEXT NOT NULL,                        -- uploaded | data_import | matcher_report | ocrd | extracted | redacted | prior_year_linked
    actor_type     TEXT NOT NULL,                        -- user | system | portal_user
    actor_id       TEXT,
    occurred_at    INTEGER NOT NULL,
    detail_json    TEXT,
    UNIQUE (evidence_id, chain_ordinal),
    FOREIGN KEY (evidence_id) REFERENCES Evidence(id)
);
CREATE INDEX idx_EvidenceProvenance_evidence ON EvidenceProvenance (evidence_id);
