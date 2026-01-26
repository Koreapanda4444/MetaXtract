import sys
import platform
import importlib.util
import shutil

REQUIRED_BINARIES = [
    ("ffprobe", "영상 메타데이터 분석에 필요"),
    ("ffmpeg", "영상/오디오 처리에 권장"),
    ("exiftool", "이미지/문서 메타데이터 추출에 권장"),
]
REQUIRED_PACKAGES = [
    ("Pillow", "이미지 처리에 필요"),
    ("python-docx", "docx 문서 추출에 필요"),
    ("PyPDF2", "PDF 추출에 권장"),
    ("pypdf", "PDF 추출에 권장"),
]


def check_binaries():
    results = []
    for name, desc in REQUIRED_BINARIES:
        path = shutil.which(name)
        results.append({
            "name": name,
            "desc": desc,
            "found": bool(path),
            "path": path or "(없음)"
        })
    return results


def check_python_deps():
    results = []
    for pkg, desc in REQUIRED_PACKAGES:
        found = importlib.util.find_spec(pkg) is not None
        results.append({
            "name": pkg,
            "desc": desc,
            "found": found
        })
    return results


def check_env():
    return {
        "os": platform.platform(),
        "python_version": sys.version,
        "executable": sys.executable
    }


def run_doctor():
    env = check_env()
    bins = check_binaries()
    pkgs = check_python_deps()
    warnings = []
    if not any(b["name"] == "ffprobe" and b["found"] for b in bins):
        warnings.append("[경고] ffprobe가 없으므로 영상 메타데이터 분석이 제한됩니다.")
    if not any(p["name"] == "Pillow" and p["found"] for p in pkgs):
        warnings.append("[경고] Pillow가 없으므로 이미지 추출이 제한됩니다.")
    return {
        "env": env,
        "binaries": bins,
        "python_packages": pkgs,
        "warnings": warnings
    }


def print_doctor():
    result = run_doctor()
    print("[환경 정보]")
    print(f"OS: {result['env']['os']}")
    print(f"Python: {result['env']['python_version']}")
    print(f"실행 파일: {result['env']['executable']}")
    print("\n[외부 바이너리]")
    for b in result["binaries"]:
        status = "O" if b["found"] else "X"
        print(f"{status} {b['name']}: {b['desc']} ({b['path']})")
    print("\n[파이썬 패키지]")
    for p in result["python_packages"]:
        status = "O" if p["found"] else "X"
        print(f"{status} {p['name']}: {p['desc']}")
    if result["warnings"]:
        print("\n[경고/권장사항]")
        for w in result["warnings"]:
            print(w)
