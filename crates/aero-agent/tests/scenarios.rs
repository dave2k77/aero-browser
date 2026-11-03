//! Full-product browser scenarios (implementation plan §9). They compile only
//! with `--features browser-tests` and then require the pinned browser: a
//! missing prerequisite fails the test, it never skips.

#[cfg(feature = "browser-tests")]
mod support;

/// C07: the harness itself — a local fixture server and a sandboxed browser
/// are both reachable, and both are reaped on the normal and the failure path.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn fixture_harness_smoke() {
    use serde_json::json;
    use std::time::Duration;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");

    // The server answers the host directly.
    let response = support::http_get(fixture.server.address(), "/basic.html")
        .await
        .expect("fixture server reachable");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(response.contains("<title>AeroBrowser basic fixture</title>"));
    // `..` is refused even when it would lead back to a real fixture.
    for path in [
        "/../fixtures/basic.html",
        "/../../Cargo.toml",
        "/BASIC.html",
    ] {
        let refused = support::http_get(fixture.server.address(), path)
            .await
            .unwrap();
        assert!(refused.starts_with("HTTP/1.1 404"), "{path}: {refused}");
    }

    // The browser answers over CDP and loads the fixture from the server.
    let version = fixture
        .cdp()
        .call("Browser.getVersion", json!({}), None, support::CALL)
        .await
        .unwrap();
    assert!(version["product"].as_str().unwrap().contains("Chrome/"));
    let mut events = fixture.cdp().subscribe();
    fixture
        .cdp()
        .call(
            "Target.setDiscoverTargets",
            json!({ "discover": true }),
            None,
            support::CALL,
        )
        .await
        .unwrap();
    let url = fixture.server.url("basic.html");
    let created = fixture
        .cdp()
        .call(
            "Target.createTarget",
            json!({ "url": url }),
            None,
            support::CALL,
        )
        .await
        .unwrap();
    let target = created["targetId"].clone();
    // Event path: the new page target reports the fixture URL.
    support::wait_until(&mut events, Duration::from_secs(10), |event| {
        let info = &event.params["targetInfo"];
        (event.method == "Target.targetInfoChanged"
            && info["targetId"] == target
            && info["url"] == url.as_str())
        .then_some(())
    })
    .await
    .expect("the page target navigated to the fixture");
    // Poll path: the loaded document's title becomes the target title.
    let title = support::eventually(Duration::from_secs(10), || async {
        let targets = fixture
            .cdp()
            .call("Target.getTargets", json!({}), None, support::CALL)
            .await
            .ok()?;
        targets["targetInfos"]
            .as_array()?
            .iter()
            .find(|info| info["targetId"] == target)
            .and_then(|info| info["title"].as_str())
            .filter(|title| *title == "AeroBrowser basic fixture")
            .map(str::to_owned)
    })
    .await;
    assert_eq!(
        title.as_deref(),
        Some("AeroBrowser basic fixture"),
        "requests: {:?}",
        fixture.server.requests()
    );
    assert!(
        fixture
            .server
            .requests()
            .contains(&"GET /basic.html".to_owned())
    );

    fixture
        .shutdown()
        .await
        .expect("server and browser teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());

    // Failure path: a scenario that panics still reaps browser and server.
    let (started, address) = tokio::sync::oneshot::channel();
    let crashed = tokio::spawn(async move {
        let fixture = support::Fixture::start()
            .await
            .expect("second fixture starts");
        assert!(!support::sandbox_processes().is_empty());
        started.send(fixture.server.address()).unwrap();
        panic!("simulated scenario failure");
    })
    .await;
    assert!(crashed.unwrap_err().is_panic());
    let address = address.await.unwrap();
    assert_eq!(
        support::sandbox_processes(),
        Vec::<u32>::new(),
        "browser reaped during unwinding"
    );
    assert!(
        support::refuses_connections(address, Duration::from_secs(2)).await,
        "fixture server stopped during unwinding"
    );
}

