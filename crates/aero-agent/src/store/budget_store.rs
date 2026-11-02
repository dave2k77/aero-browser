//! Private N03 transactions; all work and browser-action paths share Usage.
use super::*;
use crate::budget::{BudgetReceipt, ReservationState};

/// Validate the aggregate, not merely each individual hold against the same
/// counter. Iteration is bounded by the admission limits and retains no history.
pub(super) fn validate_holds(c: &Connection, task: &TaskId, u: &Usage) -> Result<bool, StoreError> {
    let mut uncertain_inference = false;
    let mut totals = [0u64; 6];
    let mut sum = |r: Resources, charges: &Charges| -> Result<(), StoreError> {
        r.validate().map_err(|_| StoreError::CorruptDatabase)?;
        let (inference, purchase) = charges
            .quote(r, &u.currency)
            .map_err(|_| StoreError::CorruptDatabase)?;
        for (total, value) in totals.iter_mut().zip([
            r.input_tokens,
            r.output_tokens,
            r.active_ms,
            inference.unwrap_or(0),
            purchase,
            u64::from(inference.is_none()),
        ]) {
            *total = add(*total, value).map_err(|_| StoreError::CorruptDatabase)?;
        }
        Ok(())
    };
    let mut statement=sql(c.prepare("SELECT b.id,b.session_id,s.contract_revision,substr(b.reservation_json,1,?2),b.state FROM budget_reservations b JOIN sessions s ON s.id=b.session_id WHERE b.task_id=?1 AND b.state IN ('reserved','uncertain') LIMIT 2"))?;
    let mut rows = sql(statement.query(params![task.as_str(), READ_LIMIT]))?;
    let mut actions = 0;
    while let Some(row) = sql(rows.next())? {
        actions += 1;
        if actions > 1 {
            return Err(StoreError::CorruptDatabase);
        }
        let r: Reservation = decode(&sql(row.get::<_, String>(3))?)?;
        let id: String = sql(row.get(0))?;
        let run: String = sql(row.get(1))?;
        let revision = sql(counter(row, 2))?;
        let state: String = sql(row.get(4))?;
        context_valid(&r.context).map_err(|_| StoreError::CorruptDatabase)?;
        if r.schema_version != 1
            || r.step == 0
            || r.step >= MAX_COUNTER
            || r.context.task_id != *task
            || r.context.run_id.as_str() != run
            || r.context.contract_revision != revision
            || id != reservation_id(&r.context, r.step)
        {
            return Err(StoreError::CorruptDatabase);
        }
        let action = read_action(c, &r.context, r.step)?;
        if (state == "reserved" && action.state != ActionState::Prepared)
            || (state == "uncertain" && action.state != ActionState::Uncertain)
        {
            return Err(StoreError::CorruptDatabase);
        }
        sum(r.reserve, &r.charges)?;
        uncertain_inference |= state == "uncertain"
            && (!matches!(&r.charges.inference, crate::budget::Pricing::NoInference)
                || action
                    .reported_overrun
                    .as_ref()
                    .and_then(|report| report.actual)
                    .is_some_and(|actual| actual.input_tokens > 0 || actual.output_tokens > 0));
    }
    let pending_actions:u64=sql(c.query_row("SELECT count(*) FROM actions a JOIN sessions s ON s.id=a.session_id WHERE s.task_id=?1 AND a.state IN ('prepared','uncertain')",[task.as_str()],|r|counter(r,0)))?;
    if pending_actions != actions {
        return Err(StoreError::CorruptDatabase);
    }
    let mut statement=sql(c.prepare("SELECT session_id,contract_revision,id FROM work_reservations WHERE task_id=?1 AND state IN ('reserved','uncertain') LIMIT 65"))?;
    let mut rows = sql(statement.query([task.as_str()]))?;
    let mut work = 0;
    while let Some(row) = sql(rows.next())? {
        work += 1;
        if work > 64 || actions > 0 {
            return Err(StoreError::CorruptDatabase);
        }
        let ticket = BudgetTicket {
            context: RunContext {
                task_id: task.clone(),
                run_id: aero_core::ids::RunId::from(sql(row.get::<_, String>(0))?),
                contract_revision: sql(counter(row, 1))?,
            },
            id: sql(row.get(2))?,
        };
        let (request, receipt) = read_work(c, &ticket)?;
        uncertain_inference |= receipt.state == ReservationState::Uncertain
            && (!matches!(&request.pricing, crate::budget::Pricing::NoInference)
                || receipt
                    .reported_overrun
                    .is_some_and(|actual| actual.input_tokens > 0 || actual.output_tokens > 0));
        sum(
            request.reserve,
            &Charges {
                inference: request.pricing,
                purchase: None,
            },
        )?;
    }
    if totals
        != [
            u.input_reserved,
            u.output_reserved,
            u.active_reserved,
            u.inference_reserved,
            u.purchase_reserved,
            u.unknown_reserved,
        ]
    {
        return Err(StoreError::CorruptDatabase);
    }
    Ok(uncertain_inference)
}

