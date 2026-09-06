# Die Projektseite

Statisches HTML, CSS und zwei Bilder. Kein Build-Schritt, keine
Abhängigkeiten, kein `node_modules`. Der Ordner lässt sich so ausliefern, wie
er hier liegt.

## Lokal ansehen

```bash
python3 -m http.server 4173 --directory site
```

Dann `http://localhost:4173` öffnen. Ein Doppelklick auf `index.html` tut es
auch — relative Pfade funktionieren über `file://`.

## Veröffentlichen

**GitHub Pages** — erledigt [`.github/workflows/pages.yml`](../.github/workflows/pages.yml)
bei jedem Push auf `main`, der `site/` berührt. Dafür muss unter
_Settings → Pages → Build and deployment_ die Quelle auf **GitHub Actions**
stehen. Die Alternative „Deploy from a branch" kann nur `/` oder `/docs`
bedienen und würde diesen Ordner nicht finden.

**Vercel, Netlify, Cloudflare Pages** — im Projekt als Root-Verzeichnis
`site` eintragen, Build-Command leer lassen, Output-Verzeichnis `.` (bei
Cloudflare: `/`). Es gibt nichts zu bauen.

## Aufbau

| Datei | Inhalt |
| --- | --- |
| `index.html` | Die gesamte Seite |
| `styles.css` | Alle Stile, Farben oben als Custom Properties |
| `assets/favicon.svg` | Die Wellenmarke, auch als Favicon |
| `assets/inventory-fixture.png` | Screenshot der App, auch vom Haupt-README verlinkt |

Die Farben stammen aus der App (`src/styles.css`). Wer eine ändert, sollte
beide Stellen anfassen.
