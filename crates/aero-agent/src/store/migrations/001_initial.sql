-- Unshipped schema v1. The opener wraps this entire script in one transaction.
-- JSON checks establish syntactic objects only, not trusted N01 semantics/grants.
CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    status TEXT NOT NULL CHECK(status IN ('draft','ready','running','awaiting_user','paused','needs_review','completed','failed','cancelled')),
    attempts_spent INTEGER NOT NULL DEFAULT 0 CHECK(attempts_spent >= 0),
    tokens_spent INTEGER NOT NULL DEFAULT 0 CHECK(tokens_spent >= 0),
    tokens_reserved INTEGER NOT NULL DEFAULT 0 CHECK(tokens_reserved >= 0),
    active_ms_spent INTEGER NOT NULL DEFAULT 0 CHECK(active_ms_spent >= 0),
    recovery_count INTEGER NOT NULL DEFAULT 0 CHECK(recovery_count >= 0),
    input_tokens_spent INTEGER NOT NULL DEFAULT 0 CHECK(input_tokens_spent >= 0),
    output_tokens_spent INTEGER NOT NULL DEFAULT 0 CHECK(output_tokens_spent >= 0),
    input_tokens_reserved INTEGER NOT NULL DEFAULT 0 CHECK(input_tokens_reserved >= 0),
    output_tokens_reserved INTEGER NOT NULL DEFAULT 0 CHECK(output_tokens_reserved >= 0),
    active_ms_reserved INTEGER NOT NULL DEFAULT 0 CHECK(active_ms_reserved >= 0),
    inference_spent INTEGER NOT NULL DEFAULT 0 CHECK(inference_spent >= 0),
    inference_reserved INTEGER NOT NULL DEFAULT 0 CHECK(inference_reserved >= 0),
    purchase_spent INTEGER NOT NULL DEFAULT 0 CHECK(purchase_spent >= 0),
    purchase_reserved INTEGER NOT NULL DEFAULT 0 CHECK(purchase_reserved >= 0),
    unknown_inference_spent INTEGER NOT NULL DEFAULT 0 CHECK(unknown_inference_spent >= 0),
    unknown_inference_reserved INTEGER NOT NULL DEFAULT 0 CHECK(unknown_inference_reserved >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(tokens_spent <= 9223372036854775807 - tokens_reserved),
    CHECK(input_tokens_spent <= tokens_spent AND output_tokens_spent = tokens_spent - input_tokens_spent),
    CHECK(input_tokens_reserved <= tokens_reserved AND output_tokens_reserved = tokens_reserved - input_tokens_reserved),
    CHECK(active_ms_spent <= 9223372036854775807 - active_ms_reserved),
    CHECK(inference_spent <= 9223372036854775807 - inference_reserved),
    CHECK(purchase_spent <= 9223372036854775807 - purchase_reserved),
    CHECK(unknown_inference_spent <= 9223372036854775807 - unknown_inference_reserved)
) STRICT;
CREATE TABLE task_contracts (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK(revision >= 1),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    contract_json TEXT NOT NULL CHECK(json_valid(contract_json) AND json_type(contract_json) = 'object'),
    created_at TEXT NOT NULL,
    PRIMARY KEY(task_id, revision)
) STRICT;
CREATE TRIGGER immutable_contract BEFORE UPDATE ON task_contracts
BEGIN SELECT RAISE(ABORT, 'task contract revisions are immutable'); END;
CREATE TRIGGER no_contract_replacement BEFORE INSERT ON task_contracts
WHEN EXISTS(SELECT 1 FROM task_contracts WHERE task_id=NEW.task_id AND revision=NEW.revision)
BEGIN SELECT RAISE(ABORT, 'task contract revision already exists'); END;
CREATE TRIGGER no_contract_delete BEFORE DELETE ON task_contracts
WHEN EXISTS(SELECT 1 FROM tasks WHERE id=OLD.task_id)
BEGIN SELECT RAISE(ABORT, 'delete the owning task, not its revision'); END;
CREATE TRIGGER no_task_replacement BEFORE INSERT ON tasks
WHEN EXISTS(SELECT 1 FROM tasks WHERE id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'task already exists'); END;
CREATE TRIGGER immutable_task_identity BEFORE UPDATE OF id ON tasks
BEGIN SELECT RAISE(ABORT, 'task identity is immutable'); END;
CREATE TABLE capability_grants (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    issued_by TEXT NOT NULL CHECK(length(issued_by) > 0),
    grant_json TEXT NOT NULL CHECK(json_valid(grant_json) AND json_type(grant_json) = 'object'),
    expires_at TEXT NOT NULL,
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1)),
    quota_used INTEGER NOT NULL DEFAULT 0 CHECK(quota_used >= 0),
    FOREIGN KEY(task_id, contract_revision) REFERENCES task_contracts(task_id, revision) ON DELETE CASCADE
) STRICT;
CREATE TRIGGER immutable_grant_payload
BEFORE UPDATE OF id, task_id, contract_revision, issued_by, grant_json, expires_at ON capability_grants
BEGIN SELECT RAISE(ABORT, 'capability grant payload is immutable'); END;
CREATE TRIGGER no_grant_replacement BEFORE INSERT ON capability_grants
WHEN EXISTS(SELECT 1 FROM capability_grants WHERE id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'grant already exists'); END;
CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    goal TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running','awaiting_user','paused','needs_review','completed','failed','cancelled')),
    next_step INTEGER NOT NULL DEFAULT 1 CHECK(next_step >= 1),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(id, task_id),
    UNIQUE(id, task_id, contract_revision),
    FOREIGN KEY(task_id, contract_revision) REFERENCES task_contracts(task_id, revision) ON DELETE CASCADE
) STRICT;
CREATE INDEX sessions_by_task_status ON sessions(task_id,status,id);
CREATE TRIGGER immutable_run_owner BEFORE UPDATE OF id,task_id,contract_revision ON sessions
BEGIN SELECT RAISE(ABORT, 'run ownership is immutable'); END;
CREATE TRIGGER no_run_replacement BEFORE INSERT ON sessions
WHEN EXISTS(SELECT 1 FROM sessions WHERE id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'run already exists'); END;
CREATE TABLE actions (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    step INTEGER NOT NULL CHECK(step >= 1),
    observation_id TEXT NOT NULL CHECK(length(observation_id) > 0),
    action_json TEXT NOT NULL CHECK(json_valid(action_json) AND json_type(action_json) = 'object'),
    state TEXT NOT NULL CHECK(state IN ('prepared','committed','uncertain','failed')),
    public_rationale TEXT NOT NULL,
    outcome_json TEXT CHECK(outcome_json IS NULL OR (json_valid(outcome_json) AND json_type(outcome_json) = 'object')),
    created_at TEXT NOT NULL,
    PRIMARY KEY(session_id, step)
) STRICT;
CREATE TABLE budget_reservations (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    task_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    reservation_json TEXT NOT NULL CHECK(json_valid(reservation_json) AND json_type(reservation_json) = 'object'),
    state TEXT NOT NULL CHECK(state IN ('reserved','settled','uncertain')),
    created_at TEXT NOT NULL,
    settlement_json TEXT CHECK(settlement_json IS NULL OR (json_valid(settlement_json) AND json_type(settlement_json) = 'object')),
    overrun_json TEXT CHECK(overrun_json IS NULL OR (json_valid(overrun_json) AND json_type(overrun_json) = 'object')),
    CHECK(overrun_json IS NULL OR state = 'uncertain'),
    CHECK((state = 'settled') = (settlement_json IS NOT NULL)),
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX action_reservations_by_task_state ON budget_reservations(task_id,state,session_id);
CREATE TRIGGER no_action_reservation_replacement BEFORE INSERT ON budget_reservations
WHEN EXISTS(SELECT 1 FROM budget_reservations WHERE id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'action reservation already exists'); END;
CREATE TRIGGER immutable_action_settlement BEFORE UPDATE OF settlement_json ON budget_reservations
WHEN OLD.settlement_json IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'action settlement is immutable'); END;
CREATE TRIGGER immutable_action_overrun BEFORE UPDATE OF overrun_json ON budget_reservations
WHEN OLD.overrun_json IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'action overrun report is immutable'); END;
CREATE TRIGGER no_action_reservation_delete BEFORE DELETE ON budget_reservations
WHEN EXISTS(SELECT 1 FROM tasks WHERE id=OLD.task_id)
BEGIN SELECT RAISE(ABORT, 'delete the owning task, not its reservation'); END;
CREATE TABLE work_reservations (
    task_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    id TEXT NOT NULL CHECK(length(id) > 0),
    request_json TEXT NOT NULL CHECK(json_valid(request_json) AND json_type(request_json) = 'object'),
    state TEXT NOT NULL CHECK(state IN ('reserved','settled','uncertain')),
    actual_json TEXT CHECK(actual_json IS NULL OR (json_valid(actual_json) AND json_type(actual_json) = 'object')),
    created_at TEXT NOT NULL,
    overrun_json TEXT CHECK(overrun_json IS NULL OR (json_valid(overrun_json) AND json_type(overrun_json) = 'object')),
    CHECK(overrun_json IS NULL OR state = 'uncertain'),
    PRIMARY KEY(task_id,id),
    FOREIGN KEY(session_id,task_id,contract_revision) REFERENCES sessions(id,task_id,contract_revision) ON DELETE CASCADE
) STRICT;
CREATE INDEX work_reservations_by_task_state ON work_reservations(task_id,state,session_id);
CREATE TRIGGER immutable_work_reservation BEFORE UPDATE OF task_id,session_id,contract_revision,id,request_json ON work_reservations
BEGIN SELECT RAISE(ABORT, 'work reservation identity is immutable'); END;
CREATE TRIGGER immutable_work_settlement BEFORE UPDATE OF actual_json,state ON work_reservations
WHEN OLD.state='settled'
BEGIN SELECT RAISE(ABORT, 'work settlement is immutable'); END;
CREATE TRIGGER immutable_work_overrun BEFORE UPDATE OF overrun_json ON work_reservations
WHEN OLD.overrun_json IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'work overrun report is immutable'); END;
CREATE TRIGGER no_work_reservation_delete BEFORE DELETE ON work_reservations
WHEN EXISTS(SELECT 1 FROM tasks WHERE id=OLD.task_id)
BEGIN SELECT RAISE(ABORT, 'delete the owning task, not its reservation'); END;
CREATE TRIGGER no_work_replacement BEFORE INSERT ON work_reservations
WHEN EXISTS(SELECT 1 FROM work_reservations WHERE task_id=NEW.task_id AND id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'work reservation already exists'); END;
CREATE TABLE checkpoints (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    step INTEGER NOT NULL CHECK(step >= 0),
    schema_version INTEGER NOT NULL CHECK(schema_version = 1),
    metadata_json TEXT NOT NULL CHECK(json_valid(metadata_json) AND json_type(metadata_json) = 'object'),
    snapshot_nonce BLOB,
    snapshot_ciphertext BLOB,
    key_id TEXT,
    created_at TEXT NOT NULL,
    CHECK((snapshot_ciphertext IS NULL AND snapshot_nonce IS NULL AND key_id IS NULL)
       OR (snapshot_ciphertext IS NOT NULL AND length(snapshot_ciphertext) > 0
           AND snapshot_nonce IS NOT NULL AND length(snapshot_nonce) > 0
           AND key_id IS NOT NULL AND length(key_id) > 0)),
    PRIMARY KEY(session_id, step)
) STRICT;
CREATE TABLE approvals (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    session_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    observation_id TEXT NOT NULL CHECK(length(observation_id) > 0),
    material_effect_digest TEXT NOT NULL CHECK(length(material_effect_digest) > 0),
    preview_json TEXT NOT NULL CHECK(json_valid(preview_json) AND json_type(preview_json) = 'object'),
    origin TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN (0,1)),
    FOREIGN KEY(session_id, task_id, contract_revision) REFERENCES sessions(id, task_id, contract_revision) ON DELETE CASCADE
) STRICT;
CREATE TABLE evidence (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    task_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL CHECK(json_valid(metadata_json) AND json_type(metadata_json) = 'object'),
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id) ON DELETE CASCADE
) STRICT;
CREATE TRIGGER immutable_evidence_record BEFORE UPDATE ON evidence
BEGIN SELECT RAISE(ABORT, 'evidence records are immutable'); END;
CREATE TRIGGER no_evidence_delete BEFORE DELETE ON evidence
WHEN EXISTS(SELECT 1 FROM tasks WHERE id=OLD.task_id)
BEGIN SELECT RAISE(ABORT, 'delete the owning task, not its evidence record'); END;
CREATE TRIGGER no_evidence_replacement BEFORE INSERT ON evidence
WHEN EXISTS(SELECT 1 FROM evidence WHERE id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'evidence record already exists'); END;
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY NOT NULL CHECK(length(id) > 0),
    task_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL CHECK(json_valid(metadata_json) AND json_type(metadata_json) = 'object'),
    created_at TEXT NOT NULL,
    FOREIGN KEY(session_id, task_id) REFERENCES sessions(id, task_id) ON DELETE CASCADE
) STRICT;
CREATE TRIGGER immutable_artifact_record BEFORE UPDATE ON artifacts
BEGIN SELECT RAISE(ABORT, 'artifact records are immutable'); END;
CREATE TRIGGER no_artifact_delete BEFORE DELETE ON artifacts
WHEN EXISTS(SELECT 1 FROM tasks WHERE id=OLD.task_id)
BEGIN SELECT RAISE(ABORT, 'delete the owning task, not its artifact record'); END;
CREATE TRIGGER no_artifact_replacement BEFORE INSERT ON artifacts
WHEN EXISTS(SELECT 1 FROM artifacts WHERE id=NEW.id)
BEGIN SELECT RAISE(ABORT, 'artifact record already exists'); END;
CREATE TABLE criterion_results (
    task_id TEXT NOT NULL,
    contract_revision INTEGER NOT NULL,
    criterion_id TEXT NOT NULL CHECK(length(criterion_id) > 0),
    session_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('not_checked','passed','failed','inconclusive')),
    result_json TEXT NOT NULL CHECK(json_valid(result_json) AND json_type(result_json) = 'object'),
    PRIMARY KEY(task_id, contract_revision, criterion_id, session_id),
    FOREIGN KEY(session_id, task_id, contract_revision) REFERENCES sessions(id, task_id, contract_revision) ON DELETE CASCADE
) STRICT;
PRAGMA application_id = 1095062095;
PRAGMA user_version = 1;
