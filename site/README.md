# Wave Uninstaller — website

Static marketing and documentation site. No framework, no build step — the same
rule the product follows.

## Deploy to Vercel

The `site/` directory is the whole deployment. Nothing is compiled.

**From the dashboard**

1. Import the repository at [vercel.com/new](https://vercel.com/new).
2. Set **Root Directory** to `site`.
3. Framework preset: **Other**. Leave the build and output commands empty.
4. Deploy.

**From the CLI**

```bash
cd site
vercel --prod
```

`vercel.json` sets `cleanUrls` (so `/roadmap` serves `roadmap.html`) and the
security headers, including a CSP with no inline script or style. Keep it that
way: an inline `style=` attribute or `<script>` block is silently dropped by the
browser once the site is live, which is a miserable thing to debug.

## Deploy to GitHub Pages

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) publishes this
folder on every push to `main` that touches it. It needs
_Settings → Pages → Build and deployment_ set to **GitHub Actions** — the
alternative, "deploy from a branch", can only serve `/` or `/docs` and would
never find this folder.

Pages is a mirror. Vercel is the canonical host, which is what the `canonical`
links, the Open Graph URLs and `sitemap.xml` point at.

## The download links

`download.html` points at
`github.com/aquaxs1/Wave-uninstaller/releases/latest/download/<asset>`, which
always resolves to the newest published release. The asset names carry no
version for exactly that reason, so a new release needs no change here.
The binaries come from
[`.github/workflows/release.yml`](../.github/workflows/release.yml).

## The domain

`https://wave-uninstaller.vercel.app` is written into four places. Change all
four together if the site ever moves:

- the `<link rel="canonical">` and `og:url` in every `.html`
- `og:image` in every `.html`
- `robots.txt`
- `sitemap.xml`

## Pages

| File | Purpose |
|---|---|
| `index.html` | Landing page — what it does, why it is safe, where it stands |
| `download.html` | The Windows builds, plus checksums and how to build it yourself |
| `roadmap.html` | All six phases in detail, plus how the code is laid out |
| `terms.html` | Terms of Use |
| `404.html` | Not found |

## Assets

| File | Purpose |
|---|---|
| `assets/css/site.css` | The design system |
| `assets/js/site.js` | Theme, mobile nav, footer year |
| `assets/img/logo.svg` | The wordmark's mark, also the favicon |
| `assets/img/og-image.png` | Social preview, 1200×630 |
| `assets/img/inventory.png` | Screenshot of the app, also used by the root README |

Colours are monochrome by design, the same system the sibling projects use:
white, black, grey, with colour reserved for state. Green means done, amber
means be careful with this.

## Viewing it locally

```bash
python3 -m http.server 4173 --directory site
```

Then open `http://localhost:4173`. Opening `index.html` by double-click works
too — every path is relative.

## Editing

Plain HTML. The header and footer are repeated per page; keep them in sync when
you change navigation. Adding a page means adding it to:

1. The nav markup in each page header
2. The footer links
3. `sitemap.xml`
