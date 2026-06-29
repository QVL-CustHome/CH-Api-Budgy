mod common;

use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::db::Db;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::{ConsentId, ConsentStatus, NouveauConsent};
use ch_api_budgy::repository::chiffrement::ChiffrementError;
use ch_api_budgy::repository::consents::SqlxConsentsRepository;
use chrono::{TimeZone, Utc};
use common::DisposableDb;
use uuid::Uuid;

const OWNER: &str = "owner-scrum-221";
const EXTERNAL_REF_CLAIR: &str = "consent-ref-acces-bancaire-tres-secret-42";

macro_rules! require_db {
    () => {
        match DisposableDb::create().await {
            Some(db) => {
                db.migrate().await;
                db
            }
            None => {
                eprintln!(
                    "SCRUM-221 ignoré : variable {} absente (Postgres jetable requis)",
                    common::ENV_ADMIN_URL
                );
                return;
            }
        }
    };
}

fn crypto() -> CryptoService {
    CryptoService::from_key(&[42u8; 32]).expect("clé de test 32 octets valide")
}

async fn inserer_consent(db: &DisposableDb, crypto: &CryptoService) -> Uuid {
    let repo = SqlxConsentsRepository::new(db.pool.clone());
    let id = repo
        .insert(
            crypto,
            NouveauConsent {
                proprietaire: ProprietaireId(OWNER.to_string()),
                external_ref: EXTERNAL_REF_CLAIR.to_string(),
                status: ConsentStatus::Active,
                expires_at: Some(Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()),
            },
        )
        .await
        .expect("insertion du consent");
    id.0
}

async fn raw_blob(pool: &Db, id: Uuid) -> Vec<u8> {
    sqlx::query_scalar("SELECT external_ref FROM budgy.consent WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("blob brut lisible")
}

#[tokio::test]
async fn ca03_alteration_d_un_octet_du_blob_fait_echouer_le_dechiffrement() {
    let db = require_db!();
    let crypto = crypto();
    let id = inserer_consent(&db, &crypto).await;

    let mut blob = raw_blob(&db.pool, id).await;
    let dernier = blob.len() - 1;
    blob[dernier] ^= 0x01;

    sqlx::query("UPDATE budgy.consent SET external_ref = $1 WHERE id = $2")
        .bind(&blob)
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("écriture du blob altéré");

    let repo = SqlxConsentsRepository::new(db.pool.clone());
    let resultat = repo.fetch(&crypto, &ConsentId(id)).await;

    assert!(
        matches!(resultat, Err(ChiffrementError::Crypto(_))),
        "un blob altéré doit faire échouer le déchiffrement (AEAD), obtenu : {resultat:?}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca03_alteration_du_nonce_fait_echouer_le_dechiffrement() {
    let db = require_db!();
    let crypto = crypto();
    let id = inserer_consent(&db, &crypto).await;

    let mut blob = raw_blob(&db.pool, id).await;
    blob[0] ^= 0x01;

    sqlx::query("UPDATE budgy.consent SET external_ref = $1 WHERE id = $2")
        .bind(&blob)
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("écriture du nonce altéré");

    let repo = SqlxConsentsRepository::new(db.pool.clone());
    let resultat = repo.fetch(&crypto, &ConsentId(id)).await;

    assert!(
        matches!(resultat, Err(ChiffrementError::Crypto(_))),
        "un nonce altéré doit faire échouer le déchiffrement (AEAD), obtenu : {resultat:?}"
    );

    db.destroy().await;
}
