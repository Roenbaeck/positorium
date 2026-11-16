# Positorium website

A self-contained static website for **positorium.com**, including the real WASM
playground and the complete Query Studio / Terrain explorer. All fonts, scripts,
examples, and the engine are served from your own host. No CDN, API service,
database server, cookies, analytics, or Sites account is required.

## Upload the prepared website

For the existing Apache server, use the [Positorium virtual hosts and certificate
setup guide](apache/README.md).

1. Extract `dist/positorium-website.zip`.
2. Upload **its contents** to the document root for positorium.com (often
   `public_html` or `www`). `index.html` must be directly in that directory;
   upload all subdirectories, including `pkg`, `assets`, `examples`, and `traqula`.
3. Remove the existing domain forwarding / redirect in your hosting control
   panel and enable HTTPS. A server-side redirect may also be set in an existing
   `.htaccess`; remove that redirect while preserving unrelated hosting rules.
4. Open the homepage, run the playground, then open **Query Studio** and run its
   quick example. Both should work without a Positorium server.

Keep these MIME types: `.wasm` → `application/wasm`, `.js` → `text/javascript`,
`.css` → `text/css`. The bundle includes an optional `.htaccess` MIME mapping for
Apache-compatible hosting. Other hosts should set these in their own settings.
The site needs ordinary HTTPS static hosting; opening `index.html` as a `file://`
URL will not work for ES modules, workers, and WASM. No SPA rewrites, cross-origin
isolation headers, or special backend routes are needed.

If your host adds a Content Security Policy, allow same-origin scripts, styles,
fonts, fetches, and workers, plus `wasm-unsafe-eval` for WebAssembly compilation.
The existing full Studio also contains inline script and style code; allow those
there or configure hashes appropriate to the built file. The homepage itself
uses external modules and stylesheets.

Use normal revalidation for HTML, JavaScript, CSS, and WASM (for example,
`Cache-Control: no-cache`). These filenames are not content-hashed. Purge any
hosting/CDN cache after future uploads so the JS bindings and WASM stay matched.
Relative asset URLs also support previews below a path; canonical and social
metadata intentionally name `https://positorium.com/`.

## Rebuild

Requirements: Python 3.11+, Rust with `wasm32-unknown-unknown`, and `wasm-pack`
(tested with 0.14.0). From the repository root:

```sh
rustup target add wasm32-unknown-unknown
python3 scripts/build_website.py
```

Output:

- `dist/positorium-site/` — deployable files, with `index.html` at the root.
- `dist/positorium-website.zip` — the same files, ready to extract and upload.
- `dist/positorium-apache.zip` — Apache virtual hosts and certificate setup
  instructions; install these outside the website document root.

After changing only website assets, use `python3 scripts/build_website.py
--skip-wasm` to reuse `target/website-wasm/`. Rebuild WASM whenever the Rust code
or dependencies change. `--offline` uses Cargo's cached dependencies. The script
reads the release version from `Cargo.toml`, draws seed data from the tested
welcome fixture, and leaves the existing GitHub Pages testbed unchanged.

Preview over HTTP:

```sh
python3 -m http.server 4173 --directory dist/positorium-site
```

Then visit `http://localhost:4173`. The full Studio is at `/studio.html`.

## Playground behaviour

The four editable examples demonstrate the current evidence, an earlier assertion
cutoff, the full assertion history, and the typed execution plan. The data is
fictional. Every run uses a fresh in-memory database seeded from the welcome
fixture; edits and mutations are discarded on the next run. The homepage stores
no user data. The full Studio retains its existing browser-local editor settings.

WASM runs in a module Web Worker. **Stop** terminates the worker; a 15-second
timeout also terminates it, and the next run creates a new worker. Queries have a
1,000,000-unit work budget, a 32 MiB accounted workspace budget, and 100 rows per
result set. These engine budgets are not a total browser-memory cap; the worker
timeout is a separate safeguard. Displayed timing is measured query execution,
excluding initial engine download and seed setup. The normal browser cache can
reuse the engine, but this is not an offline/PWA installation.

## Design and assets

The evidence diagram is SVG/CSS, and its two time controls are illustrative. The
playground below it always renders actual engine output. Motion respects
`prefers-reduced-motion`. Space Grotesk and IBM Plex Mono are bundled under the
SIL Open Font License; their license files are in `assets/fonts/`.

The website sources are buildless HTML/CSS/ES modules in this directory. The
packager copies the existing Studio assets from the repository root, adds a
homepage link to its logo, and adjusts the packaged Studio metadata. Do not edit
`dist/` directly; rebuilding replaces its generated website files.

## Checks

The optional Node dependencies are for development checks only; none are shipped
with the site. After building the website, run from this directory:

```sh
npm ci
npx playwright install chromium
npm run test:all
```

The browser suite starts a local preview if one is not running. It checks all
four examples against the real WASM engine, result switching, safe text
rendering, empty results, fresh seed data, cancellation and retry, the full
Studio and Terrain, and layouts from 320 to 1440 pixels. The Node tests cover
worker timeouts, cancellation, late responses, and recovery from load failures.
To use an existing Chromium binary, set `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH`.

The CI WASM job also builds both deployment bundles and runs this test suite
against the freshly built website.

To run the same browser checks against the deployed website instead of starting
a local server:

```sh
WEBSITE_BASE_URL=https://positorium.com npm run test:browser
```

The suite's sample writes stay in its browser's in-memory WASM database. The
cancellation and download-failure tests intercept requests inside the test
browser; they do not change the public server.

`npm run social-image` regenerates `assets/social.png` from the homepage artwork
while the preview is running. Rebuild the website afterwards to include it in
the ZIP. The social image is checked in, so ordinary builds do not need a browser
or Node dependencies.
