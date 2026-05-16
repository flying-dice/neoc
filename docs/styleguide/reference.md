# `POST /auth/token`

**Stable**

The **`POST /auth/token`** endpoint issues an access token using [OAuth 2.0](https://datatracker.ietf.org/doc/html/rfc6749) client credentials grant. Client exchanges credentials for a bearer token authorizing subsequent requests.

|Property|Value|
|---|---|
|HTTP method|`POST`|
|Authentication|Client credentials (ID + secret)|
|Content type|`application/x-www-form-urlencoded`|
|Rate limited|60 requests/minute/client|

## Syntax

```http
POST /auth/token HTTP/1.1
Host: api.example.com
Content-Type: application/x-www-form-urlencoded

client_id=<client_id>&client_secret=<client_secret>&grant_type=<grant_type>
```

## Parameters

`client_id`
:   **String, required.** Unique identifier from [client registration](/docs/authentication/guides/oauth2#client-registration).

`client_secret`
:   **String, required.** Secret paired with `client_id`. Must remain confidential — never expose in client-side code.

`grant_type`
:   **String, required.** Must be `"client_credentials"`. No other grant types accepted.

## Description

Validates `client_id` and `client_secret` against registered credentials. On success, returns JSON with bearer access token, type, and lifetime in seconds.

Token scope limited to permissions assigned at registration. `expires_in` indicates seconds until expiration — request new tokens before expiry.

> **Warning:** `client_secret` transmitted in request body. HTTPS required. Unencrypted connections expose credentials to interception.

## Examples

### Requesting an access token

```http
POST /auth/token HTTP/1.1
Host: api.example.com
Content-Type: application/x-www-form-urlencoded

client_id=app_8f3k2m&client_secret=sk_live_abc123xyz&grant_type=client_credentials
```

Response:

```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "access_token": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIi...",
  "token_type": "Bearer",
  "expires_in": 3600
}
```

Use `access_token` in subsequent requests via [`Authorization`](/docs/http/reference/headers/authorization) header:

```http
GET /resources HTTP/1.1
Authorization: Bearer eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIi...
```

### Invalid credentials

```http
HTTP/1.1 401 Unauthorized
Content-Type: application/json

{
  "error": "invalid_client",
  "error_description": "Client authentication failed."
}
```

## Error responses

|Status|Error code|Description|
|---|---|---|
|`400 Bad Request`|`invalid_request`|Required parameter missing or malformed|
|`401 Unauthorized`|`invalid_client`|Credentials incorrect|
|`429 Too Many Requests`|`rate_limit`|Rate limit exceeded|

## Specifications

|Specification|Status|
|---|---|
|[RFC 6749 — OAuth 2.0 Authorization Framework](https://datatracker.ietf.org/doc/html/rfc6749)|Internet Standard|
|[RFC 6750 — Bearer Token Usage](https://datatracker.ietf.org/doc/html/rfc6750)|Internet Standard|

## See also

- [`POST /auth/revoke`](/docs/authentication/reference/endpoints/auth-revoke)
- [`Authorization` header](/docs/http/reference/headers/authorization)
- [Using OAuth 2.0](/docs/authentication/guides/oauth2)
- [`429 Too Many Requests`](/docs/http/reference/status/429)
