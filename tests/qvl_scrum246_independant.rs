mod support;

use ch_api_budgy::adapters::bank::enable_banking::ClientEnableBanking;
use ch_api_budgy::adapters::bank::enable_banking::jwt::SignataireJwt;
use ch_api_budgy::adapters::bank::reel::EnableBankingBankDataSource;
use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentId, ConsentStatus};
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, BankDataSourceError, DemandeConsentement, ReponseAutorisation,
};
use chrono::{NaiveDate, Utc};
use std::sync::Arc;
use support::{EchangeSimule, TransportSimule, paire_rsa_test};

const APP_ID_SECRET: &str = "app-id-confidentiel-7f3a";
const OWNER: &str = "owner-indep-246";
const IBAN_CLAIR: &str = "FR7630006000011234567890189";

fn signataire_secret() -> (SignataireJwt, String) {
    let paire = paire_rsa_test();
    let signataire = SignataireJwt::nouveau(APP_ID_SECRET, paire.privee_pem.as_bytes())
        .expect("clé privée RSA de test valide");
    (signataire, paire.privee_pem)
}

fn source(reponses: Vec<EchangeSimule>) -> EnableBankingBankDataSource<Arc<TransportSimule>> {
    let transport = Arc::new(TransportSimule::nouveau(reponses));
    let (signataire, _) = signataire_secret();
    let client = ClientEnableBanking::nouveau(
        transport,
        signataire,
        "https://budgy.custhome.app/banque/retour".to_string(),
    );
    EnableBankingBankDataSource::avec_client(client)
}

fn consent_actif(session_id: &str) -> Consent {
    let horodatage = Utc::now();
    Consent {
        id: ConsentId(uuid::Uuid::new_v4()),
        proprietaire: ProprietaireId(OWNER.to_string()),
        external_ref: session_id.to_string(),
        etablissement: None,
        status: ConsentStatus::Active,
        expires_at: Some(horodatage),
        created_at: horodatage,
        updated_at: horodatage,
    }
}

fn compte_actif() -> ch_api_budgy::domain::bank_account::BankAccount {
    use ch_api_budgy::domain::bank_account::{BankAccount, BankAccountId};
    let horodatage = Utc::now();
    BankAccount {
        id: BankAccountId(uuid::Uuid::new_v4()),
        proprietaire: ProprietaireId(OWNER.to_string()),
        consent: ConsentId(uuid::Uuid::new_v4()),
        external_account_id: "acc-eb-indep".to_string(),
        iban_masked: "************0189".to_string(),
        currency: "EUR".to_string(),
        next_sync_at: None,
        sync_count_today: 0,
        created_at: horodatage,
        updated_at: horodatage,
    }
}

fn demande() -> DemandeConsentement {
    DemandeConsentement {
        consent_id: ConsentId(uuid::Uuid::new_v4()),
        proprietaire: ProprietaireId(OWNER.to_string()),
        etablissement: "Banque Demo".to_string(),
        url_retour: "https://budgy.custhome.app/banque/retour".to_string(),
    }
}

#[test]
fn ac3_debug_config_ne_revele_ni_app_id_ni_cle_privee() {
    let paire = paire_rsa_test();
    let config = EnableBankingConfig {
        base_url: "https://api.enablebanking.com".to_string(),
        app_id: Some(APP_ID_SECRET.to_string()),
        private_key_pem: Some(paire.privee_pem.clone()),
        private_key_path: None,
        redirect_url: Some("https://budgy.custhome.app/banque/retour".to_string()),
    };

    let rendu = format!("{config:?}");

    assert!(!rendu.contains(APP_ID_SECRET));
    assert!(!rendu.contains("BEGIN RSA PRIVATE KEY"));
    assert!(!rendu.contains("PRIVATE KEY"));
    let fragment_cle = &paire.privee_pem[40..80];
    assert!(!rendu.contains(fragment_cle));
}

#[test]
fn ac3_debug_signataire_ne_revele_ni_app_id_ni_cle() {
    let (signataire, pem) = signataire_secret();

    let rendu = format!("{signataire:?}");

    assert!(!rendu.contains(APP_ID_SECRET));
    assert!(!rendu.contains("PRIVATE KEY"));
    let fragment_cle = &pem[40..80];
    assert!(!rendu.contains(fragment_cle));
}

