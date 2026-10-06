use std::convert::Infallible;

use crate::{cors::Vary, test_helpers::Body};
use http::{header, HeaderName, HeaderValue, Method, Request, Response};
use tower::{service_fn, util::ServiceExt, Layer};

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

mod decision {
    use std::convert::Infallible;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    use crate::test_helpers::Body;
    use http::{header, HeaderValue, Method, Request, Response, StatusCode};
    use tower::{service_fn, util::ServiceExt, Layer, Service};

    use crate::cors::{
        AllowCredentials, AllowOrigin, Any, CorsLayer, Decision, DecisionLayer, DenyReason,
        InvalidCorsPolicy,
    };

    const ALLOWED_ORIGIN: HeaderValue = HeaderValue::from_static("https://allowed.example");

    fn sealed_layer() -> DecisionLayer {
        DecisionLayer::from_layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list([ALLOWED_ORIGIN]))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE])
                .allow_credentials(true),
        )
        .unwrap()
    }

    fn counting_service(
        hits: &Arc<AtomicUsize>,
    ) -> impl Service<
        Request<Body>,
        Response = Response<Body>,
        Error = Infallible,
        Future = impl std::future::Future<Output = Result<Response<Body>, Infallible>>,
    > + Clone {
        let hits = Arc::clone(hits);
        service_fn(move |_: Request<Body>| {
            hits.fetch_add(1, Ordering::SeqCst);
            async move { Ok::<_, Infallible>(Response::new(Body::empty())) }
        })
    }

    #[tokio::test]
    async fn seal_rejects_credentials_with_wildcard_origin() {
        let err = CorsLayer::new()
            .allow_credentials(true)
            .allow_origin(Any)
            .seal()
            .unwrap_err();
        assert_eq!(err, InvalidCorsPolicy::CredentialsWithWildcardOrigin);
    }

    #[tokio::test]
    async fn seal_rejects_credentials_with_other_wildcards() {
        let err = CorsLayer::new()
            .allow_credentials(true)
            .allow_methods(Any)
            .seal()
            .unwrap_err();
        assert_eq!(err, InvalidCorsPolicy::CredentialsWithWildcardAllowMethods);

        let err = CorsLayer::new()
            .allow_credentials(true)
            .allow_headers(Any)
            .seal()
            .unwrap_err();
        assert_eq!(err, InvalidCorsPolicy::CredentialsWithWildcardAllowHeaders);

        let err = CorsLayer::new()
            .allow_credentials(true)
            .expose_headers(Any)
            .seal()
            .unwrap_err();
        assert_eq!(err, InvalidCorsPolicy::CredentialsWithWildcardExposeHeaders);
    }

    #[tokio::test]
    async fn preflight_and_actual_request_share_sealed_policy() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = sealed_layer().layer(counting_service(&hits));

        // Preflight for `GET` is allowed by the sealed policy.
        let preflight = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(preflight).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&ALLOWED_ORIGIN)
        );
        assert_eq!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_METHODS),
            Some(&HeaderValue::from_static("GET,POST"))
        );
        assert_eq!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_HEADERS),
            Some(&HeaderValue::from_static("content-type"))
        );
        assert_eq!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            Some(&HeaderValue::from_static("true"))
        );
        // preflights never execute the inner service
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        // The actual request is evaluated against the same sealed policy.
        let actual = Request::builder()
            .method(Method::GET)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(actual).await.unwrap();

        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&ALLOWED_ORIGIN)
        );
        assert_eq!(
            res.headers().get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS),
            Some(&HeaderValue::from_static("true"))
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn denied_request_does_not_execute_inner_service_and_does_not_echo_origin() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = sealed_layer().layer(counting_service(&hits));

        let req = Request::builder()
            .method(Method::GET)
            .header(header::ORIGIN, "https://evil.example")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        // the unknown origin is not echoed back
        assert!(res
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
        // the business service was not executed
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        // the policy's Vary values are still emitted for cache correctness
        assert_eq!(
            res.headers().get(header::VARY),
            Some(&HeaderValue::from_static("origin"))
        );
    }

    #[tokio::test]
    async fn denied_preflight_does_not_execute_inner_service() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = sealed_layer().layer(counting_service(&hits));

        let req = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "DELETE")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();

        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        assert!(res
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .is_none());
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn methods_are_case_sensitive_per_http_spec() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = sealed_layer().layer(counting_service(&hits));

        // `get` is not `GET`: HTTP methods are case-sensitive tokens.
        let req = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "get")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);

        let req = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn headers_are_case_insensitive_per_http_spec() {
        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = sealed_layer().layer(counting_service(&hits));

        // Field names are case-insensitive: `CONTENT-TYPE` matches the
        // configured `content-type`.
        let req = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "CONTENT-TYPE")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let req = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "x-not-allowed")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn vary_merging_preserves_existing_values() {
        async fn inner_svc(_: Request<Body>) -> Result<Response<Body>, Infallible> {
            Ok(Response::builder()
                .header(header::VARY, "accept, accept-encoding")
                .body(Body::empty())
                .unwrap())
        }

        let mut svc = sealed_layer().layer(service_fn(inner_svc));

        let req = Request::builder()
            .method(Method::GET)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();

        let vary = res
            .headers()
            .get_all(header::VARY)
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        // the inner service's values are kept and the policy's are appended
        assert_eq!(
            vary,
            vec![
                HeaderValue::from_static("accept, accept-encoding"),
                HeaderValue::from_static("origin"),
            ]
        );
    }

    #[tokio::test]
    async fn dynamic_origin_policy_is_evaluated_once_per_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let predicate_calls = Arc::clone(&calls);
        let layer = DecisionLayer::from_layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(move |origin, _| {
                    predicate_calls.fetch_add(1, Ordering::SeqCst);
                    origin == ALLOWED_ORIGIN
                }))
                .allow_methods([Method::GET]),
        )
        .unwrap();

        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = layer.layer(counting_service(&hits));

        let req = Request::builder()
            .method(Method::GET)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let req = Request::builder()
            .method(Method::OPTIONS)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dynamic_async_origin_policy_is_evaluated_once_per_request() {
        let calls = Arc::new(AtomicUsize::new(0));
        let predicate_calls = Arc::clone(&calls);
        let layer = DecisionLayer::from_layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::async_predicate(move |origin, _| {
                    predicate_calls.fetch_add(1, Ordering::SeqCst);
                    async move { origin == ALLOWED_ORIGIN }
                }))
                .allow_methods([Method::GET]),
        )
        .unwrap();

        let hits = Arc::new(AtomicUsize::new(0));
        let mut svc = layer.layer(counting_service(&hits));

        let req = Request::builder()
            .method(Method::GET)
            .header(header::ORIGIN, ALLOWED_ORIGIN)
            .body(Body::empty())
            .unwrap();
        let res = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// Table-driven policy checks: each row seals a configuration, evaluates
    /// one request through `CorsPolicy::decide`, and compares the explicit
    /// decision snapshot against the expected outcome.
    #[tokio::test]
    async fn policy_table() {
        #[derive(Debug, PartialEq)]
        enum Expected {
            Allow { preflight: bool },
            Deny(DenyReason),
        }

        struct Case {
            name: &'static str,
            layer: CorsLayer,
            method: Method,
            origin: Option<&'static str>,
            request_method: Option<&'static str>,
            request_headers: Option<&'static str>,
            expected: Expected,
        }

        fn base() -> CorsLayer {
            CorsLayer::new()
                .allow_origin(AllowOrigin::list([ALLOWED_ORIGIN]))
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([header::CONTENT_TYPE])
        }

        let cases = vec![
            Case {
                name: "known origin, actual request",
                layer: base(),
                method: Method::GET,
                origin: Some("https://allowed.example"),
                request_method: None,
                request_headers: None,
                expected: Expected::Allow { preflight: false },
            },
            Case {
                name: "unknown origin is rejected, not echoed",
                layer: base(),
                method: Method::GET,
                origin: Some("https://evil.example"),
                request_method: None,
                request_headers: None,
                expected: Expected::Deny(DenyReason::OriginNotAllowed),
            },
            Case {
                name: "no origin header passes through",
                layer: base(),
                method: Method::GET,
                origin: None,
                request_method: None,
                request_headers: None,
                expected: Expected::Allow { preflight: false },
            },
            Case {
                name: "preflight with allowed method",
                layer: base(),
                method: Method::OPTIONS,
                origin: Some("https://allowed.example"),
                request_method: Some("POST"),
                request_headers: None,
                expected: Expected::Allow { preflight: true },
            },
            Case {
                name: "preflight with disallowed method",
                layer: base(),
                method: Method::OPTIONS,
                origin: Some("https://allowed.example"),
                request_method: Some("DELETE"),
                request_headers: None,
                expected: Expected::Deny(DenyReason::MethodNotAllowed),
            },
            Case {
                name: "preflight method is case-sensitive",
                layer: base(),
                method: Method::OPTIONS,
                origin: Some("https://allowed.example"),
                request_method: Some("get"),
                request_headers: None,
                expected: Expected::Deny(DenyReason::MethodNotAllowed),
            },
            Case {
                name: "preflight header names are case-insensitive",
                layer: base(),
                method: Method::OPTIONS,
                origin: Some("https://allowed.example"),
                request_method: Some("GET"),
                request_headers: Some("Content-Type"),
                expected: Expected::Allow { preflight: true },
            },
            Case {
                name: "preflight with unknown header",
                layer: base(),
                method: Method::OPTIONS,
                origin: Some("https://allowed.example"),
                request_method: Some("GET"),
                request_headers: Some("x-evil"),
                expected: Expected::Deny(DenyReason::RequestHeadersNotAllowed),
            },
            Case {
                name: "predicate credentials with wildcard origin denied at request time",
                layer: CorsLayer::new()
                    .allow_origin(Any)
                    .allow_credentials(AllowCredentials::predicate(|_, _| true)),
                method: Method::GET,
                origin: Some("https://allowed.example"),
                request_method: None,
                request_headers: None,
                expected: Expected::Deny(DenyReason::CredentialsWithWildcardOrigin),
            },
            Case {
                name: "plain OPTIONS without request-method is an actual request",
                layer: base(),
                method: Method::OPTIONS,
                origin: Some("https://allowed.example"),
                request_method: None,
                request_headers: None,
                expected: Expected::Allow { preflight: false },
            },
        ];

        for case in cases {
            let name = case.name;
            let policy = case
                .layer
                .seal()
                .unwrap_or_else(|err| panic!("case `{}` failed to seal: {}", name, err));

            let mut builder = Request::builder().method(case.method);
            if let Some(origin) = case.origin {
                builder = builder.header(header::ORIGIN, origin);
            }
            if let Some(request_method) = case.request_method {
                builder = builder.header(header::ACCESS_CONTROL_REQUEST_METHOD, request_method);
            }
            if let Some(request_headers) = case.request_headers {
                builder = builder.header(header::ACCESS_CONTROL_REQUEST_HEADERS, request_headers);
            }
            let (parts, _) = builder.body(()).unwrap().into_parts();

            let decision = policy.decide(&parts).await;
            let actual = match &decision {
                Decision::Allow(allow) => Expected::Allow {
                    preflight: allow.is_preflight(),
                },
                Decision::Deny(deny) => {
                    // denied decisions never echo the request origin
                    assert!(
                        deny.headers()
                            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                            .is_none(),
                        "case `{}` echoes the origin on deny",
                        name
                    );
                    Expected::Deny(deny.reason())
                }
            };
            assert_eq!(actual, case.expected, "case `{}`", name);
        }
    }
}
