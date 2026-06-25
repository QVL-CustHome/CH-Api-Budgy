use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::ports::bank_data_source::DemandeConsentement;
use ch_api_budgy::domain::transaction_bancaire::{TransactionStatus, dedup_key};
use chrono::NaiveDate;
use std::collections::HashSet;

const OWNER: &str = "owner-scrum-245";
const ETABLISSEMENT: &str = "banque-demo";

fn demande() -> DemandeConsentement {
    DemandeConsentement {
        proprietaire: ProprietaireId(OWNER.to_string()),
        etablissement: ETABLISSEMENT.to_string(),
        url_retour: "https://budgy.custhome.app/retour".to_string(),
    }
}

fn depuis() -> NaiveDate {
    NaiveDate::from_ymd_opt(2023, 1, 1).expect("date de référence valide")
}

#[tokio::test]
async fn la_bascule_par_configuration_fournit_le_mock() {
    let source = construire_source(SourceBancaire::Mock);
    let consent = source
        .initier_consentement(demande())
        .await
        .expect("le mock initie un consentement");
    let comptes = source
        .lister_comptes(&consent)
        .await
        .expect("le mock liste des comptes");

    assert!(!comptes.is_empty());
}

#[tokio::test]
async fn le_mock_est_deterministe() {
    let premiere = construire_source(SourceBancaire::Mock);
    let seconde = construire_source(SourceBancaire::Mock);

    let consent_a = premiere.initier_consentement(demande()).await.unwrap();
    let consent_b = seconde.initier_consentement(demande()).await.unwrap();
    assert_eq!(consent_a.id, consent_b.id);
    assert_eq!(consent_a.external_ref, consent_b.external_ref);
    assert_eq!(consent_a.expires_at, consent_b.expires_at);

    let comptes_a = premiere.lister_comptes(&consent_a).await.unwrap();
    let comptes_b = seconde.lister_comptes(&consent_b).await.unwrap();
    let ids_a: Vec<_> = comptes_a.iter().map(|c| c.id.clone()).collect();
    let ids_b: Vec<_> = comptes_b.iter().map(|c| c.id.clone()).collect();
    assert_eq!(ids_a, ids_b);

    let soldes_a = premiere.solde(&consent_a, &comptes_a[0]).await.unwrap();
    let soldes_b = seconde.solde(&consent_b, &comptes_b[0]).await.unwrap();
    let montants_a: Vec<_> = soldes_a.iter().map(|s| s.amount_cents).collect();
    let montants_b: Vec<_> = soldes_b.iter().map(|s| s.amount_cents).collect();
    assert_eq!(montants_a, montants_b);
}

#[tokio::test]
async fn les_transactions_du_mock_se_dedupliquent_par_cle() {
    let source = construire_source(SourceBancaire::Mock);
    let consent = source.initier_consentement(demande()).await.unwrap();
    let comptes = source.lister_comptes(&consent).await.unwrap();
    let compte = &comptes[0];

    let premier_lot = source
        .lister_transactions(&consent, compte, depuis())
        .await
        .unwrap();
    let rejeu = source
        .lister_transactions(&consent, compte, depuis())
        .await
        .unwrap();

    let cles_premier: HashSet<String> = premier_lot
        .iter()
        .map(|t| dedup_key(&t.bank_account, &t.external_transaction_id))
        .collect();
    let cles_rejeu: HashSet<String> = rejeu
        .iter()
        .map(|t| dedup_key(&t.bank_account, &t.external_transaction_id))
        .collect();

    assert_eq!(cles_premier, cles_rejeu);
    assert_eq!(cles_premier.len(), premier_lot.len());
}

#[tokio::test]
async fn une_transaction_passe_de_pending_a_booked_au_rejeu() {
    let source = construire_source(SourceBancaire::Mock);
    let consent = source.initier_consentement(demande()).await.unwrap();
    let comptes = source.lister_comptes(&consent).await.unwrap();
    let compte = &comptes[0];

    let premier_lot = source
        .lister_transactions(&consent, compte, depuis())
        .await
        .unwrap();
    let rejeu = source
        .lister_transactions(&consent, compte, depuis())
        .await
        .unwrap();

    let pending = premier_lot
        .iter()
        .find(|t| t.status == TransactionStatus::Pending)
        .expect("le premier lot contient une transaction pending");

    let rejouee = rejeu
        .iter()
        .find(|t| t.external_transaction_id == pending.external_transaction_id)
        .expect("la transaction pending est resservie au rejeu");

    assert_eq!(rejouee.status, TransactionStatus::Booked);
    assert!(rejouee.booking_date.is_some());
}