/// C05: one attachment owner, tabs exclude non-page targets, page methods go
/// to the page's own session, and navigation really changes title and URL.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn page_target_navigation() {
    use aero_cdp::CdpError;
    use aero_cdp::page::Page;
    use aero_cdp::runtime::RuntimeError;
    use aero_cdp::target_registry::{AttachError, TargetKind};
    use serde_json::{Value, json};
    use std::time::Duration;

    async fn document(page: &Page) -> Option<(String, String)> {
        let reply = page
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": "document.readyState === 'complete' ? [document.title, location.href] : null",
                    "returnByValue": true
                }),
            )
            .await
            .ok()?;
        match &reply["result"]["value"] {
            Value::Array(pair) => {
                Some((pair[0].as_str()?.to_owned(), pair[1].as_str()?.to_owned()))
            }
            _ => None,
        }
    }

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();

    // Existing targets were enumerated: the startup tab is known without
    // creating anything, and only real pages count as tabs.
    let tab = runtime
        .wait_for_tab(Duration::from_secs(10))
        .await
        .expect("the startup tab is discovered");
    assert_eq!(tab.kind, TargetKind::Page);
    for target in runtime.targets() {
        assert_eq!(
            runtime
                .tabs()
                .iter()
                .any(|t| t.target_id == target.target_id),
            target.kind == TargetKind::Page && !target.url.starts_with("devtools://"),
            "{target:?}"
        );
    }

    // One owner: an explicit flattened session, and no second one.
    let page = runtime
        .attach(&tab.target_id)
        .await
        .expect("attach the tab");
    assert_ne!(page.session().as_str(), tab.target_id.as_str());
    assert_eq!(
        runtime.attach(&tab.target_id).await.err(),
        Some(RuntimeError::Attach(AttachError::AlreadyAttached))
    );
    // Chromium always exposes non-tab targets (browser UI, extension service
    // workers); they are discovered but cannot be attached as tabs.
    let other = support::eventually(Duration::from_secs(5), || async {
        runtime.targets().into_iter().find(|t| !t.is_tab())
    })
    .await
    .expect("a non-tab target is discovered");
    assert!(
        !runtime
            .tabs()
            .iter()
            .any(|t| t.target_id == other.target_id)
    );
    assert_eq!(
        runtime.attach(&other.target_id).await.err(),
        Some(RuntimeError::Attach(AttachError::NotATab))
    );

    // Page methods belong to the page session; the browser endpoint is not a page.
    assert!(matches!(
        runtime
            .connection()
            .call(
                "Runtime.evaluate",
                json!({ "expression": "1" }),
                None,
                support::CALL
            )
            .await,
        Err(CdpError::Protocol { .. })
    ));

    // Navigation changes the actual document, observed through the session.
    for (name, title) in [
        ("basic.html", "AeroBrowser basic fixture"),
        ("second.html", "AeroBrowser second fixture"),
    ] {
        let url = fixture.server.url(name);
        page.navigate(&url).await.expect("navigation starts");
        let seen = support::eventually(Duration::from_secs(10), || async {
            document(&page)
                .await
                .filter(|(t, href)| t == title && *href == url)
        })
        .await;
        assert_eq!(seen, Some((title.to_owned(), url.clone())));
        let registered = support::eventually(Duration::from_secs(10), || async {
            runtime.target(&tab.target_id).filter(|t| t.url == url)
        })
        .await;
        assert!(
            registered.is_some(),
            "the registry follows the tab to {url}"
        );
    }

    // A navigation the browser rejects is an error, not a silent success.
    let refused = page.navigate("http://127.0.0.1:1/unreachable.html").await;
    assert!(
        matches!(refused, Err(RuntimeError::NavigationFailed(ref text)) if text.contains("ERR_")),
        "{refused:?}"
    );
    assert_eq!(runtime.resyncs(), 0, "no events were missed");
    assert!(
        fixture
            .server
            .requests()
            .iter()
            .any(|r| r == "GET /second.html")
    );

    // Closing the tab while a call is in flight ends that call promptly (the
    // browser never answers it) and later calls are refused outright.
    let in_flight = page.call(
        "Runtime.evaluate",
        json!({ "expression": "new Promise(() => {})", "awaitPromise": true }),
    );
    let close = async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        fixture
            .cdp()
            .call(
                "Target.closeTarget",
                json!({ "targetId": tab.target_id.as_str() }),
                None,
                support::CALL,
            )
            .await
    };
    let started = std::time::Instant::now();
    let (pending, closed) = tokio::join!(in_flight, close);
    closed.expect("close the tab");
    assert_eq!(pending.err(), Some(RuntimeError::Detached));
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the in-flight call waited {:?}",
        started.elapsed()
    );
    assert_eq!(
        page.call("Runtime.evaluate", json!({ "expression": "1" }))
            .await
            .err(),
        Some(RuntimeError::Detached)
    );
    assert_eq!(
        page.evaluate("1").await,
        Err(aero_cdp::evaluate::EvaluateError::Detached)
    );
    // The session ends before the target is destroyed; removal follows.
    let removed = support::eventually(Duration::from_secs(5), || async {
        runtime.target(&tab.target_id).is_none().then_some(())
    })
    .await;
    assert!(removed.is_some(), "the closed tab leaves the registry");

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// C06: evaluation results are values or explicit errors, bounded in size and
/// time; a destroyed context is retried only when the caller opts in.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn evaluate_exception_is_error() {
    use aero_cdp::CdpError;
    use aero_cdp::evaluate::{EvaluateError, EvaluateOptions};
    use aero_cdp::page::NavigationLimits;
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let tab = runtime.wait_for_tab(Duration::from_secs(10)).await.unwrap();
    let page = runtime.attach(&tab.target_id).await.unwrap();
    let basic = fixture.server.url("basic.html");
    let second = fixture.server.url("second.html");
    page.navigate_and_wait(&basic, &NavigationLimits::default())
        .await
        .expect("fixture loads");

    assert_eq!(page.evaluate("1 + 1").await, Ok(json!(2)));
    assert_eq!(
        page.evaluate("document.title").await,
        Ok(json!("AeroBrowser basic fixture"))
    );
    assert_eq!(page.evaluate("undefined").await, Ok(Value::Null));
    for (expression, message) in [
        ("throw new Error('fixture boom')", "fixture boom"),
        ("Promise.reject(new Error('async boom'))", "async boom"),
    ] {
        match page.evaluate(expression).await {
            Err(EvaluateError::Exception(text)) => assert!(text.contains(message), "{text}"),
            other => panic!("{expression}: {other:?}"),
        }
    }
    assert_eq!(
        page.evaluate("NaN").await,
        Err(EvaluateError::Unserializable("NaN".into()))
    );
    assert!(matches!(
        page.evaluate("'x'.repeat(2 * 1024 * 1024)").await,
        Err(EvaluateError::ResultTooLarge { .. })
    ));

    // A promise that never settles is bounded by the deadline.
    let short = EvaluateOptions {
        deadline: Duration::from_millis(500),
        ..EvaluateOptions::default()
    };
    let started = Instant::now();
    assert_eq!(
        page.evaluate_with("new Promise(() => {})", &short).await,
        Err(EvaluateError::Cdp(CdpError::Timeout))
    );
    assert!(started.elapsed() < Duration::from_secs(3));

    // Hangs on the basic fixture, answers on the next document.
    let hang = "location.pathname === '/basic.html' \
        ? new Promise(() => {}) \
        : new Promise(resolve => document.readyState === 'loading' \
            ? addEventListener('DOMContentLoaded', () => resolve(document.title)) \
            : resolve(document.title))";
    let navigate_soon = |target: String| {
        let page = &page;
        async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            page.navigate(&target).await
        }
    };
    // Without opt-in, a destroyed context is reported, not retried or swallowed.
    let (result, navigated) = tokio::join!(page.evaluate(hang), navigate_soon(second.clone()));
    navigated.unwrap();
    assert_eq!(result, Err(EvaluateError::ContextDestroyed));
    // With opt-in, the idempotent expression runs again in the new document.
    page.navigate_and_wait(&basic, &NavigationLimits::default())
        .await
        .unwrap();
    let retry = EvaluateOptions {
        context_destroyed_retries: 3,
        ..EvaluateOptions::default()
    };
    let (result, navigated) = tokio::join!(
        page.evaluate_with(hang, &retry),
        navigate_soon(second.clone())
    );
    navigated.unwrap();
    assert_eq!(result, Ok(json!("AeroBrowser second fixture")));

    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// C06: navigation waits for the new document and bounded stability; HTTP and
