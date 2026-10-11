import importlib.util
import platform
import shutil
import sys


PYTHON_DEPENDENCIES = [
    {
        "name": "cryptography",
        "module": "cryptography",
        "required": True,
        "description": "case bundle signing and signature verification",
    },
    {
        "name": "Pillow",
        "module": "PIL",
        "required": True,
        "description": "image metadata extraction",
    },
    {
        "name": "python-docx",
        "module": "docx",
        "required": True,
        "description": "DOCX metadata extraction",
    },
    {
        "name": "pypdf",
        "module": "pypdf",
        "required": True,
        "description": "PDF metadata extraction",
    },
]

OPTIONAL_BINARIES = [
    {
        "name": "ffprobe",
        "description": "video metadata extraction",
    },
]


def check_binaries():
    results = []
    for dependency in OPTIONAL_BINARIES:
        path = shutil.which(dependency["name"])
        results.append(
            {
                **dependency,
                "required": False,
                "found": bool(path),
                "path": path,
            }
        )
    return results


def check_python_deps():
    results = []
    for dependency in PYTHON_DEPENDENCIES:
        try:
            found = importlib.util.find_spec(dependency["module"]) is not None
        except (ImportError, ModuleNotFoundError, ValueError):
            found = False
        results.append({**dependency, "found": found})
    return results


def check_env():
    return {
        "os": platform.platform(),
        "python_version": sys.version,
        "executable": sys.executable,
    }


def run_doctor():
    binaries = check_binaries()
    packages = check_python_deps()
    warnings = []
    for package in packages:
        if package["required"] and not package["found"]:
            warnings.append(
                f"Required dependency missing: {package['name']} "
                f"({package['description']})"
            )
    for binary in binaries:
        if not binary["found"]:
            warnings.append(
                f"Optional dependency missing: {binary['name']} "
                f"({binary['description']})"
            )
    return {
        "env": check_env(),
        "binaries": binaries,
        "python_packages": packages,
        "warnings": warnings,
        "ok": all(not item["required"] or item["found"] for item in packages),
    }


def print_doctor():
    result = run_doctor()
    print("Environment")
    print(f"OS: {result['env']['os']}")
    print(f"Python: {result['env']['python_version']}")
    print(f"Executable: {result['env']['executable']}")
    print("\nPython dependencies")
    for package in result["python_packages"]:
        status = "OK" if package["found"] else "MISSING"
        kind = "required" if package["required"] else "optional"
        print(f"{status} {package['name']} [{kind}]: {package['description']}")
    print("\nExternal tools")
    for binary in result["binaries"]:
        status = "OK" if binary["found"] else "MISSING"
        path = binary["path"] or "not found"
        print(f"{status} {binary['name']} [optional]: {binary['description']} ({path})")
    if result["warnings"]:
        print("\nWarnings")
        for warning in result["warnings"]:
            print(warning)
    return result
