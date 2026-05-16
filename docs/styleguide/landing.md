# Authentication

Authentication controls how clients prove identity to the API. Covers OAuth 2.0, API keys, and session tokens.

## Guides

[Using OAuth 2.0 with your application](/docs/authentication/guides/oauth2)
:   Client credentials flow from registration to token refresh, with code examples.

[Authenticating with API keys](/docs/authentication/guides/api-keys)
:   Generate, rotate, and scope API keys for server-to-server authentication.

[Managing session tokens](/docs/authentication/guides/session-tokens)
:   Session-based auth for browser clients: storage, expiration, renewal.

## Reference

### Headers

- [`Authorization`](/docs/authentication/reference/headers/authorization) — Transmits credentials or bearer tokens with each request.

### Endpoints

- [`POST /auth/token`](/docs/authentication/reference/endpoints/auth-token) — Issues access token via OAuth 2.0 client credentials grant.
- [`POST /auth/revoke`](/docs/authentication/reference/endpoints/auth-revoke) — Invalidates active access or refresh token.

## See also

- [Security best practices](/docs/security/guides/best-practices)
- [Rate limiting](/docs/platform/guides/rate-limiting) — auth interaction with per-client rate limits