/// network failures and missed deadlines are errors; endless mutations and
/// long-lived requests cannot hang the wait.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn navigation_and_stability_deadlines() {
    use aero_cdp::page::NavigationLimits;
    use aero_cdp::runtime::RuntimeError;
    use std::time::{Duration, Instant};

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let tab = runtime.wait_for_tab(Duration::from_secs(10)).await.unwrap();
    let page = runtime.attach(&tab.target_id).await.unwrap();
    let limits = NavigationLimits {
        stability_cap: Duration::from_secs(2),
        ..NavigationLimits::default()
    };

    let basic = fixture.server.url("basic.html");
    let loaded = page
        .navigate_and_wait(&basic, &limits)
        .await
        .expect("loads");
    assert!(!loaded.same_document);
    assert_eq!(loaded.status, Some(200));
    assert_eq!(loaded.url, basic);
    assert!(loaded.stable, "a static page becomes stable");

    // Same-document (fragment) navigation has no new loader.
    let fragment = format!("{basic}#section");
    let moved = page
        .navigate_and_wait(&fragment, &limits)
        .await
        .expect("fragment");
    assert!(moved.same_document);
    assert_eq!(moved.url, fragment);
    assert_eq!(moved.status, None);

    // A page that replaces timers, animation frames and MutationObserver
    // cannot break or fake the check: it runs in an isolated world.
    let tampered = page
        .navigate_and_wait(&fixture.server.url("tampered.html"), &limits)
        .await
        .expect("loads");
    assert!(
        tampered.stable,
        "stability is measured outside the page's world"
    );

    // Endless mutations and a request that never completes: the document
    // loads, stability gives up at its cap, nothing hangs.
    let started = Instant::now();
    let dynamic = page
        .navigate_and_wait(&fixture.server.url("dynamic.html"), &limits)
        .await
        .expect("loads despite the pending request");
    let waited = started.elapsed();
    assert!(!dynamic.stable);
    assert!(waited >= Duration::from_millis(1900), "{waited:?}");
    assert!(waited < Duration::from_secs(6), "{waited:?}");
    assert!(fixture.server.requests().iter().any(|r| r == "GET /hold"));

    // HTTP and network failures are errors, not successes.
    assert_eq!(
        page.navigate_and_wait(&fixture.server.url("missing.html"), &limits)
            .await
            .err(),
        Some(RuntimeError::HttpStatus(404))
    );
    let refused = page
        .navigate_and_wait("http://127.0.0.1:1/unreachable.html", &limits)
        .await;
    assert!(
        matches!(refused, Err(RuntimeError::NavigationFailed(ref text)) if text.contains("ERR_")),
        "{refused:?}"
    );

    // A document that never arrives is bounded by the navigation deadline.
    let short = NavigationLimits {
        navigation: Duration::from_secs(1),
        ..limits.clone()
    };
    let started = Instant::now();
    assert_eq!(
        page.navigate_and_wait(&fixture.server.url("hold"), &short)
            .await
            .err(),
        Some(RuntimeError::NavigationTimeout)
    );
    assert!(started.elapsed() < Duration::from_secs(4));

    // The page is still usable afterwards.
    let again = page
        .navigate_and_wait(&basic, &limits)
        .await
        .expect("recovers");
    assert_eq!(again.status, Some(200));

    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D02 helpers: the click fixture, attached and loaded, and exact counts.
