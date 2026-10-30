//! Coeur metier de Money 2026.
//!
//! Ce crate ne depend d'aucun framework d'interface graphique : il peut etre
//! appele depuis une coquille Tauri (desktop Windows), depuis des tests, ou
//! un jour depuis une autre plateforme. Toute la logique testable vit ici ;
//! la coquille Tauri (src-tauri/) ne fait qu'exposer ces fonctions comme
//! commandes IPC pour le frontend.

use rusqlite::{params, Connection, OptionalExtension};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SCHEMA: &str = include_str!("../../schema.sql");
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Identifiant unique, trie-able par ordre de creation (horodatage + compteur).
pub fn new_id() -> String {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{ts:x}-{n:x}")
}

pub struct Db {
    pub conn: Connection,
}

impl Db {
    /// Ouvre (ou cree) la base au chemin donne et applique le schema si absent.
    /// `:memory:` est accepte pour les tests.
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // -------------------- Comptes --------------------

    pub fn add_currency(&self, code: &str, name: &str, symbol: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO currencies(code,name,symbol,rate_to_base) VALUES (?1,?2,?3,1)",
            params![code, name, symbol],
        )?;
        Ok(())
    }

    pub fn add_account(&self, name: &str, kind: &str, currency: &str, opening_cents: i64) -> rusqlite::Result<String> {
        let id = new_id();
        self.conn.execute(
            "INSERT INTO accounts(id,name,type,currency_code,opening_balance) VALUES (?1,?2,?3,?4,?5)",
            params![id, name, kind, currency, opening_cents],
        )?;
        Ok(id)
    }

    /// Solde courant d'un compte = solde d'ouverture + somme des transactions.
    pub fn account_balance(&self, account_id: &str) -> rusqlite::Result<i64> {
        let opening: i64 = self.conn.query_row(
            "SELECT opening_balance FROM accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )?;
        let moved: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(amount),0) FROM transactions WHERE account_id = ?1",
            params![account_id],
            |r| r.get(0),
        )?;
        Ok(opening + moved)
    }

    // -------------------- Categories / tiers --------------------

    pub fn add_category(&self, name: &str, kind: &str, parent_id: Option<&str>) -> rusqlite::Result<String> {
        let id = new_id();
        self.conn.execute(
            "INSERT INTO categories(id,parent_id,name,kind) VALUES (?1,?2,?3,?4)",
            params![id, parent_id, name, kind],
        )?;
        Ok(id)
    }

    pub fn add_payee(&self, name: &str) -> rusqlite::Result<String> {
        let id = new_id();
        self.conn.execute("INSERT INTO payees(id,name) VALUES (?1,?2)", params![id, name])?;
        Ok(id)
    }

    fn find_payee_id(&self, name: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row("SELECT id FROM payees WHERE name = ?1", params![name], |r| r.get(0))
            .optional()
    }

    // -------------------- Transactions --------------------

    /// Ajoute une transaction simple. `amount_cents` est signe (negatif = depense).
    /// Cree le tiers s'il n'existe pas encore, et applique les regles d'import actives.
    pub fn add_transaction(
        &self,
        account_id: &str,
        date: &str,
        payee_name: &str,
        amount_cents: i64,
        memo: &str,
        import_source: Option<&str>,
        import_hash: Option<&str>,
    ) -> rusqlite::Result<String> {
        let payee_id = match self.find_payee_id(payee_name)? {
            Some(id) => id,
            None => self.add_payee(payee_name)?,
        };
        let category_id = self.apply_import_rules(payee_name, memo, amount_cents)?;
        let id = new_id();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        self.conn.execute(
            "INSERT INTO transactions(id,account_id,date,payee_id,category_id,amount,memo,import_source,import_hash,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
            params![id, account_id, date, payee_id, category_id, amount_cents, memo, import_source, import_hash, now],
        )?;
        Ok(id)
    }

    /// Cree un virement entre deux comptes : deux transactions liees par transfer_id,
    /// montants opposes.
    pub fn add_transfer(&self, from_account: &str, to_account: &str, date: &str, amount_cents: i64, memo: &str) -> rusqlite::Result<(String, String)> {
        let transfer_id = new_id();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        let out_id = new_id();
        let in_id = new_id();
        self.conn.execute(
            "INSERT INTO transactions(id,account_id,date,amount,memo,transfer_id,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![out_id, from_account, date, -amount_cents.abs(), memo, transfer_id, now],
        )?;
        self.conn.execute(
            "INSERT INTO transactions(id,account_id,date,amount,memo,transfer_id,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![in_id, to_account, date, amount_cents.abs(), memo, transfer_id, now],
        )?;
        Ok((out_id, in_id))
    }

    pub fn update_transaction(&self, id: &str, date: &str, amount_cents: i64, memo: &str) -> rusqlite::Result<usize> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs().to_string();
        self.conn.execute(
            "UPDATE transactions SET date=?2, amount=?3, memo=?4, updated_at=?5 WHERE id=?1",
            params![id, date, amount_cents, memo, now],
        )
    }

    // -------------------- Regles d'import --------------------

    pub fn add_import_rule(&self, priority: i64, field: &str, match_type: &str, value: &str, category_id: &str) -> rusqlite::Result<String> {
        let id = new_id();
        self.conn.execute(
            "INSERT INTO import_rules(id,priority,match_field,match_type,match_value,set_category_id) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, priority, field, match_type, value, category_id],
        )?;
        Ok(id)
    }

    /// Retourne la categorie a affecter selon la premiere regle active qui correspond
    /// (triee par priorite decroissante), ou None si aucune ne correspond.
    fn apply_import_rules(&self, payee: &str, memo: &str, amount_cents: i64) -> rusqlite::Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT match_field, match_type, match_value, set_category_id FROM import_rules
             WHERE active = 1 ORDER BY priority DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        for row in rows {
            let (field, mtype, value, cat) = row?;
            let subject = match field.as_str() {
                "payee" => payee,
                "memo" => memo,
                _ => "",
            };
            let matched = match mtype.as_str() {
                "contains" => subject.to_lowercase().contains(&value.to_lowercase()),
                "equals" => subject.eq_ignore_ascii_case(&value),
                "gt" => value.parse::<i64>().map(|v| amount_cents > v).unwrap_or(false),
                "lt" => value.parse::<i64>().map(|v| amount_cents < v).unwrap_or(false),
                _ => false,
            };
            if matched {
                return Ok(cat);
            }
        }
        Ok(None)
    }

    // -------------------- Budget --------------------

    pub fn set_budget(&self, category_id: &str, month: &str, amount_cents: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO budgets(category_id,month,amount) VALUES (?1,?2,?3)
             ON CONFLICT(category_id,month) DO UPDATE SET amount = excluded.amount",
            params![category_id, month, amount_cents],
        )?;
        Ok(())
    }

    pub fn spent_for_category(&self, category_id: &str, month: &str) -> rusqlite::Result<i64> {
        let pattern = format!("{month}%");
        self.conn.query_row(
            "SELECT COALESCE(-SUM(amount),0) FROM transactions
             WHERE category_id = ?1 AND date LIKE ?2 AND amount < 0",
            params![category_id, pattern],
            |r| r.get(0),
        )
    }

    // -------------------- Echeances & tresorerie previsionnelle --------------------

    pub fn add_scheduled(&self, account_id: &str, payee_name: &str, amount_cents: i64, next_date: &str, frequency: &str) -> rusqlite::Result<String> {
        let payee_id = match self.find_payee_id(payee_name)? {
            Some(id) => id,
            None => self.add_payee(payee_name)?,
        };
        let id = new_id();
        self.conn.execute(
            "INSERT INTO scheduled_transactions(id,account_id,payee_id,amount,next_date,frequency) VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, account_id, payee_id, amount_cents, next_date, frequency],
        )?;
        Ok(id)
    }

    /// Projette le solde d'un compte a J+`days`, en appliquant les echeances actives
    /// qui tombent dans la fenetre (pure simulation, rien n'est ecrit en base).
    pub fn forecast_balance(&self, account_id: &str, from_date: &str, days: i64) -> rusqlite::Result<i64> {
        let mut balance = self.account_balance(account_id)?;
        let from = parse_date(from_date);
        let horizon = from + days;

        let mut stmt = self.conn.prepare(
            "SELECT amount, next_date, frequency FROM scheduled_transactions
             WHERE account_id = ?1 AND active = 1",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for row in rows {
            let (amount, next_date, frequency) = row?;
            let mut d = parse_date(&next_date);
            let step = match frequency.as_str() {
                "weekly" => 7,
                "monthly" => 30,
                "yearly" => 365,
                _ => 30,
            };
            while d <= horizon {
                if d >= from {
                    balance += amount;
                }
                d += step;
            }
        }
        Ok(balance)
    }
}

