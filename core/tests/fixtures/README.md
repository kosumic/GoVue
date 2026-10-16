`ai-test-private.der` and `ai-test-jwks.json` form a generated RSA key pair used only to sign and verify synthetic OAuth identities in the provider boundary tests. This publicly committed fixture is not an application credential and must never be used outside tests.

`ai-test-ec-private.der` and `ai-test-ec-jwks.json` are a generated P-256/ES256 pair for the xAI OIDC tests. They are also public test fixtures, unrelated to any real account or service credential.
