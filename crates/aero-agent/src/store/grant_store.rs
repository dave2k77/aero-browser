//! Private N06 transactions: grant issue, revocation and authorization.
use super::*;
use crate::capabilities::{Authorization, Mismatch, ResolvedEffect, ReviewReason, evaluate};
use crate::policy::allowed_target;
use aero_core::capability::CapabilityGrant;

/// Live (unrevoked) grants per task.
pub const MAX_LIVE_GRANTS: usize = 64;

pub(super) fn now_ms() -> Result<u64, StoreError> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Io)?
        .as_millis();
    u64::try_from(millis).map_err(|_| StoreError::Io)
}

/// The task exists (else `Deleted` or `NotFound`).
pub(super) fn task_exists(c: &Connection, task: &TaskId) -> Result<(), StoreError> {
    let exists: bool = sql(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
        [task.as_str()],
        |r| r.get(0),
    ))?;
    if exists {
        Ok(())
    } else if tombstone_state(c, task)?.is_some() {
        Err(StoreError::Deleted)
    } else {
        Err(StoreError::NotFound)
    }
}

/// The task's live grants and their quota use, in issue order, each
/// revalidated against its stored contract revision.
fn live_grants(c: &Connection, task: &TaskId) -> Result<Vec<(CapabilityGrant, u64)>, StoreError> {
    let mut statement = sql(c.prepare(
        "SELECT id,contract_revision,expires_at,quota_used,substr(CAST(grant_json AS BLOB),1,?2) \
         FROM capability_grants WHERE task_id=?1 AND revoked=0 ORDER BY rowid LIMIT ?3",
    ))?;
    let mut rows = sql(statement.query(params![
        task.as_str(),
        READ_LIMIT,
        integer(MAX_LIVE_GRANTS as u64 + 1)?
    ]))?;
    let mut grants = Vec::new();
    while let Some(row) = sql(rows.next())? {
        if grants.len() == MAX_LIVE_GRANTS {
            return Err(StoreError::CorruptDatabase);
        }
        let id: String = sql(row.get(0))?;
        let revision = sql(counter(row, 1))?;
        let expires: String = sql(row.get(2))?;
        let used = sql(counter(row, 3))?;
        let json = String::from_utf8(sql(row.get::<_, Vec<u8>>(4))?)
            .map_err(|_| StoreError::CorruptDatabase)?;
        let grant: CapabilityGrant = decode(&json)?;
        let contract = read_task_contract(c, task, revision)?;
        if grant.id != id
            || grant.task_id != *task
            || grant.contract_revision != revision
            || grant.expires_at_ms.to_string() != expires
            || used > grant.quota
            || grant.validate_against(&contract).is_err()
        {
            return Err(StoreError::CorruptDatabase);
        }
        grants.push((grant, used));
    }
    Ok(grants)
}

impl Store {
    pub(super) fn issue_grant(
        &mut self,
        grant: CapabilityGrant,
        issuer: &'static str,
    ) -> Result<(), StoreError> {
        let json = encode(&grant)?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        task_exists(&tx, &grant.task_id)?;
        task_not_completed(&tx, &grant.task_id)?;
        if latest_revision(&tx, &grant.task_id)? != grant.contract_revision {
            return Err(StoreError::Conflict);
        }
        let contract = read_task_contract(&tx, &grant.task_id, grant.contract_revision)?;
        grant
            .validate_against(&contract)
            .map_err(|_| StoreError::InvalidInput)?;
        if grant.expires_at_ms <= now_ms()? {
            return Err(StoreError::InvalidInput);
        }
        if live_grants(&tx, &grant.task_id)?.len() >= MAX_LIVE_GRANTS {
            return Err(StoreError::Conflict);
        }
        let exists: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM capability_grants WHERE id=?1)",
            [grant.id.as_str()],
            |r| r.get(0),
        ))?;
        if exists {
            return Err(StoreError::Conflict);
        }
        sql(tx.execute(
            "INSERT INTO capability_grants(id,task_id,contract_revision,issued_by,grant_json,expires_at) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                grant.id,
                grant.task_id.as_str(),
                integer(grant.contract_revision)?,
                issuer,
                json,
                grant.expires_at_ms.to_string()
            ],
        ))?;
        sql(tx.commit())
    }

    pub(super) fn revoke_grant(&mut self, task: &TaskId, id: &str) -> Result<(), StoreError> {
        let changed = sql(self.connection.execute(
            "UPDATE capability_grants SET revoked=1 WHERE task_id=?1 AND id=?2 AND revoked=0",
            params![task.as_str(), id],
        ))?;
        if changed == 1 {
            Ok(())
        } else {
            Err(StoreError::NotFound)
        }
    }

    pub(super) fn authorize_effect(
        &mut self,
        effect: ResolvedEffect,
        allow_private_network: bool,
    ) -> Result<Authorization, StoreError> {
        if let Err(why) = allowed_target(&effect.target, allow_private_network) {
            return Ok(Authorization::Denied(why));
        }
        if effect.effect.is_none() {
            return Ok(Authorization::ReviewRequired(ReviewReason::UnknownEffect));
        }
        let now = now_ms()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        task_exists(&tx, &effect.task_id)?;
        let latest = latest_revision(&tx, &effect.task_id)?;
        let grants = live_grants(&tx, &effect.task_id)?;
        let mut closest: Option<Mismatch> = None;
        for (grant, used) in &grants {
            match evaluate(grant, *used, &effect, latest, now) {
                Ok(()) => {
                    let changed = sql(tx.execute(
                        "UPDATE capability_grants SET quota_used=quota_used+1 \
                         WHERE id=?1 AND revoked=0 AND quota_used=?2",
                        params![grant.id, integer(*used)?],
                    ))?;
                    if changed != 1 {
                        return Err(StoreError::CorruptDatabase);
                    }
                    sql(tx.commit())?;
                    return Ok(Authorization::Allowed {
                        grant_id: grant.id.clone(),
                        remaining: grant.quota - used - 1,
                    });
                }
                Err(mismatch) => closest = closest.max(Some(mismatch)),
            }
        }
        Ok(Authorization::ReviewRequired(match closest {
            None => ReviewReason::NoGrant,
            Some(mismatch) => ReviewReason::Boundary(mismatch),
        }))
    }
}
