#!/usr/bin/env python3
"""Build a self-contained, static Positorium website and an uploadable ZIP.

Uses Python's standard library and wasm-pack; no frontend build tool is needed.
"""

import argparse
from datetime import datetime, timezone
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
from zipfile import ZipFile, ZIP_DEFLATED


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "website"
WASM = ROOT / "target" / "website-wasm"
DIST = ROOT / "dist"


def assemble(destination: Path, version: str) -> None:
    for name in ("site.css", "site.js", "examples.js", "runner-client.js", "runner-worker.js"):
        shutil.copy2(SOURCE / name, destination / name)
    shutil.copytree(SOURCE / "assets", destination / "assets")
    html = (SOURCE / "index.html").read_text(encoding="utf-8")
    html = html.replace("{{VERSION}}", version).replace("{{YEAR}}", str(datetime.now(timezone.utc).year))
    (destination / "index.html").write_text(html, encoding="utf-8")

    (destination / "pkg").mkdir()
    for name in ("positorium.js", "positorium_bg.wasm"):
        if not (WASM / name).is_file():
            raise SystemExit(f"Missing {WASM / name}. Run this script without --skip-wasm first.")
        shutil.copy2(WASM / name, destination / "pkg" / name)

    # Share the seed with the repository's welcome fixture to keep the two
    # experiences consistent. Include no query results or simulated responses.
    welcome = (ROOT / "traqula" / "welcome.traqula").read_text(encoding="utf-8")
    marker = "/* RESULT 1"
    if marker not in welcome:
        raise SystemExit("The welcome fixture changed; update the website seed extraction.")
    (destination / "examples").mkdir()
    (destination / "examples" / "seed.traqula").write_text(welcome.split(marker, 1)[0], encoding="utf-8")

    # Preserve the complete Query Studio and Terrain explorer, including their
    # relative asset URLs. The GitHub Pages testbed remains independent.
    studio = (ROOT / "positorium.html").read_text(encoding="utf-8")
    studio = studio.replace('href="https://roenbaeck.github.io/positorium/"', 'href="https://positorium.com/studio.html"')
    studio = studio.replace('content="https://roenbaeck.github.io/positorium/"', 'content="https://positorium.com/studio.html"')
    studio = studio.replace('https://roenbaeck.github.io/positorium/og.png?v=0.9.0', 'https://positorium.com/assets/social.png')
    studio = studio.replace('<title>Positorium — A database for facts that disagree</title>', '<title>Query Studio — Positorium</title>')
    # Only the packaged copy receives a home link; the standalone testbed keeps
    # its existing behaviour at the GitHub Pages root.
    logo = '<span class="brand-mark" aria-hidden="true">P</span>'
    if logo not in studio:
        raise SystemExit("The Studio header changed; update its packaged homepage link.")
    studio = studio.replace(logo, '<a class="brand-mark" href="./" aria-label="Back to the Positorium website" title="Back to the website" style="text-decoration:none">P</a>')
    (destination / "studio.html").write_text(studio, encoding="utf-8")
    for name in ("positorium.css", "positorium-terrain.css", "positorium-terrain.js"):
        shutil.copy2(ROOT / name, destination / name)
    (destination / "traqula").mkdir()
    for name in ("welcome.traqula", "blackthorn.traqula", "terrain.traqula"):
        shutil.copy2(ROOT / "traqula" / name, destination / "traqula" / name)
    for name in ("LICENSE.MIT", "LICENSE.Apache-2.0"):
        shutil.copy2(ROOT / name, destination / name)

    # A PNG is used for broad support in social-link previews.
    social = SOURCE / "assets" / "social.png"
    if not social.is_file():
        shutil.copy2(ROOT / "og.png", destination / "assets" / "social.png")
    (destination / "robots.txt").write_text("User-agent: *\nAllow: /\nSitemap: https://positorium.com/sitemap.xml\n", encoding="utf-8")
    (destination / "sitemap.xml").write_text(
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        '  <url><loc>https://positorium.com/</loc></url>\n'
        '  <url><loc>https://positorium.com/studio.html</loc></url>\n'
        '</urlset>\n', encoding="utf-8",
    )
    # Apache-compatible hosting: the MIME mapping matters for WASM streaming.
    # Other hosts can ignore this file; no rewrite rules or server API required.
    (destination / ".htaccess").write_text(
        '<IfModule mod_mime.c>\n'
        '  AddType application/wasm .wasm\n'
        '  AddType text/javascript .js\n'
        '</IfModule>\n', encoding="utf-8",
    )
    (destination / ".nojekyll").touch()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-wasm", action="store_true", help="Reuse target/website-wasm after a successful build (for website-only edits).")
    parser.add_argument("--offline", action="store_true", help="Use Cargo's cached dependencies when building WASM.")
    args = parser.parse_args()
    if not args.skip_wasm:
        command = ["wasm-pack", "build", "--release", "--target", "web", "--out-dir", str(WASM), ".", "--no-default-features", "--features", "wasm"]
        if args.offline:
            command.append("--offline")
        subprocess.run(command, cwd=ROOT, check=True)

    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["package"]["version"]
    DIST.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".website-", dir=DIST) as staging:
        prepared = Path(staging) / "site"
        prepared.mkdir()
        assemble(prepared, version)
        archive = Path(staging) / "positorium-website.zip"
        with ZipFile(archive, "w", compression=ZIP_DEFLATED, compresslevel=9) as bundle:
            for path in sorted(prepared.rglob("*")):
                if path.is_file():
                    bundle.write(path, path.relative_to(prepared))
        apache_archive = Path(staging) / "positorium-apache.zip"
        with ZipFile(apache_archive, "w", compression=ZIP_DEFLATED, compresslevel=9) as bundle:
            for name in ("README.md", "http.www.positorium.com.bootstrap", "http.www.positorium.com.conf", "https.www.positorium.com.conf"):
                bundle.write(SOURCE / "apache" / name, name)
        output = DIST / "positorium-site"
        if output.is_symlink():
            raise SystemExit(f"Refusing to replace symlink {output}")
        if output.exists():
            shutil.rmtree(output)
        shutil.move(str(prepared), output)
        shutil.move(str(archive), DIST / archive.name)
        shutil.move(str(apache_archive), DIST / apache_archive.name)
    print(f"Built Positorium {version}")
    print(f"Website: {output}")
    print(f"Upload:  {DIST / 'positorium-website.zip'}")
    print(f"Apache:  {DIST / 'positorium-apache.zip'}")
    print("Preview: python3 -m http.server 4173 --directory dist/positorium-site")


if __name__ == "__main__":
    main()
