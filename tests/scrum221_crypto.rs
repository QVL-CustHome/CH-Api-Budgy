use ch_api_budgy::config::{self, ConfigError};
use ch_api_budgy::crypto::{CryptoError, CryptoService, NONCE_BYTES};
use std::sync::Mutex;

const TEST_KEY: [u8; 32] = [42u8; 32];
const TEST_KEY_B64: &str = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
const AAD: &str = "budgy:test:owner-1:bank_credential:access_token";

static ENV_GUARD: Mutex<()> = Mutex::new(());

fn service() -> CryptoService {
    CryptoService::from_key(&TEST_KEY).expect("clé de test 32 octets valide")
}

#[test]
fn ca05_nonce_non_reutilise_deux_chiffrements_du_meme_plaintext_different() {
    let crypto = service();
    let plaintext = b"sk_live_token_bancaire_secret";

    let blob_a = crypto.encrypt(plaintext, AAD).expect("chiffrement 1");
    let blob_b = crypto.encrypt(plaintext, AAD).expect("chiffrement 2");

    assert_ne!(
        blob_a, blob_b,
        "deux chiffrements du même plaintext doivent produire des blobs différents"
    );
    assert_ne!(
        &blob_a[..NONCE_BYTES],
        &blob_b[..NONCE_BYTES],
        "le nonce doit être aléatoire à chaque chiffrement"
    );
}

#[test]
fn ca05_round_trip_au_niveau_service_restitue_le_plaintext() {
    let crypto = service();
    let plaintext = b"sk_live_token_bancaire_secret";

    let blob = crypto.encrypt(plaintext, AAD).expect("chiffrement");
    let restitue = crypto.decrypt(&blob, AAD).expect("déchiffrement");

    assert_eq!(restitue, plaintext);
}

#[test]
fn ca06_debug_des_secrets_ne_revele_pas_la_cle() {
    let settings = config::Settings {
        config: config::Config {
            server: config::ServerConfig {
                port: 8183,
                log_level: "INFO".to_string(),
            },
        },
        secrets: config::Secrets {
            database_url: "postgres://user:motdepasse@localhost/db".to_string(),
            encryption_key: TEST_KEY.to_vec(),
        },
    };

    let rendu = format!("{:?}", settings.secrets);

    let cle_hex: String = TEST_KEY.iter().map(|b| format!("{b:02x}")).collect();
    let cle_b64 = TEST_KEY_B64;

    assert!(
        !rendu.contains(&cle_hex),
        "le Debug ne doit pas exposer la clé en hexadécimal : {rendu}"
    );
    assert!(
        !rendu.contains(cle_b64),
        "le Debug ne doit pas exposer la clé en base64 : {rendu}"
    );
    assert!(
        !rendu.contains("42, 42, 42"),
        "le Debug ne doit pas exposer les octets de la clé : {rendu}"
    );
    assert!(
        !rendu.contains("motdepasse"),
        "le Debug ne doit pas exposer la database_url en clair : {rendu}"
    );
}

#[test]
fn ca04_cle_longueur_differente_de_32_octets_rejetee() {
    let trop_courte = CryptoService::from_key(&[0u8; 16]);
    let trop_longue = CryptoService::from_key(&[0u8; 48]);

    assert!(matches!(trop_courte, Err(CryptoError::InvalidKey)));
    assert!(matches!(trop_longue, Err(CryptoError::InvalidKey)));
}

#[derive(Debug, PartialEq)]
enum LoadOutcome {
    Ok { key_len: usize },
    MissingSecret,
    InvalidEncryptionKey,
    OtherError,
}

fn load_outcome(key: Option<&str>) -> LoadOutcome {
    with_env(key, |path| match config::load(path) {
        Ok(settings) => LoadOutcome::Ok {
            key_len: settings.secrets.encryption_key.len(),
        },
        Err(ConfigError::MissingSecret(_)) => LoadOutcome::MissingSecret,
        Err(ConfigError::InvalidEncryptionKey) => LoadOutcome::InvalidEncryptionKey,
        Err(_) => LoadOutcome::OtherError,
    })
}

#[test]
fn ca04_load_echoue_si_cle_absente() {
    let _guard = ENV_GUARD.lock().unwrap();
    assert_eq!(load_outcome(None), LoadOutcome::MissingSecret);
}

#[test]
fn ca04_load_echoue_si_base64_invalide() {
    let _guard = ENV_GUARD.lock().unwrap();
    assert_eq!(
        load_outcome(Some("ceci n'est pas du base64 valide !!!")),
        LoadOutcome::InvalidEncryptionKey
    );
}

#[test]
fn ca04_load_echoue_si_longueur_differente_de_32_octets() {
    let _guard = ENV_GUARD.lock().unwrap();
    let seize_octets_b64 = "AAAAAAAAAAAAAAAAAAAAAA==";
    assert_eq!(
        load_outcome(Some(seize_octets_b64)),
        LoadOutcome::InvalidEncryptionKey
    );
}

#[test]
fn ca04_load_accepte_une_cle_valide_de_32_octets() {
    let _guard = ENV_GUARD.lock().unwrap();
    assert_eq!(
        load_outcome(Some(TEST_KEY_B64)),
        LoadOutcome::Ok { key_len: 32 }
    );
}

fn with_env<R>(key: Option<&str>, run: impl FnOnce(&str) -> R) -> R {
    let dir = std::env::temp_dir().join(format!("budgy_cfg_{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let cfg_path = dir.join("config.toml");
    std::fs::write(&cfg_path, "[server]\nport = 8183\nlog_level = \"INFO\"\n").unwrap();

    let prev_db = std::env::var("DATABASE_URL").ok();
    let prev_key = std::env::var("BUDGY_ENCRYPTION_KEY").ok();

    unsafe {
        std::env::set_var("DATABASE_URL", "postgres://u:p@localhost/db");
        match key {
            Some(v) => std::env::set_var("BUDGY_ENCRYPTION_KEY", v),
            None => std::env::remove_var("BUDGY_ENCRYPTION_KEY"),
        }
    }

    let result = run(cfg_path.to_str().unwrap());

    unsafe {
        match prev_db {
            Some(v) => std::env::set_var("DATABASE_URL", v),
            None => std::env::remove_var("DATABASE_URL"),
        }
        match prev_key {
            Some(v) => std::env::set_var("BUDGY_ENCRYPTION_KEY", v),
            None => std::env::remove_var("BUDGY_ENCRYPTION_KEY"),
        }
    }
    std::fs::remove_dir_all(&dir).ok();

    result
}
