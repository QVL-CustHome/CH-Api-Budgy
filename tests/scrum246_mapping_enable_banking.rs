mod support;

use ch_api_budgy::adapters::bank::enable_banking::ClientEnableBanking;
use ch_api_budgy::adapters::bank::enable_banking::jwt::SignataireJwt;
use ch_api_budgy::adapters::bank::enable_banking::transport::MethodeHttp;
use ch_api_budgy::adapters::bank::reel::EnableBankingBankDataSource;
use ch_api_budgy::domain::balance::BalanceType;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentId, ConsentStatus};
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, DemandeConsentement, ReponseAutorisation,
};
use ch_api_budgy::domain::transaction_bancaire::{TransactionStatus, dedup_key};
use chrono::{NaiveDate, Utc};
use std::collections::HashSet;
use std::sync::Arc;
use support::{EchangeSimule, TransportSimule, paire_rsa_test};

const APP_ID: &str = "app-test-246";
const OWNER: &str = "owner-246";

fn signataire() -> SignataireJwt {
    let paire = paire_rsa_test();
    SignataireJwt::nouveau(APP_ID, paire.privee_pem.as_bytes()).expect("clé privée de test valide")
}

fn source(reponses: Vec<EchangeSimule>) -> (EnableBankingBankDataSource<Arc<TransportSimule>>, Arc<TransportSimule>) {
    let transport = Arc::new(TransportSimule::nouveau(reponses));
    let client = ClientEnableBanking::nouveau(
        transport.clone(),
        signataire(),
        "https://budgy.custhome.app/banque/retour".to_string(),
    );
    (EnableBankingBankDataSource::avec_client(client), transport)
}

fn consent_session(session_id: &str) -> Consent {
    let horodatage = Utc::now();
    Consent {
        id: ConsentId(uuid::Uuid::new_v4()),
        proprietaire: ProprietaireId(OWNER.to_string()),
        external_ref: session_id.to_string(),
        status: ConsentStatus::Active,
        expires_at: Some(horodatage),
        created_at: horodatage,
        updated_at: horodatage,
    }
}

fn compte_session() -> ch_api_budgy::domain::bank_account::BankAccount {
    use ch_api_budgy::domain::bank_account::{BankAccount, BankAccountId};
    let horodatage = Utc::now();
    BankAccount {
        id: BankAccountId(uuid::Uuid::new_v4()),
        proprietaire: ProprietaireId(OWNER.to_string()),
        consent: ConsentId(uuid::Uuid::new_v4()),
        external_account_id: "acc-eb-001".to_string(),
        iban_masked: "************0189".to_string(),
        currency: "EUR".to_string(),
        next_sync_at: None,
        sync_count_today: 0,
        created_at: horodatage,
        updated_at: horodatage,
    }
}

#[tokio::test]
async fn initier_consentement_renvoie_url_de_redirection_et_consent_pending() {
    let reponse = r#"{"url":"https://banque.example/authorize?id=abc","authorization_id":"auth-123"}"#;
    let (source, transport) = source(vec![EchangeSimule::ok(reponse)]);

    let consent_id = ConsentId(uuid::Uuid::new_v4());
    let demande = DemandeConsentement {
        consent_id: consent_id.clone(),
        proprietaire: ProprietaireId(OWNER.to_string()),
        etablissement: "Banque Demo".to_string(),
        url_retour: "https://budgy.custhome.app/banque/retour".to_string(),
    };

    let initie = source
        .initier_consentement(demande)
        .await
        .expect("initiation du consentement");

    assert_eq!(initie.consent.id, consent_id);
    assert!(initie.url_autorisation.starts_with("https://banque.example/authorize?id=abc"));
    assert!(initie
        .url_autorisation
        .contains(&format!("state={}", consent_id.0)));
    assert_eq!(initie.consent.status, ConsentStatus::Pending);
    assert_eq!(initie.consent.external_ref, "auth-123");

    let requetes = transport.requetes();
    assert_eq!(requetes.len(), 1);
    assert_eq!(requetes[0].methode, MethodeHttp::Post);
    assert_eq!(requetes[0].chemin, "/auth");
    assert!(!requetes[0].jeton.is_empty());
    let corps = requetes[0].corps_json.as_deref().unwrap();
    assert!(corps.contains(&consent_id.0.to_string()));
}

#[tokio::test]
async fn completer_consentement_echange_le_code_en_session_active() {
    let reponse = r#"{"session_id":"sess-789","status":"AUTHORIZED","accounts":[],"access":{"valid_until":"2026-09-01T00:00:00Z"}}"#;
    let (source, transport) = source(vec![EchangeSimule::ok(reponse)]);

    let consent_id_stable = uuid::Uuid::new_v4();
    let consent = source
        .completer_consentement(
            &ProprietaireId(OWNER.to_string()),
            ReponseAutorisation {
                reference_autorisation: consent_id_stable.to_string(),
                code_autorisation: "code-abc".to_string(),
            },
        )
        .await
        .expect("complétion du consentement");

    assert_eq!(consent.status, ConsentStatus::Active);
    assert_eq!(consent.id, ConsentId(consent_id_stable));
    assert_eq!(consent.external_ref, "sess-789");

    let requetes = transport.requetes();
    assert_eq!(requetes[0].methode, MethodeHttp::Post);
    assert_eq!(requetes[0].chemin, "/sessions");
    assert!(requetes[0].corps_json.as_deref().unwrap().contains("code-abc"));
}

