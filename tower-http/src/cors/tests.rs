use std::convert::Infallible;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use crate::{cors::Vary, test_helpers::Body};
use http::{header, HeaderName, HeaderValue, Method, Request, Response, StatusCode};
use tower::{service_fn, util::ServiceExt, Layer};

use crate::cors::policy::{CorsPolicy, Decision, DenyReason, PolicyError};
use crate::cors::{AllowHeaders, AllowMethods, AllowOrigin, Any, Cors, CorsLayer};

const INITIAL_VARY_HEADERS: HeaderValue = HeaderValue::from_static("accept, accept-encoding");
const ADDITIONAL_VARY_HEADERS: [HeaderName; 3] = [
    header::ORIGIN,
    header::ACCESS_CONTROL_REQUEST_METHOD,
    header::ACCESS_CONTROL_REQUEST_HEADERS,
];

#[tokio::test]
async fn permissive_vary_header_is_empty() {
    let svc = CorsLayer::permissive().layer(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }));

    let req = Request::builder().body(Body::empty()).unwrap();

    let res = svc.oneshot(req).await.unwrap();
    assert!(
        res.headers().get(header::VARY).is_none(),
        "Vary header should be omitted for permissive config"
    );
}

#[tokio::test]
async fn include_custom_permissive_to_vary_set_by_inner_service() {
    const PERMISSIVE_CORS_VARY_HEADERS: HeaderValue = HeaderValue::from_static(
        "origin, access-control-request-method, access-control-request-headers",
    );

    async fn inner_svc(_: Request<Body>) -> Result<Response<Body>, Infallible> {
        Ok(Response::builder()
            .header(header::VARY, INITIAL_VARY_HEADERS)
            .body(Body::empty())
            .unwrap())
    }

    let svc = CorsLayer::permissive()
        .vary(Vary::list(ADDITIONAL_VARY_HEADERS))
        .layer(service_fn(inner_svc));

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();
    let mut vary_headers = res.headers().get_all(header::VARY).into_iter();
    assert_eq!(vary_headers.next(), Some(&INITIAL_VARY_HEADERS));
    assert_eq!(vary_headers.next(), Some(&PERMISSIVE_CORS_VARY_HEADERS));
    assert_eq!(vary_headers.next(), None);
}

#[tokio::test]
async fn permissive_with_custom_vary_builder() {
    let custom_vary = HeaderValue::from_static("x-foo");
    let svc = CorsLayer::permissive()
        .vary(Vary::list([header::HeaderName::from_static("x-foo")]))
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    let req = Request::builder().body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();
    let vary = res.headers().get(header::VARY);
    assert_eq!(vary, Some(&custom_vary));
}

#[tokio::test]
async fn permissive_with_inner_and_builder_vary() {
    let custom_vary = HeaderValue::from_static("x-foo");
    let inner_vary = HeaderValue::from_static("accept-encoding");
    let svc = CorsLayer::permissive()
        .vary(Vary::list([header::HeaderName::from_static("x-foo")]))
        .layer(service_fn(|_: Request<Body>| {
            let inner_vary = inner_vary.clone();
            async move {
                Ok::<_, Infallible>(
                    Response::builder()
                        .header(header::VARY, inner_vary)
                        .body(Body::empty())
                        .unwrap(),
                )
            }
        }));

    let req = Request::builder().body(Body::empty()).unwrap();
    let res = svc.oneshot(req).await.unwrap();
    let mut vary_headers = res.headers().get_all(header::VARY).iter();
    assert_eq!(vary_headers.next(), Some(&inner_vary));
    assert_eq!(vary_headers.next(), Some(&custom_vary));
    assert_eq!(vary_headers.next(), None);
}

#[tokio::test]
async fn test_allow_origin_async_predicate() {
    #[derive(Clone)]
    struct Client;

    impl Client {
        async fn fetch_allowed_origins_for_path(&self, _path: String) -> Vec<HeaderValue> {
            vec![HeaderValue::from_static("http://example.com")]
        }
    }

    let client = Client;

    let allow_origin = AllowOrigin::async_predicate(|origin, parts| {
        let path = parts.uri.path().to_owned();

        async move {
            let origins = client.fetch_allowed_origins_for_path(path).await;

            origins.contains(&origin)
        }
    });

    let valid_origin = HeaderValue::from_static("http://example.com");
    let parts = http::Request::new("hello world").into_parts().0;

    let header = allow_origin
        .to_future(Some(&valid_origin), &parts)
        .await
        .unwrap();
    assert_eq!(header.0, header::ACCESS_CONTROL_ALLOW_ORIGIN);
    assert_eq!(header.1, valid_origin);

    let invalid_origin = HeaderValue::from_static("http://example.org");
    let parts = http::Request::new("hello world").into_parts().0;

    let res = allow_origin.to_future(Some(&invalid_origin), &parts).await;
    assert!(res.is_none());
}

