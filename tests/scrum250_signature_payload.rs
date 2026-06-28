mod support;

use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::ports::evenement_synchro::{EvenementSynchro, TypeEvenementSynchro};
use ch_api_budgy::relay::publisher::{construire_payload, topic_pour};
use ch_api_budgy::relay::signature::SignataireRs256;
use chrono::{TimeZone, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use support::paire_rsa_test;
use uuid::Uuid;

const SUB: &str = "5f1c7d2e-0000-4a11-9c33-000000000001";
const ISSUER: &str = "ch-api-budgy";
const PREFIXE: &str = "budgy";
const COMPTE: &str = "11111111-2222-3333-4444-555555555555";

#[derive(Debug, Deserialize)]
struct EnveloppeLue {
    iss: String,
    sub: String,
    event_type: String,
    #[serde(default)]
    account: Option<String>,
    #[serde(default)]
    count: Option<u64>,
    at: String,
    iat: i64,
    exp: i64,
}

fn signataire() -> SignataireRs256 {
    let paire = paire_rsa_test();
    SignataireRs256::nouveau(paire.privee_pem.as_bytes()).expect("clé privée RS256 de test valide")
}

fn signataire_et_cle_publique() -> (SignataireRs256, String) {
    let paire = paire_rsa_test();
    let signataire =
        SignataireRs256::nouveau(paire.privee_pem.as_bytes()).expect("clé privée RS256 valide");
    (signataire, paire.publique_pem)
}

fn proprietaire() -> ProprietaireId {
    ProprietaireId(SUB.to_string())
}

fn moment() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 27, 10, 0, 0).unwrap()
}

#[test]
fn le_message_publie_est_signe_en_rs256() {
    let signataire = signataire();
    let evenement =
        EvenementSynchro::sync_started(proprietaire(), COMPTE.to_string(), moment());

    let message = construire_payload(&evenement, ISSUER, &signataire).expect("payload signé");
    let header = decode_header(&message).expect("header JWT lisible");

    assert_eq!(header.alg, Algorithm::RS256);
    assert_eq!(header.typ.as_deref(), Some("JWT"));
}

#[test]
fn le_message_est_verifiable_avec_la_cle_publique() {
    let (signataire, cle_publique) = signataire_et_cle_publique();
    let evenement = EvenementSynchro::sync_succeeded(
        proprietaire(),
        COMPTE.to_string(),
        3,
        moment(),
    );

    let message = construire_payload(&evenement, ISSUER, &signataire).expect("payload signé");

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_required_spec_claims(&["exp"]);
    let cle = DecodingKey::from_rsa_pem(cle_publique.as_bytes()).expect("clé publique valide");
    let donnees =
        decode::<EnveloppeLue>(&message, &cle, &validation).expect("message vérifiable");

    assert_eq!(donnees.claims.iss, ISSUER);
    assert_eq!(donnees.claims.sub, SUB);
    assert_eq!(donnees.claims.event_type, "sync.succeeded");
    assert_eq!(donnees.claims.account.as_deref(), Some(COMPTE));
    assert_eq!(donnees.claims.count, Some(3));
    assert!(!donnees.claims.at.is_empty());
    assert!(donnees.claims.exp > donnees.claims.iat);
}

#[test]
fn les_topics_suivent_la_structure_par_type_d_evenement() {
    let cas = [
        (
            EvenementSynchro::sync_started(proprietaire(), COMPTE.to_string(), moment()),
            format!("budgy/{SUB}/sync/started"),
        ),
        (
            EvenementSynchro::sync_succeeded(proprietaire(), COMPTE.to_string(), 1, moment()),
            format!("budgy/{SUB}/sync/succeeded"),
        ),
        (
            EvenementSynchro::sync_failed(proprietaire(), COMPTE.to_string(), moment()),
            format!("budgy/{SUB}/sync/failed"),
        ),
        (
            EvenementSynchro::account_transactions(
                proprietaire(),
                COMPTE.to_string(),
                5,
                moment(),
            ),
            format!("budgy/{SUB}/account/transactions"),
        ),
        (
            EvenementSynchro::balance_updated(proprietaire(), COMPTE.to_string(), moment()),
            format!("budgy/{SUB}/account/balance"),
        ),
        (
            EvenementSynchro::consent_renewal_required(proprietaire(), moment()),
            format!("budgy/{SUB}/consent/renewal-required"),
        ),
        (
            EvenementSynchro::consent_expired(proprietaire(), moment()),
            format!("budgy/{SUB}/consent/expired"),
        ),
    ];

    for (evenement, attendu) in cas {
        assert_eq!(topic_pour(PREFIXE, &evenement), attendu);
    }
}

#[test]
fn le_sub_du_topic_est_un_identifiant_opaque() {
    let evenement = EvenementSynchro::sync_started(proprietaire(), COMPTE.to_string(), moment());
    let topic = topic_pour(PREFIXE, &evenement);
    let segment_sub = topic.split('/').nth(1).expect("segment sub présent");

    assert!(Uuid::parse_str(segment_sub).is_ok(), "le sub doit rester un UUID opaque");
}

#[test]
fn le_payload_ne_contient_aucune_donnee_bancaire() {
    let (signataire, cle_publique) = signataire_et_cle_publique();
    let evenement = EvenementSynchro::account_transactions(
        proprietaire(),
        COMPTE.to_string(),
        7,
        moment(),
    );

    let message = construire_payload(&evenement, ISSUER, &signataire).expect("payload signé");

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_required_spec_claims(&["exp"]);
    let cle = DecodingKey::from_rsa_pem(cle_publique.as_bytes()).expect("clé publique valide");
    let donnees =
        decode::<serde_json::Value>(&message, &cle, &validation).expect("payload décodable");

    let objet = donnees.claims.as_object().expect("payload est un objet json");
    let cles_autorisees = ["iss", "sub", "event_type", "account", "count", "at", "iat", "exp"];
    for cle in objet.keys() {
        assert!(
            cles_autorisees.contains(&cle.as_str()),
            "clé inattendue dans le payload : {cle}"
        );
    }

    let interdits = ["amount", "amount_cents", "balance", "iban", "label", "currency", "montant"];
    let brut = donnees.claims.to_string().to_lowercase();
    for terme in interdits {
        assert!(
            !brut.contains(terme),
            "le payload ne doit jamais contenir '{terme}' : {brut}"
        );
    }
}

#[test]
fn seuls_les_events_de_consentement_sont_retenus() {
    assert!(TypeEvenementSynchro::ConsentRenewalRequired.retenu());
    assert!(TypeEvenementSynchro::ConsentExpired.retenu());

    assert!(!TypeEvenementSynchro::SyncStarted.retenu());
    assert!(!TypeEvenementSynchro::SyncSucceeded.retenu());
    assert!(!TypeEvenementSynchro::SyncFailed.retenu());
    assert!(!TypeEvenementSynchro::AccountTransactions.retenu());
    assert!(!TypeEvenementSynchro::BalanceUpdated.retenu());
}
