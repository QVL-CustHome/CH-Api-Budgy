use ch_api_budgy::adapters::bank::selection::{SourceBancaire, construire_source};
use ch_api_budgy::config;
use ch_api_budgy::config::EnableBankingConfig;
use ch_api_budgy::domain::compte::ProprietaireId;
use ch_api_budgy::domain::consent::ConsentId;
use ch_api_budgy::domain::ports::bank_data_source::{BankDataSourceError, DemandeConsentement};
use std::sync::Mutex;

const TEST_KEY_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct ScenarioConfig {
    dir: std::path::PathBuf,
    cfg_path: std::path::PathBuf,
    previous: Vec<(&'static str, Option<String>)>,
}

impl ScenarioConfig {
    fn nouveau(section_bank: &str, bank_source_env: Option<&str>) -> Self {
        let dir =
            std::env::temp_dir().join(format!("budgy_qa_245_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).expect("création du dossier de config jetable");
        let cfg_path = dir.join("config.toml");
        let contenu = format!("[server]\nport = 8183\nlog_level = \"INFO\"\n{section_bank}");
        std::fs::write(&cfg_path, contenu).expect("écriture du fichier de config jetable");

        let noms = [
            "DATABASE_URL",
            "BUDGY_ENCRYPTION_KEY",
            "JWT_SECRET",
            "BANK_SOURCE",
        ];
        let previous = noms
            .iter()
            .map(|nom| (*nom, std::env::var(nom).ok()))
            .collect();

        unsafe {
            std::env::set_var("DATABASE_URL", "postgres://u:p@localhost/db");
            std::env::set_var("BUDGY_ENCRYPTION_KEY", TEST_KEY_B64);
            std::env::set_var(
                "JWT_SECRET",
                "this_is_a_test_jwt_secret_at_least_32_bytes_long",
            );
            match bank_source_env {
                Some(valeur) => std::env::set_var("BANK_SOURCE", valeur),
                None => std::env::remove_var("BANK_SOURCE"),
            }
        }

        Self {
            dir,
            cfg_path,
            previous,
        }
    }

    fn source_chargee(&self) -> SourceBancaire {
        config::load(self.cfg_path.to_str().expect("chemin de config valide"))
            .expect("chargement de la configuration")
            .config
            .bank
            .source
    }
}

impl Drop for ScenarioConfig {
    fn drop(&mut self) {
        unsafe {
            for (nom, valeur) in &self.previous {
                match valeur {
                    Some(v) => std::env::set_var(nom, v),
                    None => std::env::remove_var(nom),
                }
            }
        }
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

#[test]
fn la_section_bank_mock_de_la_config_selectionne_le_mock() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scenario = ScenarioConfig::nouveau("[bank]\nsource = \"mock\"\n", None);

    assert_eq!(scenario.source_chargee(), SourceBancaire::Mock);
}

#[test]
fn la_section_bank_enablebanking_de_la_config_selectionne_le_reel() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scenario = ScenarioConfig::nouveau("[bank]\nsource = \"enablebanking\"\n", None);

    assert_eq!(scenario.source_chargee(), SourceBancaire::EnableBanking);
}

#[test]
fn sans_section_bank_la_source_par_defaut_est_le_mock() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scenario = ScenarioConfig::nouveau("", None);

    assert_eq!(scenario.source_chargee(), SourceBancaire::Mock);
}

#[test]
fn la_variable_env_bank_source_remplace_la_section_de_config() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scenario = ScenarioConfig::nouveau("[bank]\nsource = \"mock\"\n", Some("enablebanking"));

    assert_eq!(scenario.source_chargee(), SourceBancaire::EnableBanking);
}

#[test]
fn la_variable_env_bank_source_force_le_mock_malgre_la_config_reelle() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scenario = ScenarioConfig::nouveau("[bank]\nsource = \"enablebanking\"\n", Some("mock"));

    assert_eq!(scenario.source_chargee(), SourceBancaire::Mock);
}

#[test]
fn une_valeur_bank_source_invalide_est_ignoree_et_conserve_la_config() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scenario = ScenarioConfig::nouveau("[bank]\nsource = \"enablebanking\"\n", Some("revolut"));

    assert_eq!(scenario.source_chargee(), SourceBancaire::EnableBanking);
}

#[tokio::test]
async fn la_source_reelle_sans_credentials_refuse_proprement() {
    let source = construire_source(
        SourceBancaire::EnableBanking,
        &EnableBankingConfig::default(),
    );
    let demande = DemandeConsentement {
        consent_id: ConsentId(uuid::Uuid::new_v4()),
        proprietaire: ProprietaireId("owner-qa-245".to_string()),
        etablissement: "banque-demo".to_string(),
        url_retour: "https://budgy.custhome.app/retour".to_string(),
    };

    let resultat = source.initier_consentement(demande).await;

    assert!(matches!(
        resultat,
        Err(BankDataSourceError::SourceNonConfiguree)
    ));
}
