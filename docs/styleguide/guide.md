# How rate limiting works

Rate limiting via token bucket algorithm. Covers: bucket mechanics, HTTP headers, [`429 Too Many Requests`](/docs/http/reference/status/429) handling. Assumes HTTP request-response familiarity.

## The token bucket algorithm

Each client gets a bucket with fixed token capacity. Every request consumes one token. Tokens replenish at constant rate — 120/minute for most endpoints.

Full bucket discards overflow (no accumulation beyond max). Empty bucket → server rejects with `429` until tokens replenish.

Permits short bursts up to capacity while enforcing sustainable average rate.

> **Note:** Rate limits are per `client_id`. Multiple users sharing credentials share one bucket.

## Rate limit headers

Every response includes bucket state headers:

`X-RateLimit-Limit`
:   Maximum requests permitted in current window. Equals bucket capacity.

`X-RateLimit-Remaining`
:   Requests available before exhaustion. Decreases per request, increases on replenish.

`Retry-After`
:   Present only on `429` responses. Seconds until at least one token available.

### Reading headers

```http
HTTP/1.1 200 OK
Content-Type: application/json
X-RateLimit-Limit: 120
X-RateLimit-Remaining: 87

{
  "data": { "id": "usr_12345", "name": "Jane Doe" }
}
```

`X-RateLimit-Remaining: 87` — 87 requests remain before bucket empties.

## Handling `429` responses

Empty bucket → `429` with `Retry-After`:

```http
HTTP/1.1 429 Too Many Requests
Content-Type: application/json
Retry-After: 12
X-RateLimit-Limit: 120
X-RateLimit-Remaining: 0

{
  "error": "rate_limit_exceeded",
  "error_description": "Request limit reached. Retry after 12 seconds."
}
```

Respect `Retry-After`. Continuing to send does not accelerate replenishment and may extend throttling.

### Retry logic

Exponential backoff as fallback when `Retry-After` absent or retry still returns `429`:

1. Read `Retry-After` value
2. Wait specified seconds
3. Retry
4. If still `429`, double wait (max 60s)
5. After five consecutive `429`s, log failure and stop

```python
import time
import requests

def request_with_retry(url, headers, max_retries=5):
    wait = 1
    for attempt in range(max_retries):
        response = requests.get(url, headers=headers)
        if response.status_code != 429:
            return response
        retry_after = int(response.headers.get("Retry-After", wait))
        time.sleep(retry_after)
        wait = min(wait * 2, 60)
    return response
```

## Rate limits by endpoint

|Endpoint category|Requests/min|Bucket size|
|---|---|---|
|Authentication|60|60|
|Read (GET)|120|120|
|Write (POST)|60|60|
|Bulk operations|10|10|

> **Warning:** Bulk endpoints have significantly lower limits. Monitor `X-RateLimit-Remaining` and implement request queuing.

## See also

- [`429 Too Many Requests`](/docs/http/reference/status/429)
- [`Retry-After` header](/docs/http/reference/headers/retry-after)
- [Authentication](/docs/authentication/) — rate limits scoped per authenticated client
- [Error handling](/docs/platform/guides/error-handling)
- [RFC 6585 — Additional HTTP Status Codes](https://datatracker.ietf.org/doc/html/rfc6585)