/// Recompute spent values from task-owned, versioned settled records. This
/// detects inconsistent storage, not coordinated tampering with both copies.
/// History is streamed with bounded per-record memory, but total time grows
/// with task history; no hard wall-clock deadline is implied.
pub(super) fn validate_spent(c: &Connection, task: &TaskId, u: &Usage) -> Result<(), StoreError> {
    let mut totals = [0u64; 6];
    let mut sum = |actual: Resources,
                   pricing: &crate::budget::Pricing,
                   purchase: u64|
     -> Result<(), StoreError> {
        let inference = pricing
            .quote(actual, &u.currency)
            .map_err(|_| StoreError::CorruptDatabase)?;
        for (total, value) in totals.iter_mut().zip([
            actual.input_tokens,
            actual.output_tokens,
            actual.active_ms,
            inference.unwrap_or(0),
            purchase,
            u64::from(inference.is_none()),
        ]) {
            *total = add(*total, value).map_err(|_| StoreError::CorruptDatabase)?;
        }
        Ok(())
    };
    let action_count: u64 = sql(c.query_row(
        "SELECT count(*) FROM actions a JOIN sessions s ON s.id=a.session_id WHERE s.task_id=?1",
        [task.as_str()],
        |r| counter(r, 0),
    ))?;
    let reservation_count: u64 = sql(c.query_row(
        "SELECT count(*) FROM budget_reservations WHERE task_id=?1",
        [task.as_str()],
        |r| counter(r, 0),
    ))?;
    if action_count != u.attempts || reservation_count != action_count {
        return Err(StoreError::CorruptDatabase);
    }
    let mut statement = sql(c.prepare("SELECT b.id,b.session_id,s.contract_revision,substr(CAST(b.reservation_json AS BLOB),1,?2),substr(CAST(b.settlement_json AS BLOB),1,?2) FROM budget_reservations b JOIN sessions s ON s.id=b.session_id AND s.task_id=b.task_id WHERE b.task_id=?1 AND b.state='settled'"))?;
    let mut rows = sql(statement.query(params![task.as_str(), READ_LIMIT]))?;
    while let Some(row) = sql(rows.next())? {
        let id: String = sql(row.get(0))?;
        let run: String = sql(row.get(1))?;
        let revision = sql(counter(row, 2))?;
        let reservation_bytes: Vec<u8> = sql(row.get(3))?;
        let settlement_bytes: Vec<u8> = sql(row.get(4))?;
        let reservation: Reservation = decode(
            &String::from_utf8(reservation_bytes).map_err(|_| StoreError::CorruptDatabase)?,
        )?;
        let settlement: ActionSettlement =
            decode(&String::from_utf8(settlement_bytes).map_err(|_| StoreError::CorruptDatabase)?)?;
        context_valid(&reservation.context).map_err(|_| StoreError::CorruptDatabase)?;
        if reservation.schema_version != 1
            || reservation.step == 0
            || reservation.step >= MAX_COUNTER
            || reservation.context.task_id != *task
            || reservation.context.run_id.as_str() != run
            || reservation.context.contract_revision != revision
            || id != reservation_id(&reservation.context, reservation.step)
            || settlement.schema_version != 1
        {
            return Err(StoreError::CorruptDatabase);
        }
        let action = read_action(c, &reservation.context, reservation.step)
            .map_err(|_| StoreError::CorruptDatabase)?;
        if !matches!(action.state, ActionState::Committed | ActionState::Failed) {
            return Err(StoreError::CorruptDatabase);
        }
        within_reservation(settlement.actual, reservation.reserve)
            .map_err(|_| StoreError::CorruptDatabase)?;
        let (_, purchase) = reservation
            .charges
            .quote(reservation.reserve, &u.currency)
            .map_err(|_| StoreError::CorruptDatabase)?;
        if settlement.purchase_actual > purchase {
            return Err(StoreError::CorruptDatabase);
        }
        sum(
            settlement.actual,
            &reservation.charges.inference,
            settlement.purchase_actual,
        )?;
    }
    let mut statement = sql(c.prepare("SELECT session_id,contract_revision,id FROM work_reservations WHERE task_id=?1 AND state='settled'"))?;
    let mut rows = sql(statement.query([task.as_str()]))?;
    while let Some(row) = sql(rows.next())? {
        let ticket = BudgetTicket {
            context: RunContext {
                task_id: task.clone(),
                run_id: aero_core::ids::RunId::from(sql(row.get::<_, String>(0))?),
                contract_revision: sql(counter(row, 1))?,
            },
            id: sql(row.get(2))?,
        };
        let (request, receipt) = read_work(c, &ticket).map_err(|_| StoreError::CorruptDatabase)?;
        let actual = receipt.actual.ok_or(StoreError::CorruptDatabase)?;
        if receipt.state != ReservationState::Settled {
            return Err(StoreError::CorruptDatabase);
        }
        sum(actual, &request.pricing, 0)?;
    }
    if totals
        != [
            u.input_spent,
            u.output_spent,
            u.active_spent,
            u.inference_spent,
            u.purchase_spent,
            u.unknown_spent,
        ]
    {
        return Err(StoreError::CorruptDatabase);
    }
    Ok(())
}

