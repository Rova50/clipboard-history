# clipboard-history

Historique des copies (Ctrl+C) de la session, avec un sélecteur ouvert par **Super+V** (touche Windows + V).

Un seul exécutable Rust (~650 Ko) qui ne dépend que de GTK 4, présent sur les distributions de bureau récentes.

## Installation

### Paquet Debian / Ubuntu (Ubuntu 22.04+, Debian 12+, Linux Mint 21+)

Télécharger `clipboard-history_amd64.deb` depuis la page *Releases* du dépôt, puis :

```bash
sudo apt install ./clipboard-history_amd64.deb
clipboard-history --daemon &    # ou fermer puis rouvrir la session
```

Au premier démarrage, sous GNOME, Super+V est associé automatiquement à l'historique.
GNOME utilise Super+V pour la liste des notifications, qui reste accessible avec Super+M.

### Autres bureaux (KDE, XFCE, i3…)

- lancer `clipboard-history --daemon` à l'ouverture de session (fourni par le paquet via `/etc/xdg/autostart`) ;
- associer Super+V à la commande `clipboard-history show` dans les réglages du clavier.

## Utilisation

| Touche | Action |
|---|---|
| Super+V | ouvrir la liste (Super+V à nouveau : entrée suivante) |
| ↑ / ↓, Tab | défiler |
| taper du texte | filtrer |
| Entrée / clic | mettre l'entrée dans le presse-papiers et coller |
| Ctrl+P ou clic sur ☆ | épingler l'entrée dans les favoris (ou l'en retirer) |
| Suppr | retirer l'entrée |
| Échap | fermer |

L'historique de la session est en mémoire uniquement (100 entrées max, texte seulement) : il disparaît
à la fin de la session.

Les **favoris** (entrées épinglées) sont affichés en tête de liste et sont les seules entrées gardées
d'une session à l'autre, dans `~/.local/share/clipboard-history/favorites.json` (lisible par vous seul).
Rien n'y est écrit sans que vous ayez épinglé l'entrée.

Ne sont pas enregistrés :
- les copies marquées comme secrètes par les gestionnaires de mots de passe (KeePassXC, Bitwarden, 1Password…) ;
- les textes de plus de 1 Mo (ils ne sont pas conservés en mémoire).

Commandes :

```
clipboard-history show               ouvrir la liste
clipboard-history list               afficher l'historique dans le terminal
clipboard-history clear              vider l'historique de la session (les favoris sont gardés)
clipboard-history install-shortcut   associer Super+V (GNOME)
clipboard-history remove-shortcut    retirer le raccourci et rendre Super+V aux notifications
```

### Collage automatique

Après la sélection, l'entrée est dans le presse-papiers : **Ctrl+V** la colle.
Pour que le collage soit automatique, installer un outil de simulation clavier :
- X11 : `xdotool`
- Wayland : `ydotool` (nécessite le service `ydotoold` et l'accès à `/dev/uinput`)

### Compatibilité

- X11 : tous les bureaux.
- Wayland : via XWayland (GNOME, KDE). Nécessite que XWayland soit actif (`DISPLAY` défini).

## Désinstallation

```bash
sudo apt remove clipboard-history
```

Le paquet retire le raccourci Super+V (et le rend aux notifications) pour chaque utilisateur qui
l'avait, connecté ou non, et arrête le programme en cours. Seule exception : un utilisateur dont
`XDG_CONFIG_HOME` n'est pas `~/.config` doit lancer `clipboard-history remove-shortcut` avant.

## Développement

```bash
sudo apt install cargo libgtk-4-dev xvfb
cargo build --release                  # target/release/clipboard-history
cargo test                             # tests unitaires + tests X11 (Xvfb)
cargo install cargo-deb && cargo deb   # target/debian/*.deb
```

### Intégration continue

`.github/workflows/ci.yml`, sur Ubuntu 22.04 (le paquet fonctionne ainsi sur toutes les versions
plus récentes) : `cargo fmt --check`, `clippy`, tests (X11 compris, sous Xvfb), paquet `.deb`
(téléchargeable dans les *artifacts* de chaque exécution), puis installation et désinstallation
du paquet dans un conteneur propre (`packaging/test-package.sh`).

Publier une version :

```bash
# 1. mettre à jour `version` dans Cargo.toml, puis :
git commit -am "Version 0.2.0"
git tag v0.2.0 && git push origin main v0.2.0
```

La CI vérifie que le tag correspond à la version de `Cargo.toml`, puis crée la *Release* GitHub avec le `.deb`.

### Tests

Les tests X11 (`tests/x11_watch.rs`) lancent leur propre serveur X sans écran et jouent le rôle
de l'application qui copie : ils ne touchent jamais au presse-papiers de la session.
- sans Xvfb, ils sont ignorés ; `CLIPBOARD_HISTORY_REQUIRE_XVFB=1` les rend obligatoires (CI) ;
- `CLIPBOARD_HISTORY_X_SERVER=Xephyr` les lance dans une petite fenêtre, sans Xvfb.

Organisation :

| Fichier | Rôle | Dépend de GTK |
|---|---|---|
| `src/history.rs` | historique : favoris, ordre, doublons, capacité, taille max | non |
| `src/favorites.rs` | sauvegarde des favoris sur le disque | non |
| `src/preview.rs` | aperçu sur une ligne, recherche | non |
| `src/cli.rs` | commandes | non |
| `src/paste.rs` | choix de l'outil de collage automatique | non |
| `src/x11_watch.rs` | surveillance du presse-papiers X11 (`x11rb`, pur Rust) | non |
| `src/app.rs` | processus d'arrière-plan GTK, relais des commandes | oui |
| `src/picker.rs` | fenêtre de sélection | oui |
| `src/shortcut.rs` | raccourci GNOME | oui |

## Licence

MIT — voir [LICENSE](LICENSE).
