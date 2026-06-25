use chrono::{DateTime, TimeZone, Utc};
use uuid::Uuid;

pub const ANCRAGE_TEMPOREL: i64 = 1_700_000_000;

pub fn horodatage_ancre() -> DateTime<Utc> {
    Utc.timestamp_opt(ANCRAGE_TEMPOREL, 0)
        .single()
        .expect("ancrage temporel constant valide")
}

const NAMESPACE_MOCK_BANK: Uuid = Uuid::from_bytes([
    0x1b, 0x9d, 0x6f, 0x4c, 0x2a, 0x83, 0x4e, 0x71, 0x9c, 0x55, 0xd0, 0xe2, 0x47, 0x8a, 0x3f, 0x10,
]);

pub fn uuid_depuis(graine: &str) -> Uuid {
    Uuid::new_v5(&NAMESPACE_MOCK_BANK, graine.as_bytes())
}
