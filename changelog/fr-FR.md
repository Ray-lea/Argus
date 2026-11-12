# Journal des modifications

[简体中文](zh-CN.md) · [繁體中文](zh-TW.md) · [English](en-US.md) · [Español](es-ES.md) · [Deutsch](de-DE.md) · [日本語](ja-JP.md) · **Français** · [Русский](ru-RU.md)

Modifications visibles par les utilisateurs dans chaque version d’Argus. Les numéros de version suivent la [gestion sémantique de version](https://semver.org/lang/fr/). Ce fichier est traduit du [journal des modifications en chinois simplifié](zh-CN.md), auquel il correspond entrée par entrée.

## [0.5.0](https://github.com/Ray-lea/Argus/compare/v0.4.0...v0.5.0) - 2026-11-12

v0.5.0 ajoute la suspension et la reprise des processus, le regroupement des processus par utilisateur, quatre colonnes d’informations sur les processus et des largeurs de colonnes calculées selon le contenu. La liste des périphériques de la page Performances permet de masquer et de réordonner les périphériques. L’interface est désormais disponible en chinois traditionnel, espagnol, allemand, japonais, français et russe. Cette version évite aussi le blocage de certains systèmes Intel RST après le démarrage, corrige notamment l’impossibilité d’éjecter en toute sécurité les disques USB et réduit l’utilisation de la mémoire et du CPU lorsque la fenêtre est masquée.

### Nouvelles fonctionnalités

- **Suspendre et reprendre des processus**: Le menu contextuel des processus et des groupes d’applications propose désormais « Suspendre le processus » et « Reprendre le processus ». Avant la suspension, Argus explique les conséquences et demande une confirmation. Les processus système critiques et Argus lui-même ne peuvent pas être suspendus. Un processus suspendu affiche l’indicateur « Suspendu » après son nom. ([@digital-bombyx](https://github.com/digital-bombyx) [#65](https://github.com/Ray-lea/Argus/pull/65))
- **Colonnes d’informations sur les processus**: Quatre colonnes facultatives sont ajoutées : « Version du fichier », « Architecture », « Élevé » et « Ligne de commande ». Elles sont masquées par défaut. Vous pouvez les activer dans le menu contextuel des en-têtes de colonnes ou dans « Choisir les colonnes ». ([@digital-bombyx](https://github.com/digital-bombyx) [#72](https://github.com/Ray-lea/Argus/pull/72))
- **Largeur des colonnes de processus**: La largeur automatique des colonnes est calculée selon le contenu. L’espace libre reste à droite du tableau. Faites glisser la bordure d’une colonne pour régler la largeur de cette seule colonne. Double-cliquez sur la bordure pour rétablir la largeur automatique. Le menu des en-têtes de colonnes ajoute « Pleine largeur ». Lorsque cette option est activée, la colonne « Nom » occupe la largeur restante.
- **Grouper les processus par utilisateur**: « Paramètres → Langue et apparence → Grouper les processus par utilisateur » propose trois modes : « Automatique », « Toujours grouper » et « Ne jamais grouper ». Le mode automatique groupe les processus uniquement lorsque plusieurs utilisateurs sont connectés. Le menu contextuel des lignes de groupe propose « Gérer les comptes d’utilisateurs » et « Fermer la session ».
- **Masquer les processus système**: Les pages Processus et Connexions ajoutent le commutateur « Masquer les processus système ». Il est désactivé par défaut. Chaque page enregistre son propre état. Les listes de programmes « Toujours afficher » et « Masquer également » sont modifiables sur la page Paramètres. ([#32](https://github.com/Ray-lea/Argus/issues/32))
- **Arborescence des processus et vue d’ensemble des ressources**: L’arborescence des processus est réduite par défaut. Le menu contextuel des en-têtes de colonnes propose « Tout développer » et « Tout réduire ». « Vue d’ensemble des ressources » accepte les modes « Automatique », « Toujours afficher » et « Masquée ». En mode automatique, la vue d’ensemble se replie lorsque la hauteur de la fenêtre est insuffisante.
- **Liste des périphériques de la page Performances**: Faites un clic droit sur un périphérique pour le masquer. Vous pouvez aussi choisir les périphériques un par un dans le sous-menu « Périphériques affichés ». Faites glisser un périphérique pour modifier l’ordre. Les disques peuvent être triés par numéro de disque, par lettre de lecteur ou selon un ordre personnalisé.
- **Vue multicœur du CPU**: Le graphique de chaque processeur logique affiche le temps du noyau. Vous pouvez désactiver cet affichage dans le menu contextuel du graphique CPU. Argus identifie les cœurs d’efficacité basse consommation d’Intel (cœurs LP E). Il les affiche dans une couleur distincte et permet de les filtrer séparément. Les graphiques sont disposés de préférence en lignes et en colonnes sans emplacement vide.
- **Informations matérielles**: La page GPU affiche la version du pilote, la date du pilote et le niveau de fonctionnalité DirectX. Les détails du disque indiquent la durée de vie restante et le volume total lu et écrit pour davantage de SSD SATA. Des statistiques de fonctionnement et de secteurs sont ajoutées pour les disques durs mécaniques. Les colonnes du tableau des attributs SMART s’adaptent à la largeur du contenu.
- **Langues de l’interface**: Les langues suivantes sont ajoutées : chinois traditionnel, espagnol, allemand, japonais, français et russe. L’interface, la zone de notification, les messages système et le programme d’installation sont traduits. Les nombres et les dates utilisent le format régional de Windows. ([@abc0922001](https://github.com/abc0922001) [#45](https://github.com/Ray-lea/Argus/pull/45), [@Rimuwu](https://github.com/Rimuwu) [#51](https://github.com/Ray-lea/Argus/pull/51), [@s0ftw4r8](https://github.com/s0ftw4r8) [#52](https://github.com/Ray-lea/Argus/pull/52), [@khativari](https://github.com/khativari) [#55](https://github.com/Ray-lea/Argus/pull/55), [@hayxe1](https://github.com/hayxe1) [#59](https://github.com/Ray-lea/Argus/pull/59) [#62](https://github.com/Ray-lea/Argus/pull/62))
- **Nouveautés**: La page « À propos » et la boîte de dialogue de mise à jour affichent les nouveautés dans la langue de l’interface. Dans « Paramètres → Mises à jour », le bouton « Voir les nouveautés » ouvre désormais la boîte de dialogue de mise à jour. La boîte de dialogue propose « Tout développer » et « Tout réduire ».
- **Icônes et animations**: Les icônes de l’interface proviennent désormais de la bibliothèque Lucide. Lorsque le pointeur passe sur un bouton ou un élément de navigation, son icône joue une courte animation une seule fois. L’animation n’est pas jouée lorsque l’option « Animations de l’interface » est désactivée.
- **Fenêtre et zone de notification**: Un commutateur distinct « Transitions de fenêtres » est ajouté. « Réouverture depuis la zone de notification » peut être réglé sur « Ouvrir la page par défaut ». La taille par défaut de la fenêtre passe à 1000×680. La taille minimale descend à 640×480. ([#27](https://github.com/Ray-lea/Argus/issues/27), [#40](https://github.com/Ray-lea/Argus/issues/40))
- **Remplacement du Gestionnaire des tâches**: « Remplacer le Gestionnaire des tâches » ajoute le bouton « Ouvrir temporairement le Gestionnaire des tâches Windows ». Ce bouton ouvre le Gestionnaire des tâches intégré à Windows. Les paramètres de remplacement ne changent pas.
- **Avis et rappels**: La version portable présente ses différences avec la version installée dans le guide de démarrage et dans un avis en bas à droite. Elle propose aussi des liens pour télécharger la version installée. Un « Rappel d’étoile » est ajouté. Il s’affiche au maximum une fois par jour. Vous pouvez le désactiver dans « Paramètres → Mises à jour ». Les astuces au-dessus de « Ce PC » peuvent être réduites.

### Performances

- **Utilisation de la mémoire en arrière-plan**: Le paramètre « Libérer la mémoire lorsque la fenêtre est masquée » est ajouté. Lorsque la fenêtre reste réduite ou masquée dans la zone de notification pendant la durée définie (10 minutes par défaut), Argus libère les images des graphiques et rend la mémoire inutilisée à Windows. Lorsque toutes les fenêtres sont masquées, l’horloge d’affichage est suspendue. ([@SpookySandwich](https://github.com/SpookySandwich) [#54](https://github.com/Ray-lea/Argus/pull/54))

### Corrections

- **Blocage du système**: Argus ne lit plus la température des disques connectés à un contrôleur RAID, comme Intel RST. Certaines versions du pilote ne répondaient pas à cette requête, et le système se bloquait quelques secondes après le démarrage d’Argus. La température de ces disques s’affiche comme inconnue. ([#48](https://github.com/Ray-lea/Argus/issues/48))
- **Éjection sécurisée des disques USB**: La surveillance des disques n’occupe plus les périphériques en permanence. Les disques USB peuvent être éjectés en toute sécurité. ([#57](https://github.com/Ray-lea/Argus/issues/57))
- **Liste des disques et données de performances**: Certains disques NVMe affichaient uniquement les informations du périphérique, sans données de performances. Les disques disparaissaient aussi de la liste lorsque les compteurs de performances n’étaient pas disponibles. Ces deux problèmes sont corrigés.
- **Données des processus**: Lorsque les requêtes système sont lentes, les valeurs des processus continuent d’être actualisées et ne manquent plus en grand nombre. Les processus terminés ne restent plus dans la liste. Lorsque l’actualisation est suspendue, une action sur un processus met désormais à jour son état. ([#66](https://github.com/Ray-lea/Argus/issues/66))
- **Page Connexions**: Les connexions sans processus associé ne s’affichent plus comme « System Idle Process ».
- **Mesures du CPU**: Les processeurs à architecture hybride affichent désormais la bonne vitesse. Certains processeurs Ryzen à six cœurs affichent désormais la bonne tension et la bonne température de chaque cœur. Les mesures des capteurs internes Intel ne s’interrompent plus, et la courbe de température n’est plus coupée. ([#58](https://github.com/Ray-lea/Argus/issues/58))
- **Autorisation et désinstallation**: Argus peut révoquer l’autorisation et effectuer la désinstallation après une modification des droits d’accès de Program Files ou du dossier d’exécution. Les capteurs du noyau peuvent utiliser un pilote PawnIO déjà en cours d’exécution. La désinstallation ne laisse plus le dossier Argus Sensor Setup ni un dossier Argus Runtime vide. ([#46](https://github.com/Ray-lea/Argus/issues/46))
- **Droits au démarrage**: Après l’installation ou une mise à jour depuis l’application, Argus s’exécute avec les droits standard du compte du bureau, comme lors d’un lancement depuis un raccourci. Lorsqu’Argus s’exécute en tant qu’administrateur, il peut redémarrer un Explorateur de fichiers exécuté avec des droits élevés.
- **Affichage de la fenêtre**: Après la restauration de la taille de la fenêtre, l’interface ne s’affiche plus uniquement dans le coin supérieur gauche. Après un démarrage à froid depuis le remplacement du Gestionnaire des tâches, la fenêtre agrandie s’affiche entièrement. Sous Windows 11 21H2, l’arrière-plan de la fenêtre n’est plus transparent. Après un passage à une autre application, la fenêtre ne reste plus au-dessus des autres fenêtres. ([#44](https://github.com/Ray-lea/Argus/issues/44), [#37](https://github.com/Ray-lea/Argus/issues/37), [#35](https://github.com/Ray-lea/Argus/issues/35))
- **Zone de notification**: Lorsque la fenêtre est fermée et masquée dans la zone de notification, l’utilisation du CPU ne reste plus élevée. Après une mise à jour depuis l’application, l’icône de notification ne passe plus dans la zone de dépassement. ([#31](https://github.com/Ray-lea/Argus/issues/31))
- **Affichage de l’interface**: Les nombres dynamiques ne se décalent plus horizontalement lors de leur actualisation. La boîte de dialogue de mise à jour reste centrée pendant son chargement. La boîte de dialogue des fichiers utilisés ne tremble plus à l’affichage des résultats. Le menu contextuel des zones de texte ne s’affiche plus toujours en anglais. ([#39](https://github.com/Ray-lea/Argus/issues/39))
- **Stabilité**: Argus ne se ferme plus lorsque le minutage de l’affichage est anormal ou lorsque l’énumération des adresses des cartes réseau échoue plusieurs fois de suite. L’interface ne se fige plus lorsque la récupération du périphérique graphique coïncide avec la fermeture de la fenêtre.
- **Grandes polices, règles de processus et tâches de démarrage**: Avec une grande taille de police, les mesures de la page Performances ne chevauchent plus les spécifications. Lorsque le nom d’un programme contient des lettres non ASCII, le menu des processus n’ajoute plus de règle en double. Argus ne se ferme plus de manière inattendue lors de la lecture des tâches de démarrage des applications empaquetées. ([@hayxe1](https://github.com/hayxe1) [#59](https://github.com/Ray-lea/Argus/pull/59))

### Remarques sur la mise à jour

- **Service privilégié**: Cette version fait passer le protocole de communication du service à la version 11. Une mise à niveau de la version installée par le programme d’installation ou depuis l’application met aussi à jour le service installé et la copie de remplacement du Gestionnaire des tâches. Pour la version portable, après le remplacement de l’exécutable, cliquez sur « Mettre le service à jour » dans « Paramètres → Autorisations → Service privilégié », puis validez la demande administrateur.
- **Paramètres et historique**: La mise à jour conserve les paramètres et l’historique existants. Au premier démarrage, « Transitions de fenêtres » reprend le réglage des transitions de fenêtre défini auparavant dans « Animations de l’interface ».

## [0.4.0](https://github.com/Ray-lea/Argus/compare/v0.3.4...v0.4.0) - 2026-11-06

v0.4.0 migre entièrement l’interface de Tauri, SolidJS et WebView2 vers une architecture native Rust + GPUI Kit. Elle supprime la surcharge du moteur de navigateur, reconstruit le rendu natif DirectX et les interactions, et optimise en profondeur les animations à haute fréquence, les courbes en temps réel, l’actualisation des échantillons des listes et les ressources consommées en arrière-plan. Cette version ajoute aussi des colonnes de processus personnalisables, l’état de santé des disques et les détails SMART, davantage de mesures des GPU intégrés Intel et des options de disposition de la fenêtre. Le [site officiel d’Argus](https://argustask.com/fr/) est désormais en ligne et relié au miroir officiel des mises à jour.

### Nouvelles fonctionnalités

- **Colonnes de processus personnalisables**: 40 colonnes réparties en six catégories : informations générales, CPU, mémoire, disque et I/O, réseau, GPU. Les ajouts comprennent la mémoire GPU dédiée et partagée, le moteur GPU, le nom de l’image, l’espace d’adressage virtuel, la plage de travail maximale, les défauts de page et les compteurs I/O cumulés. L’espace d’adressage virtuel et la plage de travail maximale sont affichés par processus. Le menu contextuel des en-têtes propose des sous-menus par catégorie avec sélection successive ; « Choisir les colonnes » permet de filtrer par catégorie, rechercher et réordonner dans deux volets. Le déplacement d’un en-tête ou d’une colonne sélectionnée réordonne immédiatement les colonnes, avec un décalage fluide des voisines. La largeur automatique réserve de la place selon le format numérique pour éviter les sauts ; un réglage manuel reste possible. La colonne « Nom » des vues en liste et en arborescence suit la vue par application : elle privilégie le nom de l’application ou la description du fichier. Le nom de l’exécutable apparaît en texte secondaire ou dans « Nom de l’image ».
- **Santé et détails des disques**: La page de performances du disque ajoute « Vue d’ensemble / Détails ». Une lecture ponctuelle à la demande des périphériques NVMe, ATA/SATA, USB-SAT et SCSI/SAS fournit l’état de santé, la durée de vie restante, la température et les seuils, la durée et le nombre de mises sous tension, les volumes totaux lus et écrits, les erreurs de support et de lecture/écriture, la liaison PCIe et les attributs SMART. La lecture peut être relancée manuellement. Le numéro de série privilégie celui du fabricant fourni par l’identification du protocole et reste masqué par défaut.
- **Vue d’ensemble du système et graphiques de performances**: Le bandeau supérieur de la page Processus a été repensé. Les courbes remplissent les cartes jusqu’aux bords et s’estompent sous les chiffres à gauche ; la limite d’échelle figure en haut et toute la carte est mise en évidence au survol. La température du CPU et les puissances CPU et GPU ont été ajoutées. La vue d’ensemble du CPU dans Performances regroupe la puissance du package et des cœurs sous « Puissance » (priorité au package, source au survol, deux courbes conservées dans Détails). Les échelles des graphiques de disque et de réseau changent par paliers fixes. Les séries secondaires des petits graphiques, dont les aperçus de périphériques dans la barre latérale, utilisent des traits continus semi-transparents pour améliorer la lisibilité.
- **Mesures des GPU intégrés Intel**: Les températures, fréquences, tensions et puissances fournies par le pilote sont enrichies. Les capteurs du noyau (module IntelMCHBAR) et RAPL PP1 complètent, pour les GPU intégrés des Core de 10e génération (Comet Lake S/H), la température, la fréquence, la fréquence de mémoire partagée, le VID du cœur, l’activité GT/média et la puissance. L’arrêt d’horloge des tranches graphiques est distingué ; les puissances inférieures à 1 W sont affichées et la puissance du GPU intégré figure sur sa page GPU. Les mesures RAPL valides priment sur les zéros des compteurs de puissance Windows. Les valeurs non fournies par le matériel ou le pilote restent inconnues.
- **Fenêtre et disposition de l’interface**: « Paramètres → Démarrage et zone de notification » permet de choisir la page au lancement. Le bord droit de la navigation latérale déployée permet de régler sa largeur par glissement ; un double-clic rétablit la valeur par défaut. Le volet de détails ajoute « Afficher lorsqu’un processus est sélectionné » : il apparaît en glissant à la sélection et se replie lorsque celle-ci est effacée. Un changement de plage dans Historique affiche toujours la période la plus récente jusqu’au présent. La taille minimale passe à 1240×800 pour préserver la disposition complète, avec réduction automatique si la zone de travail de l’écran principal est plus petite.
- **À propos, modifications et miroir officiel**: « À propos » devient une page de navigation indépendante (Ctrl+8), avec des cartes de liens vers le site d’Argus et le dépôt GitHub. Un badge de version à gauche de la barre d’état ouvre les modifications de la version au clic et permet de copier son numéro par clic droit. Les modifications sont regroupées par catégorie, les nouveautés présentées dans des cartes sur deux colonnes. Si GitHub est inaccessible, les mises à jour automatiques essaient d’abord le miroir officiel d’Argus, puis les proxys GitHub. La signature de l’installateur comme celle des notes de mise à jour est vérifiée.

### Performances

- **Réactivité et animations à haute fréquence**: Réduction du travail du fil principal consacré aux mises à jour des échantillons, à la mesure du texte et aux dessins répétés. Réutilisation des fonds statiques et des couches de composition, dessin parallèle des grandes textures de courbes sur plusieurs fils. La charge GPU des graphiques diminue fortement lorsque la planification GPU à accélération matérielle (HAGS) est désactivée. Les transitions de pages, le volet de détails, le défilement des listes et le réordonnancement des colonnes suivent la fréquence de l’écran. Les grilles multicœurs et les graphiques des moteurs GPU se redimensionnent dans la même image que la fenêtre.
- **Changement de page et fonctionnement en arrière-plan**: Les listes de services, de démarrage et de connexions sont préchargées après le lancement et immédiatement disponibles à la première ouverture. Les préférences d’interface sont regroupées et enregistrées de manière asynchrone en arrière-plan, supprimant les pauses au changement de page. Les pages sans graphique en temps réel évitent les reconstructions répétées pour réduire la charge CPU. Le dessin s’arrête lorsque la fenêtre est réduite ou masquée dans la zone de notification ; les caches de couches de graphiques, icônes périmées et atlas de textures sont libérés au besoin pour réduire la mémoire et la mémoire GPU en arrière-plan.
- **Forte charge et manque de mémoire**: Un fil dédié regroupe les mises à jour de la zone de notification et améliore la réactivité des opérations et de l’échantillonnage sous forte charge ou lorsque le shell est bloqué. En cas de manque de mémoire, les tampons des graphiques et la précision des textures sont réduits automatiquement pour préserver les courbes en temps réel ; la pleine précision revient lorsque la mémoire disponible se rétablit. L’activation de la fenêtre gère mieux les grandes fenêtres superposées toujours au premier plan.

### Corrections

- **Trafic réseau par processus**: Correction des mesures restant à zéro sur certaines machines et des valeurs alternant avec zéro ou actualisées en retard avec un intervalle de 0,5 s.
- **Service privilégié**: Correction de l’arrêt du service lorsqu’un client se déconnecte anormalement avant la fin de la connexion au canal, ainsi que des délais expirés et des faux résultats sans données lors de lectures fréquentes.
- **Noms et icônes d’applications**: Correction des applications empaquetées (MSIX) affichant la description de l’exécutable au lieu du nom utilisé par le Gestionnaire des tâches. Correction également des icônes et descriptions absentes en cas de correspondance de chemin de périphérique manquante ou de refus d’ouverture du processus.
- **Mémoire GPU par processus**: Utilisation de compteurs de mémoire résidente, dédupliqués par processus et adaptateur physique, pour corriger les valeurs excessives ou comptées plusieurs fois sur certains systèmes ou pilotes. Les mesures absentes sont distinguées d’une consommation nulle.
- **Puissance des GPU intégrés Intel**: Correction de la puissance du GPU intégré inactif apparaissant et disparaissant sans cesse dans la vue d’ensemble du CPU.
- **État de virtualisation du CPU**: Correction des hôtes Hyper-V affichant encore « Désactivé » alors que la virtualisation matérielle est active.
- **Autorisation du lancement administrateur**: Correction de l’échec de résolution du SID lorsque le nom de l’utilisateur est identique à celui de l’ordinateur, qui empêchait l’autorisation. Amélioration de la reconnaissance des copies recompilées d’une même version.
- **Mémoire sur de longues sessions**: Déduplication des icônes par contenu, nettoyage périodique des métadonnées des programmes terminés et limitation des requêtes en attente lorsque l’analyseur est occupé. Ces changements corrigent la croissance continue de mémoire lors de lancements fréquents depuis de nouveaux chemins.
- **Liens externes**: Les liens vers le site, le dépôt et les pages de publication sont confiés directement au shell du système, supprimant le délai dû à un nouveau processus intermédiaire de l’Explorateur.

### Remarques sur la mise à jour

- **Configuration requise**: Windows 10 22H2 (build 19045) ou ultérieur, y compris Windows 11, x64 uniquement ; périphérique graphique et pilote compatibles avec DirectX 11. L’interface ne dépend plus du runtime WebView2.
- **Paramètres et historique**: Les paramètres et fichiers d’historique sont conservés. Depuis v0.3.3 ou v0.3.4, les préférences d’interface enregistrées séparément sont reprises. Une mise à jour directe depuis v0.3.2 ou antérieur rétablit les valeurs par défaut des préférences présentes uniquement dans l’ancien cache WebView2 (disposition, notes de lignes et éléments épinglés) ; elles doivent être reconfigurées. La nouvelle version nettoie automatiquement les anciennes données WebView2 d’Argus en arrière-plan.
- **Service privilégié**: Le protocole du service passe à la version 10. Une mise à jour de la version installée par l’installateur ou depuis l’application actualise aussi le service installé et la copie remplaçant le Gestionnaire des tâches. Après remplacement de l’exécutable portable, il faut sélectionner « Mettre le service à jour » dans « Paramètres → Autorisations → Service privilégié » et valider la demande administrateur.

## [0.3.4](https://github.com/Ray-lea/Argus/compare/v0.3.3...v0.3.4) - 2026-10-26

### Corrections

- **Redémarrage de l’Explorateur**: Correction de l’absence de restauration du bureau et de la barre des tâches lorsque l’ancien processus reste bloqué pendant le nettoyage de fin ou le rapport d’erreur. Si l’arrêt normal expire, seule l’ancienne instance identifiée est terminée ; le bureau est restauré après confirmation de son arrêt. Si Windows a déjà restauré le bureau, l’instance existante est réutilisée pour éviter un lancement en double. ([#20](https://github.com/Ray-lea/Argus/issues/20))

### Remarques sur la mise à jour

- La mise à jour depuis v0.3.x conserve les paramètres et l’historique existants.

## [0.3.3](https://github.com/Ray-lea/Argus/compare/v0.3.2...v0.3.3) - 2026-10-26

Cette version ajoute le réglage de l’échelle de l’interface et de la taille du texte, améliore la sauvegarde des préférences de fenêtre et de listes, et ajuste les statistiques GPU.

### Nouvelles fonctionnalités

- **Échelle de l’interface**: Réglage de 80 % à 150 % dans « Paramètres → Langue et apparence », avec 100 % par défaut et prise en charge de pourcentages entiers comme 113 %.
- **Taille du texte**: La taille de base peut varier de 11 à 20 px, avec 13 px par défaut. Elle se combine à l’échelle de l’interface, s’applique immédiatement et s’enregistre automatiquement. Les deux réglages peuvent être réinitialisés séparément.

### Corrections

- **Statistiques GPU**: Tentative de correction d’une utilisation globale trop faible sous certaines charges de calcul, des pics anormaux à la reprise de l’échantillonnage et des totaux excessifs avec plusieurs GPU ou moteurs. Ajout des compteurs de moteurs par processus et de l’utilisation du cœur NVIDIA lorsqu’elle est disponible, avec affichage de la source. ([#19](https://github.com/Ray-lea/Argus/issues/19))
- **Restauration de la fenêtre**: Taille, position et état agrandi sont restaurés après fermeture et relancement. Une fermeture à l’état réduit ne laisse plus la fenêtre réduite au prochain démarrage ; une fenêtre hors écran revient dans la zone visible disponible.
- **Préférences des listes**: Processus, services, démarrage et connexions conservent chacun la colonne de tri et son sens. Services et Connexions restaurent également les filtres sélectionnés.
- **Enregistrement des préférences**: Les préférences d’interface sont enregistrées séparément, avec migration depuis l’ancien cache. Elles restent récupérables après nettoyage du cache de l’interface.
- **Disposition et boutons de fenêtre**: Les barres d’outils et la navigation s’adaptent aux fenêtres étroites avec un texte ou une échelle élevés. La zone de réponse du bouton d’agrandissement a été corrigée.

### Remarques sur la mise à jour

- La mise à jour depuis v0.3.x conserve les paramètres et l’historique. Les anciennes préférences d’interface migrent automatiquement au premier lancement, sans reconfiguration manuelle.

## [0.3.2](https://github.com/Ray-lea/Argus/compare/v0.3.1...v0.3.2) - 2026-10-23

Cette version remplace v0.3.1, qui a été retirée. Elle corrige la détection erronée de la version installée comme portable après un lancement administrateur ou via le Gestionnaire des tâches, qui empêchait les mises à jour intégrées.

### Remarques sur la mise à jour

- Si une copie installée affiche « La version portable ne se met pas à jour automatiquement », il suffit de télécharger et d’exécuter directement l’installateur depuis la page de cette version, sans désinstaller Argus au préalable. Après mise à jour, le lancement administrateur et les points d’entrée du Gestionnaire des tâches conservent les mises à jour intégrées.

### Corrections

- **Mise à jour de la version installée**: Reconnaissance correcte des copies de lancement administrateur et de remplacement du Gestionnaire des tâches, rétablissant la recherche, le téléchargement et l’installation automatiques des mises à jour.
- **Détection de la version portable**: La présence d’un désinstalleur à côté du programme ne la fait plus identifier comme installée. Les mises à jour restent à télécharger manuellement depuis la page de publication.

### Changements inclus

- Inclut le lancement administrateur après une autorisation unique de v0.3.1, ainsi que les corrections du redémarrage de l’Explorateur, des statistiques CPU, des courbes après restauration de la fenêtre, du curseur de la liste des processus et de l’installation des mises à jour.

## [0.3.1](https://github.com/Ray-lea/Argus/compare/v0.3.0...v0.3.1) - 2026-10-23

**Cette version a été retirée. Ses fonctionnalités et corrections sont incluses dans v0.3.2.**

Ajout du lancement administrateur après une autorisation unique, avec des droits uniformes à l’ouverture de session, depuis les raccourcis et les points d’entrée du Gestionnaire des tâches. Correction de problèmes de redémarrage de l’Explorateur, d’utilisation du CPU et d’installation des mises à jour.

### Remarques sur la mise à jour

- **Exécution en tant qu’administrateur**: Désactivée par défaut, activable dans « Paramètres → Autorisations » ou dans le guide de démarrage. Réservée aux comptes administrateurs. Argus redémarre après activation en conservant paramètres et historique. Le service privilégié, les capteurs du noyau et le remplacement du Gestionnaire des tâches conservent des autorisations distinctes.
- **Version installée**: Une mise à jour par l’installateur ou depuis l’application actualise également le service privilégié installé et la copie remplaçant le Gestionnaire des tâches.
- **Version portable**: Fermer d’abord l’ancienne version, puis remplacer l’exécutable. Si le service privilégié est installé, sélectionner « Mettre le service à jour » dans « Paramètres → Autorisations » de la nouvelle version. Avant les mises à jour manuelles suivantes, le lancement administrateur peut être désactivé avant de quitter Argus ; après remplacement, lancer le nouvel exécutable et réactiver l’option.

### Nouvelles fonctionnalités

- **Lancement administrateur**: Après une autorisation unique, Argus s’ouvre avec les droits administrateur à l’ouverture de session, depuis les raccourcis et les icônes épinglées. Avec le remplacement du Gestionnaire des tâches activé, Ctrl+Shift+Esc et le menu contextuel de la barre des tâches fonctionnent également. Les paramètres et le guide affichent l’état de l’autorisation et les droits actuels, avec désactivation et réparation de l’autorisation. ([#13](https://github.com/Ray-lea/Argus/issues/13))

### Corrections

- **Explorateur**: Correction des échecs de redémarrage avec droits standard ou administrateur, des anomalies du bureau après redémarrage et des droits insuffisants lors d’un nouveau redémarrage. La liste des fichiers utilisés propose « Redémarrer » pour l’Explorateur du bureau actuel, évitant la disparition du bureau et de la barre des tâches causée par une fermeture directe. ([#14](https://github.com/Ray-lea/Argus/issues/14), [#15](https://github.com/Ray-lea/Argus/issues/15))
- **Statistiques CPU**: Correction de l’utilisation restant à 100 % lorsque les compteurs d’inactivité cessent de se mettre à jour sur certains systèmes. ([#12](https://github.com/Ray-lea/Argus/issues/12))
- **Courbes**: Correction des données accumulées formant des pics serrés à la restauration depuis l’état réduit ou la zone de notification.
- **Liste des processus**: Suppression du curseur de texte clignotant après un clic dans la liste, tout en conservant la recherche par saisie directe et les méthodes de saisie chinoises. ([#16](https://github.com/Ray-lea/Argus/issues/16))
- **Installation des mises à jour**: Affichage des processus utilisant l’exécutable et possibilité de réessayer, ajout des étapes en échec et des codes d’erreur. Correction de la lecture impossible des journaux de diagnostic après une autorisation avec un autre compte administrateur.

### Problèmes connus

- **Signature de code**: Les exécutables publiés n’ont pas encore de signature de code Windows ; Windows SmartScreen peut signaler un « éditeur inconnu » au premier lancement. Les mises à jour intégrées restent vérifiées par leur signature de mise à jour.

## [0.3.0](https://github.com/Ray-lea/Argus/compare/v0.2.0...v0.3.0) - 2026-10-22

Ajout de l’exécution de nouvelles tâches, du redémarrage de l’Explorateur et de la copie des caractéristiques du PC. Les groupes de processus, les vues de performances et la compatibilité avec Windows 10 sont améliorés.

### Changements incompatibles

- **Historique**: Après mise à jour, la page Historique commence de nouveaux enregistrements et remplace l’ancien fichier du jour. Pour conserver les anciens enregistrements, une [sauvegarde du dossier d’historique](https://github.com/Ray-lea/Argus/blob/main/docs/guide/README.md#历史) est nécessaire au préalable.

### Remarques sur la mise à jour

- **Version installée**: Une mise à jour par l’installateur ou depuis l’application actualise aussi le service privilégié et la copie remplaçant le Gestionnaire des tâches.
- **Version portable**: Fermer l’ancienne version avant de remplacer l’exécutable. Si le service privilégié est installé, sélectionner « Mettre le service à jour » dans « Paramètres → Autorisations » et autoriser l’opération.

### Nouvelles fonctionnalités

- **Exécuter une nouvelle tâche**: Un nouvel accès dans la barre d’outils permet d’ouvrir un programme, un dossier, un document ou une adresse web, avec prise en charge des droits administrateur.
- **Redémarrer l’Explorateur**: Le menu contextuel de l’Explorateur Windows dans Processus propose « Redémarrer ».
- **Raccourcis clavier**: Maintenir Ctrl suspend l’actualisation ; relâcher la touche la reprend. Alt+E termine le processus sélectionné dans Processus. Les paramètres donnent accès à la liste des raccourcis.
- **Groupes de processus**: La vue par application est divisée en Applications, Processus en arrière-plan et Processus Windows, avec des en-têtes repliables indiquant le total des ressources utilisées.
- **Recherche rapide**: La saisie directe dans Processus lance une recherche, avec prise en charge des méthodes de saisie chinoises. Un filtre actif est signalé par la couleur d’accentuation.
- **Caractéristiques du PC**: Le volet de détails reste ouvert par défaut et sa largeur est réglable. Sans processus sélectionné, il affiche la configuration matérielle, qui peut être masquée ou copiée en texte.
- **Page CPU**: Séparation en Vue d’ensemble et Détails. Les graphiques des cœurs se réorganisent avec des transitions fluides selon la taille de la fenêtre.
- **Page GPU**: Les moteurs actifs sont affichés automatiquement. Le choix des moteurs, la disposition des graphiques et leur hauteur sont également réglables manuellement.
- **Composition de la mémoire**: Ajout de la mémoire modifiée. Le survol des icônes de légende affiche une explication.
- **Historique de batterie**: La page Historique ajoute les courbes de charge et la puissance de charge ou de décharge pour les comparer à l’utilisation des ressources.
- **Épingles et notes**: Les menus contextuels de Services, Démarrage et Connexions permettent d’épingler des entrées et d’ajouter des notes.
- **Animations de fenêtre**: Réduction, restauration et affichage ou masquage depuis la zone de notification utilisent un fondu sur place.
- **Notes de mise à jour**: Les paramètres et les dialogues partagent une présentation uniforme, acceptent les notes en anglais et adaptent la hauteur du dialogue à la fenêtre.
- **Journaux de diagnostic**: Ajout des étapes en échec, codes d’erreur, chemins pertinents et modèles de CPU et de GPU pour faciliter les signalements.
- **À propos**: Ajout d’un lien vers la page d’accueil du projet.

### Corrections

- **Statistiques CPU**: Correction des valeurs trop faibles pour l’utilisation globale, par processeur logique et par processus pendant les charges brèves.
- **Windows 10**: Correction du bureau visible à travers l’arrière-plan, des fenêtres de paramètres système vides et des données de lecture/écriture disque absentes par processus.
- **Remplacement du Gestionnaire des tâches**: Correction des échecs d’activation et des faux messages indiquant un démarrage ou un arrêt en cours dans certains environnements. Ajout d’indications sur les blocages par les logiciels de sécurité.
- **Capteurs du noyau**: Correction de la détection et de la mise à jour des anciennes versions de PawnIO, ainsi que de l’état activé affiché malgré un dysfonctionnement du pilote.
- **Applications au démarrage**: Affichage, activation et désactivation des entrées d’applications empaquetées complétés ; noms et états corrigés.
- **Lancement du programme**: Correction de l’échec d’ouverture de la fenêtre d’Argus après autorisation par un autre compte administrateur ou lorsque l’Explorateur est arrêté.
- **Identification des processus**: Correction de l’application propriétaire de WebView2, de la fusion erronée de programmes homonymes et des groupes incorrects lorsque le tri est suspendu.
- **Saisie chinoise**: Correction de la perte du premier caractère saisi après un retour à la page Processus.
- **Courbes**: Correction du rognage progressif à gauche, des saccades du point au survol et des sauts de courbes après une pause. Amélioration de la transition lors d’un changement de limite supérieure.
- **Disposition des pages**: Correction des sauts de mise en page des pages et dialogues à l’apparition d’une barre de défilement.
- **Sauvegarde de l’historique**: Correction de la perte des données déjà enregistrées lorsque la lecture du fichier du jour échoue.
- **Renforcement de la sécurité**: Mise à jour des composants de base et renforcement des restrictions de chargement de contenu externe dans les pages intégrées.

### Performances

- Amélioration du défilement de la liste des processus et réduction des actualisations répétées en arrière-plan et de leur consommation de ressources.

### Problèmes connus

- **Signature de code**: Le certificat officiel est en cours de demande. Windows SmartScreen peut signaler un « éditeur inconnu » au premier lancement.

Merci à @kostyanp95 pour sa contribution à l’historique de batterie.

## [0.2.0](https://github.com/Ray-lea/Argus/compare/v0.1.0...v0.2.0) - 2026-10-18

Cette version ajoute les débits de lecture et d’écriture disque par processus et des intervalles distincts pour la fenêtre réduite et masquée dans la zone de notification. Elle améliore les mises à jour intégrées et les raccourcis, et corrige notamment des plantages après un certain temps sur certains PC. Avec le service privilégié installé, la section « Changements incompatibles » est à consulter en premier.

### Changements incompatibles

- **Mise à jour du service privilégié requise**: La communication entre Argus et le service privilégié change. Le service de 0.1.0 est incompatible avec la nouvelle version. Les installations sans ce service ne sont pas concernées.
- **Mise à jour par l’installateur**: Cela comprend les mises à jour intégrées et l’exécution directe de l’installateur. Il ferme d’abord les anciennes copies d’Argus encore actives et lancées par le remplacement du Gestionnaire des tâches, puis actualise le service avec la même autorisation administrateur, sans autre opération. Si « Remplacer le Gestionnaire des tâches » est activé, il le reste ; après une mise à jour réussie, Ctrl+Shift+Esc ouvre la nouvelle version.
- **Mise à jour de la version portable**: Si le service privilégié est installé, il doit être actualisé manuellement après remplacement de l’exécutable principal. Il suffit de choisir « Mettre le service à jour » dans l’invite de la nouvelle version d’Argus ou dans « Paramètres → Autorisations », puis d’autoriser. La même procédure s’applique en cas d’échec de l’installateur à actualiser le service. Avant cette mise à jour, les données réseau, disque et capteurs CPU dépendant du service sont indisponibles, et les opérations nécessitant une élévation peuvent échouer. Avec « Remplacer le Gestionnaire des tâches » activé, Ctrl+Shift+Esc peut encore ouvrir l’ancienne version ; la mise à jour du service actualise aussi le programme de remplacement tout en maintenant la fonction active.

### Nouvelles fonctionnalités

- **Lectures et écritures disque par processus**: Processus ajoute « Lecture disque » et « Écriture disque », deux colonnes triables indiquant les volumes lus et écrits par seconde. La vue par application affiche le total du groupe ; les valeurs figurent aussi dans le volet de détails. Les colonnes I/O comprennent également les accès réseau et périphériques et sont comptabilisées séparément. Windows 11 24H2 et ultérieur ne demandent aucun droit supplémentaire ; les systèmes plus anciens nécessitent le service privilégié ou les droits administrateur, ce qui figure désormais dans l’explication d’autorisation du service. Sans mesure disponible, « — » est affiché et le survol donne le motif. Les deux colonnes restent visibles dans une fenêtre étroite, avec défilement horizontal et colonne Nom fixée à gauche ; les colonnes threads, handles, PID et I/O sont donc masquées plus tôt qu’auparavant.
- **Mises à jour intégrées**: À partir de cette version, une fois le téléchargement et la vérification de signature terminés, un bouton apparaît à droite de la barre de titre, un point sur l’icône de notification et un avis unique par version en bas à droite. La version portable ne télécharge pas automatiquement et signale la nouvelle version dès sa détection. Le dialogue présente par catégorie les changements de toutes les versions entre l’actuelle et la nouvelle, avec leur nombre par version. Les notes sont vérifiées par signature comme l’installateur. Le menu contextuel du bouton propose « Ne plus rappeler cette version ». Un téléchargement terminé reste disponible après redémarrage d’Argus.
- **Affichage des changements après mise à jour**: La première ouverture de la fenêtre après une mise à jour affiche les changements de la nouvelle version. « Afficher les nouveautés après la mise à jour » peut être désactivé dans « Paramètres → Mises à jour » ; seul un avis de mise à jour terminée apparaît alors en bas à droite. « Voir les changements de cette version », sur la même page, les ouvre à tout moment.
- **Intervalle en arrière-plan**: Les intervalles à l’état réduit et dans la zone de notification se règlent séparément de 0,5 à 30 s, avec 5 s par défaut, dans « Paramètres → Données et historique » et le guide de démarrage. Sous 5 s, une explication préalable signale la hausse de charge CPU et de consommation d’énergie ; au-delà de 5 s, l’historique devient moins détaillé. Le retour au premier plan déclenche une actualisation immédiate. Si l’intervalle de fond dépasse celui du premier plan, un avis unique en bas à droite permet d’accéder aux paramètres ou de ne plus être averti.
- **Réglage des raccourcis**: Un raccourci peut être désactivé tout en conservant sa combinaison, puis rétabli à sa réactivation. L’enregistrement affiche les touches maintenues en temps réel, explique les combinaisons indisponibles et propose « Restaurer les valeurs par défaut ». Les combinaisons comme Ctrl+lettre et Alt+lettre, susceptibles de conflit, nécessitent une confirmation avant enregistrement. Une combinaison occupée par un autre programme peut être réessayée directement ; une fois ce programme fermé, Argus réessaie aussi automatiquement lors du prochain affichage de la fenêtre.
- **Durée de fonctionnement du CPU**: Affichage à la seconde, par exemple « 5 jours 14:03:07 », mis à jour à chaque actualisation.

### Corrections

- **Plantages**: Correction d’un possible plantage d’Argus après un certain temps sur des PC sans GPU Intel disponible.
- **Perte de paramètres**: Correction de la perte de paramètres après une coupure de courant ou un redémarrage forcé peu après leur sauvegarde, qui faisait réapparaître le guide initial et rétablissait le choix de langue selon le système.
- **Lancement automatique**: Le lancement ne vole plus le focus de la fenêtre active. Auparavant, les frappes étaient envoyées à la fenêtre invisible d’Argus jusqu’à un clic ailleurs. Masquée dans la zone de notification, l’interface cesse de se redessiner en continu, réduisant fortement les charges CPU et GPU et de plus de moitié la mémoire utilisée.
- **Entrée de démarrage automatique**: Activer « Lancer à l’ouverture de session Windows » dans Argus ouvert par Ctrl+Shift+Esc fait pointer l’entrée vers la version installée. Les entrées visant déjà la copie de remplacement sont corrigées automatiquement après une première ouverture de la nouvelle version depuis le menu Démarrer ou un accès équivalent.
- **Actualisation en arrière-plan**: L’intervalle réel à l’état réduit ou masqué correspond au réglage. Un intervalle de 5 s durait auparavant souvent 6 s, parfois 10 s. Avec des droits standard et le service privilégié, les colonnes réseau de Processus ne disparaissent plus brièvement après restauration depuis la zone de notification ou l’état réduit.
- **Fenêtre**: Suppression des deux groupes de boutons superposés en haut à droite à l’état agrandi. La fenêtre peut être déplacée avec le pointeur contre le bord supérieur de l’écran.
- **Installateur et désinstalleur**: « Fermer » sur la page finale, « Annuler » sur la confirmation de désinstallation et la ligne de la case à cocher sont centrés ; ils étaient auparavant légèrement décalés vers la gauche.
- **Mise à jour par installation**: Une exécution directe de l’installateur ferme aussi les anciennes copies actives d’Argus lancées par Ctrl+Shift+Esc ou d’anciennes entrées de démarrage. Cela évite le transfert des lancements suivants vers l’ancienne version et dispense d’une fermeture préalable depuis la zone de notification.
- **Désinstallation**: L’Argus ouvert par Ctrl+Shift+Esc est également fermé. Auparavant, il restait en arrière-plan : même après « Supprimer aussi les paramètres, l’historique et les journaux » puis une réinstallation, le guide initial n’apparaissait pas et les anciens paramètres étaient réécrits. Si des données restent, le désinstalleur énumère les dossiers non supprimés.
- **Guide de démarrage**: Maintenir Ctrl+Shift+Esc à l’ouverture d’Argus ne laisse plus la touche Esc fermer directement le guide et le marquer comme terminé.
- **Enregistrement des raccourcis**: Appuyer sur la combinaison actuelle ou maintenir une touche plus longtemps ne masque plus accidentellement la fenêtre pendant l’enregistrement. Sur les dispositions de clavier non américaines, la touche enregistrée correspond à celle qui déclenche réellement l’action. Shift+Backspace et Win+Backspace n’effacent plus la combinaison par erreur.
- **Combinaisons de raccourcis**: Les nouvelles combinaisons doivent contenir Ctrl, Alt ou Win, sauf certaines touches comme F13–F24. Les combinaisons perturbant la saisie, comme Shift+lettre, et les commandes habituelles comme Ctrl+C ou Alt+F4 sont refusées. Les combinaisons déjà enregistrées restent inchangées.
- **Raccourcis**: Certaines touches du pavé numérique ne sont plus enregistrées comme d’autres touches. Un échec de sauvegarde lors d’une modification ne crée plus de raccourci supplémentaire impossible à annuler. Les motifs d’échec d’enregistrement suivent la langue de l’interface. Dans le guide initial, une combinaison occupée ou inutilisable affiche uniquement la raison et la possibilité de la modifier dans les paramètres.
- **Remplacer le Gestionnaire des tâches**: Le remplacement reste actif lors de la mise à jour du service, et sa copie d’Argus est actualisée. Le rétablissement préalable du Gestionnaire des tâches Windows puis la réactivation manuelle sont supprimés. En cas d’échec d’activation, l’étape refusée et la cause possible sont indiquées, par exemple une modification du registre bloquée par un logiciel de sécurité. « Configuré par un autre programme » énumère les configurations concernées. Après désactivation, un outil comme Process Explorer prenant en charge le Gestionnaire des tâches ne bloque plus la mise à jour ou la désinstallation du service. Un échec d’activation ne laisse plus de copie superflue d’Argus dans le dossier du service ; les anciennes copies sont nettoyées lors d’une mise à niveau, de l’activation ou de la désactivation du remplacement, ou de la mise à jour ou suppression du service, sauf si elles sont en cours d’exécution.
- **Processus**: Un clic sur « Afficher les détails » sans sélection demande désormais de choisir une entrée, alors qu’il ne produisait auparavant aucun effet. Lorsqu’un menu contextuel ou une confirmation est ouvert, la barre d’état n’affiche plus « Tri interrompu alors que le pointeur est sur la liste » et la liste ne se retrie pas à l’apparition de la confirmation. Un changement de page par raccourci avec le pointeur sur la liste ne laisse plus ce message sur les autres pages.
- **Graphiques**: Le point de lecture ne saccade plus lorsque le pointeur reste près du bord droit du graphique.
- **Changement de canal**: Passer entre Stable et Bêta pendant une vérification ou un téléchargement relance la vérification sur le nouveau canal, au lieu de proposer la version du précédent.
- **Journal de diagnostic**: Les échecs indiquent le code d’erreur système plutôt que le générique operation failed, utilisé auparavant dans presque tous les cas sur les systèmes non anglais. Au lancement, le motif d’échec de connexion au service est également enregistré, notamment une version incompatible ou un délai de réponse expiré.

### Performances

- **Page Processus**: Le dessin des petites barres d’utilisation par processeur logique dans la vue d’ensemble coûte moins de ressources. Pendant le défilement des courbes, l’interface envoie environ un tiers de données en moins au GPU par image. Les lectures et écritures WebView2 dans I/O provenant surtout de ces données, elles diminuent également.

### Limitations connues

- **Lectures et écritures disque**: La méthode de Windows 11 24H2 et ultérieur n’a été vérifiée que sur le build 26300. Les systèmes antérieurs passent par le service privilégié ou les droits administrateur et n’ont pas encore été vérifiés. Si les deux colonnes affichent toujours « — », un signalement peut inclure le journal de diagnostic, qui contient le motif de l’échec de lecture.
- **Signature de code**: Le programme n’est pas encore signé ; Windows SmartScreen peut signaler un « éditeur inconnu » au premier lancement.
- **Autres**: Les autres limitations figurent dans le document [Problèmes connus](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md) sur GitHub.

## 0.1.0 - 2026-10-16

Première version publique d’Argus.

### Nouvelles fonctionnalités

- **Processus**: Affichage par application, arborescence ou liste ; le tri par nom sépare « Applications » et « Processus en arrière-plan ». CPU, mémoire, I/O et GPU sont affichés par processus, ainsi que le trafic réseau avec le service privilégié ou les droits administrateur. Recherche et arrêt d’un processus ou de toute son arborescence sont disponibles. Le volet de détails indique ligne de commande, utilisateur, niveau d’intégrité et autres informations ; le regroupement par application présente son total.
- **Priorité et mode d’efficacité**: Réglage depuis le menu contextuel ou par des règles appliquées automatiquement à chaque lancement du programme.
- **Performances**: CPU (par cœur, cœurs de performance et d’efficacité, fréquence, température, puissance et états d’inactivité), mémoire, disques, réseau, GPU (avec puissance en watts pour NVIDIA, AMD et Intel) et batterie. Les courbes défilent à la fréquence de l’écran et s’adaptent à la fenêtre ; les informations moins courantes sont regroupées dans des cartes dépliables.
- **Capteurs du noyau facultatifs**: Après activation dans les paramètres ou le guide initial, les capteurs internes du CPU sont lus par le service privilégié et le pilote officiel PawnIO : température et tension par cœur, puissance et états de limitation, données AMD SMU, ainsi que vitesse des ventilateurs, tensions et températures des cartes mères compatibles.
- **Services et applications au démarrage**: Démarrage et arrêt des services, modification de leur type de démarrage ; activation ou désactivation des applications au démarrage avec le même interrupteur que le Gestionnaire des tâches système.
- **Connexions et ports**: Points de terminaison TCP et UDP avec leurs processus, recherche exacte par port et fermeture des connexions TCP IPv4.
- **Fichiers utilisés**: Déposer un fichier ou dossier sur la fenêtre recherche les processus qui l’utilisent, en combinant Restart Manager et l’énumération des handles, y compris les répertoires courants des processus.
- **Historique**: Enregistrement des 7 derniers jours d’utilisation des ressources ; un clic sur un instant affiche les processus les plus consommateurs à ce moment-là.
- **Zone de notification**: Le raccourci global Ctrl+Alt+Esc affiche ou masque la fenêtre, avec une très faible consommation lorsqu’elle est masquée.
- **Service privilégié facultatif**: Après une autorisation administrateur unique, l’interface avec droits standard peut gérer les processus système, appliquer des règles automatiquement, mesurer le trafic réseau par processus, contrôler les services et fermer des connexions.
- **Remplacement du Gestionnaire des tâches**: Ctrl+Shift+Esc et les autres points d’entrée peuvent ouvrir Argus. Un programme de restauration autonome fonctionne sans l’interface ni le service.
- **Mises à jour intégrées**: Canaux Stable et Bêta, avec proxy GitHub facultatif. Seules les versions dont la signature est validée sont installées.
- **Apparence**: La barre d’outils fait aussi office de barre de titre, avec navigation en dessous ou à gauche. Arrière-plan Mica et couleur d’accentuation système ; les thèmes clair et sombre suivent Windows par défaut et peuvent être choisis dans les paramètres.
- **Installation et désinstallation**: Installateur sur une seule page, suivant le mode clair ou sombre de Windows. Une ancienne version installée est mise à niveau sur place ; la désinstallation permet de supprimer également paramètres, historique et journaux.
- **Langues et guide de démarrage**: Interface, zone de notification et installateur en chinois simplifié et en anglais, suivant par défaut la langue d’affichage de Windows. Le premier lancement guide les réglages de langue, d’affichage et d’actualisation, de présence dans la zone de notification, de mises à jour et autres options courantes. Il permet aussi d’installer directement le service privilégié facultatif, d’activer les capteurs du noyau ou de remplacer le Gestionnaire des tâches.

### Limitations connues

- Le programme n’est pas encore signé ; Windows SmartScreen peut signaler un « éditeur inconnu » au premier lancement.
- Les capteurs internes du CPU nécessitent l’activation des capteurs du noyau (service privilégié et pilote PawnIO). Les processeurs et cartes mères compatibles figurent dans les [données matérielles](https://github.com/Ray-lea/Argus/blob/main/docs/guide/hardware-data.md#消费级硬件兼容范围).
- Les autres limitations figurent dans les [problèmes connus](https://github.com/Ray-lea/Argus/blob/main/docs/known-issues.md).
