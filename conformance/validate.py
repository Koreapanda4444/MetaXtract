from __future__ import annotations

import argparse
import concurrent.futures
import json
import re
import subprocess
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def load(path: str) -> dict:
    return tomllib.loads((ROOT / path).read_text(encoding="utf-8"))


def validate_repository() -> list[str]:
    errors = []
    architecture = load("architecture.toml")
    components = architecture["components"]
    names = {
        load(f"{component['path']}/Cargo.toml")["package"]["name"]: name
        for name, component in components.items()
        if (ROOT / component["path"] / "Cargo.toml").is_file()
    }
    for name, component in components.items():
        directory = ROOT / component["path"]
        if not directory.is_dir():
            errors.append(f"missing architecture component: {name}")
            continue
        manifest = directory / "Cargo.toml"
        if not manifest.is_file():
            continue
        package = tomllib.loads(manifest.read_text(encoding="utf-8"))
        dependencies = set(package.get("dependencies", {})) & names.keys()
        forbidden = {names[dependency] for dependency in dependencies} - set(component["allows"])
        if forbidden:
            errors.append(f"forbidden component dependency: {name} -> {sorted(forbidden)}")

    forbidden_cpp = re.compile(r"\b(?:system|popen|_popen|fopen|freopen|socket|connect|CreateProcess[AW]?|ShellExecute[AW]?)\s*\(|\b(?:ifstream|ofstream|fstream)\b")
    for path in (ROOT / components["extractor"]["path"]).rglob("*"):
        if path.suffix in {".cpp", ".hpp"} and forbidden_cpp.search(path.read_text(encoding="utf-8")):
            errors.append(f"native filesystem/process/network API: {path.relative_to(ROOT)}")
    for name in ("schema", "core", "native"):
        for path in (ROOT / components[name]["path"] / "src").rglob("*.rs"):
            if re.search(r"\bCommand\s*::\s*new|std::process::Command", path.read_text(encoding="utf-8")):
                errors.append(f"production runtime subprocess: {path.relative_to(ROOT)}")
    rules = architecture["rules"]
    if rules["product_owner"] != "rust" or rules["raw_extraction_owner"] != "cpp":
        errors.append("product or extraction ownership was changed")
    for rule in ("external_executables", "native_filesystem_writes", "native_network_access", "exceptions_across_ffi", "opaque_library_objects_across_ffi", "python_in_production"):
        if rules[rule]:
            errors.append(f"required architecture restriction disabled: {rule}")

    policy = load("dependency-policy.toml")
    dependencies = load("third-party.lock.toml")["dependency"]
    seen = set()
    for dependency in dependencies:
        name = dependency["name"]
        if name in seen:
            errors.append(f"duplicate dependency: {name}")
        seen.add(name)
        if not re.fullmatch(r"[0-9a-f]{40}", dependency["revision"]):
            errors.append(f"dependency is not commit-pinned: {name}")
        if not dependency["tag"].endswith(dependency["version"]):
            errors.append(f"release tag and version differ: {name}")
        licenses = set(re.findall(r"[A-Za-z0-9][A-Za-z0-9.+-]*", dependency["license"])) - {"OR", "AND"}
        if not licenses or licenses - set(policy["allowed_licenses"]) or licenses & set(policy["denied_licenses"]):
            errors.append(f"unreviewed dependency license: {name}")
        if not re.fullmatch(r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", dependency["source"]):
            errors.append(f"unexpected dependency source: {name}")
    workspace = load("Cargo.toml")
    pinned = {dependency["name"]: dependency["version"] for dependency in dependencies}
    for name in ("cxx", "cxx-build", "cmake"):
        if workspace["workspace"]["dependencies"][name] != "=" + pinned["cxx" if name == "cxx-build" else name]:
            errors.append(f"Cargo version differs from reviewed lock: {name}")
    cargo_lock = load("Cargo.lock")
    for package in cargo_lock["package"]:
        if package.get("source", "").startswith("registry+") and not re.fullmatch(r"[0-9a-f]{64}", package.get("checksum", "")):
            errors.append(f"Cargo dependency lacks content checksum: {package['name']}")
    for name in ("cxx", "cxx-build", "cmake"):
        matches = [package for package in cargo_lock["package"] if package["name"] == name]
        if len(matches) != 1 or matches[0]["version"] != pinned["cxx" if name == "cxx-build" else name]:
            errors.append(f"Cargo.lock version mismatch: {name}")

    baseline = json.loads((ROOT / "reference/python/baseline.json").read_text(encoding="utf-8"))
    result = subprocess.run(["git", "rev-parse", "HEAD:legacy/python/src"], cwd=ROOT, text=True, capture_output=True, check=True)
    if result.stdout.strip() != baseline["source_tree_git_sha1"]:
        errors.append("frozen Python source tree differs from the baseline")
    changes = subprocess.run(["git", "diff", "HEAD", "--", "legacy/python/src"], cwd=ROOT, text=True, capture_output=True, check=True)
    if changes.stdout:
        errors.append("frozen Python source has uncommitted changes")
    extras = subprocess.run(["git", "ls-files", "--others", "--exclude-standard", "legacy/python/src"], cwd=ROOT, text=True, capture_output=True, check=True)
    if extras.stdout:
        errors.append("frozen Python source has new files")
    return errors


def verify_source(dependency: dict) -> str | None:
    tag = "refs/tags/" + dependency["tag"]
    result = subprocess.run(["git", "ls-remote", "--tags", dependency["source"], tag, tag + "^{}"], text=True, capture_output=True, timeout=60)
    if result.returncode:
        return f"source verification failed: {dependency['name']}: {result.stderr.strip()}"
    refs = dict(line.split()[::-1] for line in result.stdout.splitlines())
    revision = refs.get(tag + "^{}", refs.get(tag))
    if revision != dependency["revision"]:
        return f"release commit differs from lock: {dependency['name']}: {revision}"
    return None


def validate_cargo_licenses() -> list[str]:
    result = subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT, text=True, capture_output=True, check=True)
    policy = load("dependency-policy.toml")
    errors = []
    for package in json.loads(result.stdout)["packages"]:
        if package["source"] is None:
            continue
        licenses = set(re.findall(r"[A-Za-z0-9][A-Za-z0-9.+-]*", package.get("license") or "")) - {"OR", "AND"}
        if not licenses or licenses - set(policy["allowed_licenses"]) or licenses & set(policy["denied_licenses"]):
            errors.append(f"unreviewed Cargo license: {package['name']} {package['version']}: {package['license']}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate MetaXtract migration ownership and dependency locks")
    parser.add_argument("--online", action="store_true", help="verify release tags against upstream commit IDs")
    parser.add_argument("--cargo", action="store_true", help="check transitive Cargo license declarations")
    args = parser.parse_args()
    errors = validate_repository()
    if args.cargo and not errors:
        errors.extend(validate_cargo_licenses())
    if args.online and not errors:
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
            errors.extend(error for error in pool.map(verify_source, load("third-party.lock.toml")["dependency"]) if error)
    if errors:
        for error in errors:
            print(f"- {error}")
        return 1
    print("Repository ownership, frozen Python source, and dependency locks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
