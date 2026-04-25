-- Extend Finding with the CCCER (Condition, Criteria, Cause, Effect,
-- Recommendation) blob split. The MVP shipped with just condition_text and
-- recommendation_text because elevation produces those two straight from the
-- matcher; the working-paper editor needs the full quintet so auditors can
-- record the standard breached, the root cause, and the consequence.
--
-- All three new columns are nullable — existing draft findings remain valid
-- and pick up the new fields once an auditor opens them for editing.

ALTER TABLE Finding ADD COLUMN criteria_text TEXT;
ALTER TABLE Finding ADD COLUMN cause_text TEXT;
ALTER TABLE Finding ADD COLUMN effect_text TEXT;
