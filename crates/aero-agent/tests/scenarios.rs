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

/// D03 helpers: the forms fixture and its event log.
#[cfg(feature = "browser-tests")]
mod forms {
    use std::time::Duration;

    use aero_cdp::page::{NavigationLimits, Page};
    use aero_core::ids::ObservationId;
    use aero_perception::observation::{Observation, observe_page};
    use serde_json::{Map, Value};

    use crate::support::Fixture;

    pub async fn open(fixture: &Fixture) -> (Page, Observation) {
        let runtime = fixture.runtime();
        let tab = runtime.wait_for_tab(Duration::from_secs(10)).await.unwrap();
        let page = runtime.attach(&tab.target_id).await.unwrap();
        page.navigate_and_wait(
            &fixture.server.url("forms.html"),
            &NavigationLimits::default(),
        )
        .await
        .expect("forms fixture loads");
        let observation = observe_page(
            &page,
            ObservationId::from("forms-1"),
            runtime.epoch().clone(),
        )
        .await
        .expect("observation");
        (page, observation)
    }

    /// The handle of the one element with this role and name.
    pub fn handle(observation: &Observation, role: &str, name: &str) -> String {
        let mut found: Vec<String> = observation
            .handles()
            .iter()
            .filter(|(_, target)| target.role == role && target.name == name)
            .map(|(handle, _)| handle.to_string())
            .collect();
        assert_eq!(
            found.len(),
            1,
            "one {role} {name:?} in:\n{}",
            observation.text()
        );
        found.remove(0)
    }

    pub async fn log(page: &Page) -> Map<String, Value> {
        let text = page.evaluate("JSON.stringify(window.log)").await.unwrap();
        serde_json::from_str(text.as_str().unwrap()).unwrap()
    }

    pub fn count(log: &Map<String, Value>, key: &str) -> u64 {
        log.get(key).and_then(Value::as_u64).unwrap_or(0)
    }

    /// No pointer input of any kind reached the page.
    pub fn no_pointer_input(log: &Map<String, Value>) -> bool {
        !log.keys().any(|key| {
            key.starts_with("pointerdown:")
                || key.starts_with("mousedown:")
                || key.starts_with("click:")
        })
    }

    pub async fn value(page: &Page, id: &str) -> Value {
        page.evaluate(&format!("document.getElementById('{id}').value"))
            .await
            .unwrap()
    }
}

