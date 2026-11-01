// Coquille Tauri : expose le coeur metier (money_core) au frontend via des commandes IPC.
// Toute la logique (SQL, calculs) vit dans money_core, testee independamment (cargo test).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use money_core::Db;
use std::sync::Mutex;
use tauri::{Manager, State};

struct AppState(Mutex<Db>);

/// Essaie plusieurs emplacements dans l'ordre, pour ne jamais rester bloque
/// si le premier choix (dossier de donnees applicatives) pose probleme.
fn candidate_dirs(app: &tauri::AppHandle) -> Vec<std::path::PathBuf> {
    let mut v = vec![];
    if let Some(dir) = app.path_resolver().app_local_data_dir() {
        v.push(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.to_path_buf());
        }
    }
    v.push(std::env::temp_dir());
    v
}

/// Ouvre la base en essayant chaque emplacement candidat ; renvoie une erreur
/// lisible (avec le chemin essaye) si aucun n'a fonctionne, plutot que de paniquer.
fn open_db(app: &tauri::AppHandle) -> Result<(Db, std::path::PathBuf), String> {
    let mut last_err = String::from("aucun emplacement candidat");
    for dir in candidate_dirs(app) {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            last_err = format!("création du dossier {} impossible : {e}", dir.display());
            continue;
        }
        let path = dir.join("money2026.db");
        match Db::open(&path.to_string_lossy()) {
            Ok(db) => return Ok((db, path)),
            Err(e) => last_err = format!("ouverture de {} impossible : {e}", path.display()),
        }
    }
    Err(last_err)
}

#[tauri::command]
fn list_accounts_with_balance(state: State<AppState>) -> Result<Vec<serde_json::Value>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .conn
        .prepare("SELECT id, name, type FROM accounts WHERE archived = 0")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
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
            let handle = app.handle();
            match open_db(&handle) {
                Ok((db, _path)) => {
                    db.add_currency("EUR", "Euro", "€").ok();
                    app.manage(AppState(Mutex::new(db)));
                }
                Err(e) => {
                    let msg = format!("Impossible d'ouvrir la base de données Money 2026.\n\n{e}");
                    if let Ok(exe) = std::env::current_exe() {
                        if let Some(dir) = exe.parent() {
                            let _ = std::fs::write(dir.join("money2026-crash.log"), &msg);
                        }
                    }
                    // La boite de dialogue bloquante ne doit pas tourner sur le thread
                    // principal : on la lance sur un thread a part, puis on quitte.
                    std::thread::spawn(move || {
                        tauri::api::dialog::blocking::message(
                            None::<&tauri::Window>,
                            "Money 2026 — erreur de démarrage",
                            &msg,
                        );
                        std::process::exit(1);
                    });
                }
            }
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
