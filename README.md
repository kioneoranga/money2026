# Money 2026 — prototype d'architecture (v2)

Ce dossier est la fondation d'une réécriture de Money 2026 sur une architecture plus
proche de ce que demande le cahier des charges : Tauri (coquille Rust légère + webview
du système, pas de Chromium embarqué) au lieu d'Electron.

## Pourquoi ce choix, et ce qui a vraiment été vérifié ici

- `core/` est un crate Rust **pur**, sans dépendance graphique. Il contient tout le
  métier : comptes, transactions, virements (double écriture), règles de
  catégorisation automatique à l'import, budgets, trésorerie prévisionnelle (J+30/60/90
  à partir des échéances). **Compilé et testé ici même** : `cd core && cargo test` passe
  (7 tests, base SQLite réelle en mémoire).
- `schema.sql` est le schéma complet (comptes, transactions, tiers, catégories
  hiérarchiques, tags, règles d'import, budgets, placements, prêts, devises,
  journal de sauvegardes), source unique utilisée par `core`.
- `app/src-tauri/` est la coquille Tauri (config + commandes IPC qui exposent `core`
  au frontend). Elle est écrite selon la documentation officielle de Tauri 1.x, mais
  **n'a pas pu être compilée dans ce bac à sable Linux** : il manque les bibliothèques
  système d'interface graphique (`webkit2gtk`), et surtout, compiler pour Windows
  depuis Linux n'est pas fiable sans une vraie machine Windows.
- `app/src/index.html` est une mini-page de démonstration (pas l'interface finale)
  qui prouve le branchement de bout en bout : lister des comptes, en ajouter un,
  via de vrais appels au cœur Rust.

## Comment obtenir un vrai `.exe` testé

Deux façons, au choix :

1. **Sur une machine Windows** (la vôtre ou une machine de test) :
   installer Rust, Node.js, puis `npm install -g @tauri-apps/cli@1` et
   `cd app && tauri dev` (pour tester) ou `tauri build` (pour l'exécutable final,
   dans `app/src-tauri/target/release/`).
2. **Sans machine Windows, via GitHub** : poussez ce dossier sur un dépôt GitHub.
   Le fichier `.github/workflows/build-windows.yml` est prêt : il compile
   automatiquement sur une vraie machine Windows (fournie par GitHub Actions) à
   chaque envoi sur `main`, et dépose l'exécutable en téléchargement dans l'onglet
   "Actions" du dépôt. Aucune installation nécessaire de votre côté.

## Comparaison avec la version actuelle (Electron, livrée en v1 à v5)

| | Electron (actuel) | Tauri (ce prototype) |
|---|---|---|
| Taille du dossier livré | ~115 Mo | ~10-20 Mo attendus |
| Démarrage | correct | plus rapide (pas de Chromium à charger) |
| Stockage | `localStorage` (non chiffré, non structuré) | SQLite réel, chiffrable (SQLCipher, à ajouter) |
| Je peux vous livrer un `.exe` testé directement depuis cette conversation | **oui** | **non** — il faut passer par GitHub Actions ou une machine Windows (ci-dessus) |
| Interface déjà construite | complète (comptes, transactions, budget, échéancier, placements, prêts, synthèse) | à reporter depuis la version Electron (environ 1 jour de travail, réutilisable à 90 %) |

## Prochaines étapes possibles

- Reporter l'interface déjà construite (HTML/CSS) dans `app/src/`, en remplaçant les
  appels `localStorage`/Electron par des appels `invoke(...)` vers les commandes Rust.
- Ajouter le chiffrement (SQLCipher) et un code PIN/mot de passe maître au démarrage.
- Étendre les commandes Tauri pour couvrir l'import QIF/OFX/CSV (logique à porter en
  Rust, ou conservée en JavaScript côté frontend, qui appelle ensuite `add_transaction`
  pour chaque ligne).
