//! Private N07 transactions: approval issue and single-use consumption.
use super::*;
use crate::approval::Refusal;
use crate::preview::Preview;
use grant_store::now_ms;

/// Run, task, revision, digest, preview JSON, origin, expiry, consumed.
type ApprovalRow = (String, String, u64, String, Vec<u8>, String, String, bool);

/// Run states in which an approval may be issued or consumed.
const ACTIVE_RUN: [&str; 3] = ["running", "awaiting_user", "paused"];

fn approval_id(id: &str) -> Result<(), StoreError> {
    if (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        Ok(())
    } else {
        Err(StoreError::InvalidInput)
    }
}

/// The run's task, revision and status.
fn run_state(c: &Connection, run: &str) -> Result<Option<(String, u64, String)>, StoreError> {
    sql(c
        .query_row(
            "SELECT task_id,contract_revision,status FROM sessions WHERE id=?1",
            [run],
            |r| Ok((r.get(0)?, counter(r, 1)?, r.get(2)?)),
        )
        .optional())
}

impl Store {
    pub(super) fn approve(
        &mut self,
        id: String,
        preview: Preview,
        expires_at_ms: u64,
    ) -> Result<(), StoreError> {
        approval_id(&id)?;
        preview.approvable().map_err(|_| StoreError::InvalidInput)?;
        let json = encode(&preview)?;
        let digest = preview.material_digest();
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        grant_store::task_exists(&tx, &preview.task_id)?;
        task_not_completed(&tx, &preview.task_id)?;
        if latest_revision(&tx, &preview.task_id)? != preview.contract_revision {
            return Err(StoreError::Conflict);
        }
        // Within the contract: an allowed, unforbidden effect (or an unknown
        // one, approved as such), an allowed origin, a contract account.
        let contract = read_task_contract(&tx, &preview.task_id, preview.contract_revision)?;
        if !preview.effect.is_none_or(|effect| {
            contract.allowed_effects.contains(&effect)
                && !contract.forbidden_effects.contains(&effect)
        }) || !contract.allowed_origins.contains(&preview.origin)
            || !preview
                .account_ref
                .as_ref()
                .is_none_or(|account| contract.account_refs.contains(account))
        {
            return Err(StoreError::InvalidInput);
        }
        let (task, revision, status) =
            run_state(&tx, preview.run_id.as_str())?.ok_or(StoreError::NotFound)?;
        if task != preview.task_id.as_str()
            || revision != preview.contract_revision
            || !ACTIVE_RUN.contains(&status.as_str())
        {
            return Err(StoreError::Conflict);
        }
        if expires_at_ms <= now_ms()? {
            return Err(StoreError::InvalidInput);
        }
        let exists: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM approvals WHERE id=?1)",
            [id.as_str()],
            |r| r.get(0),
        ))?;
        if exists {
            return Err(StoreError::Conflict);
        }
        sql(tx.execute(
            "INSERT INTO approvals(id,session_id,task_id,contract_revision,observation_id,\
             material_effect_digest,preview_json,origin,expires_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                id,
                preview.run_id.as_str(),
                preview.task_id.as_str(),
                integer(preview.contract_revision)?,
                preview.observation_id.as_str(),
                digest,
                json,
                preview.origin,
                expires_at_ms.to_string()
            ],
        ))?;
        sql(tx.commit())
    }

    /// Mark every unconsumed approval of the run consumed: the page may
    /// have changed under manual input since they were issued.
    pub(super) fn void_run_approvals(&mut self, context: &RunContext) -> Result<(), StoreError> {
        sql(self.connection.execute(
            "UPDATE approvals SET consumed=1 WHERE session_id=?1 AND task_id=?2 AND consumed=0",
            params![context.run_id.as_str(), context.task_id.as_str()],
        ))?;
        Ok(())
    }

    pub(super) fn consume_approval(
        &mut self,
        id: String,
        current: Preview,
    ) -> Result<Result<(), Refusal>, StoreError> {
        current.validate().map_err(|_| StoreError::InvalidInput)?;
        if approval_id(&id).is_err() {
            return Ok(Err(Refusal::NotFound));
        }
        let now = now_ms()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let row: Option<ApprovalRow> = sql(tx
            .query_row(
                "SELECT session_id,task_id,contract_revision,material_effect_digest,\
                 substr(CAST(preview_json AS BLOB),1,?2),origin,expires_at,consumed \
                 FROM approvals WHERE id=?1",
                params![id, READ_LIMIT],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        counter(r, 2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get::<_, i64>(7)? == 1,
                    ))
                },
            )
            .optional())?;
        let Some((run, task, revision, digest, json, origin, expires, consumed)) = row else {
            return Ok(Err(Refusal::NotFound));
        };
        if task != current.task_id.as_str() {
            return Ok(Err(Refusal::NotFound));
        }
        let approved: Preview =
            decode(&String::from_utf8(json).map_err(|_| StoreError::CorruptDatabase)?)?;
        if approved.validate().is_err()
            || approved.material_digest() != digest
            || approved.run_id.as_str() != run
            || approved.task_id.as_str() != task
            || approved.contract_revision != revision
            || approved.origin != origin
        {
            return Err(StoreError::CorruptDatabase);
        }
        let expires: u64 = expires.parse().map_err(|_| StoreError::CorruptDatabase)?;
        let refusal = refusal(&tx, &approved, &current, consumed, now >= expires)?;
        if let Some(refusal) = refusal {
            return Ok(Err(refusal));
        }
        if sql(tx.execute(
            "UPDATE approvals SET consumed=1 WHERE id=?1 AND consumed=0",
            [id.as_str()],
        ))? != 1
        {
            return Err(StoreError::CorruptDatabase);
        }
        sql(tx.commit())?;
        Ok(Ok(()))
    }
}

/// The first reason `current` may not use the approval of `approved`.
fn refusal(
    c: &Connection,
    approved: &Preview,
    current: &Preview,
    consumed: bool,
    expired: bool,
) -> Result<Option<Refusal>, StoreError> {
    if consumed {
        return Ok(Some(Refusal::Consumed));
    }
    if expired {
        return Ok(Some(Refusal::Expired));
    }
    let active = run_state(c, approved.run_id.as_str())?
        .is_some_and(|(_, _, status)| ACTIVE_RUN.contains(&status.as_str()));
    if current.run_id != approved.run_id || !active {
        return Ok(Some(Refusal::RunChanged));
    }
    if current.contract_revision != approved.contract_revision
        || latest_revision(c, &approved.task_id)? != approved.contract_revision
    {
        return Ok(Some(Refusal::StaleRevision));
    }
    if current.target.frame != approved.target.frame
        || current.target.document != approved.target.document
    {
        return Ok(Some(Refusal::StaleDocument));
    }
    if current.target.backend_node_id != approved.target.backend_node_id {
        return Ok(Some(Refusal::TargetChanged));
    }
    if current.origin != approved.origin {
        return Ok(Some(Refusal::OriginChanged));
    }
    if current.material_digest() != approved.material_digest() {
        return Ok(Some(Refusal::MaterialChanged));
    }
    Ok(None)
}
