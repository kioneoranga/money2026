// Coquille Tauri : expose le coeur metier (money_core) au frontend via des commandes IPC.
// Toute la logique (SQL, calculs) vit dans money_core, testee independamment (cargo test).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use money_core::Db;
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState(Mutex<Db>);

fn db_path(app: &tauri::AppHandle) -> String {
    // Portable : la base vit a cote de l'executable, pas dans %APPDATA%.
    let dir = app.path_resolver().app_local_data_dir().unwrap_or_else(|| ".".into());
    std::fs::create_dir_all(&dir).ok();
    dir.join("money2026.db").to_string_lossy().to_string()
}

#[tauri::command]
fn list_accounts_with_balance(state: State<AppState>) -> Result<Vec<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db.conn.prepare("SELECT id, name, type FROM accounts WHERE archived = 0")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
        .map_err(|e| e.to_string())?;
    let mut out = vec![];
    for row in rows {
        let (id, name, kind) = row.map_err(|e| e.to_string())?;
        let balance = db.account_balance(&id).map_err(|e| e.to_string())?;
        out.push(serde_json::json!({ "id": id, "name": name, "type": kind, "balance_cents": balance }));
    }
    Ok(out)
}

#[tauri::command]
fn add_account(state: State<AppState>, name: String, kind: String, opening_cents: i64) -> Result<String, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.add_account(&name, &kind, "EUR", opening_cents).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_transaction(state: State<AppState>, account_id: String, date: String, payee: String, amount_cents: i64, memo: String) -> Result<String, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.add_transaction(&account_id, &date, &payee, amount_cents, &memo, None, None).map_err(|e| e.to_string())
}

#[tauri::command]
fn forecast_balance(state: State<AppState>, account_id: String, from_date: String, days: i64) -> Result<i64, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.forecast_balance(&account_id, &from_date, days).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let path = db_path(&app.handle());
            let db = Db::open(&path).expect("ouverture base de donnees");
            db.add_currency("EUR", "Euro", "€").ok();
            app.manage(AppState(Mutex::new(db)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_accounts_with_balance,
            add_account,
            add_transaction,
            forecast_balance
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de Money 2026");
}