/// Convertit une date ISO (YYYY-MM-DD) en nombre de jours depuis une epoque arbitraire,
/// suffisant pour comparer/additionner des dates sans dependance externe.
fn parse_date(s: &str) -> i64 {
    let parts: Vec<i64> = s.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return 0;
    }
    let (y, m, d) = (parts[0], parts[1], parts[2]);
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_db() -> Db {
        let db = Db::open(":memory:").unwrap();
        db.add_currency("EUR", "Euro", "€").unwrap();
        db
    }

    #[test]
    fn schema_applies_and_accounts_work() {
        let db = fresh_db();
        let acc = db.add_account("Compte courant", "checking", "EUR", 100_00).unwrap();
        assert_eq!(db.account_balance(&acc).unwrap(), 100_00);
    }

    #[test]
    fn transactions_affect_balance() {
        let db = fresh_db();
        let acc = db.add_account("Compte courant", "checking", "EUR", 0).unwrap();
        db.add_transaction(&acc, "2026-01-05", "Employeur", 2500_00, "Salaire", None, None).unwrap();
        db.add_transaction(&acc, "2026-01-10", "Supermarché", -45_30, "Courses", None, None).unwrap();
        assert_eq!(db.account_balance(&acc).unwrap(), 2454_70);
    }

    #[test]
    fn transfer_balances_both_accounts() {
        let db = fresh_db();
        let a = db.add_account("Courant", "checking", "EUR", 1000_00).unwrap();
        let b = db.add_account("Livret A", "savings", "EUR", 0).unwrap();
        db.add_transfer(&a, &b, "2026-02-01", 300_00, "Épargne mensuelle").unwrap();
        assert_eq!(db.account_balance(&a).unwrap(), 700_00);
        assert_eq!(db.account_balance(&b).unwrap(), 300_00);
    }

    #[test]
    fn import_rule_auto_categorizes() {
        let db = fresh_db();
        let acc = db.add_account("Courant", "checking", "EUR", 0).unwrap();
        let cat = db.add_category("Alimentation", "expense", None).unwrap();
        db.add_import_rule(10, "payee", "contains", "Carrefour", &cat).unwrap();
        let tx = db.add_transaction(&acc, "2026-03-01", "Carrefour Market", -60_00, "", Some("qif"), None).unwrap();
        let assigned: Option<String> = db.conn.query_row(
            "SELECT category_id FROM transactions WHERE id = ?1", params![tx], |r| r.get(0),
        ).unwrap();
        assert_eq!(assigned.as_deref(), Some(cat.as_str()));
    }

    #[test]
    fn duplicate_import_hash_is_rejected() {
        let db = fresh_db();
        let acc = db.add_account("Courant", "checking", "EUR", 0).unwrap();
        db.add_transaction(&acc, "2026-03-01", "EDF", -80_00, "", Some("qif"), Some("hash-1")).unwrap();
        let second = db.add_transaction(&acc, "2026-03-01", "EDF", -80_00, "", Some("qif"), Some("hash-1"));
        assert!(second.is_err(), "un meme import_hash sur le meme compte doit etre rejete (anti-doublon)");
    }

    #[test]
    fn budget_tracks_spending() {
        let db = fresh_db();
        let acc = db.add_account("Courant", "checking", "EUR", 0).unwrap();
        let cat = db.add_category("Loisirs", "expense", None).unwrap();
        db.set_budget(&cat, "2026-04", 200_00).unwrap();
        db.add_transaction(&acc, "2026-04-03", "Cinéma", -12_50, "", None, None).unwrap();
        db.add_transaction(&acc, "2026-04-20", "Concert", -55_00, "", None, None).unwrap();
        db.conn.execute(
            "UPDATE transactions SET category_id=?1 WHERE account_id=?2",
            params![cat, acc],
        ).unwrap();
        assert_eq!(db.spent_for_category(&cat, "2026-04").unwrap(), 67_50);
    }

    #[test]
    fn forecast_includes_upcoming_scheduled_transactions() {
        let db = fresh_db();
        let acc = db.add_account("Courant", "checking", "EUR", 500_00).unwrap();
        db.add_scheduled(&acc, "Employeur", 2000_00, "2026-05-01", "monthly").unwrap();
        db.add_scheduled(&acc, "Loyer", -800_00, "2026-05-03", "monthly").unwrap();
        let projected = db.forecast_balance(&acc, "2026-04-25", 30).unwrap();
        assert_eq!(projected, 500_00 + 2000_00 - 800_00);
    }
}
