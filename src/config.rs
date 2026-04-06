use std::env;

pub fn get_host() -> String {
    env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into())
}

pub fn get_port() -> u16 {
    env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000)
}

pub fn get_database_url() -> String {
    env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:data/income.db?mode=rwc".into())
}

pub fn tmo_company_id() -> String {
    env::var("TMO_COMPANY_ID").unwrap_or_else(|_| "vci".into())
}

pub fn tmo_account() -> String {
    env::var("TMO_ACCOUNT").expect("TMO_ACCOUNT must be set")
}

pub fn tmo_pin() -> String {
    env::var("TMO_PIN").expect("TMO_PIN must be set")
}

pub fn monarch_token() -> String {
    env::var("MONARCH_TOKEN").expect("MONARCH_TOKEN must be set")
}

pub fn monarch_account_id() -> String {
    env::var("MONARCH_ACCOUNT_ID").unwrap_or_else(|_| "217902882668946592".into())
}