/// D03: Unicode is inserted exactly (input, textarea, a controlled input),
/// clearing works, newline text cannot reach a single-line input, and Enter is
/// one dedicated key event that submits exactly once, only when requested.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn unicode_type_clear_and_enter() {
    use aero_executor::TargetRef;
    use aero_executor::dispatcher::{InputError, type_text};

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let (page, observation) = forms::open(&fixture).await;
    let handles: Vec<String> = [
        "Name",
        "Notes",
        "Controlled",
        "Read only",
        "Slippery",
        "Sticky",
        "Short",
        "Prefilled",
    ]
    .iter()
    .map(|name| forms::handle(&observation, "textbox", name))
    .collect();
    let target = |index: usize| TargetRef {
        handles: observation.handles(),
        observation: observation.id(),
        runtime: runtime.epoch(),
        handle: &handles[index],
    };

    let unicode = "Grüße, 世界 🙂 مرحبا";
    type_text(&page, target(0), unicode, true, false)
        .await
        .expect("type into Name");
    assert_eq!(forms::value(&page, "name").await, unicode);

    // Appending keeps the existing text; a textarea keeps its newlines.
    type_text(&page, target(1), "line one\nline two ✓", false, false)
        .await
        .expect("type into Notes");
    type_text(&page, target(1), " — end", false, false)
        .await
        .expect("append to Notes");
    assert_eq!(
        forms::value(&page, "notes").await,
        "line one\nline two ✓ — end"
    );

    // A controlled input keeps the text because real input events updated its state.
    type_text(&page, target(2), "Ünïcode ✓", true, false)
        .await
        .expect("type into Controlled");
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(forms::value(&page, "controlled").await, "Ünïcode ✓");

    // Newline text never reaches a single-line input, and nothing submits.
    assert_eq!(
        type_text(&page, target(0), "x\ny", true, false).await,
        Err(InputError::NewlineInSingleLine)
    );
    assert_eq!(forms::value(&page, "name").await, unicode);
    assert_eq!(
        type_text(&page, target(3), "nope", true, false).await,
        Err(InputError::Readonly)
    );
    assert_eq!(forms::value(&page, "ro").await, "fixed");
    let log = forms::log(&page).await;
    assert_eq!(forms::count(&log, "submit"), 0);
    assert_eq!(forms::count(&log, "keydown:Enter:name"), 0);

    // Results are verified, not assumed: a field that will not take focus,
    // one that refuses to be emptied, and one that cuts the text.
    assert_eq!(
        type_text(&page, target(4), "x", true, false).await,
        Err(InputError::FocusFailed)
    );
    assert_eq!(
        type_text(&page, target(5), "new", true, false).await,
        Err(InputError::ClearFailed)
    );
    assert_eq!(
        type_text(&page, target(6), "abcdef", true, false).await,
        Err(InputError::ValueMismatch)
    );
    // Without clearing, text goes after the existing value.
    type_text(&page, target(7), " end", false, false)
        .await
        .expect("append");
    assert_eq!(forms::value(&page, "prefilled").await, "start end");

    // Enter only when requested: one key event, one submission.
    type_text(&page, target(0), "go", true, true)
        .await
        .expect("type and submit");
    let log = forms::log(&page).await;
    assert_eq!(forms::value(&page, "name").await, "go");
    assert_eq!(forms::count(&log, "keydown:Enter:name"), 1);
    assert_eq!(forms::count(&log, "submit"), 1);

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D03: typing focuses the field once with no pointer input at all, uses only
/// the clearing keys it needs, and allow-listed keys go to the focused element.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn typing_does_not_double_click() {
    use aero_executor::TargetRef;
    use aero_executor::dispatcher::{InputError, press_key, type_text};

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let (page, observation) = forms::open(&fixture).await;
    let name = forms::handle(&observation, "textbox", "Name");
    let target = TargetRef {
        handles: observation.handles(),
        observation: observation.id(),
        runtime: runtime.epoch(),
        handle: &name,
    };

    type_text(&page, target, "first", true, false)
        .await
        .expect("type");
    type_text(&page, target, " second", false, false)
        .await
        .expect("type again");
    assert_eq!(forms::value(&page, "name").await, "first second");
    let log = forms::log(&page).await;
    assert!(forms::no_pointer_input(&log), "{log:?}");
    assert_eq!(
        forms::count(&log, "focusin:name"),
        1,
        "focused once: {log:?}"
    );
    assert_eq!(
        forms::count(&log, "keydown:a:name"),
        1,
        "one select-all: {log:?}"
    );
    assert_eq!(
        forms::count(&log, "keydown:Backspace:name"),
        1,
        "one backspace: {log:?}"
    );
    let keys: u64 = log
        .iter()
        .filter(|(key, _)| key.starts_with("keydown:"))
        .map(|(_, n)| n.as_u64().unwrap())
        .sum();
    assert_eq!(keys, 2, "no other key events: {log:?}");

    // Allow-listed keys act on the focused element; others are refused.
    press_key(&page, "Tab").await.expect("Tab");
    assert_eq!(
        page.evaluate("document.activeElement.id").await,
        Ok(serde_json::json!("notes"))
    );
    assert_eq!(
        press_key(&page, "F5").await,
        Err(InputError::UnsupportedKey)
    );
    assert_eq!(press_key(&page, "a").await, Err(InputError::UnsupportedKey));
    let log = forms::log(&page).await;
    assert_eq!(forms::count(&log, "keydown:Tab:name"), 1);
    assert!(forms::no_pointer_input(&log), "{log:?}");

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D03: select and checkbox changes are typed operations whose results are
/// read back; refusals leave the controls unchanged.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn select_and_checkbox_are_verified() {
    use aero_executor::TargetRef;
    use aero_executor::dispatcher::{InputError, select_values, set_checked};
    use aero_executor::target::GroundingError;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let (page, observation) = forms::open(&fixture).await;
    let size = forms::handle(&observation, "combobox", "Size");
    let toppings = forms::handle(&observation, "listbox", "Toppings");
    let pinned = forms::handle(&observation, "combobox", "Pinned");
    let name = forms::handle(&observation, "textbox", "Name");
    let checkbox = |label: &str| forms::handle(&observation, "checkbox", label);
    let radio = |label: &str| forms::handle(&observation, "radio", label);
    let target = |handle: &str| TargetRef {
        handles: observation.handles(),
        observation: observation.id(),
        runtime: runtime.epoch(),
        handle: handle.to_owned().leak(),
    };
    let values = |list: &[&str]| list.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
    let selected = |id: &'static str| {
        let page = &page;
        async move {
            page.evaluate(&format!(
                "Array.from(document.getElementById('{id}').selectedOptions, o => o.value)"
            ))
            .await
            .unwrap()
        }
    };
    let checked = |id: &'static str| {
        let page = &page;
        async move {
            page.evaluate(&format!("document.getElementById('{id}').checked"))
                .await
                .unwrap()
        }
    };

    // Single select: exactly one known, enabled value.
    select_values(&page, target(&size), &values(&["l"]))
        .await
        .expect("select Large");
    assert_eq!(selected("size").await, serde_json::json!(["l"]));
    for (bad, refusal) in [
        (values(&["xxl"]), InputError::UnknownOption),
        (values(&["xl"]), InputError::UnknownOption),
        (values(&["s", "m"]), InputError::WrongValueCount),
        (values(&[]), InputError::WrongValueCount),
    ] {
        assert_eq!(
            select_values(&page, target(&size), &bad).await,
            Err(refusal),
            "{bad:?}"
        );
    }
    assert_eq!(
        selected("size").await,
        serde_json::json!(["l"]),
        "refusals change nothing"
    );
    // Multiple select: exactly the requested set.
    select_values(&page, target(&toppings), &values(&["basil", "cheese"]))
        .await
        .expect("select toppings");
    assert_eq!(
        selected("toppings").await,
        serde_json::json!(["cheese", "basil"])
    );
    assert_eq!(
        select_values(&page, target(&name), &values(&["x"])).await,
        Err(InputError::NotASelect)
    );
    // The application put its own value back: reported, not assumed.
    assert_eq!(
        select_values(&page, target(&pinned), &values(&["b"])).await,
        Err(InputError::NotApplied)
    );
    assert_eq!(selected("pinned").await, serde_json::json!(["a"]));
    let log = forms::log(&page).await;
    assert_eq!(forms::count(&log, "change:size"), 1);
    assert_eq!(forms::count(&log, "change:toppings"), 1);

    // Checkbox: one click per change, none when already in the requested state.
    set_checked(&page, target(&checkbox("Agree")), true)
        .await
        .expect("check");
    set_checked(&page, target(&checkbox("Agree")), true)
        .await
        .expect("already checked");
    assert_eq!(checked("agree").await, true);
    assert_eq!(forms::count(&forms::log(&page).await, "click:agree"), 1);
    set_checked(&page, target(&checkbox("Agree")), false)
        .await
        .expect("uncheck");
    assert_eq!(checked("agree").await, false);
    assert_eq!(forms::count(&forms::log(&page).await, "click:agree"), 2);
    // The application refused the change: reported, not assumed.
    assert_eq!(
        set_checked(&page, target(&checkbox("Stuck")), true).await,
        Err(InputError::NotApplied)
    );
    assert_eq!(checked("stuck").await, false);
    assert_eq!(
        set_checked(&page, target(&checkbox("Locked")), true).await,
        Err(InputError::Grounding(GroundingError::Disabled))
    );
    assert_eq!(
        set_checked(&page, target(&name), true).await,
        Err(InputError::NotACheckbox)
    );
    // Radio buttons can be chosen but not unchosen.
    set_checked(&page, target(&radio("Fast")), true)
        .await
        .expect("choose Fast");
    assert_eq!(
        (checked("fast").await, checked("slow").await),
        (true.into(), false.into())
    );
    assert_eq!(
        set_checked(&page, target(&radio("Fast")), false).await,
        Err(InputError::CannotUncheckRadio)
    );
    assert_eq!(checked("fast").await, true);
    assert_eq!(forms::count(&forms::log(&page).await, "submit"), 0);

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D04 helpers: frame fixtures, frame lookups and per-frame evaluation.
#[cfg(feature = "browser-tests")]
mod frames {
    use std::time::Duration;

