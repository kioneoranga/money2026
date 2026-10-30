-- Money 2026 — schéma de base de données (SQLite, chiffrable via SQLCipher)
-- Toutes les clés primaires sont des identifiants texte (ULID/UUID) générés côté application,
-- pour permettre une fusion sans conflit lors d'imports ou d'une future synchronisation.

PRAGMA foreign_keys = ON;

-- Paramètres globaux de l'application (clé/valeur), ex: thème, devise par défaut, format de date.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Devises gérées et leur taux de change vers la devise de référence (celle à taux = 1).
CREATE TABLE currencies (
    code         TEXT PRIMARY KEY,         -- ISO 4217, ex: 'EUR', 'USD'
    name         TEXT NOT NULL,
    symbol       TEXT NOT NULL,
    rate_to_base REAL NOT NULL DEFAULT 1,  -- taux vers la devise de référence
    updated_at   TEXT                      -- date de dernière mise à jour du taux (ISO 8601)
);

-- Catégories hiérarchiques (une catégorie peut avoir un parent).
CREATE TABLE categories (
    id        TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES categories(id) ON DELETE SET NULL,
    name      TEXT NOT NULL,
    icon      TEXT,
    kind      TEXT NOT NULL CHECK (kind IN ('income','expense','transfer')) DEFAULT 'expense',
    archived  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_categories_parent ON categories(parent_id);

-- Tags transverses, applicables à n'importe quelle transaction (plusieurs tags possibles).
CREATE TABLE tags (
    id   TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

-- Tiers (bénéficiaires / payeurs), avec catégorie et compte par défaut pour l'auto-complétion.
CREATE TABLE payees (
    id                   TEXT PRIMARY KEY,
    name                 TEXT NOT NULL UNIQUE,
    default_category_id  TEXT REFERENCES categories(id) ON DELETE SET NULL
);

-- Comptes : courant, épargne, carte à débit différé, placements, prêts/dettes.
CREATE TABLE accounts (
    id               TEXT PRIMARY KEY,
    name             TEXT NOT NULL,
    type             TEXT NOT NULL CHECK (type IN ('checking','savings','deferred_card','investment','loan')),
    currency_code    TEXT NOT NULL REFERENCES currencies(code),
    opening_balance  INTEGER NOT NULL DEFAULT 0,  -- en centimes, pour éviter les erreurs d'arrondi flottant
    archived         INTEGER NOT NULL DEFAULT 0,
    sort_order       INTEGER NOT NULL DEFAULT 0
);

-- Transactions. Un virement entre deux comptes est représenté par deux lignes liées via transfer_id.
CREATE TABLE transactions (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    date            TEXT NOT NULL,          -- ISO 8601 (YYYY-MM-DD)
    payee_id        TEXT REFERENCES payees(id) ON DELETE SET NULL,
    category_id     TEXT REFERENCES categories(id) ON DELETE SET NULL,
    amount          INTEGER NOT NULL,       -- en centimes, signé (négatif = dépense)
    memo            TEXT,
    cleared         INTEGER NOT NULL DEFAULT 0,  -- pointé
    reconciled      INTEGER NOT NULL DEFAULT 0,  -- rapproché
    transfer_id     TEXT,                   -- relie les deux lignes d'un virement entre comptes
    import_source   TEXT,                   -- 'qif' | 'ofx' | 'csv' | 'mny' | NULL (saisie manuelle)
    import_hash     TEXT,                   -- empreinte pour la détection de doublons à l'import
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_tx_account_date ON transactions(account_id, date);
CREATE INDEX idx_tx_category     ON transactions(category_id);
CREATE INDEX idx_tx_transfer     ON transactions(transfer_id);
CREATE UNIQUE INDEX idx_tx_import_dedup ON transactions(account_id, import_hash)
    WHERE import_hash IS NOT NULL;

-- Association transactions <-> tags (plusieurs-à-plusieurs).
CREATE TABLE transaction_tags (
    transaction_id TEXT NOT NULL REFERENCES transactions(id) ON DELETE CASCADE,
    tag_id         TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (transaction_id, tag_id)
);

-- Échéances / opérations récurrentes.
CREATE TABLE scheduled_transactions (
    id           TEXT PRIMARY KEY,
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    payee_id     TEXT REFERENCES payees(id) ON DELETE SET NULL,
    category_id  TEXT REFERENCES categories(id) ON DELETE SET NULL,
    amount       INTEGER NOT NULL,
    memo         TEXT,
    next_date    TEXT NOT NULL,
    frequency    TEXT NOT NULL CHECK (frequency IN ('weekly','monthly','yearly')),
    interval_n   INTEGER NOT NULL DEFAULT 1,
    active       INTEGER NOT NULL DEFAULT 1
);

-- Budgets mensuels par catégorie (un montant par catégorie et par mois).
CREATE TABLE budgets (
    category_id TEXT NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    month       TEXT NOT NULL,  -- 'YYYY-MM'
    amount      INTEGER NOT NULL,
    PRIMARY KEY (category_id, month)
);

-- Positions de placement (titres, ETF, crypto...) rattachées à un compte de type 'investment'.
CREATE TABLE holdings (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    symbol      TEXT NOT NULL,
    quantity    REAL NOT NULL,
    unit_cost   INTEGER NOT NULL,  -- en centimes
    last_price  INTEGER NOT NULL, -- en centimes
    updated_at  TEXT
);

-- Prêts / dettes rattachés à un compte de type 'loan'.
CREATE TABLE loans (
    id              TEXT PRIMARY KEY,
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    principal       INTEGER NOT NULL,  -- capital emprunté, en centimes
    annual_rate_bp  INTEGER NOT NULL,  -- taux annuel en points de base (ex: 350 = 3,50 %)
    term_months     INTEGER NOT NULL,
    start_date      TEXT NOT NULL
);

-- Règles d'import automatique : si le libellé/montant correspond, affecter tiers/catégorie/tags.
CREATE TABLE import_rules (
    id             TEXT PRIMARY KEY,
    priority       INTEGER NOT NULL DEFAULT 0,
    match_field    TEXT NOT NULL CHECK (match_field IN ('payee','memo','amount')),
    match_type     TEXT NOT NULL CHECK (match_type IN ('contains','equals','regex','gt','lt')),
    match_value    TEXT NOT NULL,
    set_payee_id   TEXT REFERENCES payees(id) ON DELETE SET NULL,
    set_category_id TEXT REFERENCES categories(id) ON DELETE SET NULL,
    active         INTEGER NOT NULL DEFAULT 1
);

-- Journal des sauvegardes automatiques effectuées.
CREATE TABLE backups_log (
    id         TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    file_path  TEXT NOT NULL,
    size_bytes INTEGER
);
