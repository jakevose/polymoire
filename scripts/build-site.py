#!/usr/bin/env python3
"""Builds the Polymoire website (GitHub Pages) from the repo's own data.

Everything on the page that can drift is generated, not hand-written:
- download links come from the latest GitHub release (asset names matched per platform);
- dependency versions come from Cargo.lock, tree sizes from `cargo tree` per desktop
  target, risk notes from site/dependencies.toml;
- the advisory summary comes from `cargo audit --json` when available;
- the test count comes from the source; the screenshots and their trait captions from
  site/screenshots (made by `mise run screenshots`).

Usage:
  python3 scripts/build-site.py [--out DIR] [--release-json FILE] [--audit-json FILE]
                                [--fetch-release] [--check]

--check builds into target/site and fails on anything stale or missing: a direct
dependency with no risk entry (or an entry for a dependency that's gone), a screenshot
named in shots.json that doesn't exist, or an unfilled template placeholder.
Standard library only (Python 3.11+), so it runs anywhere CI does.
"""

import argparse
import datetime as dt
import html
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
DEFAULT_REPO = "jakevose/polymoire"
DESKTOP_TARGETS = {
    "Linux": "x86_64-unknown-linux-gnu",
    "Windows": "x86_64-pc-windows-msvc",
    "macOS": "aarch64-apple-darwin",
}
# (os, label, asset name pattern, install note key)
PLATFORMS = [
    ("macOS", "Apple Silicon (M1 and later)", r"_aarch64\.dmg$", "mac"),
    ("macOS", "Intel", r"_x86_64\.dmg$", "mac"),
    ("Windows", "Windows 10/11, 64-bit", r"_x64-setup\.exe$", "win"),
    ("Linux", "x64 · .deb (Debian, Ubuntu)", r"_amd64\.deb$", "deb"),
    ("Linux", "x64 · AppImage (any distro)", r"_x86_64\.AppImage$", "appimage"),
    ("Linux", "arm64 · .deb", r"_arm64\.deb$", "deb"),
    ("Linux", "arm64 · AppImage", r"_aarch64\.AppImage$", "appimage"),
]
# Crates that would mean the app can talk to the network.
NETWORK_CRATES = {
    "reqwest", "ureq", "hyper", "h2", "curl", "isahc", "surf", "attohttpc",
    "tokio", "async-std", "native-tls", "rustls", "openssl", "websocket", "tungstenite",
}


class SiteError(Exception):
    pass


def esc(s):
    return html.escape(str(s), quote=True)


def run(cmd, **kw):
    try:
        r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, **kw)
    except FileNotFoundError:
        return None
    return r.stdout if r.returncode == 0 else None


# ---------------------------------------------------------------- data gathering


def repo_slug():
    slug = os.environ.get("GITHUB_REPOSITORY")
    if slug:
        return slug
    url = run(["git", "remote", "get-url", "origin"]) or ""
    m = re.search(r"github\.com[:/](.+?)(?:\.git)?\s*$", url)
    return m.group(1) if m else DEFAULT_REPO


def site_url(slug):
    """Absolute URL of the published site (link previews need absolute image URLs)."""
    owner, _, repo = slug.partition("/")
    return f"https://{owner.lower()}.github.io/{repo}/"


def direct_deps(manifest):
    """{name: scope} for every dependency declared in Cargo.toml."""
    deps = {}
    for name in manifest.get("dependencies", {}):
        deps[name] = "app"
    for spec, table in manifest.get("target", {}).items():
        for name in table.get("dependencies", {}):
            deps.setdefault(name, "linux" if "linux" in spec else "app")
    for name in manifest.get("dev-dependencies", {}):
        deps.setdefault(name, "dev")
    return deps


def lock_versions():
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    out = {}
    for p in lock.get("package", []):
        out.setdefault(p["name"], []).append(p["version"])
    return out