    use aero_cdp::frame_manager::{FrameInfo, FrameSnapshot};
    use aero_cdp::page::{NavigationLimits, Page};
    use aero_core::ids::{FrameId, ObservationId};
    use aero_perception::observation::{HandleTarget, Observation, observe_page};
    use serde_json::{Value, json};

    use crate::support::{self, Fixture};

    pub async fn open(fixture: &Fixture, name: &str) -> Page {
        let runtime = fixture.runtime();
        let tab = runtime.wait_for_tab(Duration::from_secs(10)).await.unwrap();
        let page = runtime.attach(&tab.target_id).await.unwrap();
        page.navigate_and_wait(&fixture.server.url(name), &NavigationLimits::default())
            .await
            .expect("frames fixture loads");
        page
    }

    /// The frame graph once every listed path has a frame with a document
    /// and a default context.
    pub async fn settled(page: &Page, paths: &[&str]) -> FrameSnapshot {
        let snapshot = support::eventually(Duration::from_secs(10), || async {
            let snapshot = page.frames().ok()?;
            let mut found = 0;
            for info in &snapshot.frames {
                if info.document.is_none() || info.context.is_none() {
                    return None;
                }
                if let Some(path) = path_of(page, info).await
                    && paths.contains(&path.as_str())
                {
                    found += 1;
                }
            }
            (found == paths.len() && snapshot.frames.len() == paths.len()).then_some(snapshot)
        })
        .await;
        snapshot.unwrap_or_else(|| panic!("frames {paths:?} did not settle: {:?}", page.frames()))
    }

