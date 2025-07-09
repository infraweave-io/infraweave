# InfraWeave Web Server

This package provides a REST-API interface to interact with the InfraWeave platform. It is primarily built for use with the Backstage plugin, but is designed to be generic for any integration.

The server is built to run with access to the central account and includes JWT-based authentication for secure project access control.

## Authentication

The server supports multiple authentication modes:

### 1️⃣ 🚨 Development Mode (INSECURE)

**⚠️ WARNING: Only use this in development environments! This completely bypasses all authentication.**

```bash
DISABLE_JWT_AUTH_INSECURE=true \
AWS_PROFILE=central \
AWS_REGION=us-west-2 \
LOG_LEVEL=info \
INFRAWEAVE_ENV=dev \
cargo run -p webserver-openapi
```

Then open http://localhost:8081/swagger-ui/# and start making requests without any authentication.

### 2️⃣ 🔐 Production Mode with JWT Authentication

For production deployments, JWT authentication is required. The server supports two JWT verification methods.

#### Method 1: Static HMAC Key (Development/Testing)

For development and testing with a static secret:

```bash
JWT_SIGNING_KEY=a-string-secret-at-least-256-bits-long \
JWT_PROJECT_CLAIM_KEY=infraweave-projects \
JWT_AUDIENCE=infraweave-api \
AWS_PROFILE=central \
AWS_REGION=us-west-2 \
LOG_LEVEL=info \
INFRAWEAVE_ENV=dev \
cargo run -p webserver-openapi
```

#### Method 2: Identity Provider (Production - Recommended)

For production with a proper identity provider. You can either specify the JWKS endpoint directly or just the issuer (auto-discovery):

```bash
JWKS_URL=https://your-identity-provider.com/.well-known/jwks.json \
JWT_PROJECT_CLAIM_KEY=infraweave-projects \
JWT_AUDIENCE=infraweave-api \
JWT_ISSUER=https://your-identity-provider.com \
AWS_PROFILE=central \
AWS_REGION=us-west-2 \
LOG_LEVEL=info \
INFRAWEAVE_ENV=dev \
cargo run -p webserver-openapi
```

> **💡 Auto-discovery**: When `JWKS_URL` is omitted, the server automatically appends `/.well-known/jwks.json` to the `JWT_ISSUER` URL to discover the JWKS endpoint.

> **💡 Note**: `JWT_PROJECT_CLAIM_KEY=infraweave-projects` is used in all examples above because this is the standard claim name for InfraWeave. You only need to change this if your identity provider uses a different claim name (see [Integration Examples](#integration-examples) below).

## Environment Variables Reference

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DISABLE_JWT_AUTH_INSECURE` | No | `false` | **INSECURE**: Completely bypass authentication (dev only!) |
| `JWT_PROJECT_CLAIM_KEY` | Yes* | `infraweave-projects` | JWT claim key containing project access list |
| `JWT_AUDIENCE` | No | `infraweave-api` | Expected JWT audience claim |
| `JWT_SIGNING_KEY` | No** | - | Static HMAC signing key (256+ bits) |
| `JWKS_URL` | No** | - | JWKS endpoint URL for RSA verification |
| `JWT_ISSUER` | No** | - | JWT issuer (auto-discovers JWKS if set) |

\* Required unless `DISABLE_JWT_AUTH_INSECURE=true`. Most deployments should use `projects`
\*\* At least one verification method required for production

## Creating JWT Tokens

### For Development (Static Key)

You can generate test JWT tokens using [jwt.io](https://jwt.io):

**Header:**
```json
{
  "alg": "HS256",
  "typ": "JWT"
}
```

**Payload:**
```json
{
  "aud": "infraweave-api",
  "sub": "test-user-123", 
  "infraweave-projects": [
    "111111111111",
    "222222222222"
  ],
  "exp": 1999999999
}
```

**Secret:**
```
a-string-secret-at-least-256-bits-long
```

![](images/jwt.io.png)

### Example Token

The above configuration generates this token:
```
eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJhdWQiOiJpbmZyYXdlYXZlLWFwaSIsInN1YiI6InRlc3QtdXNlci0xMjMiLCJpbmZyYXdlYXZlLXByb2plY3RzIjpbIjExMTExMTExMTExMSIsIjIyMjIyMjIyMjIyMiJdLCJleHAiOjE5OTk5OTk5OTl9.0mujfO7hcDutgD2PVg9u4-9ppyeMTg8MqifzP-42k4A
```

### Using in Swagger UI

1. Open http://localhost:8081/swagger-ui/#
2. Click the "Authorize" button
3. Enter: `<your-jwt-token>`
4. Click "Authorize"

![](images/swagger-auth.png)

## JWT Claims Structure

The JWT token must include:

- **`aud`** (audience): Must match `JWT_AUDIENCE` (default: "infraweave-api")
- **`sub`** (subject): User identifier (or `oid`, `user_id`, `username`, `email`, `upn`, `appid`)
- **`infraweave-projects`**: Array of project IDs or single project ID string
- **`exp`** (expiration): Token expiration timestamp

### Project Access Control

The `projects` claim determines which InfraWeave projects the user can access:

```json
{
   ...
  "infraweave-projects": ["111111111111", "222222222222"]
}
```

Or single project:
```json
{
  ...
  "infraweave-projects": "111111111111"
}
```

## Integration Examples

> ⚠️ Below are suggestions that have not yet been verified

### Azure AD / Entra ID

When using Azure AD groups for project access control:

```bash
JWT_ISSUER=https://login.microsoftonline.com/your-tenant-id/v2.0 \
JWT_PROJECT_CLAIM_KEY=groups \
JWT_AUDIENCE=api://infraweave-api \
cargo run -p webserver-openapi
```

> **Note**: Using `groups` claim means your Azure AD group names must match your InfraWeave project IDs.

### Okta

Standard Okta setup with custom `infraweave-projects` claim:

```bash
JWT_ISSUER=https://your-domain.okta.com/oauth2/default \
JWT_PROJECT_CLAIM_KEY=infraweave-projects \
JWT_AUDIENCE=api://infraweave-api \
cargo run -p webserver-openapi
```

### Auth0

Auth0 with namespaced custom claims:

```bash
JWT_ISSUER=https://your-domain.auth0.com/ \
JWT_PROJECT_CLAIM_KEY=infraweave-projects \
JWT_AUDIENCE=https://api.infraweave.io \
cargo run -p webserver-openapi
```