def cargo_tree(target, package=None):
    """Set of (name, version) in the shipped (normal) tree for one target, or None."""
    cmd = ["cargo", "tree", "-e", "normal", "--target", target, "--prefix", "none",
           "--format", "{p}"]
    if package:
        cmd += ["-p", package]
    out = run(cmd, env={**os.environ, "CARGO_TERM_COLOR": "never"})
    if out is None:
        return None
    crates = set()
    for line in out.splitlines():
        parts = line.replace(" (*)", "").split()
        if len(parts) >= 2 and parts[1].startswith("v"):
            crates.add((parts[0], parts[1][1:]))
    return crates


def load_release(path, fetch, slug):
    if path:
        return json.loads(pathlib.Path(path).read_text())
    if fetch:
        out = run(["gh", "release", "view", "-R", slug, "--json",
                   "tagName,name,publishedAt,url,assets"])
        if out:
            return json.loads(out)
    return None


def load_audit(path):
    if not path or not pathlib.Path(path).exists():
        return None
    text = pathlib.Path(path).read_text().strip()
    return json.loads(text) if text else None


def count_tests():
    n = 0
    for f in list((ROOT / "src").rglob("*.rs")) + list((ROOT / "tests").rglob("*.rs")):
        n += f.read_text().count("#[test]")
    return n


# ---------------------------------------------------------------- page sections


def downloads_html(release, slug, version):
    latest = f"https://github.com/{slug}/releases/latest"
    if release:
        tag = release["tagName"]
        when = release.get("publishedAt", "")[:10]
        assets = release.get("assets", [])
        head = (f'<p class="release-meta">Latest release: <a href="{esc(release["url"])}">'
                f"{esc(tag)}</a>" + (f" · {esc(when)}" if when else "") + "</p>")
    else:
        tag = f"v{version}"
        assets = []
        head = (f'<p class="release-meta">Release details weren\'t available when this page '
                f'was built: see <a href="{latest}">the latest release</a>.</p>')
    rows = []
    for os_name, label, pattern, note in PLATFORMS:
        match = next((a for a in assets if re.search(pattern, a["name"])), None)
        if match:
            url = match.get("url") or match.get("browser_download_url")
            size = match.get("size", 0) / 1_048_576
            link = (f'<a class="dl" href="{esc(url)}">{esc(match["name"])}</a>'
                    f'<span class="size">{size:.1f} MB</span>')
        else:
            link = f'<a class="dl" href="{latest}">Latest release</a>'
        rows.append(
            f'<tr data-os="{os_name}" data-note="{note}"><th scope="row">{esc(os_name)}</th>'
            f"<td>{esc(label)}</td><td>{link}</td></tr>"
        )
    return tag, head + (
        '<table class="downloads"><thead><tr><th scope="col">System</th>'
        '<th scope="col">Build</th><th scope="col">Download</th></tr></thead><tbody>'
        + "".join(rows) + "</tbody></table>"
    )


def gallery_html(shots, out_dir, check):
    src_dir = SITE / "screenshots"
    dst = out_dir / "screenshots"
    dst.mkdir(parents=True, exist_ok=True)
    figs = []
    for i, s in enumerate(shots):
        stem = pathlib.Path(s["file"]).stem
        found = next((src_dir / f"{stem}{ext}" for ext in (".webp", ".png", ".jpg")
                      if (src_dir / f"{stem}{ext}").exists()), None)
        if not found:
            raise SiteError(f"screenshot {stem} is in shots.json but not in site/screenshots "
                            "(run `mise run screenshots`)")
        shutil.copy2(found, dst / found.name)
        traits = "".join(
            f"<li><span>{esc(t['trait'])}</span> {esc(t['option'])}</li>" for t in s["traits"]
        )
        loading = "eager" if i == 0 else "lazy"
        figs.append(
            f'<figure class="shot{" hero-shot" if i == 0 else ""}">'
            f'<img src="screenshots/{esc(found.name)}" width="1600" height="1000" '
            f'loading="{loading}" alt="Polymoire showing {esc(s["title"])}: '
            f'{esc(", ".join(t["option"] for t in s["traits"][:5]))}">'
            f'<figcaption><strong>{esc(s["title"])}</strong>'
            f'<ul class="traits">{traits}</ul></figcaption></figure>'
        )
    return "\n".join(figs)