    /// `location.pathname` evaluated in the frame's own default context.
    pub async fn path_of(page: &Page, info: &FrameInfo) -> Option<String> {
        Some(
            evaluate_in(page, info, "location.pathname")
                .await
                .ok()?
                .as_str()?
                .to_owned(),
        )
    }

    pub async fn evaluate_in(
        page: &Page,
        info: &FrameInfo,
        expression: &str,
    ) -> Result<Value, String> {
        let context = info.context.as_ref().ok_or("no context")?;
        let reply = page
            .frame_call(
                &info.frame,
                "Runtime.evaluate",
                json!({ "expression": expression, "contextId": context.id, "returnByValue": true }),
            )
            .await
            .map_err(|error| format!("{error:?}"))?;
        Ok(reply["result"]["value"].clone())
    }

    pub async fn by_path<'a>(
        page: &Page,
        snapshot: &'a FrameSnapshot,
        path: &str,
    ) -> &'a FrameInfo {
        for info in &snapshot.frames {
            if path_of(page, info).await.as_deref() == Some(path) {
                return info;
            }
        }
        panic!("no frame at {path}");
    }

    pub async fn observe(page: &Page, id: &str) -> Observation {
        let runtime = aero_core::ids::RuntimeEpoch::from("epoch-1");
        support::eventually(Duration::from_secs(5), || async {
            observe_page(page, ObservationId::from(id), runtime.clone())
                .await
                .ok()
        })
        .await
        .expect("observation")
    }

    /// The handle target of the one button with this name.
    pub fn button<'a>(observation: &'a Observation, name: &str) -> (String, &'a HandleTarget) {
        let found: Vec<_> = observation
            .handles()
            .iter()
            .filter(|(_, target)| target.role == "button" && target.name == name)
            .collect();
        assert_eq!(
            found.len(),
            1,
            "one button {name:?} in:\n{}",
            observation.text()
        );
        (found[0].0.to_string(), found[0].1)
    }

    pub fn frame_ids(snapshot: &FrameSnapshot) -> Vec<FrameId> {
        snapshot
            .frames
            .iter()
            .map(|info| info.frame.clone())
            .collect()
    }
}

