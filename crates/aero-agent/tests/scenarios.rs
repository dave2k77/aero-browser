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