#[tokio::test]
async fn ac3_adapter_sans_credentials_refuse_proprement_sur_toutes_les_operations() {
    let config = EnableBankingConfig::default();
    let adapter = EnableBankingBankDataSource::depuis_config(&config);

    let initie = adapter.initier_consentement(demande()).await;
    assert!(matches!(initie, Err(BankDataSourceError::SourceNonConfiguree)));

    let complete = adapter
        .completer_consentement(
            &ProprietaireId(OWNER.to_string()),
            ReponseAutorisation {
                reference_autorisation: "ref".to_string(),
                code_autorisation: "code".to_string(),
            },
        )
        .await;
    assert!(matches!(complete, Err(BankDataSourceError::SourceNonConfiguree)));

    let comptes = adapter.lister_comptes(&consent_actif("s")).await;
    assert!(matches!(comptes, Err(BankDataSourceError::SourceNonConfiguree)));

    let solde = adapter.solde(&consent_actif("s"), &compte_actif()).await;
    assert!(matches!(solde, Err(BankDataSourceError::SourceNonConfiguree)));

    let transactions = adapter
        .lister_transactions(
            &consent_actif("s"),
            &compte_actif(),
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        )
        .await;
    assert!(matches!(transactions, Err(BankDataSourceError::SourceNonConfiguree)));

    let revoque = adapter.revoquer_consentement(&consent_actif("s")).await;
    assert!(matches!(revoque, Err(BankDataSourceError::SourceNonConfiguree)));
}

#[tokio::test]
async fn ac3_credentials_partiels_degradent_sans_paniquer() {
    let config = EnableBankingConfig {
        app_id: Some(APP_ID_SECRET.to_string()),
        redirect_url: None,
        private_key_pem: None,
        private_key_path: None,
        base_url: "https://api.enablebanking.com".to_string(),
    };
    let adapter = EnableBankingBankDataSource::depuis_config(&config);

    let initie = adapter.initier_consentement(demande()).await;

    assert!(matches!(initie, Err(BankDataSourceError::SourceNonConfiguree)));
}

