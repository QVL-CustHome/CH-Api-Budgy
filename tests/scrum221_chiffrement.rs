mod common;

use ch_api_budgy::crypto::CryptoService;
use ch_api_budgy::repository::bank_credential::{self, BankCredentialError};
use common::DisposableDb;
use uuid::Uuid;

const OWNER: &str = "owner-scrum-221";
const PLAINTEXT: &str = "sk_live_acces_bancaire_tres_secret_42";

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

async fn raw_blob(pool: &ch_api_budgy::db::Db, id: Uuid) -> Vec<u8> {
    sqlx::query_scalar("SELECT access_token FROM budgy.bank_credential WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("blob brut lisible")
}

#[tokio::test]
async fn ca01_le_token_stocke_en_base_n_est_pas_en_clair() {
    let db = require_db!();
    let crypto = crypto();

    let id = bank_credential::insert(&db.pool, &crypto, OWNER, PLAINTEXT)
        .await
        .expect("insertion du token");

    let blob = raw_blob(&db.pool, id).await;

    let plaintext_bytes = PLAINTEXT.as_bytes();
    let contient_sous_chaine = blob
        .windows(plaintext_bytes.len())
        .any(|fenetre| fenetre == plaintext_bytes);
    assert!(
        !contient_sous_chaine,
        "le plaintext ne doit jamais apparaître dans le blob stocké"
    );

    let blob_utf8 = String::from_utf8_lossy(&blob);
    assert!(
        !blob_utf8.contains(PLAINTEXT),
        "le plaintext ne doit pas apparaître en UTF-8 dans le blob"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca02_round_trip_restitue_exactement_le_plaintext_d_origine() {
    let db = require_db!();
    let crypto = crypto();

    let id = bank_credential::insert(&db.pool, &crypto, OWNER, PLAINTEXT)
        .await
        .expect("insertion du token");

    let relu = bank_credential::fetch(&db.pool, &crypto, id)
        .await
        .expect("lecture du token");

    assert_eq!(relu, Some(PLAINTEXT.to_string()));

    db.destroy().await;
}

#[tokio::test]
async fn ca03_alteration_d_un_octet_du_blob_fait_echouer_le_dechiffrement() {
    let db = require_db!();
    let crypto = crypto();

    let id = bank_credential::insert(&db.pool, &crypto, OWNER, PLAINTEXT)
        .await
        .expect("insertion du token");

    let mut blob = raw_blob(&db.pool, id).await;
    let dernier = blob.len() - 1;
    blob[dernier] ^= 0x01;

    sqlx::query("UPDATE budgy.bank_credential SET access_token = $1 WHERE id = $2")
        .bind(&blob)
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("écriture du blob altéré");

    let resultat = bank_credential::fetch(&db.pool, &crypto, id).await;

    assert!(
        matches!(resultat, Err(BankCredentialError::Crypto(_))),
        "un blob altéré doit faire échouer le déchiffrement (AEAD), obtenu : {resultat:?}"
    );

    db.destroy().await;
}

#[tokio::test]
async fn ca03_alteration_du_nonce_fait_echouer_le_dechiffrement() {
    let db = require_db!();
    let crypto = crypto();

    let id = bank_credential::insert(&db.pool, &crypto, OWNER, PLAINTEXT)
        .await
        .expect("insertion du token");

    let mut blob = raw_blob(&db.pool, id).await;
    blob[0] ^= 0x01;

    sqlx::query("UPDATE budgy.bank_credential SET access_token = $1 WHERE id = $2")
        .bind(&blob)
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("écriture du nonce altéré");

    let resultat = bank_credential::fetch(&db.pool, &crypto, id).await;

    assert!(
        matches!(resultat, Err(BankCredentialError::Crypto(_))),
        "un nonce altéré doit faire échouer le déchiffrement (AEAD), obtenu : {resultat:?}"
    );

    db.destroy().await;
}