#[cfg(feature = "browser-tests")]
mod clicks {
    use std::time::Duration;

    use aero_cdp::page::{NavigationLimits, Page};
    use aero_core::ids::ObservationId;
    use aero_perception::observation::{Observation, observe_page};
    use serde_json::{Map, Value};

    use crate::support::Fixture;

    pub async fn open(fixture: &Fixture) -> Page {
        let runtime = fixture.runtime();
        let tab = runtime.wait_for_tab(Duration::from_secs(10)).await.unwrap();
        let page = runtime.attach(&tab.target_id).await.unwrap();
        load(fixture, &page).await;
        page
    }

    pub async fn load(fixture: &Fixture, page: &Page) {
        page.navigate_and_wait(
            &fixture.server.url("click.html"),
            &NavigationLimits::default(),
        )
        .await
        .expect("click fixture loads");
    }

    pub async fn observe(fixture: &Fixture, page: &Page, id: &str) -> Observation {
        observe_page(
            page,
            ObservationId::from(id),
            fixture.runtime().epoch().clone(),
        )
        .await
        .expect("observation of the live page")
    }

    /// The handle of the one button named `name`.
    pub fn button(observation: &Observation, name: &str) -> String {
        let mut found: Vec<String> = observation
            .handles()
            .iter()
            .filter(|(_, target)| target.role == "button" && target.name == name)
            .map(|(handle, _)| handle.to_string())
            .collect();
        assert_eq!(
            found.len(),
            1,
            "one button {name:?} in:\n{}",
            observation.text()
        );
        found.remove(0)
    }

