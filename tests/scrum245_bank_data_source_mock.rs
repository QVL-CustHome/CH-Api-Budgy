use ch_api_budgy::adapters::bank::mock::MockBankDataSource;
use ch_api_budgy::domain::bank_account::BankAccountId;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{Consent, ConsentId};
use ch_api_budgy::domain::horloge::Horloge;
use ch_api_budgy::domain::ports::bank_data_source::{
    BankDataSource, DemandeConsentement, ReponseAutorisation,
};
use ch_api_budgy::domain::transaction_bancaire::TransactionStatus;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use std::collections::HashSet;
use std::sync::Arc;

const OWNER: &str = "owner-scrum-245";
const ETABLISSEMENT: &str = "banque-demo";

fn cle_transaction(bank_account: &BankAccountId, external_transaction_id: &str) -> String {
    format!("{}:{external_transaction_id}", bank_account.0)
}

struct HorlogeFixe(DateTime<Utc>);

impl Horloge for HorlogeFixe {
    fn maintenant(&self) -> DateTime<Utc> {
        self.0
    }
}

fn horloge_fixe() -> Arc<dyn Horloge> {
    Arc::new(HorlogeFixe(
        Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap(),
    ))
}

fn mock() -> Arc<dyn BankDataSource> {
    Arc::new(MockBankDataSource::new())
}

fn mock_avec_horloge(horloge: Arc<dyn Horloge>) -> Arc<dyn BankDataSource> {
    Arc::new(MockBankDataSource::avec_horloge(horloge))
}

async fn consentement_actif(source: &Arc<dyn BankDataSource>) -> Consent {
    let initie = source
        .initier_consentement(demande())
        .await
        .expect("le mock initie un consentement");
    source
        .completer_consentement(
            &ProprietaireId(OWNER.to_string()),
            ReponseAutorisation {
                reference_autorisation: initie.consent.id.0.to_string(),
                code_autorisation: "code-mock".to_string(),
            },
        )
        .await
        .expect("le mock complète le consentement")
}

const CONSENT_ID_DETERMINISTE: uuid::Uuid =
    uuid::Uuid::from_u128(0x5c2f_245d_4a11_4b22_9c33_d044_e055_f066);

fn demande() -> DemandeConsentement {
    DemandeConsentement {
        consent_id: ConsentId(CONSENT_ID_DETERMINISTE),
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
    let source = mock();
    let consent = consentement_actif(&source).await;
    let comptes = source
        .lister_comptes(&consent)
        .await
        .expect("le mock liste des comptes");

    assert!(!comptes.is_empty());
}

#[tokio::test]
async fn le_mock_est_deterministe() {
    let premiere = mock_avec_horloge(horloge_fixe());
    let seconde = mock_avec_horloge(horloge_fixe());

    let consent_a = consentement_actif(&premiere).await;
    let consent_b = consentement_actif(&seconde).await;
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
    let source = mock();
    let consent = consentement_actif(&source).await;
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
        .map(|t| cle_transaction(&t.bank_account, &t.external_transaction_id))
        .collect();
    let cles_rejeu: HashSet<String> = rejeu
        .iter()
        .map(|t| cle_transaction(&t.bank_account, &t.external_transaction_id))
        .collect();

    assert_eq!(cles_premier, cles_rejeu);
    assert_eq!(cles_premier.len(), premier_lot.len());
}

#[tokio::test]
async fn une_transaction_passe_de_pending_a_booked_au_rejeu() {
    let source = mock();
    let consent = consentement_actif(&source).await;
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
