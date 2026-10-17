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


def allowed_license(expression: str, policy: dict) -> bool:
    tokens = set(re.findall(r"[A-Za-z0-9][A-Za-z0-9.+-]*", expression))
    exceptions = set(re.findall(r"\bWITH\s+([A-Za-z0-9][A-Za-z0-9.+-]*)", expression))
    licenses = tokens - {"OR", "AND", "WITH"} - exceptions
    return bool(licenses) and not (
        licenses - set(policy["allowed_licenses"])
        or licenses & set(policy["denied_licenses"])
        or exceptions - set(policy.get("allowed_license_exceptions", []))
    )


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
    for name, component in components.items():
        if not component["language"].startswith("rust"):
            continue
        for path in (ROOT / components[name]["path"] / "src").rglob("*.rs"):
            text = path.read_text(encoding="utf-8")
            worker_client = path == ROOT / "crates/metaxtract-core/src/scan/worker.rs"
            if re.search(r"\bCommand\s*::\s*new|std::process::Command", text) and not (
                worker_client and "Command::new(&self.path)" in text
                and '"--protocol=1"' in text
            ):
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
        if not allowed_license(dependency["license"], policy):
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

    captured = json.loads((ROOT / "conformance/expected/python.json").read_text(encoding="utf-8"))
    baseline = captured["baseline"]
    for field in ("baseline_commit", "source_tree_git_sha1"):
        if not re.fullmatch(r"[0-9a-f]{40}", baseline[field]):
            errors.append("missing captured Python provenance: " + field)
    for directory in ("legacy", "reference"):
        if (ROOT / directory).exists():
            errors.append("retired application tree remains: " + directory)
    for component in components.values():
        if component["language"].startswith("rust") and any((ROOT / component["path"] / "src").rglob("*.py")):
            errors.append("Python product source remains: " + component["path"])
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
    result = subprocess.run(["cargo", "metadata", "--format-version", "1", "--locked"], cwd=ROOT, text=True, capture_output=True)
    if result.returncode:
        detail = (result.stderr or result.stdout).strip()[-6000:]
        return [f"Cargo dependency inspection failed (exit {result.returncode}): {detail}"]
    policy = load("dependency-policy.toml")
    errors = []
    for package in json.loads(result.stdout)["packages"]:
        if package["source"] is None:
            continue
        if not allowed_license(package.get("license") or "", policy):
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
    print("Repository ownership, captured reference provenance, and dependency locks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