impl Usage {
    pub(super) fn snapshot(&self) -> Result<BudgetSnapshot, StoreError> {
        let known = add(self.inference_spent, self.inference_reserved)?;
        let unknown = add(self.unknown_spent, self.unknown_reserved)?;
        add(self.purchase_spent, self.purchase_reserved)?;
        add(self.active_spent, self.active_reserved)?;
        Ok(BudgetSnapshot {
            currency: self.currency.clone(),
            unknown_inference_spent_count: self.unknown_spent,
            unknown_inference_reserved_count: self.unknown_reserved,
            uncertain_inference: self.uncertain_inference,
            spent: Resources {
                input_tokens: self.input_spent,
                output_tokens: self.output_spent,
                active_ms: self.active_spent,
            },
            reserved: Resources {
                input_tokens: self.input_reserved,
                output_tokens: self.output_reserved,
                active_ms: self.active_reserved,
            },
            action_attempts: self.attempts,
            known_inference_spent_upper_bound: self.inference_spent,
            known_inference_reserved_upper_bound: self.inference_reserved,
            total_inference_upper_bound: if unknown == 0 && !self.uncertain_inference {
                Some(known)
            } else {
                None
            },
            purchase_spent: self.purchase_spent,
            purchase_reserved: self.purchase_reserved,
        })
    }
    pub(super) fn money_within(&self, c: &TaskContract) -> Result<(), StoreError> {
        self.snapshot()?;
        if add(self.purchase_spent, self.purchase_reserved)? > c.budgets.max_purchase_minor_units {
            return Err(StoreError::BudgetExceeded);
        }
        if let Some(limit) = c.budgets.max_inference_minor_units {
            if add(self.unknown_spent, self.unknown_reserved)? > 0 {
                return Err(StoreError::UnknownPricing);
            }
            let total = add(self.inference_spent, self.inference_reserved)?;
            if total > limit {
                return Err(StoreError::BudgetExceeded);
            }
        }
        Ok(())
    }
    pub(super) fn reserve(
        &mut self,
        r: Resources,
        charges: &Charges,
        c: &TaskContract,
    ) -> Result<(), StoreError> {
        let (inference, purchase) = charges.quote(r, &c.budgets.budget_currency)?;
        self.input_reserved = add(self.input_reserved, r.input_tokens)?;
        self.output_reserved = add(self.output_reserved, r.output_tokens)?;
        self.active_reserved = add(self.active_reserved, r.active_ms)?;
        self.inference_reserved = add(self.inference_reserved, inference.unwrap_or(0))?;
        self.unknown_reserved = add(self.unknown_reserved, u64::from(inference.is_none()))?;
        self.purchase_reserved = add(self.purchase_reserved, purchase)?;
        self.within(c)
    }
    pub(super) fn contains(
        &self,
        r: Resources,
        charges: &Charges,
        c: &TaskContract,
    ) -> Result<(), StoreError> {
        let (inference, purchase) = charges
            .quote(r, &c.budgets.budget_currency)
            .map_err(|_| StoreError::CorruptDatabase)?;
        if self.input_reserved < r.input_tokens
            || self.output_reserved < r.output_tokens
            || self.active_reserved < r.active_ms
            || self.inference_reserved < inference.unwrap_or(0)
            || self.unknown_reserved < u64::from(inference.is_none())
            || self.purchase_reserved < purchase
        {
            return Err(StoreError::CorruptDatabase);
        }
        Ok(())
    }
    pub(super) fn settle(
        &mut self,
        reserved: Resources,
        charges: &Charges,
        actual: Resources,
        purchase_actual: u64,
        c: &TaskContract,
    ) -> Result<(), StoreError> {
        self.contains(reserved, charges, c)?;
        within_reservation(actual, reserved)?;
        let (inference, purchase) = charges.quote(reserved, &c.budgets.budget_currency)?;
        if purchase_actual > purchase {
            return Err(StoreError::BudgetExceeded);
        }
        let actual_inference = charges
            .inference
            .quote(actual, &c.budgets.budget_currency)?;
        self.input_reserved = sub(self.input_reserved, reserved.input_tokens)?;
        self.output_reserved = sub(self.output_reserved, reserved.output_tokens)?;
        self.active_reserved = sub(self.active_reserved, reserved.active_ms)?;
        self.inference_reserved = sub(self.inference_reserved, inference.unwrap_or(0))?;
        self.unknown_reserved = sub(self.unknown_reserved, u64::from(inference.is_none()))?;
        self.purchase_reserved = sub(self.purchase_reserved, purchase)?;
        self.input_spent = add(self.input_spent, actual.input_tokens)?;
        self.output_spent = add(self.output_spent, actual.output_tokens)?;
        self.active_spent = add(self.active_spent, actual.active_ms)?;
        self.inference_spent = add(self.inference_spent, actual_inference.unwrap_or(0))?;
        self.unknown_spent = add(self.unknown_spent, u64::from(actual_inference.is_none()))?;
        self.purchase_spent = add(self.purchase_spent, purchase_actual)?;
        self.within(c)
    }
}
fn within_reservation(actual: Resources, reserved: Resources) -> Result<(), StoreError> {
    actual.validate()?;
    if actual.input_tokens > reserved.input_tokens
        || actual.output_tokens > reserved.output_tokens
        || actual.active_ms > reserved.active_ms
    {
        return Err(StoreError::BudgetExceeded);
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkRecord {
    schema_version: u32,
    request: BudgetRequest,
}
fn read_work(
    c: &Connection,
    ticket: &BudgetTicket,
) -> Result<(BudgetRequest, BudgetReceipt), StoreError> {
    let (run,revision,json,state,actual,overrun):(String,u64,String,String,Option<String>,Option<String>)=sql(c.query_row("SELECT session_id,contract_revision,substr(CAST(request_json AS BLOB),1,?3),state,substr(actual_json,1,?3),substr(overrun_json,1,?3) FROM work_reservations WHERE task_id=?1 AND id=?2",params![ticket.context.task_id.as_str(),ticket.id,READ_LIMIT],|r|{
        let bytes:Vec<u8>=r.get(2)?;
        let json=String::from_utf8(bytes).map_err(|_|rusqlite::Error::InvalidQuery)?;
        Ok((r.get(0)?,counter(r,1)?,json,r.get(3)?,r.get(4)?,r.get(5)?))
    }).optional())?.ok_or(StoreError::NotFound)?;
    if run != ticket.context.run_id.as_str() || revision != ticket.context.contract_revision {
        return Err(StoreError::OwnershipMismatch);
    }
    let record: WorkRecord = decode(&json)?;
    let r = record.request;
    r.validate().map_err(|_| StoreError::CorruptDatabase)?;
    if record.schema_version != 1 || r.context != ticket.context || r.id != ticket.id {
        return Err(StoreError::CorruptDatabase);
    }
    let (contract, _, _) = read_run(c, &ticket.context)?;
    let quote = r
        .pricing
        .quote(r.reserve, &contract.budgets.budget_currency)
        .map_err(|_| StoreError::CorruptDatabase)?;
    let actual: Option<Resources> = actual.map(|s| decode(&s)).transpose()?;
    if let Some(a) = actual {
        within_reservation(a, r.reserve).map_err(|_| StoreError::CorruptDatabase)?;
    }
    let state = match (state.as_str(), actual.is_some()) {
        ("reserved", false) => ReservationState::Reserved,
        ("uncertain", false) => ReservationState::Uncertain,
        ("settled", true) => ReservationState::Settled,
        _ => return Err(StoreError::CorruptDatabase),
    };
    let reported_overrun: Option<Resources> = overrun.map(|s| decode(&s)).transpose()?;
    if reported_overrun.is_some_and(|a| within_reservation(a, r.reserve).is_ok())
        || (reported_overrun.is_some() && state != ReservationState::Uncertain)
    {
        return Err(StoreError::CorruptDatabase);
    }
    let receipt = BudgetReceipt {
        state,
        reserved: r.reserve,
        actual,
        inference_reserved_upper_bound: quote,
        reported_overrun,
    };
    Ok((r, receipt))
}
fn mark_review(c: &Connection, context: &RunContext) -> Result<(), StoreError> {
    let now = stamp()?;
    sql(c.execute(
        "UPDATE tasks SET status='needs_review',updated_at=?2 WHERE id=?1",
        params![context.task_id.as_str(), now],
    ))?;
    sql(c.execute(
        "UPDATE sessions SET status='needs_review',updated_at=?2 WHERE id=?1",
        params![context.run_id.as_str(), now],
    ))?;
    Ok(())
}
impl Store {
    pub(super) fn budget_history(
        &self,
        context: RunContext,
        after: Option<String>,
        limit: usize,
    ) -> Result<crate::budget::BudgetPage, StoreError> {
        read_run(&self.connection, &context)?;
        read_usage(&self.connection, &context.task_id)?;
        let mut statement=sql(self.connection.prepare("SELECT id,session_id,contract_revision FROM work_reservations WHERE task_id=?1 AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT ?3"))?;
        let rows = sql(statement.query_map(
            params![context.task_id.as_str(), after, (limit + 1) as i64],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    counter(r, 2)?,
                ))
            },
        ))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(database_error)?;
        let has_more = rows.len() > limit;
        let entries = rows
            .into_iter()
            .take(limit)
            .map(|(id, run, revision)| {
                let ticket = BudgetTicket {
                    context: RunContext {
                        task_id: context.task_id.clone(),
                        run_id: aero_core::ids::RunId::from(run),
                        contract_revision: revision,
                    },
                    id,
                };
                let receipt = read_work(&self.connection, &ticket)?.1;
                Ok(crate::budget::BudgetHistoryEntry { ticket, receipt })
            })
            .collect::<Result<_, StoreError>>()?;
        Ok(crate::budget::BudgetPage { entries, has_more })
    }
    pub(super) fn validate_budget_holds(&self) -> Result<(), StoreError> {
        let mut statement = sql(self
            .connection
            .prepare("SELECT substr(CAST(id AS BLOB),1,257) FROM tasks"))?;
        let mut rows = sql(statement.query([]))?;
        while let Some(row) = sql(rows.next())? {
            let bytes: Vec<u8> = sql(row.get(0))?;
            let id = String::from_utf8(bytes).map_err(|_| StoreError::CorruptDatabase)?;
            identifier(&id).map_err(|_| StoreError::CorruptDatabase)?;
            read_usage(&self.connection, &TaskId::from(id))?;
        }
        Ok(())
    }
    pub(super) fn reserve_budget(
        &mut self,
        request: BudgetRequest,
    ) -> Result<BudgetTicket, StoreError> {
        request.validate()?;
        let json = encode(&WorkRecord {
            schema_version: 1,
            request: request.clone(),
        })?;
        let now = stamp()?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (contract, _, status) = read_run(&tx, &request.context)?;
        if !matches!(status.as_str(), "running" | "paused")
            || latest_revision(&tx, &request.context.task_id)? != request.context.contract_revision
        {
            return Err(StoreError::Conflict);
        }
        let blocked:bool=sql(tx.query_row("SELECT EXISTS(SELECT 1 FROM actions a JOIN sessions s ON s.id=a.session_id WHERE s.task_id=?1 AND a.state IN ('prepared','uncertain')) OR EXISTS(SELECT 1 FROM budget_reservations WHERE task_id=?1 AND state IN ('reserved','uncertain')) OR EXISTS(SELECT 1 FROM work_reservations WHERE task_id=?1 AND state='uncertain') OR EXISTS(SELECT 1 FROM tasks WHERE id=?1 AND status='needs_review')",[request.context.task_id.as_str()],|r|r.get(0)))?;
        if blocked {
            return Err(StoreError::NeedsReview);
        }
        let duplicate: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM work_reservations WHERE task_id=?1 AND id=?2)",
            params![request.context.task_id.as_str(), request.id],
            |r| r.get(0),
        ))?;
        if duplicate {
            return Err(StoreError::Conflict);
        }
        let count: u64 = sql(tx.query_row(
            "SELECT count(*) FROM work_reservations WHERE task_id=?1 AND state='reserved'",
            [request.context.task_id.as_str()],
            |r| counter(r, 0),
        ))?;
        if count >= 64 {
            return Err(StoreError::Conflict);
        }
        let mut usage = read_usage(&tx, &request.context.task_id)?;
        usage.reserve(
            request.reserve,
            &Charges {
                inference: request.pricing.clone(),
                purchase: None,
            },
            &contract,
        )?;
        write_usage(&tx, &request.context.task_id, &usage, &now)?;
        sql(tx.execute(
            "INSERT INTO work_reservations(task_id,session_id,contract_revision,id,request_json,state,actual_json,created_at) VALUES (?1,?2,?3,?4,?5,'reserved',NULL,?6)",
            params![
                request.context.task_id.as_str(),
                request.context.run_id.as_str(),
                integer(request.context.contract_revision)?,
                request.id,
                json,
                now
            ],
        ))?;
        sql(tx.commit())?;
        Ok(BudgetTicket {
            context: request.context,
            id: request.id,
        })
    }
    pub(super) fn settle_budget(
        &mut self,
        ticket: BudgetTicket,
        actual: Option<Resources>,
    ) -> Result<(), StoreError> {
        context_valid(&ticket.context)?;
        identifier(&ticket.id)?;
        let tx = sql(self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (request, receipt) = read_work(&tx, &ticket)?;
        if receipt.state != ReservationState::Reserved {
            return Err(StoreError::Conflict);
        }
        let (contract, _, _) = read_run(&tx, &ticket.context)?;
        let charges = Charges {
            inference: request.pricing,
            purchase: None,
        };
        let mut usage = read_usage(&tx, &ticket.context.task_id)?;
        usage.contains(request.reserve, &charges, &contract)?;
        let overrun = actual.is_some_and(|a| within_reservation(a, request.reserve).is_err());
        let reported_overrun = if overrun {
            actual.map(|r| encode(&r)).transpose()?
        } else {
            None
        };
        let actual = if overrun { None } else { actual };
        if let Some(actual) = actual {
            usage.settle(request.reserve, &charges, actual, 0, &contract)?;
            write_usage(&tx, &ticket.context.task_id, &usage, &stamp()?)?;
        } else {
            mark_review(&tx, &ticket.context)?;
        }
        sql(tx.execute(
            "UPDATE work_reservations SET state=?3,actual_json=?4,overrun_json=?5 WHERE task_id=?1 AND id=?2",
            params![
                ticket.context.task_id.as_str(),
                ticket.id,
                if actual.is_some() {
                    "settled"
                } else {
                    "uncertain"
                },
                actual.map(|r| encode(&r)).transpose()?,
                reported_overrun,
            ],
        ))?;
        sql(tx.commit())?;
        if overrun {
            Err(StoreError::BudgetExceeded)
        } else {
            Ok(())
        }
    }
    pub(super) fn budget_receipt(&self, ticket: BudgetTicket) -> Result<BudgetReceipt, StoreError> {
        let receipt = read_work(&self.connection, &ticket)?.1;
        read_usage(&self.connection, &ticket.context.task_id)?;
        Ok(receipt)
    }
    pub(super) fn recover_work(&mut self) -> Result<(), StoreError> {
        loop {
            let tx = sql(self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            let row:Option<(String,String,u64,String)>=sql(tx.query_row("SELECT task_id,session_id,contract_revision,id FROM work_reservations WHERE state='reserved' ORDER BY task_id,id LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,counter(r,2)?,r.get(3)?))).optional())?;
            let Some((task, run, revision, id)) = row else {
                sql(tx.commit())?;
                return Ok(());
            };
            let ticket = BudgetTicket {
                context: RunContext {
                    task_id: TaskId::from(task),
                    run_id: aero_core::ids::RunId::from(run),
                    contract_revision: revision,
                },
                id,
            };
            let (request, _) = read_work(&tx, &ticket)?;
            let (contract, _, _) = read_run(&tx, &ticket.context)?;
            read_usage(&tx, &ticket.context.task_id)?.contains(
                request.reserve,
                &Charges {
                    inference: request.pricing,
                    purchase: None,
                },
                &contract,
            )?;
            sql(tx.execute(
                "UPDATE work_reservations SET state='uncertain' WHERE task_id=?1 AND id=?2",
                params![ticket.context.task_id.as_str(), ticket.id],
            ))?;
            mark_review(&tx, &ticket.context)?;
            sql(tx.commit())?;
        }
    }
}
