use crate::adapters::bank::determinisme::uuid_depuis;
use crate::adapters::bank::enable_banking::dto::{
    AspspWire, BalanceWire, CompteSession, MontantWire, TransactionWire,
};
use crate::domain::balance::{Balance, BalanceId, BalanceType};
use crate::domain::bank_account::{BankAccount, BankAccountId, masquer_iban};
use crate::domain::consent::{Consent, ConsentId};
use crate::domain::ports::bank_data_source::{BankDataSourceError, Etablissement};
use crate::domain::transaction_bancaire::{
    CategorizationSource, TransactionBancaire, TransactionBancaireId, TransactionStatus,
};
use chrono::{DateTime, NaiveDate, Utc};

pub fn montant_en_centimes(montant: &MontantWire) -> Result<i64, BankDataSourceError> {
    let texte = montant.amount.trim();
    let (signe, chiffres) = match texte.strip_prefix('-') {
        Some(reste) => (-1i64, reste),
        None => (1i64, texte.strip_prefix('+').unwrap_or(texte)),
    };
    let (entiers, decimales) = match chiffres.split_once('.') {
        Some((e, d)) => (e, d),
        None => (chiffres, ""),
    };
    let centimes_decimaux = centimes_arrondis(decimales)?;
    let entiers = if entiers.is_empty() {
        0
    } else {
        parser_entier(entiers)?
    };
    Ok(signe * (entiers * 100 + centimes_decimaux))
}

fn centimes_arrondis(decimales: &str) -> Result<i64, BankDataSourceError> {
    if decimales.chars().any(|c| !c.is_ascii_digit()) {
        return Err(BankDataSourceError::ReponseInvalide(format!(
            "décimales illisibles : {decimales}"
        )));
    }
    let chiffre = |position: usize| -> i64 {
        decimales
            .as_bytes()
            .get(position)
            .map(|octet| i64::from(octet - b'0'))
            .unwrap_or(0)
    };
    let centimes = chiffre(0) * 10 + chiffre(1);
    let arrondi = i64::from(chiffre(2) >= 5);
    Ok(centimes + arrondi)
}

fn parser_entier(valeur: &str) -> Result<i64, BankDataSourceError> {
    valeur
        .parse::<i64>()
        .map_err(|_| BankDataSourceError::ReponseInvalide(format!("montant illisible : {valeur}")))
}

fn parser_date(valeur: &Option<String>) -> Option<NaiveDate> {
    valeur
        .as_ref()
        .and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
}

fn parser_horodatage(valeur: &Option<String>, defaut: DateTime<Utc>) -> DateTime<Utc> {
    valeur
        .as_ref()
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|| {
            valeur
                .as_ref()
                .and_then(|v| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
                .and_then(|d| d.and_hms_opt(0, 0, 0))
                .map(|naive| DateTime::from_naive_utc_and_offset(naive, Utc))
        })
        .unwrap_or(defaut)
}

pub fn vers_bank_account(
    compte: &CompteSession,
    consent: &Consent,
    horodatage: DateTime<Utc>,
) -> BankAccount {
    let iban = compte
        .account_id
        .as_ref()
        .and_then(|id| id.iban.clone())
        .unwrap_or_default();
    BankAccount {
        id: BankAccountId(uuid_depuis(&format!("eb-account-{}", compte.uid))),
        proprietaire: consent.proprietaire.clone(),
        consent: consent.id.clone(),
        external_account_id: compte.uid.clone(),
        iban_masked: masquer_iban(&iban),
        currency: compte.currency.clone().unwrap_or_default(),
        next_sync_at: None,
        sync_count_today: 0,
        created_at: horodatage,
        updated_at: horodatage,
    }
}

fn type_balance(valeur: &str) -> BalanceType {
    match valeur.to_uppercase().as_str() {
        "CLBD" | "BOOKED" | "PRCD" => BalanceType::Booked,
        "XPCD" | "EXPECTED" => BalanceType::Expected,
        _ => BalanceType::Available,
    }
}

pub fn vers_balance(
    balance: &BalanceWire,
    compte: &BankAccount,
    horodatage: DateTime<Utc>,
) -> Result<Balance, BankDataSourceError> {
    let reference_date = parser_horodatage(&balance.reference_date, horodatage);
    let balance_type = type_balance(&balance.balance_type);
    Ok(Balance {
        id: BalanceId(uuid_depuis(&format!(
            "eb-balance-{}-{}",
            compte.id.0,
            balance_type.as_str()
        ))),
        bank_account: compte.id.clone(),
        balance_type,
        amount_cents: montant_en_centimes(&balance.balance_amount)?,
        currency: balance.balance_amount.currency.clone(),
        reference_date,
        created_at: horodatage,
    })
}

fn statut_transaction(valeur: &str) -> TransactionStatus {
    match valeur.to_uppercase().as_str() {
        "BOOK" | "BOOKED" => TransactionStatus::Booked,
        _ => TransactionStatus::Pending,
    }
}

fn reference_externe(transaction: &TransactionWire) -> Option<String> {
    transaction
        .entry_reference
        .clone()
        .or_else(|| transaction.transaction_id.clone())
}

pub fn vers_transaction(
    transaction: &TransactionWire,
    compte: &BankAccount,
    horodatage: DateTime<Utc>,
) -> Result<TransactionBancaire, BankDataSourceError> {
    let external_transaction_id = reference_externe(transaction).ok_or_else(|| {
        BankDataSourceError::ReponseInvalide("transaction sans référence".to_string())
    })?;
    let mut amount_cents = montant_en_centimes(&transaction.transaction_amount)?;
    if matches!(
        transaction.credit_debit_indicator.as_deref(),
        Some("DBIT") | Some("Debit")
    ) {
        amount_cents = -amount_cents.abs();
    }
    let label = if transaction.remittance_information.is_empty() {
        external_transaction_id.clone()
    } else {
        transaction.remittance_information.join(" ")
    };
    Ok(TransactionBancaire {
        id: TransactionBancaireId(uuid_depuis(&format!(
            "eb-tx-{}-{}",
            compte.id.0, external_transaction_id
        ))),
        bank_account: compte.id.clone(),
        external_transaction_id,
        status: statut_transaction(&transaction.status),
        label,
        amount_cents,
        currency: transaction.transaction_amount.currency.clone(),
        booking_date: parser_date(&transaction.booking_date),
        value_date: parser_date(&transaction.value_date),
        category: None,
        categorization_source: CategorizationSource::None,
        rule_id: None,
        created_at: horodatage,
    })
}

pub fn consent_id_depuis_reference(reference_autorisation: &str) -> Option<ConsentId> {
    uuid::Uuid::parse_str(reference_autorisation)
        .ok()
        .map(ConsentId)
}

pub fn vers_etablissement(aspsp: &AspspWire) -> Etablissement {
    let pays = aspsp.country.clone().unwrap_or_default();
    Etablissement {
        id: format!("{}|{}", aspsp.name, pays),
        nom: aspsp.name.clone(),
        pays,
    }
}
