# Cahier des charges fonctionnel — Application de transfert de fichiers de type WinSCP

> **Document de référence** — Description fonctionnelle d’une application de transfert de fichiers inspirée de WinSCP.
> Ce document peut être copié directement dans un fichier Markdown (`.md`). Il décrit les fonctionnalités attendues sans prétendre reproduire les détails internes ou l’interface exacte de chaque version de WinSCP.

## 1. Objet du document

Ce cahier des charges décrit le fonctionnement attendu d’une application de transfert et de gestion de fichiers entre un ordinateur local et un serveur distant.

L’application permet notamment de se connecter à des serveurs distants, de parcourir leurs répertoires, de transférer des fichiers, de gérer des sessions et d’automatiser certaines opérations.

## 2. Présentation générale

L’application est un logiciel multiplateforme de transfert et de gestion de fichiers entre un ordinateur local et un serveur distant. Elle doit pouvoir être déployée sur macOS, Linux et Windows.

Le logiciel doit être développé en Rust. Les interfaces et bibliothèques utilisées doivent être compatibles avec cette exigence et permettre la création de versions destinées aux trois systèmes d’exploitation.

L’application doit fournir une interface graphique permettant à un utilisateur de :

- se connecter à un serveur distant ;
- consulter les fichiers et répertoires locaux et distants ;
- transférer des fichiers dans les deux sens ;
- créer, renommer, déplacer ou supprimer des fichiers et répertoires, selon les droits disponibles ;
- enregistrer et réutiliser des paramètres de connexion ;
- suivre les opérations en cours et consulter leurs résultats.

Les commandes exécutées sur le serveur dépendent du protocole utilisé et des autorisations accordées au compte connecté.

## 3. Objectifs

L’application doit :

1. simplifier les transferts de fichiers entre un ordinateur et un serveur ;
2. prendre en charge des protocoles de transfert sécurisés ;
3. permettre de gérer plusieurs connexions et profils de connexion ;
4. réduire les erreurs grâce à des confirmations, des indicateurs d’état et une gestion explicite des conflits ;
5. proposer des fonctions de productivité telles que la synchronisation, la reprise des transferts et l’automatisation.

## 4. Périmètre fonctionnel

### 4.1 Inclus

- Connexion à des serveurs distants.
- Navigation dans les espaces de fichiers local et distant.
- Transfert de fichiers et de répertoires.
- Gestion des fichiers distants, dans la limite des droits accordés.
- Enregistrement et organisation des profils de connexion.
- Gestion des transferts, des erreurs et des conflits.
- Synchronisation de répertoires.
- Journalisation des opérations.
- Fonctions d’automatisation et interface en ligne de commande, si elles sont activées.

### 4.2 Hors périmètre

Sauf décision contraire du projet, l’application ne doit pas :

- administrer les comptes du serveur ;
- modifier les règles réseau ou les pare-feu ;
- contourner les permissions du serveur ;
- garantir la disponibilité ou la sauvegarde des données distantes ;
- remplacer un service de stockage, de sauvegarde ou de gestion de versions.

## 5. Utilisateurs et rôles

### Utilisateur standard

Peut créer ou ouvrir des connexions, naviguer dans les fichiers et effectuer les actions autorisées par ses identifiants.

### Utilisateur avancé

Peut configurer des paramètres plus détaillés, utiliser des mécanismes d’authentification avancés, automatiser des opérations et consulter des journaux plus complets.

### Administrateur de l’application

Peut définir les paramètres généraux de déploiement, les stratégies de sécurité, les options disponibles et, le cas échéant, les paramètres partagés entre utilisateurs.

> Les rôles de l’application ne remplacent jamais les permissions définies sur le serveur distant.

## 6. Parcours utilisateur principal

1. L’utilisateur ouvre l’application.
2. Il choisit un profil enregistré ou crée une nouvelle connexion.
3. Il renseigne le serveur, le protocole, le port et les informations d’authentification.
4. Il lance la connexion.
5. L’application établit la session ou présente une erreur explicite.
6. L’utilisateur parcourt les fichiers locaux et distants.
7. Il sélectionne un ou plusieurs éléments et lance une opération.
8. L’application affiche la progression, les résultats et les éventuels conflits.
9. L’utilisateur peut répéter l’opération, l’annuler ou fermer la session.

## 7. Exigences fonctionnelles

### 7.1 Connexion à un serveur

L’application doit permettre de configurer au minimum :

