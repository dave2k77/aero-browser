//! N06: bounded delegation envelopes (REQ-GRANT-01, REQ-GRANT-03).

use aero_agent::capabilities::{
    Authorization, Mismatch, NetworkAccess, ResolvedEffect, ReviewReason,
};
use aero_agent::policy::UnsafeTarget;
use aero_agent::store::StoreError;
use aero_agent::store::worker::Journal;
use aero_agent::tasks::{TaskAuthority, TaskClient};
use aero_core::capability::{CapabilityGrant, GRANT_SCHEMA_VERSION};
use aero_core::ids::TaskId;
use aero_core::task::{Effect, RouteScope, TaskContract};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TASK: &str = "supplier-research";

fn directory() -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::tempdir().unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = parent.path().join("journal");
    (parent, path)
}

/// The plan's §7.10 fixture contract, with one account and one destination.
fn contract(revision: u64) -> TaskContract {
    let plan = include_str!("../../../docs/implementation-plan.md");
    let json = plan
        .split("### 7.10 TaskContract")
        .nth(1)
        .unwrap()
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    let mut contract: TaskContract = serde_json::from_str(json).unwrap();
    contract.task_id = TaskId::from(TASK);
    contract.revision = revision;
    contract.account_refs = vec!["buyer".into()];
    contract.export_grants = vec!["local-report".into()];
    contract
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// Navigate and extract on supplier A's catalogue, anonymously.
fn research_grant(id: &str, quota: u64) -> CapabilityGrant {
    CapabilityGrant {
        schema_version: GRANT_SCHEMA_VERSION,
        id: id.into(),
        task_id: TaskId::from(TASK),
        contract_revision: 1,
        routes: vec![RouteScope {
            origin: "https://supplier-a.test".into(),
            path_prefix: "/catalogue".into(),
        }],
        effects: vec![Effect::Navigate, Effect::Extract],
        account_ref: None,
        destinations: vec![],
        quota,
        expires_at_ms: now_ms() + 3_600_000,
    }
}

fn effect(kind: Option<Effect>, url: &str) -> ResolvedEffect {
    ResolvedEffect {
        task_id: TaskId::from(TASK),
        contract_revision: 1,
        effect: kind,
        target: url::Url::parse(url).unwrap(),
        account_ref: None,
        destination: None,
    }
}

struct Setup {
    _parent: tempfile::TempDir,
    path: PathBuf,
    journal: Option<Journal>,
}

impl Setup {
    fn new() -> Self {
        let (parent, path) = directory();
        let journal = Journal::start(&path, 8).unwrap();
        let setup = Self {
            _parent: parent,
            path,
            journal: Some(journal),
        };
        setup
            .owner()
            .create_task(contract(1))
            .unwrap()
            .blocking_recv()
            .unwrap();
        setup
    }
    fn authority(&self) -> TaskAuthority {
        TaskAuthority::new(self.journal.as_ref().unwrap())
    }
    fn owner(&self) -> TaskClient {
        self.authority().trusted_user()
    }
    fn issue(&self, grant: CapabilityGrant) -> Result<(), StoreError> {
        self.owner().issue_grant(grant)?.blocking_recv()
    }
    fn authorize(&self, effect: ResolvedEffect) -> Authorization {
        self.authority()
            .capabilities(NetworkAccess::PublicOnly)
            .authorize(effect)
            .unwrap()
            .blocking_recv()
            .unwrap()
    }
    fn reopen(&mut self) {
        self.journal.take().unwrap().close().unwrap();
        self.journal = Some(Journal::start(Path::new(&self.path), 8).unwrap());
    }
}

fn allowed(remaining: u64) -> Authorization {
    Authorization::Allowed {
        grant_id: "research".into(),
        remaining,
    }
}

fn boundary(mismatch: Mismatch) -> Authorization {
    Authorization::ReviewRequired(ReviewReason::Boundary(mismatch))
}

#[test]
fn research_grant_avoids_per_click_prompts() {
    let setup = Setup::new();
    setup.issue(research_grant("research", 10)).unwrap();
    let steps = [
        (Effect::Navigate, "https://supplier-a.test/catalogue"),
        (Effect::Extract, "https://supplier-a.test/catalogue"),
        (
            Effect::Navigate,
            "https://supplier-a.test/catalogue/widgets?page=2",
        ),
        (
            Effect::Extract,
            "https://supplier-a.test/catalogue/widgets?page=2#lead",
        ),
        (
            Effect::Navigate,
            "https://SUPPLIER-A.test:443/catalogue/a/../b",
        ),
    ];
    for (index, (kind, url)) in steps.into_iter().enumerate() {
        assert_eq!(
            setup.authorize(effect(Some(kind), url)),
            allowed(10 - 1 - index as u64),
            "{url}"
        );
    }
}

#[test]
fn boundary_crossing_requires_review() {
    let setup = Setup::new();
    // No grant yet: even routine navigation needs review.
    assert_eq!(
        setup.authorize(effect(
            Some(Effect::Navigate),
            "https://supplier-a.test/catalogue"
        )),
        Authorization::ReviewRequired(ReviewReason::NoGrant)
    );
    setup.issue(research_grant("research", 3)).unwrap();
    // Another origin, a route outside the prefix (including look-alike and
    // encoded-separator paths), another effect class, an account, a
    // destination.
    for url in [
        "https://supplier-b.test/catalogue",
        "http://supplier-a.test/catalogue",
        "https://supplier-a.test:8443/catalogue",
        "https://supplier-a.test/",
        "https://supplier-a.test/catalogue-admin",
        "https://supplier-a.test/catalogue/..%2fadmin",
        "https://supplier-a.test/catalogue%2f../admin",
        "https://supplier-a.test/catalogue/%2E%2E/admin",
    ] {
        assert_eq!(
            setup.authorize(effect(Some(Effect::Navigate), url)),
            boundary(Mismatch::Route),
            "{url}"
        );
    }
    let inside = "https://supplier-a.test/catalogue/item";
    // StageArtifact is allowed by the contract but not by this grant; submit
    // and purchase are never in an envelope.
    for kind in [
        Effect::StageArtifact,
        Effect::Submit,
        Effect::Purchase,
        Effect::Download,
    ] {
        assert_eq!(
            setup.authorize(effect(Some(kind), inside)),
            boundary(Mismatch::EffectClass),
            "{kind:?}"
        );
    }
    let mut as_buyer = effect(Some(Effect::Navigate), inside);
    as_buyer.account_ref = Some("buyer".into());
    assert_eq!(setup.authorize(as_buyer), boundary(Mismatch::Account));
    let mut exporting = effect(Some(Effect::Extract), inside);
    exporting.destination = Some("local-report".into());
    assert_eq!(setup.authorize(exporting), boundary(Mismatch::Destination));
    // What the host cannot classify (a submit-shaped click, autosave) is
    // never inside an envelope, whatever the model claims.
    assert_eq!(
        setup.authorize(effect(None, inside)),
        Authorization::ReviewRequired(ReviewReason::UnknownEffect)
    );
    // Unsafe targets are refused outright, granted or not.
    assert_eq!(
        setup.authorize(effect(
            Some(Effect::Navigate),
            "http://169.254.169.254/latest"
        )),
        Authorization::Denied(UnsafeTarget::PrivateAddress)
    );
    assert_eq!(
        setup.authorize(effect(Some(Effect::Navigate), "file:///etc/passwd")),
        Authorization::Denied(UnsafeTarget::Scheme)
    );
    // None of the refusals used quota: three allowed, then exhausted.
    for remaining in [2, 1, 0] {
        assert_eq!(
            setup.authorize(effect(Some(Effect::Extract), inside)),
            allowed(remaining)
        );
    }
    assert_eq!(
        setup.authorize(effect(Some(Effect::Extract), inside)),
        boundary(Mismatch::QuotaExhausted)
    );
}

#[test]
fn model_cannot_grant_its_own_capability() {
    let setup = Setup::new();
    let model = setup
        .authority()
        .model_for_task(TaskId::from(TASK))
        .unwrap();
    assert_eq!(
        model.issue_grant(research_grant("research", 10)).err(),
        Some(StoreError::PermissionDenied)
    );
    setup.issue(research_grant("research", 10)).unwrap();
    assert_eq!(
        model
            .revoke_grant(TaskId::from(TASK), "research".into())
            .err(),
        Some(StoreError::PermissionDenied)
    );
    // Nothing the model attempted took effect: the owner's grant still works.
    assert_eq!(
        setup.authorize(effect(
            Some(Effect::Navigate),
            "https://supplier-a.test/catalogue"
        )),
        allowed(9)
    );
}

#[test]
fn expired_or_revised_grant_is_rejected() {
    let setup = Setup::new();
    let inside = || effect(Some(Effect::Navigate), "https://supplier-a.test/catalogue");
    let mut brief = research_grant("brief", 10);
    brief.expires_at_ms = now_ms() + 150;
    setup.issue(brief).unwrap();
    assert!(matches!(
        setup.authorize(inside()),
        Authorization::Allowed { .. }
    ));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(setup.authorize(inside()), boundary(Mismatch::Expired));

    setup.issue(research_grant("research", 10)).unwrap();
    assert_eq!(setup.authorize(inside()), allowed(9));
    // An action planned under an older revision is stale even with a grant.
    setup
        .owner()
        .revise_contract(contract(2))
        .unwrap()
        .blocking_recv()
        .unwrap();
    // Revising revoked every grant; old-revision effects and new ones alike
    // need a fresh grant.
    assert_eq!(
        setup.authorize(inside()),
        Authorization::ReviewRequired(ReviewReason::NoGrant)
    );
    let mut current = inside();
    current.contract_revision = 2;
    assert_eq!(
        setup.authorize(current),
        Authorization::ReviewRequired(ReviewReason::NoGrant)
    );
    // A grant for the superseded revision cannot be issued.
    assert_eq!(
        setup.issue(research_grant("old-revision", 10)),
        Err(StoreError::Conflict)
    );
    let mut renewed = research_grant("renewed", 10);
    renewed.contract_revision = 2;
    setup.issue(renewed).unwrap();
    assert_eq!(setup.authorize(inside()), boundary(Mismatch::StaleRevision));
    let mut current = inside();
    current.contract_revision = 2;
    assert!(matches!(
        setup.authorize(current),
        Authorization::Allowed { .. }
    ));
    // An explicit revocation ends a grant at once.
    setup
        .owner()
        .revoke_grant(TaskId::from(TASK), "renewed".into())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut current = inside();
    current.contract_revision = 2;
    assert_eq!(
        setup.authorize(current),
        Authorization::ReviewRequired(ReviewReason::NoGrant)
    );
}

#[test]
fn grant_quota_survives_restart() {
    let mut setup = Setup::new();
    let inside = || effect(Some(Effect::Extract), "https://supplier-a.test/catalogue/x");
    setup.issue(research_grant("research", 2)).unwrap();
    assert_eq!(setup.authorize(inside()), allowed(1));
    setup.reopen();
    assert_eq!(setup.authorize(inside()), allowed(0));
    setup.reopen();
    assert_eq!(
        setup.authorize(inside()),
        boundary(Mismatch::QuotaExhausted)
    );
}

#[test]
fn grants_stay_within_their_contract() {
    let setup = Setup::new();
    let invalid = |change: &dyn Fn(&mut CapabilityGrant)| {
        let mut grant = research_grant("candidate", 10);
        change(&mut grant);
        setup.issue(grant)
    };
    for (what, change) in [
        (
            "origin outside the contract",
            &(|g: &mut CapabilityGrant| g.routes[0].origin = "https://supplier-c.test".into())
                as &dyn Fn(&mut CapabilityGrant),
        ),
        ("malformed route prefix", &|g| {
            g.routes[0].path_prefix = "catalogue".into()
        }),
        ("no routes", &|g| g.routes.clear()),
        ("forbidden effect", &|g| g.effects.push(Effect::Submit)),
        ("effect the contract does not allow", &|g| {
            g.effects.push(Effect::Download)
        }),
        ("commit effect", &|g| g.effects = vec![Effect::Export]),
        ("duplicate effect", &|g| g.effects.push(Effect::Navigate)),
        ("no effects", &|g| g.effects.clear()),
        ("unknown account", &|g| {
            g.account_ref = Some("someone-else".into())
        }),
        ("unknown destination", &|g| {
            g.destinations = vec!["elsewhere".into()]
        }),
        ("zero quota", &|g| g.quota = 0),
        ("excessive quota", &|g| g.quota = 10_001),
        ("already expired", &|g| g.expires_at_ms = now_ms() - 1),
        ("bad id", &|g| g.id = "has space".into()),
        ("wrong schema", &|g| g.schema_version = 2),
    ] {
        assert_eq!(invalid(change), Err(StoreError::InvalidInput), "{what}");
    }
    assert_eq!(
        invalid(&|g| g.task_id = TaskId::from("no-such-task")),
        Err(StoreError::NotFound)
    );
    // A narrower route, the contract's account and destination are fine.
    let mut grant = research_grant("buyer", 10);
    grant.account_ref = Some("buyer".into());
    grant.destinations = vec!["local-report".into()];
    grant.effects.push(Effect::StageArtifact);
    setup.issue(grant).unwrap();
    assert_eq!(
        setup.issue(research_grant("buyer", 10)),
        Err(StoreError::Conflict),
        "grant IDs are never reused"
    );
}

#[test]
fn grant_payloads_cannot_be_rewritten_in_place() {
    let mut setup = Setup::new();
    setup.issue(research_grant("research", 10)).unwrap();
    setup.journal.take().unwrap().close().unwrap();
    let raw = rusqlite::Connection::open(setup.path.join("journal.sqlite3")).unwrap();
    // The payload is immutable in the schema itself.
    assert!(
        raw.execute(
            "UPDATE capability_grants SET grant_json=replace(grant_json,'supplier-a','supplier-b')",
            [],
        )
        .is_err()
    );
    // Removing that protection to widen a grant makes the journal refuse to
    // open rather than trust the rewritten row.
    raw.execute_batch(
        "DROP TRIGGER immutable_grant_payload;
         UPDATE capability_grants SET grant_json=replace(grant_json,'supplier-a','supplier-b');",
    )
    .unwrap();
    drop(raw);
    assert_eq!(
        Journal::start(&setup.path, 8).err(),
        Some(StoreError::SchemaMismatch)
    );
}

#[test]
fn each_containment_rule_holds_on_its_own() {
    let setup = Setup::new();
    // A contract whose routes are narrower than whole origins and which
    // allows effects an envelope must still not cover.
    let mut narrow = contract(1);
    narrow.task_id = TaskId::from("narrow");
    narrow.allowed_routes = vec![RouteScope {
        origin: "https://supplier-a.test".into(),
        path_prefix: "/catalogue".into(),
    }];
    narrow.allowed_effects = vec![
        Effect::Navigate,
        Effect::Extract,
        Effect::Download,
        Effect::Export,
    ];
    setup
        .owner()
        .create_task(narrow)
        .unwrap()
        .blocking_recv()
        .unwrap();
    let grant = |change: &dyn Fn(&mut CapabilityGrant)| {
        let mut grant = research_grant("narrow-grant", 10);
        grant.task_id = TaskId::from("narrow");
        change(&mut grant);
        setup.issue(grant)
    };
    for (what, change) in [
        (
            "route outside the contract's prefix",
            &(|g: &mut CapabilityGrant| g.routes[0].path_prefix = "/admin".into())
                as &dyn Fn(&mut CapabilityGrant),
        ),
        ("route wider than the contract's prefix", &|g| {
            g.routes[0].path_prefix = "/".into()
        }),
        ("grantable effect the contract does not allow", &|g| {
            g.effects = vec![Effect::StageArtifact]
        }),
        ("allowed download, not envelope-grantable", &|g| {
            g.effects = vec![Effect::Download]
        }),
        ("allowed export, not envelope-grantable", &|g| {
            g.effects = vec![Effect::Export]
        }),
    ] {
        assert_eq!(grant(change), Err(StoreError::InvalidInput), "{what}");
    }
    grant(&|g| g.routes[0].path_prefix = "/catalogue/widgets".into()).unwrap();
}
