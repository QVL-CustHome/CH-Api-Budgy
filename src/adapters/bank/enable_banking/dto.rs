use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct DemandeAuth {
    pub access: AccessDemande,
    pub aspsp: AspspReference,
    pub state: String,
    pub redirect_url: String,
    pub psu_type: String,
}

#[derive(Debug, Serialize)]
pub struct AccessDemande {
    pub valid_until: String,
}

#[derive(Debug, Serialize)]
pub struct AspspReference {
    pub name: String,
    pub country: String,
}

#[derive(Debug, Deserialize)]
pub struct ReponseAspsps {
    #[serde(default)]
    pub aspsps: Vec<AspspWire>,
}

#[derive(Debug, Deserialize)]
pub struct AspspWire {
    pub name: String,
    #[serde(default)]
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReponseAuth {
    pub url: String,
    pub authorization_id: String,
}

#[derive(Debug, Serialize)]
pub struct DemandeSession {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct ReponseSession {
    // Absent en Restricted Production (EnableBanking ne renvoie pas de session_id).
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    // Full production : liste d'objets compte. Restricted : liste d'uids (strings).
    #[serde(default)]
    pub accounts: Vec<CompteRef>,
    // Restricted Production : les objets compte sont ici (uid + hashes).
    #[serde(default)]
    pub accounts_data: Vec<CompteSession>,
    #[serde(default)]
    pub access: Option<AccessSession>,
}

// Un compte dans la reponse peut etre soit un objet (full prod), soit un simple
// uid string (restricted). On accepte les deux.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum CompteRef {
    Objet(CompteSession),
    Uid(String),
}

impl ReponseSession {
    // Normalise la liste des comptes quelle que soit la forme de la reponse.
    pub fn comptes(&self) -> Vec<CompteSession> {
        if !self.accounts_data.is_empty() {
            return self.accounts_data.clone();
        }
        self.accounts
            .iter()
            .map(|a| match a {
                CompteRef::Objet(compte) => compte.clone(),
                CompteRef::Uid(uid) => CompteSession {
                    uid: uid.clone(),
                    account_id: None,
                    currency: None,
                },
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct AccessSession {
    #[serde(default)]
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompteSession {
    pub uid: String,
    #[serde(default)]
    pub account_id: Option<IdentifiantCompte>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdentifiantCompte {
    #[serde(default)]
    pub iban: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReponseBalances {
    #[serde(default)]
    pub balances: Vec<BalanceWire>,
}

#[derive(Debug, Deserialize)]
pub struct BalanceWire {
    pub balance_type: String,
    pub balance_amount: MontantWire,
    #[serde(default)]
    pub reference_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MontantWire {
    pub amount: String,
    pub currency: String,
}

#[derive(Debug, Deserialize)]
pub struct ReponseTransactions {
    #[serde(default)]
    pub transactions: Vec<TransactionWire>,
    #[serde(default)]
    pub continuation_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TransactionWire {
    #[serde(default)]
    pub entry_reference: Option<String>,
    #[serde(default)]
    pub transaction_id: Option<String>,
    pub transaction_amount: MontantWire,
    #[serde(default)]
    pub credit_debit_indicator: Option<String>,
    pub status: String,
    #[serde(default)]
    pub booking_date: Option<String>,
    #[serde(default)]
    pub value_date: Option<String>,
    #[serde(default)]
    pub remittance_information: Vec<String>,
}