#[tokio::test]
async fn derived_vary_header_for_mixed_wildcard_configuration() {
    let svc = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get(header::VARY),
        Some(&HeaderValue::from_static(
            "access-control-request-method, access-control-request-headers",
        ))
    );
}

#[tokio::test]
async fn very_permissive_emits_vary_headers() {
    let svc = CorsLayer::very_permissive().layer(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }));

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get(header::VARY),
        Some(&HeaderValue::from_static(
            "origin, access-control-request-method, access-control-request-headers",
        ))
    );
}

#[tokio::test]
async fn cors_map_layer_smoke_without_vary_header() {
    let svc = Cors::new(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }))
    .allow_origin(Any)
    .allow_methods(Any)
    .allow_headers(Any);

    let res = svc.oneshot(Request::new(Body::empty())).await.unwrap();

    assert!(res.headers().get(header::VARY).is_none());
}

#[tokio::test]
async fn cors_map_layer_smoke_with_vary_header() {
    let svc = Cors::new(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }))
    .allow_origin(Any)
    .allow_methods(AllowMethods::mirror_request())
    .allow_headers(Any);

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert_eq!(
        res.headers().get(header::VARY),
        Some(&HeaderValue::from_static("access-control-request-method"))
    );
}

#[tokio::test]
async fn exact_origin_does_not_emit_origin_vary_header() {
    let svc = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(HeaderValue::from_static(
            "http://example.com",
        )))
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE])
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    let req = Request::builder()
        .header(header::ORIGIN, "http://example.com")
        .body(Body::empty())
        .unwrap();

    let res = svc.oneshot(req).await.unwrap();

    assert!(res.headers().get(header::VARY).is_none());
}

fn sealed_test_policy() -> CorsPolicy {
    CorsPolicy::builder()
        .allow_credentials(true)
        .allow_origins([HeaderValue::from_static("https://app.example")])
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE])
        .seal()
        .unwrap()
}

#[test]
fn policy_seal_rejects_credentials_combined_with_wildcards() {
    // The origin/credentials combination is checked first, even when
    // several wildcard rules are misconfigured at once.
    let err = CorsPolicy::builder()
        .allow_credentials(true)
        .allow_any_origin()
        .allow_any_methods()
        .allow_any_headers()
        .seal()
        .unwrap_err();
    assert_eq!(err, PolicyError::CredentialsWithWildcardOrigin);

    let err = CorsPolicy::builder()
        .allow_credentials(true)
        .allow_any_origin()
        .seal()
        .unwrap_err();
    assert_eq!(err, PolicyError::CredentialsWithWildcardOrigin);

    let err = CorsPolicy::builder()
        .allow_credentials(true)
        .allow_origins([HeaderValue::from_static("https://app.example")])
        .allow_any_methods()
        .seal()
        .unwrap_err();
    assert_eq!(err, PolicyError::CredentialsWithWildcardMethods);

    let err = CorsPolicy::builder()
        .allow_credentials(true)
        .allow_origins([HeaderValue::from_static("https://app.example")])
        .allow_methods([Method::GET])
        .allow_any_headers()
        .seal()
        .unwrap_err();
    assert_eq!(err, PolicyError::CredentialsWithWildcardHeaders);

    // Without credentials, wildcards are fine.
    assert!(CorsPolicy::builder()
        .allow_any_origin()
        .allow_any_methods()
        .allow_any_headers()
        .seal()
        .is_ok());
}