- un nom de profil ;
- un nom d’hôte ou une adresse IP ;
- un protocole ;
- un port, renseigné par défaut selon le protocole lorsqu’un port standard existe ;
- un nom d’utilisateur ;
- un mode d’authentification ;
- un répertoire distant initial, facultatif ;
- des options de connexion supplémentaires, si le protocole le permet.

Elle doit :

- tester la connexion à la demande de l’utilisateur ;
- indiquer clairement les étapes ou l’échec de la connexion ;
- distinguer une erreur réseau, une erreur d’authentification et un refus de permission lorsque cela est possible ;
- avertir l’utilisateur en cas de changement ou d’incertitude concernant l’identité du serveur ;
- permettre la déconnexion explicite.

### 7.2 Protocoles

L’application peut prendre en charge plusieurs protocoles de transfert, notamment :

- **SFTP**, au-dessus de SSH ;
- **SCP**, au-dessus de SSH ;
- **FTP**, sans chiffrement du canal FTP ;
- **FTPS**, FTP protégé par TLS ;
- **WebDAV** et **WebDAV sécurisé (HTTPS)**, selon les besoins du projet.

La disponibilité d’un protocole dépend de l’implémentation et de la configuration. L’interface doit expliquer les exigences de sécurité associées et éviter de présenter FTP comme équivalent à SFTP ou FTPS.

### 7.3 Authentification

Selon le protocole, l’application peut prendre en charge :

- un mot de passe ;
- une clé privée SSH ;
- une phrase secrète associée à une clé ;
- une clé ou un certificat client ;
- des mécanismes d’authentification intégrés au système ou à l’environnement.

Exigences :

- ne jamais afficher un mot de passe en clair dans les champs de saisie ;
- demander confirmation avant d’enregistrer des secrets ;
- proposer un stockage sécurisé lorsque celui-ci est disponible ;
- ne pas inclure de secrets dans les journaux, scripts exportés ou messages d’erreur ;
- expliquer à l’utilisateur les risques liés à l’enregistrement des identifiants.

### 7.4 Gestion des profils de connexion

L’application doit permettre de :

- créer, modifier, dupliquer et supprimer un profil ;
- regrouper ou organiser les profils ;
- ajouter une description ou des étiquettes ;
- rechercher un profil ;
- ouvrir une connexion à partir d’un profil ;
- importer ou exporter des paramètres non sensibles, si cette fonction est prévue.

Par défaut, les secrets doivent être exclus des exports. Toute exception doit être clairement signalée et protégée.

### 7.5 Navigation dans les fichiers

L’application doit présenter les fichiers locaux et distants de façon lisible. Une présentation en deux panneaux est une option privilégiée : un panneau local et un panneau distant.

Pour chaque élément, l’interface peut afficher :

- le nom ;
- le type ;
- la taille ;
- la date de modification ;
- les permissions, si disponibles ;
- le propriétaire ou le groupe, si disponibles.

L’utilisateur doit pouvoir :

- parcourir les répertoires ;
- revenir au répertoire précédent ou au répertoire parent ;
- saisir ou sélectionner un chemin ;
- actualiser l’affichage ;
- trier et filtrer les éléments ;
- sélectionner un ou plusieurs fichiers ;
- afficher les propriétés accessibles.

Les différences entre systèmes de fichiers, notamment les règles de casse, les caractères autorisés et les formats de dates, doivent être prises en compte.

### 7.6 Transfert de fichiers

L’application doit permettre de transférer des fichiers et des répertoires :

- de l’ordinateur local vers le serveur ;
- du serveur vers l’ordinateur local ;
- par glisser-déposer si l’interface le permet ;
- au moyen de commandes accessibles depuis les panneaux de fichiers.

Avant ou pendant un transfert, l’application doit permettre de définir les options pertinentes, telles que :

- le répertoire de destination ;
- le traitement des fichiers déjà présents ;
- le transfert des sous-répertoires ;
- la conservation des dates ou des attributs, si le protocole le permet ;
- le mode de transfert, si le protocole distingue différents modes.

Les fichiers volumineux ou multiples doivent pouvoir être transférés sans bloquer l’interface.

### 7.7 File d’attente et suivi des opérations

L’application doit afficher les transferts en cours et permettre, selon les capacités du protocole, de :

- consulter la progression ;
- connaître le fichier traité et le nombre d’éléments restants ;
- suspendre, reprendre ou annuler une opération ;
- réessayer un transfert échoué ;
- consulter le résultat final.

