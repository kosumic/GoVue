use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde_json::{Value, json};

#[test]
fn optional_not_before_claim_rejects_malformed_and_future_values() {
    let secret = b"synthetic-jwt-regression-key";
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_nbf = true;
    let verify = |claims: &Value| {
        let token = encode(
            &Header::default(),
            claims,
            &EncodingKey::from_secret(secret),
        )?;
        decode::<Value>(&token, &DecodingKey::from_secret(secret), &validation)
    };
    let claims = json!({"sub":"fixture", "exp":4_000_000_000_u64});
    assert!(verify(&claims).is_ok());
    for nbf in [json!("4000000000"), json!(null), json!({}), json!(true)] {
        let mut malformed = claims.clone();
        malformed["nbf"] = nbf;
        assert!(verify(&malformed).is_err());
    }
    let mut future = claims.clone();
    future["nbf"] = json!(4_000_000_000_u64);
    assert!(verify(&future).is_err());
    let mut past = claims;
    past["nbf"] = json!(1);
    assert!(verify(&past).is_ok());
}
