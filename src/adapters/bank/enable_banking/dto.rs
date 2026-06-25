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
    pub session_id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub accounts: Vec<CompteSession>,
    #[serde(default)]
    pub access: Option<AccessSession>,
}

#[derive(Debug, Deserialize)]
pub struct AccessSession {
    #[serde(default)]
    pub valid_until: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CompteSession {
    pub uid: String,
    #[serde(default)]
    pub account_id: Option<IdentifiantCompte>,
    #[serde(default)]
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
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
