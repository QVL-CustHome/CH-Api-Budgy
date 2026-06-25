mod support;

use ch_api_budgy::adapters::bank::enable_banking::jwt::SignataireJwt;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use support::paire_rsa_test;

const APP_ID: &str = "11111111-2222-3333-4444-555555555555";

#[derive(Debug, Deserialize)]
struct ClaimsApplicatives {
    iss: String,
    aud: String,
    iat: i64,
    exp: i64,
}

#[test]
fn le_header_porte_alg_rs256_et_kid_app_id() {
    let paire = paire_rsa_test();
    let signataire = SignataireJwt::nouveau(APP_ID, paire.privee_pem.as_bytes())
        .expect("clé privée RSA de test valide");

    let jeton = signataire.jeton().expect("signature du jeton de test");
    let header = decode_header(&jeton).expect("header JWT lisible");

    assert_eq!(header.alg, Algorithm::RS256);
    assert_eq!(header.typ.as_deref(), Some("JWT"));
    assert_eq!(header.kid.as_deref(), Some(APP_ID));
}

#[test]
fn les_claims_portent_issuer_audience_et_expiration_bornee() {
    let paire = paire_rsa_test();
    let signataire = SignataireJwt::nouveau(APP_ID, paire.privee_pem.as_bytes())
        .expect("clé privée RSA de test valide");

    let jeton = signataire.jeton().expect("signature du jeton de test");

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["enablebanking.com"]);
    validation.set_audience(&["api.enablebanking.com"]);
    validation.set_required_spec_claims(&["exp"]);

    let cle = DecodingKey::from_rsa_pem(paire.publique_pem.as_bytes())
        .expect("clé publique RSA de test valide");
    let donnees = decode::<ClaimsApplicatives>(&jeton, &cle, &validation)
        .expect("jeton vérifiable avec la clé publique");

    assert_eq!(donnees.claims.iss, "enablebanking.com");
    assert_eq!(donnees.claims.aud, "api.enablebanking.com");
    assert!(donnees.claims.exp > donnees.claims.iat);
    assert!(donnees.claims.exp - donnees.claims.iat <= 86_400);
}

#[test]
fn le_jeton_est_reutilise_tant_qu_il_reste_valide() {
    let paire = paire_rsa_test();
    let signataire = SignataireJwt::nouveau(APP_ID, paire.privee_pem.as_bytes())
        .expect("clé privée RSA de test valide");

    let premier = signataire.jeton().expect("premier jeton");
    let second = signataire.jeton().expect("second jeton");

    assert_eq!(premier, second);
}