def deps_html(manifest, register, versions, trees, per_dep):
    declared = direct_deps(manifest)
    missing = sorted(set(declared) - set(register))
    stale = sorted(set(register) - set(declared))
    if missing:
        raise SiteError("no risk entry in site/dependencies.toml for: " + ", ".join(missing))
    if stale:
        raise SiteError("site/dependencies.toml has entries for removed dependencies: "
                        + ", ".join(stale))
    order = {"app": 0, "linux": 1, "dev": 2}
    scope_label = {"app": "All platforms", "linux": "Linux only", "dev": "Tests and tools"}
    rows = []
    for name in sorted(declared, key=lambda n: (order[register[n]["scope"]], n)):
        r = register[name]
        if r["risk"] not in ("low", "medium", "high"):
            raise SiteError(f"{name}: risk must be low, medium or high")
        ver = ", ".join(versions.get(name, ["?"]))
        size = per_dep.get(name)
        size_txt = f"{size} crates" if size else "—"
        rows.append(
            f'<tr><th scope="row"><a href="https://crates.io/crates/{esc(name)}">{esc(name)}</a>'
            f'<span class="ver">{esc(ver)}</span></th>'
            f'<td><span class="risk risk-{r["risk"]}">{r["risk"].title()}</span></td>'
            f"<td>{esc(scope_label[r['scope']])}</td><td>{size_txt}</td>"
            f'<td><p class="role">{esc(r["role"])}</p><p>{esc(" ".join(r["why"].split()))}</p>'
            "</td></tr>"
        )
    table = (
        '<table class="deps"><thead><tr><th scope="col">Crate</th><th scope="col">Risk</th>'
        '<th scope="col">Shipped</th><th scope="col">Pulls in</th><th scope="col">What it does '
        "and why that risk</th></tr></thead><tbody>" + "".join(rows) + "</tbody></table>"
    )

    if all(t is not None for t in trees.values()):
        union = set().union(*trees.values())
        counts = ", ".join(f"{os_name} {len(t)}" for os_name, t in trees.items())
        net = sorted({n for n, _ in union} & NETWORK_CRATES)
        net_txt = (
            "No networking library (HTTP, TLS, async network runtime) is in any desktop build."
            if not net else
            "Networking-capable crates in the desktop builds: " + ", ".join(net) + "."
        )
        listing = "".join(
            f"<tr><td>{esc(n)}</td><td>{esc(v)}</td>"
            + "".join(f"<td>{'●' if (n, v) in trees[o] else ''}</td>" for o in trees)
            + "</tr>"
            for n, v in sorted(union)
        )
        tree = (
            f"<p>Counting everything they pull in, a build ships <strong>{counts}</strong> "
            f"crates (Rust packages), {len(union)} distinct across all three. {net_txt}</p>"
            "<details><summary>Every crate in the desktop builds</summary>"
            '<div class="scroll"><table class="tree"><thead><tr><th scope="col">Crate</th>'
            '<th scope="col">Version</th>'
            + "".join(f'<th scope="col">{o}</th>' for o in trees)
            + f"</tr></thead><tbody>{listing}</tbody></table></div></details>"
        )
    else:
        tree = ("<p>Full dependency counts weren't computed for this build "
                "(<code>cargo</code> unavailable).</p>")
    return table + tree


