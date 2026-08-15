# Calib Laban — étalonnage multi-caméras

Application d'étalonnage d'un dispositif de plusieurs caméras, destinée à la
reconstruction 3D du geste dansé dans le projet de recherche **Laban-Notation**.

Elle calcule les **intrinsèques** (focale, point principal, distorsion) et les
**extrinsèques** (position et orientation relatives) de chaque caméra, avec les
écarts-types de chaque paramètre, puis exporte le dispositif sous une forme
directement utilisable pour trianguler les articulations du danseur en 3D.

```
┌ poses / caméras ┬ image + détection ┬ vue 3D du dispositif ┐
│                 ├───────────────────┼──────────────────────┤
│                 │ journal           │ onglets d'analyse    │
└─────────────────┴───────────────────┴──────────────────────┘
```

## Installation

```bash
cd laban-calib
python -m pip install -e ".[gui]"     # cœur + interface graphique
python -m pip install -e ".[dev]"     # + pytest
```

Dépendances : NumPy, OpenCV (`opencv-contrib-python`, pour ArUco/ChArUco),
SciPy ; PySide6 et Matplotlib pour l'interface.

## Démarrage

```bash
python -m labancalib          # interface graphique
python -m labancalib.cli -h   # version en ligne de commande
```

## Chaîne de travail