Les états possibles doivent être identifiables, par exemple :

- en attente ;
- en cours ;
- terminé ;
- annulé ;
- en échec ;
- terminé avec avertissements.

Une interruption du réseau ne doit pas être présentée comme un transfert réussi.

### 7.8 Conflits et fichiers existants

Lorsque le fichier de destination existe déjà, l’application doit proposer une règle explicite, telle que :

- remplacer le fichier ;
- ignorer l’élément ;
- conserver les deux fichiers en modifiant le nom ;
- appliquer le choix à tous les conflits similaires, si l’utilisateur le demande.

Avant une suppression ou une opération susceptible d’écraser des données, l’application doit présenter une confirmation adaptée à l’impact de l’action.

### 7.9 Opérations sur les fichiers

Dans la limite des droits accordés par le serveur, l’application peut permettre de :

- créer un répertoire ;
- renommer un fichier ou un répertoire ;
- supprimer un fichier ou un répertoire ;
- déplacer ou copier un élément, lorsque le protocole le permet ;
- consulter ou modifier les permissions, si elles sont disponibles ;
- éditer un fichier distant au moyen d’un éditeur local.

L’édition distante doit gérer le téléchargement temporaire, l’ouverture locale et la proposition de renvoi des modifications. L’utilisateur doit être averti si le fichier distant a changé entre-temps ou si le renvoi risque de l’écraser.

### 7.10 Synchronisation de répertoires

L’application peut comparer un répertoire local et un répertoire distant afin d’identifier les différences et de proposer des transferts.

Les critères de comparaison peuvent inclure :

- le nom ;
- la taille ;
- la date de modification ;
- le contenu, si une vérification par empreinte est disponible.

La synchronisation doit distinguer clairement :

- l’envoi vers le serveur ;
- la récupération depuis le serveur ;
- la synchronisation dans les deux sens.

Avant l’exécution, l’application doit présenter un aperçu des actions prévues. Toute suppression automatique doit être désactivée par défaut ou faire l’objet d’une confirmation explicite. Les règles de synchronisation doivent pouvoir préciser les exclusions et les comportements en cas de conflit.

### 7.11 Actions automatisées et ligne de commande

Si ces fonctions sont incluses, l’application peut permettre :

- d’exécuter des transferts par script ;
- de lancer des tâches depuis un ordonnanceur externe ;
- d’utiliser des commandes pour ouvrir une session et effectuer des opérations ;
- d’obtenir un code de sortie ou un rapport exploitable par un autre outil.

Les scripts doivent éviter d’exposer des secrets. Les opérations destructives doivent être documentées et contrôlables. Le comportement en cas d’erreur doit être prévisible et défini.

### 7.12 Journaux et historique

L’application doit présenter un journal des opérations comprenant, au minimum :

- la date et l’heure ;
- l’action effectuée ;
- l’état final ;
- une description utile de l’erreur, le cas échéant.

L’historique ne doit pas contenir de mots de passe, de phrases secrètes ou de clés privées. L’utilisateur doit pouvoir consulter les détails techniques nécessaires au diagnostic et, si prévu, effacer ou exporter les journaux.

## 8. Exigences non fonctionnelles

### 8.1 Sécurité

- Utiliser un protocole chiffré lorsque celui-ci est disponible et approprié.
- Vérifier et présenter l’identité du serveur selon les mécanismes du protocole.
- Ne pas désactiver silencieusement la validation des certificats ou des clés.
- Stocker les secrets de manière protégée, si leur enregistrement est proposé.
- Limiter les informations sensibles présentes dans les journaux.
- Confirmer les actions à risque et éviter les valeurs par défaut destructives.

### 8.2 Fiabilité

- Gérer les interruptions réseau sans bloquer toute l’application.
- Signaler les transferts incomplets.
- Préserver l’intégrité des fichiers autant que le permettent le protocole et l’environnement.
- Permettre une reprise lorsque le protocole et le serveur l’autorisent.

### 8.3 Performances

- La navigation et l’interface doivent rester réactives pendant les transferts.
- Les transferts de plusieurs fichiers doivent être gérés par une file d’attente.
- Les performances doivent être évaluées selon la taille des fichiers, le débit réseau et les limites du serveur.

### 8.4 Compatibilité et accessibilité

L’application doit être conçue et testée pour fonctionner sur les systèmes suivants :

- macOS ;
- Linux ;
- Windows.