#[tokio::test]
async fn ac1_erreur_401_est_mappee_en_consentement_invalide() {
    let adapter = source(vec![EchangeSimule::statut(401, "unauthorized")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(resultat, Err(BankDataSourceError::ConsentementInvalide)));
}

#[tokio::test]
async fn ac1_erreur_403_est_mappee_en_consentement_invalide() {
    let adapter = source(vec![EchangeSimule::statut(403, "forbidden")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(resultat, Err(BankDataSourceError::ConsentementInvalide)));
}

#[tokio::test]
async fn ac1_erreur_503_est_mappee_en_etablissement_indisponible() {
    let adapter = source(vec![EchangeSimule::statut(503, "unavailable")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(resultat, Err(BankDataSourceError::EtablissementIndisponible)));
}

#[tokio::test]
async fn ac1_erreur_500_est_mappee_en_etablissement_indisponible() {
    let adapter = source(vec![EchangeSimule::statut(500, "boom")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(
        resultat,
        Err(BankDataSourceError::EtablissementIndisponible)
    ));
}

#[tokio::test]
async fn ac1_erreur_429_est_mappee_en_etablissement_indisponible() {
    let adapter = source(vec![EchangeSimule::statut(429, "too many")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(
        resultat,
        Err(BankDataSourceError::EtablissementIndisponible)
    ));
}

#[tokio::test]
async fn ac1_erreur_404_est_mappee_en_ressource_introuvable() {
    let adapter = source(vec![EchangeSimule::statut(404, "not found")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(
        resultat,
        Err(BankDataSourceError::RessourceIntrouvable)
    ));
}

#[tokio::test]
async fn ac1_corps_illisible_est_mappe_en_reponse_invalide() {
    let adapter = source(vec![EchangeSimule::ok("ceci n'est pas du json")]);

    let resultat = adapter.lister_comptes(&consent_actif("sess")).await;

    assert!(matches!(resultat, Err(BankDataSourceError::ReponseInvalide(_))));
}

#[tokio::test]
async fn ac1_iban_n_apparait_jamais_en_clair_dans_le_compte_mappe() {
    let reponse = format!(
        r#"{{"session_id":"sess","accounts":[{{"uid":"acc-1","currency":"EUR","account_id":{{"iban":"{IBAN_CLAIR}"}}}}]}}"#
    );
    let adapter = source(vec![EchangeSimule::ok(&reponse)]);

    let comptes = adapter
        .lister_comptes(&consent_actif("sess"))
        .await
        .expect("liste des comptes");

    assert_eq!(comptes.len(), 1);
    assert_ne!(comptes[0].iban_masked, IBAN_CLAIR);
    assert!(!comptes[0].iban_masked.contains("FR76"));
    assert!(!comptes[0].iban_masked.contains("1234567890"));
    assert!(comptes[0].iban_masked.ends_with("0189"));
}

#[tokio::test]
async fn ac1_session_sans_compte_renvoie_une_liste_vide() {
    let reponse = r#"{"session_id":"sess","accounts":[]}"#;
    let adapter = source(vec![EchangeSimule::ok(reponse)]);

    let comptes = adapter
        .lister_comptes(&consent_actif("sess"))
        .await
        .expect("liste des comptes vide");

    assert!(comptes.is_empty());
}

#[tokio::test]
async fn ac1_consent_id_reste_stable_de_l_initiation_a_la_completion() {
    let reponse_init = r#"{"url":"https://banque.example/authorize?id=abc","authorization_id":"auth-cycle"}"#;
    let reponse_complete = r#"{"session_id":"sess-cycle","status":"AUTHORIZED","accounts":[],"access":{"valid_until":"2026-09-01T00:00:00Z"}}"#;
    let adapter = source(vec![
        EchangeSimule::ok(reponse_init),
        EchangeSimule::ok(reponse_complete),
    ]);

    let initie = adapter
        .initier_consentement(demande())
        .await
        .expect("initiation du consentement");
    let id_initial = initie.consent.id;

    let complete = adapter
        .completer_consentement(
            &ProprietaireId(OWNER.to_string()),
            ReponseAutorisation {
                reference_autorisation: id_initial.0.to_string(),
                code_autorisation: "code-cycle".to_string(),
            },
        )
        .await
        .expect("complétion du consentement");

    assert_eq!(complete.id, id_initial);
    assert_eq!(complete.status, ConsentStatus::Active);
    assert_eq!(complete.external_ref, "sess-cycle");
}

#[tokio::test]
async fn ac1_solde_a_trois_decimales_est_arrondi_au_centime_superieur() {
    let reponse = r#"{"balances":[{"balance_type":"CLBD","balance_amount":{"amount":"3127.116","currency":"EUR"},"reference_date":"2026-06-20"}]}"#;
    let adapter = source(vec![EchangeSimule::ok(reponse)]);

    let soldes = adapter
        .solde(&consent_actif("sess"), &compte_actif())
        .await
        .expect("liste des soldes");

    assert_eq!(soldes[0].amount_cents, 312_712);
}

#[tokio::test]
async fn ac1_solde_a_trois_decimales_est_arrondi_au_centime_inferieur() {
    let reponse = r#"{"balances":[{"balance_type":"CLBD","balance_amount":{"amount":"3127.113","currency":"EUR"},"reference_date":"2026-06-20"}]}"#;
    let adapter = source(vec![EchangeSimule::ok(reponse)]);

    let soldes = adapter
        .solde(&consent_actif("sess"), &compte_actif())
        .await
        .expect("liste des soldes");

    assert_eq!(soldes[0].amount_cents, 312_711);
}

#[tokio::test]
async fn ac1_transaction_a_trois_decimales_est_arrondie_au_centime_pas_tronquee() {
    let reponse = r#"{"transactions":[{"entry_reference":"tx-arrondi","transaction_amount":{"amount":"45.906","currency":"EUR"},"credit_debit_indicator":"CRDT","status":"BOOK","booking_date":"2026-06-18","value_date":"2026-06-18","remittance_information":["ARRONDI"]}],"continuation_key":null}"#;
    let adapter = source(vec![EchangeSimule::ok(reponse)]);

    let transactions = adapter
        .lister_transactions(
            &consent_actif("sess"),
            &compte_actif(),
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        )
        .await
        .expect("liste des transactions");

    assert_eq!(transactions[0].amount_cents, 4_591);
}

#[tokio::test]
async fn ac4_construire_source_enablebanking_renvoie_un_adapter_reel_qui_degrade_sans_creds() {
    let config = EnableBankingConfig::default();
    let source = construire_source(SourceBancaire::EnableBanking, &config);

    let resultat = source.initier_consentement(demande()).await;

    assert!(matches!(resultat, Err(BankDataSourceError::SourceNonConfiguree)));
}

#[tokio::test]
async fn ac4_construire_source_mock_reste_fonctionnel() {
    let config = EnableBankingConfig::default();
    let source = construire_source(SourceBancaire::Mock, &config);

    let initie = source
        .initier_consentement(demande())
        .await
        .expect("le mock initie un consentement");

    assert!(!initie.url_autorisation.is_empty());
    assert_eq!(initie.consent.status, ConsentStatus::Pending);
}
