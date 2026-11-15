<div align="center">

<img src="resources/logo.svg" width="112" height="112" alt="Logo d’Argus">

# Argus

**Un gestionnaire de tâches rapide, moderne et riche en informations pour Windows**

[简体中文](README.md) · [繁體中文](README.zh-TW.md) · [English](README.en.md) · [Español](README.es.md) · [Deutsch](README.de.md) · [日本語](README.ja.md) · **Français** · [Русский](README.ru.md)

[![Site web et démo](https://img.shields.io/static/v1?label=%F0%9F%8C%90+Site+web+et+d%C3%A9mo&message=argustask.com&color=2563eb&style=for-the-badge)](https://argustask.com/fr/)

Essayez l’interface interactive et la présentation des fonctions dans le navigateur sur le [site officiel](https://argustask.com/fr/), sans installer l’application.

[![Dernière version](https://img.shields.io/github/v/release/Ray-lea/Argus?include_prereleases=&sort=semver&label=Derni%C3%A8re+version&color=2f80ed)](https://github.com/Ray-lea/Argus/releases)
[![Téléchargements](https://img.shields.io/github/downloads/Ray-lea/Argus/total?label=T%C3%A9l%C3%A9chargements&color=2ea44f)](https://github.com/Ray-lea/Argus/releases)
[![Plateforme](https://img.shields.io/static/v1?label=Plateforme&message=Windows+10+%2F+11+x64&color=0078D4)](#system-requirements)
[![Licence](https://img.shields.io/static/v1?label=Licence&message=AGPL-3.0-or-later&color=A42E2B&logo=gnu&logoColor=white)](LICENSE)

<a href="https://trendshift.io/repositories/310031?utm_source=trendshift-badge&amp;utm_medium=badge&amp;utm_campaign=badge-trendshift-310031" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/trendshift/repositories/310031/daily" alt="Ray-lea/Argus | Trendshift" width="250" height="55"/></a>
<a href="https://trendshift.io/repositories/310031?utm_source=trendshift-badge&amp;utm_medium=badge&amp;utm_campaign=badge-trendshift-310031" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/trendshift/repositories/310031/daily?language=Rust" alt="Ray-lea/Argus | Trendshift Rust" width="250" height="55"/></a>

[Fonctionnalités](#features) · [Téléchargement](#download) · [Compilation depuis les sources](#build-from-source) · [Raccourcis clavier](#keyboard-shortcuts) · [Documentation](#documentation) · [Communauté](#community)

</div>

<br>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/images/screenshot-en-dark.png">
  <img src="docs/images/screenshot-en-light.png" alt="La page Processus d’Argus, avec les applications triées par nom et leur consommation de ressources. Les graphiques affichent la dernière minute d’activité du processeur, de la mémoire, du disque, du réseau et du GPU.">
</picture>

<sub>Aperçu de l’interface en anglais.</sub>

<br>

Argus interroge directement les API natives de Windows en Rust. Son interface repose sur les composants natifs de GPUI Kit, avec des graphiques qui défilent à la fréquence de rafraîchissement de l’écran. Le projet tient son nom d’Argos Panoptès, le géant aux cent yeux de la mythologie grecque.

<a id="star-history"></a>

## ⭐ Historique des étoiles

Si Argus facilite le travail au quotidien, une étoile sur GitHub est une façon directe de soutenir le projet et de le faire découvrir à davantage de personnes.

<a href="https://www.star-history.com/#Ray-lea/Argus&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=Ray-lea/Argus&type=Date&theme=dark">
    <img src="https://api.star-history.com/svg?repos=Ray-lea/Argus&type=Date" alt="Évolution du nombre d’étoiles d’Argus sur GitHub">
  </picture>
</a>

<a id="features"></a>

## ✨ Fonctionnalités

<table>
<tr>
<td width="50%" valign="top"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/features/processes-fr-dark.svg"><img src="docs/images/features/processes-fr-light.svg" width="390" alt="La page Processus passe de l’affichage par application à l’arborescence, puis une recherche de code ne garde que Visual Studio Code et ses processus enfants"></picture><br><b>Processus</b> : affichage par application, arborescence ou liste ; rechercher, trier et terminer une arborescence entière</td>
<td width="50%" valign="top"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/features/performance-fr-dark.svg"><img src="docs/images/features/performance-fr-light.svg" width="390" alt="La page Performances affiche l’utilisation, la vitesse, la température et la puissance du CPU, avec un graphique par cœur P et E"></picture><br><b>Performances</b> : charge par cœur, vitesse, température et puissance du CPU, ainsi que mémoire, disques, réseau et GPU</td>
</tr>
<tr>
<td width="50%" valign="top"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/features/history-fr-dark.svg"><img src="docs/images/features/history-fr-light.svg" width="390" alt="Deux instants sont sélectionnés sur les graphiques sur 7 jours de la page Historique ; les processus les plus chargés de chacun s’affichent dessous"></picture><br><b>Historique</b> : un clic sur un instant des graphiques sur 7 jours affiche les processus les plus chargés à ce moment-là</td>
<td width="50%" valign="top"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/features/files-fr-dark.svg"><img src="docs/images/features/files-fr-light.svg" width="390" alt="Après le dépôt d’un classeur sur la fenêtre, une boîte de dialogue indique qu’Excel et l’Explorateur Windows l’utilisent, et l’un d’eux est terminé"></picture><br><b>Fichiers utilisés</b> : déposer un fichier ou un dossier sur la fenêtre affiche les processus qui l’utilisent</td>
</tr>
<tr>
<td width="50%" valign="top"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/features/rules-fr-dark.svg"><img src="docs/images/features/rules-fr-light.svg" width="390" alt="Un clic droit sur HandBrake règle sa priorité sur Basse, puis active le mode d’efficacité"></picture><br><b>Menu contextuel</b> : définir la priorité, activer le mode d’efficacité ou ajouter une règle appliquée à chaque démarrage</td>
<td width="50%" valign="top"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/features/ports-fr-dark.svg"><img src="docs/images/features/ports-fr-light.svg" width="390" alt="Une recherche de 8080 dans la page Connexions trouve le processus à l’écoute sur ce port et celui qui y est connecté, puis la connexion est fermée"></picture><br><b>Connexions et ports</b> : trouver les connexions et leurs processus par port, et les fermer</td>
</tr>
</table>

<table>
<tr>
<td width="50%" valign="top"><b>Services et applications au démarrage</b><br>Démarrer ou arrêter les services, modifier leur type de démarrage, activer ou désactiver les applications au démarrage</td>
<td width="50%" valign="top"><b>Zone de notification</b><br><kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd> affiche ou masque la fenêtre ; consommation faible lorsqu’elle est masquée</td>
</tr>
<tr>
<td width="50%" valign="top"><b>Remplacement du Gestionnaire des tâches</b><br>Peut <a href="docs/architecture/task-manager.md">remplacer le Gestionnaire des tâches Windows</a> —documentation en chinois— et s’ouvrir avec <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Esc</kbd> ; lance de nouvelles tâches et redémarre l’Explorateur Windows</td>
<td width="50%" valign="top"><b>Aspect et langues</b><br>Arrière-plan Mica, couleur d’accentuation et thème clair ou sombre selon Windows ; 8 langues d’interface</td>
</tr>
<tr>
<td width="50%" valign="top"><b>Mises à jour intégrées</b><br>Canaux Stable et Bêta, téléchargement en arrière-plan et notes de version dans l’application</td>
<td width="50%" valign="top"><b>Autorisations</b><br>Tous les processus sont visibles avec des droits standard ; le service privilégié et le lancement administrateur sont facultatifs</td>
</tr>
</table>

<sub>En raison des contraintes de place et de format d’image d’un README, les démonstrations ci-dessus sont des reproductions simplifiées de l’interface d’Argus, avec des processus et des données d’exemple. Elles sont fournies à titre indicatif ; l’application elle-même fait foi.</sub>

<a id="download"></a>

## 📦 Téléchargement

La dernière version stable et ses sommes de contrôle SHA-256 sont disponibles sur la [page officielle de téléchargement](https://argustask.com/fr/download/). Toutes les versions se trouvent dans [GitHub Releases](https://github.com/Ray-lea/Argus/releases).

| Fichier | Description |
|---|---|
| `Argus_<version>_x64-setup.exe` | **Installateur, recommandé.** Installe pour tous les utilisateurs dans `Program Files`. L’installation et les mises à jour nécessitent une autorisation d’administrateur. Prend en charge les mises à jour intégrées. |
| `Argus_<version>_x64.exe` | **Version portable.** Exécutable autonome qui partage les paramètres et l’historique avec la version installée. Les mises à jour automatiques ne sont pas prises en charge. |
| `SHA256SUMS.txt` | Sommes de contrôle SHA-256 des fichiers de la version. |

[Politique de signature du code](#code-signing-policy) · [Politique de confidentialité en anglais](PRIVACY.md#english)

<a id="system-requirements"></a>

## 💻 Configuration requise

- **Système** : Windows 10 22H2, build 19045, ou version ultérieure, y compris Windows 11 ; architecture x64 uniquement.
- **Graphiques** : périphérique graphique et pilote compatibles avec DirectX 11.

Les versions de Windows vérifiées, les différences fonctionnelles et les autorisations nécessaires sont décrites dans la [documentation de compatibilité](docs/guide/hardware-data.md#系统兼容范围), en chinois.

<a id="build-from-source"></a>

## 🔧 Compilation depuis les sources

Prérequis :

- Chaîne d’outils Rust `stable-x86_64-pc-windows-msvc`, version 1.100.0 ou ultérieure.
- Node.js 26.x à partir de 26.13.1, ou 24.x LTS à partir de 24.22.3, et pnpm 12.18.0.
- Visual Studio Build Tools avec la charge de travail « Développement Desktop en C++ ».

Exécuter dans PowerShell à la racine du dépôt :

```powershell
.\start.bat           # Développement : recompile et redémarre après modification de Rust ou des ressources ; conserve l’instance actuelle en cas d’échec
.\start.bat build     # Compilation de distribution : produit target\release\argus.exe
```

Le [guide de développement](docs/development/README.md), en chinois, détaille les vérifications du code, la création de l’installateur et le nettoyage des caches.

<a id="keyboard-shortcuts"></a>

## 🔑 Raccourcis clavier

| Action | Touches |
|---|---|
| Changer de page | <kbd>Ctrl</kbd>+<kbd>1</kbd> … <kbd>Ctrl</kbd>+<kbd>8</kbd> |
| Rechercher des processus | <kbd>Ctrl</kbd>+<kbd>F</kbd> |
| Suspendre temporairement l’actualisation | Maintenir <kbd>Ctrl</kbd>, puis relâcher pour reprendre, sur les pages Processus et Performances |
| Ouvrir les détails | <kbd>Enter</kbd> |
| Terminer une tâche | <kbd>Alt</kbd>+<kbd>E</kbd> ou <kbd>Delete</kbd> ; <kbd>Shift</kbd>+<kbd>Delete</kbd> ignore la confirmation |
| Afficher ou masquer la fenêtre, globalement | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Esc</kbd> |

La liste complète se trouve dans **Paramètres → Raccourcis clavier → Voir les raccourcis**. Le [guide d’utilisation](docs/guide/README.md), en chinois, fournit d’autres explications.

<a id="documentation"></a>

## 📚 Documentation

La documentation détaillée est en chinois. La [documentation en ligne](https://argustask.com/docs/) propose une édition stable et une édition de développement.

| Document | Contenu |
|---|---|
| [Guide d’utilisation](docs/guide/README.md) | Interface et opérations, zone de notification et raccourcis, paramètres, sources et limites des données matérielles |
| [Guide de développement](docs/development/README.md) | Environnement et commandes, compilation et caches, références et mesures de performances |
| [Architecture](docs/architecture/README.md) | Couches du système, flux de données, technologies et responsabilités des modules |
| [Conception](docs/design/README.md) | Principes de conception, animations des graphiques, apparence et matériaux des fenêtres |
| [Problèmes connus](docs/known-issues.md) | Limites de la version actuelle |
| [Journal des modifications](changelog/fr-FR.md) | Détail des changements de chaque version |

<a id="community"></a>

## 💬 Communauté

[![Groupe QQ 453803497](https://img.shields.io/badge/QQ-453803497-12B7F5?logo=qq&logoColor=white)](https://qm.qq.com/q/97YqJAQRNK) [![Chaîne Telegram @argustask](https://img.shields.io/badge/Telegram-%40argustask-26A5E4?logo=telegram&logoColor=white)](https://t.me/argustask)

Le groupe QQ, de langue chinoise, permet de signaler des problèmes, de proposer des fonctions et d’échanger avec d’autres utilisateurs. La chaîne Telegram publie les nouvelles versions et l’actualité du projet. Les deux sont également accessibles depuis **À propos → Rejoindre le groupe QQ** et **À propos → Rejoindre la chaîne** dans Argus.

<a id="contributing"></a>

## 🤝 Contribuer

Les signalements de problèmes et les pull requests sont les bienvenus. Consulter d’abord le [guide de contribution](.github/CONTRIBUTING.md), en chinois. Les vulnérabilités doivent être signalées en privé selon la [politique de sécurité](.github/SECURITY.md), également en chinois.

<a id="roadmap"></a>

## 🧭 Feuille de route

La [feuille de route](docs/roadmap.md), en chinois, présente les étapes terminées et le programme complet. Les travaux en cours ou prévus comprennent :

- [ ] **Davantage de capteurs matériels** : élargir la prise en charge du matériel grand public à l’aide du pilote officiel PawnIO ; voir le [matériel pris en charge](docs/guide/hardware-data.md), en chinois.
- [ ] **Raccourcis étendus** : distinguer les modificateurs gauches et droits et accepter les combinaisons composées uniquement de modificateurs ou de deux touches ordinaires.

<a id="code-signing-policy"></a>

## 🔐 Politique de signature du code

Le service gratuit de signature du code est fourni par [SignPath.io](https://about.signpath.io/) et le certificat par [SignPath Foundation](https://signpath.org/).

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

La signature de test est intégrée. Les versions officielles sont actuellement distribuées sans signature Windows Authenticode. Lorsque la signature de production sera activée, elle couvrira l’application Argus, les auxiliaires du Gestionnaire des tâches, le désinstallateur et l’installateur compilés depuis ce dépôt par GitHub Actions. Les programmes tiers inclus conserveront leurs signatures d’origine.

- **Maintenance du code et revue** : [@Ray-lea](https://github.com/Ray-lea), responsable de la maintenance du code et de la revue des contributions externes.
- **Approbation** : [@Ray-lea](https://github.com/Ray-lea), responsable de l’approbation manuelle de chaque demande de signature de production.
- **Politique de confidentialité** : [English](PRIVACY.md#english) · [简体中文](PRIVACY.md#简体中文). Elle décrit les vérifications de mises à jour activées par défaut, le recours aux proxys, les téléchargements de PawnIO à la demande, les retours volontaires et le traitement des données par des tiers.

Les mises à jour intégrées utilisent une clé de publication Minisign distincte pour vérifier les installateurs et les numéros de version. Windows Authenticode identifie le signataire d’un exécutable.

<a id="acknowledgements"></a>

## 🙏 Remerciements

Argus s’appuie sur les projets open source suivants :

| Projet | Utilisation |
|---|---|
| [GPUI Kit](https://gpui-kit.com/) | Composants natifs de bureau, fenêtres, tableaux et graphiques |
| [windows-rs](https://github.com/microsoft/windows-rs) | Liaisons Rust vers les API natives de Windows |
| [Biome](https://biomejs.dev/), [NSIS](https://nsis.sourceforge.io/) | Vérifications de développement et création des paquets Windows |
| [serde](https://serde.rs/), [reqwest](https://github.com/seanmonstar/reqwest) et d’autres crates Rust | Sérialisation, requêtes réseau et fonctions de base |
| [PawnIO](https://pawnio.eu/), [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) | Pilote signé et modules de collecte des capteurs facultatifs du noyau |
| [ZenStates-Core](https://github.com/irusanov/ZenStates-Core), [LibreHardwareMonitor](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor), [GNR-SMU](https://github.com/Kyworn/gnr-smu) | Références des registres CPU, notamment les tables AMD SMU |
| [Intel IGCL](https://github.com/intel/drivers.gpu.control-library), [AMD ADL](https://github.com/GPUOpen-LibrariesAndSDKs/display-library) | Interfaces de télémétrie GPU |

La liste complète des composants tiers et de leurs licences figure dans [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

Merci à la communauté [LINUX DO](https://linux.do/) pour son soutien.

<!-- supporters:start · node scripts/supporters.mjs -->
<p align="center">
  <a href="https://github.com/bronyaicu" target="_blank" rel="noopener"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/images/supporters/bronyaicu-fr-dark.svg"><img src="docs/images/supporters/bronyaicu-fr-light.svg" width="720" alt="Soutien au projet &amp; remerciements particuliers: bronyaicu"></picture></a>
</p>
<!-- supporters:end -->

<a id="license"></a>

## 📄 Licence

Argus est un logiciel libre distribué sous la [GNU Affero General Public License](LICENSE), version 3 ou, au choix, toute version ultérieure (`AGPL-3.0-or-later`).

<a href="https://next.ossinsight.io/widgets/official/compose-recent-active-contributors?repo_id=1415551131&limit=30" target="_blank" style="display: block" align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://next.ossinsight.io/widgets/official/compose-recent-active-contributors/thumbnail.png?repo_id=1415551131&limit=30&image_size=auto&color_scheme=dark" width="655" height="auto">
    <img alt="Active Contributors of Ray-lea/Argus - Last 28 days" src="https://next.ossinsight.io/widgets/official/compose-recent-active-contributors/thumbnail.png?repo_id=1415551131&limit=30&image_size=auto&color_scheme=light" width="655" height="auto">
  </picture>
</a>

<!-- Made with [OSS Insight](https://ossinsight.io/) -->

<div align="center">
<br>
<sub>Argus · Gestionnaire de tâches pour Windows · © 2026 Argus contributors</sub>
</div>