#[tokio::test]
async fn lister_comptes_mappe_uid_devise_et_iban_masque() {
    let reponse = r#"{"session_id":"sess-789","accounts":[{"uid":"acc-eb-001","currency":"EUR","account_id":{"iban":"FR7630006000011234567890189"}}]}"#;
    let (source, transport) = source(vec![EchangeSimule::ok(reponse)]);

    let comptes = source
        .lister_comptes(&consent_session("sess-789"))
        .await
        .expect("liste des comptes");

    assert_eq!(comptes.len(), 1);
    assert_eq!(comptes[0].external_account_id, "acc-eb-001");
    assert_eq!(comptes[0].currency, "EUR");
    assert!(comptes[0].iban_masked.ends_with("0189"));
    assert!(comptes[0].iban_masked.starts_with('*'));

    assert_eq!(transport.requetes()[0].chemin, "/sessions/sess-789");
}

#[tokio::test]
async fn solde_mappe_booked_et_available_en_centimes() {
    let reponse = r#"{"balances":[{"balance_type":"CLBD","balance_amount":{"amount":"3127.11","currency":"EUR"},"reference_date":"2026-06-20"},{"balance_type":"XPCD","balance_amount":{"amount":"3081.21","currency":"EUR"}}]}"#;
    let (source, transport) = source(vec![EchangeSimule::ok(reponse)]);

    let soldes = source
        .solde(&consent_session("sess-789"), &compte_session())
        .await
        .expect("liste des soldes");

    let booked = soldes
        .iter()
        .find(|b| b.balance_type == BalanceType::Booked)
        .expect("solde booked présent");
    assert_eq!(booked.amount_cents, 312_711);

    let expected = soldes
        .iter()
        .find(|b| b.balance_type == BalanceType::Expected)
        .expect("solde expected présent");
    assert_eq!(expected.amount_cents, 308_121);

    assert_eq!(transport.requetes()[0].chemin, "/accounts/acc-eb-001/balances");
}

#[tokio::test]
async fn lister_transactions_mappe_pending_booked_signe_et_dedup_key() {
    let reponse = r#"{"transactions":[
        {"entry_reference":"tx-salaire","transaction_amount":{"amount":"2450.00","currency":"EUR"},"credit_debit_indicator":"CRDT","status":"BOOK","booking_date":"2026-06-18","value_date":"2026-06-18","remittance_information":["VIREMENT SALAIRE"]},
        {"entry_reference":"tx-achat","transaction_amount":{"amount":"45.90","currency":"EUR"},"credit_debit_indicator":"DBIT","status":"PDNG","remittance_information":["CARTE ACHAT COMMERCE"]}
    ],"continuation_key":null}"#;
    let (source, transport) = source(vec![EchangeSimule::ok(reponse)]);
    let compte = compte_session();

    let transactions = source
        .lister_transactions(
            &consent_session("sess-789"),
            &compte,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        )
        .await
        .expect("liste des transactions");

    assert_eq!(transactions.len(), 2);

    let salaire = transactions
        .iter()
        .find(|t| t.external_transaction_id == "tx-salaire")
        .expect("transaction salaire");
    assert_eq!(salaire.status, TransactionStatus::Booked);
    assert_eq!(salaire.amount_cents, 245_000);
    assert_eq!(salaire.label, "VIREMENT SALAIRE");

    let achat = transactions
        .iter()
        .find(|t| t.external_transaction_id == "tx-achat")
        .expect("transaction achat");
    assert_eq!(achat.status, TransactionStatus::Pending);
    assert_eq!(achat.amount_cents, -4_590);
    assert!(achat.booking_date.is_none());

    let cles: HashSet<String> = transactions
        .iter()
        .map(|t| dedup_key(&t.bank_account, &t.external_transaction_id))
        .collect();
    assert_eq!(cles.len(), transactions.len());

    let chemin = &transport.requetes()[0].chemin;
    assert!(chemin.starts_with("/accounts/acc-eb-001/transactions"));
    assert!(chemin.contains("date_from=2026-01-01"));
}

#[tokio::test]
async fn lister_transactions_suit_la_pagination_par_continuation_key() {
    let page1 = r#"{"transactions":[{"transaction_id":"tx-1","transaction_amount":{"amount":"10.00","currency":"EUR"},"status":"BOOK","remittance_information":["A"]}],"continuation_key":"page-2"}"#;
    let page2 = r#"{"transactions":[{"transaction_id":"tx-2","transaction_amount":{"amount":"20.00","currency":"EUR"},"status":"BOOK","remittance_information":["B"]}],"continuation_key":null}"#;
    let (source, transport) = source(vec![EchangeSimule::ok(page1), EchangeSimule::ok(page2)]);

    let transactions = source
        .lister_transactions(
            &consent_session("sess-789"),
            &compte_session(),
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        )
        .await
        .expect("liste paginée");

    assert_eq!(transactions.len(), 2);
    let requetes = transport.requetes();
    assert_eq!(requetes.len(), 2);
    assert!(requetes[1].chemin.contains("continuation_key=page-2"));
}

#[tokio::test]
async fn revoquer_consentement_termine_la_session_et_marque_revoked() {
    let (source, transport) = source(vec![EchangeSimule::statut(204, "")]);

    let consent = source
        .revoquer_consentement(&consent_session("sess-789"))
        .await
        .expect("révocation du consentement");

    assert_eq!(consent.status, ConsentStatus::Revoked);

    let requetes = transport.requetes();
    assert_eq!(requetes[0].methode, MethodeHttp::Delete);
    assert_eq!(requetes[0].chemin, "/sessions/sess-789");
}