#[test]
fn policy_decision_snapshot_is_explicit_and_frozen() {
    let policy = sealed_test_policy();

    let req = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://app.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .body(())
        .unwrap();
    let (parts, _) = req.into_parts();
    match policy.decide(&parts) {
        Decision::Allow(snapshot) => {
            assert!(snapshot.is_preflight());
            assert_eq!(
                snapshot.allow_origin(),
                &HeaderValue::from_static("https://app.example")
            );
            assert!(snapshot.allow_credentials());
            assert_eq!(
                snapshot.allow_methods(),
                Some(&HeaderValue::from_static("GET,POST"))
            );
            assert_eq!(
                snapshot.allow_headers(),
                Some(&HeaderValue::from_static("content-type"))
            );
        }
        other => panic!("expected allow, got {:?}", other),
    }

    // Unknown origins are denied; the snapshot carries nothing to echo.
    let req = Request::builder()
        .header(header::ORIGIN, "https://evil.example")
        .body(())
        .unwrap();
    let (parts, _) = req.into_parts();
    assert_eq!(
        policy.decide(&parts),
        Decision::Deny(DenyReason::OriginNotAllowed)
    );

    // Requests without an Origin header are not CORS requests.
    let req = Request::builder().body(()).unwrap();
    let (parts, _) = req.into_parts();
    assert_eq!(policy.decide(&parts), Decision::PassThrough);
}

#[tokio::test]
async fn policy_preflight_and_actual_request_share_the_sealed_policy() {
    let svc = sealed_test_policy()
        .layer()
        .layer(service_fn(|_: Request<Body>| async {
            Ok::<_, Infallible>(Response::new(Body::empty()))
        }));

    // Preflight: the allowed method is answered from the sealed policy.
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://app.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();
    let res = svc.clone().oneshot(preflight).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://app.example"
    );
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS],
        "true"
    );
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
        "GET,POST"
    );
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "content-type"
    );

    // The actual request is answered with the same sealed policy.
    let actual = Request::builder()
        .method(Method::GET)
        .header(header::ORIGIN, "https://app.example")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(actual).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://app.example"
    );
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS],
        "true"
    );
}

#[tokio::test]
async fn policy_vary_merge_preserves_existing_values_without_duplicates() {
    async fn inner(_: Request<Body>) -> Result<Response<Body>, Infallible> {
        Ok(Response::builder()
            .header(header::VARY, "accept-encoding")
            .header(header::VARY, "Origin")
            .body(Body::empty())
            .unwrap())
    }

    let svc = sealed_test_policy().layer().layer(service_fn(inner));

    let req = Request::builder()
        .header(header::ORIGIN, "https://app.example")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(req).await.unwrap();

    let vary = res.headers().get(header::VARY).unwrap().to_str().unwrap();
    // Existing values are preserved...
    assert!(vary.contains("accept-encoding"), "vary: {}", vary);
    // ...the policy's own entries are merged in...
    assert!(
        vary.contains("access-control-request-method"),
        "vary: {}",
        vary
    );
    assert!(
        vary.contains("access-control-request-headers"),
        "vary: {}",
        vary
    );
    // ...and `Origin` (set by the inner service) is not duplicated by the
    // policy's `origin` entry.
    assert_eq!(vary.to_ascii_lowercase().matches("origin").count(), 1);
}