/// D04: a same-process iframe is tracked, observed under its `<iframe>` and
/// evaluated in its own context; navigating it replaces its document, after
/// which its old nodes and handles fail.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn same_process_frame_is_observable() {
    use aero_cdp::runtime::RuntimeError;
    use aero_executor::target::GroundingError;
    use serde_json::json;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let page = frames::open(&fixture, "frames.html").await;
    let snapshot = frames::settled(&page, &["/frames.html", "/frames-inner.html"]).await;
    let main = frames::by_path(&page, &snapshot, "/frames.html")
        .await
        .clone();
    let inner = frames::by_path(&page, &snapshot, "/frames-inner.html")
        .await
        .clone();
    assert_eq!(main.parent, None);
    assert_eq!(inner.parent.as_ref(), Some(&main.frame));
    assert_eq!(snapshot.frames[0].frame, main.frame, "parents first");
    // Same process: both frames belong to the page's own session.
    assert_eq!(
        (&main.session, &inner.session),
        (page.session(), page.session())
    );
    assert!(main.local_root && !inner.local_root);
    assert_ne!(main.frame, inner.frame);
    assert_ne!(
        main.context, inner.context,
        "each frame has its own context"
    );
    assert_eq!(
        page.main_frame().await.unwrap().document,
        main.document.clone().unwrap()
    );

    // The observation shows the inner document under its iframe, with the
    // handle bound to the inner frame and document.
    let observation = frames::observe(&page, "frames-1").await;
    assert!(
        observation
            .text()
            .contains("iframe \"Inner frame\"\n  @e1 button \"Inner button\"\n"),
        "{}",
        observation.text()
    );
    let (handle, target) = frames::button(&observation, "Inner button");
    assert_eq!(target.frame, inner.frame);
    assert_eq!(Some(&target.document), inner.document.as_ref());
    let old_node = target.backend_node_id;
    assert!(
        page.frame_call(
            &inner.frame,
            "DOM.resolveNode",
            json!({ "backendNodeId": old_node })
        )
        .await
        .is_ok()
    );
    // Acting inside frames is D05: refused before any input.
    assert_eq!(
        aero_executor::click_handle(
            &page,
            observation.handles(),
            observation.id(),
            runtime.epoch(),
            &handle
        )
        .await
        .err()
        .map(|error| format!("{error:?}")),
        Some(format!(
            "{:?}",
            aero_executor::ClickError::Grounding(GroundingError::UnsupportedFrame)
        ))
    );

    // Navigate only the iframe.
    page.evaluate("document.getElementById('inner').src = 'frames-inner2.html'; 1")
        .await
        .unwrap();
    let after = frames::settled(&page, &["/frames.html", "/frames-inner2.html"]).await;
    let inner2 = frames::by_path(&page, &after, "/frames-inner2.html").await;
    assert_eq!(inner2.frame, inner.frame, "same frame, new document");
    assert_ne!(inner2.document, inner.document);
    assert_eq!(
        frames::by_path(&page, &after, "/frames.html")
            .await
            .document,
        main.document
    );
    assert!(after.version > snapshot.version);
    let error = page
        .frame_call(
            &inner.frame,
            "DOM.resolveNode",
            json!({ "backendNodeId": old_node }),
        )
        .await
        .expect_err("old node");
    assert!(
        format!("{error:?}").contains("does not belong to the document"),
        "{error:?}"
    );
    let observation = frames::observe(&page, "frames-2").await;
    assert!(
        observation.text().contains("Second inner button"),
        "{}",
        observation.text()
    );
    assert!(
        !observation.text().contains("\"Inner button\""),
        "{}",
        observation.text()
    );
    assert_eq!(
        frames::button(&observation, "Second inner button")
            .1
            .document,
        inner2.document.clone().unwrap()
    );

    // Unknown frames are refused.
    assert_eq!(
        page.frame_call(
            &aero_core::ids::FrameId::from("NOPE"),
            "DOM.getDocument",
            json!({})
        )
        .await,
        Err(RuntimeError::FrameGone)
    );

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}