1. **Configurer la mire** — saisir la géométrie *réelle* de la mire imprimée.
   Le côté de case fixe l'échelle métrique de tout le dispositif : une erreur
   de 1 mm sur une case de 60 mm, ce sont 1,7 % d'erreur sur toutes les
   distances 3D. L'application génère la mire en PDF à l'échelle
   (`Exporter en PDF à l'échelle…`, à imprimer **sans** mise à l'échelle).

   | Mire | Quand l'utiliser |
   |------|------------------|
   | **ChArUco** | Recommandée. Détection robuste même si la mire est partiellement hors champ ou occultée — le cas courant avec des caméras grand-angle en studio. Détections partielles pleinement exploitées. |
   | **Cercles asymétriques** | Centres très précis sous éclairage diffus, mais la mire doit être entièrement visible. |
   | **Cercles symétriques**, **échiquier** | Fournis pour compatibilité avec des jeux d'images existants. |

2. **Alimenter le projet** — trois sources, au choix ou combinées :
   - `Définir les images…` : un dossier par caméra ;
   - `Importer des vidéos…` : extraction de trames (pas, nombre maximum,
     filtre de netteté par variance du laplacien) ;
   - `Capture en direct…` : prévisualisation des flux avec la détection
     superposée, et capture de poses synchronisées.

   > **La pose *i* est la *i*-ème image de chaque caméra.** Les prises doivent
   > être synchronisées ; l'application signale les caméras dont le nombre
   > d'images diffère.

3. **Détecter la mire** (F5) — chaque image est analysée ; les vues sans mire
   sont simplement ignorées.

4. **Optimiser les caméras** (F6) — trois étapes, tracées dans le journal :
   1. intrinsèques de chaque caméra séparément ;
   2. initialisation du dispositif : poses relatives estimées par paires de
      caméras voyant la mire au même instant, puis chaînées le long d'un arbre
      couvrant maximal depuis la caméra de référence ;
   3. ajustement de faisceaux : intrinsèques libres, poses des caméras et pose
      de la mire à chaque instant sont raffinées ensemble en minimisant
      l'erreur de reprojection.

5. **Exporter** — JSON du dispositif, `FileStorage` OpenCV, ou rapport texte.

### Ce qu'il faut capturer

- Au moins **20 poses par caméra**, mire inclinée dans plusieurs directions
  (l'inclinaison est ce qui sépare la focale de la distance).
- Couvrir **tout le champ**, bords et coins compris : l'onglet *Couverture*
  montre où les points manquent.
- Pour lier deux caméras, il faut des poses où la mire est vue
  **simultanément** par les deux. L'onglet *Initialisation* compte, pour
  chaque pose, le nombre de caméras qui voient la mire.

## Lecture des résultats

- **RPE** (erreur de reprojection, en pixels) : ordre de grandeur attendu
  0,1–0,5 px. Un RPE très bas avec peu de poses signale un sur-ajustement
  plutôt qu'une bonne calibration.
- **± sur chaque paramètre** : écart-type issu de la jacobienne à l'optimum
  (`cov = s²(JᵀJ)⁻¹`). Un σ énorme sur `cx`/`cy` ou sur `k3` indique un
  paramètre mal contraint par les données — le fixer (fenêtre *Modèle et
  optimisation*) donne un modèle plus stable.
- **Nuage RPE** : les résidus doivent former une tache isotrope centrée sur
  zéro. Une structure (croissant, spirale) trahit un modèle de distorsion
  inadapté — passer au modèle fisheye pour un objectif très grand-angle.
- **Vue 3D** : contrôle de bon sens sur le placement des caméras et des poses.

## Utilisation en ligne de commande

```bash
# mire imprimable à l'échelle
python -m labancalib.cli board --kind charuco --cols 8 --rows 6 \
    --square 60 --marker 45 --out mire.pdf

# extraction de trames depuis une captation
python -m labancalib.cli frames captation_cam0.mp4 --out images/cam0 \
    --stride 20 --max-frames 60 --min-sharpness 40

# étalonnage complet
python -m labancalib.cli calibrate images/cam0 images/cam1 images/cam2 \
    --kind charuco --cols 8 --rows 6 --square 60 --marker 45 \
    --model fisheye --out dispositif.json --project etude.lcalib
```

## Réutilisation : triangulation des articulations

### Import direct MediaPipe

MediaPipe Pose fournit, par caméra et par trame, 33 points BlazePose en
coordonnées **image normalisées** [0, 1] avec un score de visibilité.
`labancalib.mediapipe_io` les convertit en pixels via la taille d'image de
chaque caméra étalonnée, écarte les points peu visibles, aligne les séquences
et les triangule.

```python
from labancalib import Rig, triangulate_mediapipe

rig = Rig.from_json("dispositif.json")

# un fichier par caméra, dans l'ordre du dispositif
reconstruction = triangulate_mediapipe(
    rig,
    ["cam0.json", "cam1.json", "cam2.json"],
    min_visibility=0.5,   # seuil sur la visibilité MediaPipe
    max_error=8.0,        # rejet au-delà de 8 px de reprojection (0 = désactivé)
    offsets=[0, 0, 2],    # caméra 2 démarrée 2 trames plus tard
)

reconstruction.points      # (n_trames, 33, 3) en mètres, nan si non reconstruit
reconstruction.errors      # (n_trames, 33) erreur de reprojection, en pixels
reconstruction.seen_by     # (n_trames, 33) nombre de caméras exploitables
reconstruction.completeness()      # part des points reconstruits
reconstruction.segment_lengths()   # longueurs médianes des segments, en mètres
reconstruction.save("points3d.npz")   # ou .csv
```

Sources acceptées, indifféremment : chemins de fichiers (`.json`, `.jsonl`,
`.npy`, `.npz`), tableaux NumPy, ou listes de résultats MediaPipe en mémoire —
API Tasks (`PoseLandmarker`) comme ancienne API `solutions.pose`. Le multi-
personnes est géré par `person=`.

> **Ce sont les points *image* qui sont triangulés, pas les
> `pose_world_landmarks`.** Ces derniers sont métriques mais recentrés sur le
> bassin et estimés d'une seule vue : ils ne portent aucune géométrie du
> dispositif. C'est la triangulation sur les caméras étalonnées qui donne des
> positions métriques dans le repère du studio.

**Contrôle de sanité** : `segment_lengths()` donne la longueur médiane de
chaque segment du squelette. Les os ne s'allongent pas — une dispersion
importante d'une trame à l'autre signale un étalonnage ou une association
inter-caméras défaillante, avant même de passer à l'analyse Laban.

En ligne de commande :

```bash
# points MediaPipe d'une captation (nécessite le modèle .task, cf. plus bas)
python -m labancalib.cli pose captation_cam0.mp4 --model pose_landmarker.task --out cam0.json

# reconstruction 3D sur le dispositif étalonné
python -m labancalib.cli triangulate dispositif.json cam0.json cam1.json cam2.json \
    --min-visibility 0.5 --max-error 8 --out points3d.csv
```

Dans l'interface : menu **Analyse → Trianguler des points MediaPipe…** (F7).

Le sous-programme `pose` a besoin du modèle MediaPipe, à télécharger une fois :

```bash
curl -L -o pose_landmarker.task \
  https://storage.googleapis.com/mediapipe-models/pose_landmarker/pose_landmarker_lite/float16/1/pose_landmarker_lite.task
```

(ou définir `MEDIAPIPE_POSE_MODEL`). Si vous produisez déjà les points avec
votre propre script MediaPipe, cette étape est inutile : passez directement
vos fichiers à `triangulate`.

### API générique

```python
# tracks : (n_caméras, n_trames, n_articulations, 2) en pixels, nan si absent
points_3d, erreurs = rig.triangulate_tracks(tracks)   # (n_trames, n_articulations, 3) en mètres
```

`triangulate_point` accepte aussi un simple dictionnaire
`{indice de caméra: (x, y)}` et renvoie le point 3D **et** son erreur de
reprojection, qui sert de mesure de confiance en aval de l'analyse Laban.

## Conventions

- Longueurs en **mètres**, angles en **radians**, pixels en coordonnées image
  OpenCV (origine au coin supérieur gauche, y vers le bas).
- Une pose `(rvec, tvec)` transforme le **repère de référence vers la caméra** :
  `X_caméra = R(rvec) · X_référence + tvec`.
- Le repère de référence est celui de la caméra de référence (par défaut la
  caméra 0), fixée à l'identité ; l'échelle métrique vient de la mire.
- Matrice intrinsèque `K = [[f, α·f, cx], [0, ar·f, cy], [0, 0, 1]]`, avec `ar`
  le rapport `fy/fx` et `α` l'obliquité normalisée (fixée à 0 pour le modèle
  fisheye, dont la projection OpenCV ne l'utilise pas).

## Structure du code

| Module | Rôle |
|--------|------|
| `labancalib/geometry.py` | Poses rigides, projection vectorisée des deux modèles, triangulation DLT |
| `labancalib/board.py` | Mires : géométrie, détection, rendu imprimable, calques |
| `labancalib/models.py` | Intrinsèques, poses, masque de paramètres, résultats |
| `labancalib/intrinsics.py` | Étalonnage d'une caméra isolée, PnP |
| `labancalib/bundle.py` | Initialisation du dispositif et ajustement de faisceaux |
| `labancalib/pipeline.py` | Enchaînement détection → étalonnage, journal et progression |
| `labancalib/project.py` | Projet `.lcalib` : caméras, images, détections, résultat |
| `labancalib/sources.py` | Dossiers d'images, extraction vidéo, capture en direct |
| `labancalib/export.py` | Export JSON / OpenCV / rapport texte |
| `labancalib/triangulate.py` | Reconstruction 3D à partir d'un dispositif étalonné |
| `labancalib/mediapipe_io.py` | Import des points MediaPipe Pose et reconstruction 3D |
| `labancalib/cli.py` | Interface en ligne de commande |
| `labancalib/gui/` | Interface PySide6 (fenêtre, calques, graphiques, vue 3D, tâches de fond) |

## Tests

```bash
python -m pytest tests -q
```

71 tests : accord de la projection vectorisée avec OpenCV, aller-retour de
détection sur les quatre types de mires, précision de l'étalonnage contre une
vérité terrain synthétique (sténopé et fisheye), rejet des aberrants,
diagnostic des caméras sans vue commune, persistance du projet, export, import
MediaPipe (sur les vrais objets de l'API Tasks, sur l'ancienne API, et sur les
formats de fichiers) avec reconstruction 3D vérifiée contre un squelette de
référence, et un essai de bout en bout sur des images réellement rendues
(détection → étalonnage → export → triangulation).

Les tests MediaPipe sont ignorés automatiquement si `mediapipe` n'est pas
installé.
