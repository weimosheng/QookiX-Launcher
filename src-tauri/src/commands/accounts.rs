use crate::accounts;
use crate::models::*;
use crate::state::AppState;
use serde_json::Value;
use tauri::State;

// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(state: State<AppState>) -> Result<Vec<Value>, String> {
    Ok(accounts::load_accounts(&state)
        .into_iter()
        .map(|a| strip_account_tokens(serde_json::to_value(&a).unwrap_or_default()))
        .collect())
}

#[tauri::command]
pub fn login_offline(state: State<AppState>, username: String) -> Result<Account, String> {
    accounts::create_offline(&state, &username)
}

#[tauri::command]
pub async fn login_ms_start(state: State<'_, AppState>) -> Result<Value, String> {
    accounts::ms_start(&state).await
}

#[tauri::command]
pub async fn login_ms_poll(state: State<'_, AppState>) -> Result<Value, String> {
    let acc = accounts::ms_poll(&state).await?;
    Ok(strip_account_tokens(serde_json::to_value(&acc).unwrap_or_default()))
}

/// Remove sensitive token fields before sending an Account to the frontend.
fn strip_account_tokens(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("msa_refresh_token");
        obj.remove("msa_access_token");
    }
    v
}

#[tauri::command]
pub fn logout_account(state: State<AppState>, uuid: String) -> Result<(), String> {
    accounts::remove_account(&state, &uuid)
}

