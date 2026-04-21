-- Module 3: Engagement Core.

CREATE TABLE EngagementStatus (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    sort_order          INTEGER NOT NULL,
    is_terminal         INTEGER NOT NULL DEFAULT 0,
    is_builtin          INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE Engagement (
    id                       TEXT PRIMARY KEY,
    client_id                TEXT NOT NULL,
    name                     TEXT NOT NULL,
    period_id                TEXT,
    status_id                TEXT NOT NULL,
    prior_engagement_id      TEXT,
    library_version_at_start TEXT NOT NULL,
    encryption_key_id        TEXT NOT NULL,
    lead_partner_id          TEXT,
    created_at               INTEGER NOT NULL,
    closed_at                INTEGER,
    archive_bundle_blob_id   TEXT,
    FOREIGN KEY (client_id) REFERENCES Client(id),
    FOREIGN KEY (status_id) REFERENCES EngagementStatus(id),
    FOREIGN KEY (prior_engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (encryption_key_id) REFERENCES KeychainEntry(id),
    FOREIGN KEY (lead_partner_id) REFERENCES User(id),
    FOREIGN KEY (archive_bundle_blob_id) REFERENCES EncryptedBlob(id)
);

CREATE TABLE EngagementPeriod (
    id                  TEXT PRIMARY KEY,
    engagement_id       TEXT NOT NULL,
    start_date          TEXT NOT NULL,
    end_date            TEXT NOT NULL,
    fiscal_year_label   TEXT NOT NULL,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id)
);

CREATE TABLE EngagementTeam (
    id                  TEXT PRIMARY KEY,
    engagement_id       TEXT NOT NULL,
    user_id             TEXT NOT NULL,
    team_role           TEXT NOT NULL,
    assigned_at         INTEGER NOT NULL,
    unassigned_at       INTEGER,
    UNIQUE (engagement_id, user_id),
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (user_id) REFERENCES User(id)
);

CREATE TABLE EngagementScope (
    id                      TEXT PRIMARY KEY,
    engagement_id           TEXT NOT NULL,
    scope_statement_blob_id TEXT,
    approved_by             TEXT,
    approved_at             INTEGER,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (scope_statement_blob_id) REFERENCES EncryptedBlob(id),
    FOREIGN KEY (approved_by) REFERENCES User(id)
);

CREATE TABLE EngagementBudget (
    id                  TEXT PRIMARY KEY,
    engagement_id       TEXT NOT NULL,
    total_hours         REAL NOT NULL,
    hours_by_role_json  TEXT,
    actual_hours        REAL NOT NULL DEFAULT 0,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id)
);

-- Seed built-in engagement statuses.
INSERT INTO EngagementStatus (id, name, sort_order, is_terminal, is_builtin) VALUES
    ('status-planning',  'Planning',  10, 0, 1),
    ('status-fieldwork', 'Fieldwork', 20, 0, 1),
    ('status-review',    'Review',    30, 0, 1),
    ('status-reporting', 'Reporting', 40, 0, 1),
    ('status-closed',    'Closed',    50, 1, 1);