    /// Every pointer/mouse/click event so far, keyed `type:element-id`.
    pub async fn counts(page: &Page) -> Map<String, Value> {
        let text = page
            .evaluate("JSON.stringify(window.counts)")
            .await
            .unwrap();
        serde_json::from_str(text.as_str().unwrap()).unwrap()
    }

    pub fn expected(id: &str) -> Map<String, Value> {
        [
            "pointermove",
            "mousemove",
            "pointerdown",
            "mousedown",
            "pointerup",
            "mouseup",
            "click",
        ]
        .iter()
        .map(|kind| (format!("{kind}:{id}"), Value::from(1)))
        .collect()
    }
}

/// D02: one grounded click is exactly one move/press/release sequence on the
/// target; the root viewport is the configured 1280×800.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn click_has_one_event_sequence() {
    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let page = clicks::open(&fixture).await;
    assert_eq!(
        page.evaluate("[innerWidth, innerHeight, devicePixelRatio]")
            .await,
        Ok(serde_json::json!([1280, 800, 1])),
        "the configured root viewport is in effect"
    );
    let observation = clicks::observe(&fixture, &page, "obs-1").await;
    let plain = clicks::button(&observation, "Plain inner");
    aero_executor::click_handle(
        &page,
        observation.handles(),
        observation.id(),
        runtime.epoch(),
        &plain,
    )
    .await
    .expect("grounded click");
    assert_eq!(clicks::counts(&page).await, clicks::expected("plain"));

    // The press lands on a child that fills the button: a hit on a
    // descendant is still a hit on the target.
    let filled = clicks::button(&observation, "Filled");
    aero_executor::click_handle(
        &page,
        observation.handles(),
        observation.id(),
        runtime.epoch(),
        &filled,
    )
    .await
    .expect("grounded click on a filled button");
    let mut both = clicks::expected("plain");
    both.extend(clicks::expected("filled"));
    assert_eq!(clicks::counts(&page).await, both);
    let last = page
        .evaluate("window.clicks[window.clicks.length - 1]")
        .await
        .unwrap();
    assert_eq!(
        (&last["id"], &last["tag"]),
        (&"filled".into(), &"SPAN".into()),
        "{last}"
    );

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D02: a target below the fold is scrolled into view and measured again; the
/// press lands inside its post-scroll box.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn scroll_remeasures_target() {
    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let page = clicks::open(&fixture).await;
    let before = page
        .evaluate("document.getElementById('far').getBoundingClientRect().top")
        .await
        .unwrap();
    assert!(
        before.as_f64().unwrap() > 1500.0,
        "starts far below the fold: {before}"
    );
    let observation = clicks::observe(&fixture, &page, "obs-1").await;
    let far = clicks::button(&observation, "Far");
    let grounded = aero_executor::click_handle(
        &page,
        observation.handles(),
        observation.id(),
        runtime.epoch(),
        &far,
    )
    .await
    .expect("grounded click");
    assert_eq!(clicks::counts(&page).await, clicks::expected("far"));
    let recorded = page.evaluate("window.clicks[0]").await.unwrap();
    let (x, y) = grounded.point.coordinates();
    let num = |key: &str| recorded[key].as_f64().unwrap();
    assert_eq!(recorded["id"], "far");
    assert!(num("top") < 800.0, "scrolled into view: {recorded}");
    assert!(
        num("left") < x && x < num("right") && num("top") < y && y < num("bottom"),
        "{recorded} vs ({x}, {y})"
    );
    assert!(
        (num("x") - x).abs() <= 1.0 && (num("y") - y).abs() <= 1.0,
        "{recorded} vs ({x}, {y})"
    );

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D02: handles from an earlier document, a removed node, another observation
/// or another runtime generation are refused before any input is sent.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn stale_observation_cannot_click() {
    use aero_core::ids::{ObservationId, RuntimeEpoch};
    use aero_executor::ClickError;
    use aero_executor::target::GroundingError;
    use aero_perception::observation::HandleError;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let page = clicks::open(&fixture).await;
    let click = |observation: &aero_perception::observation::Observation,
                 id: ObservationId,
                 epoch: RuntimeEpoch| {
        let handle = clicks::button(observation, "Plain inner");
        let page = &page;
        let handles = observation.handles().clone();
        async move { aero_executor::click_handle(page, &handles, &id, &epoch, &handle).await }
    };

    // Same URL, new document: the old observation is stale.
    let old = clicks::observe(&fixture, &page, "obs-1").await;
    clicks::load(&fixture, &page).await;
    assert_eq!(
        click(&old, old.id().clone(), runtime.epoch().clone())
            .await
            .err(),
        Some(ClickError::Grounding(GroundingError::StaleDocument))
    );
    assert!(clicks::counts(&page).await.is_empty());

    // The node was removed after the observation.
    let fresh = clicks::observe(&fixture, &page, "obs-2").await;
    page.evaluate("document.getElementById('plain').remove(), 1")
        .await
        .unwrap();
    assert_eq!(
        click(&fresh, fresh.id().clone(), runtime.epoch().clone())
            .await
            .err(),
        Some(ClickError::Grounding(GroundingError::NodeGone))
    );

    // Presented for another observation, or after the runtime was replaced.
    clicks::load(&fixture, &page).await;
    let current = clicks::observe(&fixture, &page, "obs-3").await;
    assert_eq!(
        click(
            &current,
            ObservationId::from("obs-2"),
            runtime.epoch().clone()
        )
        .await
        .err(),
        Some(ClickError::Grounding(GroundingError::Handle(
            HandleError::OtherObservation
        )))
    );
    assert_eq!(
        click(
            &current,
            current.id().clone(),
            RuntimeEpoch::from("epoch-0")
        )
        .await
        .err(),
        Some(ClickError::Grounding(GroundingError::Handle(
            HandleError::OtherRuntime
        )))
    );
    assert!(
        clicks::counts(&page).await.is_empty(),
        "no input reached the page"
    );

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D02: covered, disabled and transformed targets are refused with no input at
/// all, even though page script has replaced `elementFromPoint` and
/// `contains` to make a page-world hit test lie.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn occluded_target_is_rejected() {
    use aero_executor::ClickError;
    use aero_executor::target::GroundingError;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let page = clicks::open(&fixture).await;
    let observation = clicks::observe(&fixture, &page, "obs-1").await;
    for (name, refusal) in [
        ("Covered", GroundingError::Occluded),
        ("Disabled", GroundingError::Disabled),
        ("Rotated", GroundingError::UnsupportedTransform),
    ] {
        let handle = clicks::button(&observation, name);
        assert_eq!(
            aero_executor::click_handle(
                &page,
                observation.handles(),
                observation.id(),
                runtime.epoch(),
                &handle
            )
            .await
            .err(),
            Some(ClickError::Grounding(refusal)),
            "{name}"
        );
    }
    assert!(
        clicks::counts(&page).await.is_empty(),
        "no input reached the page"
    );

    // A zoomed visual viewport makes CSS boxes and input coordinates disagree.
    page.call(
        "Emulation.setPageScaleFactor",
        serde_json::json!({ "pageScaleFactor": 2 }),
    )
    .await
    .unwrap();
    let plain = clicks::button(&observation, "Plain inner");
    assert_eq!(
        aero_executor::click_handle(
            &page,
            observation.handles(),
            observation.id(),
            runtime.epoch(),
            &plain
        )
        .await
        .err(),
        Some(ClickError::Grounding(GroundingError::ViewportScaled))
    );
    assert!(clicks::counts(&page).await.is_empty());

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}
