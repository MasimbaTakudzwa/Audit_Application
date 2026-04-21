-- Cross-cutting: reviewer-facing audit trail.

CREATE TABLE ActivityLog (
    id                  TEXT PRIMARY KEY,
    engagement_id       TEXT NOT NULL,
    entity_type         TEXT NOT NULL,
    entity_id           TEXT NOT NULL,
    action              TEXT NOT NULL,
    performed_by        TEXT,
    performed_at        INTEGER NOT NULL,
    summary             TEXT,
    detail_json         TEXT,
    FOREIGN KEY (engagement_id) REFERENCES Engagement(id),
    FOREIGN KEY (performed_by) REFERENCES User(id)
);
CREATE INDEX idx_ActivityLog_engagement_time ON ActivityLog (engagement_id, performed_at);
CREATE INDEX idx_ActivityLog_entity_time ON ActivityLog (entity_type, entity_id, performed_at);