Les versions minimales prises en charge pour chaque système doivent être définies avant la réalisation du projet. La distribution devra tenir compte des pratiques propres à chaque plateforme, notamment les formats d’installation, la signature des exécutables et les éventuelles dépendances système.

L’interface doit proposer des libellés compréhensibles, un contraste lisible et des commandes accessibles au clavier lorsque cela est applicable.


### 8.5 Technologies et maintenabilité

Le logiciel doit être développé en Rust. Le code doit être organisé en modules clairement séparés, notamment pour :

- l’interface utilisateur ;
- la gestion des connexions et des protocoles ;
- les transferts de fichiers ;
- la configuration et le stockage des profils ;
- la journalisation et la gestion des erreurs.

Les dépendances doivent être documentées, maintenues et compatibles avec les plateformes ciblées. Le projet doit fournir des instructions de compilation et de test pour macOS, Linux et Windows

## 9. Écrans et composants principaux

1. **Écran des profils** : liste, recherche, création et modification des connexions.
2. **Éditeur de connexion** : paramètres réseau, protocole et authentification.
3. **Vue de transfert** : navigation locale et distante.
4. **File d’attente** : progression et gestion des opérations.
5. **Boîtes de dialogue de conflit** : choix face aux fichiers déjà présents.
6. **Écran de synchronisation** : comparaison, aperçu et confirmation.
7. **Journal** : historique et détails des erreurs.
8. **Paramètres de l’application** : langue, interface, sécurité et préférences.

## 10. Règles de gestion

- Une connexion n’est utilisable que si le serveur accepte l’authentification.
- Les opérations disponibles dépendent des capacités du protocole et des droits du compte.
- Un transfert terminé doit être distingué d’un transfert interrompu ou partiellement échoué.
- Une opération de suppression ou de remplacement doit être identifiable avant son exécution.
- Les paramètres sensibles ne doivent pas être exportés par défaut.
- Une opération lancée par script doit produire un résultat vérifiable.

## 11. Principaux cas d’erreur

L’application doit gérer et expliquer, autant que possible :

- hôte introuvable ou inaccessible ;
- délai de connexion dépassé ;
- port fermé ou protocole incompatible ;
- échec d’authentification ;
- identité du serveur inconnue ou modifiée ;
- certificat invalide ou expiré ;
- permissions insuffisantes ;
- espace de stockage insuffisant ;
- nom de fichier non accepté ;
- fichier verrouillé ou déjà modifié ;
- interruption réseau ;
- échec partiel d’un transfert multiple.

Les messages doivent être compréhensibles par un utilisateur non spécialiste tout en permettant un diagnostic technique lorsque nécessaire.

## 12. Critères d’acceptation

Le produit pourra être considéré comme fonctionnel lorsque les tests démontreront que :

- un utilisateur peut créer un profil et se connecter à un serveur configuré ;
- une erreur de connexion est signalée sans révéler d’informations sensibles ;
- les espaces local et distant peuvent être parcourus ;
- un fichier peut être transféré dans les deux sens ;
- les conflits de fichiers sont présentés selon une règle explicite ;
- l’utilisateur peut annuler un transfert lorsque le protocole le permet ;
- les échecs sont enregistrés et distingués des succès ;
- les opérations de gestion sont limitées par les permissions du serveur ;
- les secrets ne figurent ni dans les exports ordinaires ni dans les journaux ;
- une synchronisation présente ses actions avant exécution et protège contre les suppressions accidentelles.

## 13. Éléments à préciser avant réalisation

Les choix suivants devront être confirmés par le projet :

- systèmes d’exploitation et plateformes ciblés ;
- protocoles obligatoires et facultatifs ;
- prise en charge du français et des autres langues ;
- méthode de stockage des profils et des secrets ;
- nombre de transferts simultanés ;
- limites de taille des fichiers et de profondeur des répertoires ;
- prise en charge de scripts ou d’une interface en ligne de commande ;
- politique de conservation des journaux ;
- exigences de conformité, d’audit ou de déploiement en entreprise ;
- niveau de compatibilité attendu avec les versions de serveurs utilisées.

## 14. Conclusion

L’application doit offrir un moyen sûr et compréhensible de se connecter à des serveurs, parcourir leurs fichiers et effectuer des transferts. Les priorités sont la sécurité des identifiants, la clarté des opérations, le traitement fiable des erreurs et la prévention des pertes de données. Les fonctionnalités exactes doivent être adaptées aux protocoles, aux serveurs et aux besoins du projet.