#[tokio::test]
async fn policy_denied_request_never_reaches_inner_service_and_never_echoes_origin() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let svc = sealed_test_policy()
        .layer()
        .layer(service_fn(move |_: Request<Body>| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, Infallible>(Response::new(Body::empty())) }
        }));

    // Actual request from an unknown origin.
    let req = Request::builder()
        .method(Method::GET)
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::empty())
        .unwrap();
    let res = svc.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(res
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
    assert!(res
        .headers()
        .values()
        .all(|value| value != "https://evil.example"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Preflight from an unknown origin.
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://evil.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(preflight).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(res
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
    assert!(res
        .headers()
        .values()
        .all(|value| value != "https://evil.example"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn policy_dynamic_origin_predicate_is_called_exactly_once_per_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let policy = CorsPolicy::builder()
        .allow_origin_predicate(move |origin: &HeaderValue, _| {
            counter.fetch_add(1, Ordering::SeqCst);
            origin.as_bytes().ends_with(b".example.com")
        })
        .allow_methods([Method::GET])
        .seal()
        .unwrap();

    let svc = policy.layer().layer(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }));

    let actual = Request::builder()
        .method(Method::GET)
        .header(header::ORIGIN, "https://a.example.com")
        .body(Body::empty())
        .unwrap();
    svc.clone().oneshot(actual).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://a.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();
    svc.oneshot(preflight).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn policy_header_matching_is_case_insensitive_and_method_matching_case_sensitive() {
    let policy = CorsPolicy::builder()
        .allow_origins([HeaderValue::from_static("https://app.example")])
        .allow_methods([Method::GET])
        .allow_headers([HeaderName::from_static("x-custom")])
        .seal()
        .unwrap();
    let svc = policy.layer().layer(service_fn(|_: Request<Body>| async {
        Ok::<_, Infallible>(Response::new(Body::empty()))
    }));

    // Header names are case-insensitive per the HTTP spec.
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://app.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "X-CuStOm")
        .body(Body::empty())
        .unwrap();
    let res = svc.clone().oneshot(preflight).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "x-custom"
    );

    // Methods are case-sensitive per the HTTP spec: `get` is not `GET`.
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .header(header::ORIGIN, "https://app.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "get")
        .body(Body::empty())
        .unwrap();
    let res = svc.oneshot(preflight).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

/// Table-driven test mirroring the policy decision table in
/// `crate::cors::policy`.
#[tokio::test]
async fn policy_decision_table() {
    struct Row {
        name: &'static str,
        origin: Option<&'static str>,
        method: Method,
        request_method: Option<&'static str>,
        request_headers: Option<&'static str>,
        want_status: StatusCode,
        want_allow_origin: Option<&'static str>,
        want_inner_calls: usize,
    }

    let table = [
        Row {
            name: "no origin passes through without CORS headers",
            origin: None,
            method: Method::GET,
            request_method: None,
            request_headers: None,
            want_status: StatusCode::OK,
            want_allow_origin: None,
            want_inner_calls: 1,
        },
        Row {
            name: "known origin actual request allowed",
            origin: Some("https://app.example"),
            method: Method::GET,
            request_method: None,
            request_headers: None,
            want_status: StatusCode::OK,
            want_allow_origin: Some("https://app.example"),
            want_inner_calls: 1,
        },
        Row {
            name: "unknown origin actual request denied",
            origin: Some("https://evil.example"),
            method: Method::GET,
            request_method: None,
            request_headers: None,
            want_status: StatusCode::FORBIDDEN,
            want_allow_origin: None,
            want_inner_calls: 0,
        },
        Row {
            name: "preflight with allowed method and headers",
            origin: Some("https://app.example"),
            method: Method::OPTIONS,
            request_method: Some("GET"),
            request_headers: Some("content-type"),
            want_status: StatusCode::OK,
            want_allow_origin: Some("https://app.example"),
            want_inner_calls: 0,
        },
        Row {
            name: "preflight with unknown origin denied without echo",
            origin: Some("https://evil.example"),
            method: Method::OPTIONS,
            request_method: Some("GET"),
            request_headers: None,
            want_status: StatusCode::FORBIDDEN,
            want_allow_origin: None,
            want_inner_calls: 0,
        },
        Row {
            name: "preflight with disallowed method denied",
            origin: Some("https://app.example"),
            method: Method::OPTIONS,
            request_method: Some("DELETE"),
            request_headers: None,
            want_status: StatusCode::FORBIDDEN,
            want_allow_origin: None,
            want_inner_calls: 0,
        },
        Row {
            name: "preflight with disallowed header denied",
            origin: Some("https://app.example"),
            method: Method::OPTIONS,
            request_method: Some("GET"),
            request_headers: Some("x-secret"),
            want_status: StatusCode::FORBIDDEN,
            want_allow_origin: None,
            want_inner_calls: 0,
        },
        Row {
            name: "preflight with malformed method denied",
            origin: Some("https://app.example"),
            method: Method::OPTIONS,
            request_method: Some("GE T"),
            request_headers: None,
            want_status: StatusCode::FORBIDDEN,
            want_allow_origin: None,
            want_inner_calls: 0,
        },
    ];

    for row in &table {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let svc = sealed_test_policy()
            .layer()
            .layer(service_fn(move |_: Request<Body>| {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, Infallible>(Response::new(Body::empty())) }
            }));

        let mut builder = Request::builder().method(row.method.clone());
        if let Some(origin) = row.origin {
            builder = builder.header(header::ORIGIN, origin);
        }
        if let Some(method) = row.request_method {
            builder = builder.header(header::ACCESS_CONTROL_REQUEST_METHOD, method);
        }
        if let Some(headers) = row.request_headers {
            builder = builder.header(header::ACCESS_CONTROL_REQUEST_HEADERS, headers);
        }

        let res = svc
            .oneshot(builder.body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(res.status(), row.want_status, "{}", row.name);
        assert_eq!(
            res.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .map(|value| value.to_str().unwrap()),
            row.want_allow_origin,
            "{}",
            row.name
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            row.want_inner_calls,
            "{}",
            row.name
        );
    }
}