def audit_html(audit, slug):
    if audit is None:
        return ("<p>The <a href=\"https://rustsec.org\">RustSec</a> advisory scan didn't run "
                "for this build of the page. The published site runs it on every update.</p>")
    vulns = audit.get("vulnerabilities", {}).get("list", [])
    warns = audit.get("warnings", {})
    items = []
    for v in vulns:
        a, p = v["advisory"], v["package"]
        items.append(
            f'<li><span class="risk risk-high">Vulnerability</span> {esc(p["name"])} '
            f'{esc(p["version"])}: <a href="https://rustsec.org/advisories/{esc(a["id"])}">'
            f'{esc(a["id"])}</a> {esc(a.get("title", ""))}</li>'
        )
    for kind, lst in sorted(warns.items()):
        for w in lst or []:
            a = w.get("advisory") or {}
            p = w.get("package", {})
            ident = a.get("id", "")
            link = (f'<a href="https://rustsec.org/advisories/{esc(ident)}">{esc(ident)}</a> '
                    if ident else "")
            items.append(
                f'<li><span class="risk risk-medium">{esc(kind.title())}</span> '
                f'{esc(p.get("name", "?"))} {esc(p.get("version", ""))}: {link}'
                f'{esc(a.get("title", ""))}</li>'
            )
    when = dt.date.today().isoformat()
    summary = (
        f"<p>Scanned against the <a href=\"https://rustsec.org\">RustSec</a> advisory "
        f"database on {when}: <strong>{len(vulns)} known "
        f"vulnerabilit{'y' if len(vulns) == 1 else 'ies'}</strong>"
        f" and {len(items) - len(vulns)} warning{'' if len(items) - len(vulns) == 1 else 's'} "
        "(unmaintained, unsound or yanked crates). The scan covers the whole lock file, "
        "including crates only used on platforms Polymoire doesn't ship for.</p>"
    )
    return summary + (f'<ul class="advisories">{"".join(items)}</ul>' if items else "")


# ---------------------------------------------------------------- main


def build(args):
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = manifest["package"]["version"]
    register = tomllib.loads((SITE / "dependencies.toml").read_text())
    shots = json.loads((SITE / "screenshots" / "shots.json").read_text())
    slug = repo_slug()
    out = pathlib.Path(args.out) if args.out else ROOT / "target" / "site"
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    trees = {os_name: cargo_tree(t) for os_name, t in DESKTOP_TARGETS.items()}
    per_dep = {}
    for name, scope in direct_deps(manifest).items():
        if scope == "dev":
            continue
        target = DESKTOP_TARGETS["Linux"] if scope == "linux" else DESKTOP_TARGETS["macOS"]
        t = cargo_tree(target, name)
        if t is not None:
            per_dep[name] = len(t) - 1

    release = load_release(args.release_json, args.fetch_release, slug)
    tag, downloads = downloads_html(release, slug, version)
    commit = (run(["git", "rev-parse", "--short", "HEAD"]) or "").strip()

    page = (SITE / "index.html").read_text()
    values = {
        "version": esc(tag),
        "repo": esc(slug),
        "repo_url": f"https://github.com/{esc(slug)}",
        "site_url": esc(site_url(slug)),
        "downloads": downloads,
        "gallery": gallery_html(shots, out, args.check),
        "deps": deps_html(manifest, register, lock_versions(), trees, per_dep),
        "audit": audit_html(load_audit(args.audit_json), slug),
        "tests": str(count_tests()),
        "built": dt.date.today().isoformat(),
        "commit": esc(commit or "unknown"),
    }
    for k, v in values.items():
        page = page.replace("{{" + k + "}}", v)
    left = re.findall(r"\{\{\s*[a-z_]+\s*\}\}", page)
    if left:
        raise SiteError("unfilled placeholders in site/index.html: " + ", ".join(left))
    (out / "index.html").write_text(page)
    shutil.copy2(SITE / "style.css", out / "style.css")
    if not (SITE / "og.jpg").exists():
        raise SiteError("site/og.jpg is missing (run `mise run screenshots`)")
    shutil.copy2(SITE / "og.jpg", out / "og.jpg")
    shutil.copy2(ROOT / "packaging" / "icons" / "64x64.png", out / "icon.png")
    shutil.copy2(ROOT / "packaging" / "icons" / "256x256.png", out / "icon-256.png")
    (out / ".nojekyll").write_text("")
    print(f"site built in {out} ({tag}, {len(shots)} screenshots, "
          f"release data: {'yes' if release else 'no'})")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out")
    ap.add_argument("--release-json")
    ap.add_argument("--audit-json")
    ap.add_argument("--fetch-release", action="store_true",
                    help="ask `gh` for the latest release when no JSON is given")
    ap.add_argument("--check", action="store_true",
                    help="build into target/site and fail on stale or missing data")
    args = ap.parse_args()
    try:
        build(args)
    except SiteError as e:
        print(f"site: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