/// D04: a cross-site iframe and the nested frame inside it are attached as
/// out-of-process frames with their own sessions and contexts, observed in
/// place, survive a full rebuild, and disappear with their sessions when the
/// outer iframe is removed. Workers are never attached.
#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn nested_oopif_attaches_and_detaches() {
    use std::time::{Duration, Instant};

    use aero_cdp::runtime::RuntimeError;
    use aero_cdp::target_registry::TargetKind;
    use serde_json::json;

    let _serial = support::serial().await;
    let fixture = support::Fixture::start()
        .await
        .expect("required native browser fixture failed to start");
    let runtime = fixture.runtime();
    let mut events = fixture.cdp().subscribe();
    let page = frames::open(&fixture, "frames-oopif.html").await;
    let paths = [
        "/frames-oopif.html",
        "/frames-cross.html",
        "/frames-leaf.html",
    ];
    let snapshot = frames::settled(&page, &paths).await;
    // Only iframes were ever auto-attached (the worker never was, even
    // briefly).
    let mut attached = Vec::new();
    while let Ok(event) = tokio::time::timeout(Duration::from_millis(100), events.recv()).await {
        let event = event.expect("no missed events");
        if event.method == "Target.attachedToTarget" && event.session_id.is_some() {
            attached.push(
                event.params["targetInfo"]["type"]
                    .as_str()
                    .unwrap_or("")
                    .to_owned(),
            );
        }
    }
    assert_eq!(attached, ["iframe", "iframe"]);
    let main = frames::by_path(&page, &snapshot, paths[0]).await.clone();
    let cross = frames::by_path(&page, &snapshot, paths[1]).await.clone();
    let leaf = frames::by_path(&page, &snapshot, paths[2]).await.clone();
    assert_eq!(cross.parent.as_ref(), Some(&main.frame));
    assert_eq!(leaf.parent.as_ref(), Some(&cross.frame));
    assert_eq!(
        frames::frame_ids(&snapshot),
        [main.frame.clone(), cross.frame.clone(), leaf.frame.clone()]
    );
    // Three sessions, three contexts, all distinct; each OOPIF is its
    // session's root.
    assert_eq!(&main.session, page.session());
    assert!(
        cross.session != main.session
            && leaf.session != main.session
            && leaf.session != cross.session
    );
    assert!(cross.local_root && leaf.local_root);
    let contexts: std::collections::HashSet<_> = snapshot
        .frames
        .iter()
        .map(|info| info.context.clone())
        .collect();
    assert_eq!(contexts.len(), 3);
    let port = fixture.server.address().port();
    assert_eq!(
        frames::evaluate_in(&page, &cross, "location.host").await,
        Ok(json!(format!("localhost:{port}")))
    );
    assert_eq!(
        frames::evaluate_in(&page, &leaf, "location.host").await,
        Ok(json!(format!("127.0.0.1:{port}")))
    );
    // The worker exists but was never attached (its URL may still be empty;
    // the extension service workers are the browser's own).
    let worker = support::eventually(Duration::from_secs(5), || async {
        runtime.targets().into_iter().find(|target| {
            target.kind == TargetKind::Worker && !target.url.starts_with("chrome-extension://")
        })
    })
    .await
    .expect("dedicated worker target");
    assert!(!worker.attached, "{worker:?}");

    // Observed in place: each button under its own iframe, bound to its frame.
    let observation = frames::observe(&page, "oopif-1").await;
    let text = observation.text();
    assert!(
        text.contains("iframe \"Cross frame\"\n  @e1 button \"Cross button\"\n  iframe \"Leaf frame\"\n    @e2 button \"Leaf button\"\n"),
        "{text}"
    );
    assert_eq!(
        frames::button(&observation, "Cross button").1.frame,
        cross.frame
    );
    assert_eq!(
        frames::button(&observation, "Leaf button").1.frame,
        leaf.frame
    );
    // The leaf's owner is the `<iframe id="leaf">` element in the cross document.
    let owner = page
        .frame_owner(&leaf.frame)
        .await
        .expect("owner of the leaf");
    let described = page
        .frame_call(
            &cross.frame,
            "DOM.describeNode",
            json!({ "backendNodeId": owner }),
        )
        .await
        .expect("owner lives in the cross document");
    assert_eq!(described["node"]["nodeName"], "IFRAME");
    assert!(
        described["node"]["attributes"]
            .as_array()
            .unwrap()
            .contains(&json!("leaf")),
        "{described}"
    );

    // A rebuild re-attaches both OOPIFs with new sessions; the old sessions
    // are no longer ours and the graph is otherwise unchanged.
    page.resync_frames().await.expect("rebuild");
    assert_eq!(page.frame_resyncs(), 1);
    let rebuilt = frames::settled(&page, &paths).await;
    assert_eq!(frames::frame_ids(&rebuilt), frames::frame_ids(&snapshot));
    for (old, new) in snapshot.frames.iter().zip(&rebuilt.frames) {
        assert_eq!(old.document, new.document);
    }
    let cross2 = frames::by_path(&page, &rebuilt, paths[1]).await.clone();
    assert_ne!(cross2.session, cross.session);
    let raw = fixture
        .cdp()
        .call(
            "Runtime.evaluate",
            json!({ "expression": "1" }),
            Some(&cross.session),
            Duration::from_secs(2),
        )
        .await;
    assert!(raw.is_err(), "the old session has ended: {raw:?}");

    // A call in flight when its frame is removed ends promptly as detached.
    let pending = page.frame_call(
        &cross2.frame,
        "Runtime.evaluate",
        json!({ "expression": "new Promise(() => {})", "awaitPromise": true,
            "contextId": cross2.context.as_ref().unwrap().id }),
    );
    let remove = async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        page.evaluate("document.getElementById('cross').remove(); 1")
            .await
            .unwrap();
    };
    let started = Instant::now();
    let (outcome, ()) = tokio::join!(pending, remove);
    assert_eq!(outcome, Err(RuntimeError::Detached));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );

    // Both frames and their sessions are gone.
    let remaining = frames::settled(&page, &paths[..1]).await;
    assert_eq!(
        frames::frame_ids(&remaining),
        std::slice::from_ref(&main.frame)
    );
    for gone in [&cross2.frame, &leaf.frame] {
        assert_eq!(
            page.frame_call(gone, "Runtime.evaluate", json!({ "expression": "1" }))
                .await,
            Err(RuntimeError::FrameGone)
        );
    }
    let observation = frames::observe(&page, "oopif-2").await;
    assert!(
        !observation.text().contains("Cross button"),
        "{}",
        observation.text()
    );
    assert!(
        !observation.text().contains("Leaf button"),
        "{}",
        observation.text()
    );

    drop(page);
    fixture.shutdown().await.expect("teardown");
    assert_eq!(support::sandbox_processes(), Vec::<u32>::new());
}
