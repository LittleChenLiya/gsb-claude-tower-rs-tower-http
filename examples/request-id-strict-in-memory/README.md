# Strict request ids in memory

Demonstrates the strict request-id layers (`StrictSetRequestIdLayer` and
`StrictPropagateRequestIdLayer`) without binding any network socket: requests are passed straight
through the in-memory tower service.

Three requests are sent:

1. A request with a valid `x-request-id` — the id is echoed verbatim even though the handler
   mutates the request header.
2. A request without an id — the generator is consulted exactly once and its value is sealed.
3. A request with a duplicated `x-request-id` header — rejected with `400 Bad Request` before the
   handler runs, and the offending values are never reflected in the response.

Run it with:

```not_rust
cargo run -p request-id-strict-in-memory
```
